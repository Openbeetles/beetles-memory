use serde::{Deserialize, Serialize};

use std::net::SocketAddr;

use crate::{
    ExecutableFileIdentity, ManagedRunnerReport, ObservedProcess, OllamaTransparentState,
    PortBindingReport,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreflightBlockerCode {
    InvalidConfig,
    PublicPortOwnedByUnknownProcess,
    PublicPortOwnedByManagedRunner,
    OfficialOllamaStopNotAllowed,
    UpstreamPortUnavailable,
    ManagedRunnerUnavailable,
    GatewayFrontUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightBlocker {
    pub code: PreflightBlockerCode,
    pub message: String,
}

impl PreflightBlocker {
    pub fn new(code: PreflightBlockerCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialOllamaStopPlan {
    pub(crate) allowed: bool,
    pub(crate) targets: Vec<OfficialOllamaStopTarget>,
    pub(crate) reason: String,
}

impl OfficialOllamaStopPlan {
    pub fn allowed(&self) -> bool {
        self.allowed
    }

    pub fn targets(&self) -> &[OfficialOllamaStopTarget] {
        &self.targets
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialOllamaStopTarget {
    pub(crate) bind: SocketAddr,
    pub(crate) process: ObservedProcess,
}

impl OfficialOllamaStopTarget {
    pub fn bind(&self) -> SocketAddr {
        self.bind
    }

    pub fn process(&self) -> &ObservedProcess {
        &self.process
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaTransparentPreflightReport {
    pub accepted: bool,
    pub resulting_state: OllamaTransparentState,
    pub public_port: PortBindingReport,
    pub upstream_port: PortBindingReport,
    pub managed_runner: ManagedRunnerReport,
    pub gateway_executable: Option<ExecutableFileIdentity>,
    pub stop_plan: Option<OfficialOllamaStopPlan>,
    pub blockers: Vec<PreflightBlocker>,
}
