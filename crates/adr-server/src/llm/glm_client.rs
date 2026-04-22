//! Minimal Zhipu GLM chat client. GLM speaks the OpenAI chat-completions
//! shape at `https://api.z.ai/api/coding/paas/v4/chat/completions` (the
//! coding-plan endpoint Avi subscribes to) with Bearer auth and a
//! stringified `arguments` field on tool calls (OpenAI-style, not
//! Ollama-style object-arguments).
//!
//! We normalise the response into the same [`ChatResponse`] the Ollama
//! client emits so the agent loop doesn't care which provider answered.
//!
//! References:
//! - <https://docs.z.ai/api-reference/llm/chat-completion>
//! - <https://open.bigmodel.cn/dev/api>

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use super::ollama_client::{ChatMessage, ChatRequest, ChatResponse, ToolCall, ToolCallFunction};

const DEFAULT_BASE_URL: &str = "https://api.z.ai/api/coding/paas/v4";

/// Default concurrency cap on in-flight GLM calls across the whole
/// process. GLM's coding-paas tier rate-limits at a low burst —
/// firing all the intent + proof + probe sessions in parallel hits
/// 1302 `Rate limit reached for requests`. 3 is the empirical sweet
/// spot — 1× probe side (3 probes sequentially inside the pipeline)
/// + 2 proof sessions overlapping keeps the running queue busy
/// without tripping the limiter. Override via `ADR_GLM_CONCURRENCY`.
const DEFAULT_CONCURRENCY: usize = 3;

/// Per-call 429 retry budget. Bounded — the circuit breaker layer
/// above takes over once the bucket is empty.
const PER_CALL_MAX_RETRIES: u32 = 3;

/// Initial backoff on 429. Doubles each retry (1s, 2s, 4s) with
/// ±25% jitter.
const BACKOFF_BASE: Duration = Duration::from_millis(1000);

/// Consecutive 429s from a Closed breaker that trip it to Open.
const TRIP_THRESHOLD: u32 = 3;

/// How long the breaker stays Open before one HalfOpen probe.
/// Doubles on repeated trips (capped at `MAX_COOLDOWN`) so a
/// persistently-rate-limited account backs off instead of hammering.
const INITIAL_COOLDOWN: Duration = Duration::from_secs(20);
const MAX_COOLDOWN: Duration = Duration::from_secs(300);

/// Process-wide GLM semaphore. Shared across every `GlmClient` so
/// probe + intent + proof pipelines all respect the same cap.
fn glm_semaphore() -> &'static Arc<Semaphore> {
    static SEM: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| {
        let permits = std::env::var("ADR_GLM_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_CONCURRENCY);
        tracing::info!(
            permits,
            "glm concurrency semaphore armed — configure ADR_GLM_CONCURRENCY to change"
        );
        Arc::new(Semaphore::new(permits))
    })
}

/// Process-wide GLM circuit breaker. Three states:
///
/// - **Closed** — normal operation. Per-call retry with backoff
///   handles transient 429s. After [`TRIP_THRESHOLD`] consecutive
///   429s, the breaker flips to Open.
/// - **Open** — calls fail fast with a `circuit-open` error until
///   the cooldown elapses. Cooldown starts at [`INITIAL_COOLDOWN`]
///   and doubles on repeated trips (capped at [`MAX_COOLDOWN`]) so
///   a truly rate-limited account stops hammering.
/// - **HalfOpen** — exactly one call is allowed through as a probe.
///   If it succeeds the breaker closes and cooldown resets; if it
///   429s again, the breaker reopens with doubled cooldown.
///
/// This keeps the worst case bounded: `MAX_RETRIES × BACKOFF` per
/// call **and** a rate-limited account won't see every subsequent
/// call sit through the retry loop — they bail immediately with a
/// clear "circuit open" error until the window expires.
struct Breaker {
    state: BreakerState,
    /// Consecutive 429s while Closed. Resets on any success.
    consecutive_429: u32,
    /// Current cooldown duration — doubles each time the breaker
    /// reopens, resets to `INITIAL_COOLDOWN` on close.
    cooldown: Duration,
}

enum BreakerState {
    Closed,
    /// Calls refused until `until`.
    Open { until: Instant },
    /// One probe in flight; other calls are refused until it resolves.
    HalfOpen { probe_in_flight: bool },
}

impl Breaker {
    fn new() -> Self {
        Self {
            state: BreakerState::Closed,
            consecutive_429: 0,
            cooldown: INITIAL_COOLDOWN,
        }
    }
}

fn glm_breaker() -> &'static Arc<Mutex<Breaker>> {
    static B: OnceLock<Arc<Mutex<Breaker>>> = OnceLock::new();
    B.get_or_init(|| Arc::new(Mutex::new(Breaker::new())))
}

/// Result of the breaker's pre-call check.
enum BreakerGate {
    /// Call may proceed. If `is_probe`, caller must update breaker
    /// state based on the outcome.
    Proceed { is_probe: bool },
    /// Circuit is open; call is refused. Caller fails fast.
    Refused { retry_after: Duration },
}

fn breaker_check_or_open() -> BreakerGate {
    let mut b = glm_breaker().lock().expect("breaker mutex poisoned");
    match b.state {
        BreakerState::Closed => BreakerGate::Proceed { is_probe: false },
        BreakerState::Open { until } => {
            let now = Instant::now();
            if now >= until {
                // Transition Open → HalfOpen: let one call probe.
                b.state = BreakerState::HalfOpen { probe_in_flight: true };
                BreakerGate::Proceed { is_probe: true }
            } else {
                BreakerGate::Refused { retry_after: until - now }
            }
        }
        BreakerState::HalfOpen { probe_in_flight: true } => BreakerGate::Refused {
            retry_after: Duration::from_secs(1),
        },
        BreakerState::HalfOpen { probe_in_flight: false } => {
            // Should not happen — a HalfOpen without an in-flight
            // probe should have transitioned. Treat as Closed to
            // recover.
            b.state = BreakerState::Closed;
            b.consecutive_429 = 0;
            BreakerGate::Proceed { is_probe: false }
        }
    }
}

fn breaker_record_success(was_probe: bool) {
    let mut b = glm_breaker().lock().expect("breaker mutex poisoned");
    b.consecutive_429 = 0;
    if was_probe {
        tracing::info!("glm circuit breaker: probe succeeded, closing");
        b.state = BreakerState::Closed;
        b.cooldown = INITIAL_COOLDOWN;
    }
}

fn breaker_record_429(was_probe: bool) {
    let mut b = glm_breaker().lock().expect("breaker mutex poisoned");
    b.consecutive_429 = b.consecutive_429.saturating_add(1);
    let should_open = matches!(b.state, BreakerState::Closed) && b.consecutive_429 >= TRIP_THRESHOLD;
    if should_open {
        let cooldown = b.cooldown;
        tracing::warn!(
            cooldown_ms = cooldown.as_millis(),
            "glm circuit breaker: tripping Closed → Open after {} consecutive 429s",
            TRIP_THRESHOLD
        );
        b.state = BreakerState::Open {
            until: Instant::now() + cooldown,
        };
        // Arm next cooldown (doubled, capped).
        b.cooldown = (cooldown * 2).min(MAX_COOLDOWN);
        return;
    }
    if was_probe {
        let cooldown = b.cooldown;
        tracing::warn!(
            cooldown_ms = cooldown.as_millis(),
            "glm circuit breaker: probe 429, reopening"
        );
        b.state = BreakerState::Open {
            until: Instant::now() + cooldown,
        };
        b.cooldown = (cooldown * 2).min(MAX_COOLDOWN);
    }
}

/// Release a HalfOpen probe slot when a probe call errored with
/// something other than 429 (so the outcome doesn't count as "GLM
/// is rate-limiting" — it's an unrelated fault).
fn breaker_release_probe_on_error() {
    let mut b = glm_breaker().lock().expect("breaker mutex poisoned");
    if let BreakerState::HalfOpen { .. } = b.state {
        // Leave the window open; allow the next call to try again.
        b.state = BreakerState::HalfOpen {
            probe_in_flight: false,
        };
    }
}

pub struct GlmClient {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl GlmClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Accepts an Ollama-shaped [`ChatRequest`] and forwards it to GLM,
    /// dropping the Ollama-only `keep_alive` field (GLM manages model
    /// residence server-side; no equivalent knob).
    ///
    /// Two throttling layers live here:
    /// 1. **Process-wide semaphore** (`ADR_GLM_CONCURRENCY`) — only N
    ///    GLM calls in flight across all pipelines (probe + intent +
    ///    proof). This keeps burst parallelism well under the
    ///    coding-paas rate limiter.
    /// 2. **429 retry with exponential backoff** — if the cap slips
    ///    through anyway (other processes, upstream spikes), we
    ///    retry [`MAX_RETRIES`] times with doubling delay. All other
    ///    HTTP errors surface immediately.
    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let (temperature, max_tokens) = extract_sampling(&req.options);
        let body = json!({
            "model": req.model,
            "messages": req.messages.iter().map(glm_out_message).collect::<Vec<_>>(),
            "tools": req.tools,
            "tool_choice": "auto",
            "stream": false,
            "temperature": temperature.unwrap_or(0.8),
            "max_tokens": max_tokens.unwrap_or(8192),
        });
        if std::env::var("ADR_LLM_DEBUG").is_ok() {
            tracing::debug!(body = %serde_json::to_string(&body).unwrap_or_default(), "glm chat body");
        }

        // Breaker gate first — fail fast if the circuit is open.
        // If we're the HalfOpen probe, remember so we can record the
        // outcome correctly.
        let is_probe = match breaker_check_or_open() {
            BreakerGate::Proceed { is_probe } => is_probe,
            BreakerGate::Refused { retry_after } => {
                return Err(anyhow!(
                    "glm circuit breaker open — refusing call for {}s (rate-limit cooldown)",
                    retry_after.as_secs().max(1)
                ));
            }
        };

        let sem = glm_semaphore();
        let mut attempt = 0u32;
        let outcome = loop {
            // Acquire a permit BEFORE each attempt. Released on retry
            // so the next caller can try while we back off.
            let _permit = sem
                .clone()
                .acquire_owned()
                .await
                .map_err(|e| anyhow!("glm semaphore closed: {e}"))?;

            let resp = self
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .with_context(|| format!("POST {url}"));
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    // Network-layer error — not rate-limit-related.
                    // Don't count against the breaker; free the probe
                    // slot so the next caller can try.
                    if is_probe {
                        breaker_release_probe_on_error();
                    }
                    break Err(e);
                }
            };
            let status = resp.status();
            let text = match resp.text().await.context("reading glm body") {
                Ok(t) => t,
                Err(e) => {
                    if is_probe {
                        breaker_release_probe_on_error();
                    }
                    break Err(e);
                }
            };
            if std::env::var("ADR_LLM_DEBUG").is_ok() {
                tracing::debug!(status = %status, body = %text, "glm chat response");
            }
            if status.is_success() {
                break Ok(text);
            }
            // 429 → per-call retry budget; anything else → bail.
            if status.as_u16() == 429 && attempt < PER_CALL_MAX_RETRIES {
                drop(_permit);
                let backoff = jittered_backoff(attempt);
                tracing::warn!(
                    attempt = attempt + 1,
                    max = PER_CALL_MAX_RETRIES,
                    backoff_ms = backoff.as_millis(),
                    "glm 429 — retrying with backoff"
                );
                tokio::time::sleep(backoff).await;
                attempt += 1;
                continue;
            }
            // Budget exhausted on a 429, or non-429 failure.
            let is_429 = status.as_u16() == 429;
            break Err(anyhow!("glm HTTP {status}: {text}"))
                .map_err(|e| {
                    if is_429 {
                        breaker_record_429(is_probe);
                    } else if is_probe {
                        breaker_release_probe_on_error();
                    }
                    e
                });
        };

        match outcome {
            Ok(text) => {
                breaker_record_success(is_probe);
                parse_glm_response(&text)
            }
            Err(e) => Err(e),
        }
    }
}

/// Exponential backoff with ±25% jitter. Attempt 0 → ~1s, 1 → ~2s,
/// 2 → ~4s, 3 → ~8s. Jitter reduces thundering-herd when many
/// sessions hit 429 at once.
fn jittered_backoff(attempt: u32) -> Duration {
    // Cheap LCG jitter — we don't need cryptographic randomness here,
    // just "different enough across concurrent callers".
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as u64;
    let base_ms = (BACKOFF_BASE.as_millis() as u64) << attempt;
    let jitter_range = base_ms / 2; // ±25%
    let jitter = (nanos % jitter_range.max(1)) as i64 - (jitter_range as i64 / 2);
    let total = (base_ms as i64 + jitter).max(100) as u64;
    Duration::from_millis(total)
}

/// Map our (Ollama-shaped) message to GLM/OpenAI shape. The only
/// structural difference is that a `role: "tool"` message uses
/// `tool_call_id` in OpenAI land; our simplified agent doesn't track
/// per-call ids, so we fall back to the tool name — GLM accepts it.
fn glm_out_message(m: &ChatMessage) -> Value {
    let mut v = json!({ "role": m.role, "content": m.content });
    if !m.tool_calls.is_empty() {
        // OpenAI-style outgoing tool_calls serialize `arguments` as a
        // stringified JSON. Ours are already `Value`; stringify here.
        let calls: Vec<Value> = m
            .tool_calls
            .iter()
            .map(|c| {
                json!({
                    "type": "function",
                    "function": {
                        "name": c.function.name,
                        "arguments": serde_json::to_string(&c.function.arguments).unwrap_or_default(),
                    },
                })
            })
            .collect();
        v["tool_calls"] = Value::Array(calls);
    }
    if let Some(name) = &m.tool_name {
        v["tool_call_id"] = Value::String(name.clone());
    }
    v
}

fn extract_sampling(options: &Option<Value>) -> (Option<f64>, Option<u64>) {
    let mut temperature = None;
    let mut max_tokens = None;
    if let Some(Value::Object(m)) = options {
        if let Some(Value::Number(n)) = m.get("temperature") {
            temperature = n.as_f64();
        }
        if let Some(Value::Number(n)) = m.get("num_predict") {
            max_tokens = n.as_u64();
        }
    }
    (temperature, max_tokens)
}

/* -------------------------------------------------------------------------- */
/* Response parsing                                                           */
/* -------------------------------------------------------------------------- */

#[derive(Deserialize)]
struct GlmResponse {
    choices: Vec<GlmChoice>,
    #[serde(default)]
    usage: Option<GlmUsage>,
}

#[derive(Deserialize)]
struct GlmUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct GlmChoice {
    message: GlmMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GlmMessage {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: Option<String>,
    /// Hybrid reasoning models (GLM-4.5, 4.6, 4.7) emit this alongside
    /// `content`. We surface its presence + length in diagnostic logs
    /// when `content` is empty — if the model is dumping its answer
    /// here instead of in `content`, the agent loop sees a blank turn.
    #[serde(default, rename = "reasoning_content")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<GlmToolCall>,
}

#[derive(Deserialize)]
struct GlmToolCall {
    #[serde(rename = "type")]
    _kind: Option<String>,
    function: GlmToolCallFunction,
}

#[derive(Deserialize)]
struct GlmToolCallFunction {
    name: String,
    /// Stringified JSON per OpenAI spec — we parse it into a `Value` so
    /// the rest of the loop treats it identically to Ollama's native
    /// object-arguments.
    arguments: String,
}

fn parse_glm_response(body: &str) -> Result<ChatResponse> {
    let parsed: GlmResponse = serde_json::from_str(body)
        .with_context(|| format!("parsing glm response: {body}"))?;
    let choice = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("glm response has no choices"))?;
    let finish_reason = choice.finish_reason.clone();

    // Diagnostic: when `content` is absent or blank AND no tool calls
    // AND no reasoning_content either, the agent loop will see a
    // blank turn and nudge (or bail). We log the shape so "empty
    // turn" cases are debuggable — is it a truncated response (length
    // finish_reason)? a reasoning-only dump (content null, reasoning
    // populated)? a filtered output (content_filter)? etc.
    let content_len = choice.message.content.as_deref().map(|s| s.len()).unwrap_or(0);
    let tool_calls_count = choice.message.tool_calls.len();
    let reasoning_len = choice
        .message
        .reasoning_content
        .as_deref()
        .map(|s| s.len())
        .unwrap_or(0);
    if content_len == 0 && tool_calls_count == 0 {
        tracing::warn!(
            finish_reason = ?finish_reason,
            content_present = choice.message.content.is_some(),
            content_len,
            reasoning_len,
            tool_calls_count,
            "glm response had no content and no tool calls"
        );
        // If the model put its answer in reasoning_content but the
        // finish_reason signals a clean stop, try promoting reasoning
        // to content. Better than bailing — the downstream JSON
        // extractor tolerates prose around the object.
        if reasoning_len > 0
            && finish_reason.as_deref().unwrap_or("") != "length"
        {
            if let Some(r) = choice.message.reasoning_content.clone() {
                tracing::info!(
                    reasoning_preview = %r.chars().take(200).collect::<String>(),
                    "promoting reasoning_content to content (empty content + clean finish)"
                );
                let tool_calls: Vec<ToolCall> = Vec::new();
                let message = ChatMessage {
                    role: if choice.message.role.is_empty() {
                        "assistant".into()
                    } else {
                        choice.message.role.clone()
                    },
                    content: r,
                    tool_calls,
                    tool_name: None,
                };
                let (tokens_in, tokens_out) = parsed
                    .usage
                    .map(|u| (u.prompt_tokens, u.completion_tokens))
                    .unwrap_or_default();
                return Ok(ChatResponse {
                    message,
                    done: true,
                    done_reason: finish_reason,
                    tokens_in,
                    tokens_out,
                });
            }
        }
    } else if matches!(finish_reason.as_deref(), Some("length")) {
        tracing::warn!(
            content_len,
            tool_calls_count,
            "glm finish_reason=length — response truncated mid-output, downstream parsers may fail"
        );
    }

    let tool_calls: Vec<ToolCall> = choice
        .message
        .tool_calls
        .into_iter()
        .map(|tc| {
            // GLM under load occasionally returns malformed JSON in
            // arguments (zai-org/GLM-5 #15). Try a one-shot repair: if
            // the parse fails, wrap in `{}` as an empty object — the
            // host's per-call validation will reject, which the model
            // can then correct on the next turn.
            let args = serde_json::from_str::<Value>(&tc.function.arguments)
                .unwrap_or_else(|_| Value::Object(Default::default()));
            ToolCall {
                function: ToolCallFunction {
                    name: tc.function.name,
                    arguments: args,
                },
            }
        })
        .collect();
    let message = ChatMessage {
        role: if choice.message.role.is_empty() {
            "assistant".into()
        } else {
            choice.message.role
        },
        content: choice.message.content.unwrap_or_default(),
        tool_calls,
        tool_name: None,
    };
    let (tokens_in, tokens_out) = parsed
        .usage
        .map(|u| (u.prompt_tokens, u.completion_tokens))
        .unwrap_or_default();
    Ok(ChatResponse {
        message,
        done: true,
        done_reason: finish_reason,
        tokens_in,
        tokens_out,
    })
}

pub fn default_base_url() -> &'static str {
    DEFAULT_BASE_URL
}

#[derive(Debug, Serialize)]
struct _UnusedMarker;
