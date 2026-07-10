# dcm-hl7-server

A from-scratch Rust implementation of DICOM (and eventually HL7 v2)
parsing and networking, built for learning — not a production system, and
not a replacement for Orthanc. See `ROADMAP.md` for the full milestone
plan and the reasoning behind each one.

**Safety note:** this is a standalone project, deliberately decoupled from
any production system. Only ever run it against synthetic fixtures or
properly anonymized test files — never real patient data.

## Structure

```
dcm-hl7-server/
├── Cargo.toml              workspace root
├── ROADMAP.md
├── fixtures/                synthetic test files
├── scripts/
│   └── make_fixture.py      builds fixtures/synthetic_mg.dcm by hand
└── crates/
    ├── dcm-core/             parsing library (Tag, VR, Reader, parser)
    │   └── src/
    │       ├── types.rs      Tag + Vr
    │       ├── reader.rs     bounds-checked byte cursor
    │       ├── element.rs    Element + Value + decoding
    │       ├── dictionary.rs tag→VR lookup for Implicit VR
    │       ├── parser.rs     file meta + dataset parsing
    │       ├── error.rs
    │       └── lib.rs
    └── dcm-cli/              `dcm-dump` binary
        └── src/main.rs
```

## Quick start

```bash
# generate the synthetic fixture (needs python3, no deps)
python3 scripts/make_fixture.py

# build + run
cargo run --bin dcm-dump -- fixtures/synthetic_mg.dcm

# against a real file
cargo run --bin dcm-dump -- /path/to/some/study.dcm
```

## What M1 currently supports

- File meta group parsing (always Explicit VR Little Endian per PS3.10 §7.1)
- Dataset parsing in **Implicit VR Little Endian** (`1.2.840.10008.1.2`)
  and **Explicit VR Little Endian** (`1.2.840.10008.1.2.1`)
- Correct short-form (2-byte length) vs. long-form (2-byte reserved +
  4-byte length) handling per PS3.5 Table 7.1-1
- A small hand-maintained tag→VR dictionary for Implicit VR decoding
  (extend `dictionary.rs` as you hit `UN` for tags you recognize)

## What it doesn't support yet (see ROADMAP.md)

- Sequences (SQ) and anything else using undefined-length / Item framing
  — the parser stops cleanly and tells you where, rather than misparsing
- Compressed/encapsulated transfer syntaxes (JPEG, JPEG2000, RLE, ...)
- Any networking (that's M4+ — the DICOM upper layer protocol, hand-rolled)
- HL7 (M6+)

## Useful references for the next milestones

- DICOM PS3.5 — Data Structures and Encoding
- DICOM PS3.7 — Message Exchange
- DICOM PS3.8 — Network Communication Support for Message Exchange
- DICOM PS3.4 Annex B — Verification (C-ECHO) and Storage (C-STORE)
  Service Classes
