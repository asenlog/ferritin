mod config;
use crate::config::{Config, StorageBackend};
use ferritin_cloud::s3::S3ObjectStore;
use ferritin_core::{db::PgStore, scp, store::FsObjectStore};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let cfg = Config::load()?;

    tracing::info!("facility node starting: {}", cfg.dicom_server.facility_name);

    let db = PgStore::connect(&cfg.storage.database_url)?;
    match cfg.storage.backend {
        StorageBackend::Fs => {
            let store = FsObjectStore::new(cfg.storage.storage_root.clone());
            scp::Server::new(cfg.dicom_server.clone(), store, db.clone(), db).run()?;
        }
        StorageBackend::S3 => {
            let store = S3ObjectStore::connect(&cfg.aws.s3_bucket, "studies")?;
            scp::Server::new(cfg.dicom_server.clone(), store, db.clone(), db).run()?;
        }
    }

    Ok(())
}
