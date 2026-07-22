//Service Class Provider
use crate::config::DICOMServerConfig;
use dicom_core::header::Header;
use dicom_core::{dicom_value, DataElement, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use dicom_ul::association::server::ServerAssociationOptions;
use dicom_ul::association::Association;
use dicom_ul::pdu::{PDataValue, PDataValueType, Pdu};
use std::net::{TcpListener, TcpStream};

pub struct Server {
    config: DICOMServerConfig,
}

impl Server {
    pub fn new(config: DICOMServerConfig) -> Self {
        Self { config }
    }

    pub fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind((self.config.host.as_str(), self.config.port))?;
        tracing::info!(
            "DICOM Server listening on {}:{}",
            self.config.host,
            self.config.port
        );

        for stream in listener.incoming() {
            let socket = match stream {
                Ok(socket) => socket,
                Err(e) => {
                    tracing::warn!("failed to accept incoming connection {e}");
                    continue;
                },
            };

            if let Err(e) = self.handle_association(socket) {
                tracing::warn!("association ended with error: {e:#}");
            }
        }
        Ok(())
    }

    fn handle_association(&self, socket: TcpStream) -> anyhow::Result<()> {
        let scp_options = ServerAssociationOptions::new()
            .accept_any() 
            .ae_title(self.config.ae_title.as_str())
            .with_abstract_syntax("1.2.840.10008.1.1"); // node needs to be into the known nodes

        let mut association = scp_options.establish(socket)?;
        tracing::info!("association from '{}'", association.peer_ae_title());

        loop {
            match association.receive()? {
                Pdu::PData { data } => {
                  let ts = dicom_transfer_syntax_registry::entries::IMPLICIT_VR_LITTLE_ENDIAN.erased();
                  let command = dicom_object::InMemDicomObject::read_dataset_with_ts(&data[0].data[..], &ts)?;
                  tracing::info!(
                      "PDV: ctx_id={} type={:?} is_last={} ({} bytes)",
                      data[0].presentation_context_id,
                      data[0].value_type,
                      data[0].is_last,
                      data[0].data.len(),
                  );

                  for elem in &command {
                      tracing::info!("  {} {} {:?}", elem.tag(), elem.vr(), elem.value());
                  }

                  let command_field = command.element(tags::COMMAND_FIELD)?.to_int::<u16>()?;
                  if command_field == 0x0030 {
                      let message_id = command.element(tags::MESSAGE_ID)?.to_int::<u16>()?;

                      // C-ECHO-RSP, per PS3.7 §9.1.5: a fresh command object.
                      // No CommandGroupLength element — the serializer computes it.
                      let rsp = InMemDicomObject::command_from_element_iter([
                          DataElement::new(
                              tags::AFFECTED_SOP_CLASS_UID,
                              VR::UI,
                              dicom_value!(Str, "1.2.840.10008.1.1"),
                          ),
                          DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [0x8030])),
                          DataElement::new(
                              tags::MESSAGE_ID_BEING_RESPONDED_TO,
                              VR::US,
                              dicom_value!(U16, [message_id]),
                          ),
                          DataElement::new(
                              tags::COMMAND_DATA_SET_TYPE,
                              VR::US,
                              dicom_value!(U16, [0x0101]),
                          ),
                          DataElement::new(tags::STATUS, VR::US, dicom_value!(U16, [0x0000])),
                      ]);

                      let mut rsp_bytes = Vec::new();
                      rsp.write_dataset_with_ts(&mut rsp_bytes, &ts)?;

                      association.send(&Pdu::PData {
                          data: vec![PDataValue {
                              presentation_context_id: data[0].presentation_context_id,
                              value_type: PDataValueType::Command,
                              is_last: true,
                              data: rsp_bytes,
                          }],
                      })?;
                      tracing::info!("answered C-ECHO (message id {message_id})");
                  } else {
                      tracing::warn!("unsupported command field: 0x{command_field:04x}");
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
