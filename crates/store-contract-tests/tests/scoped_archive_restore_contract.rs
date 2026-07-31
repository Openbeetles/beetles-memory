mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use bm_core::feature_gate::ProfileId;
use bm_core::skills::RuntimeSkillOwnerRecord;
use bm_sdk::nonproduction_replay_harness::{
    export_memory_space, import_memory_space, StorePhysicalOwningScope, StoreSnapshot,
};
use bm_sdk::{
    GovernedRuntimeSkillWriteInput, GovernedScopeArchiveRootV1, MemoryArchiveScope,
    MemoryCapabilityPolicy, MemoryClock, MemoryIdentity, MemoryPrivacyClass, MemoryPrivacyPolicy,
    MemoryRuntime, MemoryScope, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy, MemoryStoreHandle, MemoryWriteRequest, NoopMemoryAuditSink,
    RuntimeSkillCreationRef, RuntimeSkillOwningScope, RuntimeSkillWrite, RuntimeSkillWriteSource,
    StoreBackendConfig,
};
use sha2::{Digest, Sha256};

const MEMORY_SPACE_ID: &str = "space:archive-owner";
const SUBJECT_A: &str = "subject:archive-a";
const SUBJECT_B: &str = "subject:archive-b";
const SUBJECT_GLOBAL_SOUL_NAMESPACES: &[&str] = &[
    "self_model",
    "self_authored_core",
    "core_revision_ledger",
    "self_continuity",
    "relationship_portfolio",
    "relationship_topology",
    "autonomy_strategy",
    "inner_life",
    "felt_significance",
    "temperament_continuity",
    "inner_conflict",
    "private_doc",
];

struct FixedClock;

impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_000
    }
}

fn runtime(handle: &MemoryStoreHandle, subject_id: &str) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(
            MemoryIdentity::new("scoped-archive-contract", "archive-owner")
                .expect("archive fixture identity"),
        )
        .subject_id(subject_id)
        .scope(MemoryScope::new("contract", "archive").expect("archive fixture scope"))
        .store(handle.clone())
        .clock(Arc::new(FixedClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("archive fixture runtime")
}

fn seed_runtime_skill(runtime: &MemoryRuntime, owning_scope: RuntimeSkillOwningScope, label: &str) {
    let candidate_ref = format!("scoped-archive-contract:{label}");
    let verification_receipt_digest =
        format!("sha256:{:x}", Sha256::digest(candidate_ref.as_bytes()));
    let report = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![GovernedRuntimeSkillWriteInput {
                write: RuntimeSkillWrite {
                    name: label.to_string(),
                    topic: "scoped archive restore".to_string(),
                    title: format!("Scoped archive {label}"),
                    summary: format!("Typed scoped archive fixture {label}."),
                    content: format!(
                        "1. Verify the archive physical owner for {label}.\n\
                         2. Replace only that scope and preserve every sibling scope.\n\
                         3. Reject the restore when the root or owner kind differs."
                    ),
                    citations: vec!["store-contract:scoped-archive".to_string()],
                    source_chat_id: Some("archive".to_string()),
                    observed_at: 1_000,
                },
                creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                    candidate_ref,
                    verification_receipt_digest,
                },
                privacy_class: MemoryPrivacyClass::PublicRuntime,
            }],
            owning_scope,
            source: RuntimeSkillWriteSource::Manual,
        })
        .unwrap_or_else(|error| panic!("seed runtime skill {label}: {error}"));
    assert!(
        report.accepted,
        "fixture write rejected for {label}: {report:#?}"
    );
    assert_eq!(report.changed, 1, "fixture write count for {label}");
}

fn full_snapshot(handle: &MemoryStoreHandle) -> StoreSnapshot {
    handle
        .replay_harness()
        .export_store_snapshot()
        .expect("full-store diagnostic snapshot")
}

fn runtime_skill_docs(
    snapshot: &StoreSnapshot,
    owning_scope: &RuntimeSkillOwningScope,
) -> BTreeMap<String, serde_json::Value> {
    snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "runtime_skill_records")
        .filter_map(|doc| {
            let record =
                serde_json::from_value::<RuntimeSkillOwnerRecord>(doc.value.clone()).ok()?;
            (record.memory_space_id == MEMORY_SPACE_ID && &record.owning_scope == owning_scope)
                .then(|| (doc.key.clone(), doc.value.clone()))
        })
        .collect()
}

fn scoped_event_ids(
    snapshot: &StoreSnapshot,
    physical_scope: &StorePhysicalOwningScope,
) -> BTreeSet<String> {
    snapshot
        .events
        .iter()
        .filter(|event| {
            event.scope.memory_space_id == MEMORY_SPACE_ID
                && &event.scope.physical_owning_scope == physical_scope
        })
        .map(|event| event.event_id.clone())
        .collect()
}

fn subject_global_soul_docs(
    snapshot: &StoreSnapshot,
) -> BTreeMap<(String, String), serde_json::Value> {
    snapshot
        .json_docs
        .iter()
        .filter(|doc| SUBJECT_GLOBAL_SOUL_NAMESPACES.contains(&doc.namespace.as_str()))
        .map(|doc| ((doc.namespace.clone(), doc.key.clone()), doc.value.clone()))
        .collect()
}

fn subject_scope(subject_id: &str) -> RuntimeSkillOwningScope {
    RuntimeSkillOwningScope::Subject {
        mounted_subject_id: subject_id.to_string(),
    }
}

fn subject_event_scope(subject_id: &str) -> StorePhysicalOwningScope {
    StorePhysicalOwningScope::Subject {
        mounted_subject_id: subject_id.to_string(),
    }
}

fn archive_scope_subject(subject_id: &str) -> MemoryArchiveScope {
    MemoryArchiveScope::subject(MEMORY_SPACE_ID, subject_id).expect("subject archive scope")
}

fn export_scope(
    handle: &MemoryStoreHandle,
    scope: MemoryArchiveScope,
) -> bm_sdk::MemorySpaceArchive {
    export_memory_space(
        handle,
        MemorySpaceExportRequest {
            scope,
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        },
    )
    .expect("export typed scoped archive")
    .archive
}

fn backend_config(backend: &str, role: &str, root: &Path) -> StoreBackendConfig {
    match backend {
        "in_memory" => StoreBackendConfig::in_memory(support::native_persistent_profile())
            .expect("in-memory config"),
        "embedded" => {
            StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk).expect("embedded config")
        }
        "file" => StoreBackendConfig::file(
            root.join(format!("{backend}-{role}")),
            support::native_persistent_profile(),
        )
        .expect("file config"),
        "sqlite" => StoreBackendConfig::sqlite(
            root.join(format!("{backend}-{role}.sqlite3")),
            support::native_persistent_profile(),
        )
        .expect("sqlite config"),
        other => panic!("unsupported backend fixture {other}"),
    }
}

fn assert_backend_scoped_archive_restore(backend: &str, root: &Path) {
    let source =
        MemoryStoreHandle::open_for_nonproduction_harness(backend_config(backend, "source", root))
            .unwrap_or_else(|error| panic!("open {backend} source: {error}"));
    let target =
        MemoryStoreHandle::open_for_nonproduction_harness(backend_config(backend, "target", root))
            .unwrap_or_else(|error| panic!("open {backend} target: {error}"));
    let source_a = runtime(&source, SUBJECT_A);
    let target_a = runtime(&target, SUBJECT_A);
    let target_b = runtime(&target, SUBJECT_B);

    seed_runtime_skill(&source_a, subject_scope(SUBJECT_A), "source-subject-a");
    seed_runtime_skill(
        &source_a,
        RuntimeSkillOwningScope::SharedProgram,
        "source-shared",
    );
    seed_runtime_skill(&target_a, subject_scope(SUBJECT_A), "target-old-subject-a");
    seed_runtime_skill(
        &target_a,
        RuntimeSkillOwningScope::SharedProgram,
        "target-old-shared",
    );
    seed_runtime_skill(&target_b, subject_scope(SUBJECT_B), "target-sibling-b");
    for namespace in SUBJECT_GLOBAL_SOUL_NAMESPACES {
        source
            .replay_harness()
            .tamper_json_document_for_nonproduction_harness(
                namespace,
                SUBJECT_A,
                serde_json::json!({"backend": backend, "owner": "source", "namespace": namespace}),
            )
            .unwrap_or_else(|error| panic!("seed {backend} source Soul {namespace}: {error}"));
        target
            .replay_harness()
            .tamper_json_document_for_nonproduction_harness(
                namespace,
                SUBJECT_A,
                serde_json::json!({"backend": backend, "owner": "target", "namespace": namespace}),
            )
            .unwrap_or_else(|error| panic!("seed {backend} target Soul {namespace}: {error}"));
    }

    let source_snapshot = full_snapshot(&source);
    let target_before = full_snapshot(&target);
    let subject_archive = export_scope(&source, archive_scope_subject(SUBJECT_A));
    for namespace in SUBJECT_GLOBAL_SOUL_NAMESPACES {
        assert!(
            !subject_archive.contains_json_namespace(namespace),
            "{backend}: subject-global Soul namespace {namespace} must not enter a memory-space archive"
        );
    }
    let shared_archive = export_scope(
        &source,
        MemoryArchiveScope::shared_program(MEMORY_SPACE_ID).expect("shared archive scope"),
    );

    import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: archive_scope_subject(SUBJECT_A),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: subject_archive.clone(),
        },
    )
    .unwrap_or_else(|error| panic!("{backend} Subject replace: {error}"));
    let after_subject = full_snapshot(&target);
    assert_eq!(
        runtime_skill_docs(&after_subject, &subject_scope(SUBJECT_A)),
        runtime_skill_docs(&source_snapshot, &subject_scope(SUBJECT_A)),
        "{backend}: Subject owner replacement"
    );
    assert_eq!(
        runtime_skill_docs(&after_subject, &RuntimeSkillOwningScope::SharedProgram),
        runtime_skill_docs(&target_before, &RuntimeSkillOwningScope::SharedProgram),
        "{backend}: Subject replace must preserve SharedProgram"
    );
    assert_eq!(
        runtime_skill_docs(&after_subject, &subject_scope(SUBJECT_B)),
        runtime_skill_docs(&target_before, &subject_scope(SUBJECT_B)),
        "{backend}: Subject replace must preserve sibling Subject"
    );
    assert_eq!(
        scoped_event_ids(&after_subject, &subject_event_scope(SUBJECT_A)),
        scoped_event_ids(&source_snapshot, &subject_event_scope(SUBJECT_A)),
        "{backend}: Subject events"
    );
    assert_eq!(
        scoped_event_ids(&after_subject, &StorePhysicalOwningScope::SharedProgram),
        scoped_event_ids(&target_before, &StorePhysicalOwningScope::SharedProgram),
        "{backend}: Subject replace must not cross into SharedProgram events"
    );
    assert_eq!(
        subject_global_soul_docs(&after_subject),
        subject_global_soul_docs(&target_before),
        "{backend}: Subject replace must preserve every subject-global Soul/private owner"
    );

    import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: MemoryArchiveScope::shared_program(MEMORY_SPACE_ID)
                .expect("shared import scope"),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: shared_archive,
        },
    )
    .unwrap_or_else(|error| panic!("{backend} SharedProgram replace: {error}"));
    let after_shared = full_snapshot(&target);
    assert_eq!(
        runtime_skill_docs(&after_shared, &RuntimeSkillOwningScope::SharedProgram),
        runtime_skill_docs(&source_snapshot, &RuntimeSkillOwningScope::SharedProgram),
        "{backend}: SharedProgram owner replacement"
    );
    assert_eq!(
        runtime_skill_docs(&after_shared, &subject_scope(SUBJECT_A)),
        runtime_skill_docs(&after_subject, &subject_scope(SUBJECT_A)),
        "{backend}: SharedProgram replace must preserve Subject"
    );
    assert_eq!(
        runtime_skill_docs(&after_shared, &subject_scope(SUBJECT_B)),
        runtime_skill_docs(&target_before, &subject_scope(SUBJECT_B)),
        "{backend}: SharedProgram replace must preserve sibling Subject"
    );
    assert_eq!(
        scoped_event_ids(&after_shared, &StorePhysicalOwningScope::SharedProgram),
        scoped_event_ids(&source_snapshot, &StorePhysicalOwningScope::SharedProgram),
        "{backend}: SharedProgram events"
    );
    assert_eq!(
        scoped_event_ids(&after_shared, &subject_event_scope(SUBJECT_A)),
        scoped_event_ids(&after_subject, &subject_event_scope(SUBJECT_A)),
        "{backend}: SharedProgram replace must not cross into Subject events"
    );

    let before_forged = full_snapshot(&target);
    let forged_root = GovernedScopeArchiveRootV1::build(
        archive_scope_subject(SUBJECT_A),
        MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        [],
    )
    .expect("structurally valid root with a missing payload closure");
    let forged_archive = subject_archive.with_replaced_root_for_nonproduction_harness(forged_root);
    let forged_error = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: archive_scope_subject(SUBJECT_A),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: forged_archive,
        },
    )
    .expect_err("forged or missing root closure must fail");
    assert!(
        forged_error
            .to_string()
            .contains("archive root does not match its canonical payload closure"),
        "{backend}: unexpected forged-root error: {forged_error}"
    );
    assert_eq!(
        full_snapshot(&target),
        before_forged,
        "{backend}: forged root must be full-store atomic"
    );

    let before_cross_kind = full_snapshot(&target);
    let cross_kind_error = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: MemoryArchiveScope::shared_program(MEMORY_SPACE_ID)
                .expect("cross-kind request scope"),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: subject_archive,
        },
    )
    .expect_err("Subject archive must not restore as SharedProgram");
    assert_eq!(
        cross_kind_error.stage(),
        "memory_archive_scope",
        "{backend}: cross-kind stage"
    );
    assert_eq!(
        full_snapshot(&target),
        before_cross_kind,
        "{backend}: cross-kind rejection must be full-store atomic"
    );
}

#[test]
fn scoped_archive_restore_is_exact_and_atomic_across_all_backends() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-scoped-archive-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    for backend in ["in_memory", "embedded", "file"] {
        assert_backend_scoped_archive_restore(backend, &root);
    }
    #[cfg(feature = "sqlite-store")]
    assert_backend_scoped_archive_restore("sqlite", &root);
    let _ = std::fs::remove_dir_all(root);
}
