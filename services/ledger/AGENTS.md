# Ledger service

## Purpose

The append-only system of record. Accepts writes for runs, events, tool invocations, side effects, guardrails, human approvals, bias slices, artifacts, custody events, and cases; computes per-row hash-chain entries; periodically anchors merkle roots over new rows to S3 WORM with an ed25519 signature. Also serves a filtered read surface (run list + dossiers) and exports signed bundles.

**Out of scope:** inbound auth/authn (actor is asserted by gateway via `X-Trackward-Actor`), tool execution (gateway concern), UI.

## Entry points

- `src/main.rs` — process bootstrap; spawns the anchoring loop.
- `src/lib.rs` — router assembly.
- `src/routes/` — one file per write-path resource (see `mod.rs`). Each handler: extract actor → start tx → `SELECT row_hash … FOR UPDATE` last row of `(table, run_id)` → compute new row hash via `chain-core` → `INSERT` with `prev_hash` + `row_hash` → commit.
- `src/anchoring.rs` + `src/anchors/` — periodic merkle build over un-anchored rows, sign, write `anchors` row, persist to `AnchorSink` (S3 object-lock impl in `anchors/s3.rs`).
- `src/db.rs`, `src/config.rs`, `src/tls.rs`, `src/otel.rs`, `src/signing.rs`, `src/s3.rs`, `src/hash.rs`, `src/actor.rs` — supporting infra.
- `migrations/` — append-only, numbered. Current head: `0011_merkle_anchors.sql`.

## Contracts & invariants

- **Append-only.** No `UPDATE` or `DELETE` in route handlers. Corrections are new rows.
- **Actor required.** Every write-path handler reads `actor_id` from request extensions (set by `actor.rs` from `X-Trackward-Actor`). Missing/blank → 400. Actor is part of the canonical bytes hashed into `row_hash` — a DBA who rewrites the actor column without recomputing the chain will be caught by the verifier.
- **Hash-chain ordering is per `(table, run_id)`.** Always lock the last row of that scope inside the same tx as the insert. Don't precompute outside the tx.
- **Canonical bytes live in `chain-core`.** Never inline canonicalization or hashing logic in this crate — call `chain_core::canonical_bytes()` / `compute_row_hash()`. A second implementation here is a chain-format fork.
- **Migrations are immutable once shipped.** New schema changes get a new numbered file.
- **Anchors are append-only too.** The anchor loop must be safe to restart: it resumes from the last `anchored_to` seq.
- **TLS PEM loading** uses `rustls_pki_types::pem::PemObject` (see `src/tls.rs`).

## Patterns

To add a new write-path resource:

1. Migration `migrations/NNNN_<resource>.sql` with `actor_id text not null`, `prev_hash bytea`, `row_hash bytea not null`, plus a `(table, run_id)`-aware index for last-hash lookup.
2. Model in `src/models/<resource>.rs` and re-export from `models/mod.rs`.
3. Route in `src/routes/<resource>.rs` following the lock-then-chain-then-insert pattern. Register in `routes/mod.rs`.
4. RED test in `tests/` exercising: actor required, row stored, tamper detection, chain links.
5. Gateway-side: add a record method to `services/gateway/src/ledger_client.rs` taking `actor_id: &str`.

To add a phase test:

- Place `tests/phaseN_<slug>.rs`. Tests hit a real Postgres (see existing tests for fixtures); do not mock the DB.

## Anti-patterns

- Don't reimplement canonical bytes or hashing here — use `chain-core`.
- Don't issue `UPDATE`/`DELETE` against any ledger table.
- Don't skip the `FOR UPDATE` lock on the previous row "because the test passed" — concurrent writes will silently produce a forked chain.
- Don't default a missing actor to a service account inside the ledger; that resolution belongs in the gateway, where the inbound principal is visible.
- Don't edit a shipped migration; add a new one.

## Related context

- Shared chain code: `crates/chain-core/`
- Verifier (re-derives the same hashes/roots): `tools/verifier/`
- Gateway (sets `X-Trackward-Actor`, calls these routes): `services/gateway/src/ledger_client.rs`
- Roadmap and phase rationale: `docs/launch-plan.md`
