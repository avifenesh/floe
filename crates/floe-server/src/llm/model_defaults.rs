//! Per-model defaults for `max_tokens` + `temperature`. User overrides
//! via `FLOE_GLM_MAX_TOKENS` / `FLOE_GLM_TEMP` / `FLOE_OLLAMA_PREDICT` /
//! `FLOE_OLLAMA_TEMP` always win; these defaults apply only when no
//! explicit override is set.
//!
//! Philosophy (2026-04-18, Avi): "we should give enough to have results
//! correct plus headroom", and "temperature should be closer to one if we
//! want structural output". The second is counter-intuitive versus the
//! textbook "low temperature = deterministic JSON" advice, but it holds
//! empirically on the hybrid-reasoning GLM tier — a bit of sampling
//! entropy helps the model escape reasoning-mode ruts and actually emit
//! the tool call. Ollama local models keep a lower temperature (0.4) that
//! was validated against Qwen 3.5 27B on glide-mq #181.

use super::config::LlmProvider;

pub struct ModelDefaults {
    /// Max tokens per assistant turn. For GLM this caps total output
    /// including `reasoning_content`; for Ollama it's `num_predict`.
    pub max_tokens: u64,
    pub temperature: f32,
}

/// Return the best-known defaults for a `(provider, model)` pair, falling
/// back to sensible per-provider generics for unknown models.
pub fn defaults_for(provider: LlmProvider, model: &str) -> ModelDefaults {
    match provider {
        LlmProvider::Glm => glm_defaults(model),
        LlmProvider::Ollama => ollama_defaults(model),
    }
}

fn glm_defaults(model: &str) -> ModelDefaults {
    // Reasoning-family models (`glm-4.5+`, `glm-5.x`, `glm-4.6`) spend
    // tokens on `reasoning_content` alongside visible output, so they
    // need a bigger budget. Pure-output models (`glm-4.5-air`, `glm-4.7`)
    // still get plenty of headroom.
    let lower = model.to_ascii_lowercase();
    if lower.starts_with("glm-5") {
        ModelDefaults { max_tokens: 16_384, temperature: 0.9 }
    } else if lower.starts_with("glm-4.6") {
        ModelDefaults { max_tokens: 8_192, temperature: 0.85 }
    } else if lower.starts_with("glm-4.5") {
        // Includes `glm-4.5`, `glm-4.5-air`, `glm-4.5-turbo`, etc.
        ModelDefaults { max_tokens: 6_144, temperature: 0.85 }
    } else if lower.starts_with("glm-4.7") {
        ModelDefaults { max_tokens: 6_144, temperature: 0.8 }
    } else {
        // Unknown GLM variant — generous floor, mid temperature.
        ModelDefaults { max_tokens: 8_192, temperature: 0.8 }
    }
}

fn ollama_defaults(model: &str) -> ModelDefaults {
    // Same "closer to 1 for structural output" principle as GLM — a bit
    // of sampling entropy helps the model escape reasoning-mode ruts and
    // actually emit the tool call. Previously Qwen/Gemma defaulted to
    // 0.4 empirically; bumped to 0.8 on 2026-04-18 for consistency.
    let lower = model.to_ascii_lowercase();
    // Qwen 3.5 / 3 gets a wider predict budget — it emits richer
    // per-flow rationales than the smaller local models. Everything
    // else (including the dropped Gemma 4 tier, kept for anyone
    // running it off-label) shares the conservative 1024 default.
    if lower.starts_with("qwen3.5") || lower.starts_with("qwen3") {
        ModelDefaults { max_tokens: 2_048, temperature: 0.8 }
    } else {
        ModelDefaults { max_tokens: 1_024, temperature: 0.8 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glm_5_gets_big_budget() {
        let d = defaults_for(LlmProvider::Glm, "glm-5.1");
        assert!(d.max_tokens >= 8192);
        assert!(d.temperature >= 0.8);
    }

    #[test]
    fn glm_47_gets_mid_budget() {
        let d = defaults_for(LlmProvider::Glm, "glm-4.7");
        assert!(d.max_tokens >= 4096);
    }

    #[test]
    fn qwen_gets_ollama_defaults() {
        let d = defaults_for(LlmProvider::Ollama, "qwen3.5:27b-q4_K_M");
        assert!(d.max_tokens <= 4096);
        // Structural-output principle applies to local reasoning models
        // too — temperature lives in the 0.7–0.9 band.
        assert!(d.temperature >= 0.7 && d.temperature <= 0.95);
    }

    #[test]
    fn unknown_glm_gets_safe_fallback() {
        let d = defaults_for(LlmProvider::Glm, "glm-hypothetical-10");
        assert!(d.max_tokens >= 4096);
    }
}
