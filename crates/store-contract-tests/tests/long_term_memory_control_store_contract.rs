use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    LongTermMemoryConfidence, LongTermMemoryControlAuditEvent, LongTermMemoryControlRevision,
    LongTermMemoryFreshness, LongTermMemoryStaleHint, LongTermMemoryTombstone,
    LONG_TERM_CONTROL_AUDIT_NAMESPACE, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE, LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
};
use bm_sdk::nonproduction_replay_harness::{StoreBackendConfig, StoreEventScope};
use bm_sdk::{
    LongTermMemoryDraft, LongTermMemoryKind, LongTermMemoryQuery, LongTermMemorySourceScope,
    LongTermMemorySourceType, MemoryCapabilityPolicy, MemoryClock, MemoryGovernancePolicyMutation,
    MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration, MemoryIdentity,
    MemoryLongTermControlView, MemoryLongTermGovernancePolicy, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest,
    MemoryLongTermTarget, MemoryPrivacyClass, MemoryPrivacyPolicy, MemoryRuntime, MemoryScope,
    MemoryStoreHandle, MemoryWriteRequest, NoopMemoryAuditSink, ParsedLongTermMemoryExtraction,
    RuntimeLifecycleModeInput,
};
use std::collections::BTreeSet;
use std::sync::Arc;

const NOW_SECS: u64 = 1_780_010_000;
const MEMORY_SPACE_ID: &str = "space:owner-default";
const MOUNTED_SUBJECT_ID: &str = "subject-human";

struct FixedMemoryClock;

impl MemoryClock for FixedMemoryClock {
    fn now_secs(&self) -> u64 {
        NOW_SECS
    }
}

#[derive(Clone)]
struct ControlFixture {
    owner_id: String,
    revisions: Vec<LongTermMemoryControlRevision>,
    tombstone: LongTermMemoryTombstone,
    policy: MemoryLongTermGovernancePolicy,
    audits: Vec<LongTermMemoryControlAuditEvent>,
}

fn control_scope(memory_space_id: &str) -> StoreEventScope {
    StoreEventScope::new(
        "agent:assistant-main",
        "owner:owner-default",
        "test",
        "chat-a",
    )
    .with_memory_space(memory_space_id)
    .with_subject(MOUNTED_SUBJECT_ID)
}

fn preferred_editor_draft(content: &str, source_revision: u64) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Preference,
        topic: "preferred editor".to_string(),
        content: content.to_string(),
        keywords: vec!["editor".to_string(), "preference".to_string()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("chat-a".to_string()),
        source_type: Some(LongTermMemorySourceType::Conversation),
        source_scope: Some(LongTermMemorySourceScope::User),
        confidence: Some(LongTermMemoryConfidence::High),
        freshness: Some(LongTermMemoryFreshness::Dynamic),
        stale_hint: Some(LongTermMemoryStaleHint::ReviewBeforeUse),
        supporting_citations: vec!["transcript:space-user:chat-a:turn-preferred-editor".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: Some(NOW_SECS - 60),
        last_confirmed_at: Some(NOW_SECS - 60),
        source_revision: Some(source_revision),
    }
}

fn open_store() -> MemoryStoreHandle {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("store config")
    .with_event_scope(control_scope(MEMORY_SPACE_ID));
    MemoryStoreHandle::open_for_nonproduction_harness(config).expect("store handle")
}

fn runtime_for(handle: &MemoryStoreHandle) -> MemoryRuntime {
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("assistant-main", "owner-default").expect("identity"))
        .subject_id(MOUNTED_SUBJECT_ID)
        .scope(MemoryScope::new("test", "chat-a").expect("scope"))
        .store(handle.clone())
        .clock(Arc::new(FixedMemoryClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime");
    assert_eq!(runtime.memory_space_id(), MEMORY_SPACE_ID);
    assert_eq!(runtime.subject_id(), MOUNTED_SUBJECT_ID);
    runtime
}

fn seed_control_store(handle: &MemoryStoreHandle) -> ControlFixture {
    let runtime = runtime_for(handle);
    let write = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![preferred_editor_draft(
                    "Use the integrated editor for this project.",
                    1,
                )],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed preferred-editor owner");
    assert!(write.accepted);
    assert_eq!(write.changed, 1);

    let records = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("list seeded owner")
        .records;
    assert_eq!(records.len(), 1);
    let owner_id = records[0].record.id.clone();

    let correction = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                replacement: preferred_editor_draft("Use the native editor for this project.", 2),
            },
            reason: "user corrected preference".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("correct preferred-editor owner");
    assert!(correction.accepted);

    let deletion = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
            },
            reason: "user deleted memory".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete preferred-editor owner");
    assert!(deletion.accepted);
    assert_eq!(deletion.tombstones.len(), 1);

    let policy_report = runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(MEMORY_SPACE_ID.to_string()),
                    subject_id: Some(MOUNTED_SUBJECT_ID.to_string()),
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: Some("temporary-*".to_string()),
                    source_chat_id: None,
                    source_scope: Some(LongTermMemorySourceScope::User),
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "user does not want temporary preferences remembered".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("persist suppression policy");
    assert!(policy_report.accepted);

    let control = handle
        .replay_harness()
        .scoped_long_term_memory_control_read_store(MEMORY_SPACE_ID)
        .expect("scoped control read store");
    let revisions = control
        .list_long_term_control_revisions(&owner_id, 10)
        .expect("typed control revisions");
    assert!(!revisions.is_empty());
    assert!(revisions.iter().all(|revision| {
        revision.transition.predecessor.owner_ref.owner_id == owner_id
            && revision.memory_space_id == MEMORY_SPACE_ID
            && revision.mounted_subject_id == MOUNTED_SUBJECT_ID
    }));
    let tombstone = control
        .get_long_term_control_tombstone(&owner_id)
        .expect("tombstone read")
        .expect("typed tombstone");
    let policies = control
        .list_long_term_governance_policies(10)
        .expect("policy read");
    assert_eq!(policies.len(), 1);
    let audits = control
        .list_long_term_control_audit(10)
        .expect("audit read");
    let expected_audit_ids = [
        correction.audit_event_id,
        deletion.audit_event_id,
        policy_report.audit_event_id,
    ]
    .into_iter()
    .flatten()
    .collect::<BTreeSet<_>>();
    assert_eq!(
        audits
            .iter()
            .map(|audit| audit.event_id.clone())
            .collect::<BTreeSet<_>>(),
        expected_audit_ids
    );

    ControlFixture {
        owner_id,
        revisions,
        tombstone,
        policy: policies.into_iter().next().expect("policy"),
        audits,
    }
}

#[test]
fn store_platform_persists_long_term_control_metadata_and_events() {
    let handle = open_store();
    let fixture = seed_control_store(&handle);
    let control = handle
        .replay_harness()
        .scoped_long_term_memory_control_read_store(MEMORY_SPACE_ID)
        .expect("scoped control read store");

    assert_eq!(
        control
            .list_long_term_control_revisions(&fixture.owner_id, 10)
            .unwrap(),
        fixture.revisions
    );
    assert_eq!(
        control
            .get_long_term_control_tombstone(&fixture.owner_id)
            .unwrap(),
        Some(fixture.tombstone)
    );
    assert_eq!(
        control.list_long_term_governance_policies(10).unwrap(),
        vec![fixture.policy]
    );
    assert_eq!(
        control.list_long_term_control_audit(10).unwrap(),
        fixture.audits
    );

    let events = handle.replay_harness().read_events().unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind_name == "memory.control"));
    assert!(events
        .iter()
        .any(|event| event.kind_name == "memory.delete"));
    assert!(events
        .iter()
        .any(|event| event.kind_name == "memory.policy"));
}

#[test]
fn scoped_control_events_expose_logical_ids_not_physical_storage_keys() {
    let handle = open_store();
    let fixture = seed_control_store(&handle);
    let control = handle
        .replay_harness()
        .scoped_long_term_memory_control_read_store(MEMORY_SPACE_ID)
        .expect("scoped control read store");
    let other_space = handle
        .replay_harness()
        .scoped_long_term_memory_control_read_store("space:other")
        .expect("other scoped control read store");

    assert_eq!(
        control
            .list_long_term_control_revisions(&fixture.owner_id, 10)
            .unwrap(),
        fixture.revisions
    );
    assert!(other_space
        .list_long_term_control_revisions(&fixture.owner_id, 10)
        .unwrap()
        .is_empty());
    assert!(other_space
        .get_long_term_control_tombstone(&fixture.owner_id)
        .unwrap()
        .is_none());

    let record_keys = handle
        .replay_harness()
        .read_events()
        .expect("read events")
        .into_iter()
        .filter(|event| {
            [
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
                LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
                LONG_TERM_CONTROL_AUDIT_NAMESPACE,
            ]
            .contains(&event.plane.as_str())
        })
        .map(|event| event.record_key)
        .collect::<BTreeSet<_>>();
    let expected_record_keys = fixture
        .audits
        .iter()
        .map(|audit| audit.event_id.clone())
        .chain([
            fixture.tombstone.record_id.clone(),
            fixture.policy.policy_id.clone(),
        ])
        .chain(
            fixture
                .revisions
                .iter()
                .map(|revision| revision.revision_id.clone()),
        )
        .collect::<BTreeSet<_>>();
    assert_eq!(record_keys, expected_record_keys);
}

#[test]
fn snapshot_export_import_preserves_long_term_control_namespaces() {
    let source = open_store();
    let fixture = seed_control_store(&source);

    let snapshot = source.export_replay_snapshot().expect("export snapshot");
    for namespace in [
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    ] {
        assert!(
            snapshot
                .json_docs
                .iter()
                .any(|doc| doc.namespace == namespace),
            "missing namespace {namespace}"
        );
    }

    let target = open_store();
    target
        .import_replay_snapshot(&snapshot)
        .expect("import snapshot");
    let control = target
        .replay_harness()
        .scoped_long_term_memory_control_read_store(MEMORY_SPACE_ID)
        .expect("scoped control read store");

    assert_eq!(
        control
            .list_long_term_control_revisions(&fixture.owner_id, 10)
            .unwrap(),
        fixture.revisions
    );
    assert_eq!(
        control
            .get_long_term_control_tombstone(&fixture.owner_id)
            .unwrap(),
        Some(fixture.tombstone)
    );
    assert_eq!(
        control.list_long_term_governance_policies(10).unwrap(),
        vec![fixture.policy]
    );
    assert_eq!(
        control.list_long_term_control_audit(10).unwrap(),
        fixture.audits
    );
}
