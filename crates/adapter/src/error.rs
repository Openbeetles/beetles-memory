use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorKey {
    InvalidJson,
    Unauthorized,
    Duplicated,
    PayloadTooLarge,
    OperationMismatch,
    UnsupportedOperation,
    RuntimeRejected,
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("operation mismatch: envelope={envelope:?} command={command:?}")]
    OperationMismatch {
        envelope: crate::AdapterOperation,
        command: crate::AdapterOperation,
    },
}
