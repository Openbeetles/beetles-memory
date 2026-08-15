use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{OllamaTransparentError, PortOwnerClassifier, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OllamaTransparentMode {
    Disabled,
    Enabled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaTransparentMemoryAuthority {
    pub owner_id: String,
    pub agent_id: String,
    pub channel: String,
    pub store_path: PathBuf,
}

impl OllamaTransparentMemoryAuthority {
    pub fn new(
        owner_id: impl Into<String>,
        agent_id: impl Into<String>,
        channel: impl Into<String>,
        store_path: impl Into<PathBuf>,
    ) -> Result<Self> {
        let authority = Self {
            owner_id: owner_id.into(),
            agent_id: agent_id.into(),
            channel: channel.into(),
            store_path: store_path.into(),
        };
        authority.validate()?;
        Ok(authority)
    }

    fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("owner_id", self.owner_id.as_str()),
            ("agent_id", self.agent_id.as_str()),
            ("channel", self.channel.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(OllamaTransparentError::invalid_config(format!(
                    "memory authority {name} must not be empty"
                )));
            }
        }
        require_absolute("memory authority store_path", &self.store_path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OllamaTransparentConfig {
    pub mode: OllamaTransparentMode,
    pub app_bundle_path: PathBuf,
    pub official_ollama_binary: PathBuf,
    pub managed_runner_path: PathBuf,
    pub gateway_binary_path: PathBuf,
    pub transition_lease_path: PathBuf,
    pub managed_process_receipt_path: PathBuf,
    pub managed_log_dir: PathBuf,
    pub memory_authority: OllamaTransparentMemoryAuthority,
    pub public_bind: SocketAddr,
    pub upstream_bind: SocketAddr,
    pub allow_stop_official_ollama: bool,
    pub open_app_after_enable: bool,
    pub restore_official_after_disable: bool,
}

impl OllamaTransparentConfig {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        gateway_binary_path: impl Into<PathBuf>,
        memory_authority: OllamaTransparentMemoryAuthority,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        require_absolute("data_dir", &data_dir)?;
        let config = Self {
            mode: OllamaTransparentMode::Disabled,
            app_bundle_path: PathBuf::from("/Applications/Ollama.app"),
            official_ollama_binary: PathBuf::from(
                "/Applications/Ollama.app/Contents/Resources/ollama",
            ),
            managed_runner_path: data_dir.join("ollama").join("bin").join("bm-real-ollama"),
            gateway_binary_path: gateway_binary_path.into(),
            transition_lease_path: data_dir
                .join("ollama")
                .join("control")
                .join("transition.lock"),
            managed_process_receipt_path: data_dir
                .join("ollama")
                .join("control")
                .join("managed-processes.json"),
            managed_log_dir: data_dir.join("ollama").join("logs"),
            memory_authority,
            public_bind: loopback(11434),
            upstream_bind: loopback(11435),
            allow_stop_official_ollama: false,
            open_app_after_enable: true,
            restore_official_after_disable: true,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !self.public_bind.ip().is_loopback() {
            return Err(OllamaTransparentError::invalid_config(
                "public_bind must be loopback because Ollama App transparent mode is local only",
            ));
        }
        if !self.upstream_bind.ip().is_loopback() {
            return Err(OllamaTransparentError::invalid_config(
                "upstream_bind must be loopback because managed Ollama upstream is local only",
            ));
        }
        if self.public_bind == self.upstream_bind {
            return Err(OllamaTransparentError::invalid_config(
                "public_bind and upstream_bind must be different",
            ));
        }
        for (name, path) in [
            ("app_bundle_path", &self.app_bundle_path),
            ("official_ollama_binary", &self.official_ollama_binary),
            ("managed_runner_path", &self.managed_runner_path),
            ("gateway_binary_path", &self.gateway_binary_path),
            ("transition_lease_path", &self.transition_lease_path),
            (
                "managed_process_receipt_path",
                &self.managed_process_receipt_path,
            ),
            ("managed_log_dir", &self.managed_log_dir),
        ] {
            require_absolute(name, path)?;
        }
        self.memory_authority.validate()?;
        if self.managed_runner_path == self.official_ollama_binary {
            return Err(OllamaTransparentError::invalid_config(
                "managed_runner_path must not equal official_ollama_binary",
            ));
        }
        if path_is_under(&self.managed_runner_path, &self.app_bundle_path) {
            return Err(OllamaTransparentError::invalid_config(
                "managed_runner_path must not live inside the Ollama.app bundle",
            ));
        }
        Ok(())
    }

    pub fn port_owner_classifier(&self) -> PortOwnerClassifier {
        PortOwnerClassifier::new(
            self.official_ollama_binary.clone(),
            self.managed_runner_path.clone(),
        )
    }
}

fn require_absolute(name: &str, path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(OllamaTransparentError::invalid_config(format!(
            "{name} must be an explicit absolute path"
        )));
    }
    Ok(())
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn path_is_under(path: &Path, parent: &Path) -> bool {
    let path_components = path.components().collect::<Vec<_>>();
    let parent_components = parent.components().collect::<Vec<_>>();
    path_components.len() >= parent_components.len()
        && path_components
            .iter()
            .zip(parent_components.iter())
            .all(|(left, right)| left == right)
}
