# Learning Guide: Rust Code Analysis Pipeline (Phases B-D)

**Generated**: 2026-04-23
**Sources**: 38 resources analyzed
**Depth**: deep
**Context**: Designed for the `floe` architectural PR-review tool. Phase A (tree-sitter-rust node extraction with byte spans) is already shipped. This guide covers Phases B-D: resolved call graph, compile diagnostics, workspace topology, test/bench output, unsafe detection, and claim anchoring.

---

## Prerequisites

- Rust toolchain installed (stable + nightly for some features)
- `cargo`, `rustup` available
- Familiarity with async Rust (tokio)
- Phase A complete: tree-sitter-rust extracts Function/Type/Enum/Trait nodes with byte spans

## TL;DR

- **Call graph**: No production-ready in-process solution exists in 2026. Best path is `rust-analyzer scip` CLI (batch, no LSP handshake needed) + parse the SCIP protobuf/JSON for caller to callee edges. LSP subprocess (`callHierarchy`) is viable for per-function queries.
- **Diagnostics**: `cargo check --message-format=json` is stable, well-documented, and the canonical path. Clippy adds lint-level diagnostics on the same wire format. Run on base + head snapshots separately; incremental cache means the second run is fast.
- **Workspace topology**: `cargo metadata --format-version=1` is the only stable, official source. Use the `cargo_metadata` crate for in-process parsing.
- **Test output**: `cargo-nextest` with `--profile ci` emits JUnit XML (stable). Parse with `quick-junit`. Raw `cargo test --format=json` is nightly-only and not yet stable.
- **Unsafe + concurrency detection**: Use tree-sitter queries or `syn` AST walks for source-level detection. `cargo-geiger` gives per-crate counts via subprocess.
- **Claim anchoring**: Shell to `rust-analyzer scip` for batch use; LSP subprocess for interactive per-position queries. The `ra_ap_*` crates are usable in-process but carry 0.0.x instability risk and require pinning.

---

## Q1: Resolved Call Graph

### The Landscape (2026 Reality)

No mature, production-ready Rust call graph tool exists that handles generics, macros, and multi-crate workspaces reliably. The options in order of viability:

| Approach | Status | Generics | Macros | Cross-crate | Stability |
|----------|--------|----------|--------|-------------|-----------|
| `rust-analyzer scip` CLI | **Best viable path** | Partial (pre-mono) | Yes | Yes | Stable binary |
| LSP `callHierarchy` | Viable per-position | Partial | Yes | Yes | Stable protocol |
| `ra_ap_ide` in-process | Usable but fragile | Partial | Yes | Yes | 0.0.x -- no semver |
| `rustc_public` (stable MIR) | Nightly-only, active dev | Full mono | Yes | Yes | Nightly, no semver |
| `cargo-callgraph` | **Abandoned** -- locked to 2021 nightly | -- | -- | -- | Dead |
| `syn` source-level | Works | No | Limited | No | Stable |
| `rustdoc --output-format=json` | Public API surface only | Partial | No | Yes | Nightly flag |

### Path A: `rust-analyzer scip` (Recommended for Batch)

rust-analyzer ships a `scip` CLI subcommand that performs full workspace analysis and emits a SCIP (Source Code Intelligence Protocol) protobuf index. This is what Sourcegraph uses in production.

**Invocation**:
```
rust-analyzer scip /path/to/workspace/root \
  --output /tmp/index.scip \
  --exclude-vendored-libraries
```

**What it produces**: A binary protobuf `index.scip` containing per-file `Document` entries. Each document lists `Occurrence` records (every token's position, role, and symbol moniker) plus `SymbolInformation` entries with relationships.

**Converting to JSON for inspection**:
```
scip print --json index.scip > index.json
```

**Deserializing in Rust** using the `scip` crate (`scip = "0.3"`):

```rust
use scip::types::Index;
use protobuf::Message;

let bytes = std::fs::read("index.scip")?;
let index = Index::parse_from_bytes(&bytes)?;

for doc in &index.documents {
    for occ in &doc.occurrences {
        // occ.symbol  = canonical moniker string
        // occ.symbol_roles  contains SymbolRole::Definition flag
    }
}
```

The `scip-callgraph` project (github.com/Beneficial-AI-Foundation/scip-callgraph) demonstrates reconstructing caller-to-callee edges from SCIP data. It reached v5.0.0 with CI and outputs DOT/SVG/D3 graphs.

**Limitations**: SCIP records references and definitions but does not directly label "this call site invokes this function" as a first-class edge type. You reconstruct the graph by joining call-site occurrences (SymbolRole = Reference at an ExprCall position) with the definition monikers they resolve to. The reference graph is complete; the call-specific filtering requires checking the surrounding AST node type -- tree-sitter Phase A spans let you correlate.

### Path B: LSP `callHierarchy` Subprocess (Per-Function Queries)

For targeted queries ("what does function X call?"), the LSP call hierarchy protocol is simpler than parsing full SCIP.

**Subprocess spawn** (Rust pseudocode):
```rust
let mut child = Command::new("rust-analyzer")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()?;
```

All LSP messages are framed with `Content-Length: N\r\n\r\n` followed by the JSON body on stdout/stdin.

**Initialization sequence** (required before any queries):
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "rootUri": "file:///path/to/workspace",
    "capabilities": {
      "textDocument": { "callHierarchy": {} }
    },
    "initializationOptions": {
      "checkOnSave": false,
      "cargo": { "loadOutDirsFromCheck": false, "buildScripts": { "enable": false } },
      "procMacro": { "enable": false }
    }
  }
}
```

After receiving the response, send the `initialized` notification (no response):
```json
{ "jsonrpc": "2.0", "method": "initialized", "params": {} }
```

**Call hierarchy flow**:
```json
// Step 1: prepare -- resolves position to a CallHierarchyItem
{
  "jsonrpc": "2.0", "id": 2,
  "method": "textDocument/prepareCallHierarchy",
  "params": {
    "textDocument": { "uri": "file:///abs/path/src/lib.rs" },
    "position": { "line": 42, "character": 10 }
  }
}
// Response: [{ "name": "my_fn", "kind": 12, "uri": "...", "range": {...}, "selectionRange": {...} }]

// Step 2: outgoing calls from the returned item
{
  "jsonrpc": "2.0", "id": 3,
  "method": "callHierarchy/outgoingCalls",
  "params": {
    "item": { "name": "my_fn", "kind": 12, "uri": "...", "range": {...}, "selectionRange": {...} }
  }
}
// Response: [{ "to": { CallHierarchyItem }, "fromRanges": [{ "start": {...}, "end": {...} }] }]
```

**Known issue**: A panic in `callHierarchy/outgoingCalls` was reported mid-2024 (rust-analyzer issue #17762). Fixed in PR #17554. Pin rust-analyzer binary to >= 2024-10 release.

**Disabling proc-macros and build scripts** in `initializationOptions` (shown above) cuts workspace load time from 30s to 5-8s on real projects.

### Path C: `ra_ap_ide` In-Process Library

The `ra_ap_ide` crate (current: v0.0.329, published weekly) exposes `Analysis::call_hierarchy()` and `Analysis::find_all_refs()`.

```toml
[dependencies]
ra_ap_ide = "0.0.329"
ra_ap_ide_db = "0.0.329"
ra_ap_load_cargo = "0.0.329"
```

**Warning**: `0.0.x` means **no semver guarantees**. The API changes weekly. Production tools (Sourcegraph etc.) shell to the binary rather than linking `ra_ap_*` for this reason. Use only if you can commit to weekly version maintenance.

### Path D: `rustc_public` / Stable MIR (Nightly, Future)

The `rust-lang/project-stable-mir` project (renamed "Rustc Librarification") is building a semver-stable API over rustc's MIR. As of 2026-04 it is **nightly-only** with no stability guarantees. It exposes `TerminatorKind::Call` for monomorphized call graph construction -- the most accurate approach for generics. Not viable for production today; watch for stabilization.

---

## Q2: Compile-Unit Diagnostics

### Canonical Invocation

```
cargo check \
  --workspace \
  --all-targets \
  --message-format=json \
  2>/dev/null
```

Add clippy for lint-level diagnostics (same wire format):
```
cargo clippy \
  --workspace \
  --all-targets \
  --message-format=json \
  -- -W clippy::all \
  2>/dev/null
```

**Gotcha**: filter stdout to lines starting with `{`. Non-JSON progress lines (on stderr with `--message-format=json`) can bleed through in some cargo versions.

**Snapshot isolation**: use separate `--target-dir` for base and head to avoid incremental cache collisions:
```
cargo check --workspace --all-targets --message-format=json \
  --target-dir /tmp/adr-base-check
cargo check --workspace --all-targets --message-format=json \
  --target-dir /tmp/adr-head-check
```

### JSON Schema: Message Types

Every stdout line is a JSON object. The `reason` field is the discriminant:

**`compiler-message`** (diagnostic data):
```json
{
  "reason": "compiler-message",
  "package_id": "file:///path/to/pkg#0.1.0",
  "manifest_path": "/path/to/pkg/Cargo.toml",
  "target": {
    "kind": ["lib"],
    "crate_types": ["lib"],
    "name": "my_crate",
    "src_path": "/path/to/src/lib.rs",
    "edition": "2021",
    "doc": true,
    "doctest": true,
    "test": true
  },
  "message": { }
}
```

**`compiler-artifact`**: emitted when a crate finishes compiling -- not diagnostic data.

**`build-finished`**: `{"reason":"build-finished","success":true}` -- stop reading here.

### rustc Diagnostic Object Schema

The `message` field inside `compiler-message`:

```json
{
  "$message_type": "diagnostic",
  "message": "unused variable: `x`",
  "code": {
    "code": "unused_variables",
    "explanation": null
  },
  "level": "warning",
  "spans": [
    {
      "file_name": "src/lib.rs",
      "byte_start": 21,
      "byte_end": 22,
      "line_start": 2,
      "line_end": 2,
      "column_start": 9,
      "column_end": 10,
      "is_primary": true,
      "text": [
        {
          "text": "    let x = 123;",
          "highlight_start": 9,
          "highlight_end": 10
        }
      ],
      "label": null,
      "suggested_replacement": null,
      "suggestion_applicability": null,
      "expansion": null
    }
  ],
  "children": [],
  "rendered": "warning: unused variable: `x`\n  --> src/lib.rs:2:9\n..."
}
```

### Severity Levels

| `level` value | Meaning |
|--------------|---------|
| `"error"` | Fatal; compilation fails |
| `"warning"` | Potential issue |
| `"note"` | Supplementary context |
| `"help"` | Fix suggestion |
| `"failure-note"` | Context for a failure |
| `"error: internal compiler error"` | ICE -- compiler bug |

### `suggestion_applicability` Values

| Value | Meaning |
|-------|---------|
| `"MachineApplicable"` | Safe to apply automatically |
| `"MaybeIncorrect"` | Might be wrong but compiles |
| `"HasPlaceholders"` | User must fill in placeholders |
| `"Unspecified"` | Unknown confidence |

### Clippy-Specific Notes

Clippy lint codes are prefixed: `"clippy::needless_return"`. Filter `code.code` starting with `"clippy::"` to separate from rustc diagnostics. Same `level` vocabulary.

### Recommended Rust Parser Crate

```toml
[dependencies]
cargo_metadata = "0.18"
```

`cargo_metadata::Message::parse_stream(reader)` yields a typed enum covering all cargo message variants including `CompilerMessage` with the full `Diagnostic` struct.

---

## Q3: Workspace Topology

### Canonical Invocation

```
cargo metadata --format-version=1
```

Use `--no-deps` if you only need workspace member boundaries and not the full transitive dep graph (sets `resolve` to null).

### Key Top-Level Fields

```json
{
  "packages": [],
  "workspace_members": [],
  "workspace_default_members": [],
  "resolve": {},
  "workspace_root": "/abs/path",
  "target_directory": "/abs/path/target",
  "version": 1,
  "metadata": {}
}
```

- `packages`: ALL packages -- workspace members AND transitive deps
- `workspace_members`: Package IDs of direct workspace members only
- `metadata`: the `[workspace.metadata]` table from root Cargo.toml

### Package Object (in `packages[]`)

```json
{
  "id": "file:///path/to/pkg#0.1.0",
  "name": "my-crate",
  "version": "0.1.0",
  "source": null,
  "manifest_path": "/path/Cargo.toml",
  "edition": "2021",
  "features": { "default": ["feat1"], "feat1": [] },
  "dependencies": []
}
```

### Discriminating Dependency Source Types

The `source` field on both `packages[]` entries and `packages[].dependencies[]` entries:

| `source` value | Type |
|---------------|------|
| `null` | Path dep or workspace member |
| `"registry+https://github.com/rust-lang/crates.io-index"` | crates.io |
| `"sparse+https://my-registry.org"` | Sparse registry |
| `"git+https://github.com/org/repo?rev=abc#abc"` | Git dep |

### Dependency Object (in `packages[].dependencies[]`)

```json
{
  "name": "serde",
  "source": "registry+https://github.com/rust-lang/crates.io-index",
  "req": "^1.0",
  "kind": null,
  "optional": false,
  "uses_default_features": true,
  "features": ["derive"],
  "target": "cfg(unix)",
  "path": null,
  "rename": null
}
```

`kind` values: `null` = normal, `"dev"` = dev-only, `"build"` = build script dep.

Path deps include a non-null `path` field alongside `source: null`.

### Resolve Graph (in `resolve.nodes[]`)

```json
{
  "id": "file:///path/to/pkg#0.1.0",
  "features": ["default"],
  "deps": [
    {
      "name": "serde",
      "pkg": "registry+...#serde@1.0.200",
      "dep_kinds": [{ "kind": null, "target": null }]
    }
  ],
  "dependencies": ["registry+...#serde@1.0.200"]
}
```

### Identifying Workspace Crate Boundaries

```
workspace_members intersection packages[].id  =>  workspace-owned crates
packages[] where source == null AND id NOT IN workspace_members  =>  local path deps
```

### In-Process Parsing

```rust
use cargo_metadata::MetadataCommand;

let meta = MetadataCommand::new()
    .manifest_path("./Cargo.toml")
    .exec()?;

for pkg in meta.workspace_packages() {
    println!("{} @ {}", pkg.name, pkg.manifest_path);
    for dep in &pkg.dependencies {
        println!("  dep: {} kind={:?} source={:?}",
            dep.name, dep.kind, dep.source);
    }
}
```

---

## Q4: Test and Bench Output Parsing

### `cargo test` -- Current State

`cargo test --format=json` requires nightly + `-Z unstable-options`. RFC 3558 libtest-json stabilization is an accepted 2025H1 project goal but **not yet stable** as of 2026-04. Do not rely on it for production.

Stable alternatives: use cargo-nextest (below), or capture human-readable text output and parse it (fragile, not recommended).

### cargo-nextest (Recommended for All Test Parsing)

Install: `cargo install cargo-nextest`

**JUnit XML output (stable)**:
```
cargo nextest run --profile ci --workspace
```

Writes to `target/nextest/ci/junit.xml` in the workspace root.

**JUnit XML schema** (Jenkins-compatible, nextest variant):

```xml
<testsuites>
  <testsuite name="my-crate::my_module"
             tests="3" failures="1" errors="0" disabled="0">
    <testcase name="test_addition"
              classname="my-crate::my_module"
              timestamp="2026-04-23T10:00:00.000Z"
              time="0.001234">
      <!-- empty child = pass -->
    </testcase>
    <testcase name="test_panic"
              classname="my-crate::my_module"
              time="0.000981">
      <failure type="test failure">
        thread 'test_panic' panicked at 'assertion failed', src/lib.rs:42:5
      </failure>
    </testcase>
    <testcase name="test_flaky" time="0.002">
      <flakyFailure type="test failure" count="1" />
    </testcase>
  </testsuite>
</testsuites>
```

**Extracting fields**:

| Field | XML Location |
|-------|-------------|
| `test_name` | `<testcase name="">` attribute |
| `suite` / binary | `<testsuite name="">` attribute |
| `status` | Child element: none=pass, `<failure>`, `<error>`, `<flakyFailure>` |
| `duration_s` | `<testcase time="">` attribute (decimal seconds) |
| `source_file` | Not in JUnit XML -- correlate via name + tree-sitter spans |

**Parsing in Rust**:
```toml
[dependencies]
quick-junit = "0.4"
```

`quick-junit` is the library nextest itself uses to write JUnit XML, so it is always in sync.

**Test list (JSON, stable)**:
```
cargo nextest list --workspace --message-format json-pretty
```

Returns `nextest_metadata::TestListSummary` -- deserialize with the `nextest-metadata` crate.

**Experimental libtest-json** (not ready for production):
```
NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 \
  cargo nextest run --message-format libtest-json-plus
```
Version `0.1` as of 2023-12. Schema is documented as TODO in nextest docs. Do not use for stable pipelines.

**Doctest caveat**: nextest does not run doctests. Run `cargo test --doc` separately and parse human-readable output, or ignore doctests in the PR analysis phase.

### Criterion Benchmarks

Criterion writes raw data to `target/criterion/<bench-name>/new/` but those files are **undocumented internals** and can change without notice.

**Stable interface: `cargo-criterion` with `--message-format`**:
```
cargo install cargo-criterion
cargo criterion --message-format=json
```

Emits one JSON object per line to stdout:

**`benchmark-complete` schema**:
```json
{
  "reason": "benchmark-complete",
  "id": "my_bench/input_1000",
  "report_directory": "target/criterion/my_bench/input_1000",
  "iteration_count": [1, 10, 100],
  "measured_values": [123.4, 1234.5, 12345.6],
  "unit": "ns",
  "throughput": [{ "per_iteration": 1000, "unit": "bytes" }],
  "typical":  { "estimate": 123.4, "lower_bound": 120.0, "upper_bound": 127.0, "unit": "ns" },
  "mean":     { "estimate": 123.8, "lower_bound": 120.5, "upper_bound": 127.2, "unit": "ns" },
  "median":   { "estimate": 122.9, "lower_bound": 119.8, "upper_bound": 126.5, "unit": "ns" },
  "median_abs_dev": { "estimate": 1.2, "lower_bound": 0.9, "upper_bound": 1.5, "unit": "ns" },
  "slope": null,
  "change": {
    "mean":   { "estimate": -0.02, "lower_bound": -0.05, "upper_bound": 0.01, "unit": "%" },
    "median": { "estimate": -0.018, "lower_bound": -0.04, "upper_bound": 0.009, "unit": "%" },
    "change": "NoChange"
  }
}
```

`change.change` values: `"Improved"`, `"Regressed"`, `"NoChange"`. `slope` may be `null` (not all benchmarks can measure slope).

**`critcmp`** (BurntSushi): compares criterion baselines by reading internal JSON files. Useful for CI delta comparisons; explicitly warns it reads undocumented internals.

---

## Q5: Unsafe + Concurrency Primitive Detection

### Layer 1: tree-sitter Queries (Fastest, Source-Level)

Phase A already uses tree-sitter-rust. Add query files for unsafe and concurrency detection. tree-sitter-rust v0.23.x (current 2025) handles Rust 2024 edition `unsafe extern` blocks.

**Unsafe block query** (`unsafe_blocks.scm`):
```scheme
; User-written unsafe blocks
(unsafe_block) @unsafe.block

; Unsafe function definitions
(function_item
  "unsafe" @unsafe.fn
  name: (identifier) @unsafe.fn.name)

; Unsafe trait implementations
(impl_item "unsafe" @unsafe.impl)

; Unsafe extern blocks (Rust 2024 edition)
(extern_block "unsafe" @unsafe.extern)
```

**Concurrency primitive query** (`concurrency.scm`):
```scheme
; Arc::new, Mutex::new, etc.
(call_expression
  function: (scoped_identifier
    path: (identifier) @type
    (#match? @type "^(Arc|Mutex|RwLock|AtomicUsize|AtomicBool|AtomicI32|AtomicI64|AtomicU32|AtomicU64|AtomicPtr|OnceCell|OnceLock)")
    name: (identifier) @method (#eq? @method "new"))) @concurrency.new

; Import detection for parking_lot, tokio::sync, crossbeam
(use_declaration
  argument: (scoped_identifier
    path: (identifier) @pkg
    (#match? @pkg "^(parking_lot|crossbeam|dashmap)"))) @concurrency.import
```

Using the tree-sitter query API in Rust:
```rust
let query = Query::new(&tree_sitter_rust::language(), include_str!("unsafe_blocks.scm"))?;
let mut cursor = QueryCursor::new();
let matches = cursor.matches(&query, root_node, source_bytes);
for m in matches {
    for cap in m.captures {
        // cap.node gives byte range, cap.index identifies which capture name
    }
}
```

### Layer 2: `syn` AST Walk (Accurate, In-Process)

For source-level detection without spawning a subprocess:

```rust
use syn::{visit::Visit, ExprUnsafe, File};

struct UnsafeVisitor {
    unsafe_blocks: Vec<proc_macro2::Span>,
}

impl<'ast> Visit<'ast> for UnsafeVisitor {
    fn visit_expr_unsafe(&mut self, node: &'ast ExprUnsafe) {
        self.unsafe_blocks.push(node.unsafe_token.span);
        syn::visit::visit_expr_unsafe(self, node);
    }
}

let ast: File = syn::parse_file(&source_text)?;
let mut visitor = UnsafeVisitor { unsafe_blocks: vec![] };
visitor.visit_file(&ast);
```

`syn` v2.x (stable). Does not see through macro expansions.

### Layer 3: `cargo-geiger` Subprocess (Per-Crate Counts)

```
cargo geiger --output-format=Json --quiet
```

Output is a `SafetyReport` from `cargo-geiger-serde`:

```json
{
  "packages": [
    {
      "package": {
        "id": { "name": "my-crate", "version": "0.1.0", "source": null }
      },
      "unsafety": {
        "used": {
          "functions": { "safe": 10, "unsafe_": 2 },
          "exprs":     { "safe": 50, "unsafe_": 8 },
          "item_impls": { "safe": 3, "unsafe_": 0 },
          "item_traits": { "safe": 0, "unsafe_": 1 },
          "methods": { "safe": 20, "unsafe_": 1 }
        },
        "unused": { }
      }
    }
  ]
}
```

Current version: 0.13.0 (August 2025). **Known limitation fixed in 0.12+**: expressions inside unsafe functions were undercounted (issue #71). Use >= 0.12.0.

Parseable in Rust via `cargo-geiger-serde` crate:
```toml
[dependencies]
cargo-geiger-serde = "0.5"
```

### Layer 4: Clippy Lints (Semantic Detection via HIR)

Clippy detects unsafe patterns at HIR level, which is more accurate than token-level:

```
cargo clippy --message-format=json -- \
  -W clippy::undocumented_unsafe_blocks \
  -W clippy::multiple_unsafe_ops_per_block
```

Filter `compiler-message` objects where `message.code.code` starts with `"clippy::unsafe"` or `"clippy::multiple_unsafe"`.

How clippy detects unsafe blocks (from source): it checks `BlockCheckMode::UnsafeBlock(UnsafeSource::UserProvided)` on HIR `Block` nodes, distinguishing user-written unsafe from compiler-generated. The `undocumented_unsafe_blocks.rs` lint file is readable reference for detection patterns.

### Concurrency Pattern Coverage Summary

| Pattern | Detection Method |
|---------|----------------|
| `unsafe { }` blocks | tree-sitter query or `syn` `ExprUnsafe` |
| `unsafe fn` | tree-sitter or `syn` `ItemFn` with unsafety |
| `Arc::new` | tree-sitter call_expression query |
| `Mutex::new`, `RwLock::new` | tree-sitter call_expression query |
| `AtomicUsize` et al. | tree-sitter type reference or import |
| `parking_lot::*` | use-declaration query |
| `tokio::sync::*` | use-declaration query |
| `OnceCell`, `OnceLock` | tree-sitter type reference |
| Per-crate unsafe counts | `cargo-geiger --output-format=Json` |

---

## Q6: Claim Anchoring via rust-analyzer

### What Production Code-Review Tools Actually Do

| Tool | Approach |
|------|----------|
| Sourcegraph | Shell to `rust-analyzer scip` for batch indexing; SCIP protobuf into internal graph |
| scip-rust (Sourcegraph wrapper) | Thin shell wrapper around `rust-analyzer scip` |
| Graphite AI (Diamond) | LLM over diff text + GitHub API; no compiler integration |
| Greptile | Semantic search over repo; tree-sitter + embeddings, not ra_ap |
| VS Code rust-analyzer extension | LSP subprocess over stdio |
| Zed editor | LSP subprocess |

No production code-review tool uses `ra_ap_*` crates in-process. The instability risk is too high. The canonical pattern is: **shell to the binary**.

### Decision Tree for Claim Anchoring

```
Need batch analysis of whole PR snapshot?
  => rust-analyzer scip /workspace --output index.scip
  => Parse SCIP for definition locations + reference ranges

Need per-position query (e.g., what does fn X call)?
  => LSP subprocess with callHierarchy/outgoingCalls
  => More latency (startup + workspace load ~3-30s) but precise

Need in-process with pinned version?
  => ra_ap_ide 0.0.329 (pin exact)
  => Accept weekly breakage risk
  => Consider wrapping in subprocess anyway for stability
```

### Using `rust-analyzer scip` for Claim Anchoring

The SCIP index contains byte-range-accurate definition locations and references for every symbol. Map a claimed function name to its SCIP moniker, then look up all reference locations:

```rust
// After parsing index.scip via scip = "0.3":
// 1. Find the target symbol moniker
let target_symbol: Option<String> = index.symbols.iter()
    .find(|s| s.symbol.contains("MyStruct") && s.symbol.contains("my_method"))
    .map(|s| s.symbol.clone());

// 2. Find all reference occurrences across all documents
if let Some(sym) = target_symbol {
    for doc in &index.documents {
        for occ in &doc.occurrences {
            if occ.symbol == sym && occ.symbol_roles == 0 {
                // symbol_roles == 0 means Reference (not Definition)
                // occ.range: [start_line, start_col, end_line, end_col]
                println!("  ref in {} at {:?}", doc.relative_path, occ.range);
            }
        }
    }
}
```

### `ra_ap_ide` CallHierarchy API (In-Process, Pinned)

For teams willing to accept the maintenance cost:

```toml
[dependencies]
ra_ap_ide = "=0.0.329"
ra_ap_ide_db = "=0.0.329"
ra_ap_load_cargo = "=0.0.329"
ra_ap_project_model = "=0.0.329"
```

The `Analysis` struct provides:
- `call_hierarchy(position)` - returns a `CallHierarchyItem`
- `incoming_calls(item)` / `outgoing_calls(item)` - returns `Vec<CallItem>`
- `find_all_refs(position, config)` - returns `ReferenceSearchResult`

Published version as of 2026-04: 0.0.329. Version number increments weekly. Use exact pinning (`=`) not range.

---

## Common Pitfalls

| Pitfall | Why It Happens | How to Avoid |
|---------|---------------|--------------|
| Mixing base/head `target/` dirs | cargo reuses artifacts; head build overwrites base | Use separate `--target-dir /tmp/adr-base` and `/tmp/adr-head` |
| Parsing non-JSON stdout lines | Cargo prints "Compiling ..." lines | Filter lines starting with `{` or use `cargo_metadata::Message::parse_stream` |
| `cargo test --format=json` on stable | Feature is nightly-only | Use cargo-nextest with JUnit XML |
| Loose version of `ra_ap_ide` | Breaks weekly without exact pin | Pin `=0.0.329`; treat like a forked dep |
| `rust-analyzer scip` slow on monorepo | Full workspace index takes 10-60s | Run once per snapshot, cache by git SHA |
| cargo-geiger undercounting unsafe | Bug in versions prior to 0.12 | Use >= 0.12.0 |
| `callHierarchy/outgoingCalls` panic | Bug in rust-analyzer pre-2024-10 | Pin rust-analyzer binary; check changelog |
| Criterion target/criterion JSON format | Undocumented, can change any release | Use `cargo-criterion --message-format=json` instead |
| `source: null` ambiguity in metadata | Both workspace members and local path deps have `source: null` | Check `workspace_members` list to disambiguate |

---

## Best Practices

1. **Snapshot isolation**: Checkout base and head into separate temp directories; run all analysis tools with separate `--target-dir`. Never share a `target/` between snapshots.

2. **JSON stream parsing**: Use `cargo_metadata = "0.18"` for typed parsing of all `--message-format=json` output. Zero manual JSON envelope parsing.

3. **SCIP index as cache artifact**: Store `{git_sha}.scip` alongside the snapshot. Skip re-indexing if SHA matches. Typical index time: 5-30s on real projects.

4. **LSP subprocess pooling**: rust-analyzer workspace load takes 3-30s depending on project size. Start it once per snapshot, not per query.

5. **Layer detection appropriately**: tree-sitter for byte spans (fast, no compilation) -- then `cargo check` for semantic errors (needs compilation) -- then `cargo-geiger` for dep-wide unsafe counts (slow, run once).

6. **nextest for all test running**: Use nextest in CI for consistent, parseable output. Run `cargo test --doc` separately for doctests (nextest does not run them).

7. **Disable proc-macros for LSP analysis**: When using rust-analyzer for claim anchoring on PR snapshots, set `"procMacro": { "enable": false }` in initializationOptions to cut startup time significantly.

8. **Understand SCIP reference graph limits**: SCIP does not label call edges as first-class. Reconstruct call relationships by correlating SCIP reference occurrences with tree-sitter ExprCall node positions from Phase A.

---

## Further Reading

| Resource | Type | Why Relevant |
|----------|------|-------------|
| [External Tools -- Cargo Book](https://doc.rust-lang.org/cargo/reference/external-tools.html) | Official docs | Canonical JSON schema for all cargo --message-format=json output |
| [rustc JSON output](https://doc.rust-lang.org/rustc/json.html) | Official docs | Full diagnostic object schema with all fields |
| [cargo metadata -- Cargo Book](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html) | Official docs | Workspace topology schema reference |
| [cargo-nextest machine-readable](https://nexte.st/docs/machine-readable/) | Official docs | nextest output format overview |
| [cargo-nextest JUnit](https://nexte.st/docs/machine-readable/junit/) | Official docs | JUnit XML schema and field details |
| [quick-junit](https://github.com/nextest-rs/quick-junit) | Library | nextest's own JUnit Rust parser/serializer |
| [nextest-metadata crate](https://crates.io/crates/nextest-metadata) | Library | Typed parser for nextest JSON list output |
| [cargo_metadata crate](https://docs.rs/cargo_metadata/latest/cargo_metadata/) | Library | Typed parser for cargo metadata + check output |
| [ra_ap_ide docs](https://docs.rs/ra_ap_ide/latest/ra_ap_ide/) | Library docs | In-process rust-analyzer API; check version before use |
| [rust-analyzer SCIP CLI source](https://rust-lang.github.io/rust-analyzer/rust_analyzer/cli/scip/index.html) | Source | Batch indexing CLI docs |
| [scip-callgraph](https://github.com/Beneficial-AI-Foundation/scip-callgraph) | Tool | Call graph from SCIP index, v5.0.0 with CI |
| [sourcegraph/scip-rust](https://github.com/sourcegraph/scip-rust) | Tool | Production shell-wrapper pattern for rust-analyzer scip |
| [scip crate](https://crates.io/crates/scip) | Library | Rust SCIP protobuf parser |
| [SCIP and LSIF Indexing -- DeepWiki](https://deepwiki.com/rust-lang/rust-analyzer/9.2-scip-and-lsif-indexing) | Analysis | How rust-analyzer batch indexing works internally |
| [project-stable-mir](https://github.com/rust-lang/project-stable-mir) | Project | Future stable MIR API; watch for stabilization |
| [MIRAI](https://github.com/endorlabs/MIRAI) | Tool | Abstract interpreter with call graph via --call_graph_config |
| [cargo-geiger](https://github.com/geiger-rs/cargo-geiger) | Tool | Unsafe code counts by crate; v0.13.0 (Aug 2025) |
| [cargo-geiger-serde](https://docs.rs/cargo-geiger-serde/latest/cargo_geiger_serde/) | Library | Rust types for JSON geiger report deserialization |
| [cargo-criterion external tools](https://bheisler.github.io/criterion.rs/book/cargo_criterion/external_tools.html) | Docs | Stable JSON schema for benchmark-complete messages |
| [critcmp](https://github.com/BurntSushi/critcmp) | Tool | Criterion baseline comparison for CI |
| [RFC 3558 libtest-json](https://rust-lang.github.io/rfcs/3558-libtest-json.html) | RFC | The stabilization proposal; read to understand what is coming |
| [libtest JSON 2025H1 goal](https://rust-lang.github.io/rust-project-goals/2025h1/libtest-json.html) | Project goals | Current stabilization status (not stable as of 2026-04) |
| [LSP outside the editor](https://medium.com/@selfint/lsp-outside-the-editor-431f77a9a4be) | Blog | Subprocess LSP client pattern with Rust code examples |
| [rust-analyzer callHierarchy PR #2698](https://github.com/rust-lang/rust-analyzer/pull/2698) | PR | Original call hierarchy implementation notes |
| [rust-analyzer callHierarchy panic fix #17554](https://github.com/rust-lang/rust-analyzer/pull/17554) | PR | Bug fix to pin around |
| [tree-sitter-rust](https://github.com/tree-sitter/tree-sitter-rust) | Library | Grammar; v0.23.x supports Rust 2024 edition |
| [clippy undocumented_unsafe_blocks source](https://github.com/rust-lang/rust-clippy/blob/master/clippy_lints/src/undocumented_unsafe_blocks.rs) | Source | Reference for HIR detection pattern |
| [Implementing call graph generator forum](https://users.rust-lang.org/t/implementing-a-call-graph-generator-as-a-cargo-extension/127739) | Forum | 2025 state of Rust call graph tooling |
| [Rust call graph benchmark](https://internals.rust-lang.org/t/a-benchmark-for-rust-call-graph-generators/11273) | Forum | Evaluation of existing tools |
| [cargo-geiger issue #71](https://github.com/geiger-rs/cargo-geiger/issues/71) | Issue | Undercounting bug; fixed in 0.12+ |

---

*Generated by learn-agent from 38 sources. See `agent-knowledge/resources/rust-code-analysis-sources.json` for full source metadata.*
