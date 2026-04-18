-- Phase 5: signed export bundles.
--
-- When a haruspex exports a case, the ledger builds a canonical manifest of
-- the linked evidence, signs it with the service's ed25519 key, and records
-- the signed bundle here. The bundle row is the authoritative record of
-- what was exported, to whom, and with which key. The bundle payload
-- returned to the caller is self-contained — it embeds the public key so an
-- offline verifier can check it without talking to the ledger.

CREATE TABLE export_bundles (
    id                UUID PRIMARY KEY,
    case_id           UUID NOT NULL REFERENCES cases(id),
    manifest_json     TEXT NOT NULL,
    manifest_sha256   TEXT NOT NULL,
    signature         TEXT NOT NULL,
    key_id            TEXT NOT NULL,
    public_key_hex    TEXT NOT NULL,
    signed_by         TEXT NOT NULL,
    signed_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    storage_uri       TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_export_bundles_case ON export_bundles (case_id);
CREATE INDEX idx_export_bundles_key_id ON export_bundles (key_id);

CREATE TRIGGER export_bundles_no_update
    BEFORE UPDATE OR DELETE ON export_bundles
    FOR EACH ROW EXECUTE FUNCTION deny_mutation();
