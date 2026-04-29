#!/usr/bin/env bash
# End-to-end demo: simulate an agent task that flows through the gateway,
# logs a model exchange, runs a "tool", exports a signed bundle, and
# verifies it offline.
#
# Prereqs (script will check):
#   - docker compose
#   - cargo
#   - curl, jq, python3
#
# Adapt the "agent loop" section for your actual agent harness; the
# rest is wiring you'd use unchanged.

set -euo pipefail

LEDGER_URL="${LEDGER_URL:-http://localhost:3000}"
GATEWAY_URL="${GATEWAY_URL:-http://localhost:4000}"
ECHO_PORT="${ECHO_PORT:-9999}"
ACTOR="${ACTOR:-demo@local}"
DEMO_DIR="${DEMO_DIR:-./demo-$(date -u +%Y%m%dT%H%M%SZ)}"

cd "$(git rev-parse --show-toplevel)"
mkdir -p "$DEMO_DIR"

require() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1"; exit 2; }; }
require docker; require cargo; require curl; require jq; require python3

say() { printf '\n=== %s ===\n' "$*"; }
cleanup() {
  set +e
  [[ -n "${ECHO_PID:-}" ]] && kill "$ECHO_PID" 2>/dev/null
  [[ -n "${GATEWAY_PID:-}" ]] && kill "$GATEWAY_PID" 2>/dev/null
  [[ -n "${LEDGER_PID:-}" ]] && kill "$LEDGER_PID" 2>/dev/null
}
trap cleanup EXIT

# -----------------------------------------------------------------------------
say "1. bring up postgres + minio (docker compose)"
docker compose up -d postgres minio createbucket >/dev/null
for _ in $(seq 1 30); do
  curl -fsS http://localhost:9000/minio/health/live >/dev/null 2>&1 && break
  sleep 1
done

# -----------------------------------------------------------------------------
say "2. start an echo 'tool' backend on :$ECHO_PORT"
cat > "$DEMO_DIR/echo.py" <<'PY'
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        body = self.rfile.read(n).decode() if n else ""
        try: payload = json.loads(body) if body else {}
        except json.JSONDecodeError: payload = {"raw": body}
        out = json.dumps({"echoed": payload}).encode()
        self.send_response(200); self.send_header("content-type","application/json")
        self.send_header("content-length", str(len(out))); self.end_headers()
        self.wfile.write(out)
HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PY
python3 "$DEMO_DIR/echo.py" "$ECHO_PORT" >"$DEMO_DIR/echo.log" 2>&1 &
ECHO_PID=$!
sleep 0.5

# -----------------------------------------------------------------------------
say "3. start ledger + gateway (cargo run, plaintext, no auth)"
export DATABASE_URL="${DATABASE_URL:-postgres://trackward:trackward@localhost:5432/trackward?sslmode=disable}"
export S3_BUCKET="${S3_BUCKET:-trackward-artifacts}"
export S3_ENDPOINT="${S3_ENDPOINT:-http://localhost:9000}"
export S3_REGION="${S3_REGION:-us-east-1}"
export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-minioadmin}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-minioadmin}"
export LEDGER_SIGNING_KEY_HEX="${LEDGER_SIGNING_KEY_HEX:-0000000000000000000000000000000000000000000000000000000000000001}"
export LEDGER_DEFAULT_ACTOR="${LEDGER_DEFAULT_ACTOR:-$ACTOR}"
export TOOL_ROUTES="echo=http://127.0.0.1:$ECHO_PORT"

cargo build --release -p ledger -p gateway -p verifier --quiet
target/release/ledger >"$DEMO_DIR/ledger.log" 2>&1 &
LEDGER_PID=$!
for _ in $(seq 1 30); do
  curl -fsS "$LEDGER_URL/health" >/dev/null 2>&1 && break
  sleep 0.5
done
target/release/gateway >"$DEMO_DIR/gateway.log" 2>&1 &
GATEWAY_PID=$!
for _ in $(seq 1 30); do
  curl -fsS "$GATEWAY_URL/health" >/dev/null 2>&1 && break
  sleep 0.5
done
echo "  ledger pid=$LEDGER_PID, gateway pid=$GATEWAY_PID"

# -----------------------------------------------------------------------------
say "4. agent loop: tool call + model exchange events"
RESP_HEADERS="$DEMO_DIR/tool-headers.txt"
RESP_BODY=$(curl -fsS -D "$RESP_HEADERS" \
  -X POST "$GATEWAY_URL/tool/echo" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d '{"command":"ls /tmp"}')
RUN=$(grep -i "^x-trackward-run-id:" "$RESP_HEADERS" | tr -d '\r' | awk '{print $2}')
echo "  run=$RUN"
echo "  tool response: $RESP_BODY"

curl -fsS -X POST "$LEDGER_URL/runs/$RUN/events" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d '{
    "kind":"model_request",
    "body":{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"list /tmp"}]}
  }' >/dev/null
curl -fsS -X POST "$LEDGER_URL/runs/$RUN/events" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d '{
    "kind":"model_response",
    "body":{"stop_reason":"end_turn","content":[{"type":"text","text":"Done."}]}
  }' >/dev/null

# -----------------------------------------------------------------------------
say "5. create a case + export signed bundle"
CASE=$(curl -fsS -X POST "$LEDGER_URL/cases" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d "{\"title\":\"demo-$(date -u +%s)\",\"run_id\":\"$RUN\"}" | jq -r .id)
echo "  case=$CASE"

curl -fsS -X POST "$LEDGER_URL/cases/$CASE/exports" \
  -H "x-trackward-actor: $ACTOR" \
  > "$DEMO_DIR/bundle.json"
echo "  bundle written: $DEMO_DIR/bundle.json ($(wc -c <"$DEMO_DIR/bundle.json") bytes)"

# -----------------------------------------------------------------------------
say "6. verify offline"
target/release/verifier "$DEMO_DIR/bundle.json"

# -----------------------------------------------------------------------------
say "demo complete"
echo "  artifacts: $DEMO_DIR"
echo "  bundle:    $DEMO_DIR/bundle.json"
echo "  logs:      $DEMO_DIR/{ledger,gateway,echo}.log"
echo
echo "  inspect the run:"
echo "    curl -s $LEDGER_URL/runs/$RUN/dossier | jq ."
