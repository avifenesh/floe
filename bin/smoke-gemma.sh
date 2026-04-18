#!/usr/bin/env bash
# Smoke-test: does Gemma 4 26B MoE understand the flow_synthesis prompt and
# produce a valid JSON flow list when we feed it glide-mq PR #181 hunks
# inline? No PI, no tools — just reasoning + structured output.
#
# Expected: 4 flows named by intent (multi-metric budget, streaming chunk,
# suspend tweak, readStream reformat). JSON parse clean, all 13 hunks
# covered, no reserved names.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MOCK_DATA="$ROOT/fixtures/smoke-glide-mq-181.json"
RENDERED=$(mktemp)
trap "rm -f $RENDERED" EXIT

PROMPT=$(cat "$ROOT/prompts/flow_synthesis.md")
PROMPT="${PROMPT//\{\{hunk_count\}\}/13}"
PROMPT="${PROMPT//\{\{initial_cluster_count\}\}/5}"
PROMPT="${PROMPT//\{\{max_tool_calls\}\}/200}"

HUNKS=$(cat "$MOCK_DATA")

cat > "$RENDERED" <<EOF
$PROMPT

---

# Smoke-test override

Tool calls are disabled for this smoke run. Instead of calling \`adr:list_hunks()\` or \`adr:list_flows_initial()\`, use the inline data below. Instead of calling \`adr:propose_flow\`, return your final flows as one JSON block at the end of your response, wrapped in \`\`\`json fences.

Expected JSON shape:
\`\`\`json
{
  "flows": [
    { "name": "...", "rationale": "...", "hunk_ids": ["..."] }
  ]
}
\`\`\`

Every hunk id from the data below must appear in at least one flow.

## Inline data (what \`adr:list_hunks()\` and \`adr:list_flows_initial()\` would return)

\`\`\`json
$HUNKS
\`\`\`

Classify now.
EOF

echo "▶ rendered prompt: $(wc -c < "$RENDERED") bytes"
echo "▶ running gemma4:26b-a4b-it-q4_K_M (may take ~30s first token)"
echo ""

MODEL="${ADR_MODEL:-gemma4:26b-a4b-it-q4_K_M}"
powershell.exe -Command "Get-Content '$(cygpath -w "$RENDERED")' | ollama run $MODEL" 2>&1
