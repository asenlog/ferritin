mod config;
use crate::config::Config;
use ferritin_core::scp;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let cfg = Config::load()?;

    tracing::info!("facility node starting: {}", cfg.dicom_server.facility_name);

    let srv = scp::Server::new(cfg.dicom_server.clone());
    srv.run()?;

    Ok(())
}
