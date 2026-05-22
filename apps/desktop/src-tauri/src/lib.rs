use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, HttpMethod, HttpRuntimeRequest};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, Result, StoreBackendKind};
use serde::{Deserialize, Serialize};
use tauri::Manager;

pub struct DesktopConsoleState {
    runtime: EntryRuntime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopConsoleRequest {
    method: HttpMethod,
    path: String,
    body: String,
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
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConsoleInvokeResponse {
    pub status_code: u16,
    pub body: String,
}

impl DesktopConsoleState {
    pub fn open_for_data_dir(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let data_dir = data_dir.into();
        let store_path = desktop_store_path(&data_dir);
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        let runtime = EntryRuntime::open(EntryRuntimeConfig {
            profile: ProfileId::DesktopMacosStandaloneMemory,
            identity: EntryIdentity {
                agent_id: "bm-desktop".to_string(),
                owner_id: "local-owner".to_string(),
            },
            scope: EntryScope {
                channel: "desktop".to_string(),
                chat_id: "local-desktop".to_string(),
            },
            store: EntryStoreConfig {
                backend: StoreBackendKind::File,
                data_path: Some(store_path),
                fsync: true,
            },
            transports: EntryTransportConfig {
                cli: true,
                http_server: false,
                wss_client: false,
                wss_server: false,
                mcp_server: false,
                a2a_bridge: false,
            },
            auth: EntryAuthConfig::disabled_for_local(),
            idempotency: EntryIdempotencyConfig { max_keys: 4096 },
            privacy: MemoryPrivacyPolicy::standard_private_boundary(),
            capability,
        })?;
        Ok(Self { runtime })
    }

    pub fn handle_console_request(
        &self,
        request: DesktopConsoleRequest,
    ) -> Result<DesktopConsoleResponse> {
        let response = handle_http_request(&self.runtime, request.into_http_runtime_request())?;
        Ok(DesktopConsoleResponse {
            status_code: response.status_code,
            body: response.body,
        })
    }
}

impl TryFrom<DesktopConsoleInvokeRequest> for DesktopConsoleRequest {
    type Error = String;

    fn try_from(value: DesktopConsoleInvokeRequest) -> std::result::Result<Self, Self::Error> {
        match value.method.as_str() {
            "GET" => Ok(Self::get(value.path)),
            "POST" => Ok(Self::post_json(value.path, value.body)),
            "PATCH" => Ok(Self::patch_json(value.path, value.body)),
            "DELETE" => Ok(Self::delete(value.path)),
            other => Err(format!("unsupported desktop console method: {other}")),
        }
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

    #[tauri::command]
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
            let state = DesktopConsoleState::open_for_data_dir(data_dir)
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            app.manage(Mutex::new(state));
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
        }
    }

    pub fn post_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Post,
            path: path.into(),
            body: body.into(),
        }
    }

    pub fn patch_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Patch,
            path: path.into(),
            body: body.into(),
        }
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Delete,
            path: path.into(),
            body: String::new(),
        }
    }

    fn into_http_runtime_request(self) -> HttpRuntimeRequest {
        match self.method {
            HttpMethod::Get => {
                let mut request = HttpRuntimeRequest::get(self.path);
                request.authenticated = true;
                request
            }
            HttpMethod::Post => {
                let mut request = HttpRuntimeRequest::post_json(self.path, self.body);
                request.authenticated = true;
                request
            }
            HttpMethod::Patch => {
                let mut request = HttpRuntimeRequest::patch_json(self.path, self.body);
                request.authenticated = true;
                request
            }
            HttpMethod::Delete => {
                let mut request = HttpRuntimeRequest::delete(self.path);
                request.authenticated = true;
                request
            }
        }
    }
}

fn desktop_store_path(data_dir: &Path) -> PathBuf {
    data_dir.join("store")
}
