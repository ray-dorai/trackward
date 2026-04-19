-- Phase 9a: per-row hash chain on run-scoped append-only tables.
--
-- Each chained table gains two columns:
--
--   prev_hash BYTEA  -- 32 bytes; NULL for the first row in a chain
--                    -- and for pre-Phase-9a legacy rows
--   row_hash  BYTEA NOT NULL DEFAULT '\x0000...' (32 bytes of zeros)
--
-- Chain grouping is **per (table, run_id)**: every run gets one chain
-- per chained table. For new writes the ledger computes
-- `row_hash = SHA256("trackward-chain-v1\0" || prev_hash || canonical_row_bytes)`
-- under an advisory lock keyed on (table, run_id) so concurrent inserts
-- cannot race on prev_hash selection.
--
-- Pre-Phase-9a rows land with `row_hash = zeros` as a truthful legacy
-- marker — distinguishable from any real SHA-256 output, and mirroring
-- the Phase 8c `'legacy'` actor marker. A verifier scanning a chain
-- treats an all-zero row_hash as "unchained — predates hash-chain" and
-- reports the chain as starting at the first row with a real hash.
--
-- Both columns are CHECKed for length so a malformed hash cannot be
-- inserted via raw SQL (append-only triggers don't prevent first-time
-- inserts from going through out-of-band tooling).

ALTER TABLE events
    ADD COLUMN prev_hash BYTEA,
    ADD COLUMN row_hash  BYTEA NOT NULL DEFAULT decode(repeat('00', 32), 'hex'),
    ADD CONSTRAINT events_prev_hash_length
        CHECK (prev_hash IS NULL OR octet_length(prev_hash) = 32),
    ADD CONSTRAINT events_row_hash_length
        CHECK (octet_length(row_hash) = 32);

ALTER TABLE artifacts
    ADD COLUMN prev_hash BYTEA,
    ADD COLUMN row_hash  BYTEA NOT NULL DEFAULT decode(repeat('00', 32), 'hex'),
    ADD CONSTRAINT artifacts_prev_hash_length
        CHECK (prev_hash IS NULL OR octet_length(prev_hash) = 32),
    ADD CONSTRAINT artifacts_row_hash_length
        CHECK (octet_length(row_hash) = 32);

ALTER TABLE tool_invocations
    ADD COLUMN prev_hash BYTEA,
    ADD COLUMN row_hash  BYTEA NOT NULL DEFAULT decode(repeat('00', 32), 'hex'),
    ADD CONSTRAINT tool_invocations_prev_hash_length
        CHECK (prev_hash IS NULL OR octet_length(prev_hash) = 32),
    ADD CONSTRAINT tool_invocations_row_hash_length
        CHECK (octet_length(row_hash) = 32);

ALTER TABLE side_effects
    ADD COLUMN prev_hash BYTEA,
    ADD COLUMN row_hash  BYTEA NOT NULL DEFAULT decode(repeat('00', 32), 'hex'),
    ADD CONSTRAINT side_effects_prev_hash_length
        CHECK (prev_hash IS NULL OR octet_length(prev_hash) = 32),
    ADD CONSTRAINT side_effects_row_hash_length
        CHECK (octet_length(row_hash) = 32);

ALTER TABLE guardrails
    ADD COLUMN prev_hash BYTEA,
    ADD COLUMN row_hash  BYTEA NOT NULL DEFAULT decode(repeat('00', 32), 'hex'),
    ADD CONSTRAINT guardrails_prev_hash_length
        CHECK (prev_hash IS NULL OR octet_length(prev_hash) = 32),
    ADD CONSTRAINT guardrails_row_hash_length
        CHECK (octet_length(row_hash) = 32);

ALTER TABLE human_approvals
    ADD COLUMN prev_hash BYTEA,
    ADD COLUMN row_hash  BYTEA NOT NULL DEFAULT decode(repeat('00', 32), 'hex'),
    ADD CONSTRAINT human_approvals_prev_hash_length
        CHECK (prev_hash IS NULL OR octet_length(prev_hash) = 32),
    ADD CONSTRAINT human_approvals_row_hash_length
        CHECK (octet_length(row_hash) = 32);

ALTER TABLE bias_slices
    ADD COLUMN prev_hash BYTEA,
    ADD COLUMN row_hash  BYTEA NOT NULL DEFAULT decode(repeat('00', 32), 'hex'),
    ADD CONSTRAINT bias_slices_prev_hash_length
        CHECK (prev_hash IS NULL OR octet_length(prev_hash) = 32),
    ADD CONSTRAINT bias_slices_row_hash_length
        CHECK (octet_length(row_hash) = 32);

-- Index the "last row of chain" lookup the write path makes on every
-- insert. Partial index on row_hash != zeros so the planner skips the
-- legacy-backfill rows entirely.
CREATE INDEX idx_events_chain_tail
    ON events (run_id, created_at DESC, id DESC)
    WHERE row_hash <> decode(repeat('00', 32), 'hex');
CREATE INDEX idx_artifacts_chain_tail
    ON artifacts (run_id, created_at DESC, id DESC)
    WHERE row_hash <> decode(repeat('00', 32), 'hex');
CREATE INDEX idx_tool_invocations_chain_tail
    ON tool_invocations (run_id, created_at DESC, id DESC)
    WHERE row_hash <> decode(repeat('00', 32), 'hex');
CREATE INDEX idx_side_effects_chain_tail
    ON side_effects (run_id, created_at DESC, id DESC)
    WHERE row_hash <> decode(repeat('00', 32), 'hex');
CREATE INDEX idx_guardrails_chain_tail
    ON guardrails (run_id, created_at DESC, id DESC)
    WHERE row_hash <> decode(repeat('00', 32), 'hex');
CREATE INDEX idx_human_approvals_chain_tail
    ON human_approvals (run_id, created_at DESC, id DESC)
    WHERE row_hash <> decode(repeat('00', 32), 'hex');
CREATE INDEX idx_bias_slices_chain_tail
    ON bias_slices (run_id, created_at DESC, id DESC)
    WHERE row_hash <> decode(repeat('00', 32), 'hex');
