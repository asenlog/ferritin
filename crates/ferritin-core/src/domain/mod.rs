//! Domain layer: the types and ports the rest of the crate is built
//! around, one module per aggregate.
//!
//! `auth` — who may push to this node (`AuthorizedCaller`,
//! `CallerDirectory`). `rules` — where results are routed
//! (`ForwardingRule`, `Destination`, `RuleDirectory`). `mappings` —
//! per-study de-identification records (`StudyMapping`,
//! `MappingStore`). `models` — shared vocabulary (`ModalityType`).
//!
//! Implementations of the ports live elsewhere (`db` repositories,
//! in-memory test adapters); nothing in `domain` knows SQL or
//! sockets exist.

pub mod auth;
pub mod mappings;
pub mod models;
pub mod rules;
