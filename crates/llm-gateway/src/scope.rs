use std::collections::BTreeMap;

use bm_entry::{EntryIdentity, EntryRuntimeScope, EntryScope};

use crate::{GatewayError, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GatewayScopeRequest {
    pub auth_subject: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub workspace_root_digest: Option<String>,
    pub workspace_root_path: Option<String>,
    pub client_conversation_hint: Option<String>,
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
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GatewayTrustedHeaders {
    pub agent_id: Option<String>,
    pub channel: Option<String>,
    pub chat_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayScopeResolution {
    pub owner_id: String,
    pub agent_id: String,
    pub channel: String,
    pub chat_id: String,
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

impl GatewayScopeResolverConfig {
    pub fn default_for_local_dev() -> Self {
        Self {
            local_owner_id: Some("owner-default".to_string()),
            first_run_owner_id: Some("owner-first-run".to_string()),
            default_agent_id: "agent-main".to_string(),
            default_channel: "llm.gateway".to_string(),
            default_chat_id: None,
            trusted_headers: GatewayTrustedHeaders::none(),
        }
    }
}

impl GatewayScopeResolver {
    pub fn new(config: GatewayScopeResolverConfig) -> Self {
        Self { config }
    }

    pub fn resolve(&self, request: &GatewayScopeRequest) -> Result<GatewayScopeResolution> {
        let owner_id = first_non_empty([
            request.auth_subject.as_deref(),
            self.config.local_owner_id.as_deref(),
            self.config.first_run_owner_id.as_deref(),
        ])
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
        .unwrap_or_else(|| stable_chat_id(owner_id, agent_id, channel, request));

        Ok(GatewayScopeResolution::from_parts(
            owner_id,
            agent_id,
            channel,
            &chat_id,
            request.workspace_root_digest.as_deref(),
            request.model_alias.as_deref(),
        ))
    }
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
        workspace_digest: Option<&str>,
        model_alias: Option<&str>,
    ) -> Self {
        let audit_safe_summary = format!(
            "owner_id={owner_id} agent_id={agent_id} channel={channel} chat_id={chat_id} workspace_digest={} model_alias={}",
            workspace_digest.unwrap_or("none"),
            model_alias.unwrap_or("none")
        );
        Self::audit_only(owner_id, agent_id, channel, chat_id, &audit_safe_summary)
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
) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    hash_field(&mut hash, owner_id);
    hash_field(&mut hash, agent_id);
    hash_field(&mut hash, channel);
    hash_field(
        &mut hash,
        request.workspace_root_digest.as_deref().unwrap_or(""),
    );
    hash_field(
        &mut hash,
        request.client_conversation_hint.as_deref().unwrap_or(""),
    );
    hash_field(&mut hash, request.model_alias.as_deref().unwrap_or(""));
    format!("chat_{hash:016x}")
}

fn hash_field(hash: &mut u64, value: &str) {
    const FNV_PRIME: u64 = 0x100000001b3;
    for byte in value.as_bytes().iter().copied().chain([0]) {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}
