//! Infrastructure: every adapter that touches the outside world,
//! named for what it is.
//!
//! Driving adapters (the world calls in): `scp` (DICOM associations,
//! with its access-control policy), the SQS listener in
//! `cloud::aws::sqs`. Driven adapters (the application calls out):
//! `scu` (DICOM client), `store` (filesystem object store), `db`
//! (Postgres repositories, one module per table), `cloud` (external
//! systems, one module per system — `aws::s3`, `aws::sqs`).
//!
//! Everything here implements a port from `ports`; nothing in
//! `models`, `service`, or `dicom` imports from `infra`.

pub mod cloud;
pub mod db;
pub mod scp;
pub mod scu;
pub mod store;
