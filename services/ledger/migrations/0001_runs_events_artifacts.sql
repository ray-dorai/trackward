-- Runs: an agent session / invocation
CREATE TABLE runs (
    id          UUID PRIMARY KEY,
    agent       TEXT NOT NULL,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    metadata    JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Prevent mutation: no updates or deletes on runs
CREATE OR REPLACE FUNCTION deny_mutation() RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'mutation denied on append-only table %', TG_TABLE_NAME;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER runs_no_update
    BEFORE UPDATE OR DELETE ON runs FOR EACH ROW EXECUTE FUNCTION deny_mutation();

-- Events: ordered log entries within a run
CREATE TABLE events (
    id          UUID PRIMARY KEY,
    run_id      UUID NOT NULL REFERENCES runs(id),
    seq         BIGINT NOT NULL,
    kind        TEXT NOT NULL,
    body        JSONB NOT NULL DEFAULT '{}',
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (run_id, seq)
);

CREATE INDEX idx_events_run_id ON events(run_id);

CREATE TRIGGER events_no_update
    BEFORE UPDATE OR DELETE ON events FOR EACH ROW EXECUTE FUNCTION deny_mutation();

-- Artifacts: content-addressed blobs
CREATE TABLE artifacts (
    id          UUID PRIMARY KEY,
    run_id      UUID NOT NULL REFERENCES runs(id),
    sha256      TEXT NOT NULL,
    size_bytes  BIGINT NOT NULL,
    media_type  TEXT NOT NULL DEFAULT 'application/octet-stream',
    label       TEXT NOT NULL DEFAULT '',
    metadata    JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_artifacts_run_id ON artifacts(run_id);
CREATE INDEX idx_artifacts_sha256 ON artifacts(sha256);

CREATE TRIGGER artifacts_no_update
    BEFORE UPDATE OR DELETE ON artifacts FOR EACH ROW EXECUTE FUNCTION deny_mutation();
