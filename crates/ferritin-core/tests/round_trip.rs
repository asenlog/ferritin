//! Full round trip in-process: an identified study is C-STOREd into
//! ferritin (de-identified on intake), the stored object plays the
//! role of a cloud result, and the forwarding pipeline re-identifies
//! it and C-STOREs it to a destination AE — asserted against a dumb
//! capture SCP standing in for the PACS.

use dicom_core::{dicom_value, DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;
use dicom_transfer_syntax_registry::TransferSyntaxIndex;
use dicom_ul::association::client::ClientAssociationOptions;
use dicom_ul::association::server::ServerAssociationOptions;
use dicom_ul::pdu::{PDataValue, PDataValueType, Pdu, PresentationContextResultReason};
use ferritin_core::config::DICOMServerConfig;
use ferritin_core::db::InMemoryMappingStore;
use ferritin_core::ports::MappingStore;
use ferritin_core::scp::Server;
use ferritin_core::scu::ScuClient;
use ferritin_core::service::forward::{ForwardError, ForwardingService};
use ferritin_core::store::FsObjectStore;
use ferritin_core::{dicom::dimse, models::rules};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};

mod fixtures;

const CT_IMAGE_STORAGE: &str = uids::CT_IMAGE_STORAGE;
const SOP_INSTANCE_UID: &str = "1.2.3.4.5.6.7.8.9";
const STUDY_INSTANCE_UID: &str = "1.2.3.4.5";
const SERIES_INSTANCE_UID: &str = "1.2.3.4.5.6";

fn identified_dataset(ts_uid: &str) -> Vec<u8> {
    let obj = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            dicom_value!(Str, CT_IMAGE_STORAGE),
        ),
        DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            dicom_value!(Str, SOP_INSTANCE_UID),
        ),
        DataElement::new(
            tags::STUDY_INSTANCE_UID,
            VR::UI,
            dicom_value!(Str, STUDY_INSTANCE_UID),
        ),
        DataElement::new(
            tags::SERIES_INSTANCE_UID,
            VR::UI,
            dicom_value!(Str, SERIES_INSTANCE_UID),
        ),
        DataElement::new(tags::MODALITY, VR::CS, dicom_value!(Str, "CT")),
        DataElement::new(tags::PATIENT_NAME, VR::PN, dicom_value!(Str, "Doe^John")),
        DataElement::new(tags::PATIENT_ID, VR::LO, dicom_value!(Str, "PAT-1")),
        DataElement::new(
            tags::PATIENT_BIRTH_DATE,
            VR::DA,
            dicom_value!(Str, "19700101"),
        ),
    ]);
    let ts = dicom_transfer_syntax_registry::TransferSyntaxRegistry
        .get(ts_uid)
        .unwrap();
    let mut bytes = Vec::new();
    obj.write_dataset_with_ts(&mut bytes, ts).unwrap();
    bytes
}

/// A captured dataset: negotiated transfer syntax + raw bytes.
type CapturedDatasets = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// A running capture SCP: its address and the datasets it received.
struct Destination {
    addr: SocketAddr,
    captured: CapturedDatasets,
}

/// A minimal DICOM sink: accepts anything, answers C-STORE with
/// success, and captures raw datasets with their transfer syntax.
fn spawn_destination() -> Destination {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();

    std::thread::spawn(move || {
        let stream = listener.incoming().next().unwrap().unwrap();
        let mut assoc = ServerAssociationOptions::new()
            .accept_any()
            .ae_title("DEST")
            .with_transfer_syntax(uids::IMPLICIT_VR_LITTLE_ENDIAN)
            .with_transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
            .with_abstract_syntax(CT_IMAGE_STORAGE)
            .establish(stream)
            .unwrap();
        let contexts: HashMap<u8, String> = assoc
            .presentation_contexts()
            .iter()
            .filter(|pc| pc.reason == PresentationContextResultReason::Acceptance)
            .map(|pc| (pc.id, pc.transfer_syntax.clone()))
            .collect();

        let mut command_buf = Vec::new();
        let mut dataset_buf = Vec::new();
        let mut pending: Option<(u16, String, String, u8)> = None;
        loop {
            match assoc.receive().unwrap() {
                Pdu::PData { data } => {
                    for pdv in data {
                        match pdv.value_type {
                            PDataValueType::Command => {
                                command_buf.extend_from_slice(&pdv.data);
                                if pdv.is_last {
                                    let cmd = dimse::parse_command(&command_buf).unwrap();
                                    command_buf.clear();
                                    pending = Some((
                                        dimse::message_id(&cmd).unwrap(),
                                        dimse::affected_sop_class_uid(&cmd).unwrap(),
                                        dimse::affected_sop_instance_uid(&cmd).unwrap(),
                                        pdv.presentation_context_id,
                                    ));
                                }
                            }
                            PDataValueType::Data => {
                                dataset_buf.extend_from_slice(&pdv.data);
                                if pdv.is_last {
                                    let (message_id, sop_class, sop_instance, ctx) =
                                        pending.take().unwrap();
                                    sink.lock().unwrap().push((
                                        contexts[&ctx].clone(),
                                        std::mem::take(&mut dataset_buf),
                                    ));
                                    let rsp = dimse::store_response(
                                        message_id,
                                        &sop_class,
                                        &sop_instance,
                                        dimse::status::SUCCESS,
                                    );
                                    assoc
                                        .send(&Pdu::PData {
                                            data: vec![PDataValue {
                                                presentation_context_id: ctx,
                                                value_type: PDataValueType::Command,
                                                is_last: true,
                                                data: dimse::command_to_bytes(&rsp).unwrap(),
                                            }],
                                        })
                                        .unwrap();
                                }
                            }
                        }
                    }
                }
                Pdu::ReleaseRQ => {
                    assoc.send(&Pdu::ReleaseRP).unwrap();
                    break;
                }
                _ => {}
            }
        }
    });
    Destination { addr, captured }
}

fn text_of(obj: &InMemDicomObject, tag: dicom_core::Tag) -> String {
    obj.element(tag)
        .map(|e| {
            e.to_str()
                .unwrap()
                .trim_end_matches(['\0', ' '])
                .to_string()
        })
        .unwrap_or_default()
}

#[test]
fn round_trip_restores_identity_at_the_destination() {
    // 1. ferritin SCP with de-identifying intake
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ferritin_addr = listener.local_addr().unwrap();
    let mappings = InMemoryMappingStore::default();
    let server = Server::new(
        DICOMServerConfig {
            facility_name: "test".to_string(),
            host: "127.0.0.1".to_string(),
            port: 0,
            ae_title: "TEST-SCP".to_string(),
        },
        FsObjectStore::new(dir.path()),
        mappings.clone(),
        fixtures::StaticCallers(vec!["TEST-SCU@127.0.0.1".parse().unwrap()]),
    );
    std::thread::spawn(move || {
        let _ = server.serve(listener);
    });

    // 2. an identified study arrives
    let mut client = ClientAssociationOptions::new()
        .calling_ae_title("TEST-SCU")
        .called_ae_title("TEST-SCP")
        .with_abstract_syntax(CT_IMAGE_STORAGE)
        .establish_with(&ferritin_addr.to_string())
        .unwrap();
    let context_id = client.presentation_contexts()[0].id;
    let ts_uid = client.presentation_contexts()[0].transfer_syntax.clone();
    client
        .send(&Pdu::PData {
            data: vec![PDataValue {
                presentation_context_id: context_id,
                value_type: PDataValueType::Command,
                is_last: true,
                data: dimse::command_to_bytes(&dimse::store_request(
                    1,
                    CT_IMAGE_STORAGE,
                    SOP_INSTANCE_UID,
                ))
                .unwrap(),
            }],
        })
        .unwrap();
    client
        .send(&Pdu::PData {
            data: vec![PDataValue {
                presentation_context_id: context_id,
                value_type: PDataValueType::Data,
                is_last: true,
                data: identified_dataset(&ts_uid),
            }],
        })
        .unwrap();
    client.receive().unwrap(); // C-STORE-RSP
    client.send(&Pdu::ReleaseRQ).unwrap();

    // intake stored the de-identified Part-10 file
    let stored_path = dir.path().join(format!(
        "{STUDY_INSTANCE_UID}/{SERIES_INSTANCE_UID}/{SOP_INSTANCE_UID}.dcm"
    ));
    let stored = std::fs::read(&stored_path).unwrap();
    let stored_obj = dicom_object::from_reader(&stored[..]).unwrap();
    assert!(text_of(&stored_obj, tags::PATIENT_NAME).starts_with("ANON^"));

    // 3. the forwarding pipeline treats the stored object as a cloud
    //    result: re-identify and forward to the destination AE
    let destination = spawn_destination();
    let rule = format!(
        "CT - {CT_IMAGE_STORAGE} - DEST@127.0.0.1:{}",
        destination.addr.port()
    )
    .parse()
    .unwrap();
    let forwarding = ForwardingService::new(
        mappings,
        fixtures::StaticRules(vec![rule]),
        ScuClient::new("FERRITIN"),
    );

    forwarding.forward_result(&stored).unwrap();

    // 4. the destination received the original identity
    let captured = destination.captured.lock().unwrap();
    assert_eq!(captured.len(), 1);
    let (ts_uid, dataset) = &captured[0];
    let ts = dicom_transfer_syntax_registry::TransferSyntaxRegistry
        .get(ts_uid)
        .unwrap();
    let received = InMemDicomObject::read_dataset_with_ts(&dataset[..], ts).unwrap();
    assert_eq!(text_of(&received, tags::PATIENT_NAME), "Doe^John");
    assert_eq!(text_of(&received, tags::PATIENT_ID), "PAT-1");
    assert_eq!(
        text_of(&received, tags::STUDY_INSTANCE_UID),
        STUDY_INSTANCE_UID
    );
    // blanked on intake, never recoverable
    assert_eq!(text_of(&received, tags::PATIENT_BIRTH_DATE), "");
}

#[test]
fn forward_rejects_garbage_unknown_studies_and_unrouted() {
    fn part10(study_uid: &str) -> Vec<u8> {
        let obj = InMemDicomObject::from_element_iter([
            DataElement::new(
                tags::SOP_CLASS_UID,
                VR::UI,
                dicom_value!(Str, CT_IMAGE_STORAGE),
            ),
            DataElement::new(
                tags::SOP_INSTANCE_UID,
                VR::UI,
                dicom_value!(Str, SOP_INSTANCE_UID),
            ),
            DataElement::new(
                tags::STUDY_INSTANCE_UID,
                VR::UI,
                dicom_value!(Str, study_uid),
            ),
            DataElement::new(tags::MODALITY, VR::CS, dicom_value!(Str, "CT")),
        ]);
        let mut bytes = Vec::new();
        obj.with_meta(
            dicom_object::FileMetaTableBuilder::new()
                .transfer_syntax(uids::IMPLICIT_VR_LITTLE_ENDIAN),
        )
        .unwrap()
        .write_all(&mut bytes)
        .unwrap();
        bytes
    }

    // not a Part-10 file
    let mappings = InMemoryMappingStore::default();
    let rule: rules::ForwardingRule = format!("CT - {CT_IMAGE_STORAGE} - DEST@127.0.0.1:9")
        .parse()
        .unwrap();
    let forwarding = ForwardingService::new(
        mappings,
        fixtures::StaticRules(vec![rule]),
        ScuClient::new("FERRITIN"),
    );
    assert!(matches!(
        forwarding.forward_result(b"garbage"),
        Err(ForwardError::MalformedResult(_))
    ));

    // well-formed object, but a study the mapping store never saw
    assert!(matches!(
        forwarding.forward_result(&part10("9.9.9.9")),
        Err(ForwardError::UnknownStudy(_))
    ));

    // mapped study, but no matching rule: fails before any socket use
    let mappings = InMemoryMappingStore::default();
    mappings
        .mapping_for(STUDY_INSTANCE_UID, "PAT-1", "Doe^John")
        .unwrap();
    let forwarding = ForwardingService::new(
        mappings,
        fixtures::StaticRules(vec![]),
        ScuClient::new("FERRITIN"),
    );
    assert!(matches!(
        forwarding.forward_result(&part10(STUDY_INSTANCE_UID)),
        Err(ForwardError::NoRoute { .. })
    ));
}
