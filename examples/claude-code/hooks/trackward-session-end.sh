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
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // ""')
REASON=$(echo "$INPUT" | jq -r '.reason // "unknown"')

STATE="/tmp/trackward-claude-${SESSION_ID}.json"
[[ -f "$STATE" ]] || exit 0
RUN=$(jq -r .run_id "$STATE")

# Final snapshot pass before we close out — catches any transcript lines
# written between the last PostToolUse/Stop and now.
echo "$INPUT" | "$(dirname "$0")/trackward-snapshot.sh" || true

# Upload the raw transcript as an artifact so the dossier carries the
# complete original record, not just our parsed events. The artifact's
# sha256 lands in the hash chain via tool_invocations / events that
# reference it; a future auditor can re-derive every event from the
# transcript file.
TRANSCRIPT_ARTIFACT_ID=""
if [[ -n "$TRANSCRIPT" && -f "$TRANSCRIPT" ]]; then
  TRANSCRIPT_ARTIFACT_ID=$(curl -fsS -X POST "$LEDGER/artifacts" \
    -H "x-trackward-actor: $ACTOR" \
    "${AUTH[@]:-}" \
    -F "run_id=$RUN" \
    -F "label=claude-code-transcript" \
    -F "media_type=application/x-ndjson" \
    -F "file=@${TRANSCRIPT};filename=transcript.jsonl" \
    | jq -r .id 2>/dev/null || echo "")
fi

declare -a AUTH=()
[[ -n "$TOKEN" ]] && AUTH=(-H "Authorization: Bearer $TOKEN")

# Final session_end event, includes a reference to the transcript artifact
# so the dossier explicitly points to the ground-truth file.
curl -fsS -X POST "$LEDGER/runs/$RUN/events" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n --arg r "$REASON" --arg t "$TRANSCRIPT_ARTIFACT_ID" \
    '{kind: "session_end", body: {reason: $r, transcript_artifact_id: $t}}')" \
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
