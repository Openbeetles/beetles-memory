use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    build_long_term_memory_facet_index_doc, canonical_evidence_ref_from_source,
    plan_long_term_memory_control_mutation, plan_long_term_memory_upsert,
    scoped_long_term_control_storage_key, scoped_long_term_memory_storage_key,
    scoped_memory_facet_owner_storage_key, CanonicalEntityKey, CanonicalEntityKind,
    CanonicalEntityRef, ControlEffectRef, LongTermControlOperation, LongTermMemoryConfidence,
    LongTermMemoryControlAuditEvent, LongTermMemoryControlMutationRequest,
    LongTermMemoryControlRevision, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryEntryPlan, LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemoryOwnerWrite,
    LongTermMemorySourceScope, LongTermMemorySourceType, LongTermMemoryStaleHint,
    LongTermMemoryTombstone, MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration,
    MemoryLongTermGovernancePolicy, MemoryLongTermMutation, MemoryLongTermTarget,
    MemoryPrivacyClass, LONG_TERM_CONTROL_AUDIT_NAMESPACE, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_SCHEMA_VERSION, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE, MEMORY_FACET_INDEX_NAMESPACE,
};
use bm_sdk::nonproduction_replay_harness::{
    MemoryStoreEventKind, StoreBackendConfig, StoreEventScope, StoreJsonPrecondition,
    StoreMutation, StoreMutationBatch, StorePlatform,
};
use serde::Serialize;
use std::collections::BTreeSet;

const NOW_SECS: u64 = 1_780_010_000;

#[derive(Clone)]
struct ControlFixture {
    revision: LongTermMemoryControlRevision,
    tombstone: LongTermMemoryTombstone,
    policy: MemoryLongTermGovernancePolicy,
    audits: Vec<LongTermMemoryControlAuditEvent>,
}

fn control_scope(memory_space_id: &str) -> StoreEventScope {
    StoreEventScope::new(
        "agent:assistant-main",
        "owner:assistant-main",
        "test",
        "chat-a",
    )
    .with_memory_space(memory_space_id)
    .with_subject("subject-human")
}

fn preferred_editor_draft(content: &str, source_revision: u64) -> LongTermMemoryDraft {
    let citation = "transcript:space-user:chat-a:turn-preferred-editor";
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Preference,
        topic: "preferred editor".to_string(),
        content: content.to_string(),
        keywords: vec!["editor".to_string(), "preference".to_string()],
        privacy: MemoryPrivacyClass::PrivateGarden,
        source_chat_id: Some("chat-a".to_string()),
        source_type: Some(LongTermMemorySourceType::Conversation),
        source_scope: Some(LongTermMemorySourceScope::User),
        confidence: Some(LongTermMemoryConfidence::High),
        freshness: Some(LongTermMemoryFreshness::Dynamic),
        stale_hint: Some(LongTermMemoryStaleHint::ReviewBeforeUse),
        supporting_citations: vec![citation.to_string()],
        canonical_entities: vec![CanonicalEntityRef {
            key: CanonicalEntityKey {
                kind: CanonicalEntityKind::Product,
                canonical_id: "editor-preference".to_string(),
            },
            display_label: Some("Editor preference".to_string()),
            aliases: vec!["preferred editor".to_string()],
            evidence_refs: vec![canonical_evidence_ref_from_source(citation)
                .expect("canonical editor-preference evidence")],
        }],
        evidence_count: Some(1),
        observed_at: Some(NOW_SECS - 60),
        last_confirmed_at: Some(NOW_SECS - 60),
        source_revision: Some(source_revision),
    }
}

fn policy(memory_space_id: &str) -> MemoryLongTermGovernancePolicy {
    MemoryLongTermGovernancePolicy {
        schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
        policy_revision: 1,
        memory_space_id: memory_space_id.to_string(),
        policy_id: "policy-temp-preferences".to_string(),
        kind: "suppress".to_string(),
        selector: MemoryGovernanceSelector {
            memory_space_id: Some(memory_space_id.to_string()),
            subject_id: Some("agent:assistant-main".to_string()),
            kind: Some(LongTermMemoryKind::Preference),
            topic_pattern: Some("temporary-*".to_string()),
            source_chat_id: None,
            source_scope: Some(LongTermMemorySourceScope::User),
        },
        duration: Some(MemoryGovernanceSuppressionDuration::UntilManualResume),
        expires_at: None,
        reason: "user does not want temporary preferences remembered".to_string(),
        created_at: NOW_SECS + 3,
        updated_at: NOW_SECS + 3,
    }
}

fn policy_audit(policy: &MemoryLongTermGovernancePolicy) -> LongTermMemoryControlAuditEvent {
    LongTermMemoryControlAuditEvent::new(
        "audit-policy-suppress-temporary-preferences",
        "audit-policy-suppress-temporary-preferences",
        LongTermControlOperation::PolicySuppress,
        vec![ControlEffectRef::Policy {
            policy_id: policy.policy_id.clone(),
            policy_revision: policy.policy_revision,
            deleted: false,
        }],
        policy.reason.clone(),
        Some("subject-human".to_string()),
        policy.memory_space_id.clone(),
        NOW_SECS + 3,
    )
}

fn push_control_seed<T: Serialize>(
    mutations: &mut Vec<StoreMutation>,
    preconditions: &mut Vec<StoreJsonPrecondition>,
    memory_space_id: &str,
    namespace: &str,
    logical_key: &str,
    value: &T,
    event_kind: MemoryStoreEventKind,
) {
    let key = scoped_long_term_control_storage_key(memory_space_id, namespace, logical_key)
        .expect("scoped control key");
    preconditions.push(StoreJsonPrecondition::Absent {
        namespace: namespace.to_string(),
        key: key.clone(),
    });
    mutations.push(StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key,
        value: serde_json::to_value(value).expect("serialize control metadata"),
        event_kind,
        plane: namespace.to_string(),
        record_key: logical_key.to_string(),
    });
}

fn seed_owner(
    platform: &StorePlatform,
    memory_space_id: &str,
) -> (LongTermMemoryEntry, bm_core::memory::MemoryFacetIndexDoc) {
    let entry = match plan_long_term_memory_upsert(
        None,
        &preferred_editor_draft("Use the integrated editor for this project.", 1),
        NOW_SECS,
    ) {
        LongTermMemoryEntryPlan::Created(entry) => entry,
        other => panic!("initial preferred-editor fixture must be created, got {other:?}"),
    };
    let facet = build_long_term_memory_facet_index_doc(
        &entry,
        memory_space_id,
        vec!["subject-human".to_string()],
        1,
    );
    let owner_key =
        scoped_long_term_memory_storage_key(memory_space_id, &entry.id).expect("owner key");
    let facet_key =
        scoped_memory_facet_owner_storage_key(memory_space_id, "subject-human", &entry.id)
            .expect("facet key");
    platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: format!("seed-owner-{}", entry.id),
                operation: "test.seed_preferred_editor_owner".to_string(),
                scope: control_scope(memory_space_id),
                mutations: vec![
                    StoreMutation::PutJson {
                        namespace: "long_term".to_string(),
                        key: owner_key.clone(),
                        value: serde_json::to_value(&entry).expect("serialize owner"),
                        event_kind: MemoryStoreEventKind::MemoryWrite,
                        plane: "long_term".to_string(),
                        record_key: entry.id.clone(),
                    },
                    StoreMutation::PutJson {
                        namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                        key: facet_key.clone(),
                        value: serde_json::to_value(&facet).expect("serialize owner facet"),
                        event_kind: MemoryStoreEventKind::MemoryWrite,
                        plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                        record_key: format!("facet-owner:{}", entry.id),
                    },
                ],
            },
            &[
                StoreJsonPrecondition::Absent {
                    namespace: "long_term".to_string(),
                    key: owner_key,
                },
                StoreJsonPrecondition::Absent {
                    namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                    key: facet_key,
                },
            ],
        )
        .expect("seed preferred editor owner");
    (entry, facet)
}

fn seed_control_store(platform: &StorePlatform, memory_space_id: &str) -> ControlFixture {
    let (before, before_facet) = seed_owner(platform, memory_space_id);
    let read_store = platform
        .scoped_long_term_memory_read_store(memory_space_id)
        .expect("scoped owner read store");
    let control_read_store = platform
        .scoped_long_term_memory_control_read_store(memory_space_id)
        .expect("scoped control read store");
    let correct_plan = plan_long_term_memory_control_mutation(
        read_store.as_ref(),
        control_read_store.as_ref(),
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(before.id.clone()),
                replacement: preferred_editor_draft("Use the native editor for this project.", 2),
            },
            reason: "user corrected preference".to_string(),
            dry_run: false,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some(memory_space_id.to_string()),
            now_secs: NOW_SECS + 1,
        },
    )
    .expect("plan correction");
    let after = match correct_plan.owner_writes.as_slice() {
        [LongTermMemoryOwnerWrite::Put(entry)] => (**entry).clone(),
        other => panic!("expected one owner correction write, got {other:?}"),
    };
    let revision = correct_plan
        .control_writes
        .iter()
        .find_map(|write| match write {
            bm_core::memory::LongTermMemoryControlWrite::PutRevision(value) => Some(value.clone()),
            _ => None,
        })
        .expect("planned correction revision");
    let correct_audit = correct_plan
        .control_writes
        .iter()
        .find_map(|write| match write {
            bm_core::memory::LongTermMemoryControlWrite::AppendAudit(value) => Some(value.clone()),
            _ => None,
        })
        .expect("planned correction audit");
    let after_facet = build_long_term_memory_facet_index_doc(
        &after,
        memory_space_id,
        vec!["subject-human".to_string()],
        2,
    );
    let owner_key =
        scoped_long_term_memory_storage_key(memory_space_id, &before.id).expect("owner key");
    let facet_key =
        scoped_memory_facet_owner_storage_key(memory_space_id, "subject-human", &before.id)
            .expect("facet key");
    let mut mutations = Vec::new();
    let mut preconditions = Vec::new();
    mutations.extend([
        StoreMutation::PutJson {
            namespace: "long_term".to_string(),
            key: owner_key.clone(),
            value: serde_json::to_value(&after).expect("serialize corrected owner"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "long_term".to_string(),
            record_key: after.id.clone(),
        },
        StoreMutation::PutJson {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: facet_key.clone(),
            value: serde_json::to_value(&after_facet).expect("serialize corrected facet"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            record_key: format!("facet-owner:{}", after.id),
        },
    ]);
    preconditions.extend([
        StoreJsonPrecondition::Exact {
            namespace: "long_term".to_string(),
            key: owner_key.clone(),
            value: serde_json::to_value(&before).expect("serialize previous owner"),
        },
        StoreJsonPrecondition::Exact {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: facet_key.clone(),
            value: serde_json::to_value(&before_facet).expect("serialize previous facet"),
        },
    ]);
    push_control_seed(
        &mut mutations,
        &mut preconditions,
        memory_space_id,
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        &revision.revision_id,
        &revision,
        MemoryStoreEventKind::MemoryControl,
    );
    push_control_seed(
        &mut mutations,
        &mut preconditions,
        memory_space_id,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        &correct_audit.event_id,
        &correct_audit,
        MemoryStoreEventKind::MemoryControl,
    );
    platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: correct_audit.transaction_id.clone(),
                operation: "test.correct_preferred_editor".to_string(),
                scope: control_scope(memory_space_id),
                mutations,
            },
            &preconditions,
        )
        .expect("commit correction control transaction");

    let read_store = platform
        .scoped_long_term_memory_read_store(memory_space_id)
        .expect("scoped owner read store");
    let control_read_store = platform
        .scoped_long_term_memory_control_read_store(memory_space_id)
        .expect("scoped control read store");
    let delete_plan = plan_long_term_memory_control_mutation(
        read_store.as_ref(),
        control_read_store.as_ref(),
        LongTermMemoryControlMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(after.id.clone()),
            },
            reason: "user deleted memory".to_string(),
            dry_run: false,
            actor_subject_id: Some("subject-human".to_string()),
            memory_space_id: Some(memory_space_id.to_string()),
            now_secs: NOW_SECS + 2,
        },
    )
    .expect("plan deletion");
    let tombstone = delete_plan
        .control_writes
        .iter()
        .find_map(|write| match write {
            bm_core::memory::LongTermMemoryControlWrite::PutTombstone(value) => Some(value.clone()),
            _ => None,
        })
        .expect("planned deletion tombstone");
    let delete_audit = delete_plan
        .control_writes
        .iter()
        .find_map(|write| match write {
            bm_core::memory::LongTermMemoryControlWrite::AppendAudit(value) => Some(value.clone()),
            _ => None,
        })
        .expect("planned deletion audit");
    let mut mutations = vec![
        StoreMutation::DeleteJson {
            namespace: "long_term".to_string(),
            key: owner_key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: "long_term".to_string(),
            record_key: after.id.clone(),
        },
        StoreMutation::DeleteJson {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: facet_key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            record_key: format!("facet-owner:{}", after.id),
        },
    ];
    let mut preconditions = vec![
        StoreJsonPrecondition::Exact {
            namespace: "long_term".to_string(),
            key: owner_key,
            value: serde_json::to_value(&after).expect("serialize deleted owner"),
        },
        StoreJsonPrecondition::Exact {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: facet_key,
            value: serde_json::to_value(&after_facet).expect("serialize deleted facet"),
        },
    ];
    push_control_seed(
        &mut mutations,
        &mut preconditions,
        memory_space_id,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        &tombstone.record_id,
        &tombstone,
        MemoryStoreEventKind::MemoryDelete,
    );
    push_control_seed(
        &mut mutations,
        &mut preconditions,
        memory_space_id,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        &delete_audit.event_id,
        &delete_audit,
        MemoryStoreEventKind::MemoryControl,
    );
    platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: delete_audit.transaction_id.clone(),
                operation: "test.delete_preferred_editor".to_string(),
                scope: control_scope(memory_space_id),
                mutations,
            },
            &preconditions,
        )
        .expect("commit deletion control transaction");

    let policy = policy(memory_space_id);
    let policy_audit = policy_audit(&policy);
    let mut mutations = Vec::new();
    let mut preconditions = Vec::new();
    push_control_seed(
        &mut mutations,
        &mut preconditions,
        memory_space_id,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        &policy.policy_id,
        &policy,
        MemoryStoreEventKind::MemoryPolicy,
    );
    push_control_seed(
        &mut mutations,
        &mut preconditions,
        memory_space_id,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        &policy_audit.event_id,
        &policy_audit,
        MemoryStoreEventKind::MemoryControl,
    );
    platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: policy_audit.transaction_id.clone(),
                operation: "test.suppress_temporary_preferences".to_string(),
                scope: control_scope(memory_space_id),
                mutations,
            },
            &preconditions,
        )
        .expect("commit policy control transaction");

    ControlFixture {
        revision,
        tombstone,
        policy,
        audits: vec![policy_audit, delete_audit, correct_audit],
    }
}

#[test]
fn store_platform_persists_long_term_control_metadata_and_events() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let fixture = seed_control_store(&platform, "space-user");
    let control = platform
        .scoped_long_term_memory_control_read_store("space-user")
        .expect("scoped control read store");

    assert_eq!(
        control
            .list_long_term_control_revisions(&fixture.revision.record_id, 10)
            .unwrap(),
        vec![fixture.revision.clone()]
    );
    assert_eq!(
        control
            .get_long_term_control_tombstone(&fixture.tombstone.record_id)
            .unwrap(),
        Some(fixture.tombstone.clone())
    );
    assert_eq!(
        control.list_long_term_governance_policies(10).unwrap(),
        vec![fixture.policy.clone()]
    );
    assert_eq!(
        control.list_long_term_control_audit(10).unwrap(),
        fixture.audits.clone()
    );

    let events = platform.read_events().unwrap();
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
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let fixture = seed_control_store(&platform, "space-user");
    let control = platform
        .scoped_long_term_memory_control_read_store("space-user")
        .expect("scoped control read store");
    let other_space = platform
        .scoped_long_term_memory_control_read_store("space-other")
        .expect("other scoped control read store");

    assert_eq!(
        control
            .list_long_term_control_revisions(&fixture.revision.record_id, 10)
            .unwrap(),
        vec![fixture.revision.clone()]
    );
    assert!(other_space
        .list_long_term_control_revisions(&fixture.revision.record_id, 10)
        .unwrap()
        .is_empty());
    assert!(other_space
        .get_long_term_control_tombstone(&fixture.tombstone.record_id)
        .unwrap()
        .is_none());

    let record_keys = platform
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
    assert_eq!(
        record_keys,
        BTreeSet::from_iter(
            fixture
                .audits
                .iter()
                .map(|audit| audit.event_id.clone())
                .chain([
                    fixture.tombstone.record_id.clone(),
                    fixture.policy.policy_id.clone(),
                    fixture.revision.revision_id.clone(),
                ]),
        )
    );
}

#[test]
fn snapshot_export_import_preserves_long_term_control_namespaces() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let fixture = seed_control_store(&source, "space-user");

    let snapshot = source.export_store_snapshot().unwrap();
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

    let target = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    target.import_store_snapshot(&snapshot).unwrap();
    let control = target
        .scoped_long_term_memory_control_read_store("space-user")
        .expect("scoped control read store");

    assert_eq!(
        control
            .list_long_term_control_revisions(&fixture.revision.record_id, 10)
            .unwrap(),
        vec![fixture.revision.clone()]
    );
    assert_eq!(
        control
            .get_long_term_control_tombstone(&fixture.tombstone.record_id)
            .unwrap(),
        Some(fixture.tombstone.clone())
    );
    assert_eq!(
        control.list_long_term_governance_policies(10).unwrap(),
        vec![fixture.policy.clone()]
    );
    assert_eq!(
        control.list_long_term_control_audit(10).unwrap(),
        fixture.audits
    );
}
