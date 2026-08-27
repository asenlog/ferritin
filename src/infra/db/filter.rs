//! Repository for the `filter_rules` table: the intake filter
//! policy as kind/value rows. The port lives in `ports`; this is
//! its Postgres implementation.

use super::PgStore;
use crate::app::models::filter::FilterPolicy;
use crate::app::models::modality::ModalityType;
use crate::app::ports::FilterDirectory;
use anyhow::Context;

impl FilterDirectory for PgStore {
    fn filter_policy(&self) -> anyhow::Result<FilterPolicy> {
        use sqlx::Row;

        self.runtime.block_on(async {
            let rows = sqlx::query("SELECT kind, value FROM filter_rules WHERE deleted_at IS NULL")
                .fetch_all(&self.pool)
                .await
                .context("failed to load filter rules")?;

            let mut policy = FilterPolicy::default();
            for row in rows {
                let kind: String = row.try_get("kind")?;
                let value: String = row.try_get("value")?;
                match kind.as_str() {
                    "allow_modality" => {
                        policy.allow_modalities.push(ModalityType::from(value));
                    }
                    "allow_sop_class" => policy.allow_sop_classes.push(value),
                    "block_vendor" => policy.block_vendors.push(value),
                    // unknown kinds route nothing and are surfaced, not fatal
                    other => tracing::warn!("ignoring unknown filter rule kind {other:?}"),
                }
            }
            Ok(policy)
        })
    }
}
