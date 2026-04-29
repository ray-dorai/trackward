#!/usr/bin/env bash
# SessionStart hook: mint a Trackward run for this Claude Code session
# and stash the run_id in a session-state file the other hooks read.
#
# Hook input (stdin JSON): { session_id, source, cwd, ... }
# Output: nothing on stdout (would be injected as context). State to /tmp.

set -euo pipefail

LEDGER="${TRACKWARD_LEDGER_URL:-http://localhost:3000}"
ACTOR="${TRACKWARD_ACTOR:-claude-code@$(whoami)}"
TOKEN="${TRACKWARD_LEDGER_TOKEN:-}"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // "unknown"')
SOURCE=$(echo "$INPUT" | jq -r '.source // "unknown"')
CWD=$(echo "$INPUT" | jq -r '.cwd // ""')

STATE="/tmp/trackward-claude-${SESSION_ID}.json"

declare -a AUTH=()
[[ -n "$TOKEN" ]] && AUTH=(-H "Authorization: Bearer $TOKEN")

# If a run already exists for this session (resume), keep it.
if [[ -f "$STATE" ]]; then
  exit 0
fi

RUN=$(curl -fsS -X POST "$LEDGER/runs" \
  -H "x-trackward-actor: $ACTOR" \
  -H "content-type: application/json" \
  "${AUTH[@]}" \
  -d "$(jq -n \
    --arg sid "$SESSION_ID" \
    --arg src "$SOURCE" \
    --arg cwd "$CWD" \
    '{agent: "claude-code", metadata: {claude_session_id: $sid, source: $src, cwd: $cwd}}')" \
  | jq -r .id)

jq -n --arg run "$RUN" --arg sid "$SESSION_ID" \
  '{run_id: $run, claude_session_id: $sid}' > "$STATE"
