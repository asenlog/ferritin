# Roadmap

A production-grade DICOM/HL7 server built on `dicom-rs` rather than
hand-rolled, since it is designed to eventually handle real clinical
traffic.

Development and testing use synthetic fixtures only until the hardening
phase (P4) — never point this at real patient data before then.

# Production track — `ferritin`

Decomposed into ordered sub-projects.

## P1 — DICOM core round-trip pipeline

SCP intake (AET + source-IP authorized modalities) → Modality/SOP-Class
filter → per-study de-identification (SQLite mapping) → object-store
upload → result-queue listener → fetch → re-identification → SCU
forward-back to the resolved per-source destination AE. Persistent,
auto-retrying job queues for both the outbound (upload) and inbound
(forward-back) legs.

- [x] Three-crate scaffold: `dicom` (product + `ObjectStore`/`ResultQueue`
      ports), `cloud` (S3/SQS adapters), `app` (binary wiring)
- [x] `config`: env-based config loading, `.env` support (app)
- [ ] `scp`: association accept + AET/IP authorization (dicom)
- [x] `filter`: Modality/SOP-Class allowlist + vendor blocklist (dicom)
- [ ] `anonymize` + `db`: per-study mapping, Replace/Keep tag transform (dicom)
- [ ] `s3`: content-hash + upload with deterministic key convention (cloud)
- [ ] `sqs`: results-queue listener (S3-event message format) (cloud)
- [ ] `deanonymize` + `scu`: re-identify, resolve destination, forward (dicom)
- [ ] Outbound/inbound persistent retry-queue workers
- [ ] Interop test against DCMTK `storescu`/`storescp`, synthetic
      fixtures only

## P2 — Worklist / C-FIND SCP
Scheduled-procedure queries so modalities can pull worklists directly
from `ferritin`. Spec TBD.

## P3 — HL7 v2 / MLLP ingestion
ADT/ORM intake, patient correlation. Spec TBD.

## P4 — Production hardening
Shadow-mode validation against real traffic volumes, monitoring,
rollback plan — before any live deployment. Spec TBD.