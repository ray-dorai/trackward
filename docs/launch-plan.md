# Launch-readiness plan

From Phase 7 complete to **Stage 1: design-partner-ready**. One real customer,
hands-on, tolerates rough edges because they shape the product. Stage 2
(controlled beta) and Stage 3 (GA) are deferred until a design partner pulls
specifics out of us.

**Critical path: ~7–9 weeks** backend + UI concurrent. Parallel UI track can
start any time; everything else is sequential.

## Key reorder from the earlier "trust first" thinking

Thinking file-by-file revealed that **actor must land before trust**. If the
hash-chain doesn't include `actor_id` in its canonical bytes, a malicious DBA
can rewrite the "who did this" field without detection — exactly what
hash-chaining exists to prevent. Landing actor first means one canonical hash
format, not two. The one-week delay on the trust pitch is a conversation;
two hash-format variants would be a permanent scar.

Also: hygiene and disclosure are ~1 day each and unblock nothing, but every
day they're missing is a day we'd fail a security questionnaire. Do them
first as a cheap-but-real derisking, then actor, then trust.

## Phase 8 — launch prerequisites (hygiene + disclosure + actor)

Branch: `phase-8/launch-prep`.

### 8a — supply-chain hygiene in CI (~1 day)

1. `deny.toml` — license allow-list, advisory ignore policy
2. `.github/workflows/security.yml` — `cargo audit`, `cargo deny check`, SBOM via `cargo cyclonedx` on every PR
3. commit

### 8b — security disclosure (~1 day)

4. `SECURITY.md` — reporting channel, response SLA, safe-harbor language
5. `README.md` — link to `SECURITY.md`
6. commit

### 8c — principal/actor on every write (~1 week)

7. `services/ledger/tests/phase8_actor.rs` (RED) — write-path requires actor; actor stored on row; bad/missing actor → 400
8. `services/ledger/migrations/0009_actor.sql` — `actor_id text not null` on every write-path table (`runs`, `events`, `tool_invocations`, `side_effects`, `guardrails`, `human_approvals`, `bias_slices`, `artifacts`, `custody_events`, `cases`)
9. `services/ledger/src/auth.rs` — extract `X-Trackward-Actor`, stash in request extensions
10. `services/ledger/src/routes/{events,tool_invocations,side_effects,guardrails,human_approvals,bias_slices,artifacts,custody_events,runs,cases}.rs` — read actor from extensions, include in INSERT
11. `services/gateway/src/ledger_client.rs` — accept `actor_id: &str` on every record method, set header
12. `services/gateway/src/{tool_proxy,retrieval_proxy,approval}.rs` — resolve actor (inbound header or configured service-account) and forward
13. GREEN commit

PR: draft against main, titled "Phase 8: launch prerequisites (hygiene + disclosure + actor)".

## Phase 9 — trust story (hash-chain + anchored merkle roots)

Branch: `phase-9/trust`.

### 9a — per-row hash chain (~1.5 weeks)

14. new crate `crates/chain-core/` — `canonical_bytes()`, `compute_row_hash()`, pinned-byte-output tests. Extract so ledger + verifier cannot drift.
15. `services/ledger/Cargo.toml` + `tools/verifier/Cargo.toml` — depend on `chain-core`
16. `services/ledger/tests/phase9_chain.rs` (RED) — chain per `(table, run_id)`; row-tamper detection; actor-tamper detection
17. `services/ledger/migrations/0010_hash_chain.sql` — `prev_hash bytea`, `row_hash bytea not null`, index for last-hash lookup per (table, run_id)
18. Same route files as #10 — on insert: `SELECT row_hash … FOR UPDATE` on last row of the run → compute → insert both columns
19. GREEN commit

### 9b — merkle anchors (~1.5 weeks)

20. `services/ledger/tests/phase9_anchor.rs` (RED) — periodic job produces signed root over new rows; verifier validates; restart resumes from last anchored seq
21. `services/ledger/migrations/0011_merkle_anchors.sql` — `anchors(id, anchored_from, anchored_to, root_hash, signature, key_id, anchor_target, anchored_at)`
22. `services/ledger/src/anchoring.rs` — merkle build, root sign, insert row
23. `services/ledger/src/anchors/{mod.rs,s3.rs}` — `trait AnchorSink`, S3 object-lock impl (WORM bucket)
24. `services/ledger/src/config.rs` — `anchor_interval_secs`, `anchor_bucket`
25. `services/ledger/src/main.rs` — spawn anchor loop
26. `tools/verifier/src/anchor.rs` — fetch anchor, verify signature, recompute merkle root from dossier rows
27. `tools/verifier/src/main.rs` — `--anchor-url` flag
28. `tools/verifier/tests/verify_anchor.rs`
29. GREEN commit

PR: draft, "Phase 9: cryptographic append-only (hash-chain + anchored roots)".

**Discipline:** resist expanding into key rotation, multi-sig, HSM, revocation
in this PR. Ship hash-chain + one anchor destination, stop, move on.

## Phase 10 — mTLS option (~1 week)

Branch: `phase-10/mtls`.

30. `services/ledger/tests/phase10_mtls.rs` (RED) — rejects without cert, accepts with valid, actor derivable from cert subject
31. `services/ledger/Cargo.toml` — `axum-server` with `tls-rustls`, `rustls-pemfile`
32. `services/ledger/src/tls.rs` — load server cert/key + client CA → rustls ServerConfig
33. `services/ledger/src/config.rs` — `tls_cert_path`, `tls_key_path`, `tls_client_ca_path`
34. `services/ledger/src/main.rs` — conditional `axum_server::bind_rustls`
35. Same pattern on gateway: `services/gateway/src/tls.rs`, config, main
36. `services/gateway/src/ledger_client.rs` — client identity when `LEDGER_CLIENT_CERT_PATH` set
37. GREEN commit

Bearer stays supported. mTLS is additive for customers whose review boards
won't accept shared secrets.

## Phase 11 — one-command deploy (~1.5 weeks)

Branch: `phase-11/helm`.

38. `deploy/helm/trackward/Chart.yaml`
39. `deploy/helm/trackward/values.yaml` — replicas, images, postgres (external), S3, secret refs: signing-key, ledger-token, gateway-token, mTLS certs, anchor-bucket creds
40. `deploy/helm/trackward/templates/_helpers.tpl`
41. `templates/ledger-deployment.yaml`
42. `templates/ledger-service.yaml`
43. `templates/ledger-secret.yaml`
44. `templates/gateway-deployment.yaml`
45. `templates/gateway-service.yaml`
46. `templates/gateway-secret.yaml`
47. `templates/networkpolicy.yaml` — deny egress except postgres, S3, anchor bucket; explicit gateway→ledger
48. `templates/ingress.yaml` (optional; usually customer-provided)
49. `deploy/helm/trackward/README.md` — prereqs, install, upgrade, rollback
50. `.github/workflows/helm-lint.yml` — `helm lint` + kind-based install test on PR

## Phase 12 — operational drills + runbooks (~1 week)

Branch: `phase-12/runbooks`.

51. `docs/runbooks/backup-restore.md`
52. `docs/runbooks/key-rotation.md`
53. `docs/runbooks/incident-response.md`
54. `scripts/drill-restore.sh` — dump → nuke → restore → verifier on a known bundle
55. Execute each runbook once, amend with what surprised you
56. commit

## Parallel track — investigator UI (~2–4 weeks, separate skill set)

Depends only on existing REST surface + Phase 6 dossier shape, both stable.
Do in parallel with phases 8–12 if a frontend-leaning collaborator is
available; do last otherwise.

- `ui/` (SvelteKit or Vite+React — pick one and stay)
- `ui/src/lib/api.ts` — typed client
- `ui/src/routes/runs/+page.*` — filtered list
- `ui/src/routes/runs/[id]/+page.*` — dossier view
- `ui/src/routes/cases/[id]/+page.*` — case dossier + chain-integrity badge
- `ui/src/routes/verify/+page.*` — upload bundle, display verifier result
- `deploy/helm/trackward/templates/ui-deployment.yaml` — add to chart once UI is live

## Non-goals for Stage 1

Explicitly deferred to Stage 2 (when a design partner pulls them):

- Key rotation automation, multi-sig, HSM, revocation
- Rate limiting, per-principal quotas
- OIDC / short-lived tokens
- SOC 2 process
- Observability beyond what's already in place (OTEL traces + basic logs)
- Pricing, billing, self-serve onboarding
- EU region / data residency

## Shape of done

Stage 1 is complete when:
- `cargo audit` + `cargo deny` clean in CI
- Every row in the ledger chains cryptographically to its predecessor in the
  same run, and every such chain terminates at a signed merkle root published
  to an externally-durable location
- Every write carries an `actor_id` populated from either bearer-authenticated
  header or mTLS client cert subject
- A customer with zero Rust knowledge can `helm install` the stack against
  their own Postgres + S3 and have a working gateway + ledger
- A first-time on-call operator has runbooks for backup/restore,
  key rotation, and incident response — each exercised at least once
- An investigator with zero Rust knowledge can open the UI, pull a run, and
  verify a dossier bundle
