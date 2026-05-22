use serde::{Deserialize, Serialize};

use crate::{ManagedRunnerReport, ObservedProcess, OllamaTransparentState, PortBindingReport};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreflightBlockerCode {
    InvalidConfig,
    PublicPortOwnedByUnknownProcess,
    PublicPortOwnedByManagedRunner,
    OfficialOllamaStopNotAllowed,
    UpstreamPortUnavailable,
    ManagedRunnerUnavailable,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialOllamaStopPlan {
    pub allowed: bool,
    pub processes: Vec<ObservedProcess>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaTransparentPreflightReport {
    pub accepted: bool,
    pub resulting_state: OllamaTransparentState,
    pub public_port: PortBindingReport,
    pub upstream_port: PortBindingReport,
    pub managed_runner: ManagedRunnerReport,
    pub stop_plan: Option<OfficialOllamaStopPlan>,
    pub blockers: Vec<PreflightBlocker>,
}
