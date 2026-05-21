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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryAuthDecision {
    pub authenticated: bool,
    pub auth_kind: String,
    pub principal: String,
}

impl EntryAuthDecision {
    pub fn authenticated(auth_kind: impl Into<String>, principal: impl Into<String>) -> Self {
        Self {
            authenticated: true,
            auth_kind: auth_kind.into(),
            principal: principal.into(),
        }
    }

    pub fn unauthenticated(auth_kind: impl Into<String>) -> Self {
        Self {
            authenticated: false,
            auth_kind: auth_kind.into(),
            principal: String::new(),
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
