#[cfg(all(not(test), not(feature = "profile-desktop-macos-standalone-memory")))]
compile_error!("bm-desktop requires profile-desktop-macos-standalone-memory");

#[cfg(all(
    not(test),
    feature = "profile-desktop-macos-standalone-memory",
    not(target_os = "macos")
))]
compile_error!("profile-desktop-macos-standalone-memory requires target_os=macos");

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_http::{
    handle_http_in_process_request_with_console, HttpConsoleServices, HttpMethod,
    HttpRuntimeRequest,
};
use bm_ollama_transparent::{
    OllamaTransparentConfig, OllamaTransparentMemoryAuthority, TransparentController,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, Result, StoreBackendConfig};
use serde::{Deserialize, Serialize};
use tauri::Manager;

pub struct DesktopConsoleState {
    runtime: EntryRuntime,
    ollama_transparent: TransparentController,
    memory_event_store_paths: Vec<PathBuf>,
    memory_authority: DesktopMemoryAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopMemoryAuthority {
    pub owner_id: String,
    pub agent_id: String,
    pub channel: String,
    pub chat_id: String,
    pub store_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopRuntimeConfig {
    pub data_dir: PathBuf,
    pub gateway_binary_path: PathBuf,
    pub memory: DesktopMemoryAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopConsoleRequest {
    method: HttpMethod,
    path: String,
    body: String,
    idempotency_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopConsoleResponse {
    pub status_code: u16,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConsoleInvokeRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConsoleInvokeResponse {
    pub status_code: u16,
    pub body: String,
}

impl DesktopConsoleState {
    pub fn open(config: DesktopRuntimeConfig) -> Result<Self> {
        config.validate()?;
        let data_dir = config.data_dir;
        let memory_authority = config.memory;
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        let runtime = EntryRuntime::open(EntryRuntimeConfig {
            identity: EntryIdentity {
                agent_id: memory_authority.agent_id.clone(),
                owner_id: memory_authority.owner_id.clone(),
            },
            scope: EntryScope {
                channel: memory_authority.channel.clone(),
                chat_id: memory_authority.chat_id.clone(),
            },
            store: StoreBackendConfig::file(
                memory_authority.store_path.clone(),
                ProfileId::DesktopMacosStandaloneMemory,
            )?,
            transports: EntryTransportConfig {
                cli: true,
                http_server: false,
                wss_client: false,
                wss_server: false,
                mcp_server: false,
                a2a_bridge: false,
                llm_gateway_server: false,
            },
            auth: EntryAuthConfig::disabled_for_local(),
            idempotency: EntryIdempotencyConfig { max_keys: 4096 },
            privacy: MemoryPrivacyPolicy::standard_private_boundary(),
            capability,
        })?;
        let transparent_authority = OllamaTransparentMemoryAuthority::new(
            memory_authority.owner_id.clone(),
            memory_authority.agent_id.clone(),
            memory_authority.channel.clone(),
            memory_authority.store_path.clone(),
        )
        .map_err(|error| bm_sdk::Error::config("desktop_memory_authority", error.to_string()))?;
        let transparent_config = OllamaTransparentConfig::new(
            &data_dir,
            config.gateway_binary_path,
            transparent_authority,
        )
        .map_err(|error| bm_sdk::Error::config("desktop_ollama_transparent", error.to_string()))?;
        let memory_event_store_paths = vec![memory_authority.store_path.clone()];
        let ollama_transparent =
            TransparentController::new(transparent_config).map_err(|error| {
                bm_sdk::Error::config("desktop_ollama_transparent", error.to_string())
            })?;
        Ok(Self {
            runtime,
            ollama_transparent,
            memory_event_store_paths,
            memory_authority,
        })
    }

    pub fn memory_authority(&self) -> &DesktopMemoryAuthority {
        &self.memory_authority
    }

    pub fn ollama_transparent_config(&self) -> &OllamaTransparentConfig {
        self.ollama_transparent.config()
    }

    pub fn handle_console_request(
        &self,
        request: DesktopConsoleRequest,
    ) -> Result<DesktopConsoleResponse> {
        let response = handle_http_in_process_request_with_console(
            &self.runtime,
            request.into_http_runtime_request(),
            HttpConsoleServices::with_ollama_transparent(&self.ollama_transparent)
                .with_memory_event_store_paths(&self.memory_event_store_paths),
        )?;
        Ok(DesktopConsoleResponse {
            status_code: response.status_code,
            body: response.body,
        })
    }
}

impl TryFrom<DesktopConsoleInvokeRequest> for DesktopConsoleRequest {
    type Error = String;

    fn try_from(value: DesktopConsoleInvokeRequest) -> std::result::Result<Self, Self::Error> {
        let request = match value.method.as_str() {
            "GET" => Ok(Self::get(value.path)),
            "PUT" => Ok(Self::put_json(value.path, value.body)),
            "POST" => Ok(Self::post_json(value.path, value.body)),
            "PATCH" => Ok(Self::patch_json(value.path, value.body)),
            "DELETE" => Ok(Self::delete(value.path)),
            other => Err(format!("unsupported desktop console method: {other}")),
        }?;
        Ok(request.with_idempotency_key(value.idempotency_key))
    }
}

impl From<DesktopConsoleResponse> for DesktopConsoleInvokeResponse {
    fn from(value: DesktopConsoleResponse) -> Self {
        Self {
            status_code: value.status_code,
            body: value.body,
        }
    }
}

pub mod commands {
    use super::*;

    #[tauri::command(async)]
    pub fn console_request(
        state: tauri::State<'_, Mutex<DesktopConsoleState>>,
        request: DesktopConsoleInvokeRequest,
    ) -> std::result::Result<DesktopConsoleInvokeResponse, String> {
        let request = DesktopConsoleRequest::try_from(request)?;
        let state = state
            .lock()
            .map_err(|_| "desktop console state lock poisoned".to_string())?;
        state
            .handle_console_request(request)
            .map(Into::into)
            .map_err(|error| error.to_string())
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            std::fs::create_dir_all(&data_dir)?;
            let gateway_binary_path = bundled_gateway_path()?;
            let memory = DesktopMemoryAuthority {
                owner_id: "local-owner".to_string(),
                agent_id: "bm-desktop".to_string(),
                channel: "desktop".to_string(),
                chat_id: "local-desktop".to_string(),
                store_path: desktop_store_path(&data_dir),
            };
            let state = DesktopConsoleState::open(DesktopRuntimeConfig {
                data_dir,
                gateway_binary_path,
                memory,
            })
            .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            app.manage(Mutex::new(state));

            // On non-macOS platforms (Windows, Linux), re-enable native window decorations.
            // The titleBarStyle "Overlay" + hiddenTitle in tauri.conf.json is macOS-only;
            // on Windows it creates a frameless window without visible controls unless
            // we restore decorations at runtime.
            #[cfg(not(target_os = "macos"))]
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_decorations(true);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::console_request])
        .run(tauri::generate_context!())
        .expect("failed to run Beetle Memory desktop app");
}

impl DesktopConsoleRequest {
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            path: path.into(),
            body: String::new(),
            idempotency_key: String::new(),
        }
    }

    pub fn post_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Post,
            path: path.into(),
            body: body.into(),
            idempotency_key: String::new(),
        }
    }

    pub fn put_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Put,
            path: path.into(),
            body: body.into(),
            idempotency_key: String::new(),
        }
    }

    pub fn patch_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Patch,
            path: path.into(),
            body: body.into(),
            idempotency_key: String::new(),
        }
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Delete,
            path: path.into(),
            body: String::new(),
            idempotency_key: String::new(),
        }
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = key.into();
        self
    }

    fn into_http_runtime_request(self) -> HttpRuntimeRequest {
        let request = match self.method {
            HttpMethod::Get => HttpRuntimeRequest::get(self.path),
            HttpMethod::Post => HttpRuntimeRequest::post_json(self.path, self.body),
            HttpMethod::Put => HttpRuntimeRequest::put_json(self.path, self.body),
            HttpMethod::Patch => HttpRuntimeRequest::patch_json(self.path, self.body),
            HttpMethod::Delete => HttpRuntimeRequest::delete(self.path),
        };
        request.with_idempotency_key(self.idempotency_key)
    }
}

fn desktop_store_path(data_dir: &Path) -> PathBuf {
    data_dir.join("store")
}

impl DesktopRuntimeConfig {
    fn validate(&self) -> Result<()> {
        for (name, path) in [
            ("desktop data_dir", &self.data_dir),
            ("desktop gateway_binary_path", &self.gateway_binary_path),
            ("desktop store_path", &self.memory.store_path),
        ] {
            if !path.is_absolute() {
                return Err(bm_sdk::Error::config(
                    "desktop_runtime_config",
                    format!("{name} must be an explicit absolute path"),
                ));
            }
        }
        for (name, value) in [
            ("owner_id", self.memory.owner_id.as_str()),
            ("agent_id", self.memory.agent_id.as_str()),
            ("channel", self.memory.channel.as_str()),
            ("chat_id", self.memory.chat_id.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(bm_sdk::Error::config(
                    "desktop_runtime_config",
                    format!("desktop memory {name} must not be empty"),
                ));
            }
        }
        Ok(())
    }
}

fn bundled_gateway_path() -> std::result::Result<PathBuf, Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let parent = executable
        .parent()
        .ok_or_else(|| "desktop executable has no parent directory".to_string())?;
    let name = if cfg!(windows) {
        "bm-llm-gateway.exe"
    } else {
        "bm-llm-gateway"
    };
    Ok(parent.join(name))
}
