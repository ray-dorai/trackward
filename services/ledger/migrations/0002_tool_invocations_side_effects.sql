-- Phase 4: tool invocation + downstream side-effect records.
--
-- A `tool_invocation` is a first-class ledger row for each tool call the
-- gateway proxies (complementing the tool_call / tool_result events in 0001).
-- A `side_effect` is a downstream confirmation that a tool invocation caused
-- an observable change in an external system (DB write, HTTP POST, file,
-- email send, etc.). Append-only via the deny_mutation trigger from 0001.

CREATE TABLE tool_invocations (
    id           UUID PRIMARY KEY,
    run_id       UUID NOT NULL REFERENCES runs(id),
    tool         TEXT NOT NULL,
    input        JSONB NOT NULL DEFAULT '{}',
    output       JSONB NOT NULL DEFAULT '{}',
    status       TEXT NOT NULL,
    status_code  INT,
    started_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    metadata     JSONB NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_tool_invocations_run_id ON tool_invocations(run_id);
CREATE INDEX idx_tool_invocations_tool ON tool_invocations(tool);

CREATE TRIGGER tool_invocations_no_update
    BEFORE UPDATE OR DELETE ON tool_invocations
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();

CREATE TABLE side_effects (
    id                  UUID PRIMARY KEY,
    run_id              UUID NOT NULL REFERENCES runs(id),
    tool_invocation_id  UUID REFERENCES tool_invocations(id),
    kind                TEXT NOT NULL,
    target              TEXT NOT NULL,
    status              TEXT NOT NULL,
    confirmation        JSONB NOT NULL DEFAULT '{}',
    observed_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_side_effects_run_id ON side_effects(run_id);
CREATE INDEX idx_side_effects_tool_invocation ON side_effects(tool_invocation_id);

CREATE TRIGGER side_effects_no_update
    BEFORE UPDATE OR DELETE ON side_effects
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();
