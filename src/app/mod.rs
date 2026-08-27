//! The application core — synapse's `internal/app` analog.
//!
//! `models` — domain types (no traits, no impls). `ports` — every
//! port trait. `service` — DI orchestrators composing ports.
//! `dicom` — pure DICOM logic, functions only.
//!
//! Nothing in `app` touches SQL, sockets, files, or cloud SDKs;
//! that is what `infra` is for. Dependency direction is one-way:
//! `service` → `ports` → `models` ← `infra`.

pub mod dicom;
pub mod models;
pub mod ports;
pub mod service;
