//! Postgres integration tests for the database-backed ports.
//! Self-provisioning: each test starts a throwaway Postgres container
//! (testcontainers) that is stopped and removed on drop. Requires a
//! running Docker daemon — no DATABASE_URL, no local Postgres install.

use ferritin::app::ports::CallerDirectory;
use ferritin::app::ports::MappingStore;
use ferritin::app::ports::RuleDirectory;
use ferritin::app::ports::{FilterDirectory, JobQueue};
use ferritin::infra::db::PgStore;
use testcontainers::runners::SyncRunner;
use testcontainers_modules::postgres::Postgres;

/// A running container plus the stores built on it. Dropped last so
/// the container outlives every client of it.
struct PgRig {
    _container: testcontainers::Container<Postgres>,
    store: PgStore,
    runtime: tokio::runtime::Runtime,
    pool: sqlx::PgPool,
}

fn rig() -> PgRig {
    let container = Postgres::default()
        .start()
        .expect("failed to start postgres container — is Docker running?");
    let port = container.get_host_port_ipv4(5432).unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    let store = PgStore::connect(&url).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pool = runtime.block_on(sqlx::PgPool::connect(&url)).unwrap();
    PgRig {
        _container: container,
        store,
        runtime,
        pool,
    }
}

#[test]
fn pg_mapping_round_trip() {
    let rig = rig();

    // use a throwaway study UID per run so repeats never collide
    let study_uid = format!("1.2.3.test.{}", uuid::Uuid::new_v4());

    let first = rig
        .store
        .mapping_for(&study_uid, "PAT-1", "Doe^John")
        .unwrap();
    assert_eq!(first.patient_id, "PAT-1");
    assert!(first.anon_patient_id.starts_with("ANON-"));
    assert!(first.anon_patient_name.starts_with("ANON^"));

    // second sight of the same study returns the persisted mapping
    let second = rig
        .store
        .mapping_for(&study_uid, "PAT-1", "Doe^John")
        .unwrap();
    assert_eq!(first.anon_patient_id, second.anon_patient_id);
    assert_eq!(first.anon_patient_name, second.anon_patient_name);
    assert_eq!(first.created_at, second.created_at);

    // a different study gets its own pseudonym
    let other_uid = format!("1.2.3.test.{}", uuid::Uuid::new_v4());
    let other = rig
        .store
        .mapping_for(&other_uid, "PAT-1", "Doe^John")
        .unwrap();
    assert_ne!(first.anon_patient_id, other.anon_patient_id);

    // audit columns stay out of the row model but are maintained:
    // an UPDATE bumps updated_at past the insert-time created_at
    let (created_at, updated_at) = rig.runtime.block_on(async {
        sqlx::query(
            "UPDATE study_mappings SET patient_name = 'Doe^Jane' WHERE study_instance_uid = $1",
        )
        .bind(&study_uid)
        .execute(&rig.pool)
        .await
        .unwrap();
        let row = sqlx::query(
            "SELECT created_at, updated_at FROM study_mappings WHERE study_instance_uid = $1",
        )
        .bind(&study_uid)
        .fetch_one(&rig.pool)
        .await
        .unwrap();
        use sqlx::Row;
        (
            row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            row.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
        )
    });
    assert!(
        updated_at > created_at,
        "updated_at {updated_at} should be after created_at {created_at}"
    );
}

#[test]
fn pg_caller_directory_round_trip() {
    let rig = rig();

    // insert directly, as the frontend (or a migration) would — one
    // well-formed row and one malformed one
    let ae_title = format!("TEST-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    rig.runtime.block_on(async {
        sqlx::query("INSERT INTO authorized_callers (ae_title, network) VALUES ($1, $2)")
            .bind(&ae_title)
            .bind("127.0.0.1")
            .execute(&rig.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO authorized_callers (ae_title, network) VALUES ($1, $2)")
            .bind(format!("BAD-{ae_title}"))
            .bind("not-a-network")
            .execute(&rig.pool)
            .await
            .unwrap();
        // a soft-deleted row: present in the table, gone from reads
        sqlx::query(
            "INSERT INTO authorized_callers (ae_title, network, deleted_at) VALUES ($1, $2, now())",
        )
        .bind(format!("GONE-{ae_title}"))
        .bind("127.0.0.1")
        .execute(&rig.pool)
        .await
        .unwrap();
    });

    let callers = rig.store.authorized_callers().unwrap();

    // the well-formed row is served...
    let inserted = callers
        .iter()
        .find(|c| c.ae_title == ae_title)
        .expect("inserted caller must be readable");
    assert_eq!(inserted.network, "127.0.0.1/32".parse().unwrap());

    // ...and the malformed row is skipped instead of failing the load
    assert!(!callers
        .iter()
        .any(|c| c.ae_title == format!("BAD-{ae_title}")));
    // soft-deleted rows are invisible to directory reads
    assert!(!callers
        .iter()
        .any(|c| c.ae_title == format!("GONE-{ae_title}")));
}

#[test]
fn pg_rule_directory_round_trip() {
    let rig = rig();

    // insert directly, as the frontend (or a migration) would
    rig.runtime.block_on(async {
        sqlx::query(
            "INSERT INTO forwarding_rules (modality, sop_class_uid, ae_title, host, port)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind("MG")
        .bind("1.2.840.10008.5.1.4.1.1.13.1.3")
        .bind("PACS")
        .bind("192.168.1.10")
        .bind(104)
        .execute(&rig.pool)
        .await
        .unwrap();
    });

    let rules = rig.store.forwarding_rules().unwrap();

    let rule = rules
        .iter()
        .find(|r| r.sop_class_uid == "1.2.840.10008.5.1.4.1.1.13.1.3")
        .expect("inserted rule must be readable");
    assert_eq!(
        rule.modality,
        ferritin::app::models::modality::ModalityType::MG
    );
    assert_eq!(rule.destination.ae_title, "PACS");
    assert_eq!(rule.destination.host, "192.168.1.10");
    assert_eq!(rule.destination.port, 104);
}

#[test]
fn pg_filter_directory_round_trip() {
    let rig = rig();

    // insert directly, as the frontend (or a migration) would —
    // one rule per kind plus a soft-deleted one
    rig.runtime.block_on(async {
        for (kind, value, deleted) in [
            ("allow_modality", "MG", false),
            ("allow_sop_class", "1.2.840.10008.5.1.4.1.1.13.1.3", false),
            ("block_vendor", "EvilCorp", false),
            ("block_vendor", "GoneCorp", true),
            ("nonsense_kind", "whatever", false),
        ] {
            let mut query = sqlx::query("INSERT INTO filter_rules (kind, value) VALUES ($1, $2)");
            if deleted {
                query = sqlx::query(
                    "INSERT INTO filter_rules (kind, value, deleted_at) VALUES ($1, $2, now())",
                );
            }
            query
                .bind(kind)
                .bind(value)
                .execute(&rig.pool)
                .await
                .unwrap();
        }
    });

    let policy = rig.store.filter_policy().unwrap();

    assert_eq!(
        policy.allow_modalities,
        vec![ferritin::app::models::modality::ModalityType::MG]
    );
    assert_eq!(
        policy.allow_sop_classes,
        vec!["1.2.840.10008.5.1.4.1.1.13.1.3".to_string()]
    );
    // soft-deleted rows are invisible, unknown kinds ignored
    assert_eq!(policy.block_vendors, vec!["EvilCorp".to_string()]);
}

#[test]
fn pg_job_queue_lifecycle() {
    let rig = rig();

    rig.store
        .enqueue(ferritin::app::models::job::NewJob {
            kind: ferritin::app::models::job::JobKind::Upload,
            key: "1.2.3/4.5/6.7.dcm".to_string(),
            payload: b"dicom-bytes".to_vec(),
        })
        .unwrap();

    // claim flips it to running and counts the attempt
    let job = rig
        .store
        .claim(ferritin::app::models::job::JobKind::Upload)
        .unwrap()
        .expect("enqueued job must be claimable");
    assert_eq!(job.attempts, 1);
    assert_eq!(job.key, "1.2.3/4.5/6.7.dcm");
    assert_eq!(job.payload, b"dicom-bytes");

    // already claimed: nothing due
    assert!(rig
        .store
        .claim(ferritin::app::models::job::JobKind::Upload)
        .unwrap()
        .is_none());

    // a crash strands it in running; recovery requeues it
    assert_eq!(
        rig.store
            .recover_running(ferritin::app::models::job::JobKind::Upload)
            .unwrap(),
        1
    );
    let job = rig
        .store
        .claim(ferritin::app::models::job::JobKind::Upload)
        .unwrap()
        .expect("recovered job must be claimable");
    rig.store.complete(job.id).unwrap();
    assert!(rig
        .store
        .claim(ferritin::app::models::job::JobKind::Upload)
        .unwrap()
        .is_none());

    // a failed job goes back to pending with a future backoff...
    rig.store
        .enqueue(ferritin::app::models::job::NewJob {
            kind: ferritin::app::models::job::JobKind::Upload,
            key: "retry-me".to_string(),
            payload: b"x".to_vec(),
        })
        .unwrap();
    let job = rig
        .store
        .claim(ferritin::app::models::job::JobKind::Upload)
        .unwrap()
        .unwrap();
    rig.store.fail(job.id, "boom").unwrap();
    assert!(rig
        .store
        .claim(ferritin::app::models::job::JobKind::Upload)
        .unwrap()
        .is_none());

    // ...and after max attempts it is dead-lettered, never claimable
    rig.runtime.block_on(async {
        sqlx::query(
            "UPDATE jobs SET attempts = max_attempts, next_run_at = now() WHERE key = 'retry-me'",
        )
        .execute(&rig.pool)
        .await
        .unwrap();
    });
    let job = rig
        .store
        .claim(ferritin::app::models::job::JobKind::Upload)
        .unwrap()
        .unwrap();
    rig.store.fail(job.id, "boom again").unwrap();
    let (status,): (String,) = rig.runtime.block_on(async {
        sqlx::query_as("SELECT status FROM jobs WHERE key = 'retry-me'")
            .fetch_one(&rig.pool)
            .await
            .unwrap()
    });
    assert_eq!(status, "dead");
}
