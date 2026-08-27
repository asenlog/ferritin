//! Database layer, organized as one repository module per aggregate,
//! all sharing a single connection handle.
//!
//! `PgStore` is that handle: a pool plus a private tokio runtime,
//! clone-cheap. Each module implements its core port for `PgStore` —
//! `mappings` implements `MappingStore`, `callers` implements
//! `CallerDirectory`, `rules` implements `RuleDirectory` — and owns
//! its row model plus the row ↔ domain conversion, so SQL never
//! leaks into the domain and the domain never learns SQL exists.
//! Ports and domain types live outside `db`, next to their logic
//! (`mappings`, `auth`, `rules`). A new table gets its own module
//! here rather than growing a grab-bag file.
//!
//! These tables hold user-managed domain data — the things a frontend
//! edits — as opposed to deployment config, which lives in env vars.

mod callers;
mod mappings;
mod rules;

pub use mappings::InMemoryMappingStore;

use anyhow::Context;
use std::sync::Arc;

/// Shared Postgres handle for every repository. Owns a private tokio
/// runtime (behind an `Arc`, so clones share it) so the sync ports
/// can sit underneath the (blocking) SCP without forcing async into
/// the association loop.
#[derive(Clone)]
pub struct PgStore {
    pub(crate) pool: sqlx::PgPool,
    pub(crate) runtime: Arc<tokio::runtime::Runtime>,
}

impl PgStore {
    /// Connect and bring the schema up to date. Migrations run under
    /// a Postgres advisory lock, so concurrent first boots serialize
    /// instead of racing.
    pub fn connect(database_url: &str) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime for Postgres store")?;
        let pool = runtime.block_on(async {
            let pool = sqlx::PgPool::connect(database_url)
                .await
                .context("failed to connect to Postgres")?;
            // resolved relative to the crate, so this is the
            // workspace-root `migrations/` directory
            sqlx::migrate!("../../migrations")
                .run(&pool)
                .await
                .context("failed to run migrations")?;
            anyhow::Ok(pool)
        })?;
        Ok(Self {
            pool,
            runtime: Arc::new(runtime),
        })
    }
}
