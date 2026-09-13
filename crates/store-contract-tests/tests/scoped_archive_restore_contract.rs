mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    MemoryMutationAuditRecord, MemoryMutationOperationKind, MemoryMutationReceipt,
    ProceduralSourceDependentsRootV1,
};
use bm_core::skills::RuntimeSkillOwnerRecord;
use bm_sdk::nonproduction_replay_harness::{
    export_memory_space, import_memory_space, StorePhysicalOwningScope, StoreSnapshot,
};
use bm_sdk::{
    primary_human_subject_id, GovernedRuntimeSkillWriteInput, GovernedScopeArchiveRootV1,
    MemoryArchiveScope, MemoryCapabilityPolicy, MemoryClock, MemoryIdentity, MemoryPrivacyClass,
    MemoryPrivacyPolicy, MemoryRuntime, MemoryScope, MemorySpaceExportRequest,
    MemorySpaceImportRequest, MemorySpacePrivateMaterialPolicy, MemoryStoreHandle,
    NoopMemoryAuditSink, RuntimeSkillCreationRef, RuntimeSkillOwningScope, RuntimeSkillWrite,
    StoreBackendConfig, SubjectSoulFoundingCharterSeedV1, SubjectSoulMutationOutcomeV1,
    SubjectSoulProvisionIntentV1,
};
use sha2::{Digest, Sha256};

const MEMORY_SPACE_ID: &str = "space:archive-owner";
const SUBJECT_A: &str = "subject:archive-a";
const SUBJECT_B: &str = "subject:archive-b";
const SUBJECT_GLOBAL_SOUL_NAMESPACES: &[&str] = &[
    "subject_soul_lifecycle_heads",
    "subject_soul_revision_materials",
    "subject_soul_scope_manifests",
    "subject_soul_generation_tombstones",
    "subject_soul_relationship_projections",
    "subject_soul_operation_results",
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
        .seed_runtime_skills_for_replay(
            vec![GovernedRuntimeSkillWriteInput {
                write: RuntimeSkillWrite {
                    name: label.to_string(),
                    topic: "scoped archive restore".to_string(),
                    title: format!("Scoped archive {label}"),
                    summary: format!("Typed scoped archive fixture {label}."),
                    content: format!(
                        "1. Verify exact owner scope for {label}.\n\
                         2. Replace only that scope.\n\
                         3. Preserve all sibling scopes."
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
        )
        .unwrap_or_else(|error| panic!("seed runtime skill {label}: {error}"));
    assert!(
        report.accepted,
        "fixture write rejected for {label}: {report:#?}"
    );
    assert_eq!(report.changed, 1, "fixture write count for {label}");
}

fn seed_subject_soul(runtime: &MemoryRuntime, label: &str) {
    let charter = SubjectSoulFoundingCharterSeedV1 {
        identity_anchor: Some(format!("typed scoped archive Soul {label}")),
        character_tendencies: vec![],
        priority_constitution: vec![],
        non_negotiables: vec![],
        default_response_mode: None,
        default_initiative_posture: None,
        default_relationship_posture: None,
        boundary_doctrine: None,
        truth_seeking_commitment: None,
        self_preservation_doctrine: None,
        repair_doctrine: None,
        change_principle: None,
    }
    .canonicalize()
    .expect("canonical archive Soul seed");
    let report = runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: format!("scoped-archive-soul:{label}"),
            human_actor_subject_id: primary_human_subject_id("archive-owner"),
            charter: Box::new(charter),
            source_asserted_at: Some(1_000),
        })
        .unwrap_or_else(|error| panic!("seed typed Subject Soul {label}: {error}"));
    assert_eq!(report.outcome, SubjectSoulMutationOutcomeV1::Committed);
}

fn seed_archive_payload(runtime: &MemoryRuntime, label: &str, factual_root: bool) {
    use bm_sdk::*;
    let body = format!("Canonical archive payload for {label}.");
    let request = if factual_root {
        let target = MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Project,
            topic: label.into(),
        };
        MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: label.into(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: target.clone(),
                long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
                privacy: MemoryPrivacyClass::PublicRuntime,
                content: MemoryCandidateContent::Text {
                    topic: label.into(),
                    body,
                    keywords: vec![],
                },
                evidence_refs: vec![],
                canonical_entities: vec![],
                semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                    source: MemorySemanticJudgmentSource::RuntimeGate,
                    decision: MemoryCandidateSemanticDecision::Accept,
                    governed_target: Some(target),
                    reason: "canonical archive fixture".into(),
                }),
            }],
        }
    } else {
        let group = bm_core::memory::canonical_recall_evidence_group(label);
        let chunks = vec![GovernedEvidenceDocumentChunk {
            identity: "body".into(),
            ordinal: 0,
            body: body.clone(),
        }];
        MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: Box::new(GovernedEvidenceDocumentDraft {
                    memory_space_id: runtime.memory_space_id().into(),
                    mounted_subject_id: runtime.subject_id().into(),
                    document_id: label.into(),
                    source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
                    source_locator: label.into(),
                    canonical_evidence_group: group.clone(),
                    evidence_family_group: None,
                    source_revision: 1,
                    content_digest: governed_evidence_document_content_digest(
                        label, &group, None, &body, &chunks,
                    ),
                    body,
                    chunks,
                    authority: MemoryEvidenceAuthority::UserAsserted,
                    privacy: MemoryPrivacyClass::PublicRuntime,
                    observed_at: 1_000,
                }),
            }],
        }
    };
    let report = runtime
        .write(request)
        .expect("canonical nonempty archive payload");
    assert!(report.accepted);
    assert_eq!(report.changed, 1);
}

fn namespace_docs(
    snapshot: &StoreSnapshot,
    namespace: &str,
) -> BTreeMap<String, serde_json::Value> {
    snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == namespace)
        .map(|doc| (doc.key.clone(), doc.value.clone()))
        .collect()
}

fn assert_runtime_owner_preserved(before: &StoreSnapshot, after: &StoreSnapshot) {
    for namespace in ["runtime_skill_records", "runtime_skill_scope_manifests"] {
        let expected = namespace_docs(before, namespace);
        assert!(!expected.is_empty(), "nonempty protected owner control");
        assert!(
            namespace_docs(after, namespace) == expected,
            "protected owner must remain unchanged"
        );
    }
    let events = |snapshot: &StoreSnapshot| {
        snapshot
            .events
            .iter()
            .filter(|event| is_runtime_event(event))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert!(
        !events(before).is_empty(),
        "nonempty protected event control"
    );
    assert!(
        events(before) == events(after),
        "protected events must remain unchanged"
    );
}

fn is_runtime_event(event: &bm_sdk::nonproduction_replay_harness::MemoryStoreEvent) -> bool {
    matches!(
        event.plane.as_str(),
        "runtime_skill_records" | "runtime_skill_scope_manifests"
    ) || event
        .payload
        .get("operation")
        .is_some_and(|operation| operation.starts_with("runtime_skill."))
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

fn scoped_non_soul_event_ids(
    snapshot: &StoreSnapshot,
    physical_scope: &StorePhysicalOwningScope,
) -> BTreeSet<String> {
    scoped_event_ids(snapshot, physical_scope)
        .into_iter()
        .filter(|event_id| {
            snapshot
                .events
                .iter()
                .find(|event| &event.event_id == event_id)
                .is_some_and(|event| {
                    !is_subject_soul_event(event)
                        && !is_runtime_event(event)
                        && event.plane != "procedural_feedback"
                        && event.plane != "procedural_selection_authorities_private"
                        && !matches!(
                            event.plane.as_str(),
                            "governed_evidence_documents"
                                | "governed_evidence_source_refs"
                                | "governed_evidence_source_claim_manifests"
                        )
                        && !event
                            .payload
                            .get("operation")
                            .is_some_and(|operation| operation.starts_with("procedural_selection."))
                })
        })
        .collect()
}

fn scoped_soul_event_ids(
    snapshot: &StoreSnapshot,
    physical_scope: &StorePhysicalOwningScope,
) -> BTreeSet<String> {
    scoped_event_ids(snapshot, physical_scope)
        .into_iter()
        .filter(|event_id| {
            snapshot
                .events
                .iter()
                .find(|event| &event.event_id == event_id)
                .is_some_and(is_subject_soul_event)
        })
        .collect()
}

fn is_subject_soul_event(event: &bm_sdk::nonproduction_replay_harness::MemoryStoreEvent) -> bool {
    event.event_id.starts_with("subject-soul-event:")
        || SUBJECT_GLOBAL_SOUL_NAMESPACES.contains(&event.plane.as_str())
        || event
            .payload
            .get("operation")
            .is_some_and(|operation| operation.starts_with("subject_soul."))
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

fn subject_soul_mor_docs(
    snapshot: &StoreSnapshot,
) -> BTreeMap<(String, String), serde_json::Value> {
    snapshot
        .json_docs
        .iter()
        .filter(|doc| {
            let operation_kind = match doc.namespace.as_str() {
                "memory_mutation_receipts" => {
                    serde_json::from_value::<MemoryMutationReceipt>(doc.value.clone())
                        .ok()
                        .map(|receipt| receipt.identity.operation_kind())
                }
                "memory_mutation_audits" => {
                    serde_json::from_value::<MemoryMutationAuditRecord>(doc.value.clone())
                        .ok()
                        .map(|audit| audit.identity.operation_kind())
                }
                _ => None,
            };
            operation_kind.is_some_and(|kind| {
                matches!(
                    kind,
                    MemoryMutationOperationKind::SoulEvidence
                        | MemoryMutationOperationKind::SoulProvision
                        | MemoryMutationOperationKind::SoulRevision
                        | MemoryMutationOperationKind::SoulArchive
                        | MemoryMutationOperationKind::SoulRestore
                        | MemoryMutationOperationKind::SoulReset
                        | MemoryMutationOperationKind::SoulReseed
                        | MemoryMutationOperationKind::SoulDelete
                )
            })
        })
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArchiveScenario {
    FactualRoot,
    ProtectedRuntime,
    ProtectedSoul,
}

fn assert_backend_scoped_archive_restore(backend: &str, root: &Path, scenario: ArchiveScenario) {
    let source =
        MemoryStoreHandle::open_for_nonproduction_harness(backend_config(backend, "source", root))
            .unwrap_or_else(|error| panic!("open {backend} source: {error}"));
    let target =
        MemoryStoreHandle::open_for_nonproduction_harness(backend_config(backend, "target", root))
            .unwrap_or_else(|error| panic!("open {backend} target: {error}"));
    let source_a = runtime(&source, SUBJECT_A);
    let target_a = runtime(&target, SUBJECT_A);
    let target_b = runtime(&target, SUBJECT_B);

    if scenario == ArchiveScenario::ProtectedRuntime {
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
    }
    if scenario == ArchiveScenario::ProtectedSoul {
        seed_subject_soul(&source_a, &format!("{backend}:source"));
        seed_subject_soul(&target_a, &format!("{backend}:target"));
    }
    seed_archive_payload(&source_a, "source-subject-evidence", false);
    seed_archive_payload(&target_a, "target-old-subject-evidence", false);
    seed_archive_payload(&target_b, "target-sibling-evidence", false);
    if scenario == ArchiveScenario::FactualRoot {
        seed_archive_payload(&source_a, "source-space-fact", true);
        seed_archive_payload(&target_a, "target-old-space-fact", true);
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
    assert!(
        source_snapshot
            .events
            .iter()
            .any(|event| event.plane == "procedural_feedback"),
        "source mutation really emitted protected dependency audit events"
    );
    for archive in [&subject_archive, &shared_archive] {
        assert!(
            !archive.contains_event_plane("procedural_feedback"),
            "{backend}: source-derived learning audit must not enter a public archive"
        );
        for namespace in ["runtime_skill_records", "runtime_skill_scope_manifests"] {
            assert!(!archive.contains_json_namespace(namespace));
            assert!(!archive.contains_event_plane(namespace));
        }
    }
    let private_archive = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: archive_scope_subject(SUBJECT_A),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .unwrap()
    .archive;
    assert!(
        !private_archive.contains_event_plane("procedural_feedback"),
        "private-content disclosure does not export procedural authority or its internal audit"
    );
    assert!(subject_archive.contains_json_namespace("governed_evidence_documents"));
    assert!(
        !subject_archive.contains_event_plane("governed_evidence_documents"),
        "ExcludePrivate does not export raw evidence event metadata"
    );
    assert_eq!(
        subject_archive.contains_json_namespace("long_term_version_materials"),
        scenario == ArchiveScenario::FactualRoot
    );
    assert!(!shared_archive.contains_json_namespace("long_term_version_materials"));
    if scenario == ArchiveScenario::ProtectedSoul {
        assert!(!subject_global_soul_docs(&source_snapshot).is_empty());
        assert!(!subject_global_soul_docs(&target_before).is_empty());
        assert!(!subject_soul_mor_docs(&target_before).is_empty());
    }

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
    let procedural_events = |snapshot: &StoreSnapshot| {
        snapshot
            .events
            .iter()
            .filter(|event| event.plane == "procedural_feedback")
            .map(|event| (event.event_id.clone(), event.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    let prior_events = procedural_events(&target_before);
    let resulting_events = procedural_events(&after_subject);
    assert!(!prior_events.is_empty());
    for (id, event) in &prior_events {
        assert_eq!(
            resulting_events.get(id),
            Some(event),
            "target audit must be preserved"
        );
    }
    for id in procedural_events(&source_snapshot).keys() {
        assert!(
            !resulting_events.contains_key(id),
            "source audit must not be transplanted"
        );
    }
    let prior_roots = namespace_docs(&target_before, "procedural_source_dependents");
    let resulting_roots = namespace_docs(&after_subject, "procedural_source_dependents");
    let changed_roots = resulting_roots
        .iter()
        .filter(|(key, value)| prior_roots.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    assert!(
        !changed_roots.is_empty(),
        "{backend}/{scenario:?}: real source replacement changes dependency roots"
    );
    let added_events = resulting_events
        .iter()
        .filter(|(id, _)| !prior_events.contains_key(*id))
        .map(|(_, event)| event)
        .collect::<Vec<_>>();
    assert_eq!(added_events.len(), changed_roots.len());
    assert_eq!(
        added_events
            .iter()
            .map(|event| event.record_key.clone())
            .collect::<BTreeSet<_>>(),
        changed_roots.keys().cloned().collect()
    );
    for event in added_events {
        let root: ProceduralSourceDependentsRootV1 =
            serde_json::from_value(changed_roots[&event.record_key].clone()).unwrap();
        root.validate().unwrap();
        assert_eq!(event.scope.memory_space_id, MEMORY_SPACE_ID);
        assert_eq!(
            event.scope.physical_owning_scope,
            subject_event_scope(SUBJECT_A)
        );
        assert_eq!(
            event.payload.get("operation").map(String::as_str),
            Some("post_turn.procedural.source_archive_replace")
        );
        assert_eq!(
            event.payload.get("transaction_id"),
            Some(&root.last_change.transaction_id)
        );
        assert!(
            root.last_change.operation.is_none(),
            "archive must not invent a MOR"
        );
    }
    assert!(!export_scope(&target, archive_scope_subject(SUBJECT_A))
        .contains_event_plane("procedural_feedback"));
    assert_eq!(
        runtime_skill_docs(&after_subject, &subject_scope(SUBJECT_A)),
        runtime_skill_docs(&target_before, &subject_scope(SUBJECT_A)),
        "{backend}: Subject protected owner preservation"
    );
    if scenario == ArchiveScenario::ProtectedRuntime {
        assert_runtime_owner_preserved(&target_before, &after_subject);
    }
    let evidence = namespace_docs(&after_subject, "governed_evidence_documents");
    let mut expected_evidence = namespace_docs(&source_snapshot, "governed_evidence_documents");
    expected_evidence.extend(
        namespace_docs(&target_before, "governed_evidence_documents")
            .into_iter()
            .filter(|(_, value)| value["mounted_subject_id"] == SUBJECT_B),
    );
    assert!(
        evidence == expected_evidence,
        "subject evidence replaced, sibling preserved"
    );
    assert!(
        namespace_docs(&after_subject, "long_term_version_materials")
            == namespace_docs(&source_snapshot, "long_term_version_materials"),
        "subject archive restores its MemorySpace factual root"
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
    let actual = scoped_non_soul_event_ids(&after_subject, &subject_event_scope(SUBJECT_A));
    let expected = scoped_non_soul_event_ids(&source_snapshot, &subject_event_scope(SUBJECT_A));
    let missing = source_snapshot
        .events
        .iter()
        .filter(|event| expected.contains(&event.event_id) && !actual.contains(&event.event_id))
        .map(|event| (&event.plane, event.payload.get("operation")))
        .collect::<Vec<_>>();
    let unexpected = after_subject
        .events
        .iter()
        .filter(|event| actual.contains(&event.event_id) && !expected.contains(&event.event_id))
        .map(|event| {
            (
                &event.plane,
                event.payload.get("operation"),
                &event.record_key,
            )
        })
        .collect::<Vec<_>>();
    assert!(
        actual == expected,
        "{backend}: Subject archive events missing: {missing:?}; unexpected: {unexpected:?}"
    );
    assert_eq!(
        scoped_soul_event_ids(&after_subject, &subject_event_scope(SUBJECT_A)),
        scoped_soul_event_ids(&target_before, &subject_event_scope(SUBJECT_A)),
        "{backend}: Subject replace must preserve target Soul lifecycle events"
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
    assert_eq!(
        subject_soul_mor_docs(&after_subject),
        subject_soul_mor_docs(&target_before),
        "{backend}: Subject replace must preserve target Soul operation receipts and audits"
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
        runtime_skill_docs(&target_before, &RuntimeSkillOwningScope::SharedProgram),
        "{backend}: SharedProgram protected owner preservation"
    );
    if scenario == ArchiveScenario::ProtectedRuntime {
        assert_runtime_owner_preserved(&after_subject, &after_shared);
    }
    assert!(
        namespace_docs(&after_shared, "long_term_version_materials")
            == namespace_docs(&source_snapshot, "long_term_version_materials"),
        "SharedProgram restore preserves the MemorySpace factual root"
    );
    assert!(
        namespace_docs(&after_shared, "governed_evidence_documents")
            == namespace_docs(&after_subject, "governed_evidence_documents"),
        "shared restore preserves subject evidence"
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
        scoped_non_soul_event_ids(&after_shared, &StorePhysicalOwningScope::SharedProgram),
        scoped_non_soul_event_ids(&source_snapshot, &StorePhysicalOwningScope::SharedProgram),
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
    assert!(
        full_snapshot(&target) == before_forged,
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
    assert!(
        full_snapshot(&target) == before_cross_kind,
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
    for scenario in [
        ArchiveScenario::FactualRoot,
        ArchiveScenario::ProtectedRuntime,
        ArchiveScenario::ProtectedSoul,
    ] {
        let scenario_root = root.join(format!("{scenario:?}"));
        for backend in ["in_memory", "embedded", "file"] {
            assert_backend_scoped_archive_restore(backend, &scenario_root, scenario);
        }
        #[cfg(feature = "sqlite-store")]
        assert_backend_scoped_archive_restore("sqlite", &scenario_root, scenario);
    }
    let _ = std::fs::remove_dir_all(root);
}
