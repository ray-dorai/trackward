# Trackward

Cryptographically-anchored, append-only audit trail for AI-agent actions. Customers are regulated; attackers are real. The audit trail — commits, `deny.toml` justifications, CI gates, signed merkle anchors — is part of the product, not just process.

## Workspace layout

Cargo workspace (`resolver = "2"`):

- `services/ledger/` — Append-only Postgres-backed write surface + read API. Hash-chain per `(table, run_id)`, periodic signed merkle anchors to S3 WORM. **See `services/ledger/AGENTS.md`.**
- `services/gateway/` — Inbound proxy that records to ledger: tool-call proxy, retrieval proxy, human-approval gates. Forwards `X-Trackward-Actor` to ledger.
- `crates/chain-core/` — Shared canonical-bytes + row-hash + merkle code. **Extracted so ledger and verifier cannot drift.** Any change here is a chain-format change.
- `tools/verifier/` — Offline bundle verifier. Re-derives row hashes and merkle roots from a dossier; checks anchor signatures.
- `tools/trackward-model-proxy/` — Tier 1 capture: HTTP proxy between an agent and a model API (Anthropic / OpenAI). Records every request and response into the ledger. Universal across agents because every agent talks to a model API.
- `tools/trackward-trace/` — Tier 3 capture: OS-level syscall trace for closed agents. Wraps the agent process with `strace` (and eventually eBPF); records every `execve` / file write / network connect.
- `examples/claude-code/` — Tier 2 capture: Claude Code hook integration (six bash scripts + settings.json).
- `deploy/helm/trackward/` — Helm chart, one-command deploy.
- `infra/otel/`, `infra/aws/` — OTLP collector config, AWS infra notes.
- `registry/{prompts,policies,evals}/` — Versioned prompt/policy/eval definitions consumed by the gateway.
- `docs/launch-plan.md` — Phase-by-phase roadmap. **Authoritative for "why this phase, why now".**
- `.github/workflows/` — `security.yml` (cargo deny — single source of truth for advisories, licenses, registries, git sources) and test gates. `deny.toml` advisory ignores must carry honest justifications.

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
- **Supply-chain audit trail is part of the product.** `cargo deny check` runs on every PR (and daily against `main`). It is the single source of truth — one tool, one config (`deny.toml`), one place to keep honest. If `deny.toml` ignores an advisory, the justification must reflect the *actual* reason (transitive dep we don't control, etc.) — not aspirational future work. Audit honesty is a hard rule, not a style preference.
- **CI gates are honored, not routed around.** PRs gated on `cargo test`; deploys gated on auth posture. If a gate trips, fix the cause; don't loosen the gate.
- **TLS via rustls 0.23 stack.** PEM parsing goes through `rustls_pki_types::pem::PemObject` (the sanctioned successor to the unmaintained `rustls-pemfile`).
- **Four integration tiers, one promise.** Agent activity must be captured regardless of whether the harness exposes interception points. A customer integration document MUST declare which tier(s) it uses, so the auditor knows what guarantee they're reading.

  1. **Model API proxy** (`tools/trackward-model-proxy/`) — strongest, most universal. HTTP proxy between agent and model API records every request (model name, system prompt, messages, tools) and every response (content, stop_reason, tool_use blocks, usage). Captures the agent's *intent* before it acts. Works against any agent because every agent talks to a model API. Promise: *if the model decided to do it, it's in the dossier.*
  2. **Hook-level** (`examples/claude-code/`) — synchronous, semantically rich, agent-specific. `PreToolUse` / `PostToolUse` style API in the agent harness. Captures the agent's *interpretation* of the model's request as concrete tool calls. Promise: *if the hook didn't fire, the tool didn't run.*
  3. **OS-level syscall capture** (`tools/trackward-trace/`) — universal action capture. Wraps the agent process tree with `strace` / `auditd` / eBPF; records every `execve`, file write, network connect at the kernel boundary. No agent cooperation required. Promise: *if it ran on this machine, it's in the dossier.*
  4. **Log mirror (best-effort)** — passive scrape of agent-written session logs. Async; silent gaps possible. Bundle metadata MUST mark the dossier as `capture_tier: best_effort`. Use only when 1–3 are all unavailable.

  Tiers compose. The strongest audit posture (Tier 1 + Tier 3) gives **double-entry bookkeeping**: every model decision matched against every kernel-observed action. Discrepancy ("model wanted X, kernel saw Y") is itself an audit signal.

## Conventions

- Phases ship on branches named `phase-N[.M]/<slug>` and merge via PR to `main`.
- Tests are RED-then-GREEN per phase step (see `docs/launch-plan.md`).
- Migrations are append-only and numbered (`NNNN_description.sql`); never edit a shipped migration.
- Don't bundle unrelated changes into one commit — the commit log is part of the audit trail.
