//! Domain models: the types the rest of the crate is built around,
//! one module per aggregate — types only.
//!
//! `auth` — who may push to this node (`AuthorizedCaller`). `rules`
//! — where results are routed (`ForwardingRule`, `Destination`,
//! `resolve`). `mappings` — per-study de-identification records
//! (`StudyMapping`). `modality` — shared vocabulary (`ModalityType`).
//! `filter` — which studies are accepted (`FilterPolicy`, `evaluate`).
//!
//! Ports (the traits over these types) live in `ports`; their
//! implementations live at the edges (`db` repositories, adapters)
//! or in `tests/fixtures`. Nothing in `models` knows SQL or sockets
//! exist — and no database row types live here (those are in `db`).

pub mod auth;
pub mod filter;
pub mod mappings;
pub mod modality;
pub mod rules;
