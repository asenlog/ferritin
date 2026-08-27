//! Service layer: the business-logic orchestrators, composing domain
//! ports only — no concrete infrastructure types.
//!
//! `intake` — parse received instances, filter, de-identify, persist.
//! `anonymize` — the Replace/Keep transform and its inverse.
//! `forward` — re-identify a fetched result and route it home.
//!
//! Each service is unit-testable against in-memory port adapters;
//! the DICOM, database, and object-store adapters at the edges never
//! leak in here.

pub mod anonymize;
pub mod forward;
pub mod intake;
