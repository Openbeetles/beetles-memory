#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterBudget {
    pub max_payload_bytes: usize,
    pub max_frame_bytes: usize,
    pub max_subscriptions: usize,
}

impl AdapterBudget {
    pub const fn standard_server() -> Self {
        Self {
            max_payload_bytes: 1024 * 1024,
            max_frame_bytes: 64 * 1024,
            max_subscriptions: 64,
        }
    }

    pub const fn compact_device() -> Self {
        Self {
            max_payload_bytes: 64 * 1024,
            max_frame_bytes: 8 * 1024,
            max_subscriptions: 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterPolicy {
    pub auth_required: bool,
    pub idempotency_required: bool,
    pub source_required: bool,
    pub private_data_allowed: bool,
    pub budget: AdapterBudget,
}

impl AdapterPolicy {
    pub const fn server_authenticated() -> Self {
        Self {
            auth_required: true,
            idempotency_required: true,
            source_required: true,
            private_data_allowed: false,
            budget: AdapterBudget::standard_server(),
        }
    }

    pub const fn compact_device() -> Self {
        Self {
            auth_required: true,
            idempotency_required: true,
            source_required: true,
            private_data_allowed: false,
            budget: AdapterBudget::compact_device(),
        }
    }
}
