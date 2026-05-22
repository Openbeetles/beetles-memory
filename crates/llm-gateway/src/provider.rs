use serde::{Deserialize, Serialize};

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
}

impl GatewayProviderConfig {
    pub fn openai_compatible(base_url: impl Into<String>, api_key_env: Option<&str>) -> Self {
        Self {
            kind: GatewayProviderKind::OpenAiCompatible,
            base_url: base_url.into(),
            api_key_env: api_key_env.map(str::to_string),
            model_aliases: Vec::new(),
            timeout_ms: None,
        }
    }

    pub fn ollama_native(base_url: impl Into<String>) -> Self {
        Self {
            kind: GatewayProviderKind::OllamaNative,
            base_url: base_url.into(),
            api_key_env: None,
            model_aliases: Vec::new(),
            timeout_ms: None,
        }
    }

    pub fn secret_env_name(&self) -> Option<&str> {
        self.api_key_env.as_deref()
    }

    pub const fn protocol_endpoint_for_cut_b(&self) -> Option<&'static str> {
        None
    }
}
