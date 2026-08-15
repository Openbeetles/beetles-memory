#[cfg(all(
    not(test),
    not(any(
        feature = "profile-server-linux-memory-gateway",
        feature = "profile-desktop-macos-standalone-memory"
    ))
))]
compile_error!(
    "bm-llm-gateway executable requires exactly one production profile: profile-server-linux-memory-gateway or profile-desktop-macos-standalone-memory"
);

#[cfg(all(
    not(test),
    feature = "profile-server-linux-memory-gateway",
    feature = "profile-desktop-macos-standalone-memory"
))]
compile_error!("bm-llm-gateway executable accepts exactly one production profile");

#[cfg(all(
    not(test),
    feature = "profile-server-linux-memory-gateway",
    not(target_os = "linux")
))]
compile_error!("profile-server-linux-memory-gateway requires target_os=linux");

#[cfg(all(
    not(test),
    feature = "profile-desktop-macos-standalone-memory",
    not(target_os = "macos")
))]
compile_error!("profile-desktop-macos-standalone-memory requires target_os=macos");

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use bm_llm_gateway::{
    serve_llm_gateway_http_accepted_stream_in_request, GatewayConfig, GatewayError,
    GatewayHttpConnectionHandler, GatewayHttpFront, GatewayHttpFrontConfig,
    GatewayHttpRequestBindings, GatewayProviderConfig, GatewayRuntime, ReqwestOllamaNativeUpstream,
    ReqwestOpenAiCompatibleUpstream,
};
use bm_sdk::StoreBackendConfig;
#[cfg(test)]
use bm_sdk::StoreBackendKind;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = GatewayConfig::default_for_local_dev();
    apply_shared_memory_runtime_env(&mut config)?;
    if let Ok(bind_addr) = std::env::var("BM_LLM_GATEWAY_BIND") {
        config.server.bind_addr = bind_addr;
    }
    if let Ok(base_url) = std::env::var("BM_LLM_GATEWAY_OPENAI_BASE_URL") {
        let api_key_env = std::env::var("BM_LLM_GATEWAY_OPENAI_API_KEY_ENV").ok();
        config.providers.insert(
            config.default_provider.clone(),
            GatewayProviderConfig::openai_compatible(base_url, api_key_env.as_deref()),
        );
    }
    let ollama_base_url = std::env::var("BM_LLM_GATEWAY_OLLAMA_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434/api".to_string());
    config.providers.insert(
        "ollama".to_string(),
        GatewayProviderConfig::ollama_native(ollama_base_url),
    );
    if let Ok(default_provider) = std::env::var("BM_LLM_GATEWAY_DEFAULT_PROVIDER") {
        config.default_provider = default_provider;
    }
    config.validate()?;

    let listener = TcpListener::bind(&config.server.bind_addr)?;
    let gateway = Arc::new(GatewayRuntime::open(config)?);
    let front = GatewayHttpFront::new(Arc::clone(&gateway), GatewayHttpFrontConfig::default())?;
    let front_config = front.config();
    front.serve_listener_with_factory(listener, move || {
        Box::new(ReqwestGatewayConnectionHandler {
            gateway: Arc::clone(&gateway),
            request_timeout: front_config.request_timeout,
        })
    })?;
    Ok(())
}

struct ReqwestGatewayConnectionHandler {
    gateway: Arc<GatewayRuntime>,
    request_timeout: std::time::Duration,
}

impl GatewayHttpConnectionHandler for ReqwestGatewayConnectionHandler {
    fn handle(
        &mut self,
        context: &bm_llm_gateway::GatewayRequestBudgetContext,
        stream: &mut bm_entry::EntryAcceptedTcpStream,
    ) -> bm_llm_gateway::Result<()> {
        let mut upstream = ReqwestOpenAiCompatibleUpstream::new_with_timeout(self.request_timeout)?;
        let mut ollama_upstream =
            ReqwestOllamaNativeUpstream::new_with_timeout(self.request_timeout)?;
        serve_llm_gateway_http_accepted_stream_in_request(
            &self.gateway,
            context,
            GatewayHttpRequestBindings::new(&mut upstream, &mut ollama_upstream),
            stream,
        )
    }
}

fn apply_shared_memory_runtime_env(config: &mut GatewayConfig) -> bm_llm_gateway::Result<()> {
    let profile = config.entry.store.profile();
    let store = if env_truthy("BM_MEMORY_STORE_MEMORY") {
        StoreBackendConfig::in_memory(profile).map(|store| store.with_fsync(false))
    } else if let Ok(path) = std::env::var("BM_MEMORY_STORE_SQLITE") {
        StoreBackendConfig::sqlite(
            memory_store_path_from_env("BM_MEMORY_STORE_SQLITE", path)?,
            profile,
        )
        .map(|store| store.with_fsync(true))
    } else if let Ok(path) = std::env::var("BM_MEMORY_STORE_FILE") {
        StoreBackendConfig::file(
            memory_store_path_from_env("BM_MEMORY_STORE_FILE", path)?,
            profile,
        )
        .map(|store| store.with_fsync(true))
    } else {
        return Err(GatewayError::invalid_config(
            "memory store backend must be explicit: set BM_MEMORY_STORE_MEMORY=1 or an absolute BM_MEMORY_STORE_FILE/BM_MEMORY_STORE_SQLITE",
        ));
    }
    .map_err(|error| GatewayError::invalid_config(error.to_string()))?;
    config.entry.store = store;
    if let Ok(owner) = std::env::var("BM_MEMORY_OWNER_ID") {
        config.scope.local_owner_id = Some(owner);
    }
    if let Ok(agent) = std::env::var("BM_MEMORY_AGENT_ID") {
        config.scope.default_agent_id = agent;
    }
    if let Ok(channel) = std::env::var("BM_MEMORY_CHANNEL") {
        config.scope.default_channel = channel;
    }
    if let Ok(chat_id) = std::env::var("BM_MEMORY_CHAT_ID") {
        config.scope.default_chat_id = Some(chat_id);
    }
    if matches!(
        std::env::var("BM_LLM_GATEWAY_SCOPE_PROFILE").as_deref(),
        Ok("ollama_app")
    ) {
        config.scope.ollama_app.enabled = true;
        if let Ok(identity) = std::env::var("BM_LLM_GATEWAY_OLLAMA_APP_ID") {
            config.scope.ollama_app.local_app_identity = identity;
        } else {
            config.scope.ollama_app.local_app_identity = "ollama-app".to_string();
        }
    }
    Ok(())
}

fn memory_store_path_from_env(name: &str, raw: String) -> bm_llm_gateway::Result<PathBuf> {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(GatewayError::invalid_config(format!(
            "{name} must be an absolute path"
        )))
    }
}

fn env_truthy(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    struct EnvRestore {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn new(key: &'static str) -> Self {
            Self {
                key,
                old: std::env::var_os(key),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(value) = self.old.as_ref() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn clear_store_env() -> Vec<EnvRestore> {
        let guards = vec![
            EnvRestore::new("BM_MEMORY_STORE_FILE"),
            EnvRestore::new("BM_MEMORY_STORE_SQLITE"),
            EnvRestore::new("BM_MEMORY_STORE_MEMORY"),
        ];
        std::env::remove_var("BM_MEMORY_STORE_FILE");
        std::env::remove_var("BM_MEMORY_STORE_SQLITE");
        std::env::remove_var("BM_MEMORY_STORE_MEMORY");
        guards
    }

    fn store_env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn llm_gateway_requires_explicit_memory_store() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        let mut config = GatewayConfig::default_for_local_dev();
        let error = apply_shared_memory_runtime_env(&mut config)
            .expect_err("store backend must be explicit");
        assert!(error.to_string().contains("explicit"), "{error}");
    }

    #[test]
    fn llm_gateway_rejects_relative_persistent_store_path() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        std::env::set_var("BM_MEMORY_STORE_FILE", "target/gateway-store");
        let mut config = GatewayConfig::default_for_local_dev();
        let error = apply_shared_memory_runtime_env(&mut config)
            .expect_err("relative file store path must fail");
        assert!(error.to_string().contains("absolute"), "{error}");

        std::env::remove_var("BM_MEMORY_STORE_FILE");
        std::env::set_var("BM_MEMORY_STORE_SQLITE", "target/gateway.sqlite3");
        let mut config = GatewayConfig::default_for_local_dev();
        let error = apply_shared_memory_runtime_env(&mut config)
            .expect_err("relative sqlite store path must fail");
        assert!(error.to_string().contains("absolute"), "{error}");
    }

    #[test]
    fn llm_gateway_accepts_explicit_volatile_memory_store() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        std::env::set_var("BM_MEMORY_STORE_MEMORY", "1");
        let mut config = GatewayConfig::default_for_local_dev();
        apply_shared_memory_runtime_env(&mut config).expect("explicit memory store");
        assert_eq!(config.entry.store.backend(), StoreBackendKind::InMemory);
        assert_eq!(config.entry.store.data_path(), None);
    }
}
