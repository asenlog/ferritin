DROP TRIGGER IF EXISTS study_mappings_set_updated_at ON study_mappings;
DROP TRIGGER IF EXISTS authorized_callers_set_updated_at ON authorized_callers;
DROP TRIGGER IF EXISTS forwarding_rules_set_updated_at ON forwarding_rules;
DROP FUNCTION IF EXISTS set_updated_at();

ALTER TABLE study_mappings
    DROP COLUMN IF EXISTS updated_at,
    DROP COLUMN IF EXISTS deleted_at;

ALTER TABLE authorized_callers
    DROP COLUMN IF EXISTS updated_at,
    DROP COLUMN IF EXISTS deleted_at;

ALTER TABLE forwarding_rules
    DROP COLUMN IF EXISTS updated_at,
    DROP COLUMN IF EXISTS deleted_at;
