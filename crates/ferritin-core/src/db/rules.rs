//! Repository for the `forwarding_rules` table: routing from
//! modality + SOP class to destination nodes. The port itself lives
//! in `rules` next to the domain type; this is its Postgres
//! implementation.

use super::PgStore;
use crate::domain::models::ModalityType;
use crate::domain::rules::{Destination, ForwardingRule};
use crate::ports::RuleDirectory;
use anyhow::Context;

impl RuleDirectory for PgStore {
    fn forwarding_rules(&self) -> anyhow::Result<Vec<ForwardingRule>> {
        use sqlx::Row;

        self.runtime.block_on(async {
            let rows = sqlx::query(
                "SELECT modality, sop_class_uid, ae_title, host, port FROM forwarding_rules WHERE deleted_at IS NULL",
            )
            .fetch_all(&self.pool)
            .await
            .context("failed to load forwarding rules")?;

            // a malformed row routes nothing (its studies get NoRoute)
            // but must not break the rest of the table
            let rules = rows
                .iter()
                .filter_map(|row| {
                    let build = || -> anyhow::Result<ForwardingRule> {
                        let port: i32 = row.try_get("port")?;
                        Ok(ForwardingRule {
                            modality: ModalityType::from(row.try_get::<String, _>("modality")?),
                            sop_class_uid: row.try_get("sop_class_uid")?,
                            destination: Destination {
                                ae_title: row.try_get("ae_title")?,
                                host: row.try_get("host")?,
                                port: u16::try_from(port)
                                    .with_context(|| format!("port {port} out of range"))?,
                            },
                        })
                    };
                    build()
                        .map_err(|e| {
                            tracing::warn!("ignoring malformed forwarding_rules row: {e:#}");
                            e
                        })
                        .ok()
                })
                .collect();
            Ok(rules)
        })
    }
}
