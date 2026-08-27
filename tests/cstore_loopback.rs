//! In-process C-STORE interop test: a dicom-ul client association
//! against the real SCP adapter over loopback TCP — the same path
//! DCMTK `storescu` exercises, without external tools.

use dicom_core::{dicom_value, DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;
use dicom_transfer_syntax_registry::TransferSyntaxIndex;
use dicom_ul::association::client::ClientAssociationOptions;
use dicom_ul::pdu::{PDataValue, PDataValueType, Pdu};
use ferritin::app::dicom::dimse;
use ferritin::config::DICOMServerConfig;
use ferritin::infra::db::InMemoryMappingStore;
use ferritin::infra::scp::Server;
use ferritin::infra::store::FsObjectStore;
use std::net::TcpListener;

mod fixtures;

const CT_IMAGE_STORAGE: &str = uids::CT_IMAGE_STORAGE;
const SOP_INSTANCE_UID: &str = "1.2.3.4.5.6.7.8.9";
const STUDY_INSTANCE_UID: &str = "1.2.3.4.5";
const SERIES_INSTANCE_UID: &str = "1.2.3.4.5.6";

fn test_config() -> DICOMServerConfig {
    DICOMServerConfig {
        facility_name: "test".to_string(),
        host: "127.0.0.1".to_string(),
        port: 0,
        ae_title: "TEST-SCP".to_string(),
    }
}

fn test_callers() -> fixtures::StaticCallers {
    fixtures::StaticCallers(vec!["TEST-SCU@127.0.0.1".parse().unwrap()])
}

fn dataset_bytes(ts_uid: &str) -> Vec<u8> {
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
    ]);
    // a real SCU encodes its dataset in the negotiated transfer syntax
    let ts = dicom_transfer_syntax_registry::TransferSyntaxRegistry
        .get(ts_uid)
        .expect("negotiated transfer syntax must be in the registry");
    let mut bytes = Vec::new();
    obj.write_dataset_with_ts(&mut bytes, ts).unwrap();
    bytes
}

fn store_request(message_id: u16) -> InMemDicomObject {
    dimse::store_request(message_id, CT_IMAGE_STORAGE, SOP_INSTANCE_UID)
}

#[test]
fn c_store_round_trip_persists_instance() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = Server::new(
        test_config(),
        FsObjectStore::new(dir.path()),
        InMemoryMappingStore::default(),
        test_callers(),
    );
    // serve until the test process exits; the client disconnects below
    std::thread::spawn(move || {
        let _ = server.serve(listener);
    });

    let mut client = ClientAssociationOptions::new()
        .calling_ae_title("TEST-SCU")
        .called_ae_title("TEST-SCP")
        .with_abstract_syntax(CT_IMAGE_STORAGE)
        .establish_with(&addr.to_string())
        .unwrap();

    let context_id = client.presentation_contexts()[0].id;
    let ts_uid = client.presentation_contexts()[0].transfer_syntax.clone();

    client
        .send(&Pdu::PData {
            data: vec![PDataValue {
                presentation_context_id: context_id,
                value_type: PDataValueType::Command,
                is_last: true,
                data: dimse::command_to_bytes(&store_request(1)).unwrap(),
            }],
        })
        .unwrap();
    client
        .send(&Pdu::PData {
            data: vec![PDataValue {
                presentation_context_id: context_id,
                value_type: PDataValueType::Data,
                is_last: true,
                data: dataset_bytes(&ts_uid),
            }],
        })
        .unwrap();

    match client.receive().unwrap() {
        Pdu::PData { data } => {
            let rsp = dimse::parse_command(&data[0].data).unwrap();
            assert_eq!(
                dimse::command_field(&rsp).unwrap(),
                dimse::command::C_STORE_RSP
            );
            assert_eq!(
                rsp.element(tags::STATUS).unwrap().to_int::<u16>().unwrap(),
                dimse::status::SUCCESS
            );
            assert_eq!(
                dimse::affected_sop_instance_uid(&rsp).unwrap(),
                SOP_INSTANCE_UID
            );
        }
        other => panic!("expected C-STORE-RSP, got {}", other.short_description()),
    }
    client.send(&Pdu::ReleaseRQ).unwrap();

    // the instance landed under the deterministic key
    let stored = dir.path().join(format!(
        "{STUDY_INSTANCE_UID}/{SERIES_INSTANCE_UID}/{SOP_INSTANCE_UID}.dcm"
    ));
    assert!(stored.exists(), "expected stored object at {stored:?}");

    // and it is a valid Part-10 file
    let bytes = std::fs::read(&stored).unwrap();
    assert_eq!(&bytes[128..132], b"DICM");
}

#[test]
fn unknown_calling_ae_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = Server::new(
        test_config(),
        FsObjectStore::new(dir.path()),
        InMemoryMappingStore::default(),
        test_callers(),
    );
    std::thread::spawn(move || {
        let _ = server.serve(listener);
    });

    let result = ClientAssociationOptions::new()
        .calling_ae_title("STRANGER")
        .called_ae_title("TEST-SCP")
        .with_abstract_syntax(CT_IMAGE_STORAGE)
        .establish_with(&addr.to_string());

    assert!(result.is_err(), "unauthorized caller must not establish");
}

#[test]
fn wrong_called_ae_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = Server::new(
        test_config(),
        FsObjectStore::new(dir.path()),
        InMemoryMappingStore::default(),
        test_callers(),
    );
    std::thread::spawn(move || {
        let _ = server.serve(listener);
    });

    let result = ClientAssociationOptions::new()
        .calling_ae_title("TEST-SCU")
        .called_ae_title("SOMEONE-ELSE")
        .with_abstract_syntax(CT_IMAGE_STORAGE)
        .establish_with(&addr.to_string());

    assert!(result.is_err(), "wrong called AE must not establish");
}
