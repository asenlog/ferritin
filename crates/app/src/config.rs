use dicom::filter::SopClassFilter;
use dotenvy::dotenv;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub facility: FacilityConfig,
    pub dicom: DicomConfig,
    pub aws: AwsConfig,
    pub storage: StorageConfig,
}

fn required(key: &str) -> Result<String, ConfigError> {
    std::env::var(key).map_err(|_| ConfigError::Missing(key.to_string()))
}

impl Config {
    pub fn load() -> Result<Config, ConfigError> {
        dotenv().ok();

        Ok(Config {
            facility: FacilityConfig {
                facility_name: required("FACILITY_NAME")?,
            },
            dicom: DicomConfig {
                listen_ae_title: required("LISTEN_AE_TITLE")?,
                listen_port: required("LISTEN_PORT")?
                    .parse()
                    .map_err(|_| ConfigError::InvalidNumber("LISTEN_PORT".to_string()))?,
                filter: SopClassFilter::new(
                    required("FILTER_ALLOWED_MODALITY")?.into(),
                    required("FILTER_ALLOWED_SOP_CLASSES")?
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect(),
                    required("FILTER_VENDOR_BLOCKLIST")?
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect(),
                ),
                registered_sources: serde_json::from_str(&required("REGISTERED_SOURCES")?)?,
            },
            aws: AwsConfig {
                s3_bucket: required("S3_BUCKET")?,
                sqs_queue_url: required("SQS_QUEUE_URL")?,
            },
            storage: StorageConfig {
                storage_root: required("STORAGE_ROOT")?.into(),
                sqlite_path: required("SQLITE_PATH")?.into(),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct FacilityConfig {
    pub facility_name: String,
}

#[derive(Debug, Clone)]
pub struct DicomConfig {
    pub listen_ae_title: String,
    pub listen_port: u16,
    pub filter: SopClassFilter,
    pub registered_sources: Vec<RegisteredSource>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisteredSource {
    pub ae_title: String,
    pub host: String,
    pub result_destination_ae_title: String,
    pub result_destination_host: String,
    pub result_destination_port: u16,
}

#[derive(Debug, Clone)]
pub struct AwsConfig {
    pub s3_bucket: String,
    pub sqs_queue_url: String,
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub storage_root: PathBuf,
    pub sqlite_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required env var: {0}")]
    Missing(String),

    #[error("env var {0} is not a valid number")]
    InvalidNumber(String),

    #[error("REGISTERED_SOURCES is not valid JSON: {0}")]
    InvalidSources(#[from] serde_json::Error),
}

#[cfg(test)]
mod test {
    use super::*;
    
    fn set_test_env() {
        std::env::set_var("FACILITY_NAME", "example-clinic");
        std::env::set_var("LISTEN_AE_TITLE", "SYN_PROXY");
        std::env::set_var("LISTEN_PORT", "11113");
        std::env::set_var("FILTER_ALLOWED_MODALITY", "MG");
        std::env::set_var(
            "FILTER_ALLOWED_SOP_CLASSES",
            "1.2.840.10008.5.1.4.1.1.1.2, 1.2.840.10008.5.1.4.1.1.13.1.3",
        );
        std::env::set_var("FILTER_VENDOR_BLOCKLIST", "lunit,screenpoint");
        std::env::set_var(
            "REGISTERED_SOURCES",
            r#"[{"ae_title":"MAMMO_1","host":"192.168.1.50","result_destination_ae_title":"PACS","result_destination_host":"192.168.1.10","result_destination_port":104}]"#,
        );
        std::env::set_var("S3_BUCKET", "ferritin-exams");
        std::env::set_var(
            "SQS_QUEUE_URL",
            "https://sqs.eu-central-1.amazonaws.com/123456789012/ferritin-results",
        );
        std::env::set_var("STORAGE_ROOT", "/var/lib/ferritin/storage");
        std::env::set_var("SQLITE_PATH", "/var/lib/ferritin/node.db");
    }

    #[test]
    fn load_reads_env_vars() {
        set_test_env();

        let cfg = Config::load().unwrap();

        assert_eq!(cfg.facility.facility_name, "example-clinic");
        assert_eq!(cfg.dicom.listen_ae_title, "SYN_PROXY");
        assert_eq!(cfg.dicom.listen_port, 11113);
        assert_eq!(cfg.dicom.registered_sources.len(), 1);
        assert_eq!(cfg.dicom.registered_sources[0].ae_title, "MAMMO_1");
        assert_eq!(
            cfg.dicom.registered_sources[0].result_destination_port,
            104
        );
        assert_eq!(cfg.aws.s3_bucket, "ferritin-exams");
        assert_eq!(
            cfg.storage.sqlite_path,
            PathBuf::from("/var/lib/ferritin/node.db")
        );
    }
}
