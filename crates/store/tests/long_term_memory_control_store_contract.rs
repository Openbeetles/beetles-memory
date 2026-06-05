use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    LongTermMemoryControlAuditEvent, LongTermMemoryControlRevision, LongTermMemoryControlStore,
    LongTermMemoryKind, LongTermMemorySourceScope, LongTermMemoryTombstone,
    MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration, MemoryLongTermGovernancePolicy,
    LONG_TERM_CONTROL_AUDIT_NAMESPACE, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE, LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
};
use bm_core::platform::Platform;
use bm_store::{StoreBackendConfig, StorePlatform};

const NOW_SECS: u64 = 1_780_010_000;

fn revision() -> LongTermMemoryControlRevision {
    LongTermMemoryControlRevision {
        revision_id: "revision-preferred-editor-2".to_string(),
        record_id: "ltm-preferred-editor".to_string(),
        operation: "correct".to_string(),
        revision: 2,
        previous_digest: "old-digest".to_string(),
        new_digest: "new-digest".to_string(),
        reason: "user corrected preference".to_string(),
        actor_subject_id: Some("subject-human".to_string()),
        memory_space_id: Some("space-user".to_string()),
        created_at: NOW_SECS,
    }
}

fn tombstone() -> LongTermMemoryTombstone {
    LongTermMemoryTombstone {
        tombstone_id: "tombstone-preferred-editor".to_string(),
        record_id: "ltm-preferred-editor".to_string(),
        operation: "delete".to_string(),
        previous_digest: "old-digest".to_string(),
        reason: "user deleted memory".to_string(),
        actor_subject_id: Some("subject-human".to_string()),
        memory_space_id: Some("space-user".to_string()),
        created_at: NOW_SECS + 1,
    }
}

fn policy() -> MemoryLongTermGovernancePolicy {
    MemoryLongTermGovernancePolicy {
        policy_id: "policy-temp-preferences".to_string(),
        kind: "suppress".to_string(),
        selector: MemoryGovernanceSelector {
            memory_space_id: Some("space-user".to_string()),
            subject_id: Some("agent:assistant-main".to_string()),
            kind: Some(LongTermMemoryKind::Preference),
            topic_pattern: Some("temporary-*".to_string()),
            source_chat_id: None,
            source_scope: Some(LongTermMemorySourceScope::User),
        },
        duration: Some(MemoryGovernanceSuppressionDuration::UntilManualResume),
        expires_at: None,
        reason: "user does not want temporary preferences remembered".to_string(),
        created_at: NOW_SECS + 2,
        updated_at: NOW_SECS + 2,
    }
}

fn audit() -> LongTermMemoryControlAuditEvent {
    LongTermMemoryControlAuditEvent {
        event_id: "audit-long-term-control".to_string(),
        operation: "memory.control".to_string(),
        record_ids: vec!["ltm-preferred-editor".to_string()],
        policy_ids: vec!["policy-temp-preferences".to_string()],
        reason: "operator-visible control event".to_string(),
        actor_subject_id: Some("subject-human".to_string()),
        memory_space_id: Some("space-user".to_string()),
        created_at: NOW_SECS + 3,
    }
}

fn seed_control_store(store: &dyn LongTermMemoryControlStore) {
    store
        .put_long_term_control_revision(&revision())
        .expect("put revision");
    store
        .put_long_term_control_tombstone(&tombstone())
        .expect("put tombstone");
    store
        .put_long_term_governance_policy(&policy())
        .expect("put policy");
    store
        .put_long_term_control_audit(&audit())
        .expect("put audit");
}

#[test]
fn store_platform_persists_long_term_control_metadata_and_events() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let control = platform.long_term_memory_control_store();
    seed_control_store(control.as_ref());

    assert_eq!(
        control
            .list_long_term_control_revisions("ltm-preferred-editor", 10)
            .unwrap(),
        vec![revision()]
    );
    assert_eq!(
        control
            .get_long_term_control_tombstone("ltm-preferred-editor")
            .unwrap(),
        Some(tombstone())
    );
    assert_eq!(
        control.list_long_term_governance_policies(10).unwrap(),
        vec![policy()]
    );
    assert_eq!(
        control.list_long_term_control_audit(10).unwrap(),
        vec![audit()]
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
fn snapshot_export_import_preserves_long_term_control_namespaces() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    seed_control_store(source.long_term_memory_control_store().as_ref());

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
    let control = target.long_term_memory_control_store();

    assert_eq!(
        control
            .list_long_term_control_revisions("ltm-preferred-editor", 10)
            .unwrap(),
        vec![revision()]
    );
    assert_eq!(
        control
            .get_long_term_control_tombstone("ltm-preferred-editor")
            .unwrap(),
        Some(tombstone())
    );
    assert_eq!(
        control.list_long_term_governance_policies(10).unwrap(),
        vec![policy()]
    );
    assert_eq!(
        control.list_long_term_control_audit(10).unwrap(),
        vec![audit()]
    );
}
