#!/usr/bin/env python3
"""Poll a rebaselined job and print a compact Phase A diagnostic line
per iteration. Reports: status, hunk counts, call-edge counts + their
provenance source (tree-sitter vs adr-lsp), and detected packages."""
import json
import sys
import urllib.request

job = sys.argv[1]
iters = int(sys.argv[2]) if len(sys.argv) > 2 else 5
import time

for i in range(1, iters + 1):
    time.sleep(15)
    with urllib.request.urlopen(f"http://127.0.0.1:8787/analyze/{job}") as r:
        d = json.load(r)
    a = d.get("artifact") or {}
    hunks = a.get("hunks", [])
    call_hunks = [h for h in hunks if h.get("kind", {}).get("kind") == "call"]
    edges_b = [e for e in (a.get("base") or {}).get("edges", []) if e.get("kind") == "calls"]
    edges_h = [e for e in (a.get("head") or {}).get("edges", []) if e.get("kind") == "calls"]
    provs = sorted({e.get("provenance", {}).get("source") for e in edges_b + edges_h})
    b_nodes = (a.get("base") or {}).get("nodes", [])
    h_nodes = (a.get("head") or {}).get("nodes", [])
    pkgs = sorted({n.get("package") for n in b_nodes + h_nodes if n.get("package")})
    print(
        f"[{i}] status={d.get('status')} hunks={len(hunks)} "
        f"call_hunks={len(call_hunks)} edges_b={len(edges_b)} "
        f"edges_h={len(edges_h)} provs={provs} pkgs={pkgs}"
    )
