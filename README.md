# ferritin

A DICOM & HL7 server written in Rust — receive, filter, de-identify,
store, and route medical imaging studies between modalities, PACS, and
cloud backends.

**Safety note:** development and testing use synthetic fixtures only.
Never point this at real patient data.

## What it does

- **DICOM SCP intake** — C-ECHO / C-STORE with AE-title + source-IP
  authorization of calling nodes
- **Rule-based filtering** — modality and SOP-class allowlists, vendor
  blocklists, fail-open on missing metadata
- **De-identification** — reversible per-study pseudonym mapping backed
  by PostgreSQL, Replace/Keep tag transform (broader PS3.15 Annex E
  profile coverage lands with the hardening phase)
- **Pluggable backends** — object-store and result-queue ports with
  S3 and SQS adapters included; swap in your own without touching core
- **Re-identification & forwarding** — processed results are matched to
  their original study and C-STOREd back to the configured destination AE
- **Durable job queues** — outbound and inbound work is persisted and
  retried; a crash resumes instead of losing studies
- **HL7 v2 over MLLP** — planned (see `ROADMAP.md`)

## Structure

```
ferritin/
├── Cargo.toml     the `ferritin` binary package + workspace root
├── src/           the binary — config loading + wiring
├── ROADMAP.md
├── crates/        libraries only
│   ├── ferritin-core/    domain library — auth, dimse, intake, anonymize, db, scp
│   └── ferritin-cloud/   AWS adapters — S3 object store, SQS results queue
└── fixtures/      test DICOM files (not yet added)
```

## Configuration

Config comes from the environment: a `.env` file at the repo root for
local runs (found by walking up from the working directory; real
environment variables always win), or plain env vars in production
(e.g. systemd `EnvironmentFile=`). See `src/config.rs` for
the full list of required keys.

The boundary between env and database is deliberate:

- **Env — deployment config.** This node's own identity and
  infrastructure, set once per deployment: `FACILITY_NAME`,
  `LISTEN_HOST` / `LISTEN_PORT` / `LISTEN_AE_TITLE`, `S3_BUCKET`,
  `SQS_QUEUE_URL`, `STORAGE_ROOT`, `DATABASE_URL`, `DICOM_RULES`.
- **Database — user-managed domain data.** Everything a frontend
  administers at runtime. Today: the `authorized_callers` table
  (remote nodes allowed to push, as AE-title + CIDR rows) and the
  `study_mappings` de-identification table. The SCP reads
  `authorized_callers` fresh per association, so edits take effect
  without a restart. Forwarding rules (`DICOM_RULES`) and destination
  nodes move here when the filtering/forwarding work lands.

Migrations live in `migrations/` at the workspace root (sqlx,
embedded in the binary at compile time) and run automatically at
startup under a Postgres advisory lock, so concurrent first boots are
safe.

## Useful references

- DICOM PS3.5 — Data Structures and Encoding
- DICOM PS3.7 — Message Exchange
- DICOM PS3.8 — Network Communication Support for Message Exchange
- DICOM PS3.4 Annex B — Verification (C-ECHO) and Storage (C-STORE)
  Service Classes
- DICOM PS3.15 Annex E — De-identification profiles