use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use bm_llm_gateway::{
    serve_llm_gateway_http_stream_with_services, GatewayConfig, GatewayError,
    GatewayHttpConnectionHandler, GatewayHttpFront, GatewayHttpFrontConfig, GatewayProviderConfig,
    GatewayProviderKind, GatewayRuntime, OllamaMaintenanceLlmClient, OpenAiGatewayServices,
    OpenAiMaintenanceLlmClient, ReqwestGatewayLlmHttpClient, ReqwestOllamaNativeUpstream,
    ReqwestOpenAiCompatibleUpstream,
};
use bm_sdk::StoreBackendKind;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = GatewayConfig::default_for_local_dev();
    apply_shared_memory_runtime_env(&mut config);
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
    let gateway = Arc::new(GatewayRuntime::open(config.clone())?);
    let front = GatewayHttpFront::new(GatewayHttpFrontConfig::default())?;
    let front_config = front.config();
    front.serve_listener_with_factory(listener, move || {
        Box::new(ReqwestGatewayConnectionHandler {
            gateway: Arc::clone(&gateway),
            config: config.clone(),
            request_timeout: front_config.request_timeout,
        })
    })?;
    Ok(())
}

struct ReqwestGatewayConnectionHandler {
    gateway: Arc<GatewayRuntime>,
    config: GatewayConfig,
    request_timeout: std::time::Duration,
}

impl GatewayHttpConnectionHandler for ReqwestGatewayConnectionHandler {
    fn handle(&mut self, stream: &mut TcpStream) -> bm_llm_gateway::Result<()> {
        let mut upstream = ReqwestOpenAiCompatibleUpstream::new_with_timeout(self.request_timeout)?;
        let mut ollama_upstream =
            ReqwestOllamaNativeUpstream::new_with_timeout(self.request_timeout)?;
        let mut maintenance_http =
            ReqwestGatewayLlmHttpClient::new_with_timeout(self.request_timeout)?;
        let maintenance_provider_name = std::env::var("BM_LLM_GATEWAY_MAINTENANCE_PROVIDER")
            .unwrap_or_else(|_| self.config.default_provider.clone());
        let maintenance_provider = self
            .config
            .providers
            .get(&maintenance_provider_name)
            .ok_or_else(|| {
                GatewayError::invalid_config(format!(
                    "maintenance provider is not configured: {maintenance_provider_name}"
                ))
            })?
            .clone();
        let maintenance_model = std::env::var("BM_LLM_GATEWAY_MAINTENANCE_MODEL")
            .unwrap_or_else(|_| "local".to_string());
        match maintenance_provider.kind {
            GatewayProviderKind::OpenAiCompatible => {
                let maintenance_llm =
                    OpenAiMaintenanceLlmClient::new(maintenance_provider, maintenance_model);
                let mut services = OpenAiGatewayServices::new()
                    .with_maintenance(&mut maintenance_http, &maintenance_llm);
                serve_llm_gateway_http_stream_with_services(
                    &self.gateway,
                    &self.config,
                    &mut upstream,
                    &mut ollama_upstream,
                    &mut services,
                    stream,
                )
            }
            GatewayProviderKind::OllamaNative => {
                let maintenance_llm =
                    OllamaMaintenanceLlmClient::new(maintenance_provider, maintenance_model);
                let mut services = OpenAiGatewayServices::new()
                    .with_maintenance(&mut maintenance_http, &maintenance_llm);
                serve_llm_gateway_http_stream_with_services(
                    &self.gateway,
                    &self.config,
                    &mut upstream,
                    &mut ollama_upstream,
                    &mut services,
                    stream,
                )
            }
        }
    }
}

fn apply_shared_memory_runtime_env(config: &mut GatewayConfig) {
    config.entry.store.backend = StoreBackendKind::File;
    config.entry.store.data_path = Some("target/bm-memory-gateway-store".into());
    config.entry.store.fsync = true;

    if env_truthy("BM_MEMORY_STORE_MEMORY") {
        config.entry.store.backend = StoreBackendKind::InMemory;
        config.entry.store.data_path = None;
        config.entry.store.fsync = false;
    } else if let Ok(path) = std::env::var("BM_MEMORY_STORE_SQLITE") {
        config.entry.store.backend = StoreBackendKind::Sqlite;
        config.entry.store.data_path = Some(path.into());
        config.entry.store.fsync = true;
    } else if let Ok(path) = std::env::var("BM_MEMORY_STORE_FILE") {
        config.entry.store.backend = StoreBackendKind::File;
        config.entry.store.data_path = Some(path.into());
        config.entry.store.fsync = true;
    }
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
}

fn env_truthy(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}
