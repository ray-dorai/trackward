-- Phase 5: investigation cases + the evidence linked to them.
--
-- A `case` is a haruspex-opened wrapper around a set of evidence rows pulled
-- from across the ledger. `case_evidence` is the join table — free-form
-- (evidence_type, evidence_id) so any ledger row can be linked. Both tables
-- are append-only; closing a case or unlinking evidence is modelled as a
-- new custody_event rather than a mutation.

CREATE TABLE cases (
    id           UUID PRIMARY KEY,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    opened_by    TEXT NOT NULL,
    opened_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    metadata     JSONB NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_cases_opened_by ON cases (opened_by);

CREATE TRIGGER cases_no_update
    BEFORE UPDATE OR DELETE ON cases
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();

CREATE TABLE case_evidence (
    case_id        UUID NOT NULL REFERENCES cases(id),
    evidence_type  TEXT NOT NULL,
    evidence_id    UUID NOT NULL,
    linked_by      TEXT NOT NULL,
    linked_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    note           TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (case_id, evidence_type, evidence_id)
);

CREATE INDEX idx_case_evidence_case ON case_evidence (case_id);

CREATE TRIGGER case_evidence_no_update
    BEFORE UPDATE OR DELETE ON case_evidence
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();
