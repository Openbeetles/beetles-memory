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
pub struct OllamaTransparentConfig {
    pub mode: OllamaTransparentMode,
    pub app_bundle_path: PathBuf,
    pub official_ollama_binary: PathBuf,
    pub managed_runner_path: PathBuf,
    pub gateway_binary_path: PathBuf,
    pub managed_log_dir: PathBuf,
    pub memory_store_path: PathBuf,
    pub maintenance_model: String,
    pub public_bind: SocketAddr,
    pub upstream_bind: SocketAddr,
    pub allow_stop_official_ollama: bool,
    pub open_app_after_enable: bool,
    pub restore_official_after_disable: bool,
}

impl Default for OllamaTransparentConfig {
    fn default() -> Self {
        Self {
            mode: OllamaTransparentMode::Disabled,
            app_bundle_path: PathBuf::from("/Applications/Ollama.app"),
            official_ollama_binary: PathBuf::from(
                "/Applications/Ollama.app/Contents/Resources/ollama",
            ),
            managed_runner_path: default_app_support_dir()
                .join("ollama")
                .join("bin")
                .join("bm-real-ollama"),
            gateway_binary_path: default_gateway_binary_path(),
            managed_log_dir: default_app_support_dir().join("ollama").join("logs"),
            memory_store_path: default_app_support_dir()
                .join("ollama")
                .join("memory-store"),
            maintenance_model: std::env::var("BM_OLLAMA_TRANSPARENT_MAINTENANCE_MODEL")
                .unwrap_or_else(|_| "local".to_string()),
            public_bind: loopback(11434),
            upstream_bind: loopback(11435),
            allow_stop_official_ollama: false,
            open_app_after_enable: true,
            restore_official_after_disable: true,
        }
    }
}

impl OllamaTransparentConfig {
    pub fn for_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            managed_runner_path: data_dir.join("ollama").join("bin").join("bm-real-ollama"),
            managed_log_dir: data_dir.join("ollama").join("logs"),
            memory_store_path: data_dir.join("ollama").join("memory-store"),
            ..Self::default()
        }
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
        if self.app_bundle_path.as_os_str().is_empty() {
            return Err(OllamaTransparentError::invalid_config(
                "app_bundle_path must not be empty",
            ));
        }
        if self.official_ollama_binary.as_os_str().is_empty() {
            return Err(OllamaTransparentError::invalid_config(
                "official_ollama_binary must not be empty",
            ));
        }
        if self.managed_runner_path.as_os_str().is_empty() {
            return Err(OllamaTransparentError::invalid_config(
                "managed_runner_path must not be empty",
            ));
        }
        if self.gateway_binary_path.as_os_str().is_empty() {
            return Err(OllamaTransparentError::invalid_config(
                "gateway_binary_path must not be empty",
            ));
        }
        if self.managed_log_dir.as_os_str().is_empty() {
            return Err(OllamaTransparentError::invalid_config(
                "managed_log_dir must not be empty",
            ));
        }
        if self.memory_store_path.as_os_str().is_empty() {
            return Err(OllamaTransparentError::invalid_config(
                "memory_store_path must not be empty",
            ));
        }
        if self.maintenance_model.trim().is_empty() {
            return Err(OllamaTransparentError::invalid_config(
                "maintenance_model must not be empty",
            ));
        }
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

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn default_app_support_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Beetle Memory")
    } else {
        PathBuf::from("beetle-memory")
    }
}

fn default_gateway_binary_path() -> PathBuf {
    if let Some(path) = std::env::var_os("BM_OLLAMA_TRANSPARENT_GATEWAY_BIN") {
        return PathBuf::from(path);
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            return parent.join("bm-llm-gateway");
        }
    }
    PathBuf::from("bm-llm-gateway")
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
