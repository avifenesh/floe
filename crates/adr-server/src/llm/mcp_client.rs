//! Minimal MCP client that spawns `adr-mcp` as a child over stdio and
//! drives the JSON-RPC 2.0 protocol. We implement exactly the subset the
//! server needs: `initialize` handshake, `tools/list`, `tools/call`.
//!
//! Stderr is piped to our tracing logs so the child's logs don't go to
//! a terminal the user can't see.

use std::path::Path;
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

/// Which tool surface to expose when spawning the child.
#[derive(Debug, Clone, Copy)]
enum SpawnMode {
    /// Full synthesis toolbox (proposals/mutations). No flows-required
    /// relaxation.
    Synthesis,
    /// Navigation-only probe toolbox with the flows-required check
    /// relaxed.
    Probe,
    /// Probe tools + `adr.read_file` / `adr.grep` / `adr.glob` for the
    /// intent + proof-verification passes.
    Proof,
}

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Result of a `tools/call` from the child.
pub struct ToolCallResult {
    pub is_error: bool,
    /// Concatenated text content from all `{ type: "text" }` blocks.
    pub text: String,
}

impl McpClient {
    /// Spawn `adr-mcp` with the given artifact. The binary path defaults
    /// to `adr-mcp` (looked up in PATH); override via `ADR_MCP_BIN`.
    pub async fn spawn(
        artifact: &Path,
        model: &str,
        runtime_version: &str,
    ) -> Result<Self> {
        Self::spawn_inner(artifact, model, runtime_version, SpawnMode::Synthesis, None).await
    }

    /// Spawn `adr-mcp` in probe mode (relaxes the flows-required
    /// invariant). Used when the artifact is a side-only navigation
    /// snapshot with no flows.
    pub async fn spawn_probe(
        artifact: &Path,
        model: &str,
        runtime_version: &str,
    ) -> Result<Self> {
        Self::spawn_inner(artifact, model, runtime_version, SpawnMode::Probe, None).await
    }

    /// Spawn `adr-mcp` in proof mode — adds `adr.read_file` / `adr.grep`
    /// / `adr.glob` to the tool surface. `repo_root` is the filesystem
    /// root those tools resolve paths against (typically the head
    /// snapshot). Also passes `--probe` so the probe-style side-only
    /// artifacts are accepted.
    pub async fn spawn_proof(
        artifact: &Path,
        model: &str,
        runtime_version: &str,
        repo_root: &Path,
    ) -> Result<Self> {
        Self::spawn_inner(
            artifact,
            model,
            runtime_version,
            SpawnMode::Proof,
            Some(repo_root),
        )
        .await
    }

    async fn spawn_inner(
        artifact: &Path,
        model: &str,
        runtime_version: &str,
        mode: SpawnMode,
        repo_root: Option<&Path>,
    ) -> Result<Self> {
        let bin = std::env::var("ADR_MCP_BIN").unwrap_or_else(|_| "adr-mcp".into());
        let mut cmd = Command::new(&bin);
        cmd.arg("--artifact")
            .arg(artifact)
            .arg("--model")
            .arg(model)
            .arg("--runtime-version")
            .arg(runtime_version);
        match mode {
            SpawnMode::Synthesis => {}
            SpawnMode::Probe => {
                cmd.arg("--probe");
            }
            SpawnMode::Proof => {
                cmd.arg("--probe").arg("--proof");
                if let Some(root) = repo_root {
                    cmd.arg("--repo-root").arg(root);
                } else {
                    return Err(anyhow!("spawn_proof called without repo_root"));
                }
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning adr-mcp binary `{bin}`"))?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;

        // Pipe child stderr into our tracing output so MCP server logs
        // show up in the parent. Line-buffered.
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                tracing::debug!(target: "adr-mcp", "{l}");
            }
        });

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 1,
        })
    }

    /// MCP handshake. Must be called once before any tool calls.
    pub async fn initialize(&mut self) -> Result<Value> {
        let resp = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "adr-server", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;
        // Fire-and-forget the `initialized` notification per MCP spec.
        self.notify("notifications/initialized", Value::Null).await?;
        Ok(resp)
    }

    pub async fn list_tools(&mut self) -> Result<Vec<ToolSpec>> {
        let resp = self.request("tools/list", json!({})).await?;
        let arr = resp
            .get("tools")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("tools/list response missing `tools` array"))?;
        let mut out = Vec::with_capacity(arr.len());
        for t in arr {
            let name = t
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("tool missing `name`"))?
                .to_string();
            let description = t
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let input_schema = t
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object" }));
            out.push(ToolSpec {
                name,
                description,
                input_schema,
            });
        }
        Ok(out)
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<ToolCallResult> {
        let resp = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let is_error = resp
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let text = resp
            .get("content")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        if c.get("type").and_then(|v| v.as_str()) == Some("text") {
                            c.get("text").and_then(|v| v.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        Ok(ToolCallResult { is_error, text })
    }

    pub async fn shutdown(mut self) -> Result<()> {
        // Close stdin so the child's read loop exits; then wait briefly.
        drop(self.stdin);
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.child.wait(),
        )
        .await;
        Ok(())
    }

    /* ---- internals ----------------------------------------------------- */

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send(&msg).await?;
        loop {
            let frame = self.recv().await?;
            // Skip unrelated responses (shouldn't happen with this client,
            // but be permissive).
            if frame.get("id").and_then(|v| v.as_u64()) != Some(id) {
                tracing::warn!(?frame, "ignoring unrelated mcp frame");
                continue;
            }
            if let Some(err) = frame.get("error") {
                return Err(anyhow!("mcp error on {method}: {err}"));
            }
            return Ok(frame
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow!("mcp response missing `result`"))?);
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send(&msg).await
    }

    async fn send(&mut self, msg: &Value) -> Result<()> {
        let s = serde_json::to_string(msg)?;
        self.stdin.write_all(s.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Value> {
        loop {
            let line = self
                .stdout
                .next_line()
                .await?
                .ok_or_else(|| anyhow!("mcp child closed stdout"))?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            return Ok(serde_json::from_str(trimmed)
                .with_context(|| format!("parsing mcp frame: {trimmed}"))?);
        }
    }
}
