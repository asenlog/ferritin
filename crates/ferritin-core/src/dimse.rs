//! Pure DIMSE command handling: parsing and building command sets.
//!
//! No sockets, no PDUs — the SCP adapter (`scp`) feeds raw command-set
//! bytes in and serializes the returned objects back out. Everything
//! here is unit-testable without network I/O.

use anyhow::Context;
use dicom_core::{dicom_value, DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;
use dicom_transfer_syntax_registry::entries;

/// Verification SOP Class — the abstract syntax C-ECHO uses.
pub const VERIFICATION_SOP_CLASS: &str = uids::VERIFICATION;

/// Transfer syntaxes the SCP accepts for datasets, in preference order.
pub const TRANSFER_SYNTAXES: &[&str] = &[
    uids::IMPLICIT_VR_LITTLE_ENDIAN, // DICOM baseline
    uids::EXPLICIT_VR_LITTLE_ENDIAN,
    uids::EXPLICIT_VR_BIG_ENDIAN,
];

/// Storage SOP Classes the SCP accepts instances of.
pub const STORAGE_SOP_CLASSES: &[&str] = &[
    uids::COMPUTED_RADIOGRAPHY_IMAGE_STORAGE,
    uids::DIGITAL_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    uids::DIGITAL_X_RAY_IMAGE_STORAGE_FOR_PROCESSING,
    uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PROCESSING,
    uids::BREAST_TOMOSYNTHESIS_IMAGE_STORAGE,
    uids::CT_IMAGE_STORAGE,
    uids::MR_IMAGE_STORAGE,
    uids::SECONDARY_CAPTURE_IMAGE_STORAGE,
];

/// DIMSE command field values (0000,0100).
pub mod command {
    pub const C_STORE_RQ: u16 = 0x0001;
    pub const C_STORE_RSP: u16 = 0x8001;
    pub const C_ECHO_RQ: u16 = 0x0030;
    pub const C_ECHO_RSP: u16 = 0x8030;
}

/// DIMSE status codes relevant to intake.
pub mod status {
    pub const SUCCESS: u16 = 0x0000;
    /// Refused: out of resources — e.g. persistence failed.
    pub const REFUSED_OUT_OF_RESOURCES: u16 = 0xA700;
    /// Error: cannot understand — e.g. the dataset could not be parsed.
    pub const CANNOT_UNDERSTAND: u16 = 0xC000;
}

/// CommandDataSetType value for "no dataset follows" (PS3.7 §9.3).
const NO_DATASET: u16 = 0x0101;

/// Parse a DIMSE command set. Command sets are always encoded
/// Implicit VR Little Endian (PS3.7 §9.2), regardless of the transfer
/// syntax negotiated for datasets.
pub fn parse_command(bytes: &[u8]) -> anyhow::Result<InMemDicomObject> {
    let ts = entries::IMPLICIT_VR_LITTLE_ENDIAN.erased();
    InMemDicomObject::read_dataset_with_ts(bytes, &ts).context("failed to parse DIMSE command set")
}

/// Serialize a command set back to bytes (Implicit VR Little Endian).
pub fn command_to_bytes(cmd: &InMemDicomObject) -> anyhow::Result<Vec<u8>> {
    let ts = entries::IMPLICIT_VR_LITTLE_ENDIAN.erased();
    let mut bytes = Vec::new();
    cmd.write_dataset_with_ts(&mut bytes, &ts)
        .context("failed to serialize DIMSE command set")?;
    Ok(bytes)
}

pub fn command_field(cmd: &InMemDicomObject) -> anyhow::Result<u16> {
    Ok(cmd.element(tags::COMMAND_FIELD)?.to_int::<u16>()?)
}

pub fn message_id(cmd: &InMemDicomObject) -> anyhow::Result<u16> {
    Ok(cmd.element(tags::MESSAGE_ID)?.to_int::<u16>()?)
}

/// Affected SOP Class UID (0000,0002), trimmed of DICOM padding.
pub fn affected_sop_class_uid(cmd: &InMemDicomObject) -> anyhow::Result<String> {
    uid_element(cmd, tags::AFFECTED_SOP_CLASS_UID)
}

/// Affected SOP Instance UID (0000,1000), trimmed of DICOM padding.
pub fn affected_sop_instance_uid(cmd: &InMemDicomObject) -> anyhow::Result<String> {
    uid_element(cmd, tags::AFFECTED_SOP_INSTANCE_UID)
}

fn uid_element(cmd: &InMemDicomObject, tag: dicom_core::Tag) -> anyhow::Result<String> {
    let value = cmd.element(tag)?.to_str()?;
    Ok(value.trim_end_matches(['\0', ' ']).to_string())
}

/// Build a C-ECHO-RSP command set (PS3.7 §9.1.5). No CommandGroupLength
/// element — the serializer computes it.
pub fn echo_response(message_id: u16) -> InMemDicomObject {
    InMemDicomObject::command_from_element_iter([
        DataElement::new(
            tags::AFFECTED_SOP_CLASS_UID,
            VR::UI,
            dicom_value!(Str, VERIFICATION_SOP_CLASS),
        ),
        DataElement::new(
            tags::COMMAND_FIELD,
            VR::US,
            dicom_value!(U16, [command::C_ECHO_RSP]),
        ),
        DataElement::new(
            tags::MESSAGE_ID_BEING_RESPONDED_TO,
            VR::US,
            dicom_value!(U16, [message_id]),
        ),
        DataElement::new(
            tags::COMMAND_DATA_SET_TYPE,
            VR::US,
            dicom_value!(U16, [NO_DATASET]),
        ),
        DataElement::new(tags::STATUS, VR::US, dicom_value!(U16, [status::SUCCESS])),
    ])
}

/// Build a C-STORE-RSP command set (PS3.7 §9.1.1.1.9): echoes the
/// affected SOP Class/Instance UIDs and carries the outcome status.
pub fn store_response(
    message_id: u16,
    sop_class_uid: &str,
    sop_instance_uid: &str,
    status: u16,
) -> InMemDicomObject {
    InMemDicomObject::command_from_element_iter([
        DataElement::new(
            tags::AFFECTED_SOP_CLASS_UID,
            VR::UI,
            dicom_value!(Str, sop_class_uid),
        ),
        DataElement::new(
            tags::COMMAND_FIELD,
            VR::US,
            dicom_value!(U16, [command::C_STORE_RSP]),
        ),
        DataElement::new(
            tags::MESSAGE_ID_BEING_RESPONDED_TO,
            VR::US,
            dicom_value!(U16, [message_id]),
        ),
        DataElement::new(
            tags::COMMAND_DATA_SET_TYPE,
            VR::US,
            dicom_value!(U16, [NO_DATASET]),
        ),
        DataElement::new(tags::STATUS, VR::US, dicom_value!(U16, [status])),
        DataElement::new(
            tags::AFFECTED_SOP_INSTANCE_UID,
            VR::UI,
            dicom_value!(Str, sop_instance_uid),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_response_carries_request_message_id_and_success() {
        let rsp = echo_response(42);

        assert_eq!(command_field(&rsp).unwrap(), command::C_ECHO_RSP);
        assert_eq!(
            rsp.element(tags::MESSAGE_ID_BEING_RESPONDED_TO)
                .unwrap()
                .to_int::<u16>()
                .unwrap(),
            42
        );
        assert_eq!(
            rsp.element(tags::STATUS).unwrap().to_int::<u16>().unwrap(),
            status::SUCCESS
        );
        assert_eq!(
            rsp.element(tags::AFFECTED_SOP_CLASS_UID)
                .unwrap()
                .to_str()
                .unwrap(),
            VERIFICATION_SOP_CLASS
        );
    }

    #[test]
    fn store_response_echoes_uids_and_carries_status() {
        let rsp = store_response(7, uids::CT_IMAGE_STORAGE, "1.2.3.4.5", status::SUCCESS);

        assert_eq!(command_field(&rsp).unwrap(), command::C_STORE_RSP);
        assert_eq!(
            rsp.element(tags::MESSAGE_ID_BEING_RESPONDED_TO)
                .unwrap()
                .to_int::<u16>()
                .unwrap(),
            7
        );
        assert_eq!(
            rsp.element(tags::STATUS).unwrap().to_int::<u16>().unwrap(),
            status::SUCCESS
        );
        assert_eq!(
            affected_sop_class_uid(&rsp).unwrap(),
            uids::CT_IMAGE_STORAGE
        );
        assert_eq!(affected_sop_instance_uid(&rsp).unwrap(), "1.2.3.4.5");
    }

    #[test]
    fn command_round_trip_survives_serialization() {
        let bytes = command_to_bytes(&echo_response(7)).unwrap();
        let parsed = parse_command(&bytes).unwrap();

        assert_eq!(command_field(&parsed).unwrap(), command::C_ECHO_RSP);
        assert_eq!(
            parsed
                .element(tags::MESSAGE_ID_BEING_RESPONDED_TO)
                .unwrap()
                .to_int::<u16>()
                .unwrap(),
            7
        );
    }

    #[test]
    fn parses_echo_request_command() {
        let rq = InMemDicomObject::command_from_element_iter([
            DataElement::new(
                tags::COMMAND_FIELD,
                VR::US,
                dicom_value!(U16, [command::C_ECHO_RQ]),
            ),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [9])),
        ]);
        let parsed = parse_command(&command_to_bytes(&rq).unwrap()).unwrap();

        assert_eq!(command_field(&parsed).unwrap(), command::C_ECHO_RQ);
        assert_eq!(message_id(&parsed).unwrap(), 9);
    }
}
