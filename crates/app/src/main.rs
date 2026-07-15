mod config;
use crate::config::Config;
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let cfg = Config::load()?;

    tracing::info!("facility node starting: {}", cfg.facility.facility_name);

    Ok(())

}
