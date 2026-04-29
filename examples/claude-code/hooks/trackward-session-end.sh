#!/usr/bin/env bash
# SessionEnd hook: create a case for this session and export the signed
# bundle. The bundle path is printed on stdout for the operator's logs.

set -euo pipefail

LEDGER="${TRACKWARD_LEDGER_URL:-http://localhost:3000}"
ACTOR="${TRACKWARD_ACTOR:-claude-code@$(whoami)}"
TOKEN="${TRACKWARD_LEDGER_TOKEN:-}"
BUNDLE_DIR="${TRACKWARD_BUNDLE_DIR:-/tmp}"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // "unknown"')
REASON=$(echo "$INPUT" | jq -r '.reason // "unknown"')

STATE="/tmp/trackward-claude-${SESSION_ID}.json"
[[ -f "$STATE" ]] || exit 0
RUN=$(jq -r .run_id "$STATE")

declare -a AUTH=()
[[ -n "$TOKEN" ]] && AUTH=(-H "Authorization: Bearer $TOKEN")

# Final session_end event.
curl -fsS -X POST "$LEDGER/runs/$RUN/events" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n --arg r "$REASON" '{kind: "session_end", body: {reason: $r}}')" \
  >/dev/null

# Open a case with this run as evidence.
CASE=$(curl -fsS -X POST "$LEDGER/cases" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n --arg sid "$SESSION_ID" --arg actor "$ACTOR" \
    '{title: ("claude-code-" + $sid), opened_by: $actor}')" \
  | jq -r .id)

curl -fsS -X POST "$LEDGER/cases/$CASE/evidence" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n --arg run "$RUN" --arg actor "$ACTOR" \
    '{evidence_type: "run", evidence_id: $run, linked_by: $actor}')" \
  >/dev/null

BUNDLE="$BUNDLE_DIR/trackward-claude-${SESSION_ID}.bundle.json"
curl -fsS -X POST "$LEDGER/cases/$CASE/exports" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n --arg actor "$ACTOR" '{signed_by: $actor}')" \
  > "$BUNDLE"

echo "trackward bundle: $BUNDLE"
rm -f "/tmp/trackward-claude-${SESSION_ID}-pending.jsonl" "$STATE"
