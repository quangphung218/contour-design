//! BYOK provider presets (the original's `KNOWN_PROVIDERS` that speak a
//! protocol the daemon supports: Anthropic Messages or OpenAI-compatible
//! chat completions) and key cleanup.

/// A preset: label, protocol, base URL, suggested models (first = default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    /// `anthropic` or `openai`.
    pub protocol: &'static str,
    pub base_url: &'static str,
    pub models: &'static [&'static str],
    /// Self-hosted endpoints run without a key.
    pub needs_key: bool,
}

const fn p(
    id: &'static str,
    label: &'static str,
    protocol: &'static str,
    base_url: &'static str,
    models: &'static [&'static str],
) -> Preset {
    Preset {
        id,
        label,
        protocol,
        base_url,
        models,
        needs_key: true,
    }
}

/// In the original's picker order; `custom` last.
pub const PRESETS: &[Preset] = &[
    p(
        "anthropic",
        "Anthropic",
        "anthropic",
        "https://api.anthropic.com",
        &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
    ),
    p(
        "openai",
        "OpenAI",
        "openai",
        "https://api.openai.com/v1",
        &["gpt-5", "gpt-5-mini", "o3", "o4-mini"],
    ),
    p(
        "atlas",
        "Atlas Cloud",
        "openai",
        "https://api.atlascloud.ai/v1",
        &["qwen/qwen3.5-flash"],
    ),
    p(
        "gemini",
        "Google Gemini",
        "openai",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        &["gemini-3.5-flash", "gemini-3.5-pro"],
    ),
    p(
        "ollama-cloud",
        "Ollama Cloud",
        "openai",
        "https://ollama.com/v1",
        &["gpt-oss:120b"],
    ),
    p(
        "siliconflow-cn",
        "SiliconFlow (CN)",
        "openai",
        "https://api.siliconflow.cn/v1",
        &["deepseek-ai/DeepSeek-V3.1"],
    ),
    p(
        "siliconflow",
        "SiliconFlow (Global)",
        "openai",
        "https://api.siliconflow.com/v1",
        &["deepseek-ai/DeepSeek-V3.1"],
    ),
    p(
        "ppio",
        "PPIO",
        "openai",
        "https://api.ppinfra.com/v3/openai",
        &["deepseek/deepseek-v3.1"],
    ),
    p(
        "novita",
        "Novita AI",
        "openai",
        "https://api.novita.ai/openai",
        &["deepseek/deepseek-v4-pro"],
    ),
    p(
        "nvidia",
        "NVIDIA",
        "openai",
        "https://integrate.api.nvidia.com/v1",
        &["openai/gpt-oss-120b"],
    ),
    p(
        "stepfun",
        "StepFun",
        "openai",
        "https://api.stepfun.ai/v1",
        &["step-2-mini"],
    ),
    p(
        "deepseek",
        "DeepSeek",
        "openai",
        "https://api.deepseek.com",
        &["deepseek-v4-flash"],
    ),
    p(
        "openrouter",
        "OpenRouter",
        "openai",
        "https://openrouter.ai/api/v1",
        &["anthropic/claude-sonnet-4.6"],
    ),
    p(
        "mistral",
        "Mistral AI",
        "openai",
        "https://api.mistral.ai/v1",
        &["mistral-large-latest"],
    ),
    p("xai", "xAI", "openai", "https://api.x.ai/v1", &["grok-4"]),
    p(
        "together",
        "Together AI",
        "openai",
        "https://api.together.xyz/v1",
        &["meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo"],
    ),
    p(
        "huggingface",
        "Hugging Face",
        "openai",
        "https://router.huggingface.co/v1",
        &["openai/gpt-oss-120b"],
    ),
    p(
        "qwen",
        "Qwen",
        "openai",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        &["qwen-plus"],
    ),
    p(
        "volcengine",
        "Volcengine Ark",
        "openai",
        "https://ark.cn-beijing.volces.com/api/v3",
        &["doubao-seed-1-6"],
    ),
    p(
        "qianfan",
        "Baidu Qianfan",
        "openai",
        "https://qianfan.baidubce.com/v2",
        &["ernie-4.5-turbo-128k"],
    ),
    Preset {
        needs_key: false,
        ..p(
            "vllm",
            "vLLM",
            "openai",
            "http://127.0.0.1:8000/v1",
            &["model"],
        )
    },
    Preset {
        needs_key: false,
        ..p(
            "ollama",
            "Ollama (self-hosted)",
            "openai",
            "http://localhost:11434/v1",
            &["llama3.1"],
        )
    },
    p(
        "mimo",
        "Xiaomi MiMo",
        "openai",
        "https://token-plan-cn.xiaomimimo.com/v1",
        &["mimo-v2.5-pro"],
    ),
    p(
        "minimax",
        "MiniMax",
        "anthropic",
        "https://api.minimaxi.com/anthropic",
        &["MiniMax-M2.7-highspeed"],
    ),
    p(
        "moonshot",
        "Moonshot",
        "openai",
        "https://api.moonshot.cn/v1",
        &["kimi-k2.6"],
    ),
    p(
        "zhipu",
        "Zhipu AI",
        "openai",
        "https://open.bigmodel.cn/api/paas/v4",
        &["glm-4.6"],
    ),
    p(
        "aihubmix",
        "AIHubMix",
        "openai",
        "https://aihubmix.com/v1",
        &["gpt-5"],
    ),
];

#[must_use]
pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// The preset a stored config matches (protocol + base URL), if any.
#[must_use]
pub fn preset_for(protocol: &str, base_url: Option<&str>) -> Option<&'static Preset> {
    let norm = |u: &str| u.trim().trim_end_matches('/').to_ascii_lowercase();
    match base_url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(url) => PRESETS
            .iter()
            .find(|p| p.protocol == protocol && norm(p.base_url) == norm(url)),
        None => PRESETS
            .iter()
            .find(|p| p.protocol == protocol && matches!(p.id, "anthropic" | "openai")),
    }
}

/// Removes what a paste drags along: zero-width characters, line breaks,
/// tabs and surrounding spaces. Returns the key and whether it changed.
#[must_use]
pub fn clean_key(raw: &str) -> (String, bool) {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\r' | '\n' | '\t'
            )
        })
        .collect::<String>()
        .trim()
        .to_owned();
    let changed = cleaned != raw;
    (cleaned, changed)
}

/// A warning for keys whose shape does not fit a first-party host.
#[must_use]
pub fn key_shape_warning(preset_id: &str, key: &str) -> bool {
    match preset_id {
        "anthropic" => !key.starts_with("sk-ant-"),
        "openai" => !key.starts_with("sk-") || key.starts_with("sk-ant-"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_unique_and_supported() {
        let mut ids: Vec<_> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
        assert!(PRESETS.len() >= 26);
        assert!(
            PRESETS
                .iter()
                .all(|p| matches!(p.protocol, "anthropic" | "openai"))
        );
        assert!(PRESETS.iter().all(|p| !p.models.is_empty()));
    }

    #[test]
    fn stored_configs_find_their_preset_and_keys_are_cleaned() {
        assert_eq!(
            preset_for("openai", Some("https://api.deepseek.com/")).map(|p| p.id),
            Some("deepseek")
        );
        assert_eq!(
            preset_for("anthropic", None).map(|p| p.id),
            Some("anthropic")
        );
        assert_eq!(
            preset_for("openai", Some("https://my.proxy")).map(|p| p.id),
            None
        );
        assert_eq!(clean_key(" sk-a\u{200b}b\n"), ("sk-ab".to_owned(), true));
        assert_eq!(clean_key("sk-ab"), ("sk-ab".to_owned(), false));
        assert!(key_shape_warning("anthropic", "sk-123"));
        assert!(!key_shape_warning("anthropic", "sk-ant-1"));
        assert!(key_shape_warning("openai", "sk-ant-1"));
    }
}
