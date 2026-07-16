use bm_adapter::{AdapterCommand, AdapterOperation, AdapterResponse, AdapterSdkReport};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryRuntimeFactory, EntryRuntimeManager, EntryRuntimeScope, EntryScope, EntryTransportConfig,
    EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryRecallRequest, MemoryWriteRequest,
    RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};

mod support;

fn config() -> EntryRuntimeConfig {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let profile = support::host_production_profile();
    EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(profile)
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    }
}

fn context(operation: AdapterOperation, idempotency_key: &str) -> EntryTransportContext {
    EntryTransportContext::new(
        "req-1",
        bm_adapter::TransportKind::Cli,
        bm_adapter::TransportMode::InProcess,
        operation,
        "source-1",
        "local_cli",
        idempotency_key,
        "audit-1",
        support::trusted_local_auth("operator"),
    )
}

fn write_command(name: &str, chat_id: &str, marker: &str) -> AdapterCommand {
    AdapterCommand::Write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: name.to_string(),
            topic: "entry-runtime".to_string(),
            title: format!("Entry runtime {name}"),
            summary: format!(
                "Entry runtime factory shares one MemoryStoreHandle across scoped runtimes. {marker}"
            ),
            content: format!(
                "1. Open EntryRuntimeFactory once for the base store config.\n\
                 2. Resolve an EntryRuntimeScope before handling a gateway request.\n\
                 3. Build the scoped EntryRuntime from the shared MemoryStoreHandle.\n\
                 4. Keep identity and chat scope on the scoped runtime. Marker: {marker}."
            ),
            citations: vec!["entry runtime factory contract".to_string()],
            source_chat_id: Some(chat_id.to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })
}

#[test]
fn persistent_store_config_rejects_relative_path_before_runtime_open() {
    let profile = config().store.profile();
    let error = StoreBackendConfig::file("target/bm-memory-store", profile)
        .expect_err("relative file store path must fail during config construction");
    assert!(error.to_string().contains("absolute"), "{error}");

    let error = StoreBackendConfig::sqlite("target/bm-memory.sqlite3", profile)
        .expect_err("relative sqlite store path must fail during config construction");
    assert!(error.to_string().contains("absolute"), "{error}");
}

#[test]
fn entry_runtime_exposes_store_open_repair_report() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-entry-open-report-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let tmp = root.join("kv").join("session").join("orphan.tmp");
    std::fs::create_dir_all(tmp.parent().unwrap()).unwrap();
    std::fs::write(&tmp, b"partial").unwrap();

    let mut config = config();
    config.store = StoreBackendConfig::file(root, config.store.profile())
        .expect("file store config")
        .with_fsync(false);
    let runtime = EntryRuntime::open(config).expect("entry runtime");
    let report = runtime.store_open_report();
    assert_eq!(report.backend, "file");
    assert!(report.repair.checked);
    assert!(
        report
            .repair
            .findings
            .iter()
            .any(|finding| finding.contains("orphan.tmp")),
        "{report:?}"
    );
}

#[test]
fn entry_runtime_dispatches_adapter_command_through_sdk_runtime() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let response = runtime
        .handle(
            context(AdapterOperation::Recall, "idem-recall-1"),
            AdapterCommand::Recall(MemoryRecallRequest {
                structured_query_facets: Vec::new(),
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        )
        .expect("entry handle");

    match response.adapter {
        AdapterResponse::Accepted {
            request_id,
            audit_id,
            report: AdapterSdkReport::Recall(report),
        } => {
            assert_eq!(request_id, "req-1");
            assert_eq!(audit_id, "audit-1");
            assert_eq!(report.query, "release");
            assert_eq!(response.status.as_str(), "accepted");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn entry_runtime_response_preserves_the_dispatch_budget_lease() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let lease = runtime.acquire_budget_lease().expect("budget lease");
    let expected = lease.report().clone();
    let response = runtime
        .handle_with_budget_lease(
            context(AdapterOperation::Recall, "idem-recall-budget-lease"),
            AdapterCommand::Recall(MemoryRecallRequest {
                structured_query_facets: Vec::new(),
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
            &lease,
        )
        .expect("leased entry handle");

    assert_eq!(response.budget_report.report_id, expected.report_id);
    assert_eq!(
        response.budget_report.adapter_budget,
        expected.adapter_budget
    );
}

#[test]
fn entry_runtime_factory_builds_scoped_runtimes_on_shared_store() {
    let config = config();
    let factory = EntryRuntimeFactory::open(config.base_config()).expect("factory");
    let runtime_a = factory
        .runtime_for_scope(EntryRuntimeScope {
            identity: config.identity.clone(),
            scope: EntryScope {
                channel: "local".to_string(),
                chat_id: "chat-a".to_string(),
            },
        })
        .expect("runtime a");
    let runtime_b = factory
        .runtime_for_scope(EntryRuntimeScope {
            identity: config.identity.clone(),
            scope: EntryScope {
                channel: "local".to_string(),
                chat_id: "chat-b".to_string(),
            },
        })
        .expect("runtime b");

    assert_eq!(runtime_a.runtime().scope().chat_id, "chat-a");
    assert_eq!(runtime_b.runtime().scope().chat_id, "chat-b");

    let write = runtime_a
        .handle(
            context(AdapterOperation::Write, "idem-factory-write"),
            write_command("factory_shared_store", "chat-a", "FACTORY_SHARED_STORE"),
        )
        .expect("write through runtime a");
    match write.adapter {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::Write(report),
            ..
        } => assert!(report.accepted),
        other => panic!("unexpected write: {other:?}"),
    }

    let recall = runtime_b
        .handle(
            context(AdapterOperation::Recall, "idem-factory-recall"),
            AdapterCommand::Recall(MemoryRecallRequest {
                structured_query_facets: Vec::new(),
                query: "entry runtime".to_string(),
                limit: 4,
                tool_registry_refs: Vec::new(),
            }),
        )
        .expect("recall through runtime b");

    match recall.adapter {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::Recall(report),
            ..
        } => assert!(report
            .procedural_hits
            .iter()
            .any(|hit| hit.record.name == "runtime_skill__factory_shared_store")),
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn entry_runtime_manager_reuses_scoped_runtime_idempotency_cache() {
    let config = config();
    let manager = EntryRuntimeManager::open(config.base_config()).expect("manager");
    let scope = config.runtime_scope();
    let runtime_a = manager
        .runtime_for_scope(scope.clone())
        .expect("runtime first lookup");
    let runtime_b = manager
        .runtime_for_scope(scope)
        .expect("runtime second lookup");

    assert!(std::sync::Arc::ptr_eq(&runtime_a, &runtime_b));

    let first = runtime_a
        .handle(
            context(AdapterOperation::Write, "idem-manager-write"),
            write_command("manager_idempotency", "chat-1", "MANAGER_IDEMPOTENCY"),
        )
        .expect("first write");
    let second = runtime_b
        .handle(
            context(AdapterOperation::Write, "idem-manager-write"),
            write_command("manager_idempotency", "chat-1", "MANAGER_IDEMPOTENCY"),
        )
        .expect("second write");

    assert!(matches!(first.adapter, AdapterResponse::Accepted { .. }));
    match second.adapter {
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => assert_eq!(idempotency_key, "idem-manager-write"),
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn entry_runtime_manager_bounds_cache_without_splitting_active_scope() {
    let config = config();
    let manager = EntryRuntimeManager::open_with_requested_max_runtimes(config.base_config(), 1)
        .expect("manager");
    assert_eq!(manager.max_runtimes(), 1);
    let scope_a = config.runtime_scope();
    let scope_b = EntryRuntimeScope {
        identity: config.identity.clone(),
        scope: EntryScope {
            channel: "local".to_string(),
            chat_id: "chat-b".to_string(),
        },
    };

    let runtime_a_first = manager
        .runtime_for_scope(scope_a.clone())
        .expect("runtime a first");
    let first_write = runtime_a_first
        .handle(
            context(AdapterOperation::Write, "idem-manager-active-evicted"),
            write_command("manager_active_evicted", "chat-1", "MANAGER_ACTIVE_EVICTED"),
        )
        .expect("first write");
    let _runtime_b = manager.runtime_for_scope(scope_b).expect("runtime b");
    let runtime_a_second = manager
        .runtime_for_scope(scope_a)
        .expect("runtime a second");

    assert!(std::sync::Arc::ptr_eq(&runtime_a_first, &runtime_a_second));
    assert!(matches!(
        first_write.adapter,
        AdapterResponse::Accepted { .. }
    ));

    let second_write = runtime_a_second
        .handle(
            context(AdapterOperation::Write, "idem-manager-active-evicted"),
            write_command("manager_active_evicted", "chat-1", "MANAGER_ACTIVE_EVICTED"),
        )
        .expect("second write");
    match second_write.adapter {
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => assert_eq!(idempotency_key, "idem-manager-active-evicted"),
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn entry_runtime_manager_rejects_zero_cache_limit() {
    let config = config();
    let error = match EntryRuntimeManager::open_with_requested_max_runtimes(config.base_config(), 0)
    {
        Ok(_) => panic!("zero cache limit should fail"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "entry_runtime_manager");
}

#[test]
fn entry_runtime_manager_uses_the_store_authority_report_and_clamps_requests() {
    let config = config();
    let manager =
        EntryRuntimeManager::open_with_requested_max_runtimes(config.base_config(), usize::MAX)
            .expect("manager");
    let budget = manager.runtime_budget();

    assert_eq!(
        manager.max_runtimes(),
        budget.llm_gateway_budget.runtime_cache_max_runtimes
    );
    assert_eq!(budget.report_id, manager.runtime_budget().report_id);
}

#[test]
fn entry_runtime_rejects_operation_mismatch_before_sdk_runtime_call() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let response = runtime
        .handle(
            context(AdapterOperation::Write, "idem-mismatch-1"),
            AdapterCommand::Recall(MemoryRecallRequest {
                structured_query_facets: Vec::new(),
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        )
        .expect("entry handle");

    match response.adapter {
        AdapterResponse::Rejected { error_key, .. } => {
            assert_eq!(error_key, bm_adapter::AdapterErrorKey::OperationMismatch);
            assert_eq!(response.status.as_str(), "rejected");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}
