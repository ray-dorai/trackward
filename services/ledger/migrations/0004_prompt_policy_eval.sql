-- Phase 3: registry index tables.
--
-- `registry/` on disk is the source of truth — prompts, policies, evals live
-- as files in git. These tables are a signed-off index so every `run` can
-- carry references to the *exact* (git_sha, content_hash) that produced it.
-- The deny_mutation trigger from 0001 keeps everything append-only.

CREATE TABLE prompt_versions (
    id            UUID PRIMARY KEY,
    workflow      TEXT NOT NULL,
    version       TEXT NOT NULL,
    git_sha       TEXT NOT NULL,
    content_hash  TEXT NOT NULL,
    metadata      JSONB NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (workflow, version, content_hash)
);

CREATE INDEX idx_prompt_versions_workflow_version
    ON prompt_versions (workflow, version);

CREATE TRIGGER prompt_versions_no_update
    BEFORE UPDATE OR DELETE ON prompt_versions
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();

CREATE TABLE policy_versions (
    id            UUID PRIMARY KEY,
    scope         TEXT NOT NULL,
    version       TEXT NOT NULL,
    git_sha       TEXT NOT NULL,
    content_hash  TEXT NOT NULL,
    metadata      JSONB NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (scope, version, content_hash)
);

CREATE INDEX idx_policy_versions_scope_version
    ON policy_versions (scope, version);

CREATE TRIGGER policy_versions_no_update
    BEFORE UPDATE OR DELETE ON policy_versions
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();

CREATE TABLE eval_results (
    id                 UUID PRIMARY KEY,
    workflow           TEXT NOT NULL,
    version            TEXT NOT NULL,
    prompt_version_id  UUID REFERENCES prompt_versions(id),
    git_sha            TEXT NOT NULL,
    content_hash       TEXT NOT NULL,
    passed             BOOLEAN NOT NULL,
    summary            JSONB NOT NULL DEFAULT '{}',
    ran_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_eval_results_workflow_version
    ON eval_results (workflow, version);
CREATE INDEX idx_eval_results_prompt_version
    ON eval_results (prompt_version_id);

CREATE TRIGGER eval_results_no_update
    BEFORE UPDATE OR DELETE ON eval_results
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();

-- A run is bound to at most one (prompt, policy, eval) triple. Each field is
-- nullable so partial bindings are allowed — e.g. a run created before eval
-- results exist just points at prompt + policy.
CREATE TABLE run_version_bindings (
    run_id             UUID PRIMARY KEY REFERENCES runs(id),
    prompt_version_id  UUID REFERENCES prompt_versions(id),
    policy_version_id  UUID REFERENCES policy_versions(id),
    eval_result_id     UUID REFERENCES eval_results(id),
    bound_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_run_version_bindings_prompt
    ON run_version_bindings (prompt_version_id);
CREATE INDEX idx_run_version_bindings_policy
    ON run_version_bindings (policy_version_id);

CREATE TRIGGER run_version_bindings_no_update
    BEFORE UPDATE OR DELETE ON run_version_bindings
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();
