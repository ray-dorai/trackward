#!/usr/bin/env bash
# Drill: dump → nuke → restore → re-verify a known bundle.
#
# Targets the local docker-compose stack. Intended to be run regularly to
# prove that backup/restore actually works end-to-end and that the
# verifier still accepts the post-restore bundle.
#
# Adapt for production by swapping pg_dump/pg_restore against your real
# DATABASE_URL and pointing LEDGER_URL at a staging cluster.

set -euo pipefail

LEDGER_URL="${LEDGER_URL:-http://localhost:3000}"
DATABASE_URL="${DATABASE_URL:-postgres://trackward:trackward@localhost:5432/trackward?sslmode=disable}"
DRILL_DIR="${DRILL_DIR:-./drill-$(date -u +%Y%m%dT%H%M%SZ)}"
ACTOR="${ACTOR:-drill}"

mkdir -p "$DRILL_DIR"
cd "$DRILL_DIR"

say() { printf '\n=== %s ===\n' "$*"; }

require() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1"; exit 2; }; }
require curl
require jq
require pg_dump
require pg_restore
require psql
require cargo

say "preflight: ledger up?"
curl -fsS "$LEDGER_URL/healthz" >/dev/null

say "build verifier"
( cd "$(git rev-parse --show-toplevel)" && cargo build --release -p verifier --quiet )
VERIFIER="$(git rev-parse --show-toplevel)/target/release/verifier"

say "create a run + a few events + a case"
RUN=$(curl -fsS -X POST "$LEDGER_URL/runs" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d '{"agent":"drill"}' | jq -r .id)
echo "  run=$RUN"

for i in 1 2 3; do
  curl -fsS -X POST "$LEDGER_URL/runs/$RUN/events" \
    -H "x-trackward-actor: $ACTOR" \
    -H 'content-type: application/json' \
    -d "{\"kind\":\"drill\",\"payload\":{\"step\":$i}}" >/dev/null
done

CASE=$(curl -fsS -X POST "$LEDGER_URL/cases" \
  -H "x-trackward-actor: $ACTOR" \
  -H 'content-type: application/json' \
  -d "{\"title\":\"drill-$(date -u +%s)\",\"run_id\":\"$RUN\"}" | jq -r .id)
echo "  case=$CASE"

say "export pre-drill bundle"
curl -fsS -X POST "$LEDGER_URL/cases/$CASE/exports" \
  -H "x-trackward-actor: $ACTOR" \
  > pre.json
"$VERIFIER" pre.json

say "pg_dump"
pg_dump --format=custom --no-owner --no-acl \
  --dbname="$DATABASE_URL" --file=snapshot.pgdump
ls -lh snapshot.pgdump

say "nuke + recreate database"
DB_NAME=$(echo "$DATABASE_URL" | sed -E 's|.*/([^?]+).*|\1|')
ADMIN_URL=$(echo "$DATABASE_URL" | sed -E "s|/${DB_NAME}|/postgres|")
psql "$ADMIN_URL" -c "DROP DATABASE \"$DB_NAME\" WITH (FORCE);"
psql "$ADMIN_URL" -c "CREATE DATABASE \"$DB_NAME\";"

say "pg_restore"
pg_restore --no-owner --no-acl --dbname="$DATABASE_URL" snapshot.pgdump

say "re-export the same case from the restored DB"
echo "  (restart your ledger / docker compose restart ledger if connections are pooled)"
read -rp "  press enter once the ledger is back up: " _

curl -fsS -X POST "$LEDGER_URL/cases/$CASE/exports" \
  -H "x-trackward-actor: $ACTOR" \
  > post.json
"$VERIFIER" post.json

say "compare pre vs post"
diff <(jq -S 'del(.id, .created_at)' pre.json) \
     <(jq -S 'del(.id, .created_at)' post.json) \
  && echo "  pre and post bundles match (data-integrity check passes)" \
  || { echo "  MISMATCH — restore did not preserve canonical content"; exit 1; }

say "drill complete: $DRILL_DIR"
