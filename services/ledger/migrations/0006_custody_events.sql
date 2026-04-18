-- Phase 5: chain of custody.
--
-- Every read, export, link, or annotation performed against evidence leaves
-- a row here — so that after an export, a haruspex or auditor can see who
-- touched which evidence, when, and why. Append-only. The table stores
-- (evidence_type, evidence_id) as free-form strings / UUIDs instead of a
-- hard FK so that custody can cover any table (including cases, export
-- bundles, and future artefacts).

CREATE TABLE custody_events (
    id             UUID PRIMARY KEY,
    evidence_type  TEXT NOT NULL,
    evidence_id    UUID NOT NULL,
    action         TEXT NOT NULL,
    actor          TEXT NOT NULL,
    reason         TEXT,
    occurred_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    metadata       JSONB NOT NULL DEFAULT '{}',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_custody_events_evidence
    ON custody_events (evidence_type, evidence_id);
CREATE INDEX idx_custody_events_actor ON custody_events (actor);
CREATE INDEX idx_custody_events_occurred_at ON custody_events (occurred_at);

CREATE TRIGGER custody_events_no_update
    BEFORE UPDATE OR DELETE ON custody_events
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();
