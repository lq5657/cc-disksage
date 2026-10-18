use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::LlmError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LlmConfig {
    pub provider_id: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub timeout_secs: u64,
    pub redact_paths: bool,
    pub send_sample_names: bool,
    pub sample_name_limit: usize,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            model: "deepseek-chat".into(),
            temperature: 0.2,
            max_tokens: 1024,
            timeout_secs: 60,
            redact_paths: true,
            send_sample_names: false,
            sample_name_limit: 0,
        }
    }
}

impl LlmConfig {
    pub fn validate(&self) -> Result<(), LlmError> {
        if self.provider_id.is_empty()
            || !self.provider_id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        {
            return Err(LlmError::Config("provider_id must use [a-z0-9._-]".into()));
        }
        if self.model.trim().is_empty() {
            return Err(LlmError::Config("model must not be empty".into()));
        }

        let url = reqwest::Url::parse(self.base_url.trim())
            .map_err(|error| LlmError::Config(format!("invalid base_url: {error}")))?;
        match url.scheme() {
            "https" => {}
            "http" => {
                let host = url.host_str().unwrap_or_default();
                let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
                if !loopback {
                    return Err(LlmError::Config(
                        "plain HTTP is only allowed for loopback addresses".into(),
                    ));
                }
            }
            other => {
                return Err(LlmError::Config(format!("unsupported URL scheme: {other}")));
            }
        }

        if !self.temperature.is_finite() || !(0.0..=2.0).contains(&self.temperature) {
            return Err(LlmError::Config(
                "temperature must be between 0 and 2".into(),
            ));
        }
        if !(64..=8192).contains(&self.max_tokens) {
            return Err(LlmError::Config(
                "max_tokens must be between 64 and 8192".into(),
            ));
        }
        if !(5..=300).contains(&self.timeout_secs) {
            return Err(LlmError::Config(
                "timeout_secs must be between 5 and 300".into(),
            ));
        }
        if self.sample_name_limit > 10 {
            return Err(LlmError::Config("sample_name_limit must be <= 10".into()));
        }

        Ok(())
    }

    pub fn endpoint(&self) -> String {
        let base = self.base_url.trim().trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }

    pub fn load(path: &Path) -> Result<Self, LlmError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&text)?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<(), LlmError> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        fs::write(path, text)?;
        Ok(())
    }
}

pub fn default_config_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("cc-disksage")
        .join("llm.json")
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LlmPreset {
    pub id: &'static str,
    pub name_zh: &'static str,
    pub base_url: &'static str,
    pub model: &'static str,
}

pub const PRESETS: &[LlmPreset] = &[
    LlmPreset {
        id: "deepseek",
        name_zh: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        model: "deepseek-chat",
    },
    LlmPreset {
        id: "qwen",
        name_zh: "通义千问",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        model: "qwen-plus",
    },
    LlmPreset {
        id: "zhipu",
        name_zh: "智谱 GLM",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        model: "glm-4-flash",
    },
    LlmPreset {
        id: "moonshot",
        name_zh: "Moonshot Kimi",
        base_url: "https://api.moonshot.cn/v1",
        model: "moonshot-v1-8k",
    },
    LlmPreset {
        id: "openai",
        name_zh: "OpenAI",
        base_url: "https://api.openai.com/v1",
        model: "gpt-4.1-mini",
    },
    LlmPreset {
        id: "ollama",
        name_zh: "Ollama（本地，离线）",
        base_url: "http://127.0.0.1:11434/v1",
        model: "qwen3:8b",
    },
];

pub fn preset(id: &str) -> Option<LlmPreset> {
    PRESETS.iter().copied().find(|preset| preset.id == id)
}

pub fn config_for_preset(id: &str) -> Option<LlmConfig> {
    preset(id).map(|preset| LlmConfig {
        provider_id: preset.id.to_string(),
        base_url: preset.base_url.to_string(),
        model: preset.model.to_string(),
        ..LlmConfig::default()
    })
}
