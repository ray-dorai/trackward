-- Phase 8c: principal/actor on every append-only write.
--
-- Each row now carries `actor_id`, populated from the `X-Trackward-Actor`
-- header at write time. This is load-bearing for the Phase 9 hash-chain:
-- the canonical bytes that feed each row's hash will include actor_id, so
-- rewriting the "who did this" field on any row is detectable.
--
-- Legacy rows written before this migration get `'legacy'` as a truthful
-- marker — they predate actor tracking and that fact should be visible
-- rather than hidden behind a more-plausible-looking default.

ALTER TABLE runs                 ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE events               ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE artifacts            ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE tool_invocations     ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE side_effects         ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE guardrails           ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE human_approvals      ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE prompt_versions      ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE policy_versions      ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE eval_results         ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE run_version_bindings ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE bias_slices          ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE custody_events       ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE cases                ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE case_evidence        ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE export_bundles       ADD COLUMN actor_id TEXT NOT NULL DEFAULT 'legacy';
