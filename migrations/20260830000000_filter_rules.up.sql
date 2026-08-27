-- Intake filter policy: kind/value rules the frontend manages.
-- kind ∈ ('allow_modality', 'allow_sop_class', 'block_vendor').

CREATE TABLE IF NOT EXISTS filter_rules (
    kind       TEXT NOT NULL,
    value      TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    PRIMARY KEY (kind, value)
);

DROP TRIGGER IF EXISTS filter_rules_set_updated_at ON filter_rules;
CREATE TRIGGER filter_rules_set_updated_at
    BEFORE UPDATE ON filter_rules
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
