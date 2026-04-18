# trackward

Audit-ready agent trail logging.

## Security

Found a security issue? Please don't open a public GitHub issue —
see [SECURITY.md](SECURITY.md) for how to report, what to expect, and
safe-harbor terms.

## Overview

trackward is a black-box recorder for AI agents. A gateway sits between the
agent and its tools/retrieval, and a ledger stores every run as an append-only,
signed chain of events, tool invocations, side effects, guardrail decisions,
human approvals, and artifacts. Investigators can export a **signed dossier
bundle** for any case and verify it offline with the `verifier` CLI.

## Services

- `services/ledger` — Postgres-backed append-only store. Signs export bundles
  with a pinned Ed25519 key. `/health` is open; every other route requires
  bearer auth when `LEDGER_AUTH_TOKEN` is set.
- `services/gateway` — Proxies tool calls and retrieval, records everything to
  the ledger, enforces human approvals on gated tools. `/health` is open;
  `/tool/*`, `/retrieve`, `/approval/*` require bearer auth when
  `GATEWAY_AUTH_TOKEN` is set.
- `tools/verifier` — Offline CLI that checks an export bundle's manifest hash
  against its signature, given the pinned public key.

## Development

```bash
docker compose up -d     # Postgres + MinIO
cargo test --workspace   # full suite, hits real DB/S3
```

Without `LEDGER_ENV=production`, the ledger generates a fresh signing key per
process — fine for dev, **useless for anything you plan to verify later**.

## Running in production

The dev defaults trade safety for friction. Before pointing a real agent at a
real deployment, set all of the following. Each paragraph explains why the
value matters, not just that it exists.

### Signing key (ledger)

Every export bundle is signed with a pinned Ed25519 key. If that key isn't
stable across ledger restarts, every dossier you've already exported becomes
unverifiable because the public key no longer matches. Provision once, store
in your secret manager, pin the public key (or its sha256 `key_id`) in every
downstream verifier.

```
LEDGER_ENV=production
LEDGER_SIGNING_KEY_HEX=<64 hex chars = 32 bytes>
```

Generate with any Ed25519 tool, e.g.:

```bash
python3 -c 'import secrets; print(secrets.token_hex(32))'
```

**Rotation:** rotating the signing key means any new bundles sign with the new
key; old bundles still verify against the *old* public key. Keep every public
key you've ever used — discarding one orphans every bundle signed with it.
The ledger logs `key_id` at startup; record it alongside the date you rolled
the key.

In production mode, missing `LEDGER_SIGNING_KEY_HEX` is a hard startup error,
not a warning. That's intentional: silently generating a throwaway key is how
you end up with unverifiable audit trails six months later.

### Bearer auth (ledger and gateway)

Both services gate every non-`/health` route behind
`Authorization: Bearer <token>` when their respective token is set. When
unset, auth is disabled — fine for local dev, catastrophic in production.

```
LEDGER_AUTH_TOKEN=<ledger token, long random string>
GATEWAY_AUTH_TOKEN=<gateway token, long random string>
LEDGER_CLIENT_TOKEN=<same value as LEDGER_AUTH_TOKEN>
```

`LEDGER_CLIENT_TOKEN` is what the **gateway** presents to the ledger on every
recording call; it must match the ledger's `LEDGER_AUTH_TOKEN`. Tokens are
compared in constant time on both sides. Use two distinct tokens (one for the
ledger, one for the gateway) so a leaked gateway token doesn't give direct
ledger write access.

### TLS

Two options — pick one based on what your customer's review board accepts:

- **Reverse proxy / mesh terminates TLS.** Put nginx, Caddy, or a cloud LB
  in front of each service and terminate there. Bearer tokens are only as
  safe as the transport, so cleartext over the public internet is not an
  option.
- **Native mTLS on the pods.** Set `TLS_CERT_PATH`, `TLS_KEY_PATH`,
  `TLS_CLIENT_CA_PATH` on the ledger and gateway to bind via rustls
  directly (Phase 10). The Helm chart's `mtls.enabled=true` default
  expects this. The gateway also presents a client cert to the ledger
  via `LEDGER_CLIENT_CERT_PATH` / `LEDGER_CLIENT_KEY_PATH` /
  `LEDGER_SERVER_CA_PATH`. All-or-nothing: partial configs refuse to
  start so a misconfigured deploy doesn't silently serve plaintext.

### Database and object store

```
DATABASE_URL=postgres://...
S3_BUCKET=...
S3_REGION=...
S3_ENDPOINT=<omit for real AWS; set for MinIO / other S3-compatibles>
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
```

The ledger's Postgres schema is append-only — there's a `deny_mutation`
trigger on every table. Backups should be logical dumps (`pg_dump`) on a
schedule that matches your retention commitments. Artifacts live in S3; the
ledger stores only the sha256 and the object key. Your bucket lifecycle
policy is your retention policy — set it explicitly.

### Observability

```
OTEL_EXPORTER_OTLP_ENDPOINT=<OTLP collector URL>
```

Both services emit OpenTelemetry traces when this is set. The ledger also
logs the signing `key_id` at startup; capture that line, you'll want it when
you rotate.

### Env var reference

| Var                          | Service         | Purpose                                                    |
|------------------------------|-----------------|------------------------------------------------------------|
| `LEDGER_ENV`                 | ledger          | `production` makes the signing key mandatory               |
| `LEDGER_SIGNING_KEY_HEX`     | ledger          | 32-byte Ed25519 seed, hex-encoded                          |
| `LEDGER_AUTH_TOKEN`          | ledger          | Bearer required on all non-`/health` routes                |
| `DATABASE_URL`               | ledger          | Postgres connection string                                 |
| `S3_BUCKET` / `S3_REGION` / `S3_ENDPOINT` | ledger | Artifact store                                       |
| `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` | ledger | S3 credentials                                |
| `LISTEN_ADDR`                | ledger          | Bind address (default `0.0.0.0:3000`)                      |
| `GATEWAY_AUTH_TOKEN`         | gateway         | Bearer required on all non-`/health` routes                |
| `LEDGER_CLIENT_TOKEN`        | gateway         | Bearer the gateway presents to the ledger                  |
| `LEDGER_URL`                 | gateway         | Ledger base URL                                            |
| `TOOL_ROUTES`                | gateway         | `name1=url1,name2=url2` routing table                      |
| `GATED_TOOLS`                | gateway         | Comma-separated tools that require human approval          |
| `RETRIEVAL_BACKEND`          | gateway         | URL of retrieval backend                                   |
| `REGISTRY_DIR` / `PROMPT_WORKFLOW` / `PROMPT_VERSION` / `POLICY_SCOPE` / `POLICY_VERSION` / `GIT_SHA` | gateway | Registry binding — stamps runs with prompt+policy versions |
| `GATEWAY_LISTEN_ADDR`        | gateway         | Bind address (default `0.0.0.0:4000`)                      |
| `OTEL_EXPORTER_OTLP_ENDPOINT`| both            | OTLP collector URL                                         |
| `TLS_CERT_PATH` / `TLS_KEY_PATH` / `TLS_CLIENT_CA_PATH` | both | Native mTLS (Phase 10). All three set or all unset.        |
| `LEDGER_CLIENT_CERT_PATH` / `LEDGER_CLIENT_KEY_PATH` / `LEDGER_SERVER_CA_PATH` | gateway | Client cert the gateway presents to the ledger     |
| `ANCHOR_BUCKET` / `ANCHOR_INTERVAL_SECS` / `ANCHOR_RETAIN_DAYS` | ledger | WORM anchor bucket + cadence (Phase 9b)         |
| `LEDGER_DEFAULT_ACTOR`       | ledger          | Optional fallback actor for legacy callers (Phase 8c)      |
