use std::collections::BTreeMap;

use serde_json::Value;

use crate::ollama::OllamaGatewayMethod;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OllamaRouteAction {
    Intercept,
    Passthrough,
    Reject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OllamaKnownEndpoint {
    Chat,
    Generate,
    Version,
    Tags,
    Show,
    Pull,
    Push,
    Create,
    Copy,
    Delete,
    Embed,
    Embeddings,
    Ps,
    Me,
    ModelRecommendations,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OllamaRouteDecision {
    pub action: OllamaRouteAction,
    pub known_endpoint: Option<OllamaKnownEndpoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OllamaPassthroughRequest {
    pub method: OllamaGatewayMethod,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
}

impl OllamaPassthroughRequest {
    pub fn endpoint_suffix(&self) -> &str {
        self.path
            .strip_prefix("/api")
            .filter(|value| value.starts_with('/'))
            .unwrap_or(self.path.as_str())
    }
}

pub fn classify_ollama_route(method: OllamaGatewayMethod, path: &str) -> OllamaRouteDecision {
    let route = path.split_once('?').map(|(route, _)| route).unwrap_or(path);
    let known_endpoint = match (method, route) {
        (OllamaGatewayMethod::Post, "/api/chat") => Some(OllamaKnownEndpoint::Chat),
        (OllamaGatewayMethod::Post, "/api/generate") => Some(OllamaKnownEndpoint::Generate),
        (OllamaGatewayMethod::Get, "/api/version") => Some(OllamaKnownEndpoint::Version),
        (OllamaGatewayMethod::Get, "/api/tags") => Some(OllamaKnownEndpoint::Tags),
        (OllamaGatewayMethod::Post, "/api/show") => Some(OllamaKnownEndpoint::Show),
        (OllamaGatewayMethod::Post, "/api/pull") => Some(OllamaKnownEndpoint::Pull),
        (OllamaGatewayMethod::Post, "/api/push") => Some(OllamaKnownEndpoint::Push),
        (OllamaGatewayMethod::Post, "/api/create") => Some(OllamaKnownEndpoint::Create),
        (OllamaGatewayMethod::Post, "/api/copy") => Some(OllamaKnownEndpoint::Copy),
        (OllamaGatewayMethod::Delete, "/api/delete") => Some(OllamaKnownEndpoint::Delete),
        (OllamaGatewayMethod::Post, "/api/embed") => Some(OllamaKnownEndpoint::Embed),
        (OllamaGatewayMethod::Post, "/api/embeddings") => Some(OllamaKnownEndpoint::Embeddings),
        (OllamaGatewayMethod::Get, "/api/ps") => Some(OllamaKnownEndpoint::Ps),
        (OllamaGatewayMethod::Post, "/api/me") => Some(OllamaKnownEndpoint::Me),
        (OllamaGatewayMethod::Get, "/api/experimental/model-recommendations") => {
            Some(OllamaKnownEndpoint::ModelRecommendations)
        }
        _ => None,
    };

    let action = match known_endpoint {
        Some(OllamaKnownEndpoint::Chat | OllamaKnownEndpoint::Generate) => {
            OllamaRouteAction::Intercept
        }
        Some(_) => OllamaRouteAction::Passthrough,
        None if route.starts_with("/api/")
            && matches!(
                method,
                OllamaGatewayMethod::Get | OllamaGatewayMethod::Post | OllamaGatewayMethod::Delete
            ) =>
        {
            OllamaRouteAction::Passthrough
        }
        None => OllamaRouteAction::Reject,
    };

    OllamaRouteDecision {
        action,
        known_endpoint,
    }
}

pub fn ollama_passthrough_audit_id(endpoint: Option<OllamaKnownEndpoint>) -> &'static str {
    match endpoint {
        Some(OllamaKnownEndpoint::Version) => "ollama-version",
        Some(OllamaKnownEndpoint::Tags) => "ollama-tags",
        Some(OllamaKnownEndpoint::Show) => "ollama-show",
        Some(OllamaKnownEndpoint::Embed) => "ollama-embed",
        Some(OllamaKnownEndpoint::Embeddings) => "ollama-embeddings",
        Some(OllamaKnownEndpoint::Ps) => "ollama-ps",
        Some(OllamaKnownEndpoint::Me) => "ollama-me",
        Some(OllamaKnownEndpoint::ModelRecommendations) => "ollama-model-recommendations",
        Some(
            OllamaKnownEndpoint::Pull
            | OllamaKnownEndpoint::Push
            | OllamaKnownEndpoint::Create
            | OllamaKnownEndpoint::Copy
            | OllamaKnownEndpoint::Delete,
        ) => "ollama-model-management",
        _ => "ollama-passthrough",
    }
}

pub fn ollama_passthrough_prefers_stream(endpoint: Option<OllamaKnownEndpoint>) -> bool {
    matches!(
        endpoint,
        Some(OllamaKnownEndpoint::Pull | OllamaKnownEndpoint::Push | OllamaKnownEndpoint::Create)
    )
}
