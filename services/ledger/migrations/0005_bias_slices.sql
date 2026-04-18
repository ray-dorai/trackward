-- Phase 4: bias-slice tags attached to runs and/or eval results.
--
-- A bias_slice is a label marking a run or eval result as belonging to a
-- particular demographic / dataset slice — used to track fairness across
-- cohorts. Either `run_id` or `eval_result_id` must be set; many slices
-- per row is modelled by inserting multiple rows.

CREATE TABLE bias_slices (
    id              UUID PRIMARY KEY,
    run_id          UUID REFERENCES runs(id),
    eval_result_id  UUID REFERENCES eval_results(id),
    label           TEXT NOT NULL,
    value           TEXT,
    score           DOUBLE PRECISION,
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    CHECK (run_id IS NOT NULL OR eval_result_id IS NOT NULL)
);

CREATE INDEX idx_bias_slices_run_id ON bias_slices(run_id);
CREATE INDEX idx_bias_slices_eval_result ON bias_slices(eval_result_id);
CREATE INDEX idx_bias_slices_label ON bias_slices(label);

CREATE TRIGGER bias_slices_no_update
    BEFORE UPDATE OR DELETE ON bias_slices
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();
