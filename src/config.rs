use dotenvy::dotenv;
use std::path::PathBuf;

pub use ferritin_core::config::DICOMServerConfig;

#[derive(Debug, Clone)]
pub struct Config {
    pub dicom_server: DICOMServerConfig,
    pub rules: DicomServerRules,
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

        let config = Config {
            dicom_server: DICOMServerConfig {
                facility_name: required("FACILITY_NAME")?,
                host: required("LISTEN_HOST")?,
                port: required("LISTEN_PORT")?
                    .parse()
                    .map_err(|_| ConfigError::InvalidNumber("LISTEN_PORT".to_string()))?,
                ae_title: required("LISTEN_AE_TITLE")?,
            },
            rules: DicomServerRules {
                dicom_rules: required("DICOM_RULES")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
            },
            hl7_server: HL7ServerConfig {},
            aws: AwsConfig {
                s3_bucket: required("S3_BUCKET")?,
                sqs_queue_url: required("SQS_QUEUE_URL")?,
            },
            storage: StorageConfig {
                storage_root: required("STORAGE_ROOT")?.into(),
                database_url: required("DATABASE_URL")?,
            },
        };

        config.validate()?;
        Ok(config)
    }

    /// Single validation gate for the fully assembled config. Sections keep
    /// their own rules; this is where they all get enforced.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.rules.dicom_rules.iter().any(|r| r.is_empty()) {
            return Err(ConfigError::EmptyDicomRule);
        }
        Ok(())
    }
}

// Forwarding rules
// Ex: MG - SOP Class (tomosynthesis for example) - DICOM node
#[derive(Debug, Clone)]
pub struct DicomServerRules {
    pub dicom_rules: Vec<String>,
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
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required env var: {0}")]
    Missing(String),

    #[error("env var {0} is not a valid number")]
    InvalidNumber(String),

    #[error("DICOM_RULES contains an empty rule (trailing comma?)")]
    EmptyDicomRule,
}

#[cfg(test)]
mod test {
    use super::*;

    fn set_test_env() {
        std::env::set_var("FACILITY_NAME", "example-clinic");
        std::env::set_var("LISTEN_HOST", "0.0.0.0");
        std::env::set_var("LISTEN_PORT", "11113");
        std::env::set_var("LISTEN_AE_TITLE", "SYN_PROXY");
        std::env::set_var(
            "DICOM_RULES",
            "MG - 1.2.840.10008.5.1.4.1.1.1.2 - PACS@192.168.1.10:104, MG - 1.2.840.10008.5.1.4.1.1.13.1.3 - PACS@192.168.1.10:104",
        );
        std::env::set_var("S3_BUCKET", "ferritin-exams");
        std::env::set_var(
            "SQS_QUEUE_URL",
            "https://sqs.eu-central-1.amazonaws.com/123456789012/ferritin-results",
        );
        std::env::set_var("STORAGE_ROOT", "/var/lib/ferritin/storage");
        std::env::set_var("DATABASE_URL", "postgres://ferritin@localhost:5432/ferritin");
    }

    /// Builds a Config without touching process env — validate() tests must
    /// not race the env-based test (cargo runs tests in parallel threads).
    fn test_config(dicom_rules: Vec<String>) -> Config {
        Config {
            dicom_server: DICOMServerConfig {
                facility_name: "example-clinic".to_string(),
                host: "0.0.0.0".to_string(),
                port: 11113,
                ae_title: "SYN_PROXY".to_string(),
            },
            rules: DicomServerRules { dicom_rules },
            hl7_server: HL7ServerConfig {},
            aws: AwsConfig {
                s3_bucket: "ferritin-exams".to_string(),
                sqs_queue_url: "https://example.invalid/queue".to_string(),
            },
            storage: StorageConfig {
                storage_root: PathBuf::from("/tmp/storage"),
                database_url: "postgres://example.invalid/db".to_string(),
            },
        }
    }

    #[test]
    fn load_reads_env_vars() {
        set_test_env();

        let cfg = Config::load().unwrap();

        assert_eq!(cfg.dicom_server.facility_name, "example-clinic");
        assert_eq!(cfg.dicom_server.host, "0.0.0.0");
        assert_eq!(cfg.dicom_server.port, 11113);
        assert_eq!(cfg.dicom_server.ae_title, "SYN_PROXY");
        assert_eq!(cfg.rules.dicom_rules.len(), 2);
        assert!(cfg.rules.dicom_rules[1].starts_with("MG - 1.2.840.10008.5.1.4.1.1.13.1.3"));
        assert_eq!(cfg.aws.s3_bucket, "ferritin-exams");
        assert_eq!(
            cfg.storage.database_url,
            "postgres://ferritin@localhost:5432/ferritin"
        );
    }

    #[test]
    fn valid_rules_pass() {
        let cfg = test_config(vec![
            "MG - 1.2.840.10008.5.1.4.1.1.13.1.3 - PACS@192.168.1.10:104".to_string(),
        ]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn empty_rule_is_rejected() {
        // a trailing comma in DICOM_RULES splits into an empty entry
        let cfg = test_config(vec![
            "MG - 1.2.840.10008.5.1.4.1.1.1.2 - PACS@192.168.1.10:104".to_string(),
            "".to_string(),
        ]);
        assert!(matches!(cfg.validate(), Err(ConfigError::EmptyDicomRule)));
    }
}
