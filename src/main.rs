mod config;
use crate::config::Config;
use ferritin_core::{db::PgStore, scp, store::FsObjectStore};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let cfg = Config::load()?;

    tracing::info!("facility node starting: {}", cfg.dicom_server.facility_name);

    let db = PgStore::connect(&cfg.storage.database_url)?;
    let store = FsObjectStore::new(cfg.storage.storage_root.clone());
    let srv = scp::Server::new(cfg.dicom_server.clone(), store, db.clone(), db);
    srv.run()?;

    Ok(())
}
