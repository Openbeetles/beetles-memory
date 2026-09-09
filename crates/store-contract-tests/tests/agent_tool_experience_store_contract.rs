mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    MemoryMutationAuditRecord, MemoryPrivacyClass, MEMORY_MUTATION_AUDIT_NAMESPACE,
    MEMORY_MUTATION_RECEIPT_NAMESPACE,
};
use bm_core::skills::{
    AgentToolExperienceConfidence, AgentToolExperienceHeadBindingV1,
    AgentToolExperienceOwnerHeadV2, AgentToolExperienceOwningScopeV1,
    AgentToolExperienceRetainedRevisionDigestV2, AgentToolExperienceRevisionMaterialV2,
    AgentToolExperienceScopeManifestV1, AgentToolExperienceStatus, AgentToolOutcome,
    AgentToolRegistryScope,
};
use bm_sdk::nonproduction_replay_harness::{
    AgentToolExperienceStoreMutationOutcomeV1, AgentToolExperienceStoreMutationPlanV1,
    MemoryStoreEventKind, StoreBackendConfig, StoreEventScope, StoreMutation, StoreMutationBatch,
    StorePlatform, AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
    AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct ExperienceRevisionFixture {
    plan: AgentToolExperienceStoreMutationPlanV1,
    material: AgentToolExperienceRevisionMaterialV2,
    head: AgentToolExperienceOwnerHeadV2,
    manifest: AgentToolExperienceScopeManifestV1,
}

fn temp_root(backend: &str, scenario: &str) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "beetle-memory-{backend}-agent-tool-experience-{scenario}-{}-{sequence}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::remove_file(&path);
    path
}

fn owning_scope(subject: &str) -> AgentToolExperienceOwningScopeV1 {
    AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: subject.to_string(),
    }
}

fn event_scope(subject: &str) -> StoreEventScope {
    StoreEventScope::new(subject, "owner-a", "test", "chat-a")
        .with_memory_space("space:test")
        .with_subject(subject)
}

#[allow(clippy::too_many_arguments)]
fn revision_fixture(
    subject: &str,
    operation_id: &str,
    task_signature: &str,
    revision: u64,
    previous_material: Option<&AgentToolExperienceRevisionMaterialV2>,
    previous_head: Option<&AgentToolExperienceOwnerHeadV2>,
    previous_manifest: Option<&AgentToolExperienceScopeManifestV1>,
    evidence_refs: Vec<String>,
) -> ExperienceRevisionFixture {
    let predecessor = previous_material
        .map(AgentToolExperienceRetainedRevisionDigestV2::from_material)
        .transpose()
        .expect("canonical predecessor");
    let material = AgentToolExperienceRevisionMaterialV2::build(
        "space:test",
        owning_scope(subject),
        "host-tools",
        AgentToolRegistryScope::Project {
            project_id: "project-a".to_string(),
        },
        "pdf.extract",
        "schema-pdf-v1",
        task_signature,
        revision,
        "PDF release-note extraction",
        &format!("Use governed extraction revision {revision}."),
        vec!["filesystem.read".to_string()],
        u32::try_from(evidence_refs.len()).expect("evidence count"),
        u32::try_from(evidence_refs.len()).expect("success count"),
        0,
        AgentToolOutcome::Succeeded,
        AgentToolExperienceConfidence::High,
        AgentToolExperienceStatus::Active,
        evidence_refs,
        MemoryPrivacyClass::SharedWithSubject,
        100,
        100 + revision,
        predecessor,
    )
    .expect("canonical material");
    let mut retained = previous_head
        .map(|head| head.retained_revisions.clone())
        .unwrap_or_default();
    retained.push(
        AgentToolExperienceRetainedRevisionDigestV2::from_material(&material)
            .expect("retained material"),
    );
    let head = AgentToolExperienceOwnerHeadV2::build(
        "space:test",
        owning_scope(subject),
        material.owner_ref.clone(),
        revision,
        retained,
    )
    .expect("canonical head");
    let binding = AgentToolExperienceHeadBindingV1::from_head_and_material(&head, &material)
        .expect("head binding");
    let mut bindings = previous_manifest
        .map(|manifest| manifest.bindings.clone())
        .unwrap_or_default();
    bindings.retain(|candidate| candidate.owner_ref != head.owner_ref);
    bindings.push(binding);
    let manifest = AgentToolExperienceScopeManifestV1::build(
        previous_manifest.map_or(1, |value| value.revision + 1),
        "space:test",
        owning_scope(subject),
        bindings,
        1024,
    )
    .expect("canonical manifest");
    let plan = AgentToolExperienceStoreMutationPlanV1::try_new(
        operation_id,
        subject,
        event_scope(subject),
        previous_head.cloned(),
        previous_manifest.cloned(),
        material.clone(),
        head.clone(),
        manifest.clone(),
        100 + revision,
    )
    .expect("canonical Store mutation plan");
    ExperienceRevisionFixture {
        plan,
        material,
        head,
        manifest,
    }
}

fn in_memory_store() -> StorePlatform {
    support::open_store(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("store config"),
    )
    .expect("open store")
}

fn assert_authoritative_operation_closure(
    platform: &StorePlatform,
    outcome: &AgentToolExperienceStoreMutationOutcomeV1,
) {
    let receipt = match outcome {
        AgentToolExperienceStoreMutationOutcomeV1::Committed { receipt, .. }
        | AgentToolExperienceStoreMutationOutcomeV1::Replayed { receipt } => receipt,
    };
    let key = receipt.identity.storage_key();
    let receipt_docs = platform
        .read_json_docs_by_keys(
            MEMORY_MUTATION_RECEIPT_NAMESPACE,
            std::slice::from_ref(&key),
        )
        .expect("read receipt");
    let audit_docs = platform
        .read_json_docs_by_keys(MEMORY_MUTATION_AUDIT_NAMESPACE, std::slice::from_ref(&key))
        .expect("read audit");
    assert_eq!(receipt_docs.len(), 1);
    assert_eq!(audit_docs.len(), 1);
    let audit: MemoryMutationAuditRecord =
        serde_json::from_value(audit_docs[0].value.clone()).expect("decode authoritative audit");
    assert_eq!(audit.identity, receipt.identity);
    assert_eq!(audit.intent_digest, receipt.intent_digest);
    assert_eq!(audit.transaction_id, receipt.transaction_id);
    assert_eq!(audit.changed_count, 3);
}

fn json_projection(platform: &StorePlatform) -> Vec<(String, String, serde_json::Value)> {
    platform
        .export_store_snapshot()
        .expect("snapshot")
        .json_docs
        .into_iter()
        .map(|doc| (doc.namespace, doc.key, doc.value))
        .collect()
}

#[test]
fn store_schema_v13_is_the_only_current_schema() {
    assert_eq!(STORE_SCHEMA_ID, "beetle_memory_store_schema_v13");
    assert_eq!(STORE_SCHEMA_VERSION, 13);
}

#[test]
fn legacy_unscoped_agent_tool_experience_blob_is_forbidden() {
    let platform = in_memory_store();
    let key = "agent_tool_experience__legacy";
    let batch = StoreMutationBatch {
        transaction_id: "txn-forbidden-legacy-agent-tool-experience".to_string(),
        operation: "test.forbidden_legacy_agent_tool_experience".to_string(),
        scope: event_scope("subject:agent-a"),
        mutations: vec![StoreMutation::PutBlob {
            namespace: "skills".to_string(),
            key: key.to_string(),
            value: br#"{"experience_id":"legacy"}"#.to_vec(),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "skills".to_string(),
            record_key: key.to_string(),
        }],
    };
    let before = platform.export_store_snapshot().expect("snapshot before");
    let error = platform
        .commit_governed_memory_transaction(batch)
        .expect_err("legacy unscoped Agent Tool experience must fail closed");
    assert!(!error.to_string().is_empty());
    assert_eq!(
        platform.export_store_snapshot().expect("snapshot after"),
        before
    );
}

#[test]
fn dedicated_owner_documents_reject_primitive_store_writes_without_operation_authority() {
    let platform = in_memory_store();
    let fixture = revision_fixture(
        "subject:agent-a",
        "primitive-write-fixture",
        "extract_pdf_text",
        1,
        None,
        None,
        None,
        vec!["evidence-1".to_string()],
    );
    let batch = StoreMutationBatch {
        transaction_id: "primitive-agent-tool-experience-write".to_string(),
        operation: "test.primitive_agent_tool_experience_write".to_string(),
        scope: event_scope("subject:agent-a"),
        mutations: vec![StoreMutation::PutJson {
            namespace: AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.to_string(),
            key: fixture.material.physical_key.clone(),
            value: serde_json::to_value(&fixture.material).expect("material JSON"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "agent_tool_experience".to_string(),
            record_key: fixture.material.physical_key,
        }],
    };
    let before = platform
        .export_store_snapshot()
        .expect("before primitive write");
    let error = platform
        .commit_governed_memory_transaction(batch)
        .expect_err("primitive Store write must not bypass the typed operation owner");
    assert!(!error.to_string().is_empty());
    assert_eq!(
        platform
            .export_store_snapshot()
            .expect("after primitive write"),
        before
    );
}

#[test]
fn typed_plan_rejects_cross_subject_scope_before_store_mutation() {
    let fixture = revision_fixture(
        "subject:agent-b",
        "cross-subject-source",
        "extract_pdf_text",
        1,
        None,
        None,
        None,
        vec!["evidence-1".to_string()],
    );
    let error = AgentToolExperienceStoreMutationPlanV1::try_new(
        "cross-subject-operation",
        "subject:agent-a",
        event_scope("subject:agent-a"),
        None,
        None,
        fixture.material,
        fixture.head,
        fixture.manifest,
        101,
    )
    .expect_err("subject-a operation must not write subject-b owner closure");
    assert!(error.to_string().contains("scope"));
}

#[test]
fn operation_commit_is_atomic_durable_and_idempotent_with_stale_cas_zero_change() {
    let platform = in_memory_store();
    let first = revision_fixture(
        "subject:agent-a",
        "experience-operation-1",
        "extract_pdf_text",
        1,
        None,
        None,
        None,
        vec!["evidence-1".to_string()],
    );
    let outcome = platform
        .commit_agent_tool_experience_for_nonproduction_harness(first.plan.clone())
        .expect("commit first revision");
    let AgentToolExperienceStoreMutationOutcomeV1::Committed { report, .. } = &outcome else {
        panic!("first operation must commit")
    };
    assert_eq!(
        report.changed_json, 5,
        "three owner docs plus audit and receipt"
    );
    assert_eq!(
        report.events, 5,
        "every atomic JSON write must emit one event"
    );
    assert_authoritative_operation_closure(&platform, &outcome);

    let after_first = platform.export_store_snapshot().expect("after first");
    let replay = platform
        .commit_agent_tool_experience_for_nonproduction_harness(first.plan.clone())
        .expect("replay exact operation");
    assert!(matches!(
        replay,
        AgentToolExperienceStoreMutationOutcomeV1::Replayed { .. }
    ));
    assert_eq!(
        platform.export_store_snapshot().expect("after replay"),
        after_first
    );

    let competing_a = revision_fixture(
        "subject:agent-a",
        "experience-operation-2a",
        "extract_pdf_text",
        2,
        Some(&first.material),
        Some(&first.head),
        Some(&first.manifest),
        vec!["evidence-1".to_string(), "evidence-2a".to_string()],
    );
    let competing_b = revision_fixture(
        "subject:agent-a",
        "experience-operation-2b",
        "extract_pdf_text",
        2,
        Some(&first.material),
        Some(&first.head),
        Some(&first.manifest),
        vec!["evidence-1".to_string(), "evidence-2b".to_string()],
    );
    platform
        .commit_agent_tool_experience_for_nonproduction_harness(competing_a.plan)
        .expect("commit winning revision");
    let after_winner = platform.export_store_snapshot().expect("after winner");
    let error = platform
        .commit_agent_tool_experience_for_nonproduction_harness(competing_b.plan)
        .expect_err("stale revision must conflict");
    assert!(error.to_string().contains("precondition"));
    assert_eq!(
        platform.export_store_snapshot().expect("after stale"),
        after_winner
    );
}

#[test]
fn subject_scopes_persist_as_distinct_physical_owner_closures() {
    let platform = in_memory_store();
    for (subject, operation) in [
        ("subject:agent-a", "subject-a-operation"),
        ("subject:agent-b", "subject-b-operation"),
    ] {
        let fixture = revision_fixture(
            subject,
            operation,
            "extract_pdf_text",
            1,
            None,
            None,
            None,
            vec![format!("evidence-{subject}")],
        );
        platform
            .commit_agent_tool_experience_for_nonproduction_harness(fixture.plan)
            .expect("commit subject owner");
    }
    let snapshot = platform.export_store_snapshot().expect("snapshot");
    for namespace in [
        AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
        AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
        AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
    ] {
        let docs = snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == namespace)
            .collect::<Vec<_>>();
        assert_eq!(
            docs.len(),
            2,
            "{namespace} must retain both positive controls"
        );
        let subjects = docs
            .iter()
            .map(|doc| {
                doc.value["owning_scope"]["mounted_subject_id"]
                    .as_str()
                    .unwrap()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            subjects,
            ["subject:agent-a", "subject:agent-b"].into_iter().collect()
        );
    }
}

#[test]
fn evidence_capacity_rejection_is_exact_zero_change() {
    let platform = in_memory_store();
    let limits = platform.agent_tool_experience_limits_for_nonproduction_harness();
    let evidence_refs = (0..=limits.max_evidence_refs_per_owner)
        .map(|index| format!("evidence-{index:04}"))
        .collect();
    let oversized = revision_fixture(
        "subject:agent-a",
        "oversized-evidence-operation",
        "extract_pdf_text",
        1,
        None,
        None,
        None,
        evidence_refs,
    );
    let before = platform.export_store_snapshot().expect("before overflow");
    let error = platform
        .commit_agent_tool_experience_for_nonproduction_harness(oversized.plan)
        .expect_err("evidence footprint must exceed the governed profile");
    assert!(error.to_string().contains("material"));
    assert_eq!(
        platform.export_store_snapshot().expect("after overflow"),
        before
    );
}

#[test]
fn snapshot_import_rejects_orphaned_owner_closure() {
    let source = in_memory_store();
    let fixture = revision_fixture(
        "subject:agent-a",
        "snapshot-operation",
        "extract_pdf_text",
        1,
        None,
        None,
        None,
        vec!["evidence-1".to_string()],
    );
    source
        .commit_agent_tool_experience_for_nonproduction_harness(fixture.plan)
        .expect("seed valid closure");
    let mut corrupted = source.export_store_snapshot().expect("valid snapshot");
    corrupted
        .json_docs
        .retain(|doc| doc.namespace != AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE);
    let target = in_memory_store();
    let before = target.export_store_snapshot().expect("target before");
    let error = target
        .import_store_snapshot(&corrupted)
        .expect_err("orphaned snapshot must fail closed");
    assert!(error.to_string().contains("manifest"));
    assert_eq!(
        target.export_store_snapshot().expect("target after"),
        before
    );
}

#[test]
fn snapshot_import_rejects_tampered_material_digest() {
    let source = in_memory_store();
    let fixture = revision_fixture(
        "subject:agent-a",
        "snapshot-digest-operation",
        "extract_pdf_text",
        1,
        None,
        None,
        None,
        vec!["evidence-1".to_string()],
    );
    source
        .commit_agent_tool_experience_for_nonproduction_harness(fixture.plan)
        .expect("seed valid closure");
    let mut corrupted = source.export_store_snapshot().expect("valid snapshot");
    let material = corrupted
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
        .expect("persisted material");
    material.value["usage_guidance"] = serde_json::json!("tampered guidance");
    let target = in_memory_store();
    let before = target.export_store_snapshot().expect("target before");
    let error = target
        .import_store_snapshot(&corrupted)
        .expect_err("digest-tampered snapshot must fail closed");
    assert!(!error.to_string().is_empty());
    assert_eq!(
        target.export_store_snapshot().expect("target after"),
        before
    );
}

#[test]
fn file_reopen_preserves_closure_and_detects_missing_manifest() {
    let root = temp_root("file", "reopen");
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    let expected;
    let manifest_key;
    {
        let platform = support::open_store(config.clone()).expect("open file store");
        let fixture = revision_fixture(
            "subject:agent-a",
            "file-operation",
            "extract_pdf_text",
            1,
            None,
            None,
            None,
            vec!["evidence-1".to_string()],
        );
        manifest_key = fixture.manifest.physical_key.clone();
        platform
            .commit_agent_tool_experience_for_nonproduction_harness(fixture.plan)
            .expect("commit file owner");
        expected = json_projection(&platform);
    }
    {
        let reopened = support::open_store(config.clone()).expect("reopen valid file closure");
        assert_eq!(json_projection(&reopened), expected);
        reopened
            .delete_json_document_for_nonproduction_harness(
                AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
                &manifest_key,
            )
            .expect("inject missing manifest corruption");
    }
    let error = support::open_store(config)
        .err()
        .expect("corrupt file reopen must fail closed");
    assert!(error.to_string().contains("manifest"));
    std::fs::remove_dir_all(root).expect("remove file fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_closure_and_detects_missing_manifest() {
    let path = temp_root("sqlite", "reopen");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    let expected;
    let manifest_key;
    {
        let platform = support::open_store(config.clone()).expect("open sqlite store");
        let fixture = revision_fixture(
            "subject:agent-a",
            "sqlite-operation",
            "extract_pdf_text",
            1,
            None,
            None,
            None,
            vec!["evidence-1".to_string()],
        );
        manifest_key = fixture.manifest.physical_key.clone();
        platform
            .commit_agent_tool_experience_for_nonproduction_harness(fixture.plan)
            .expect("commit sqlite owner");
        expected = json_projection(&platform);
    }
    {
        let reopened = support::open_store(config.clone()).expect("reopen valid sqlite closure");
        assert_eq!(json_projection(&reopened), expected);
        reopened
            .delete_json_document_for_nonproduction_harness(
                AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
                &manifest_key,
            )
            .expect("inject missing manifest corruption");
    }
    let error = support::open_store(config)
        .err()
        .expect("corrupt sqlite reopen must fail closed");
    assert!(error.to_string().contains("manifest"));
    std::fs::remove_file(path).expect("remove sqlite fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_concurrent_revision_cas_has_one_winner_and_no_partial_loser() {
    let path = temp_root("sqlite", "concurrent-cas");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    let first = revision_fixture(
        "subject:agent-a",
        "sqlite-concurrent-revision-1",
        "extract_pdf_text",
        1,
        None,
        None,
        None,
        vec!["evidence-1".to_string()],
    );
    {
        let platform = support::open_store(config.clone()).expect("open seed sqlite store");
        platform
            .commit_agent_tool_experience_for_nonproduction_harness(first.plan.clone())
            .expect("commit first revision");
    }
    let left = revision_fixture(
        "subject:agent-a",
        "sqlite-concurrent-revision-2-left",
        "extract_pdf_text",
        2,
        Some(&first.material),
        Some(&first.head),
        Some(&first.manifest),
        vec!["evidence-1".to_string(), "evidence-left".to_string()],
    );
    let right = revision_fixture(
        "subject:agent-a",
        "sqlite-concurrent-revision-2-right",
        "extract_pdf_text",
        2,
        Some(&first.material),
        Some(&first.head),
        Some(&first.manifest),
        vec!["evidence-1".to_string(), "evidence-right".to_string()],
    );
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let run = |plan: AgentToolExperienceStoreMutationPlanV1,
               config: StoreBackendConfig,
               barrier: std::sync::Arc<std::sync::Barrier>| {
        std::thread::spawn(move || {
            let platform = support::open_store(config).expect("open concurrent sqlite store");
            barrier.wait();
            platform.commit_agent_tool_experience_for_nonproduction_harness(plan)
        })
    };
    let left_thread = run(left.plan, config.clone(), barrier.clone());
    let right_thread = run(right.plan, config.clone(), barrier);
    let outcomes = [
        left_thread.join().expect("left thread"),
        right_thread.join().expect("right thread"),
    ];
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
    assert!(outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .all(|error| error.to_string().contains("precondition")));

    let reopened = support::open_store(config).expect("reopen concurrent sqlite store");
    let snapshot = reopened
        .export_store_snapshot()
        .expect("concurrent snapshot");
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
            .count(),
        2
    );
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == MEMORY_MUTATION_RECEIPT_NAMESPACE)
            .count(),
        2,
        "losing revision must not leave a receipt"
    );
    std::fs::remove_file(path).expect("remove sqlite fixture");
}
