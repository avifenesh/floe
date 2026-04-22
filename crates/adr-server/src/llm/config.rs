//! Parse `ADR_LLM=<provider>:<model>` into a typed config.
//!
//! Examples that parse:
//! - `ADR_LLM=ollama:gemma4:26b-a4b-it-q4_K_M` → `Ollama` / `"gemma4:26b-a4b-it-q4_K_M"`
//! - `ADR_LLM=ollama:qwen3-coder:32b-instruct`
//!
//! Anything else (empty, malformed, or an unknown provider prefix) yields
//! `None`, which the server treats as "LLM synthesis disabled — keep the
//! structural flows."

use std::fmt;

/// The LLM-runtime provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    /// Local Ollama daemon.
    Ollama,
    /// Zhipu AI's GLM cloud API (OpenAI-compatible, Bearer auth).
    Glm,
}

impl fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ollama => write!(f, "ollama"),
            Self::Glm => write!(f, "glm"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    /// The model tag as the provider names it (e.g. `"gemma4:26b-..."`
    /// for Ollama, `"glm-4.6"` for GLM).
    pub model: String,
    /// Base URL for the provider's API. Defaults to the Ollama loopback
    /// for `ollama:` or the GLM public endpoint for `glm:`; override via
    /// `ADR_OLLAMA_URL` / `ADR_GLM_URL`.
    pub base_url: String,
    /// API key — required for `glm:`, unused for `ollama:`. Read from
    /// `ADR_GLM_API_KEY` (never logged or cached).
    pub api_key: Option<String>,
    /// The prompt version directory under `prompts/flow_synthesis/`.
    pub prompt_version: String,
    /// Context window (num_ctx) passed through Ollama options. Default 32K
    /// — big enough for the rendered prompt + ~20 tool-call round-trips of
    /// JSON payloads without triggering the model's own sliding-window
    /// truncation. Override via `ADR_OLLAMA_CTX`. Gemma 4 26B / Qwen 3.5
    /// can go to 256K; we cap at 64K by default because more context
    /// doesn't help a classification task and burns VRAM.
    pub num_ctx: u32,
    /// `num_predict` — max tokens per single assistant turn. Must be large
    /// enough that Ollama doesn't truncate a tool call mid-JSON, but not
    /// so large that an unintentional empty-content turn bleeds seconds.
    /// Override via `ADR_OLLAMA_PREDICT`.
    pub num_predict: i32,
    /// Sampling temperature. Classification wants low variance, but 0.2
    /// left Gemma 26B stuck in empty-response loops after a few turns;
    /// 0.4 gives enough variety to escape without making the tool-call
    /// JSON sloppy. Override via `ADR_OLLAMA_TEMP`.
    pub temperature: f32,
    /// `keep_alive` — how long Ollama keeps the model resident in VRAM
    /// after a request. Ollama's default is 5 min; we set 10m explicitly
    /// so the first-token latency is only paid once per run. Override
    /// via `ADR_OLLAMA_KEEP_ALIVE`.
    pub keep_alive: String,
}

impl LlmConfig {
    /// Parse `ADR_LLM` + related env vars. Returns `None` when no LLM is
    /// configured — caller keeps structural flows in that case.
    ///
    /// Convenience: if `ADR_LLM` is unset but `ADR_GLM_API_KEY` is
    /// present, default to `glm:glm-4.7` (Avi's daily-driver model from
    /// the coding-paas quota tier). This means a freshly-cloned workspace
    /// with the key exported just works without any provider wiring.
    pub fn from_env() -> Option<Self> {
        Self::from_env_key("ADR_LLM")
    }

    /// Parse the probe-specific config. Reads `ADR_PROBE_LLM` first,
    /// falls back to `ADR_LLM`. All other env knobs (`ADR_GLM_API_KEY`,
    /// `ADR_GLM_URL`, `ADR_OLLAMA_URL`, …) are shared — probe and
    /// synthesis are deliberately pinned to **different models** (for
    /// measurement stability) but the same provider wiring.
    pub fn from_env_probe() -> Option<Self> {
        Self::from_env_key("ADR_PROBE_LLM").or_else(Self::from_env)
    }

    /// Parse the intent/proof-specific config. Reads `ADR_PROOF_LLM`
    /// first; if unset, defaults to GLM-4.7 when `ADR_GLM_API_KEY` is
    /// present. Falls back to `ADR_LLM` as a last resort but emits a
    /// warning — intent + proof passes read prose (PR text, reviewer
    /// notes, free-form claims) and need strong analysis; small local
    /// models hallucinate on this workload (see
    /// `feedback_proof_uses_glm.md`).
    pub fn from_env_proof() -> Option<Self> {
        if let Some(cfg) = Self::from_env_key("ADR_PROOF_LLM") {
            if cfg.provider != LlmProvider::Glm {
                tracing::warn!(
                    provider = %cfg.provider,
                    model = %cfg.model,
                    "ADR_PROOF_LLM pinned to a non-GLM model — proof-verification \
                     reads unstructured prose and semantic intent matches; small \
                     local models hallucinate here. GLM cloud is the recommended \
                     backend."
                );
            }
            return Some(cfg);
        }
        // No explicit proof config — default to GLM when the key is
        // available, matching Avi's directive that proof needs strong
        // analysis with non-structural content.
        if std::env::var("ADR_GLM_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some()
        {
            tracing::info!(
                "ADR_PROOF_LLM unset — defaulting proof-verification to glm:glm-4.7"
            );
            return Self::build_glm("glm-4.7");
        }
        // Last-resort fallback to the main config. Warn loudly so the
        // reviewer understands why the proof claims might be weak.
        let fallback = Self::from_env()?;
        if fallback.provider != LlmProvider::Glm {
            tracing::warn!(
                provider = %fallback.provider,
                model = %fallback.model,
                "No GLM config for proof pass — falling back to ADR_LLM. Proof \
                 claims may be unreliable; set ADR_GLM_API_KEY to enable the \
                 default glm:glm-4.7 proof backend."
            );
        }
        Some(fallback)
    }

    /// Build a GLM config for a given model tag using ambient env.
    /// Returns `None` if the GLM API key isn't set.
    fn build_glm(model: &str) -> Option<Self> {
        let api_key = std::env::var("ADR_GLM_API_KEY").ok().filter(|s| !s.is_empty());
        api_key.as_ref()?;
        let base_url = std::env::var("ADR_GLM_URL")
            .unwrap_or_else(|_| super::glm_client::default_base_url().into());
        let prompt_version =
            std::env::var("ADR_PROMPT_VERSION").unwrap_or_else(|_| "v0.2.0".into());
        let defaults = super::model_defaults::defaults_for(LlmProvider::Glm, model);
        let num_predict: i32 = std::env::var("ADR_GLM_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.max_tokens as i32);
        let temperature: f32 = std::env::var("ADR_GLM_TEMP")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.temperature);
        Some(Self {
            provider: LlmProvider::Glm,
            model: model.to_string(),
            base_url,
            api_key,
            prompt_version,
            num_ctx: 16_384,
            num_predict,
            temperature,
            keep_alive: "10m".into(),
        })
    }

    fn from_env_key(key: &str) -> Option<Self> {
        let raw = std::env::var(key).ok().filter(|s| !s.is_empty());
        let raw = raw.or_else(|| {
            // Only apply the "GLM key alone → default" for the main
            // LlmConfig key; probe should fall back to the main config
            // rather than silently pinning to glm-4.7 on its own.
            if key == "ADR_LLM"
                && std::env::var("ADR_GLM_API_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .is_some()
            {
                tracing::info!(
                    "ADR_LLM unset but ADR_GLM_API_KEY present — defaulting to glm:glm-4.7"
                );
                Some("glm:glm-4.7".to_string())
            } else {
                None
            }
        })?;
        let (provider_str, model) = raw.split_once(':')?;
        let provider = match provider_str {
            "ollama" => LlmProvider::Ollama,
            "glm" => LlmProvider::Glm,
            _ => return None,
        };
        if model.is_empty() {
            return None;
        }
        let (base_url, api_key) = match provider {
            LlmProvider::Ollama => (
                std::env::var("ADR_OLLAMA_URL")
                    .unwrap_or_else(|_| "http://localhost:11434".into()),
                None,
            ),
            LlmProvider::Glm => {
                let url = std::env::var("ADR_GLM_URL")
                    .unwrap_or_else(|_| super::glm_client::default_base_url().into());
                let key = std::env::var("ADR_GLM_API_KEY").ok().filter(|s| !s.is_empty());
                if key.is_none() {
                    tracing::warn!("ADR_LLM=glm:… requested but ADR_GLM_API_KEY is not set; falling back to structural");
                    return None;
                }
                (url, key)
            }
        };
        let prompt_version = std::env::var("ADR_PROMPT_VERSION").unwrap_or_else(|_| "v0.2.0".into());
        let num_ctx = std::env::var("ADR_OLLAMA_CTX")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(16_384);
        // Per-model defaults are the fallback; user env overrides always win.
        let defaults = super::model_defaults::defaults_for(provider, model);
        let max_tokens_env = match provider {
            LlmProvider::Ollama => "ADR_OLLAMA_PREDICT",
            LlmProvider::Glm => "ADR_GLM_MAX_TOKENS",
        };
        let temp_env = match provider {
            LlmProvider::Ollama => "ADR_OLLAMA_TEMP",
            LlmProvider::Glm => "ADR_GLM_TEMP",
        };
        let num_predict: i32 = std::env::var(max_tokens_env)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.max_tokens as i32);
        let temperature: f32 = std::env::var(temp_env)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.temperature);
        let keep_alive = std::env::var("ADR_OLLAMA_KEEP_ALIVE").unwrap_or_else(|_| "10m".into());
        Some(Self {
            provider,
            model: model.to_string(),
            base_url,
            api_key,
            prompt_version,
            num_ctx,
            num_predict,
            temperature,
            keep_alive,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global; serialize access so parallel test
    // execution doesn't interleave set/unset across cases.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env(key: &str, val: Option<&str>, f: impl FnOnce()) {
        with_envs(&[(key, val)], f);
    }

    /// Set multiple env vars atomically under a single lock so nested
    /// `with_env` calls don't deadlock on the shared mutex.
    fn with_envs(entries: &[(&str, Option<&str>)], f: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev: Vec<(String, Option<String>)> = entries
            .iter()
            .map(|(k, _)| ((*k).to_string(), std::env::var(*k).ok()))
            .collect();
        for (k, v) in entries {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, v) in prev {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }

    #[test]
    fn parses_ollama() {
        with_env("ADR_LLM", Some("ollama:gemma4:26b-a4b-it-q4_K_M"), || {
            let c = LlmConfig::from_env().unwrap();
            assert_eq!(c.provider, LlmProvider::Ollama);
            assert_eq!(c.model, "gemma4:26b-a4b-it-q4_K_M");
            assert!(c.api_key.is_none());
        });
    }

    #[test]
    fn parses_glm_with_key() {
        with_envs(
            &[("ADR_LLM", Some("glm:glm-4.6")), ("ADR_GLM_API_KEY", Some("test-key"))],
            || {
                let c = LlmConfig::from_env().unwrap();
                assert_eq!(c.provider, LlmProvider::Glm);
                assert_eq!(c.model, "glm-4.6");
                assert_eq!(c.api_key.as_deref(), Some("test-key"));
            },
        );
    }

    #[test]
    fn glm_without_key_rejected() {
        with_envs(
            &[("ADR_LLM", Some("glm:glm-4.6")), ("ADR_GLM_API_KEY", None)],
            || {
                assert!(LlmConfig::from_env().is_none());
            },
        );
    }

    #[test]
    fn key_alone_defaults_to_glm_47() {
        with_envs(
            &[("ADR_LLM", None), ("ADR_GLM_API_KEY", Some("test-key"))],
            || {
                let c = LlmConfig::from_env().expect("should default to glm");
                assert_eq!(c.provider, LlmProvider::Glm);
                assert_eq!(c.model, "glm-4.7");
                assert_eq!(c.api_key.as_deref(), Some("test-key"));
            },
        );
    }

    #[test]
    fn unknown_provider_rejected() {
        with_env("ADR_LLM", Some("openai:gpt-5"), || {
            assert!(LlmConfig::from_env().is_none());
        });
    }

    #[test]
    fn missing_model_rejected() {
        with_env("ADR_LLM", Some("ollama:"), || {
            assert!(LlmConfig::from_env().is_none());
        });
    }

    #[test]
    fn unset_is_none() {
        with_env("ADR_LLM", None, || {
            assert!(LlmConfig::from_env().is_none());
        });
    }

    #[test]
    fn proof_defaults_to_glm_47_when_key_present() {
        with_envs(
            &[
                ("ADR_PROOF_LLM", None),
                ("ADR_LLM", Some("ollama:gemma4:26b")),
                ("ADR_GLM_API_KEY", Some("k")),
            ],
            || {
                let c = LlmConfig::from_env_proof().expect("should default to glm");
                assert_eq!(c.provider, LlmProvider::Glm);
                assert_eq!(c.model, "glm-4.7");
            },
        );
    }

    #[test]
    fn proof_explicit_setting_wins_over_default() {
        with_envs(
            &[
                ("ADR_PROOF_LLM", Some("glm:glm-4.6")),
                ("ADR_GLM_API_KEY", Some("k")),
            ],
            || {
                let c = LlmConfig::from_env_proof().unwrap();
                assert_eq!(c.model, "glm-4.6");
            },
        );
    }

    #[test]
    fn proof_falls_back_to_main_without_glm_key() {
        with_envs(
            &[
                ("ADR_PROOF_LLM", None),
                ("ADR_LLM", Some("ollama:gemma4:26b")),
                ("ADR_GLM_API_KEY", None),
            ],
            || {
                let c = LlmConfig::from_env_proof().unwrap();
                assert_eq!(c.provider, LlmProvider::Ollama);
            },
        );
    }
}
