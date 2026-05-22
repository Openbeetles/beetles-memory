use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    ManagedRunnerReport, OllamaTransparentConfig, OllamaTransparentTransitionReport,
    PortBindingReport, PortOwnerKind, ProcessActionReport,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OllamaTransparentState {
    Disabled,
    PreflightFailed,
    Enabling,
    Active,
    Degraded,
    Disabling,
    RollingBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaAppReport {
    pub bundle_path: PathBuf,
    pub allow_stop_official_ollama: bool,
    pub open_app_after_enable: bool,
    pub restore_official_after_disable: bool,
    pub last_action: Option<ProcessActionReport>,
}

impl OllamaAppReport {
    pub fn from_config(config: &OllamaTransparentConfig) -> Self {
        Self {
            bundle_path: config.app_bundle_path.clone(),
            allow_stop_official_ollama: config.allow_stop_official_ollama,
            open_app_after_enable: config.open_app_after_enable,
            restore_official_after_disable: config.restore_official_after_disable,
            last_action: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayFrontReport {
    pub expected_owner: PortOwnerKind,
    pub bind: std::net::SocketAddr,
    pub active: bool,
    pub message: Option<String>,
}

impl GatewayFrontReport {
    pub fn from_public_port(public_port: &PortBindingReport) -> Self {
        Self {
            expected_owner: PortOwnerKind::BeetleMemoryTransparentFront,
            bind: public_port.bind,
            active: public_port.owner == PortOwnerKind::BeetleMemoryTransparentFront,
            message: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaTransparentStatus {
    pub state: OllamaTransparentState,
    pub public_port: PortBindingReport,
    pub upstream_port: PortBindingReport,
    pub app: OllamaAppReport,
    pub managed_runner: ManagedRunnerReport,
    pub gateway_front: GatewayFrontReport,
    pub last_transition: Option<OllamaTransparentTransitionReport>,
}
