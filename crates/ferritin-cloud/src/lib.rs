//! Adapters between ferritin's core ports (`ObjectStore`, the results
//! queue, ...) and external systems.
//!
//! The crate is organized by system, one module each: `aws` holds the
//! S3 object store and SQS results listener; a future Azure, Kafka, or
//! vendor-specific system gets its own top-level module here rather
//! than new files in an ever-growing flat list. Adapters implement
//! core ports and never leak SDK types into `ferritin-core`.

pub mod aws;
