-- Phase 9b: merkle anchors.
--
-- On a schedule (or on demand in tests), the ledger collects every
-- `row_hash` generated since the previous anchor for a scope, builds a
-- merkle tree, signs the root with its ed25519 key, and ships the
-- signed manifest to an external WORM sink (S3 object-lock in prod, an
-- in-memory recorder in tests). This row is the receipt.
--
-- Scope is either:
--   * a single run — `run_id` set, used for per-run anchoring and test
--     isolation — or
--   * the whole ledger — `run_id IS NULL`, used for global periodic
--     anchors on a production timer.
--
-- The WORM sink is authoritative for "did the anchor escape our
-- database?"; this table is authoritative for "here's what we
-- anchored and where to find it."  A verifier only needs the manifest
-- + the row_hashes (from a dossier) to reproduce the merkle root and
-- validate the signature; it never has to contact the ledger.

CREATE TABLE anchors (
    id              UUID PRIMARY KEY,
    run_id          UUID NULL,
    anchored_from   TIMESTAMPTZ NOT NULL,
    anchored_to     TIMESTAMPTZ NOT NULL,
    leaf_count      BIGINT NOT NULL,
    root_hash       BYTEA NOT NULL,
    signature       BYTEA NOT NULL,
    key_id          TEXT NOT NULL,
    public_key_hex  TEXT NOT NULL,
    anchor_target   TEXT NOT NULL,
    anchored_at     TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT anchors_root_hash_len   CHECK (octet_length(root_hash) = 32),
    CONSTRAINT anchors_signature_len   CHECK (octet_length(signature) = 64),
    CONSTRAINT anchors_window_nonempty CHECK (anchored_to > anchored_from),
    CONSTRAINT anchors_leaf_count_positive CHECK (leaf_count > 0)
);

-- "latest anchor for scope" is the one query the anchor loop makes
-- every tick. Composite index so both global and per-run lookups stay
-- index-only.
CREATE INDEX idx_anchors_scope_to ON anchors (run_id, anchored_to DESC);
