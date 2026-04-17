#!/usr/bin/env bash
# End-to-end smoke test: start adr-server, fire an analyze request against a
# fixture PR, poll until ready, dump the resulting artifact. Exits non-zero on
# any failure. Cleans up the background server on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# On git-bash / MSYS, /c/foo paths aren't understood by native Rust PathBuf.
# Prefer Windows-style paths when we're on those shells.
if command -v cygpath >/dev/null 2>&1; then
  ROOT_NATIVE="$(cygpath -w "$ROOT")"
else
  ROOT_NATIVE="$ROOT"
fi

SLUG="${1:-pr-0004-combined}"
PORT="${ADR_PORT:-8787}"
CACHE_DIR="$(mktemp -d -t adr-cache.XXXXXX)"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$CACHE_DIR"
}
trap cleanup EXIT

echo "▶ building adr-server"
cargo build -q -p adr-server

echo "▶ starting server on :$PORT (cache=$CACHE_DIR)"
ADR_CACHE_DIR="$CACHE_DIR" ADR_PORT="$PORT" \
  cargo run -q -p adr-server &
SERVER_PID=$!

# Wait for /health to come up (up to ~5s).
for _ in $(seq 1 50); do
  if curl -sf "http://127.0.0.1:$PORT/health" >/dev/null; then break; fi
  sleep 0.1
done

BASE="$ROOT_NATIVE/fixtures/$SLUG/base"
HEAD="$ROOT_NATIVE/fixtures/$SLUG/head"
# JSON needs forward slashes or escaped backslashes; normalize to forward.
BASE="${BASE//\\//}"
HEAD="${HEAD//\\//}"
echo "▶ POST /analyze  base=$BASE  head=$HEAD"
RESP="$(curl -sf -X POST "http://127.0.0.1:$PORT/analyze" \
  -H 'content-type: application/json' \
  -d "{\"base_path\":\"$BASE\",\"head_path\":\"$HEAD\"}")"
JOB_ID="$(echo "$RESP" | sed -n 's/.*"job_id":"\([^"]*\)".*/\1/p')"
echo "   job_id=$JOB_ID"

echo "▶ polling /analyze/$JOB_ID (up to 30s)"
for _ in $(seq 1 300); do
  STATUS="$(curl -sf "http://127.0.0.1:$PORT/analyze/$JOB_ID" | sed -n 's/.*"status":"\([^"]*\)".*/\1/p')"
  case "$STATUS" in
    ready) echo "   ready"; break ;;
    error) echo "   ERROR"; exit 1 ;;
    *) sleep 0.1 ;;
  esac
done

echo "▶ hunks in artifact:"
curl -sf "http://127.0.0.1:$PORT/analyze/$JOB_ID" \
  | grep -oE '"source":[[:space:]]*"adr-hunks/[^"]+"' | sort -u | sed 's/^/  /'

echo "✓ demo ok"
