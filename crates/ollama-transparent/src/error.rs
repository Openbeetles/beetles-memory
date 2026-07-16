#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OllamaTransparentErrorKey {
    InvalidConfig,
    PortInspectionFailed,
    RunnerInstallFailed,
    ProcessActionFailed,
    PreflightRejected,
    TransitionLeaseFailed,
    Unsupported,
}

impl OllamaTransparentErrorKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidConfig => "invalid_config",
            Self::PortInspectionFailed => "port_inspection_failed",
            Self::RunnerInstallFailed => "runner_install_failed",
            Self::ProcessActionFailed => "process_action_failed",
            Self::PreflightRejected => "preflight_rejected",
            Self::TransitionLeaseFailed => "transition_lease_failed",
            Self::Unsupported => "unsupported",
        }
    }
}

impl std::fmt::Display for OllamaTransparentErrorKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{key}: {message}")]
pub struct OllamaTransparentError {
    key: OllamaTransparentErrorKey,
    message: String,
}

impl OllamaTransparentError {
    pub fn new(key: OllamaTransparentErrorKey, message: impl Into<String>) -> Self {
        Self {
            key,
            message: message.into(),
        }
    }

    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(OllamaTransparentErrorKey::InvalidConfig, message)
    }

    pub fn port_inspection_failed(message: impl Into<String>) -> Self {
        Self::new(OllamaTransparentErrorKey::PortInspectionFailed, message)
    }

    pub fn runner_install_failed(message: impl Into<String>) -> Self {
        Self::new(OllamaTransparentErrorKey::RunnerInstallFailed, message)
    }

    pub fn process_action_failed(message: impl Into<String>) -> Self {
        Self::new(OllamaTransparentErrorKey::ProcessActionFailed, message)
    }

    pub fn preflight_rejected(message: impl Into<String>) -> Self {
        Self::new(OllamaTransparentErrorKey::PreflightRejected, message)
    }

    pub fn transition_lease_failed(message: impl Into<String>) -> Self {
        Self::new(OllamaTransparentErrorKey::TransitionLeaseFailed, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(OllamaTransparentErrorKey::Unsupported, message)
    }

    pub const fn key(&self) -> OllamaTransparentErrorKey {
        self.key
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub type Result<T> = std::result::Result<T, OllamaTransparentError>;

#[cfg(test)]
mod tests {
    use super::OllamaTransparentErrorKey;

    #[test]
    fn transparent_error_labels_are_stable_snake_case() {
        let cases = [
            (OllamaTransparentErrorKey::InvalidConfig, "invalid_config"),
            (
                OllamaTransparentErrorKey::PortInspectionFailed,
                "port_inspection_failed",
            ),
            (
                OllamaTransparentErrorKey::RunnerInstallFailed,
                "runner_install_failed",
            ),
            (
                OllamaTransparentErrorKey::ProcessActionFailed,
                "process_action_failed",
            ),
            (
                OllamaTransparentErrorKey::PreflightRejected,
                "preflight_rejected",
            ),
            (
                OllamaTransparentErrorKey::TransitionLeaseFailed,
                "transition_lease_failed",
            ),
            (OllamaTransparentErrorKey::Unsupported, "unsupported"),
        ];
        for (key, label) in cases {
            assert_eq!(key.as_str(), label);
            assert_eq!(key.to_string(), label);
        }
    }
}
