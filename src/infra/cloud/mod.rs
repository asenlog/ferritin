//! Adapters between ferritin's ports (`ObjectStore`, the results
//! queue, ...) and external systems.
//!
//! Organized by system, one module each: `aws` holds the S3 object
//! store and SQS results listener; a future Azure, Kafka, or
//! vendor-specific system gets its own top-level module here rather
//! than new files in an ever-growing flat list. Adapters implement
//! ports and never leak SDK types into `models` or `service`.

pub mod aws;
