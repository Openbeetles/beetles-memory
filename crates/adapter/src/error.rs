use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorKey {
    InvalidJson,
    Unauthorized,
    Forbidden,
    Duplicated,
    PayloadTooLarge,
    OperationMismatch,
    RuntimeBindingMismatch,
    UnsupportedOperation,
    RuntimeRejected,
}

impl AdapterErrorKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidJson => "invalid_json",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::Duplicated => "duplicated",
            Self::PayloadTooLarge => "payload_too_large",
            Self::OperationMismatch => "operation_mismatch",
            Self::RuntimeBindingMismatch => "runtime_binding_mismatch",
            Self::UnsupportedOperation => "unsupported_operation",
            Self::RuntimeRejected => "runtime_rejected",
        }
    }
}

impl std::fmt::Display for AdapterErrorKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("operation mismatch: envelope={envelope} command={command}")]
    OperationMismatch {
        envelope: crate::AdapterOperation,
        command: crate::AdapterOperation,
    },
}
