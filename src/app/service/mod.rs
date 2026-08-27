//! Service layer: the business-logic orchestrators, composing domain
//! ports only — no concrete infrastructure types. A service here is
//! always a struct holding its collaborators (DI); pure DICOM logic
//! lives in `dicom`, not here.
//!
//! `intake` — parse received instances, filter, de-identify, persist.
//! `forward` — re-identify a fetched result and route it home.
//! `worker` — retrying queue consumers behind the two legs.
//!
//! Each service is unit-testable against in-memory port adapters;
//! the DICOM, database, and object-store adapters at the edges never
//! leak in here.

pub mod forward;
pub mod intake;
pub mod worker;
