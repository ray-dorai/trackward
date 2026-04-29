#!/usr/bin/env bash
# Stop hook: turn boundary. Record the assistant's final message as a
# `model_response` event (extracted from the transcript file) and a
# `task_complete` marker.

set -euo pipefail

LEDGER="${TRACKWARD_LEDGER_URL:-http://localhost:3000}"
ACTOR="${TRACKWARD_ACTOR:-claude-code@$(whoami)}"
TOKEN="${TRACKWARD_LEDGER_TOKEN:-}"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // "unknown"')
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // ""')

STATE="/tmp/trackward-claude-${SESSION_ID}.json"
[[ -f "$STATE" ]] || exit 0
RUN=$(jq -r .run_id "$STATE")

declare -a AUTH=()
[[ -n "$TOKEN" ]] && AUTH=(-H "Authorization: Bearer $TOKEN")

# Pull the last assistant message from the transcript JSONL, if available.
LAST_ASSISTANT=""
if [[ -n "$TRANSCRIPT" && -f "$TRANSCRIPT" ]]; then
  LAST_ASSISTANT=$(tac "$TRANSCRIPT" | jq -c 'select(.message.role == "assistant") | .message' 2>/dev/null | head -1 || true)
fi

if [[ -n "$LAST_ASSISTANT" ]]; then
  curl -fsS -X POST "$LEDGER/runs/$RUN/events" \
    -H "x-trackward-actor: $ACTOR" \
    -H "content-type: application/json" \
    "${AUTH[@]}" \
    -d "$(jq -n --argjson m "$LAST_ASSISTANT" '{kind: "model_response", body: $m}')" \
    >/dev/null
fi

curl -fsS -X POST "$LEDGER/runs/$RUN/events" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d '{"kind": "task_complete", "body": {}}' \
  >/dev/null
