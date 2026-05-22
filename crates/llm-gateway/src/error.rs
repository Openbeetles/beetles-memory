#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayErrorKey {
    InvalidConfig,
    ScopeResolutionFailed,
    RuntimeUnavailable,
}

#[derive(Debug, thiserror::Error)]
#[error("{key:?}: {message}")]
pub struct GatewayError {
    key: GatewayErrorKey,
    message: String,
}

impl GatewayError {
    pub fn new(key: GatewayErrorKey, message: impl Into<String>) -> Self {
        Self {
            key,
            message: message.into(),
        }
    }

    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::InvalidConfig, message)
    }

    pub fn scope_resolution_failed(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::ScopeResolutionFailed, message)
    }

    pub fn runtime_unavailable(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::RuntimeUnavailable, message)
    }

    pub const fn key(&self) -> GatewayErrorKey {
        self.key
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<bm_sdk::Error> for GatewayError {
    fn from(error: bm_sdk::Error) -> Self {
        Self::runtime_unavailable(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, GatewayError>;
