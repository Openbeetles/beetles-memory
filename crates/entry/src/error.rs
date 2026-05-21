use bm_adapter::AdapterErrorKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryErrorKey {
    InvalidConfig,
    Unauthorized,
    PayloadTooLarge,
    RuntimeRejected,
}

impl EntryErrorKey {
    pub const fn adapter_error_key(self) -> AdapterErrorKey {
        match self {
            Self::InvalidConfig => AdapterErrorKey::RuntimeRejected,
            Self::Unauthorized => AdapterErrorKey::Unauthorized,
            Self::PayloadTooLarge => AdapterErrorKey::PayloadTooLarge,
            Self::RuntimeRejected => AdapterErrorKey::RuntimeRejected,
        }
    }
}
