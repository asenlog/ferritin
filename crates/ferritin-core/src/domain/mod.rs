//! Domain layer: the types the rest of the crate is built around,
//! one module per aggregate — types only.
//!
//! `auth` — who may push to this node (`AuthorizedCaller`). `rules`
//! — where results are routed (`ForwardingRule`, `Destination`,
//! `resolve`). `mappings` — per-study de-identification records
//! (`StudyMapping`). `models` — shared vocabulary (`ModalityType`).
//!
//! Ports (the traits over these types) live in `ports`; their
//! implementations live at the edges (`db` repositories, adapters)
//! or in `fixtures`. Nothing in `domain` knows SQL or sockets exist.

pub mod auth;
pub mod mappings;
pub mod models;
pub mod rules;
