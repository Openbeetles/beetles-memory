use bm_adapter::AdapterAuthContext;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryAuthConfig {
    pub require_auth: bool,
    pub token: Option<String>,
}

impl EntryAuthConfig {
    pub fn disabled_for_local() -> Self {
        Self {
            require_auth: false,
            token: None,
        }
    }

    pub fn required_bearer_token(token: impl Into<String>) -> Self {
        Self {
            require_auth: true,
            token: Some(token.into()),
        }
    }

    pub fn token_fingerprint(&self) -> Option<String> {
        self.token.as_deref().map(token_fingerprint)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryAuthDecision {
    pub authenticated: bool,
    pub auth_kind: String,
    pub principal: String,
    pub auth_subject: Option<String>,
    pub token_fingerprint: Option<String>,
    pub principal_kind: String,
    pub permissions: Vec<String>,
    pub local_loopback: bool,
    pub rejection_reason: Option<String>,
}

impl EntryAuthDecision {
    pub fn authenticated(auth_kind: impl Into<String>, principal: impl Into<String>) -> Self {
        Self {
            authenticated: true,
            auth_kind: auth_kind.into(),
            principal: principal.into(),
            auth_subject: None,
            token_fingerprint: None,
            principal_kind: "unknown".to_string(),
            permissions: Vec::new(),
            local_loopback: false,
            rejection_reason: None,
        }
    }

    pub fn unauthenticated(auth_kind: impl Into<String>) -> Self {
        Self {
            authenticated: false,
            auth_kind: auth_kind.into(),
            principal: String::new(),
            auth_subject: None,
            token_fingerprint: None,
            principal_kind: "unknown".to_string(),
            permissions: Vec::new(),
            local_loopback: false,
            rejection_reason: Some("unauthenticated".to_string()),
        }
    }

    pub fn remote_bearer(
        config: &EntryAuthConfig,
        authorization: Option<&str>,
        auth_subject: Option<&str>,
    ) -> Self {
        let Some(expected_token) = config.token.as_deref() else {
            return Self::rejected_bearer(auth_subject, "token_not_configured");
        };
        let Some(actual_token) = authorization.and_then(parse_bearer_token) else {
            return Self::rejected_bearer(auth_subject, "missing_bearer_token");
        };
        if actual_token != expected_token {
            return Self::rejected_bearer(auth_subject, "token_mismatch");
        }
        let principal = auth_subject
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("remote-token-subject");
        Self {
            authenticated: true,
            auth_kind: "bearer_token".to_string(),
            principal: principal.to_string(),
            auth_subject: Some(principal.to_string()),
            token_fingerprint: Some(token_fingerprint(actual_token)),
            principal_kind: "operator_or_device".to_string(),
            permissions: vec!["memory:read".to_string(), "memory:write".to_string()],
            local_loopback: false,
            rejection_reason: None,
        }
    }

    pub fn loopback(principal: impl Into<String>) -> Self {
        Self {
            authenticated: true,
            auth_kind: "loopback".to_string(),
            principal: principal.into(),
            auth_subject: None,
            token_fingerprint: None,
            principal_kind: "local_profile".to_string(),
            permissions: vec!["memory:local".to_string()],
            local_loopback: true,
            rejection_reason: None,
        }
    }

    fn rejected_bearer(auth_subject: Option<&str>, reason: &str) -> Self {
        Self {
            authenticated: false,
            auth_kind: "bearer_token".to_string(),
            principal: auth_subject.unwrap_or("").trim().to_string(),
            auth_subject: auth_subject
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            token_fingerprint: None,
            principal_kind: "operator_or_device".to_string(),
            permissions: Vec::new(),
            local_loopback: false,
            rejection_reason: Some(reason.to_string()),
        }
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
    let (scheme, token) = header.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

fn token_fingerprint(token: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in token.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("tok_{hash:016x}")
}
