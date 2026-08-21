use bm_adapter::{AdapterCommand, AdapterOperation, AdapterResponse};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryCapabilityPolicy,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemoryPrivacyPolicy, MemorySemanticJudgmentSource,
    MemorySubjectVisibilityPolicy, MemoryWriteCandidate, MemoryWriteRequest, StoreBackendConfig,
};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

struct SyntheticStoreDir(PathBuf);

impl SyntheticStoreDir {
    fn create(test_name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bm-entry-{test_name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create synthetic store dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SyntheticStoreDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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

fn context(idempotency_key: &str) -> EntryTransportContext {
    context_for_transport(idempotency_key, bm_adapter::TransportKind::Cli)
}

fn context_for_transport(
    idempotency_key: &str,
    transport: bm_adapter::TransportKind,
) -> EntryTransportContext {
    EntryTransportContext::new(
        "req-write",
        transport,
        bm_adapter::TransportMode::InProcess,
        AdapterOperation::Write,
        "source-1",
        "local_cli",
        idempotency_key,
        "audit-write",
        support::trusted_local_auth("operator"),
    )
}

#[test]
fn explicit_idempotency_replays_and_conflicts_across_transport_boundaries() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let first = runtime
        .handle(
            context_for_transport("cross-transport-key", bm_adapter::TransportKind::Mcp),
            write_command(),
        )
        .expect("first transport write");
    let replay = runtime
        .handle(
            context_for_transport("cross-transport-key", bm_adapter::TransportKind::Wss),
            write_command(),
        )
        .expect("cross transport replay");
    let conflict = runtime
        .handle(
            context_for_transport("cross-transport-key", bm_adapter::TransportKind::Http),
            write_command_with_summary("cross transport conflict"),
        )
        .expect("cross transport conflict");

    assert!(
        matches!(first.adapter, AdapterResponse::Accepted { .. }),
        "first durable mutation must be accepted, got {:?}",
        first.adapter
    );
    assert!(matches!(replay.adapter, AdapterResponse::Replayed { .. }));
    assert!(matches!(
        conflict.adapter,
        AdapterResponse::Rejected {
            error_key: bm_adapter::AdapterErrorKey::MutationOperationConflict,
            ..
        }
    ));
}

fn write_command() -> AdapterCommand {
    write_command_with_summary("Entry runtime dispatches writes through SDK governance.")
}

fn write_command_with_summary(summary: &str) -> AdapterCommand {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Project,
        topic: "entry-idempotency".to_string(),
    };
    AdapterCommand::Write(MemoryWriteRequest::Candidates {
        candidates: vec![MemoryWriteCandidate {
            candidate_id: "entry-idempotency-candidate".to_string(),
            authority: MemoryEvidenceAuthority::ProgramMemoryCanonical,
            target: target.clone(),
            long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
            privacy: MemoryPrivacyClass::SharedWithSubject,
            content: MemoryCandidateContent::Text {
                topic: "entry-idempotency".to_string(),
                body: summary.to_string(),
                keywords: vec!["entry".to_string(), "idempotency".to_string()],
            },
            evidence_refs: vec!["chat-1:entry-idempotency-contract".to_string()],
            canonical_entities: Vec::new(),
            semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                source: MemorySemanticJudgmentSource::RuntimeGate,
                decision: MemoryCandidateSemanticDecision::Accept,
                governed_target: Some(target),
                reason: "entry durable mutation receipt fixture".to_string(),
            }),
        }],
        runtime_skill_owning_scope: None,
    })
}

#[test]
fn idempotency_key_reuse_with_a_different_mutation_payload_is_rejected() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let first = runtime
        .handle(context("idem-conflict-1"), write_command())
        .expect("first write");
    let conflicting = runtime
        .handle(
            context("idem-conflict-1"),
            write_command_with_summary("A different mutation payload must not deduplicate."),
        )
        .expect("conflicting write response");

    assert!(
        matches!(first.adapter, AdapterResponse::Accepted { .. }),
        "first durable mutation must be accepted, got {:?}",
        first.adapter
    );
    match conflicting.adapter {
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => {
            assert_eq!(
                error_key,
                bm_adapter::AdapterErrorKey::MutationOperationConflict
            );
            assert!(reason.contains("different intent"));
        }
        other => panic!("unexpected conflict response: {other:?}"),
    }
}

#[test]
fn mutation_command_with_same_idempotency_key_is_not_dispatched_twice() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let first = runtime
        .handle(context("idem-write-1"), write_command())
        .expect("first write");
    let second = runtime
        .handle(context("idem-write-1"), write_command())
        .expect("second write");

    assert!(matches!(
        first.adapter,
        AdapterResponse::Accepted {
            receipt: Some(_),
            ..
        }
    ));
    match second.adapter {
        AdapterResponse::Replayed {
            mutation_operation_id,
            receipt,
            ..
        } => {
            assert_eq!(mutation_operation_id, "idem-write-1");
            assert_eq!(receipt.mutation_operation_id, "idem-write-1");
            assert_eq!(second.status.as_str(), "replayed");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn durable_mutation_idempotency_survives_entry_runtime_reopen() {
    let store_dir = SyntheticStoreDir::create("durable-idempotency-reopen");
    let mut first_config = config();
    first_config.store =
        StoreBackendConfig::file(store_dir.path(), support::host_production_profile())
            .expect("file store config")
            .with_fsync(false);

    let first = {
        let runtime = EntryRuntime::open(first_config).expect("first entry runtime");
        runtime
            .handle(context("durable-idem-write-1"), write_command())
            .expect("first durable write")
    };
    assert!(
        matches!(first.adapter, AdapterResponse::Accepted { .. }),
        "first durable mutation must be accepted, got {:?}",
        first.adapter
    );

    let mut reopened_config = config();
    reopened_config.store =
        StoreBackendConfig::file(store_dir.path(), support::host_production_profile())
            .expect("reopened file store config")
            .with_fsync(false);
    let reopened = EntryRuntime::open(reopened_config).expect("reopened entry runtime");
    let replay = reopened
        .handle(context("durable-idem-write-1"), write_command())
        .expect("durable replay");

    assert!(
        matches!(replay.adapter, AdapterResponse::Replayed { .. }),
        "reopened runtime must replay the committed operation, got {:?}",
        replay.adapter
    );
}

#[test]
fn foreign_budget_lease_is_rejected_before_idempotency_state_changes() {
    let lease_owner = EntryRuntime::open(config()).expect("lease owner runtime");
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let foreign_lease = lease_owner.acquire_budget_lease().expect("foreign lease");

    let error = match runtime.handle_with_budget_lease(
        context("idem-foreign-budget-lease"),
        write_command(),
        &foreign_lease,
    ) {
        Ok(_) => panic!("foreign authority lease must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "runtime_budget_lease");

    let accepted = runtime
        .handle(context("idem-foreign-budget-lease"), write_command())
        .expect("idempotency key must remain unused after lease rejection");
    assert!(matches!(accepted.adapter, AdapterResponse::Accepted { .. }));
}
