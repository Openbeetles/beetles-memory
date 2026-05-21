//! WSS adapter contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WssMessageSpec {
    pub name: &'static str,
    pub inbound_operation: Option<AdapterOperation>,
    pub private_raw_allowed: bool,
}

const MESSAGE_SPECS: &[WssMessageSpec] = &[
    inbound("command.write", AdapterOperation::Write),
    inbound("command.recall", AdapterOperation::Recall),
    inbound("command.project", AdapterOperation::Project),
    inbound("command.inspect", AdapterOperation::Inspect),
    inbound("command.replay", AdapterOperation::Replay),
    inbound("command.capabilities", AdapterOperation::Capabilities),
    stream("subscribe.projection"),
    stream("subscribe.inspection"),
    stream("subscribe.replay"),
    stream("subscribe.capability"),
    stream("event.report"),
    stream("event.lifecycle"),
    stream("event.error"),
];

const fn inbound(name: &'static str, operation: AdapterOperation) -> WssMessageSpec {
    WssMessageSpec {
        name,
        inbound_operation: Some(operation),
        private_raw_allowed: false,
    }
}

const fn stream(name: &'static str) -> WssMessageSpec {
    WssMessageSpec {
        name,
        inbound_operation: None,
        private_raw_allowed: false,
    }
}

pub const fn message_specs() -> &'static [WssMessageSpec] {
    MESSAGE_SPECS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WssBudget {
    pub max_frame_bytes: usize,
    pub max_subscriptions: usize,
}

impl WssBudget {
    pub const fn esp_standalone() -> Self {
        Self {
            max_frame_bytes: 8 * 1024,
            max_subscriptions: 4,
        }
    }

    pub const fn server_gateway() -> Self {
        Self {
            max_frame_bytes: 64 * 1024,
            max_subscriptions: 64,
        }
    }
}
