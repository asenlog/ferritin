-- Audit columns on every table: created_at (already present),
-- updated_at (trigger-maintained), deleted_at (soft delete).
-- They are deliberately absent from the code's row models — only
-- the deleted_at IS NULL filter in directory reads consumes them.

CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

ALTER TABLE study_mappings
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;

ALTER TABLE authorized_callers
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;

ALTER TABLE forwarding_rules
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;

DROP TRIGGER IF EXISTS study_mappings_set_updated_at ON study_mappings;
CREATE TRIGGER study_mappings_set_updated_at
    BEFORE UPDATE ON study_mappings
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS authorized_callers_set_updated_at ON authorized_callers;
CREATE TRIGGER authorized_callers_set_updated_at
    BEFORE UPDATE ON authorized_callers
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS forwarding_rules_set_updated_at ON forwarding_rules;
CREATE TRIGGER forwarding_rules_set_updated_at
    BEFORE UPDATE ON forwarding_rules
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
