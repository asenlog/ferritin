-- Forwarding rules and destination nodes, moved out of the
-- DICOM_RULES env var into the user-managed tables (frontend-managed).

CREATE TABLE IF NOT EXISTS forwarding_rules (
    modality      TEXT NOT NULL,
    sop_class_uid TEXT NOT NULL,
    ae_title      TEXT NOT NULL,
    host          TEXT NOT NULL,
    port          INTEGER NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (modality, sop_class_uid)
);
