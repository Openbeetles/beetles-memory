#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OllamaTransparentErrorKey {
    InvalidConfig,
    PortInspectionFailed,
    RunnerInstallFailed,
    ProcessActionFailed,
    PreflightRejected,
    Unsupported,
}

#[derive(Debug, thiserror::Error)]
#[error("{key:?}: {message}")]
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
