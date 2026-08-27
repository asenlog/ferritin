//! DICOM SCU (Service Class User) — the outbound adapter: C-STORE
//! re-identified results to destination AEs.
//!
//! Pure client work: one association per object, released after the
//! response. Routing decisions live in `rules`, the pipeline in
//! `forward`; neither knows this module exists.

use crate::dimse;
use crate::domain::rules::Destination;
use anyhow::{bail, Context};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use dicom_transfer_syntax_registry::{TransferSyntaxIndex, TransferSyntaxRegistry};
use dicom_ul::association::client::ClientAssociationOptions;
use dicom_ul::pdu::{PDataValue, PDataValueType, Pdu, PresentationContextResultReason};

/// C-STORE client bound to a local AE title.
pub struct ScuClient {
    calling_ae_title: String,
}

impl ScuClient {
    pub fn new(calling_ae_title: impl Into<String>) -> Self {
        Self {
            calling_ae_title: calling_ae_title.into(),
        }
    }

    /// Store `obj` at `dest`. The association proposes the object's
    /// SOP class with the little-endian transfer syntaxes; the dataset
    /// is encoded in whichever one the destination accepts.
    pub fn cstore(&self, dest: &Destination, obj: &InMemDicomObject) -> anyhow::Result<()> {
        let sop_class_uid =
            uid_of(obj, tags::SOP_CLASS_UID).context("object has no SOP Class UID")?;
        let sop_instance_uid =
            uid_of(obj, tags::SOP_INSTANCE_UID).context("object has no SOP Instance UID")?;

        let mut assoc = ClientAssociationOptions::new()
            .calling_ae_title(self.calling_ae_title.as_str())
            .called_ae_title(dest.ae_title.as_str())
            .with_abstract_syntax(sop_class_uid.as_str())
            .establish((dest.host.as_str(), dest.port))
            .with_context(|| {
                format!(
                    "association to {}@{}:{} failed",
                    dest.ae_title, dest.host, dest.port
                )
            })?;

        let context = assoc
            .presentation_contexts()
            .first()
            .context("association negotiated no presentation contexts")?;
        if context.reason != PresentationContextResultReason::Acceptance {
            bail!("destination rejected the presentation context for {sop_class_uid}");
        }
        let context_id = context.id;
        let ts = TransferSyntaxRegistry
            .get(&context.transfer_syntax)
            .with_context(|| format!("negotiated unknown transfer syntax {}", context.transfer_syntax))?;

        assoc.send(&Pdu::PData {
            data: vec![PDataValue {
                presentation_context_id: context_id,
                value_type: PDataValueType::Command,
                is_last: true,
                data: dimse::command_to_bytes(&dimse::store_request(
                    1,
                    &sop_class_uid,
                    &sop_instance_uid,
                ))?,
            }],
        })?;

        let mut dataset = Vec::new();
        obj.write_dataset_with_ts(&mut dataset, ts)
            .context("failed to encode dataset")?;
        assoc.send(&Pdu::PData {
            data: vec![PDataValue {
                presentation_context_id: context_id,
                value_type: PDataValueType::Data,
                is_last: true,
                data: dataset,
            }],
        })?;

        let status = match assoc.receive().context("no C-STORE response")? {
            Pdu::PData { data } => {
                let rsp = dimse::parse_command(&data[0].data)?;
                if dimse::command_field(&rsp)? != dimse::command::C_STORE_RSP {
                    bail!("expected C-STORE-RSP");
                }
                rsp.element(tags::STATUS)?.to_int::<u16>()?
            }
            other => bail!("expected C-STORE-RSP, got {}", other.short_description()),
        };
        let _ = assoc.send(&Pdu::ReleaseRQ);

        if status != dimse::status::SUCCESS {
            bail!("destination refused the store, status 0x{status:04x}");
        }
        Ok(())
    }
}

/// Read a UI element trimmed of DICOM padding.
fn uid_of(obj: &InMemDicomObject, tag: dicom_core::Tag) -> Option<String> {
    let value = obj.element(tag).ok()?.to_str().ok()?;
    let trimmed = value.trim_end_matches(['\0', ' ']);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
