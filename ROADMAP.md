# Roadmap

A standalone learning project — fully decoupled from Synapsis's production
Orthanc deployment. Never point this at real patient data from the
diagnostic centers; use only synthetic fixtures or anonymized test files.

## M1 — DICOM tag dumper (done, this scaffold)
- [x] Hand-rolled byte reader over `&[u8]`
- [x] Explicit VR LE parsing (file meta group), incl. long-form vs
      short-form length encoding
- [x] Implicit VR LE parsing (dataset), incl. minimal tag→VR dictionary
- [x] `dcm-dump` CLI
- [x] Synthetic fixture generator + end-to-end test

**Try it on a real file next:** point `dcm-dump` at an actual exported
study (e.g. investigate the Series 9000 Sentinel OT object — check what
SOP Class UID, Modality, Presentation Intent Type, and Photometric
Interpretation it actually prints vs. what you expect).

## M2 — Sequences & undefined-length elements
Right now the parser deliberately stops when it hits an undefined-length
element (`0xFFFFFFFF`) — that covers Sequences (SQ) and encapsulated
(compressed) Pixel Data, both of which use Item (FFFE,E000) /
Sequence Delimiter (FFFE,E0DD) framing instead of a plain length prefix.
This is required before you can fully parse presentation state objects
(which are sequence-heavy) or anything JPEG-compressed.
- [ ] Parse Item headers within a Sequence
- [ ] Handle nested/recursive datasets within Sequence Items
- [ ] Handle encapsulated Pixel Data fragments (Basic Offset Table + frames)

## M3 — Validator layer
Turn the dumper into a pre-ingestion gate:
- [ ] Flag Photometric Interpretation / ImageType mismatches (the
      DERIVED/SECONDARY pattern seen on the Sakarellos Hologic unit)
- [ ] Flag missing StudyInstanceUID / SeriesInstanceUID before it becomes
      a routing failure downstream
- [ ] Structured validation report (not just a dump)

## M4 — DICOM upper layer protocol (PS3.8)
The actual networking milestone. Build the association state machine by
hand — this is where "learning protocols" really lives:
- [ ] TCP listener, PDU framing (A-ASSOCIATE-RQ, -AC, -RJ; P-DATA-TF)
- [ ] Presentation context negotiation (propose/accept transfer syntaxes)
- [ ] C-ECHO SCP (the "hello world" of DICOM networking — verify with a
      real `echoscu` from DCMTK or `pynetdicom`)
- [ ] C-STORE SCP (receive and persist a file)

Reference: DICOM PS3.7 (message exchange), PS3.8 (network communication
support), PS3.4 Annex B (C-STORE service class).

## M5 — SCU roles
- [ ] C-STORE SCU (push a synthetic study to a test SCP — this is the
      "fake modality" half of the site-integration simulator)
- [ ] C-FIND SCP (basic worklist query support)

## M6 — HL7 v2 over MLLP
- [ ] MLLP framing (VT/FS/CR) — `hl7-mllp` crate or hand-rolled
- [ ] Parse ADT^A01/A08 and ORM^O01 (`hl7-parser` crate as a starting
      point; consider hand-rolling the pipe-and-hat tokenizer for the
      learning value, same as the DICOM parser)
- [ ] ACK/NACK generation

## M7 — Correlation (the actual point of the simulator)
- [ ] Fire a consistent patient ID across both an HL7 ADT message and a
      C-STORE'd DICOM study, so a new site's matching/worklist logic can
      be regression-tested against known-good and known-bad scenarios
      before going live against production Orthanc.

## M8 — Stretch: fuzzing
- [ ] `cargo-fuzz` target against the M1/M2 parser with malformed and
      adversarial DICOM files. Genuinely relevant to the Application
      Cybersecurity Plan (FPS05-00-01) — "no memory corruption is
      possible" is a provable property in Rust in a way it isn't in a
      C/C++ parser.
