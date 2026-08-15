use bm_adapter::{AdapterAuthContext, AdapterOperation};
use sha2::{Digest, Sha256};
use std::fmt;
use std::net::SocketAddr;

use crate::EntryAcceptedTcpStream;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryOperationCapability {
    Write,
    FinalizeTurn,
    Recall,
    Project,
    Maintain,
    Inspect,
    Recover,
    Replay,
    LongTermList,
    LongTermDetail,
    LongTermMutate,
    LongTermPolicy,
    TranscriptAttrWrite,
    Capabilities,
    Subscribe,
    Close,
    ConsoleRead,
    ConsoleWrite,
    McpProtocol,
    LlmGatewayProtocol,
}

impl EntryOperationCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::FinalizeTurn => "finalize_turn",
            Self::Recall => "recall",
            Self::Project => "project",
            Self::Maintain => "maintain",
            Self::Inspect => "inspect",
            Self::Recover => "recover",
            Self::Replay => "replay",
            Self::LongTermList => "long_term_list",
            Self::LongTermDetail => "long_term_detail",
            Self::LongTermMutate => "long_term_mutate",
            Self::LongTermPolicy => "long_term_policy",
            Self::TranscriptAttrWrite => "transcript_attr_write",
            Self::Capabilities => "capabilities",
            Self::Subscribe => "subscribe",
            Self::Close => "close",
            Self::ConsoleRead => "console_read",
            Self::ConsoleWrite => "console_write",
            Self::McpProtocol => "mcp_protocol",
            Self::LlmGatewayProtocol => "llm_gateway_protocol",
        }
    }

    pub const fn for_adapter_operation(operation: AdapterOperation) -> Self {
        match operation {
            AdapterOperation::Write => Self::Write,
            AdapterOperation::FinalizeTurn => Self::FinalizeTurn,
            AdapterOperation::Recall => Self::Recall,
            AdapterOperation::Project => Self::Project,
            AdapterOperation::Maintain => Self::Maintain,
            AdapterOperation::Inspect => Self::Inspect,
            AdapterOperation::Recover => Self::Recover,
            AdapterOperation::Replay => Self::Replay,
            AdapterOperation::LongTermList => Self::LongTermList,
            AdapterOperation::LongTermDetail => Self::LongTermDetail,
            AdapterOperation::LongTermMutate => Self::LongTermMutate,
            AdapterOperation::LongTermPolicy => Self::LongTermPolicy,
            AdapterOperation::TranscriptAttrWrite => Self::TranscriptAttrWrite,
            AdapterOperation::Capabilities => Self::Capabilities,
            AdapterOperation::Subscribe => Self::Subscribe,
            AdapterOperation::Close => Self::Close,
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::Write,
            Self::FinalizeTurn,
            Self::Recall,
            Self::Project,
            Self::Maintain,
            Self::Inspect,
            Self::Recover,
            Self::Replay,
            Self::LongTermList,
            Self::LongTermDetail,
            Self::LongTermMutate,
            Self::LongTermPolicy,
            Self::TranscriptAttrWrite,
            Self::Capabilities,
            Self::Subscribe,
            Self::Close,
            Self::ConsoleRead,
            Self::ConsoleWrite,
            Self::McpProtocol,
            Self::LlmGatewayProtocol,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryBearerPrincipal {
    principal_id: String,
    owner_id: String,
    capabilities: Vec<EntryOperationCapability>,
}

impl EntryBearerPrincipal {
    pub fn new(
        principal_id: impl Into<String>,
        owner_id: impl Into<String>,
        capabilities: impl IntoIterator<Item = EntryOperationCapability>,
    ) -> Self {
        let mut capabilities = capabilities.into_iter().collect::<Vec<_>>();
        capabilities.sort_by_key(|capability| *capability as u8);
        capabilities.dedup();
        Self {
            principal_id: principal_id.into(),
            owner_id: owner_id.into(),
            capabilities,
        }
    }

    pub fn principal_id(&self) -> &str {
        self.principal_id.trim()
    }

    pub fn owner_id(&self) -> &str {
        self.owner_id.trim()
    }

    pub fn capabilities(&self) -> &[EntryOperationCapability] {
        &self.capabilities
    }

    pub fn allows(&self, capability: EntryOperationCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    fn is_valid(&self) -> bool {
        !self.principal_id().is_empty()
            && !self.owner_id().is_empty()
            && !self.capabilities.is_empty()
    }
}

#[derive(Clone, PartialEq, Eq)]
struct RedactedBearerSecret(String);

impl fmt::Debug for RedactedBearerSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EntryBearerCredential {
    token: RedactedBearerSecret,
    principal: EntryBearerPrincipal,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EntryAuthConfig {
    require_auth: bool,
    credential: Option<EntryBearerCredential>,
}

impl fmt::Debug for EntryAuthConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntryAuthConfig")
            .field("require_auth", &self.require_auth)
            .field("credential", &self.credential)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryLocalTransport {
    InProcess,
    Stdio,
}

impl EntryLocalTransport {
    const fn auth_kind(self) -> &'static str {
        match self {
            Self::InProcess => "local_in_process",
            Self::Stdio => "local_stdio",
        }
    }
}

impl EntryAuthConfig {
    pub fn disabled_for_local() -> Self {
        Self {
            require_auth: false,
            credential: None,
        }
    }

    pub fn required_bearer_principal(
        token: impl Into<String>,
        principal: EntryBearerPrincipal,
    ) -> Self {
        Self {
            require_auth: true,
            credential: Some(EntryBearerCredential {
                token: RedactedBearerSecret(token.into()),
                principal,
            }),
        }
    }

    pub fn has_bearer_verifier(&self) -> bool {
        self.credential.as_ref().is_some_and(|credential| {
            !credential.token.0.is_empty() && credential.principal.is_valid()
        })
    }

    pub const fn requires_auth(&self) -> bool {
        self.require_auth
    }

    pub fn authenticate_accepted_tcp_stream(
        &self,
        accepted: &EntryAcceptedTcpStream,
        authorization: Option<&str>,
        loopback_principal: &str,
    ) -> EntryAuthDecision {
        self.authenticate_network_peer(accepted.peer_addr(), authorization, loopback_principal)
    }

    pub(crate) fn authenticate_network_peer(
        &self,
        peer: SocketAddr,
        authorization: Option<&str>,
        loopback_principal: &str,
    ) -> EntryAuthDecision {
        if self.require_auth {
            return self.verify_bearer(authorization);
        }
        if !peer.ip().is_loopback() {
            return EntryAuthDecision::rejected(
                "network_peer",
                "auth_disabled_requires_loopback_peer",
            );
        }
        EntryAuthDecision::verified_loopback(loopback_principal)
    }

    pub fn authenticate_local_transport(
        &self,
        transport: EntryLocalTransport,
        principal: &str,
    ) -> EntryAuthDecision {
        EntryAuthDecision::verified_local_transport(transport, principal)
    }

    pub fn verify_bearer(&self, authorization: Option<&str>) -> EntryAuthDecision {
        let Some(credential) = self.credential.as_ref() else {
            return EntryAuthDecision::rejected_bearer("bearer_verifier_not_configured");
        };
        if !credential.principal.is_valid() || credential.token.0.is_empty() {
            return EntryAuthDecision::rejected_bearer("invalid_bearer_verifier_config");
        }
        let Some(actual_token) = authorization.and_then(parse_bearer_token) else {
            return EntryAuthDecision::rejected_bearer("missing_bearer_token");
        };
        if !constant_time_eq(actual_token.as_bytes(), credential.token.0.as_bytes()) {
            return EntryAuthDecision::rejected_bearer("token_mismatch");
        }
        EntryAuthDecision::verified_bearer(
            credential.principal.clone(),
            token_fingerprint(actual_token),
        )
    }

    pub(crate) fn verify_bearer_for_owner(
        &self,
        authorization: Option<&str>,
        expected_owner_id: &str,
    ) -> EntryAuthDecision {
        let decision = self.verify_bearer(authorization);
        if decision.is_authenticated()
            && decision
                .bearer_principal()
                .is_none_or(|principal| principal.owner_id() != expected_owner_id.trim())
        {
            return EntryAuthDecision::rejected_bearer("bearer_owner_binding_mismatch");
        }
        decision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryAuthDecision {
    authenticated: bool,
    auth_kind: String,
    principal: String,
    token_fingerprint: Option<String>,
    principal_kind: String,
    permissions: Vec<String>,
    local_loopback: bool,
    rejection_reason: Option<String>,
    bearer_principal: Option<EntryBearerPrincipal>,
    trusted_capabilities: Vec<EntryOperationCapability>,
}

impl EntryAuthDecision {
    fn verified_local_transport(transport: EntryLocalTransport, principal: &str) -> Self {
        Self {
            authenticated: true,
            auth_kind: transport.auth_kind().to_string(),
            principal: principal.to_string(),
            token_fingerprint: None,
            principal_kind: "trusted_local_transport".to_string(),
            permissions: Vec::new(),
            local_loopback: false,
            rejection_reason: None,
            bearer_principal: None,
            trusted_capabilities: EntryOperationCapability::all().to_vec(),
        }
    }

    fn rejected(auth_kind: &str, reason: &str) -> Self {
        Self {
            authenticated: false,
            auth_kind: auth_kind.to_string(),
            principal: String::new(),
            token_fingerprint: None,
            principal_kind: "unknown".to_string(),
            permissions: Vec::new(),
            local_loopback: false,
            rejection_reason: Some(reason.to_string()),
            bearer_principal: None,
            trusted_capabilities: Vec::new(),
        }
    }

    fn verified_loopback(principal: &str) -> Self {
        Self {
            authenticated: true,
            auth_kind: "loopback".to_string(),
            principal: principal.to_string(),
            token_fingerprint: None,
            principal_kind: "local_profile".to_string(),
            permissions: Vec::new(),
            local_loopback: true,
            rejection_reason: None,
            bearer_principal: None,
            trusted_capabilities: EntryOperationCapability::all().to_vec(),
        }
    }

    fn verified_bearer(principal: EntryBearerPrincipal, token_fingerprint: String) -> Self {
        let principal_id = principal.principal_id().to_string();
        Self {
            authenticated: true,
            auth_kind: "bearer_token".to_string(),
            principal: principal_id.clone(),
            token_fingerprint: Some(token_fingerprint),
            principal_kind: "configured_bearer_principal".to_string(),
            permissions: principal
                .capabilities()
                .iter()
                .map(|capability| capability.as_str().to_string())
                .collect(),
            local_loopback: false,
            rejection_reason: None,
            bearer_principal: Some(principal),
            trusted_capabilities: Vec::new(),
        }
    }

    fn rejected_bearer(reason: &str) -> Self {
        let mut decision = Self::rejected("bearer_token", reason);
        decision.principal_kind = "configured_bearer_principal".to_string();
        decision
    }

    pub const fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    pub fn auth_kind(&self) -> &str {
        &self.auth_kind
    }

    pub fn token_fingerprint(&self) -> Option<&str> {
        self.token_fingerprint.as_deref()
    }

    pub fn principal_kind(&self) -> &str {
        &self.principal_kind
    }

    pub fn permissions(&self) -> &[String] {
        &self.permissions
    }

    pub const fn is_loopback(&self) -> bool {
        self.local_loopback
    }

    pub fn rejection_reason(&self) -> Option<&str> {
        self.rejection_reason.as_deref()
    }

    pub fn principal_id(&self) -> &str {
        self.principal.trim()
    }

    pub fn bearer_principal(&self) -> Option<&EntryBearerPrincipal> {
        self.bearer_principal.as_ref()
    }

    pub fn allows(&self, capability: EntryOperationCapability) -> bool {
        self.bearer_principal.as_ref().map_or_else(
            || self.trusted_capabilities.contains(&capability),
            |principal| principal.allows(capability),
        )
    }

    pub(crate) fn into_adapter(self) -> AdapterAuthContext {
        AdapterAuthContext {
            authenticated: self.authenticated,
            auth_kind: self.auth_kind,
            principal: self.principal,
        }
    }
}

fn parse_bearer_token(header: &str) -> Option<&str> {
    let mut fields = header.split_ascii_whitespace();
    let scheme = fields.next()?;
    let token = fields.next()?;
    if fields.next().is_some() || !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return None;
    }
    Some(token)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let width = left.len().max(right.len());
    for index in 0..width {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn token_fingerprint(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"beetle-memory/bearer-token-fingerprint/v1\0");
    hasher.update((token.len() as u64).to_be_bytes());
    hasher.update(token.as_bytes());
    format!("tok_sha256_{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_disabled_rejects_non_loopback_raw_peer() {
        let decision = EntryAuthConfig::disabled_for_local().authenticate_network_peer(
            "192.0.2.10:443".parse().expect("test peer"),
            None,
            "forged-loopback",
        );

        assert!(!decision.is_authenticated());
        assert_eq!(
            decision.rejection_reason(),
            Some("auth_disabled_requires_loopback_peer")
        );
    }
}
