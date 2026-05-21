use bm_entry::{
    EntryAuthConfig, EntryIdentity, EntryIdempotencyConfig, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, HttpRuntimeRequest};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind,
};

fn main() -> bm_sdk::Result<()> {
    let runtime = entry_runtime(ProfileId::ServerLinuxDevFull)?;

    let write = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/write",
            r#"{
              "name":"runtime_skill__server_entry_guard",
              "topic":"server-entry",
              "title":"Server entry guard",
              "summary":"Server runtime accepts HTTP entry requests through bm-entry.",
              "content":"1. Open EntryRuntime with the server profile.\n2. Decode HTTP requests into adapter commands.\n3. Dispatch through the SDK runtime.\n4. Return only adapter reports."
            }"#,
        ),
    )?;
    assert_eq!(write.status_code, 200);

    let recall = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/recall", r#"{"query":"server entry","limit":4}"#),
    )?;
    assert_eq!(recall.status_code, 200);
    assert!(recall.body.contains("\"status\""));

    println!("server-runtime entry smoke passed");
    Ok(())
}

fn entry_runtime(profile: ProfileId) -> bm_sdk::Result<EntryRuntime> {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile,
        identity: EntryIdentity {
            agent_id: "server-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "server".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
}
