use serde::{Deserialize, Serialize};

use crate::{GatewayConfig, GatewayError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayProviderKind {
    OpenAiCompatible,
    OllamaNative,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayProviderConfig {
    pub kind: GatewayProviderKind,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_aliases: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub ollama_generate_system_supported: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub openai_responses_supported: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub openai_stateful_responses_supported: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub openai_embeddings_supported: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub openai_tools_supported: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub openai_streaming_supported: bool,
}

impl GatewayProviderConfig {
    pub fn openai_compatible(base_url: impl Into<String>, api_key_env: Option<&str>) -> Self {
        Self {
            kind: GatewayProviderKind::OpenAiCompatible,
            base_url: base_url.into(),
            api_key_env: api_key_env.map(str::to_string),
            model_aliases: Vec::new(),
            timeout_ms: None,
            ollama_generate_system_supported: true,
            openai_responses_supported: true,
            openai_stateful_responses_supported: false,
            openai_embeddings_supported: true,
            openai_tools_supported: true,
            openai_streaming_supported: true,
        }
    }

    pub fn ollama_native(base_url: impl Into<String>) -> Self {
        Self {
            kind: GatewayProviderKind::OllamaNative,
            base_url: base_url.into(),
            api_key_env: None,
            model_aliases: Vec::new(),
            timeout_ms: None,
            ollama_generate_system_supported: true,
            openai_responses_supported: false,
            openai_stateful_responses_supported: false,
            openai_embeddings_supported: false,
            openai_tools_supported: false,
            openai_streaming_supported: true,
        }
    }

    pub fn secret_env_name(&self) -> Option<&str> {
        self.api_key_env.as_deref()
    }
}

pub(crate) fn select_provider_for_kind<'a>(
    config: &'a GatewayConfig,
    provider_name: Option<&str>,
    kind: GatewayProviderKind,
    protocol: &str,
) -> Result<&'a GatewayProviderConfig> {
    if let Some(provider_name) = provider_name {
        let provider = config
            .providers
            .get(provider_name)
            .ok_or_else(|| GatewayError::provider_unavailable("provider is not configured"))?;
        if provider.kind != kind {
            return Err(GatewayError::provider_unavailable(format!(
                "provider is not {protocol}"
            )));
        }
        return Ok(provider);
    }

    if let Some(provider) = config.providers.get(&config.default_provider) {
        if provider.kind == kind {
            return Ok(provider);
        }
    }

    config
        .providers
        .values()
        .find(|provider| provider.kind == kind)
        .ok_or_else(|| {
            GatewayError::provider_unavailable(format!("{protocol} provider is not configured"))
        })
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_false(value: &bool) -> bool {
    !*value
}
