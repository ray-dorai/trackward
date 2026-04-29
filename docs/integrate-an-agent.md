# Integrating an agent

This guide is for someone wiring an LLM agent (Codex, an internal harness,
a third-party agent runner) to record everything it does into Trackward.
The result is an append-only, hash-chained, signed log of every tool
call, every model exchange, every approval — verifiable offline.

## Mental model

```
       agent                gateway                 ledger
  ┌────────────┐    POST   ┌─────────┐    record   ┌─────────┐
  │ tool call  ├──────────►│ /tool/* ├────────────►│ events, │
  │ POST /tool │           │  /retrieve            │ tool_   │
  │   /<name>  │◄──────────┤  /approval/*          │ invoca- │
  └────────────┘  proxied  │              x-track- │ tions,  │
                           │              ward-run-│ ...     │
                           │              id       │         │
                           │              header   │         │
                           ┌──────┐                │         │
       (real backend)◄────►│      │                │         │
                           └──────┘                └─────────┘
```

The gateway sits in front of every tool the agent uses. The agent does
**not** talk to the ledger directly; the gateway records everything as a
side-effect of forwarding calls. This is the only way to guarantee
nothing the agent does goes unrecorded — if it didn't go through the
gateway, it didn't happen.

## What gets recorded automatically

When the agent posts to `/tool/{name}`:

- `tool_call` event with the input
- `tool_result` or `tool_error` event with the output
- `tool_invocation` row that links the call to the resulting side effects

When the agent posts to `/retrieve`:

- `retrieval` event with query + results

When a tool name appears in `GATED_TOOLS`, the gateway also enforces
human approval before forwarding — see `services/gateway/src/approval`.

The gateway mints a fresh run on the first call if no `x-trackward-run-id`
header is present, and stamps the run with the active registry binding
(prompt version, policy version) so every event traces back to the
git-versioned configuration that produced it.

## What you have to record yourself

The model exchange — *the prompt sent to the model and its response* — is
not a tool call, so it doesn't flow through `/tool`. The agent (or a
wrapper around it) needs to POST these events directly:

```sh
curl -fsS -X POST "$GATEWAY/runs/$RUN/events" \
  -H "Authorization: Bearer $GATEWAY_TOKEN" \
  -H "x-trackward-actor: codex@$USER" \
  -H 'content-type: application/json' \
  -d '{
    "kind": "model_request",
    "payload": {
      "model": "claude-sonnet-4-6",
      "messages": [...],
      "tools": [...]
    }
  }'
```

Same shape for `model_response`, with the assistant turn's content,
stop_reason, and any tool_use blocks. The events thread under the same
run, so the dossier shows the full reasoning trail interleaved with the
tool calls it produced.

## Wiring Codex (or any agent)

### 1. Configure tool routes

The gateway's `TOOL_ROUTES` env maps tool names to backend URLs:

```
TOOL_ROUTES=bash=http://bash-runner:8080/exec,edit=http://edit-runner:8080/apply
```

Each backend is whatever your team already runs to *actually execute* the
tool. The gateway records the call, forwards to the backend, records the
result. If the agent currently runs tools locally (e.g. Codex's bash
tool shells out in-process), you have two options:

- **Stand up a small HTTP wrapper** around the local executor and point
  the agent's tool runner at the gateway URL. Most agent frameworks
  let you swap a tool's implementation for an HTTP call.
- **Wrap each tool call in a shell script** that POSTs to the gateway,
  receives the recorded result, and returns it. See
  `scripts/demo-agent-run.sh` for the worked example.

### 2. Pick an auth posture

| Scenario | Set |
|---|---|
| Local dev, bring-up | nothing. Both services run unauthenticated. |
| Single-tenant cluster, simple | `GATEWAY_AUTH_TOKEN` + `LEDGER_AUTH_TOKEN` + `LEDGER_CLIENT_TOKEN`. Bearer everywhere. |
| Regulated buyer, review-board | mTLS via the Helm chart's `mtls.enabled=true` (Phase 10). Bearer can be additive. |

Both apply at once if you want them to. Constant-time comparison on
both sides; `/health` is always open so LBs can probe.

### 3. Stamp the agent's identity

Every write needs `x-trackward-actor`. For Codex, that's typically
`codex@<user>` or the OAuth subject from the user's session. For an
internal agent, your service-account name. The actor becomes part of
the canonical bytes hashed into `row_hash`, so a DBA who later rewrites
the actor column will be caught by the verifier.

### 4. Configure the registry binding

Every run gets stamped with the prompt-version and policy-version it
was produced under. Set these on the gateway:

```
REGISTRY_DIR=/path/to/registry        # versioned in your repo
PROMPT_WORKFLOW=codex
PROMPT_VERSION=2026-04-28-a
POLICY_SCOPE=global
POLICY_VERSION=1.0.0
GIT_SHA=$(git rev-parse HEAD)
```

The registry directory holds the actual prompt + policy files; the
gateway computes a deterministic content_hash on startup and registers
the version with the ledger. Two deployments running the same prompt
produce the same `prompt_version_id`.

## Producing a verifiable dossier

After the agent finishes a task:

```sh
# Create a case grouping this run.
CASE=$(curl -fsS -X POST "$LEDGER/cases" \
  -H "Authorization: Bearer $LEDGER_TOKEN" \
  -H "x-trackward-actor: codex@$USER" \
  -H 'content-type: application/json' \
  -d "{\"title\":\"task-$(date -u +%s)\",\"run_id\":\"$RUN\"}" | jq -r .id)

# Export the signed bundle.
curl -fsS -X POST "$LEDGER/cases/$CASE/exports" \
  -H "Authorization: Bearer $LEDGER_TOKEN" \
  -H "x-trackward-actor: codex@$USER" \
  > bundle.json

# Verify offline. Returns 0 on OK; non-zero on chain/sig failure.
verifier bundle.json
```

The bundle is self-contained: the auditor needs only the `verifier`
binary and the bundle JSON. No callback to the ledger; no trust in the
operator.

## End-to-end demo

`scripts/demo-agent-run.sh` runs the whole loop against
`docker-compose`: brings up the stack, simulates an agent task with a
tiny echo "tool", logs a model exchange, exports the bundle, verifies.
Read it as a working template for whatever agent you're integrating.

## Common gotchas

- **The agent must thread the run-id.** First tool call returns
  `x-trackward-run-id` in the response headers; every subsequent call
  for the same task must echo it back as a request header. Otherwise
  each call mints a new run and the dossier fragments.
- **Don't fall back to "log it later".** The hash chain is per-run; a
  delayed batch write breaks the ordering and the chain. Record at the
  moment of the call.
- **`actor` is not an audit log of the human user.** It's the *service
  identity making the call*. End-user attribution belongs in the event
  body. The gateway forwards what it has; if you want the human's
  email in the dossier, put it in the model_request payload.
