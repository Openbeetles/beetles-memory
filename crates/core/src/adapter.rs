use crate::RuntimeProfile;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TransportKind {
    Sdk,
    HttpApi,
    Webhook,
    Wss,
    Mqtt,
    Mcp,
    Cli,
    A2aBridge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TransportMode {
    InProcess,
    LocalOnly,
    Client,
    Server,
    Bridge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterCapability {
    pub kind: TransportKind,
    pub mode: TransportMode,
    pub profile: RuntimeProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterPolicy {
    pub capability: AdapterCapability,
    pub requires_auth: bool,
    pub max_payload_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterEvent {
    pub kind: TransportKind,
    pub source: String,
    pub scope: String,
}
