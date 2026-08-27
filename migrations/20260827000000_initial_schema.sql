-- Initial schema.
--
-- IF NOT EXISTS throughout so databases created before migrations
-- were introduced adopt this revision cleanly.

CREATE TABLE IF NOT EXISTS study_mappings (
    study_instance_uid TEXT PRIMARY KEY,
    patient_id         TEXT NOT NULL,
    patient_name       TEXT NOT NULL,
    anon_patient_id    TEXT NOT NULL,
    anon_patient_name  TEXT NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS authorized_callers (
    ae_title   TEXT NOT NULL,
    network    TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (ae_title, network)
);
