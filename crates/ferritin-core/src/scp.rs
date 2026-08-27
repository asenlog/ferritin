//! DICOM SCP (Service Class Provider) — the inbound adapter.
//!
//! Owns everything socket- and PDU-shaped: TCP accept loop,
//! association accept with called-AE / calling-AE / source-IP
//! authorization, PDV reassembly, and DIMSE dispatch.
//! Protocol decisions live in `dimse`, persistence in `intake`,
//! domain vocabulary in `domain`; none of them knows this module exists.

use crate::config::DICOMServerConfig;
use crate::dimse;
use crate::domain::auth::AuthorizedCaller;
use crate::ports::{CallerDirectory, MappingStore, ObjectStore};
use crate::service::intake::{IntakeError, IntakeService};
use anyhow::Context;
use dicom_object::InMemDicomObject;
use dicom_ul::association::server::{AccessControl, ServerAssociation, ServerAssociationOptions};
use dicom_ul::association::Association;
use dicom_ul::pdu::{
    AssociationRJServiceUserReason, PDataValue, PDataValueType, Pdu,
    PresentationContextResultReason, UserIdentity,
};
use std::collections::HashMap;
use std::net::{IpAddr, TcpListener, TcpStream};

/// dicom-ul access-control policy enforcing the caller directory.
/// Rejects at the association level (A-ASSOCIATE-RJ), before any
/// DIMSE traffic: a wrong called AE, an unknown calling AE, or a
/// known AE calling from outside its network all get turned away.
pub struct NodeAccessControl<'a> {
    peer_addr: IpAddr,
    callers: &'a [AuthorizedCaller],
}

impl<'a> NodeAccessControl<'a> {
    pub fn new(peer_addr: IpAddr, callers: &'a [AuthorizedCaller]) -> Self {
        Self { peer_addr, callers }
    }
}

/// AE titles arrive space-padded to 16 chars; compare trimmed.
fn ae_eq(a: &str, b: &str) -> bool {
    a.trim_end() == b.trim_end()
}

impl AccessControl for NodeAccessControl<'_> {
    fn check_access(
        &self,
        this_ae_title: &str,
        calling_ae_title: &str,
        called_ae_title: &str,
        _user_identity: Option<&UserIdentity>,
    ) -> Result<(), AssociationRJServiceUserReason> {
        if !ae_eq(called_ae_title, this_ae_title) {
            return Err(AssociationRJServiceUserReason::CalledAETitleNotRecognized);
        }

        let authorized = self.callers.iter().any(|caller| {
            ae_eq(&caller.ae_title, calling_ae_title) && caller.network.contains(&self.peer_addr)
        });
        if !authorized {
            return Err(AssociationRJServiceUserReason::CallingAETitleNotRecognized);
        }

        Ok(())
    }
}

pub struct Server<S: ObjectStore, M: MappingStore, C: CallerDirectory> {
    config: DICOMServerConfig,
    intake: IntakeService<S, M>,
    callers: C,
}

impl<S: ObjectStore, M: MappingStore, C: CallerDirectory> Server<S, M, C> {
    pub fn new(config: DICOMServerConfig, store: S, mappings: M, callers: C) -> Self {
        Self {
            config,
            intake: IntakeService::new(store, mappings),
            callers,
        }
    }

    pub fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind((self.config.host.as_str(), self.config.port))?;
        self.serve(listener)
    }

    /// Serve on an already-bound listener — split from `run` so tests
    /// can bind an ephemeral port and learn the address up front.
    pub fn serve(&self, listener: TcpListener) -> anyhow::Result<()> {
        tracing::info!("DICOM Server listening on {}", listener.local_addr()?);

        for stream in listener.incoming() {
            let socket = match stream {
                Ok(socket) => socket,
                Err(e) => {
                    tracing::warn!("failed to accept incoming connection {e}");
                    continue;
                }
            };

            if let Err(e) = self.handle_association(socket) {
                tracing::warn!("association ended with error: {e:#}");
            }
        }
        Ok(())
    }

    fn handle_association(&self, socket: TcpStream) -> anyhow::Result<()> {
        let peer_addr = socket.peer_addr()?;
        // read the caller list fresh per association so directory
        // changes (e.g. from the frontend) apply without a restart;
        // a directory failure fails the association (fail closed)
        let callers = self
            .callers
            .authorized_callers()
            .context("failed to load authorized callers")?;
        let mut scp_options = ServerAssociationOptions::new()
            .ae_access_control(NodeAccessControl::new(peer_addr.ip(), &callers))
            .ae_title(self.config.ae_title.as_str());

        for ts in dimse::TRANSFER_SYNTAXES {
            scp_options = scp_options.with_transfer_syntax(*ts);
        }
        scp_options = scp_options.with_abstract_syntax(dimse::VERIFICATION_SOP_CLASS);
        for sop_class in dimse::STORAGE_SOP_CLASSES {
            scp_options = scp_options.with_abstract_syntax(*sop_class);
        }

        let mut association = scp_options.establish(socket)?;
        tracing::info!(
            "association from '{}': {}/{} presentation contexts accepted",
            association.peer_ae_title(),
            association
                .presentation_contexts()
                .iter()
                .filter(|pc| pc.reason == PresentationContextResultReason::Acceptance)
                .count(),
            association.presentation_contexts().len(),
        );

        // Presentation context id → negotiated transfer syntax, needed
        // to decode the datasets arriving on each context.
        let contexts: HashMap<u8, String> = association
            .presentation_contexts()
            .iter()
            .filter(|pc| pc.reason == PresentationContextResultReason::Acceptance)
            .map(|pc| (pc.id, pc.transfer_syntax.clone()))
            .collect();

        let mut command_buf: Vec<u8> = Vec::new();
        let mut dataset_buf: Vec<u8> = Vec::new();
        let mut pending_store: Option<PendingStore> = None;

        loop {
            match association.receive()? {
                Pdu::PData { data } => {
                    for pdv in data {
                        match pdv.value_type {
                            PDataValueType::Command => {
                                command_buf.extend_from_slice(&pdv.data);
                                if !pdv.is_last {
                                    continue;
                                }
                                let command = dimse::parse_command(&command_buf)?;
                                command_buf.clear();

                                match dimse::command_field(&command)? {
                                    dimse::command::C_ECHO_RQ => {
                                        let message_id = dimse::message_id(&command)?;
                                        let rsp = dimse::echo_response(message_id);
                                        send_command(
                                            &mut association,
                                            pdv.presentation_context_id,
                                            &rsp,
                                        )?;
                                        tracing::info!("answered C-ECHO (message id {message_id})");
                                    }
                                    dimse::command::C_STORE_RQ => {
                                        pending_store = Some(PendingStore {
                                            message_id: dimse::message_id(&command)?,
                                            sop_class_uid: dimse::affected_sop_class_uid(&command)?,
                                            sop_instance_uid: dimse::affected_sop_instance_uid(
                                                &command,
                                            )?,
                                            context_id: pdv.presentation_context_id,
                                        });
                                    }
                                    other => {
                                        tracing::warn!("unsupported command field: 0x{other:04x}")
                                    }
                                }
                            }

                            PDataValueType::Data => {
                                dataset_buf.extend_from_slice(&pdv.data);
                                if !pdv.is_last {
                                    continue;
                                }
                                let pending = pending_store
                                    .take()
                                    .context("received a dataset with no pending C-STORE-RQ")?;
                                let dataset = std::mem::take(&mut dataset_buf);

                                let ts_uid = contexts
                                    .get(&pending.context_id)
                                    .context("dataset on an unaccepted presentation context")?;

                                let status = match self.intake.store_instance(ts_uid, &dataset) {
                                    Ok(key) => {
                                        tracing::info!(
                                            "stored {} ({})",
                                            pending.sop_instance_uid,
                                            key
                                        );
                                        dimse::status::SUCCESS
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "C-STORE of {} failed: {e}",
                                            pending.sop_instance_uid
                                        );
                                        status_for(&e)
                                    }
                                };

                                let rsp = dimse::store_response(
                                    pending.message_id,
                                    &pending.sop_class_uid,
                                    &pending.sop_instance_uid,
                                    status,
                                );
                                send_command(&mut association, pending.context_id, &rsp)?;
                            }
                        }
                    }
                }

                Pdu::ReleaseRQ => {
                    association.send(&Pdu::ReleaseRP)?; // polite goodbye handshake
                    break;
                }
                Pdu::AbortRQ { .. } => break, // impolite goodbye
                other => tracing::warn!("unexpected PDU: {}", other.short_description()),
            }
        }
        Ok(())
    }
}

/// A C-STORE-RQ awaiting its dataset.
struct PendingStore {
    message_id: u16,
    sop_class_uid: String,
    sop_instance_uid: String,
    context_id: u8,
}

fn send_command(
    association: &mut ServerAssociation<TcpStream>,
    context_id: u8,
    command: &InMemDicomObject,
) -> anyhow::Result<()> {
    association.send(&Pdu::PData {
        data: vec![PDataValue {
            presentation_context_id: context_id,
            value_type: PDataValueType::Command,
            is_last: true,
            data: dimse::command_to_bytes(command)?,
        }],
    })?;
    Ok(())
}

/// Map intake failures onto DIMSE statuses (PS3.7 Annex C).
fn status_for(error: &IntakeError) -> u16 {
    match error {
        IntakeError::UnsupportedTransferSyntax(_) | IntakeError::MalformedDataset(_) => {
            dimse::status::CANNOT_UNDERSTAND
        }
        IntakeError::Mapping(_) | IntakeError::Store(_) => dimse::status::REFUSED_OUT_OF_RESOURCES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn callers() -> Vec<AuthorizedCaller> {
        vec![
            "CT_SCANNER@10.0.0.5".parse().unwrap(),
            "ORTHANC@192.168.1.0/24".parse().unwrap(),
        ]
    }

    fn check(callers: &[AuthorizedCaller], peer: &str, calling: &str, called: &str) -> bool {
        NodeAccessControl::new(peer.parse().unwrap(), callers)
            .check_access("SYN_PROXY", calling, called, None)
            .is_ok()
    }

    #[test]
    fn accepts_authorized_caller_from_its_network() {
        assert!(check(&callers(), "10.0.0.5", "CT_SCANNER", "SYN_PROXY"));
        assert!(check(&callers(), "192.168.1.77", "ORTHANC", "SYN_PROXY"));
        // space-padded AE titles, as they arrive on the wire
        assert!(check(
            &callers(),
            "10.0.0.5",
            "CT_SCANNER      ",
            "SYN_PROXY"
        ));
    }

    #[test]
    fn rejects_known_ae_from_foreign_network() {
        assert!(!check(&callers(), "10.0.0.6", "CT_SCANNER", "SYN_PROXY"));
        assert!(!check(&callers(), "192.168.2.77", "ORTHANC", "SYN_PROXY"));
    }

    #[test]
    fn rejects_unknown_calling_ae() {
        assert!(!check(&callers(), "10.0.0.5", "STRANGER", "SYN_PROXY"));
    }

    #[test]
    fn rejects_wrong_called_ae() {
        assert!(!check(&callers(), "10.0.0.5", "CT_SCANNER", "SOMEONE_ELSE"));
    }

    #[test]
    fn empty_caller_list_rejects_everyone() {
        assert!(!check(&[], "127.0.0.1", "CT_SCANNER", "SYN_PROXY"));
    }
}
