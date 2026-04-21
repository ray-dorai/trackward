# trackward Helm chart

Installs the two Rust services — **ledger** and **gateway** — plus the
Kubernetes glue around them (Services, optional NetworkPolicy, optional
Ingress). Does **not** install Postgres or S3: trackward is designed to
run against customer-managed durable stores, and pretending to provision
them would weaken the audit guarantees this project is built on.

## Prerequisites

You bring:

1. **Postgres** — a reachable cluster with the `trackward` database. The
   image runs migrations on startup; the role needs `CREATE` on the
   target schema.
2. **S3 (or compatible)** — two buckets:
   - an *artifact* bucket for blob uploads, and
   - optionally a separate **WORM** anchor bucket with Object Lock
     enabled if you want global merkle anchors (`ledger.anchor.enabled`).
3. **Secrets** — see below. This chart references pre-existing secrets
   by name rather than creating them, so you can manage key material with
   whatever tool you already use (Sealed Secrets, External Secrets, SOPS,
   Vault CSI, …).

## Secrets contract

| Secret name (default)   | Keys expected                                                | Used by                |
|-------------------------|--------------------------------------------------------------|------------------------|
| `trackward-postgres`    | `DATABASE_URL`                                               | ledger                 |
| `trackward-s3`          | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`                 | ledger                 |
| `trackward-ledger`      | `LEDGER_SIGNING_KEY_HEX`; `tls.crt`/`tls.key`/`ca.crt` if mTLS | ledger                 |
| `trackward-gateway`     | `LEDGER_BEARER_TOKEN` (optional); `tls.crt`/`tls.key`/`ca.crt` if mTLS | gateway                |

`LEDGER_SIGNING_KEY_HEX` must be a 32-byte ed25519 seed hex-encoded
(64 chars). Generate one with:

```
openssl rand -hex 32
```

Losing it means you can no longer sign new anchors *as that key*; losing
it does **not** invalidate previously signed anchors. Rotate by
deploying a new key and publishing the new pubkey alongside the old one
(see `docs/runbooks/key-rotation.md`, Phase 12).

## Quickstart

```
kubectl create namespace trackward

# 1. Create the four secrets (whatever your ops workflow uses).
kubectl -n trackward create secret generic trackward-postgres \
  --from-literal=DATABASE_URL="postgres://user:pass@pg.internal:5432/trackward?sslmode=require"

kubectl -n trackward create secret generic trackward-s3 \
  --from-literal=AWS_ACCESS_KEY_ID=... \
  --from-literal=AWS_SECRET_ACCESS_KEY=...

kubectl -n trackward create secret generic trackward-ledger \
  --from-literal=LEDGER_SIGNING_KEY_HEX="$(openssl rand -hex 32)"

kubectl -n trackward create secret generic trackward-gateway \
  --from-literal=LEDGER_BEARER_TOKEN="$(openssl rand -hex 32)"

# 2. Install the chart.
helm install trackward deploy/helm/trackward \
  -n trackward \
  --set s3.bucket=my-trackward-artifacts \
  --set s3.region=us-east-1
```

## Enabling global anchors

Set `ledger.anchor.enabled=true` and point `ledger.anchor.bucket` at an
Object-Lock-enabled S3 bucket. The ledger will write signed manifests
on the configured interval. Do **not** reuse the artifact bucket — the
retention contract is different and mixing them will either over-retain
day-to-day uploads (cost) or under-protect anchors (correctness).

```
helm upgrade trackward deploy/helm/trackward -n trackward \
  --set ledger.anchor.enabled=true \
  --set ledger.anchor.bucket=my-trackward-anchors \
  --set ledger.anchor.retainDays=2555
```

## Enabling mTLS

```
helm upgrade trackward deploy/helm/trackward -n trackward \
  --set mtls.enabled=true
```

Both secrets must carry the mTLS triple under keys `tls.crt`, `tls.key`,
and `ca.crt`. When mTLS is enabled, the gateway dials the ledger over
HTTPS and presents its own client cert; see `services/*/src/tls.rs` for
the partial-config refusal behavior.

## Upgrades

The ledger's migration story is forward-only: each release ships
migrations that the binary runs at boot. Back up Postgres before every
upgrade and validate the restore path (Phase 12 runbook covers this).

```
helm upgrade trackward deploy/helm/trackward -n trackward -f my-values.yaml
```

## Rollback

```
helm rollback trackward <REVISION> -n trackward
```

A Helm rollback reverts manifests only — it does **not** roll back
Postgres schema changes. If an upgrade applied a destructive migration,
roll back the DB from a snapshot first, then the chart.

## What this chart does NOT do

- Provision Postgres or S3.
- Manage secret rotation.
- Set up observability collectors (OTLP endpoints are an env knob away
  via `ledger.extraEnv` / `gateway.extraEnv`).
- Configure mesh-level mTLS — use `mtls.enabled=false` and rely on your
  service mesh if that's your model.
