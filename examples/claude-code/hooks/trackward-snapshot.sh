#!/usr/bin/env bash
# Snapshot helper. Reads any unposted lines from this session's transcript
# and POSTs each as a `transcript_*` event in the ledger.
#
# Idempotent: maintains a per-session cursor at /tmp/trackward-claude-<sid>-cursor
# so each line is posted exactly once. Safe to call from any hook event;
# wired into UserPromptSubmit, PostToolUse, Stop, and SessionEnd so the
# transcript-only content (assistant text, thinking, system, attachments,
# permission changes, ...) gets captured progressively as the session runs
# rather than only at SessionEnd. That makes a crashed/killed session lose
# only the lines between the last hook fire and the crash, not the whole
# narrative.
#
# Realtime hooks (trackward-pre-tool, trackward-post-tool, trackward-user-
# prompt) capture tool_call/tool_result/user_message live. Transcript
# events use a `transcript_` prefix on `kind` so a dossier reader can
# distinguish "live audit signal" from "complete narrative replay" without
# having to dedupe by content.

set -euo pipefail

LEDGER="${TRACKWARD_LEDGER_URL:-http://localhost:3000}"
ACTOR="${TRACKWARD_ACTOR:-claude-code@$(whoami)}"
TOKEN="${TRACKWARD_LEDGER_TOKEN:-}"

INPUT=$(cat)
SID=$(echo "$INPUT" | jq -r '.session_id // ""')
T=$(echo "$INPUT" | jq -r '.transcript_path // ""')
[[ -z "$SID" || -z "$T" || ! -f "$T" ]] && exit 0

STATE="/tmp/trackward-claude-${SID}.json"
[[ -f "$STATE" ]] || exit 0
RUN=$(jq -r .run_id "$STATE")

CURSOR_FILE="/tmp/trackward-claude-${SID}-cursor"
LAST=$(cat "$CURSOR_FILE" 2>/dev/null || echo 0)
TOTAL=$(wc -l < "$T")
[[ "$LAST" -ge "$TOTAL" ]] && exit 0

declare -a AUTH=()
[[ -n "$TOKEN" ]] && AUTH=(-H "Authorization: Bearer $TOKEN")

post_event() {
  local kind="$1"
  local body_json="$2"
  curl -fsS -X POST "$LEDGER/runs/$RUN/events" \
    -H "x-trackward-actor: $ACTOR" \
    -H "content-type: application/json" \
    "${AUTH[@]}" \
    -d "$(jq -n --arg k "$kind" --argjson b "$body_json" '{kind: $k, body: $b}')" \
    >/dev/null || true
}

# Walk lines [LAST+1 .. TOTAL]. awk handles empty/malformed lines safely.
awk -v skip="$LAST" 'NR > skip' "$T" | while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  TYPE=$(echo "$line" | jq -r '.type // "unknown"' 2>/dev/null || echo unknown)

  case "$TYPE" in
    assistant)
      # Walk content[] — text/thinking/tool_use each become their own event.
      while IFS= read -r item; do
        [[ -z "$item" ]] && continue
        IT=$(echo "$item" | jq -r '.type // "unknown"')
        case "$IT" in
          text)      post_event "transcript_assistant_text"     "$item" ;;
          thinking)  post_event "transcript_assistant_thinking" "$item" ;;
          tool_use)  post_event "transcript_assistant_tool_use" "$item" ;;
          *)         post_event "transcript_assistant_${IT}"    "$item" ;;
        esac
      done < <(echo "$line" | jq -c '.message.content[]?' 2>/dev/null)
      ;;

    user|system|attachment|permission-mode|queue-operation|file-history-snapshot|last-prompt)
      # Sanitize hyphens in `kind` (ledger event kinds are free-form but
      # we keep them shell-safe for downstream queries).
      KIND="transcript_${TYPE//-/_}"
      post_event "$KIND" "$line"
      ;;

    *)
      post_event "transcript_unknown" "$line"
      ;;
  esac
done

echo "$TOTAL" > "$CURSOR_FILE"
