#!/usr/bin/env bash
# Start both adr-server (:8787) and the Vite dev server (:5173). Kill everything
# on Ctrl-C. Frontend's Vite proxy forwards /analyze and /health to the backend,
# so the app just fetches relative URLs.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cleanup() {
  if [[ -n "${BACKEND_PID:-}" ]]; then kill "$BACKEND_PID" 2>/dev/null || true; fi
  if [[ -n "${FRONTEND_PID:-}" ]]; then kill "$FRONTEND_PID" 2>/dev/null || true; fi
  wait 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "▶ building + starting adr-server on :8787"
cargo run -q -p adr-server &
BACKEND_PID=$!

echo "▶ starting vite on :5173"
( cd apps/web && npm run dev -- --host 127.0.0.1 ) &
FRONTEND_PID=$!

wait
