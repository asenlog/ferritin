use dotenvy::dotenv;
use std::path::PathBuf;

/// This node's own DICOM identity (deployment config, from env).
#[derive(Debug, Clone)]
pub struct DICOMServerConfig {
    pub facility_name: String,
    pub host: String,
    pub port: u16,
    pub ae_title: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub dicom_server: DICOMServerConfig,
    /// P3 placeholder — no HL7 settings exist yet (see ROADMAP).
    #[allow(dead_code)]
    pub hl7_server: HL7ServerConfig,
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
            dicom_server: DICOMServerConfig {
                facility_name: required("FACILITY_NAME")?,
                host: required("LISTEN_HOST")?,
                port: required("LISTEN_PORT")?
                    .parse()
                    .map_err(|_| ConfigError::InvalidNumber("LISTEN_PORT".to_string()))?,
                ae_title: required("LISTEN_AE_TITLE")?,
            },
            hl7_server: HL7ServerConfig {},
            aws: AwsConfig {
                s3_bucket: required("S3_BUCKET")?,
                sqs_queue_url: required("SQS_QUEUE_URL")?,
            },
            storage: StorageConfig {
                storage_root: required("STORAGE_ROOT")?.into(),
                database_url: required("DATABASE_URL")?,
                backend: match required("STORAGE_BACKEND")?.as_str() {
                    "fs" => StorageBackend::Fs,
                    "s3" => StorageBackend::S3,
                    other => return Err(ConfigError::InvalidStorageBackend(other.to_string())),
                },
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct HL7ServerConfig {}

#[derive(Debug, Clone)]
pub struct AwsConfig {
    pub s3_bucket: String,
    pub sqs_queue_url: String,
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub storage_root: PathBuf,
    pub database_url: String,
    pub backend: StorageBackend,
}

/// Which `ObjectStore` adapter the server persists through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StorageBackend {
    Fs,
    S3,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required env var: {0}")]
    Missing(String),

    #[error("env var {0} is not a valid number")]
    InvalidNumber(String),

    #[error("STORAGE_BACKEND must be \"fs\" or \"s3\", got {0:?}")]
    InvalidStorageBackend(String),
}

#[cfg(test)]
mod test {
    use super::*;

    /// One test only: parallel tests mutating the same process env
    /// race each other.
    #[test]
    fn load_reads_env_and_rejects_bad_backend() {
        std::env::set_var("FACILITY_NAME", "example-clinic");
        std::env::set_var("LISTEN_HOST", "0.0.0.0");
        std::env::set_var("LISTEN_PORT", "11113");
        std::env::set_var("LISTEN_AE_TITLE", "SYN_PROXY");
        std::env::set_var("S3_BUCKET", "ferritin-exams");
        std::env::set_var(
            "SQS_QUEUE_URL",
            "https://sqs.eu-central-1.amazonaws.com/123456789012/ferritin-results",
        );
        std::env::set_var("STORAGE_ROOT", "/var/lib/ferritin/storage");
        std::env::set_var(
            "DATABASE_URL",
            "postgres://ferritin@localhost:5432/ferritin",
        );
        std::env::set_var("STORAGE_BACKEND", "fs");

        let cfg = Config::load().unwrap();

        assert_eq!(cfg.dicom_server.facility_name, "example-clinic");
        assert_eq!(cfg.dicom_server.host, "0.0.0.0");
        assert_eq!(cfg.dicom_server.port, 11113);
        assert_eq!(cfg.dicom_server.ae_title, "SYN_PROXY");
        assert_eq!(cfg.aws.s3_bucket, "ferritin-exams");
        assert_eq!(
            cfg.storage.database_url,
            "postgres://ferritin@localhost:5432/ferritin"
        );
        assert_eq!(cfg.storage.backend, StorageBackend::Fs);

        std::env::set_var("STORAGE_BACKEND", "floppy-disk");
        assert!(matches!(
            Config::load(),
            Err(ConfigError::InvalidStorageBackend(_))
        ));
    }
}
