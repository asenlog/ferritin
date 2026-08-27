//! Postgres integration tests for the database-backed ports.
//! Self-provisioning: each test starts a throwaway Postgres container
//! (testcontainers) that is stopped and removed on drop. Requires a
//! running Docker daemon — no DATABASE_URL, no local Postgres install.

use ferritin_core::auth::CallerDirectory;
use ferritin_core::db::{MappingStore, PgStore};
use testcontainers::runners::SyncRunner;
use testcontainers_modules::postgres::Postgres;

fn rig() -> (
    testcontainers::Container<Postgres>,
    PgStore,
    tokio::runtime::Runtime,
    sqlx::PgPool,
) {
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
    (container, store, runtime, pool)
}

#[test]
fn pg_mapping_round_trip() {
    let (_container, store, _runtime, _pool) = rig();

    // use a throwaway study UID per run so repeats never collide
    let study_uid = format!("1.2.3.test.{}", uuid::Uuid::new_v4());

    let first = store.mapping_for(&study_uid, "PAT-1", "Doe^John").unwrap();
    assert_eq!(first.patient_id, "PAT-1");
    assert!(first.anon_patient_id.starts_with("ANON-"));
    assert!(first.anon_patient_name.starts_with("ANON^"));

    // second sight of the same study returns the persisted mapping
    let second = store.mapping_for(&study_uid, "PAT-1", "Doe^John").unwrap();
    assert_eq!(first.anon_patient_id, second.anon_patient_id);
    assert_eq!(first.anon_patient_name, second.anon_patient_name);
    assert_eq!(first.created_at, second.created_at);

    // a different study gets its own pseudonym
    let other_uid = format!("1.2.3.test.{}", uuid::Uuid::new_v4());
    let other = store.mapping_for(&other_uid, "PAT-1", "Doe^John").unwrap();
    assert_ne!(first.anon_patient_id, other.anon_patient_id);
}

#[test]
fn pg_caller_directory_round_trip() {
    let (_container, store, runtime, pool) = rig();

    // insert directly, as the frontend (or a migration) would — one
    // well-formed row and one malformed one
    let ae_title = format!("TEST-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    runtime.block_on(async {
        sqlx::query("INSERT INTO authorized_callers (ae_title, network) VALUES ($1, $2)")
            .bind(&ae_title)
            .bind("127.0.0.1")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO authorized_callers (ae_title, network) VALUES ($1, $2)")
            .bind(format!("BAD-{ae_title}"))
            .bind("not-a-network")
            .execute(&pool)
            .await
            .unwrap();
    });

    let callers = store.authorized_callers().unwrap();

    // the well-formed row is served...
    let inserted = callers
        .iter()
        .find(|c| c.ae_title == ae_title)
        .expect("inserted caller must be readable");
    assert_eq!(inserted.network, "127.0.0.1/32".parse().unwrap());

    // ...and the malformed row is skipped instead of failing the load
    assert!(!callers.iter().any(|c| c.ae_title == format!("BAD-{ae_title}")));
}
