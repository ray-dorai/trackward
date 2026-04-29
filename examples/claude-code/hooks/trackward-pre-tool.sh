#!/usr/bin/env bash
# PreToolUse hook: record `tool_call` event before the tool runs.
#
# Stash the call_id in a per-session pending file so PostToolUse can
# correlate the result. Exit 0 to allow; exit 2 to block (we don't
# block here — Trackward records, doesn't enforce policy in this hook).

set -euo pipefail

LEDGER="${TRACKWARD_LEDGER_URL:-http://localhost:3000}"
ACTOR="${TRACKWARD_ACTOR:-claude-code@$(whoami)}"
TOKEN="${TRACKWARD_LEDGER_TOKEN:-}"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // "unknown"')
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // "unknown"')
TOOL_INPUT=$(echo "$INPUT" | jq -c '.tool_input // {}')

STATE="/tmp/trackward-claude-${SESSION_ID}.json"
[[ -f "$STATE" ]] || exit 0
RUN=$(jq -r .run_id "$STATE")

# Mint a call_id locally so PreToolUse and PostToolUse can correlate.
CALL_ID="cc-$(date +%s%N)-$$"
PENDING="/tmp/trackward-claude-${SESSION_ID}-pending.jsonl"
jq -n --arg cid "$CALL_ID" --arg tool "$TOOL_NAME" --argjson input "$TOOL_INPUT" \
  '{call_id: $cid, tool: $tool, input: $input}' >> "$PENDING"

declare -a AUTH=()
[[ -n "$TOKEN" ]] && AUTH=(-H "Authorization: Bearer $TOKEN")

curl -fsS -X POST "$LEDGER/runs/$RUN/events" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n --arg cid "$CALL_ID" --arg tool "$TOOL_NAME" --argjson input "$TOOL_INPUT" \
    '{kind: "tool_call", body: {call_id: $cid, tool: $tool, input: $input}}')" \
  >/dev/null
