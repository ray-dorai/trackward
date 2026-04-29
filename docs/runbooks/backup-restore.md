# Runbook: backup and restore

**Status:** v1 — drilled against `docker-compose` only. Has not yet been
exercised against a production-grade Postgres + S3.

## What is durable

Trackward keeps three pieces of state:

1. **Postgres** — the ledger. Every event, run, anchor, custody record,
   case, and export bundle. Durable record-keeping is its job.
2. **S3 artifact bucket** (`s3.bucket` in values.yaml) — blob store for
   uploaded artifacts. Referenced by `row_hash` from Postgres rows.
3. **S3 anchor bucket** (`ledger.anchor.bucket`) — WORM bucket holding
   signed merkle manifests. Object-locked by configuration; immutable
   for the retention window.

All three must be backed up. Losing any one of them breaks the
end-to-end audit story even if the other two are intact.

## Backup

### Postgres

Use whatever your platform already does. RDS / Cloud SQL automated
snapshots; for self-hosted, `pg_dump` is fine:

```sh
pg_dump --format=custom --no-owner --no-acl \
  --dbname="$DATABASE_URL" \
  --file="trackward-$(date -u +%Y%m%dT%H%M%SZ).pgdump"
```

Frequency: at least daily; ideally PITR via WAL shipping. Retention:
match the longest retention you offer customers on the audit trail
itself, plus a buffer for legal hold.

### S3 artifact bucket

Standard cross-region replication or AWS Backup. The artifact bucket
is append-only in normal operation, but is *not* object-locked — a
cluster-admin error can `aws s3 rm` it. Replicate.

### S3 anchor bucket

Object-locked at configuration time; immutable within retention. No
backup is required for the retention window — that is the whole point
of object lock. Outside the window, replicate to a second region for
defense in depth.

## Restore

### Postgres

```sh
# 1. Provision an empty database with the same name.
createdb trackward

# 2. Restore the dump.
pg_restore --no-owner --no-acl --dbname="$DATABASE_URL" \
  trackward-YYYYMMDDTHHMMSSZ.pgdump

# 3. Run the ledger against the restored DB. Migrations are idempotent
#    and will no-op against a current schema.
helm upgrade trackward ./deploy/helm/trackward --reuse-values
```

### S3

If you've replicated the artifact bucket, fail over by updating
`s3.endpoint` (or `s3.bucket` if the replicated copy lives under a
different name) and `helm upgrade`.

The anchor bucket cannot be "restored" — its object-lock contract
means within retention the bytes still exist. Outside retention, point
at the replica.

## Verify the restore

```sh
# Re-export a known case and confirm the verifier accepts the signed
# bundle (signing is deterministic for the same manifest, so the bundle
# bytes should match a pre-restore reference exactly).
curl -s -X POST "$LEDGER/cases/$KNOWN_CASE_ID/exports" > restored.json
verifier restored.json   # exit 0 means OK

# Compare against the pre-restore bundle to detect data loss.
diff <(jq -S . pre-restore.json) <(jq -S . restored.json)
```

`scripts/drill-restore.sh` automates this against the `docker-compose`
stack. Run it after every major Postgres or schema change.

## Known gotchas

- **Migrations are immutable once shipped** (per `services/ledger/AGENTS.md`).
  A restored dump from an older schema will be migrated forward by the
  ledger on startup; the reverse is not supported. Pin the image tag.
- The `actor_id` column is part of the canonical bytes hashed into
  `row_hash`. A DBA who restores from a dump where `actor_id` was
  rewritten will be caught by the verifier on the next bundle export.
  This is by design.
- Anchor signatures verify against the ledger's signing public key. If
  the key was rotated between backup and restore, the verifier needs
  *both* public keys (see `key-rotation.md`).
