#!/usr/bin/env bash
# Smoke-test: exercise the adr-mcp server over stdio and confirm tool dispatch.
#
# Picks the largest flows-carrying artifact from .adr/cache, launches the
# server against it, and pipes a minimal JSON-RPC trace (initialize → list
# tools → list hunks → propose/rename → finalize). Prints the finalize
# outcome line-by-line; fails loudly on anything but "accepted".
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${ADR_MCP_BIN:-$(cargo metadata --manifest-path "$ROOT/Cargo.toml" --format-version 1 | python3 -c 'import json,sys; m=json.load(sys.stdin); print(m["target_directory"])')/debug/adr-mcp.exe}"

if [ ! -x "$BIN" ]; then
  echo "building adr-mcp…"
  (cd "$ROOT" && cargo build -p adr-mcp)
fi

# Pick a cached artifact with flows. Largest = most interesting.
ARTIFACT="$(ls -S "$ROOT/.adr/cache/"*.json 2>/dev/null | head -1 || true)"
if [ -z "$ARTIFACT" ]; then
  echo "no cached artifact found under .adr/cache — run the server against a fixture first." >&2
  exit 1
fi

echo "▶ artifact: $ARTIFACT"
echo "▶ bin:      $BIN"
echo ""

printf '%s\n%s\n%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"adr.list_hunks","arguments":{}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"adr.finalize","arguments":{}}}' \
  | "$BIN" --artifact "$ARTIFACT" --model "${ADR_MODEL:-gemma4:26b-a4b-it-q4_K_M}" --runtime-version "${ADR_RUNTIME_VERSION:-0.3.0}" 2>/dev/null \
  | python3 -c '
import json, sys
for line in sys.stdin:
    resp = json.loads(line)
    if resp.get("id") == 1:
        print("[ok] initialize    ", resp["result"]["serverInfo"])
    elif resp.get("id") == 2:
        print("[ok] tools/list    ", len(resp["result"]["tools"]), "tools")
    elif resp.get("id") == 3:
        text = resp["result"]["content"][0]["text"]
        hunks = json.loads(text)
        print("[ok] list_hunks    ", len(hunks), "hunks")
    elif resp.get("id") == 4:
        text = resp["result"]["content"][0]["text"]
        payload = json.loads(text)
        outcome = payload["outcome"]
        mark = "ok" if outcome == "accepted" else "FAIL"
        print("[" + mark + "] finalize       " + outcome)
        for f in payload.get("flows", []):
            src = f["source"]
            k = src["kind"]
            m = src.get("model", "")
            v = src.get("version", "")
            name = f["name"]
            print("    - " + name.ljust(40) + " " + k + ":" + m + "@" + v)
        sys.exit(0 if outcome == "accepted" else 1)
'
