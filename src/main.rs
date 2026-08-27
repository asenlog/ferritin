use ferritin::app::service::forward::ForwardingService;
use ferritin::config::{Config, StorageBackend};
use ferritin::infra::cloud::aws::s3::S3ObjectStore;
use ferritin::infra::cloud::aws::sqs::SqsResultListener;
use ferritin::infra::db::PgStore;
use ferritin::infra::scp;
use ferritin::infra::scu::ScuClient;
use ferritin::infra::store::FsObjectStore;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let cfg = Config::load()?;

    tracing::info!("facility node starting: {}", cfg.dicom_server.facility_name);

    let db = PgStore::connect(&cfg.storage.database_url)?;
    spawn_results_listener(&cfg, db.clone());

    match cfg.storage.backend {
        StorageBackend::Fs => {
            let store = FsObjectStore::new(cfg.storage.storage_root.clone());
            // one store, four ports: mappings + filter + callers
            scp::Server::new(
                cfg.dicom_server.clone(),
                store,
                db.clone(),
                db.clone(),
                db.clone(),
            )
            .run()?;
        }
        StorageBackend::S3 => {
            let store = S3ObjectStore::connect(&cfg.aws.s3_bucket, "studies")?;
            scp::Server::new(
                cfg.dicom_server.clone(),
                store,
                db.clone(),
                db.clone(),
                db.clone(),
            )
            .run()?;
        }
    }

    Ok(())
}

/// The inbound leg: poll the results queue, re-identify each fetched
/// result, and forward it to its destination AE. Runs on its own
/// thread; if it dies the SCP keeps serving and the error is logged
/// (results stay on the queue for the next process run).
fn spawn_results_listener(cfg: &Config, db: PgStore) {
    let queue_url = cfg.aws.sqs_queue_url.clone();
    let scu = ScuClient::new(cfg.dicom_server.ae_title.clone());
    std::thread::spawn(move || {
        // one store, three ports: mappings + callers + rules
        let forwarding = ForwardingService::new(db.clone(), db, scu);
        let result = SqsResultListener::connect(&queue_url).and_then(|listener| {
            listener.run(move |result| {
                forwarding
                    .forward_result(&result.bytes)
                    .map_err(anyhow::Error::from)
            })
        });
        if let Err(e) = result {
            tracing::error!("results listener stopped: {e:#}");
        }
    });
}
