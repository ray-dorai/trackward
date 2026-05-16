# Integrating an agent

How to record every action an LLM agent takes — every prompt, every model
output, every tool call, every file write — into Trackward as an
append-only, hash-chained, signed dossier verifiable offline.

## Pick your tier

Four ways to capture, ranked by how strong an audit guarantee they give.
The right answer for most customers is **Tier 1 + Tier 3 together**:
double-entry bookkeeping, model-side decisions cross-checked against
kernel-side actions.

| Tier | Where it captures | Works against | Setup |
|---|---|---|---|
| **1. Model API proxy** | The HTTP boundary between the agent and the model API (Anthropic / OpenAI / etc.) | **Any agent** | `tools/trackward-model-proxy/` — point the agent at the proxy via `ANTHROPIC_BASE_URL` / `OPENAI_API_BASE` |
| **2. Hook-level** | The agent harness's own interception API | Claude Code, agents with comparable APIs (pi-mono if it exposes hooks) | `examples/claude-code/` — drop `settings.json` into `~/.claude/` |
| **3. OS syscall capture** | The kernel boundary (every `execve` / file write / network connect) | **Any agent on Linux** | `tools/trackward-trace/` — wrap the agent invocation: `trackward-trace -- codex` |
| **4. Log mirror** | After-the-fact scrape of agent-written session logs | Closed agents with no hooks and no usable process model | (case-by-case; bundle metadata flags `capture_tier: best_effort`) |

A customer integration document MUST declare which tier(s) it uses.
Auditors read that declaration to know what guarantee they're getting.

---

## Tier 1: Model API proxy

The strongest, most universal capture. Every agent on the planet talks to
a model API. The proxy sits between agent and API, mirrors every request
and response into the ledger, then forwards transparently.

What's captured per request/response:
- `model_request` event: model name, system prompt, messages array,
  tools the agent advertised, sampling params
- `model_response` event: stop_reason, content blocks (text + tool_use
  + thinking, in order), token usage

Why this catches what other tiers miss:
- Captures **intent** (what the model decided) before any execution
- Survives an agent that bypasses its own hooks
- Catches discrepancies: "model emitted Bash(rm foo); kernel never saw
  rm execute" — the agent silently dropped a tool call
- Works against agents with no hooks, no transcript files, no
  cooperation at all

Setup:

```sh
trackward-model-proxy --upstream https://api.anthropic.com --port 8088 &
export ANTHROPIC_BASE_URL=http://localhost:8088
# (agent runs as normal; every call records to the ledger)
```

Limits worth naming:
- Streaming responses (Anthropic SSE, OpenAI SSE) need stream-aware
  forwarding — the MVP records the assembled response after the stream
  closes; live streaming is a v2 item.
- TLS interception. The customer points the agent at an HTTP proxy via
  env var; HTTPS upstream is fine but the agent → proxy hop is local
  cleartext. Run on `127.0.0.1` only.
- Doesn't capture actions the agent takes *without consulting the
  model* (e.g., hardcoded behaviors, retries handled in the agent
  harness itself). That's why Tier 3 is the natural complement.

---

## Tier 2: Hook-level integration

Synchronous, blocking, semantically rich. Best when the agent harness
exposes an interception API. Today's first-class example is **Claude
Code** via `PreToolUse` / `PostToolUse` / `Stop` / `SessionStart` /
`SessionEnd`. See `examples/claude-code/` for a drop-in `settings.json`
plus six bash hooks that record every Bash/Read/Write/Edit, every model
turn, and the full transcript-line stream.

What's captured (per the two-stream model in the example README):
- **Live event stream**: `tool_call`, `tool_result`, `user_message`,
  `model_response`, `task_complete`, `session_end`
- **Transcript snapshot stream**: every line of Claude Code's transcript
  JSONL — assistant text, thinking, tool_use, system, attachment,
  permission-mode, queue-operation, file-history-snapshot, ai-title

`PreToolUse` can **block** by exiting 2 — the gateway can refuse a
tool if policy fails. We allow today; policy enforcement is a separate
concern.

When to use:
- Customer is already on Claude Code (or a hookable harness)
- You want semantically-typed events (`tool_call` not `os_execve`)
- You want synchronous policy enforcement at tool-call time

Limits:
- Agent-specific. Each new harness needs its own hook scripts.
- Bypassing requires editing `settings.json` (loggable, but possible).
- Some events (mid-iteration assistant text, thinking) only appear in
  the snapshot stream, not the live stream — Claude Code's hook
  semantics are narrower than "fire on every event."

---

## Tier 3: OS-level syscall capture

Universal action capture. The agent runs as a Linux process; every
command it executes is an `execve` syscall, every file it writes is an
`openat`+`write`, every network call is a `connect`. `tools/trackward-trace/`
wraps the agent process with kernel-level tracing and POSTs each captured
syscall as an `os_*` event into the same ledger schema.

When to use:
- Agent harness is closed (Codex CLI, dirac) — no hooks available
- You need a tamper-resistant action record (bypassing requires
  kernel-level evasion)
- You want **double-entry** with Tier 1: cross-check model decisions
  against actual kernel-observed actions

Trade-offs:
- ✅ Universal — works against any agent, no source modification
- ✅ Synchronous — captured at the boundary, not after-the-fact
- ⚠️ Lower semantic richness — `os_execve("/bin/bash", ["bash","-c","rm foo"])`
  instead of `tool_call(name="Bash", input={...})`. Captures *what
  happened*, not *what the agent thought it was doing*.
- ⚠️ Performance overhead — strace MVP is ~10–30% on syscall-heavy
  workloads. eBPF v2 (planned) targets ~1–2%.

Usage:

```sh
trackward-trace -- codex
trackward-trace -- dirac --headless
trackward-trace --actor agent@prod -- ./your-agent-binary
```

---

## Tier 4: Log mirror (best-effort fallback)

For agents with neither hooks nor a usable process boundary — rare,
mostly cloud-hosted agents that never touch a local shell. Reads the
agent's session log (Codex's rollout JSONL, etc.) and replays into the
ledger.

Bundle metadata MUST set `capture_tier: best_effort` so an auditor
reading the dossier knows there can be silent gaps (no events between
the last log write and a crash; nothing if the agent doesn't write a log).

---

## What gets recorded across all tiers

Every event lands in the same `events` table with the same hash chain
per `(table, run_id)`. The dossier reader doesn't care which tier
produced an event; the auditor reads `kind` and `body` and re-derives
the chain. Cross-tier events from the same agent activity belong to
the same run and chain together in arrival order.

A signed dossier bundle (signed by the ledger's ed25519 key, verifier
re-derives offline) wraps the run and any linked artifacts.

---

## Producing a verifiable dossier

```sh
# Open a case for this agent run
CASE=$(curl -fsS -X POST "$LEDGER/cases" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d "{\"title\":\"task-$(date -u +%s)\",\"opened_by\":\"$ACTOR\"}" \
  | jq -r .id)

# Link the run as evidence
curl -fsS -X POST "$LEDGER/cases/$CASE/evidence" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d "{\"evidence_type\":\"run\",\"evidence_id\":\"$RUN\",\"linked_by\":\"$ACTOR\"}"

# Export the signed bundle
curl -fsS -X POST "$LEDGER/cases/$CASE/exports" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d "{\"signed_by\":\"$ACTOR\"}" > bundle.json

# Verify offline
verifier bundle.json
```

---

## Common gotchas

- **Run-id threading.** Within a tier, the run-id flows automatically
  (proxy/hook/trace all stash it in session state). When mixing tiers,
  pass the same run-id explicitly so all events chain together.
- **Don't log floats.** The ledger's canonical_json layer rejects
  floats by design (precision ambiguity breaks chain verification).
  Coerce client-side to int (when whole) or string. The model proxy and
  Codex mirror both do this; copy the helper from
  `tools/trackward-model-proxy/trackward-model-proxy.py`.
- **`actor` is the *service identity*, not the human user.** End-user
  attribution belongs in the event body. Don't put a human's email in
  the actor field.
