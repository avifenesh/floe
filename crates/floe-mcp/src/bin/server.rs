//! `floe-mcp` — MCP-over-stdio server binary.
//!
//! Implements a minimal Model Context Protocol server speaking JSON-RPC 2.0
//! over stdin/stdout. Each `Session` handler from the library is exposed as
//! one MCP tool. Stdout is reserved for protocol messages; all logging
//! goes to stderr (standard MCP convention — anything on stdout that
//! isn't a valid JSON-RPC message will break the client).
//!
//! Usage:
//!
//! ```text
//! floe-mcp --artifact /path/to/artifact.json
//! ```
//!
//! Clients: `floe-server` spawns this binary as a child process per
//! analysis; any standard MCP-over-stdio client (Claude Code via
//! `claude mcp add`, Cursor, OpenCode, or hand-rolled JSON-RPC loops)
//! can drive it the same way.

use std::path::PathBuf;
use std::sync::Arc;

use floe_core::Artifact;
use floe_mcp::fs_tools::{self, ToolsRoot};
use floe_mcp::{ErrorCode, FinalizeOutcome, MutateFlowPatch, Session, ToolError};
use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// MCP protocol version we speak. If the client requests a different one
/// we echo back what we support and let the client decide.
const MCP_VERSION: &str = "2025-06-18";

const SERVER_NAME: &str = "floe-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(name = "floe-mcp", version, about = "MCP server for floe flow synthesis")]
struct Cli {
    /// Path to the analyzed artifact JSON (from `floe diff ... --out`).
    #[arg(long)]
    artifact: PathBuf,

    /// Model name to stamp onto accepted flows at finalize.
    #[arg(long, default_value = "unknown")]
    model: String,

    /// Runtime version to stamp onto accepted flows at finalize.
    #[arg(long, default_value = "0")]
    runtime_version: String,

    /// Cap on tool calls. Defaults to the contract cap (200).
    #[arg(long)]
    call_budget: Option<u32>,

    /// Relax the "artifact must carry structural flows" check. Used
    /// for navigation-only probe sessions where the caller supplies a
    /// side-only artifact with no flows.
    #[arg(long, default_value_t = false)]
    probe: bool,

    /// Enable proof-pass mode: exposes `floe.read_file` / `floe.grep` /
    /// `floe.glob` in addition to the navigation tools. Requires
    /// `--repo-root` so the fs tools know where to resolve paths.
    /// Intent + proof-verification LLM sessions need these because
    /// proof is about semantic evidence in unstructured files
    /// (examples/, benches, test bodies) that the navigation-only
    /// tool surface can't reach.
    #[arg(long, default_value_t = false)]
    proof: bool,

    /// Filesystem root the proof fs tools resolve paths against.
    /// Required when `--proof` is set; ignored otherwise. Typically
    /// the head snapshot root (where evidence for new claims lives).
    #[arg(long)]
    repo_root: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Logs to stderr — stdout is reserved for JSON-RPC frames.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let bytes = tokio::fs::read(&cli.artifact)
        .await
        .with_context(|| format!("reading artifact {}", cli.artifact.display()))?;
    let artifact: Artifact =
        serde_json::from_slice(&bytes).context("parsing artifact JSON")?;
    tracing::info!(
        artifact = %cli.artifact.display(),
        hunks = artifact.hunks.len(),
        flows = artifact.flows.len(),
        "loaded artifact"
    );

    let mut session = if cli.probe {
        Session::new_relaxed(artifact)
    } else {
        Session::new(artifact)?
    };
    if let Some(cap) = cli.call_budget {
        session = session.with_call_budget(cap);
    }
    let fs_root = if cli.proof {
        let root_path = cli
            .repo_root
            .clone()
            .ok_or_else(|| anyhow::anyhow!("--proof requires --repo-root"))?;
        Some(ToolsRoot::new(root_path).context("constructing proof fs root")?)
    } else {
        None
    };
    let server = Arc::new(Server {
        session: Mutex::new(session),
        model: cli.model,
        runtime_version: cli.runtime_version,
        fs_root,
        proof: cli.proof,
    });

    run_loop(server).await
}

struct Server {
    session: Mutex<Session>,
    model: String,
    runtime_version: String,
    /// Filesystem root for `floe.read_file` / `floe.grep` / `floe.glob`.
    /// `None` when the server wasn't started with `--proof`.
    fs_root: Option<ToolsRoot>,
    /// Mirror of `--proof`; governs tool advertisement in `tools/list`
    /// so probe/synthesis sessions don't see the fs tools even if
    /// someone sets `fs_root` manually.
    proof: bool,
}

async fn run_loop(server: Arc<Server>) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut out = stdout;

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write(&mut out, &parse_error_response(&e.to_string())).await?;
                continue;
            }
        };
        let response = handle(Arc::clone(&server), req).await;
        if let Some(resp) = response {
            write(&mut out, &resp).await?;
        }
    }
    Ok(())
}

async fn write(out: &mut tokio::io::Stdout, msg: &Value) -> Result<()> {
    let s = serde_json::to_string(msg)?;
    out.write_all(s.as_bytes()).await?;
    out.write_all(b"\n").await?;
    out.flush().await?;
    Ok(())
}

/// Dispatch one JSON-RPC frame. Returns `None` for notifications (no reply).
async fn handle(server: Arc<Server>, req: Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    tracing::debug!(method, "rpc");

    // Notifications have no id — we process then return no response.
    let is_notification = id.is_none();

    let result = match method {
        "initialize" => Ok(initialize_result()),
        "initialized" | "notifications/initialized" => {
            return None;
        }
        "tools/list" => Ok(tools_list_result(&server)),
        "tools/call" => call_tool(Arc::clone(&server), params).await,
        "ping" => Ok(json!({})),
        other => Err(ToolCallError::method_not_found(other)),
    };

    if is_notification {
        return None;
    }

    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err(err) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": err.code, "message": err.message },
        }),
    })
}

/* -------------------------------------------------------------------------- */
/* initialize + tools/list                                                    */
/* -------------------------------------------------------------------------- */

fn initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
    })
}

fn tools_list_result(server: &Server) -> Value {
    let mut tools = base_tools();
    if server.proof {
        tools.extend(proof_tools());
    }
    json!({ "tools": tools })
}

fn base_tools() -> Vec<Value> {
    vec![
        tool("floe.list_hunks",
            "Return every hunk in the artifact with a one-line summary and the \
             qualified-name entities it touches. Call this first to see the \
             starting point.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false })),
        tool("floe.get_entity",
            "Look up a single entity by qualified name. Returns the descriptor \
             (kind, file, byte span, and signature for functions/methods).",
            json!({
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Qualified name, e.g. 'Queue.setBudget'" } },
                "required": ["id"],
                "additionalProperties": false
            })),
        tool("floe.neighbors",
            "BFS the call graph around an entity. hops is capped at 3.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "hops": { "type": "integer", "minimum": 1, "maximum": 3, "default": 1 }
                },
                "required": ["id"],
                "additionalProperties": false
            })),
        tool("floe.list_flows_initial",
            "Return the deterministic structural clustering. Starting point — \
             you are expected to merge/split/rename as appropriate.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false })),
        tool("floe.list_entities",
            "Enumerate every entity in the artifact. Used for probe / \
             navigation workflows that need a bootstrap list of qualified \
             names. `side` restricts to `base` or `head`; `kind` restricts \
             to a single entity kind.",
            json!({
                "type": "object",
                "properties": {
                    "side": { "type": "string", "enum": ["base", "head"] },
                    "kind": { "type": "string", "enum": ["function", "type", "state", "api-endpoint", "file"] }
                },
                "additionalProperties": false
            })),
        tool("floe.propose_flow",
            "Create a new flow. Name must be 3..48 chars and not in the \
             reserved list (misc, various, other, unknown, cluster, group). \
             Every hunk_id must exist; every extra entity must exist.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "rationale": { "type": "string" },
                    "hunk_ids": { "type": "array", "items": { "type": "string" } },
                    "extra_entities": { "type": "array", "items": { "type": "string" }, "default": [] }
                },
                "required": ["name", "rationale", "hunk_ids"],
                "additionalProperties": false
            })),
        tool("floe.mutate_flow",
            "Apply a partial patch to an existing flow (by id). Any of \
             name/rationale/add_hunks/remove_hunks/add_entities/remove_entities \
             may be present.",
            json!({
                "type": "object",
                "properties": {
                    "flow_id": { "type": "string" },
                    "patch": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "rationale": { "type": "string" },
                            "add_hunks": { "type": "array", "items": { "type": "string" } },
                            "remove_hunks": { "type": "array", "items": { "type": "string" } },
                            "add_entities": { "type": "array", "items": { "type": "string" } },
                            "remove_entities": { "type": "array", "items": { "type": "string" } }
                        },
                        "additionalProperties": false
                    }
                },
                "required": ["flow_id", "patch"],
                "additionalProperties": false
            })),
        tool("floe.remove_flow",
            "Remove a flow. Rejected if any of its hunks would become orphan.",
            json!({
                "type": "object",
                "properties": { "flow_id": { "type": "string" } },
                "required": ["flow_id"],
                "additionalProperties": false
            })),
        tool("floe.finalize",
            "Commit the working set as the final flow list. Returns \
             {outcome: 'accepted', flows: [...]} or {outcome: 'rejected', \
             rejected_rule, detail}. Tool calls after finalize are \
             meaningless but not errored.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false })),
    ]
}

/// Tools only advertised when the server is started with `--proof`.
/// These give the intent-fit + proof-verification sessions direct
/// filesystem access (lifted from Codex CLI — see
/// `feedback_reuse_codex_tools.md`).
fn proof_tools() -> Vec<Value> {
    vec![
        tool("floe.read_file",
            "Read a file by path, returning newline-joined `L{n}: <text>` \
             lines. Use offset/limit to paginate large files; the file's \
             total line count is returned so you know when to stop. \
             `path` is the canonical parameter; `file_path` is accepted \
             as a legacy alias.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "file_path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 1 },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "additionalProperties": false
            })),
        tool("floe.grep",
            "Ripgrep-backed regex search. Respects .gitignore. Returns matches \
             with path (relative to session root), line number, and the matched \
             text. Set `case_insensitive` to true for `(?i)`-style matches.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "glob": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 2000 },
                    "case_insensitive": { "type": "boolean" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            })),
        tool("floe.glob",
            "List files matching a glob pattern (e.g. `examples/**/*.ts`). \
             Respects .gitignore. Use before `floe.read_file` when you don't \
             know the exact path of an example or benchmark file.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "required": ["pattern"],
                "additionalProperties": false
            })),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

/* -------------------------------------------------------------------------- */
/* tools/call                                                                 */
/* -------------------------------------------------------------------------- */

#[derive(Deserialize)]
struct CallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

async fn call_tool(server: Arc<Server>, params: Value) -> Result<Value, ToolCallError> {
    let call: CallParams = serde_json::from_value(params)
        .map_err(|e| ToolCallError::invalid_params(e.to_string()))?;

    let result = dispatch(&server, &call.name, call.arguments).await;
    Ok(result_to_content(result))
}

async fn dispatch(server: &Server, name: &str, args: Value) -> DispatchResult {
    // Normalise tool-name forms. Our MCP registration uses `floe.X`
    // (dot), the contract doc + prompt sometimes render `floe:X`
    // (colon), and we still accept legacy `adr.X` / `adr:X` emitted
    // by cached sessions that predate the rename. All collapse to
    // the dotted `floe.` canonical form.
    let name = if let Some(rest) = name.strip_prefix("floe:") {
        std::borrow::Cow::Owned(format!("floe.{rest}"))
    } else if let Some(rest) = name.strip_prefix("adr:").or_else(|| name.strip_prefix("adr.")) {
        std::borrow::Cow::Owned(format!("floe.{rest}"))
    } else {
        std::borrow::Cow::Borrowed(name)
    };
    match name.as_ref() {
        "floe.list_hunks" => {
            let mut s = server.session.lock().await;
            match s.list_hunks() {
                Ok(v) => DispatchResult::Ok(serde_json::to_value(v).unwrap()),
                Err(e) => DispatchResult::ToolErr(e),
            }
        }
        "floe.get_entity" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
            }
            let a: A = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return DispatchResult::BadArgs(e.to_string()),
            };
            let mut s = server.session.lock().await;
            match s.get_entity(&a.id) {
                Ok(v) => DispatchResult::Ok(serde_json::to_value(v).unwrap()),
                Err(e) => DispatchResult::ToolErr(e),
            }
        }
        "floe.neighbors" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                #[serde(default = "default_hops")]
                hops: u32,
            }
            fn default_hops() -> u32 {
                1
            }
            let a: A = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return DispatchResult::BadArgs(e.to_string()),
            };
            let mut s = server.session.lock().await;
            match s.neighbors(&a.id, a.hops) {
                Ok(v) => DispatchResult::Ok(serde_json::to_value(v).unwrap()),
                Err(e) => DispatchResult::ToolErr(e),
            }
        }
        "floe.list_flows_initial" => {
            let mut s = server.session.lock().await;
            match s.list_flows_initial() {
                Ok(v) => DispatchResult::Ok(serde_json::to_value(v).unwrap()),
                Err(e) => DispatchResult::ToolErr(e),
            }
        }
        "floe.list_entities" => {
            #[derive(Deserialize)]
            struct A {
                #[serde(default)]
                side: Option<floe_mcp::wire::SnapshotSide>,
                #[serde(default)]
                kind: Option<floe_mcp::wire::EntityKindTag>,
            }
            let a: A = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return DispatchResult::BadArgs(e.to_string()),
            };
            let mut s = server.session.lock().await;
            match s.list_entities(a.side, a.kind) {
                Ok(v) => DispatchResult::Ok(serde_json::to_value(v).unwrap()),
                Err(e) => DispatchResult::ToolErr(e),
            }
        }
        "floe.propose_flow" => {
            #[derive(Deserialize)]
            struct A {
                name: String,
                rationale: String,
                hunk_ids: Vec<String>,
                #[serde(default)]
                extra_entities: Vec<String>,
            }
            let a: A = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return DispatchResult::BadArgs(e.to_string()),
            };
            let mut s = server.session.lock().await;
            match s.propose_flow(&a.name, &a.rationale, a.hunk_ids, a.extra_entities) {
                Ok(flow_id) => DispatchResult::Ok(json!({ "flow_id": flow_id })),
                Err(e) => DispatchResult::ToolErr(e),
            }
        }
        "floe.mutate_flow" => {
            #[derive(Deserialize)]
            struct A {
                flow_id: String,
                patch: MutateFlowPatch,
            }
            let a: A = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return DispatchResult::BadArgs(e.to_string()),
            };
            let mut s = server.session.lock().await;
            match s.mutate_flow(&a.flow_id, a.patch) {
                Ok(()) => DispatchResult::Ok(json!({ "ok": true })),
                Err(e) => DispatchResult::ToolErr(e),
            }
        }
        "floe.remove_flow" => {
            #[derive(Deserialize)]
            struct A {
                flow_id: String,
            }
            let a: A = match serde_json::from_value(args) {
                Ok(v) => v,
                Err(e) => return DispatchResult::BadArgs(e.to_string()),
            };
            let mut s = server.session.lock().await;
            match s.remove_flow(&a.flow_id) {
                Ok(()) => DispatchResult::Ok(json!({ "ok": true })),
                Err(e) => DispatchResult::ToolErr(e),
            }
        }
        "floe.finalize" => {
            let mut s = server.session.lock().await;
            let outcome = s.finalize(&server.model, &server.runtime_version);
            DispatchResult::Ok(finalize_to_json(outcome))
        }
        "floe.read_file" => {
            let Some(root) = server.fs_root.as_ref() else {
                return DispatchResult::ProofOff("floe.read_file");
            };
            // Alias `file_path` → `path`: models sometimes drift
            // between the two. Harness expects `path`; fold the
            // legacy key in when present and the canonical is absent.
            let mut args = args;
            if let Value::Object(ref mut m) = args {
                if !m.contains_key("path") {
                    if let Some(v) = m.remove("file_path") {
                        m.insert("path".to_string(), v);
                    }
                }
            }
            DispatchResult::Ok(fs_tools::read_file(root, args).await)
        }
        "floe.grep" => {
            let Some(root) = server.fs_root.as_ref() else {
                return DispatchResult::ProofOff("floe.grep");
            };
            DispatchResult::Ok(fs_tools::grep(root, args).await)
        }
        "floe.glob" => {
            let Some(root) = server.fs_root.as_ref() else {
                return DispatchResult::ProofOff("floe.glob");
            };
            DispatchResult::Ok(fs_tools::glob(root, args).await)
        }
        other => DispatchResult::Unknown(other.to_string()),
    }
}

enum DispatchResult {
    Ok(Value),
    ToolErr(ToolError),
    BadArgs(String),
    /// The caller invoked a proof-only fs tool on a non-proof session.
    /// Carries the tool name so the error message can point at the
    /// missing `--proof` flag.
    ProofOff(&'static str),
    /// A proof fs tool errored (path escape, missing file, bad glob).
    /// Carries a reviewer-readable reason. Reserved for the proof-side
    /// fs tools that land with the mutation-tool expansion; unused on
    /// the read-only dispatch path today.
    #[allow(dead_code)]
    FsErr(String),
    Unknown(String),
}

/// MCP convention: a successful tool call returns `{ content: [...], isError: false }`.
/// A tool-level error returns the same shape with `isError: true`. We always
/// put the serialized JSON in a single text block — clients that understand
/// JSON will parse it; readers just see pretty text.
fn result_to_content(r: DispatchResult) -> Value {
    match r {
        DispatchResult::Ok(v) => json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap() }],
            "isError": false,
        }),
        DispatchResult::ToolErr(e) => {
            // Prefix with a literal "ERROR:" so a text-reading model can't
            // miss that this is a failure even if it ignores the JSON flag.
            let payload = json!({
                "ok": false,
                "error": code_name(e.code),
                "reason": e.reason,
            });
            let body = format!(
                "ERROR: {}\n{}",
                code_name(e.code),
                serde_json::to_string_pretty(&payload).unwrap(),
            );
            json!({
                "content": [{ "type": "text", "text": body }],
                "isError": true,
            })
        }
        DispatchResult::BadArgs(msg) => json!({
            "content": [{ "type": "text", "text": format!("ERROR: invalid arguments: {msg}") }],
            "isError": true,
        }),
        DispatchResult::ProofOff(name) => json!({
            "content": [{ "type": "text", "text": format!("ERROR: {name} only available when the server runs with --proof and --repo-root. This session is probe/synthesis-only.") }],
            "isError": true,
        }),
        DispatchResult::FsErr(msg) => json!({
            "content": [{ "type": "text", "text": format!("ERROR: fs tool failure: {msg}") }],
            "isError": true,
        }),
        DispatchResult::Unknown(name) => json!({
            "content": [{ "type": "text", "text": format!("ERROR: unknown tool `{name}`. Available tools are listed via tools/list; use floe.list_hunks / floe.propose_flow etc.") }],
            "isError": true,
        }),
    }
}

fn code_name(c: ErrorCode) -> &'static str {
    match c {
        ErrorCode::NameReserved => "NAME_RESERVED",
        ErrorCode::NameTooShort => "NAME_TOO_SHORT",
        ErrorCode::NameTooLong => "NAME_TOO_LONG",
        ErrorCode::RationaleTooShort => "RATIONALE_TOO_SHORT",
        ErrorCode::RationaleTooLong => "RATIONALE_TOO_LONG",
        ErrorCode::HunkNotFound => "HUNK_NOT_FOUND",
        ErrorCode::EntityNotFound => "ENTITY_NOT_FOUND",
        ErrorCode::FlowNotFound => "FLOW_NOT_FOUND",
        ErrorCode::CoverageBroken => "COVERAGE_BROKEN",
        ErrorCode::CallBudgetExceeded => "CALL_BUDGET_EXCEEDED",
        ErrorCode::ResultsTooLarge => "RESULTS_TOO_LARGE",
    }
}

fn finalize_to_json(o: FinalizeOutcome) -> Value {
    match o {
        FinalizeOutcome::Accepted { flows } => json!({
            "outcome": "accepted",
            "flows": flows,
        }),
        FinalizeOutcome::Rejected {
            rejected_rule,
            detail,
        } => json!({
            "outcome": "rejected",
            "rejected_rule": rejected_rule,
            "detail": detail,
        }),
    }
}

/* -------------------------------------------------------------------------- */
/* Error helpers                                                              */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Serialize)]
struct ToolCallError {
    code: i32,
    message: String,
}

impl ToolCallError {
    fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
        }
    }
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
        }
    }
}

fn parse_error_response(msg: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": -32700, "message": format!("parse error: {msg}") },
    })
}
