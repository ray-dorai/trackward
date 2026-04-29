#!/usr/bin/env bash
# PostToolUse hook: record `tool_result` event and a tool_invocation row.
# Pulls the matching call_id from the per-session pending file written
# by PreToolUse.

set -euo pipefail

LEDGER="${TRACKWARD_LEDGER_URL:-http://localhost:3000}"
ACTOR="${TRACKWARD_ACTOR:-claude-code@$(whoami)}"
TOKEN="${TRACKWARD_LEDGER_TOKEN:-}"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // "unknown"')
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // "unknown"')
TOOL_INPUT=$(echo "$INPUT" | jq -c '.tool_input // {}')
TOOL_RESPONSE=$(echo "$INPUT" | jq -c '.tool_response // {}')

STATE="/tmp/trackward-claude-${SESSION_ID}.json"
[[ -f "$STATE" ]] || exit 0
RUN=$(jq -r .run_id "$STATE")

# Pull the most-recent matching pending call (LIFO; tool calls are
# typically synchronous, so the last queued is the one resolving).
PENDING="/tmp/trackward-claude-${SESSION_ID}-pending.jsonl"
CALL_ID="cc-fallback-$(date +%s%N)-$$"
if [[ -f "$PENDING" ]]; then
  MATCH=$(grep -F "\"tool\":\"$TOOL_NAME\"" "$PENDING" | tail -1 || true)
  if [[ -n "$MATCH" ]]; then
    CALL_ID=$(echo "$MATCH" | jq -r .call_id)
    # Drop that line.
    grep -vF "\"call_id\":\"$CALL_ID\"" "$PENDING" > "$PENDING.tmp" && mv "$PENDING.tmp" "$PENDING"
  fi
fi

declare -a AUTH=()
[[ -n "$TOKEN" ]] && AUTH=(-H "Authorization: Bearer $TOKEN")

curl -fsS -X POST "$LEDGER/runs/$RUN/events" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n --arg cid "$CALL_ID" --arg tool "$TOOL_NAME" --argjson out "$TOOL_RESPONSE" \
    '{kind: "tool_result", body: {call_id: $cid, tool: $tool, output: $out}}')" \
  >/dev/null

curl -fsS -X POST "$LEDGER/tool-invocations" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n \
    --arg run "$RUN" --arg tool "$TOOL_NAME" \
    --argjson input "$TOOL_INPUT" --argjson out "$TOOL_RESPONSE" \
    '{run_id: $run, tool: $tool, input: $input, output: $out, status: "ok"}')" \
  >/dev/null
