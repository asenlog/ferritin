use ferritin::app::models::job::{JobKind, NewJob};
use ferritin::app::ports::{JobQueue, ObjectStore};
use ferritin::app::service::forward::ForwardingService;
use ferritin::app::service::worker::QueueWorker;
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

    let store: Box<dyn ObjectStore + Send> = match cfg.storage.backend {
        StorageBackend::Fs => Box::new(FsObjectStore::new(cfg.storage.storage_root.clone())),
        StorageBackend::S3 => Box::new(S3ObjectStore::connect(&cfg.aws.s3_bucket, "studies")?),
    };

    // one store, four ports: mappings + filter + callers + jobs
    spawn_upload_worker(db.clone(), store);
    spawn_forward_worker(&cfg, db.clone());
    spawn_results_ingest(&cfg, db.clone());

    scp::Server::new(
        cfg.dicom_server.clone(),
        db.clone(),
        db.clone(),
        db.clone(),
        db,
    )
    .run()?;

    Ok(())
}

/// The outbound leg: drain the persistent upload queue into the
/// object store, retrying with backoff on failure.
fn spawn_upload_worker(queue: PgStore, store: Box<dyn ObjectStore + Send>) {
    std::thread::spawn(move || {
        QueueWorker::new(queue, JobKind::Upload).run(move |job| store.put(&job.key, &job.payload));
    });
}

/// The inbound forward leg: drain the persistent forward queue —
/// re-identify each result and C-STORE it to its destination AE.
fn spawn_forward_worker(cfg: &Config, queue: PgStore) {
    let scu = ScuClient::new(cfg.dicom_server.ae_title.clone());
    std::thread::spawn(move || {
        let forwarding = ForwardingService::new(queue.clone(), queue.clone(), scu);
        QueueWorker::new(queue, JobKind::Forward).run(move |job| {
            forwarding
                .forward_result(&job.payload)
                .map_err(anyhow::Error::from)
        });
    });
}

/// Results ingest: poll the queue and persist each fetched result as
/// a forward job. Deleting the SQS message is then safe — the result
/// is durably queued locally, no longer dependent on SQS redrive.
fn spawn_results_ingest(cfg: &Config, queue: PgStore) {
    let queue_url = cfg.aws.sqs_queue_url.clone();
    std::thread::spawn(move || {
        let result = SqsResultListener::connect(&queue_url).and_then(|listener| {
            listener.run(move |result| {
                queue.enqueue(NewJob {
                    kind: JobKind::Forward,
                    key: format!("{}/{}", result.bucket, result.key),
                    payload: result.bytes,
                })
            })
        });
        if let Err(e) = result {
            tracing::error!("results listener stopped: {e:#}");
        }
    });
}
