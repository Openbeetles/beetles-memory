use serde::Serialize;
use serde_json::Value;

use crate::{
    provider::select_provider_for_kind, GatewayConfig, GatewayProviderConfig, GatewayProviderKind,
    OpenAiCompatibleUpstream, OpenAiUpstreamResponse, Result,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GatewayProviderCapabilityReport {
    pub provider_name: String,
    pub provider_kind: GatewayProviderKind,
    pub chat_completions: bool,
    pub responses: bool,
    pub stateful_responses: bool,
    pub embeddings: bool,
    pub tools: bool,
    pub streaming: bool,
    pub models: Vec<GatewayModelCapabilityReport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GatewayModelCapabilityReport {
    pub model: String,
    pub chat_completions: bool,
    pub responses: bool,
    pub stateful_responses: bool,
    pub embeddings: bool,
    pub tools: bool,
    pub streaming: bool,
}

pub fn probe_openai_provider_capabilities(
    config: &GatewayConfig,
    provider_name: Option<&str>,
    upstream: &mut dyn OpenAiCompatibleUpstream,
) -> Result<GatewayProviderCapabilityReport> {
    let selected_name = selected_openai_provider_name(config, provider_name)?;
    let provider = select_provider_for_kind(
        config,
        Some(selected_name.as_str()),
        GatewayProviderKind::OpenAiCompatible,
        "openai-compatible",
    )?;
    let response = upstream.models(provider)?;
    let models = match response {
        OpenAiUpstreamResponse::Json { body, .. } => model_capabilities_from_body(provider, &body),
        OpenAiUpstreamResponse::Sse { .. } => Vec::new(),
    };
    Ok(GatewayProviderCapabilityReport {
        provider_name: selected_name,
        provider_kind: GatewayProviderKind::OpenAiCompatible,
        chat_completions: true,
        responses: provider.openai_responses_supported,
        stateful_responses: provider.openai_stateful_responses_supported,
        embeddings: provider.openai_embeddings_supported,
        tools: provider.openai_tools_supported,
        streaming: provider.openai_streaming_supported,
        models,
    })
}

fn selected_openai_provider_name(
    config: &GatewayConfig,
    provider_name: Option<&str>,
) -> Result<String> {
    if let Some(provider_name) = provider_name {
        select_provider_for_kind(
            config,
            Some(provider_name),
            GatewayProviderKind::OpenAiCompatible,
            "openai-compatible",
        )?;
        return Ok(provider_name.to_string());
    }
    if let Some(provider) = config.providers.get(&config.default_provider) {
        if provider.kind == GatewayProviderKind::OpenAiCompatible {
            return Ok(config.default_provider.clone());
        }
    }
    let Some((name, _)) = config
        .providers
        .iter()
        .find(|(_, provider)| provider.kind == GatewayProviderKind::OpenAiCompatible)
    else {
        return select_provider_for_kind(
            config,
            None,
            GatewayProviderKind::OpenAiCompatible,
            "openai-compatible",
        )
        .map(|_| String::new());
    };
    Ok(name.clone())
}

fn model_capabilities_from_body(
    provider: &GatewayProviderConfig,
    body: &Value,
) -> Vec<GatewayModelCapabilityReport> {
    body.get("data")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.get("id").and_then(Value::as_str))
                .map(|model| GatewayModelCapabilityReport {
                    model: model.to_string(),
                    chat_completions: true,
                    responses: provider.openai_responses_supported,
                    stateful_responses: provider.openai_stateful_responses_supported,
                    embeddings: provider.openai_embeddings_supported,
                    tools: provider.openai_tools_supported,
                    streaming: provider.openai_streaming_supported,
                })
                .collect()
        })
        .unwrap_or_default()
}
