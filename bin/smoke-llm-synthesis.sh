#!/usr/bin/env bash
# End-to-end: run adr-server with ADR_LLM on, post /analyze for glide-mq #181,
# wait for the job, and print the final flow source tags so we can confirm
# the LLM path engaged (vs. the structural fallback).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASE="${ADR_BASE:-C:/Users/avife/AppData/Local/Temp/glide-mq-base-181}"
HEAD="${ADR_HEAD:-C:/Users/avife/AppData/Local/Temp/glide-mq-head-181}"
MODEL="${ADR_MODEL:-gemma4:26b-a4b-it-q4_K_M}"
# Precedence:
#   1. ADR_LLM (set explicitly by caller) wins.
#   2. If not, and ADR_GLM_API_KEY is set, let adr-server apply its
#      key-based default (→ glm:glm-4.7) — export nothing here.
#   3. Otherwise, default to ollama:$ADR_MODEL so local runs still work.
if [ -n "${ADR_LLM:-}" ]; then
  ADR_LLM_LOCAL="$ADR_LLM"
elif [ -n "${ADR_GLM_API_KEY:-}" ]; then
  ADR_LLM_LOCAL=""    # adr-server picks glm:glm-4.7 via key-default
else
  ADR_LLM_LOCAL="ollama:$MODEL"
fi
PORT="${ADR_PORT:-8788}"

# Cargo binaries live under the workspace target dir — discover it.
TARGET_DIR="$(cargo metadata --manifest-path "$ROOT/Cargo.toml" --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

echo "▶ target dir: $TARGET_DIR"
echo "▶ base:       $BASE"
echo "▶ head:       $HEAD"
echo "▶ llm:        ${ADR_LLM_LOCAL:-<key-default>}"
echo "▶ port:       $PORT"

# Build once — both bins need to be fresh.
(cd "$ROOT" && cargo build -p adr-server -p adr-mcp)

# Launch the server in the background with ADR_LLM set. Point ADR_MCP_BIN
# at the freshly built child binary so PATH lookup doesn't matter.
LOGFILE="$(mktemp -t adr-server-smoke.XXXXXX.log)"
if [ -n "$ADR_LLM_LOCAL" ]; then
  export ADR_LLM="$ADR_LLM_LOCAL"
fi
export ADR_MCP_BIN="$TARGET_DIR/debug/adr-mcp.exe"
export ADR_REPO_ROOT="$(cygpath -w "$ROOT" 2>/dev/null || echo "$ROOT")"
export RUST_LOG="${RUST_LOG:-adr_server=info,adr_mcp=info}"

echo "▶ logs:       $LOGFILE"
ADR_PORT="$PORT" "$TARGET_DIR/debug/adr-server.exe" > "$LOGFILE" 2>&1 &
SERVER_PID=$!
trap "echo; echo '▶ stopping server (pid $SERVER_PID)'; kill $SERVER_PID 2>/dev/null || true" EXIT

# Wait up to 15s for the server to come up.
for i in $(seq 1 15); do
  if curl -s "http://localhost:$PORT/health" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "▶ POST /analyze …"
BODY=$(printf '{"base_path":"%s","head_path":"%s"}' "$BASE" "$HEAD")
JOB_JSON=$(curl -s -X POST "http://localhost:$PORT/analyze" \
  -H 'Content-Type: application/json' -d "$BODY")
JOB_ID=$(printf '%s' "$JOB_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin)["job_id"])')
echo "  job_id: $JOB_ID"

echo "▶ polling status…"
for i in $(seq 1 600); do
  STATUS=$(curl -s "http://localhost:$PORT/analyze/$JOB_ID" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("status"))')
  if [ "$STATUS" = "ready" ]; then
    break
  fi
  if [ "$STATUS" = "error" ]; then
    echo "  job errored"
    curl -s "http://localhost:$PORT/analyze/$JOB_ID" | python3 -m json.tool
    exit 1
  fi
  printf '\r  %ds elapsed  status=%s' "$i" "$STATUS"
  sleep 1
done
echo
echo "▶ ready — flow source tags:"
curl -s "http://localhost:$PORT/analyze/$JOB_ID" | python3 -c '
import json, sys
d = json.load(sys.stdin)
flows = (d.get("artifact") or {}).get("flows") or []
for f in flows:
    src = f["source"]
    k = src.get("kind", "?")
    m = src.get("model", "")
    name = f["name"]
    print("  - " + name.ljust(40) + " " + k + ":" + m)
'

echo
echo "▶ last 40 log lines:"
tail -40 "$LOGFILE" | sed "s|^|  |"
