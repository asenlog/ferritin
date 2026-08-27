//! Pure DICOM logic: protocol command handling and tag-level
//! transforms. No sockets, no PDUs, no ports — functions only.
//!
//! `dimse` — DIMSE command-set parsing and building (PS3.7/PS3.5).
//! `anonymize` — the per-study Replace/Keep de-identification
//! transform and its inverse; the tag *operations* are protocol,
//! the tag *selection* is ferritin's de-id policy.

pub mod anonymize;
pub mod dimse;
