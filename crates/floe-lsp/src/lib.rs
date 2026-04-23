//! LSP client — drives `typescript-language-server` (or `vtsls`) to
//! produce the semantic substrate TS v2 depends on:
//!
//! - **Call hierarchy** resolved across files and through imports —
//!   replaces the tree-sitter syntactic call extraction that caps
//!   TS v1.
//! - **References / definition** lookups used to anchor claims to
//!   exact source ranges (RFC Appendix F, upgrade #6).
//!
//! The client is library-based and in-process: `async-lsp` drives a
//! child language-server over stdio, but the orchestration lives on
//! the same tokio runtime as the rest of `floe-server`. No child MCP
//! process here — LSP is internal.
//!
//! # Architecture
//!
//! ```text
//!                ┌──────────────── floe-lsp (this crate) ──────────────┐
//!                │                                                    │
//!   floe-parse ◀──┤  TsLspClient  ── calls ──▶  LSP JSON-RPC framed   │
//!                │   (library)        over child stdin/stdout         │
//!                │                                                    │
//!                └────────────────────┬───────────────────────────────┘
//!                                     ▼
//!                        typescript-language-server
//!                              (subprocess, --stdio)
//! ```
//!
//! # Lifecycle
//!
//! ```ignore
//! let client = TsLspClient::start(workspace_root).await?;
//! client.open_file(&path, &text).await?;
//! let items = client.prepare_call_hierarchy(&path, line, col).await?;
//! let out = client.outgoing_calls(&items[0]).await?;
//! client.shutdown().await?;
//! ```
//!
//! The client takes kill_on_drop on the child — abandoning a client
//! without shutting it down won't leak a server across process exit.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::tracing::TracingLayer;
pub mod enrich;
pub use async_lsp::lsp_types::Url;
pub use enrich::enrich_graph;

use async_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    ClientCapabilities, DidOpenTextDocumentParams, InitializeParams, InitializedParams, Location,
    Position, ReferenceContext, ReferenceParams, TextDocumentClientCapabilities,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams,
    WorkDoneProgressParams, WorkspaceFolder,
};
use async_lsp::{LanguageServer, ServerSocket};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;

/// Handle to a running TypeScript language server session. The
/// workspace root is pinned at construction; every path the caller
/// hands us resolves to a `file://` URI relative to (or absolute
/// underneath) that root.
pub struct TsLspClient {
    server: ServerSocket,
    root: PathBuf,
    /// Spawned child process. Held on the struct so `kill_on_drop`
    /// cleanly reaps the server if we panic between `shutdown` calls.
    _child: Child,
    /// Background mainloop task — polls the framed transport. Dropped
    /// when the client drops; we keep the handle for explicit
    /// `shutdown` to surface any loop-level errors.
    mainloop: Option<JoinHandle<Result<(), anyhow::Error>>>,
}

impl TsLspClient {
    /// Start a new TypeScript language server session rooted at
    /// `workspace_root`. Spawns `typescript-language-server --stdio`
    /// (must be on `PATH`) and runs the LSP handshake.
    pub async fn start(workspace_root: &Path) -> Result<Self> {
        let root = workspace_root
            .canonicalize()
            .with_context(|| format!("canonicalize workspace {}", workspace_root.display()))?;

        let (mainloop, server) = async_lsp::MainLoop::new_client(|_server| {
            let mut router = Router::new(());
            // Drain all server→client notifications we don't care
            // about. `typescript-language-server` emits window/
            // showMessage, window/logMessage, $/progress, and
            // publishDiagnostics liberally; ignoring them keeps the
            // log clean and lets the mainloop make progress.
            router
                .notification::<async_lsp::lsp_types::notification::PublishDiagnostics>(|_, _| {
                    ControlFlow::Continue(())
                })
                .notification::<async_lsp::lsp_types::notification::ShowMessage>(|_, m| {
                    tracing::debug!(kind = ?m.typ, "ts-lsp: {}", m.message);
                    ControlFlow::Continue(())
                })
                .notification::<async_lsp::lsp_types::notification::LogMessage>(|_, m| {
                    tracing::trace!(kind = ?m.typ, "ts-lsp: {}", m.message);
                    ControlFlow::Continue(())
                })
                .notification::<async_lsp::lsp_types::notification::Progress>(|_, _| {
                    ControlFlow::Continue(())
                });
            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(router)
        });

        let mut child = spawn_tsls(&root)
            .context("spawn typescript-language-server (is it on PATH?)")?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("child stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("child stdout missing"))?;

        // async-lsp wants `futures_io::AsyncRead/Write`, tokio's
        // process pipes are `tokio::io` — bridge through the compat
        // shim in `tokio-util`.
        let stdout_compat = stdout.compat();
        let stdin_compat = stdin.compat_write();
        let mainloop_handle: JoinHandle<Result<(), anyhow::Error>> = tokio::spawn(async move {
            mainloop
                .run_buffered(stdout_compat, stdin_compat)
                .await
                .map_err(|e| anyhow!("ts-lsp mainloop exited: {e}"))
        });

        let mut this = Self {
            server,
            root: root.clone(),
            _child: child,
            mainloop: Some(mainloop_handle),
        };
        this.initialize().await?;
        Ok(this)
    }

    async fn initialize(&mut self) -> Result<()> {
        let workspace_uri = Url::from_file_path(&self.root)
            .map_err(|_| anyhow!("workspace root not a valid file URI: {}", self.root.display()))?;
        let params = InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: workspace_uri,
                name: "adr".into(),
            }]),
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    call_hierarchy: Some(
                        async_lsp::lsp_types::CallHierarchyClientCapabilities {
                            dynamic_registration: Some(false),
                        },
                    ),
                    references: Some(async_lsp::lsp_types::ReferenceClientCapabilities {
                        dynamic_registration: Some(false),
                    }),
                    definition: Some(async_lsp::lsp_types::GotoCapability {
                        dynamic_registration: Some(false),
                        link_support: Some(false),
                    }),
                    document_symbol: Some(
                        async_lsp::lsp_types::DocumentSymbolClientCapabilities {
                            dynamic_registration: Some(false),
                            hierarchical_document_symbol_support: Some(true),
                            symbol_kind: None,
                            tag_support: None,
                        },
                    ),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        self.server
            .initialize(params)
            .await
            .context("ts-lsp initialize")?;
        self.server
            .initialized(InitializedParams {})
            .map_err(|e| anyhow!("ts-lsp initialized notification: {e}"))?;
        Ok(())
    }

    /// Tell the server about a file — required before most requests.
    /// Idempotent; reopens are accepted by tsserver but we version
    /// each open with `0` because we don't mutate in-memory buffers.
    pub async fn open_file(&mut self, path: &Path, text: &str) -> Result<()> {
        let uri = self.uri_for(path)?;
        self.server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: language_id_for(path).into(),
                    version: 0,
                    text: text.into(),
                },
            })
            .map_err(|e| anyhow!("did_open {}: {e}", path.display()))?;
        Ok(())
    }

    /// Call-hierarchy entry point: ask the server for the symbol at
    /// `(line, character)`. Returns zero or more items — usually one
    /// for a function / method name.
    pub async fn prepare_call_hierarchy(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<CallHierarchyItem>> {
        let params = CallHierarchyPrepareParams {
            text_document_position_params: self.doc_pos(path, line, character)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let res = self
            .server
            .prepare_call_hierarchy(params)
            .await
            .context("ts-lsp prepare_call_hierarchy")?;
        Ok(res.unwrap_or_default())
    }

    /// Outgoing calls from an item — the edges this symbol creates.
    pub async fn outgoing_calls(
        &mut self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>> {
        let res = self
            .server
            .outgoing_calls(CallHierarchyOutgoingCallsParams {
                item: item.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: Default::default(),
            })
            .await
            .context("ts-lsp outgoing_calls")?;
        Ok(res.unwrap_or_default())
    }

    /// Incoming calls — who calls into this symbol.
    pub async fn incoming_calls(
        &mut self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>> {
        let res = self
            .server
            .incoming_calls(CallHierarchyIncomingCallsParams {
                item: item.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: Default::default(),
            })
            .await
            .context("ts-lsp incoming_calls")?;
        Ok(res.unwrap_or_default())
    }

    /// All references to a symbol — used by the claim-anchoring pass
    /// to lift every usage site, not just the declaration.
    pub async fn references(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<Location>> {
        let params = ReferenceParams {
            text_document_position: self.doc_pos(path, line, character)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: Default::default(),
            context: ReferenceContext {
                include_declaration,
            },
        };
        let res = self
            .server
            .references(params)
            .await
            .context("ts-lsp references")?;
        Ok(res.unwrap_or_default())
    }

    /// Clean shutdown — send shutdown + exit, wait for the mainloop
    /// task to finish draining. Called by `Drop` as best-effort too,
    /// but explicit is better when you care about surfacing errors.
    pub async fn shutdown(mut self) -> Result<()> {
        let _ = self.server.shutdown(()).await;
        let _ = self.server.exit(());
        if let Some(handle) = self.mainloop.take() {
            // Bounded — if the child hasn't closed its stdio in
            // 2 seconds, the `kill_on_drop` will reap it on the
            // Child dropping.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        }
        Ok(())
    }

    fn uri_for(&self, path: &Path) -> Result<Url> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        Url::from_file_path(&abs)
            .map_err(|_| anyhow!("path not file URI friendly: {}", abs.display()))
    }

    fn doc_pos(
        &self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<TextDocumentPositionParams> {
        Ok(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri_for(path)?,
            },
            position: Position { line, character },
        })
    }
}

/// Spawn `typescript-language-server --stdio` with stdio piped.
///
/// On Windows, npm-installed CLIs land as `.cmd` shims that
/// `tokio::process::Command` can't spawn directly (needs a shell to
/// interpret the batch script). We resolve the actual binary by
/// walking PATH ourselves and picking the first match, preferring
/// `.exe` (rare for tsls) over `.cmd`. `FLOE_TS_LSP` overrides if the
/// user has a pinned install.
fn spawn_tsls(cwd: &Path) -> Result<Child> {
    let exe = std::env::var("FLOE_TS_LSP").ok().map(PathBuf::from)
        .or_else(|| which("typescript-language-server"))
        .ok_or_else(|| {
            anyhow!(
                "typescript-language-server not on PATH; install with \
                 `npm i -g typescript-language-server typescript` \
                 or set FLOE_TS_LSP to an absolute path"
            )
        })?;

    // On Windows, a `.cmd` needs cmd.exe; `.exe` / `.ps1` / bare file
    // with shebang can be spawned directly. Pick the strategy.
    let is_cmd = exe
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cmd") || s.eq_ignore_ascii_case("bat"))
        .unwrap_or(false);

    let mut cmd = if is_cmd {
        let mut c = Command::new("cmd");
        c.args(["/C", exe.to_str().unwrap_or_default(), "--stdio"]);
        c
    } else {
        let mut c = Command::new(&exe);
        c.arg("--stdio");
        c
    };
    cmd.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    Ok(cmd.spawn()?)
}

/// Minimal `which` — returns the first PATH hit for `name`, trying
/// Windows-style extension fallbacks (`.exe`, `.cmd`, `.bat`) when
/// the bare name doesn't match. Null on miss.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    // On Windows, prefer the batch/PowerShell shim over the bare
    // shebang file — npm-installed CLIs are `.cmd` on Windows and the
    // bare file is a Unix script that CreateProcessW can't run.
    let candidates_exts: &[&str] = if cfg!(windows) {
        &[".cmd", ".bat", ".exe", ""]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(&path) {
        for ext in candidates_exts {
            let candidate = if ext.is_empty() {
                dir.join(name)
            } else {
                dir.join(format!("{name}{ext}"))
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Pick an LSP `languageId` from a path extension. `typescript-
/// language-server` recognises these four tags.
fn language_id_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" => "javascript",
        "jsx" => "javascriptreact",
        _ => "plaintext",
    }
}
