#!/usr/bin/env bash
# Tiny adapter: wrap a single tool call so an agent's existing harness
# can route through the gateway with a one-line change.
#
# Usage:
#   trackward-tool <tool-name> <json-input>
#
# Reads from env:
#   GATEWAY_URL          (default: http://localhost:4000)
#   GATEWAY_AUTH_TOKEN   (optional bearer)
#   ACTOR                (default: $USER@$(hostname))
#   TRACKWARD_RUN_ID     (optional; reused across calls in the same task)
#
# Stdout: the backend's response body (so the agent sees it as if it
# had called the tool directly).
# Stderr: short status line.
# Side effect: TRACKWARD_RUN_ID is exported back to the caller via
# `eval $(trackward-tool ...)` if the caller wants run-id propagation.

set -euo pipefail

TOOL="${1:?usage: trackward-tool <tool> <json>}"
INPUT="${2:?usage: trackward-tool <tool> <json>}"

GATEWAY_URL="${GATEWAY_URL:-http://localhost:4000}"
ACTOR="${ACTOR:-${USER}@$(hostname -s)}"

declare -a HEADERS=(
  -H "x-trackward-actor: $ACTOR"
  -H "content-type: application/json"
)
[[ -n "${GATEWAY_AUTH_TOKEN:-}" ]] && HEADERS+=(-H "Authorization: Bearer $GATEWAY_AUTH_TOKEN")
[[ -n "${TRACKWARD_RUN_ID:-}" ]] && HEADERS+=(-H "x-trackward-run-id: $TRACKWARD_RUN_ID")

HEADERS_FILE=$(mktemp)
trap 'rm -f "$HEADERS_FILE"' EXIT

BODY=$(curl -fsS -D "$HEADERS_FILE" \
  -X POST "$GATEWAY_URL/tool/$TOOL" \
  "${HEADERS[@]}" \
  -d "$INPUT")

RUN=$(grep -i "^x-trackward-run-id:" "$HEADERS_FILE" | tr -d '\r' | awk '{print $2}')

echo "$BODY"
echo "trackward: tool=$TOOL run=$RUN" >&2

# Emit a shell-eval line so callers can pick up the run-id:
#   eval "$(trackward-tool ... 2>&1 >/dev/null | grep ^export)"
echo "export TRACKWARD_RUN_ID=$RUN" >&2
