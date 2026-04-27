# Trackward

Cryptographically-anchored, append-only audit trail for AI-agent actions. Customers are regulated; attackers are real. The audit trail — commits, `deny.toml` justifications, CI gates, signed merkle anchors — is part of the product, not just process.

## Workspace layout

Cargo workspace (`resolver = "2"`):

- `services/ledger/` — Append-only Postgres-backed write surface + read API. Hash-chain per `(table, run_id)`, periodic signed merkle anchors to S3 WORM. **See `services/ledger/AGENTS.md`.**
- `services/gateway/` — Inbound proxy that records to ledger: tool-call proxy, retrieval proxy, human-approval gates. Forwards `X-Trackward-Actor` to ledger.
- `crates/chain-core/` — Shared canonical-bytes + row-hash + merkle code. **Extracted so ledger and verifier cannot drift.** Any change here is a chain-format change.
- `tools/verifier/` — Offline bundle verifier. Re-derives row hashes and merkle roots from a dossier; checks anchor signatures.
- `deploy/helm/trackward/` — Helm chart, one-command deploy.
- `infra/otel/`, `infra/aws/` — OTLP collector config, AWS infra notes.
- `registry/{prompts,policies,evals}/` — Versioned prompt/policy/eval definitions consumed by the gateway.
- `docs/launch-plan.md` — Phase-by-phase roadmap. **Authoritative for "why this phase, why now".**
- `.github/workflows/` — `security.yml` (cargo audit + cargo deny + SBOM) and test gates. `deny.toml` advisory ignores must carry honest justifications.

## Intent Layer

**Before modifying code in a subdirectory, read its `AGENTS.md` first** to understand local patterns and invariants.

- **Ledger service**: `services/ledger/AGENTS.md` — append-only write paths, hash chain, anchoring.

(Other areas are small enough to navigate from this root file. Add a child node when a directory crosses ~20k tokens or develops hidden contracts.)

## Global invariants

- **Append-only.** No write path issues `UPDATE` or `DELETE` against ledger tables. Corrections are new rows. If you find yourself reaching for `UPDATE`, stop.
- **Actor on every write.** Every write-path row stores `actor_id text not null`. Gateway resolves actor from inbound `X-Trackward-Actor` header (or configured service-account) and forwards it. Missing/blank actor → 400, never a default.
- **One canonical hash format.** All canonical-bytes and row-hash logic lives in `crates/chain-core/`. Ledger and verifier both depend on it. Never reimplement canonicalization elsewhere — divergence silently breaks verification.
- **Hash-chain integrity.** `prev_hash` / `row_hash` writes use `SELECT … FOR UPDATE` on the last row of `(table, run_id)` inside the same transaction as the insert. Don't shortcut the lock.
- **Anchors are signed and WORM.** Merkle roots over `(anchored_from, anchored_to)` are signed (ed25519) and persisted to an S3 object-lock bucket. Restart resumes from the last anchored seq.
- **Supply-chain audit trail is part of the product.** `cargo audit` + `cargo deny check` run on every PR. If `deny.toml` ignores an advisory, the justification must reflect the *actual* reason (transitive dep we don't control, etc.) — not aspirational future work. Audit honesty is a hard rule, not a style preference.
- **CI gates are honored, not routed around.** PRs gated on `cargo test`; deploys gated on auth posture. If a gate trips, fix the cause; don't loosen the gate.
- **TLS via rustls 0.23 stack.** PEM parsing goes through `rustls_pki_types::pem::PemObject` (the sanctioned successor to the unmaintained `rustls-pemfile`).

## Conventions

- Phases ship on branches named `phase-N[.M]/<slug>` and merge via PR to `main`.
- Tests are RED-then-GREEN per phase step (see `docs/launch-plan.md`).
- Migrations are append-only and numbered (`NNNN_description.sql`); never edit a shipped migration.
- Don't bundle unrelated changes into one commit — the commit log is part of the audit trail.
