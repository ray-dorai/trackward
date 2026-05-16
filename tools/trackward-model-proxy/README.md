# trackward-model-proxy

Tier 1 capture for Trackward. HTTP proxy that mirrors every model-API
request and response into the ledger, then forwards transparently to
the upstream.

## Why this is Tier 1

Every LLM agent on the planet talks to a model API. The proxy sits at
that boundary, so:

- **Universal** — works against any agent (Codex, Claude Code, dirac,
  Cursor, internal harnesses) because every agent makes the same
  `POST /v1/messages` or `POST /v1/chat/completions` calls.
- **Captures intent before action** — records the model's `tool_use`
  blocks before any agent executes them. An agent that drops a tool
  call silently still leaves a record on the model side.
- **Tamper-resistant** — bypassing means making the model call
  yourself, which means having the model API key, which means the
  customer's compliance team is involved.
- **Composes with Tier 3** — combine with `tools/trackward-trace/` for
  double-entry bookkeeping: every model decision matched against every
  kernel-observed action.

## Usage

```sh
# Start the proxy (Anthropic example)
./trackward-model-proxy.py --upstream https://api.anthropic.com --port 8088

# In a separate shell, point your agent at it
export ANTHROPIC_BASE_URL=http://localhost:8088
codex                    # or any agent that respects ANTHROPIC_BASE_URL

# OpenAI works too — different upstream, same proxy
./trackward-model-proxy.py --upstream https://api.openai.com --port 8089
export OPENAI_API_BASE=http://localhost:8089/v1
```

The proxy mints a Trackward run on the first request it sees and posts
two events per call: `model_request` (model name, system, messages,
tools, sampling params) and `model_response` (stop_reason, content
blocks, usage). The hash chain links them in arrival order; the
verifier validates the chain offline.

## Env

| | |
|---|---|
| `TRACKWARD_LEDGER_URL`   | default `http://localhost:3000` |
| `TRACKWARD_ACTOR`        | default `model-proxy@$USER` |
| `TRACKWARD_LEDGER_TOKEN` | optional bearer if ledger has `LEDGER_AUTH_TOKEN` set |
| `TRACKWARD_FAIL_LOUD`    | if set, ledger errors abort the proxy instead of just logging |

## Limits (v1 / MVP)

- **Streaming responses** are forwarded byte-for-byte to the agent, but
  the recorded `model_response` event contains the assembled body
  rather than per-chunk events. Acceptable for audit (the final
  content is what matters); v2 should record incremental SSE chunks.
- **Single run per proxy lifetime.** The MVP uses one Trackward run
  for all traffic the proxy sees from start to stop. Multi-session
  correlation (per-agent-task, per-conversation) needs a header
  convention or session-id query param — v2.
- **No TLS termination.** The agent → proxy hop is local cleartext;
  the proxy → upstream hop is HTTPS as normal. Bind only to
  `127.0.0.1` (the default). Don't expose the proxy on a network
  interface.
- **No request body schema validation.** If the agent posts something
  that isn't an Anthropic/OpenAI shape, fields like `model` and
  `messages` come back as `null` in the recorded event. The body is
  still recorded in full so the auditor has it.

## What "captured" means

After running an agent through the proxy:

```sh
RUN=$(curl -s "$LEDGER/runs?agent=model-proxy" | jq -r '.[0].id')
curl -s "$LEDGER/runs/$RUN/dossier" | jq '.events | group_by(.kind) | map({kind: .[0].kind, count: length})'
```

Expect roughly equal counts of `model_request` and `model_response`,
plus the run's `created` event. Each `model_request` body shows the
prompt the agent sent (system + messages); each `model_response` body
shows what came back including any `tool_use` blocks the model emitted.

## What this does NOT capture

- Actions the agent takes without consulting the model — hardcoded
  fallbacks, retries handled in the harness itself, file ops the
  harness does for housekeeping. **Use Tier 3 (`trackward-trace`) to
  catch these.** Together, the two tiers give you the
  intent/action audit trail in full.
- Model API calls made through a different code path (e.g., the
  agent has a hardcoded fallback URL). Mitigate by setting
  `ANTHROPIC_BASE_URL` *and* blocking direct egress to
  `api.anthropic.com` at the firewall.
