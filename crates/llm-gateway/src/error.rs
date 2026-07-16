#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayErrorKey {
    InvalidConfig,
    InvalidRequest,
    Unauthorized,
    Forbidden,
    ProviderUnavailable,
    ScopeResolutionFailed,
    ProjectionFailed,
    UpstreamUnavailable,
    RuntimeUnavailable,
    CapacityExceeded,
}

impl GatewayErrorKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidConfig => "invalid_config",
            Self::InvalidRequest => "invalid_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ScopeResolutionFailed => "scope_resolution_failed",
            Self::ProjectionFailed => "projection_failed",
            Self::UpstreamUnavailable => "upstream_unavailable",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::CapacityExceeded => "capacity_exceeded",
        }
    }
}

impl std::fmt::Display for GatewayErrorKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{key}: {message}")]
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

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::InvalidRequest, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::Unauthorized, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::Forbidden, message)
    }

    pub fn provider_unavailable(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::ProviderUnavailable, message)
    }

    pub fn scope_resolution_failed(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::ScopeResolutionFailed, message)
    }

    pub fn projection_failed(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::ProjectionFailed, message)
    }

    pub fn upstream_unavailable(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::UpstreamUnavailable, message)
    }

    pub fn runtime_unavailable(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::RuntimeUnavailable, message)
    }

    pub fn capacity_exceeded(message: impl Into<String>) -> Self {
        Self::new(GatewayErrorKey::CapacityExceeded, message)
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

#[cfg(test)]
mod tests {
    use super::GatewayErrorKey;

    #[test]
    fn gateway_error_labels_are_stable_snake_case() {
        let cases = [
            (GatewayErrorKey::InvalidConfig, "invalid_config"),
            (GatewayErrorKey::InvalidRequest, "invalid_request"),
            (GatewayErrorKey::Unauthorized, "unauthorized"),
            (GatewayErrorKey::Forbidden, "forbidden"),
            (GatewayErrorKey::ProviderUnavailable, "provider_unavailable"),
            (
                GatewayErrorKey::ScopeResolutionFailed,
                "scope_resolution_failed",
            ),
            (GatewayErrorKey::ProjectionFailed, "projection_failed"),
            (GatewayErrorKey::UpstreamUnavailable, "upstream_unavailable"),
            (GatewayErrorKey::RuntimeUnavailable, "runtime_unavailable"),
            (GatewayErrorKey::CapacityExceeded, "capacity_exceeded"),
        ];
        for (key, label) in cases {
            assert_eq!(key.as_str(), label);
            assert_eq!(key.to_string(), label);
        }
    }
}
