use bm_entry::{
    EntryAuthConfig, EntryConsoleDeviceCreate, EntryConsoleDeviceUpdate,
    EntryConsoleTransportUpdate, EntryIdempotencyConfig, EntryIdentity, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn config() -> EntryRuntimeConfig {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "console-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 16 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    }
}

#[test]
fn console_surface_exposes_process_config_without_app_key_plaintext() {
    let runtime = EntryRuntime::open(config()).expect("runtime");

    let overview = runtime.console_overview();
    assert_eq!(overview.runtime_shape.store, "in-memory");
    assert_eq!(overview.runtime_shape.shell, "HTTP console");
    assert!(overview
        .memory_context
        .iter()
        .any(|row| row.label == "Owner" && row.value == "owner-default"));
    assert!(overview
        .memory_context
        .iter()
        .any(|row| row.label == "Chat" && row.value == "chat-1"));

    let devices = runtime.console_devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_id, "console-agent");
    assert!(devices[0].app_key_fingerprint.starts_with("fp:"));
    assert!(!devices[0].app_key_fingerprint.contains("owner-default"));
}

#[test]
fn console_device_create_and_rotate_return_plain_key_once_only() {
    let runtime = EntryRuntime::open(config()).expect("runtime");

    let created = runtime
        .console_add_device(EntryConsoleDeviceCreate {
            device_id: Some("edge-node-01".to_string()),
            label: "Edge node".to_string(),
        })
        .expect("device create");
    assert_eq!(created.device.device_id, "edge-node-01");
    assert!(created.app_key_once.starts_with("bm-api-"));
    assert_ne!(created.device.app_key_fingerprint, created.app_key_once);

    let listed = runtime.console_devices();
    let listed_device = listed
        .iter()
        .find(|device| device.device_id == "edge-node-01")
        .expect("listed device");
    assert_eq!(
        listed_device.app_key_fingerprint,
        created.device.app_key_fingerprint
    );
    assert_ne!(listed_device.app_key_fingerprint, created.app_key_once);

    let rotated = runtime
        .console_rotate_device_key("edge-node-01")
        .expect("rotate");
    assert!(rotated.app_key_once.starts_with("bm-api-"));
    assert_ne!(
        rotated.device.app_key_fingerprint,
        created.device.app_key_fingerprint
    );
}

#[test]
fn console_updates_transports_and_devices() {
    let runtime = EntryRuntime::open(config()).expect("runtime");

    let http = runtime
        .console_update_transport(
            "http",
            EntryConsoleTransportUpdate {
                enabled: Some(true),
                endpoint: Some("127.0.0.1:8718".to_string()),
            },
        )
        .expect("http transport");
    assert!(http.enabled);
    assert_eq!(http.status, "ready");
    assert_eq!(http.endpoint, "127.0.0.1:8718");

    let updated = runtime
        .console_update_device(
            "console-agent",
            EntryConsoleDeviceUpdate {
                label: Some("Console owner".to_string()),
                status: Some("disabled".to_string()),
            },
        )
        .expect("device update");
    assert_eq!(updated.label, "Console owner");
    assert_eq!(updated.status, "disabled");
}

#[test]
fn console_llm_gateway_surface_reports_protocols_rules_and_smoke_checks() {
    let mut config = config();
    config.transports = EntryTransportConfig::all_enabled();
    let runtime = EntryRuntime::open(config).expect("runtime");

    let gateway = runtime.console_llm_gateway();

    assert!(gateway.enabled);
    assert_eq!(gateway.status, "ready");
    assert_eq!(gateway.openai_base_url, "http://127.0.0.1:8787/v1");
    assert_eq!(gateway.ollama_base_url, "http://127.0.0.1:8787/api");
    assert_eq!(
        gateway.provider_capabilities_url,
        "http://127.0.0.1:8787/v1/bm/provider-capabilities"
    );
    assert!(gateway
        .protocols
        .iter()
        .any(|protocol| protocol.id == "openai-compatible" && protocol.status == "ready"));
    assert!(gateway
        .protocols
        .iter()
        .any(|protocol| protocol.id == "ollama-native" && protocol.status == "ready"));
    assert!(gateway
        .rule_exports
        .iter()
        .any(|rule| rule.target == "continue"
            && rule
                .command
                .contains("--gateway-url http://127.0.0.1:8787/v1")));
    let provider_check = gateway
        .smoke_checks
        .iter()
        .find(|check| check.id == "provider-capabilities")
        .expect("provider check");
    assert!(provider_check
        .command
        .contains("/v1/bm/provider-capabilities"));
    let run_report = runtime
        .console_run_llm_gateway_smoke_check("provider-capabilities")
        .expect("provider check run report");
    assert_eq!(run_report.id, "provider-capabilities");
    assert_eq!(run_report.command, provider_check.command);
    assert!(matches!(
        run_report.status.as_str(),
        "ready" | "blocked" | "limited"
    ));
    assert!(runtime
        .console_run_llm_gateway_smoke_check("not-a-smoke-check")
        .is_none());
}
