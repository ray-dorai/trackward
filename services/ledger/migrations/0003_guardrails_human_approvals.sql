-- Phase 4: guardrail checks + human approval decisions as first-class rows.
--
-- `guardrails` records the outcome of any policy/safety check the gateway
-- (or a downstream component) performs during a run — rate limits, allowlists,
-- content scans, etc. `human_approvals` captures a completed approval
-- lifecycle: the approval_id minted by the gateway, the tool that was gated,
-- and the final decision + reason. Pending approvals live in memory on the
-- gateway; only completed decisions are persisted here (keeps the table
-- strictly append-only).

CREATE TABLE guardrails (
    id            UUID PRIMARY KEY,
    run_id        UUID NOT NULL REFERENCES runs(id),
    name          TEXT NOT NULL,
    stage         TEXT NOT NULL,
    target        TEXT,
    outcome       TEXT NOT NULL,
    detail        JSONB NOT NULL DEFAULT '{}',
    evaluated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_guardrails_run_id ON guardrails(run_id);
CREATE INDEX idx_guardrails_name ON guardrails(name);

CREATE TRIGGER guardrails_no_update
    BEFORE UPDATE OR DELETE ON guardrails
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();

CREATE TABLE human_approvals (
    id            UUID PRIMARY KEY,
    run_id        UUID NOT NULL REFERENCES runs(id),
    tool          TEXT NOT NULL,
    decision      TEXT NOT NULL,
    reason        TEXT,
    decided_by    TEXT,
    requested_at  TIMESTAMPTZ NOT NULL,
    decided_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    metadata      JSONB NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_human_approvals_run_id ON human_approvals(run_id);
CREATE INDEX idx_human_approvals_tool ON human_approvals(tool);

CREATE TRIGGER human_approvals_no_update
    BEFORE UPDATE OR DELETE ON human_approvals
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();
