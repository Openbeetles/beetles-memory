use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_entry::{
    ConfiguredGovernanceLlmClient, EntryAuthConfig, EntryGovernanceModelAuthMode,
    EntryGovernanceModelConfigUpdate, EntryGovernanceModelProtocol, EntryIdempotencyConfig,
    EntryIdentity, EntryRuntime, EntryRuntimeConfig, EntryScope, EntryTransportConfig,
};
use bm_sdk::{
    Error, LlmClient, LlmHttpClient, MemoryCapabilityPolicy, MemoryPrivacyPolicy, Message,
    ResponseBody, StoreBackendConfig, ToolChoicePolicy,
};

mod support;

fn runtime(
    store: StoreBackendConfig,
    agent_id: &str,
    channel: &str,
    chat_id: &str,
    privacy: MemoryPrivacyPolicy,
) -> EntryRuntime {
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: agent_id.to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: channel.to_string(),
            chat_id: chat_id.to_string(),
        },
        store,
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy,
        capability: MemoryCapabilityPolicy::strict_profile(),
    })
    .expect("entry runtime")
}

fn default_runtime(store: StoreBackendConfig) -> EntryRuntime {
    runtime(
        store,
        "governance-config-agent",
        "console",
        "local-console",
        MemoryPrivacyPolicy::standard_private_boundary(),
    )
}

fn credential_env() -> EntryGovernanceModelAuthMode {
    EntryGovernanceModelAuthMode::CredentialEnv {
        credential_env: "BEETLE_MEMORY_LLM_API_KEY".to_string(),
    }
}

fn update(
    protocol: EntryGovernanceModelProtocol,
    endpoint: &str,
) -> EntryGovernanceModelConfigUpdate {
    EntryGovernanceModelConfigUpdate {
        enabled: true,
        protocol,
        endpoint: endpoint.to_string(),
        model: "qwen3:8b".to_string(),
        auth_mode: credential_env(),
        request_timeout_ms: 30_000,
        max_input_tokens: 8_192,
        max_output_tokens: 1_024,
    }
}

fn temp_store(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("bm-governance-model-{label}-{nanos}"))
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn revision_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for binding in fs::read_dir(root).expect("binding root") {
        let binding = binding.expect("binding entry").path();
        if !binding.is_dir() {
            continue;
        }
        for entry in fs::read_dir(binding).expect("binding directory") {
            let path = entry.expect("revision entry").path();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if name.starts_with("revision-") && name.ends_with(".json") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn file_and_sqlite_configs_use_immutable_per_binding_revisions() {
    const SQLITE_ENABLED: bool = cfg!(any(
        feature = "profile-desktop-macos-standalone-memory",
        feature = "profile-desktop-macos-embedded-sdk",
        feature = "profile-desktop-macos-dev-full",
        feature = "profile-desktop-windows-embedded-sdk",
        feature = "profile-desktop-windows-dev-full",
        feature = "profile-desktop-linux-embedded-sdk",
        feature = "profile-linux-device-standalone-memory",
        feature = "profile-server-linux-memory-gateway",
        feature = "profile-server-linux-dev-full"
    ));
    for backend in ["file", "sqlite"] {
        if backend == "sqlite" && !SQLITE_ENABLED {
            continue;
        }
        let path = temp_store(backend);
        let revision_root = append_suffix(&path, ".memory-governance-model");
        let profile = support::host_production_profile();
        let store = match backend {
            "file" => StoreBackendConfig::file(&path, profile),
            "sqlite" => StoreBackendConfig::sqlite(&path, profile),
            _ => unreachable!(),
        }
        .expect("persistent store")
        .with_fsync(false);

        let first = default_runtime(store.clone());
        let saved = first
            .console_update_governance_model(update(
                EntryGovernanceModelProtocol::OpenAiCompatible,
                "https://api.example.test/v1",
            ))
            .expect("save config");
        assert!(saved.configured);
        assert_eq!(saved.config_revision, Some(1));
        assert_eq!(saved.auth_mode, Some(credential_env()));
        drop(first);

        let reopened = default_runtime(store);
        assert_eq!(reopened.console_governance_model(), saved);
        let revisions = revision_files(&revision_root);
        assert_eq!(
            revisions.len(),
            1,
            "{backend} must retain one immutable revision"
        );
        assert!(revisions[0].ends_with("revision-00000000000000000001.json"));

        drop(reopened);
        let _ = fs::remove_dir_all(&revision_root);
        let _ = fs::remove_dir_all(&path);
        let _ = fs::remove_file(&path);
    }
}

#[test]
fn exact_runtime_bindings_keep_subject_scope_and_privacy_configs_isolated() {
    let path = temp_store("binding-isolation");
    let revision_root = append_suffix(&path, ".memory-governance-model");
    let store = StoreBackendConfig::file(&path, support::host_production_profile())
        .expect("file store")
        .with_fsync(false);

    let subject_a = runtime(
        store.clone(),
        "agent-a",
        "console",
        "chat-a",
        MemoryPrivacyPolicy::standard_private_boundary(),
    );
    let mut private_projection = MemoryPrivacyPolicy::standard_private_boundary();
    private_projection.private_plane_projection_allowed = true;
    let subject_b = runtime(
        store.clone(),
        "agent-b",
        "console",
        "chat-b",
        private_projection,
    );
    let saved_a = subject_a
        .console_update_governance_model(update(
            EntryGovernanceModelProtocol::OpenAiCompatible,
            "https://a.example.test/v1",
        ))
        .expect("subject a config");
    let saved_b = subject_b
        .console_update_governance_model(update(
            EntryGovernanceModelProtocol::OpenAiCompatible,
            "https://b.example.test/v1",
        ))
        .expect("subject b config");

    assert_ne!(saved_a.binding_id, saved_b.binding_id);
    assert_eq!(
        saved_a.endpoint.as_deref(),
        Some("https://a.example.test/v1")
    );
    assert_eq!(
        saved_b.endpoint.as_deref(),
        Some("https://b.example.test/v1")
    );
    assert_eq!(revision_files(&revision_root).len(), 2);

    drop((subject_a, subject_b));
    let reopened_a = runtime(
        store.clone(),
        "agent-a",
        "console",
        "chat-a",
        MemoryPrivacyPolicy::standard_private_boundary(),
    );
    assert_eq!(reopened_a.console_governance_model(), saved_a);

    drop(reopened_a);
    let _ = fs::remove_dir_all(&revision_root);
    let _ = fs::remove_dir_all(&path);
}

#[test]
fn concurrent_updates_reread_latest_revision_under_the_file_lock() {
    let path = temp_store("concurrent");
    let revision_root = append_suffix(&path, ".memory-governance-model");
    let store = StoreBackendConfig::file(&path, support::host_production_profile())
        .expect("file store")
        .with_fsync(false);
    let first = Arc::new(default_runtime(store.clone()));
    let second = Arc::new(default_runtime(store.clone()));
    let barrier = Arc::new(Barrier::new(3));

    let writers = [(first, "a"), (second, "b")]
        .into_iter()
        .map(|(runtime, model)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut candidate = update(
                    EntryGovernanceModelProtocol::OpenAiCompatible,
                    "https://api.example.test/v1",
                );
                candidate.model = model.to_string();
                barrier.wait();
                runtime
                    .console_update_governance_model(candidate)
                    .expect("concurrent update")
                    .config_revision
                    .expect("revision")
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let mut revisions = writers
        .into_iter()
        .map(|writer| writer.join().expect("writer thread"))
        .collect::<Vec<_>>();
    revisions.sort_unstable();
    assert_eq!(revisions, vec![1, 2]);
    assert_eq!(revision_files(&revision_root).len(), 2);

    let reopened = default_runtime(store);
    assert_eq!(reopened.console_governance_model().config_revision, Some(2));
    drop(reopened);
    let _ = fs::remove_dir_all(&revision_root);
    let _ = fs::remove_dir_all(&path);
}

#[test]
fn legacy_single_sidecar_requires_explicit_reset_without_mutation() {
    let path = temp_store("legacy");
    let legacy_path = append_suffix(&path, ".memory-governance-model.json");
    let revision_root = append_suffix(&path, ".memory-governance-model");
    let legacy_bytes = br#"{"schemaVersion":2,"bindingId":"old-global-sidecar"}"#;
    fs::write(&legacy_path, legacy_bytes).expect("legacy fixture");
    let store = StoreBackendConfig::file(&path, support::host_production_profile())
        .expect("file store")
        .with_fsync(false);

    let error = match EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "governance-config-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "local-console".to_string(),
        },
        store,
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability: MemoryCapabilityPolicy::strict_profile(),
    }) {
        Ok(_) => panic!("legacy sidecar must fail closed"),
        Err(error) => error,
    };
    match error {
        Error::Config { message, stage } => {
            assert_eq!(stage, "entry_governance_model");
            assert_eq!(message, "legacy_reset_required");
        }
        other => panic!("unexpected error: {other}"),
    }
    assert_eq!(
        fs::read(&legacy_path).expect("legacy remains"),
        legacy_bytes
    );
    assert!(!revision_root.exists());

    let _ = fs::remove_dir_all(&path);
    fs::remove_file(legacy_path).expect("cleanup legacy fixture");
}

#[test]
fn auth_mode_allows_unauthenticated_loopback_and_rejects_remote_endpoints() {
    let runtime = default_runtime(
        StoreBackendConfig::in_memory(support::host_production_profile())
            .expect("memory store")
            .with_fsync(false),
    );
    let mut local = update(
        EntryGovernanceModelProtocol::OllamaNative,
        "http://127.0.0.1:11434/api",
    );
    local.auth_mode = EntryGovernanceModelAuthMode::LocalUnauthenticated;
    runtime
        .console_update_governance_model(local)
        .expect("loopback unauthenticated config");

    let mut remote = update(
        EntryGovernanceModelProtocol::OpenAiCompatible,
        "https://api.example.test/v1",
    );
    remote.auth_mode = EntryGovernanceModelAuthMode::LocalUnauthenticated;
    let error = runtime
        .console_update_governance_model(remote)
        .expect_err("remote endpoint without credential must fail");
    assert!(error
        .to_string()
        .contains("remote_endpoint_requires_credential_env"));
}

#[test]
fn protocol_probe_paths_are_exact_for_local_and_remote_endpoints() {
    let runtime = default_runtime(
        StoreBackendConfig::in_memory(support::host_production_profile())
            .expect("memory store")
            .with_fsync(false),
    );

    let mut local = update(
        EntryGovernanceModelProtocol::OpenAiCompatible,
        "http://localhost:11434/v1/",
    );
    local.auth_mode = EntryGovernanceModelAuthMode::LocalUnauthenticated;
    runtime
        .console_update_governance_model(local)
        .expect("openai config");
    let openai = runtime
        .console_governance_model_probe_plan()
        .expect("openai probe");
    assert_eq!(openai.url, "http://localhost:11434/v1/chat/completions");
    assert_eq!(
        openai.auth_mode,
        EntryGovernanceModelAuthMode::LocalUnauthenticated
    );

    runtime
        .console_update_governance_model(update(
            EntryGovernanceModelProtocol::OpenAiCompatible,
            "https://api.openai.com/v1",
        ))
        .expect("remote endpoint config");
    assert_eq!(
        runtime
            .console_governance_model_probe_plan()
            .expect("remote probe")
            .url,
        "https://api.openai.com/v1/chat/completions"
    );
}

#[test]
fn invalid_endpoint_model_and_credential_reference_fail_closed() {
    let runtime = default_runtime(
        StoreBackendConfig::in_memory(support::host_production_profile())
            .expect("memory store")
            .with_fsync(false),
    );

    for (mut candidate, expected) in [
        (
            update(
                EntryGovernanceModelProtocol::OpenAiCompatible,
                "file:///tmp/model",
            ),
            "http",
        ),
        (
            update(
                EntryGovernanceModelProtocol::OpenAiCompatible,
                "https://api.example.test/v1",
            ),
            "model",
        ),
        (
            update(
                EntryGovernanceModelProtocol::OpenAiCompatible,
                "https://api.example.test/v1",
            ),
            "credential_env",
        ),
    ] {
        if expected == "model" {
            candidate.model.clear();
        }
        if expected == "credential_env" {
            candidate.auth_mode = EntryGovernanceModelAuthMode::CredentialEnv {
                credential_env: "RAW sk-secret".to_string(),
            };
        }
        let error = runtime
            .console_update_governance_model(candidate)
            .expect_err("invalid config must fail");
        assert!(
            error.to_string().contains(expected),
            "case {expected}: {error}"
        );
    }
}

#[derive(Default)]
struct RecordingGovernanceHttp {
    url: String,
    body: Vec<u8>,
    response: Vec<u8>,
}

impl LlmHttpClient for RecordingGovernanceHttp {
    fn do_post(
        &mut self,
        url: &str,
        _headers: &[(&str, &str)],
        body: &[u8],
    ) -> bm_sdk::Result<(u16, ResponseBody)> {
        self.url = url.to_string();
        self.body = body.to_vec();
        Ok((200, ResponseBody::Heap(self.response.clone())))
    }
}

#[test]
fn saved_binding_drives_the_exact_governance_llm_protocol_client() {
    for (protocol, endpoint, expected_url, response) in [
        (
            EntryGovernanceModelProtocol::OpenAiCompatible,
            "http://127.0.0.1:11434/v1",
            "http://127.0.0.1:11434/v1/chat/completions",
            br#"{"choices":[{"message":{"content":"openai-ok"},"finish_reason":"stop"}]}"#
                .as_slice(),
        ),
        (
            EntryGovernanceModelProtocol::OllamaNative,
            "http://127.0.0.1:11434/api",
            "http://127.0.0.1:11434/api/chat",
            br#"{"message":{"content":"ollama-ok"},"done_reason":"stop"}"#.as_slice(),
        ),
    ] {
        let runtime = default_runtime(
            StoreBackendConfig::in_memory(support::host_production_profile())
                .expect("memory store"),
        );
        let mut config = update(protocol, endpoint);
        config.auth_mode = EntryGovernanceModelAuthMode::LocalUnauthenticated;
        runtime
            .console_update_governance_model(config)
            .expect("save binding");
        let binding = runtime
            .governance_model_execution_binding()
            .expect("execution binding");
        assert_eq!(binding.config_revision, 1);
        let client = ConfiguredGovernanceLlmClient::new(binding);
        let mut http = RecordingGovernanceHttp {
            response: response.to_vec(),
            ..RecordingGovernanceHttp::default()
        };
        let reply = client
            .chat(
                &mut http,
                "system",
                &[Message {
                    role: "user".into(),
                    content: "hello".to_string(),
                }],
                None,
                ToolChoicePolicy::Auto,
            )
            .expect("governance chat");
        assert_eq!(http.url, expected_url);
        let body: serde_json::Value = serde_json::from_slice(&http.body).expect("request json");
        assert_eq!(body["model"], "qwen3:8b");
        assert!(reply.content.ends_with("-ok"));
    }
}

#[test]
fn immutable_config_revision_remains_resolvable_after_a_model_update() {
    let runtime = default_runtime(
        StoreBackendConfig::in_memory(support::host_production_profile()).expect("memory store"),
    );
    let mut first = update(
        EntryGovernanceModelProtocol::OpenAiCompatible,
        "http://127.0.0.1:11434/v1",
    );
    first.auth_mode = EntryGovernanceModelAuthMode::LocalUnauthenticated;
    first.model = "model-v1".to_string();
    runtime
        .console_update_governance_model(first)
        .expect("save revision one");
    let mut second = update(
        EntryGovernanceModelProtocol::OllamaNative,
        "http://127.0.0.1:11434/api",
    );
    second.auth_mode = EntryGovernanceModelAuthMode::LocalUnauthenticated;
    second.model = "model-v2".to_string();
    runtime
        .console_update_governance_model(second)
        .expect("save revision two");

    let pinned = runtime
        .governance_model_execution_binding_for_revision(1)
        .expect("resolve pinned revision");
    let current = runtime
        .governance_model_execution_binding()
        .expect("resolve current revision");
    assert_eq!(pinned.config_revision, 1);
    assert_eq!(pinned.model, "model-v1");
    assert_eq!(current.config_revision, 2);
    assert_eq!(current.model, "model-v2");
}
