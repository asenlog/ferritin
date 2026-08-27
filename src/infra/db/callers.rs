//! Repository for the `authorized_callers` table: the remote nodes
//! allowed to open associations. The port itself lives in `auth`
//! next to the domain type; this is its Postgres implementation.

use super::PgStore;
use crate::app::models::auth::AuthorizedCaller;
use crate::app::ports::CallerDirectory;
use anyhow::Context;

impl CallerDirectory for PgStore {
    fn authorized_callers(&self) -> anyhow::Result<Vec<AuthorizedCaller>> {
        use sqlx::Row;

        self.runtime.block_on(async {
            let rows = sqlx::query(
                "SELECT ae_title, network FROM authorized_callers WHERE deleted_at IS NULL",
            )
            .fetch_all(&self.pool)
            .await
            .context("failed to load authorized callers")?;

            // a malformed row authorizes no one (fail closed on that
            // row) but must not lock out every other caller
            let callers = rows
                .iter()
                .filter_map(|row| {
                    let ae_title: String = row.try_get("ae_title").ok()?;
                    let network: String = row.try_get("network").ok()?;
                    format!("{ae_title}@{network}")
                        .parse::<AuthorizedCaller>()
                        .map_err(|e| {
                            tracing::warn!("ignoring malformed authorized_callers row: {e:#}");
                            e
                        })
                        .ok()
                })
                .collect();
            Ok(callers)
        })
    }
}
