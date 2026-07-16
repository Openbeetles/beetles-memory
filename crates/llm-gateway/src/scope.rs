use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_entry::{
    EntryAuthDecision, EntryIdentity, EntryOperationCapability, EntryRuntimeScope, EntryScope,
};

use crate::{GatewayError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayScopeRequest {
    pub auth: EntryAuthDecision,
    pub headers: BTreeMap<String, String>,
    pub workspace_root_digest: Option<String>,
    pub workspace_root_path: Option<String>,
    pub client_conversation_hint: Option<String>,
    pub request_id_hint: Option<String>,
    pub body_conversation_hint: Option<String>,
    pub model_alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayScopeResolverConfig {
    pub local_owner_id: Option<String>,
    pub first_run_owner_id: Option<String>,
    pub default_agent_id: String,
    pub default_channel: String,
    pub default_chat_id: Option<String>,
    pub trusted_headers: GatewayTrustedHeaders,
    pub ollama_app: GatewayOllamaAppScopeConfig,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GatewayTrustedHeaders {
    pub agent_id: Option<String>,
    pub channel: Option<String>,
    pub chat_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayOllamaAppScopeConfig {
    pub enabled: bool,
    pub local_app_identity: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayScopeResolution {
    pub owner_id: String,
    pub agent_id: String,
    pub channel: String,
    pub chat_id: String,
    pub audit_principal_id: String,
    pub capabilities: Vec<String>,
    pub audit_safe_summary: String,
    #[serde(skip)]
    pub entry_scope: EntryRuntimeScope,
}

pub struct GatewayScopeResolver {
    config: GatewayScopeResolverConfig,
}

impl GatewayTrustedHeaders {
    pub fn none() -> Self {
        Self::default()
    }
}

impl GatewayOllamaAppScopeConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            local_app_identity: String::new(),
        }
    }
}

impl GatewayScopeResolverConfig {
    pub fn default_for_local_dev() -> Self {
        Self {
            local_owner_id: Some("owner-default".to_string()),
            first_run_owner_id: Some("owner-first-run".to_string()),
            default_agent_id: "agent-main".to_string(),
            default_channel: "llm.gateway".to_string(),
            default_chat_id: None,
            trusted_headers: GatewayTrustedHeaders::none(),
            ollama_app: GatewayOllamaAppScopeConfig::disabled(),
        }
    }
}

impl GatewayScopeResolver {
    pub fn new(config: GatewayScopeResolverConfig) -> Self {
        Self { config }
    }

    pub fn resolve(&self, request: &GatewayScopeRequest) -> Result<GatewayScopeResolution> {
        let principal = GatewayPrincipalContext::from_request(request)?;
        let owner_id = principal
            .bearer_owner_id()
            .or_else(|| {
                first_non_empty([
                    self.config.local_owner_id.as_deref(),
                    self.config.first_run_owner_id.as_deref(),
                ])
            })
            .ok_or_else(|| GatewayError::scope_resolution_failed("owner_id is unavailable"))?;
        let agent_id = first_non_empty([
            trusted_header(
                &request.headers,
                self.config.trusted_headers.agent_id.as_deref(),
            ),
            Some(self.config.default_agent_id.as_str()),
        ])
        .ok_or_else(|| GatewayError::scope_resolution_failed("agent_id is unavailable"))?;
        let channel = first_non_empty([
            trusted_header(
                &request.headers,
                self.config.trusted_headers.channel.as_deref(),
            ),
            Some(self.config.default_channel.as_str()),
        ])
        .ok_or_else(|| GatewayError::scope_resolution_failed("channel is unavailable"))?;
        let chat_id = first_non_empty([
            trusted_header(
                &request.headers,
                self.config.trusted_headers.chat_id.as_deref(),
            ),
            self.config.default_chat_id.as_deref(),
        ])
        .map(str::to_string)
        .map(|chat_id| ChatIdResolution {
            chat_id,
            ollama_app_hint_source: None,
        })
        .unwrap_or_else(|| self.stable_chat_id_resolution(owner_id, agent_id, channel, request));

        let mut resolution = GatewayScopeResolution::from_parts(
            owner_id,
            agent_id,
            channel,
            &chat_id.chat_id,
            &principal,
            request.workspace_root_digest.as_deref(),
            request.model_alias.as_deref(),
        );
        if let Some(source) = chat_id.ollama_app_hint_source {
            resolution
                .audit_safe_summary
                .push_str(&format!(" ollama_app_hint_source={source}"));
        }
        Ok(resolution)
    }

    fn stable_chat_id_resolution(
        &self,
        owner_id: &str,
        agent_id: &str,
        channel: &str,
        request: &GatewayScopeRequest,
    ) -> ChatIdResolution {
        if self.config.ollama_app.enabled {
            let hint = ollama_app_conversation_hint(request, &self.config.ollama_app);
            return ChatIdResolution {
                chat_id: stable_chat_id(owner_id, agent_id, channel, request, Some(&hint.value)),
                ollama_app_hint_source: Some(hint.source),
            };
        }
        let hint = first_non_empty([
            request.client_conversation_hint.as_deref(),
            request.request_id_hint.as_deref(),
        ]);
        ChatIdResolution {
            chat_id: stable_chat_id(owner_id, agent_id, channel, request, hint),
            ollama_app_hint_source: None,
        }
    }
}

impl GatewayScopeRequest {
    pub fn new(auth: EntryAuthDecision) -> Self {
        Self {
            auth,
            headers: BTreeMap::new(),
            workspace_root_digest: None,
            workspace_root_path: None,
            client_conversation_hint: None,
            request_id_hint: None,
            body_conversation_hint: None,
            model_alias: None,
        }
    }

    pub fn require_capabilities(&self, required: &[EntryOperationCapability]) -> Result<()> {
        let auth = &self.auth;
        if !auth.is_authenticated() {
            return Err(GatewayError::forbidden(
                "gateway principal is unauthenticated",
            ));
        }
        if let Some(missing) = required
            .iter()
            .copied()
            .find(|capability| !auth.allows(*capability))
        {
            return Err(GatewayError::forbidden(format!(
                "gateway principal lacks required capability: {}",
                missing.as_str()
            )));
        }
        Ok(())
    }
}

struct GatewayPrincipalContext {
    audit_principal_id: String,
    bearer_owner_id: Option<String>,
    capabilities: Vec<String>,
}

impl GatewayPrincipalContext {
    fn from_request(request: &GatewayScopeRequest) -> Result<Self> {
        let auth = &request.auth;
        if !auth.is_authenticated() {
            return Err(GatewayError::scope_resolution_failed(
                "gateway principal is unauthenticated",
            ));
        }
        let bearer = auth.bearer_principal();
        Ok(Self {
            audit_principal_id: auth.principal_id().to_string(),
            bearer_owner_id: bearer.map(|principal| principal.owner_id().to_string()),
            capabilities: bearer.map_or_else(
                || {
                    EntryOperationCapability::all()
                        .iter()
                        .map(|capability| capability.as_str().to_string())
                        .collect()
                },
                |principal| {
                    principal
                        .capabilities()
                        .iter()
                        .map(|capability| capability.as_str().to_string())
                        .collect()
                },
            ),
        })
    }

    fn bearer_owner_id(&self) -> Option<&str> {
        self.bearer_owner_id.as_deref()
    }
}

struct ChatIdResolution {
    chat_id: String,
    ollama_app_hint_source: Option<&'static str>,
}

struct OllamaAppConversationHint {
    value: String,
    source: &'static str,
}

impl GatewayScopeResolution {
    pub fn audit_only(
        owner_id: &str,
        agent_id: &str,
        channel: &str,
        chat_id: &str,
        audit_safe_summary: &str,
    ) -> Self {
        Self {
            owner_id: owner_id.to_string(),
            agent_id: agent_id.to_string(),
            channel: channel.to_string(),
            chat_id: chat_id.to_string(),
            audit_principal_id: "audit-only".to_string(),
            capabilities: Vec::new(),
            audit_safe_summary: audit_safe_summary.to_string(),
            entry_scope: EntryRuntimeScope {
                identity: EntryIdentity {
                    agent_id: agent_id.to_string(),
                    owner_id: owner_id.to_string(),
                },
                scope: EntryScope {
                    channel: channel.to_string(),
                    chat_id: chat_id.to_string(),
                },
            },
        }
    }

    fn from_parts(
        owner_id: &str,
        agent_id: &str,
        channel: &str,
        chat_id: &str,
        principal: &GatewayPrincipalContext,
        workspace_digest: Option<&str>,
        model_alias: Option<&str>,
    ) -> Self {
        let audit_safe_summary = format!(
            "owner_id={owner_id} agent_id={agent_id} channel={channel} chat_id={chat_id} audit_principal_id={} capabilities={} workspace_digest={} model_alias={}",
            principal.audit_principal_id,
            principal.capabilities.join(","),
            workspace_digest.unwrap_or("none"),
            model_alias.unwrap_or("none")
        );
        let mut resolution =
            Self::audit_only(owner_id, agent_id, channel, chat_id, &audit_safe_summary);
        resolution.audit_principal_id = principal.audit_principal_id.clone();
        resolution.capabilities = principal.capabilities.clone();
        resolution
    }
}

fn trusted_header<'a>(
    headers: &'a BTreeMap<String, String>,
    header_name: Option<&str>,
) -> Option<&'a str> {
    let header_name = header_name?;
    headers
        .get(header_name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn stable_chat_id(
    owner_id: &str,
    agent_id: &str,
    channel: &str,
    request: &GatewayScopeRequest,
    conversation_hint: Option<&str>,
) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    hash_field(&mut hash, owner_id);
    hash_field(&mut hash, agent_id);
    hash_field(&mut hash, channel);
    hash_field(
        &mut hash,
        request.workspace_root_digest.as_deref().unwrap_or(""),
    );
    hash_field(&mut hash, conversation_hint.unwrap_or(""));
    hash_field(&mut hash, request.model_alias.as_deref().unwrap_or(""));
    format!("chat_{hash:016x}")
}

fn ollama_app_conversation_hint(
    request: &GatewayScopeRequest,
    config: &GatewayOllamaAppScopeConfig,
) -> OllamaAppConversationHint {
    if let Some(value) = first_non_empty([request.client_conversation_hint.as_deref()]) {
        return OllamaAppConversationHint {
            value: format!("explicit:{value}"),
            source: "explicit_conversation_id",
        };
    }
    if let Some(value) = referer_chat_path(&request.headers) {
        return OllamaAppConversationHint {
            value: format!("referer:{value}"),
            source: "referer_chat_path",
        };
    }
    if let Some(value) = first_non_empty([request.body_conversation_hint.as_deref()]) {
        return OllamaAppConversationHint {
            value: format!("body:{value}"),
            source: "body_conversation_hint",
        };
    }
    OllamaAppConversationHint {
        value: format!(
            "local-app:{}:day-{}",
            non_empty_or_default(&config.local_app_identity, "ollama-app"),
            current_unix_day()
        ),
        source: "local_app_daily_bucket",
    }
}

fn referer_chat_path(headers: &BTreeMap<String, String>) -> Option<String> {
    let referer = headers
        .get("referer")
        .or_else(|| headers.get("referrer"))?
        .trim();
    let (_, after_marker) = referer.split_once("/c/")?;
    let chat_id = after_marker
        .split(|ch: char| matches!(ch, '/' | '?' | '#' | '&') || ch.is_whitespace())
        .next()
        .unwrap_or_default()
        .trim();
    let sanitized = chat_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        .collect::<String>();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

fn current_unix_day() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() / 86_400)
        .unwrap_or(0)
}

fn non_empty_or_default<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    }
}

fn hash_field(hash: &mut u64, value: &str) {
    const FNV_PRIME: u64 = 0x100000001b3;
    for byte in value.as_bytes().iter().copied().chain([0]) {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}
