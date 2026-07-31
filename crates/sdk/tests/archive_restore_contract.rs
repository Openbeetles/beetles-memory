#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    canonical_recall_evidence_group, governed_evidence_document_content_digest,
    validate_governed_evidence_source_ref, GovernedEvidenceDocument, GovernedEvidenceDocumentChunk,
    GovernedEvidenceDocumentDraft, GovernedEvidenceDocumentSourceKind, GovernedEvidenceSourceRef,
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryGovernancePolicyMutation, MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemoryWriteCandidate,
};
use bm_core::platform::Platform as _;
use bm_sdk::nonproduction_replay_harness::{
    export_memory_space, import_memory_space, GovernedEvidenceSourceClaimManifest, StoreSnapshot,
    GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE, LONG_TERM_HEAD_MANIFEST_NAMESPACE,
    LONG_TERM_VERSION_MATERIAL_NAMESPACE, LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
};
use bm_sdk::{
    default_agent_subject_id, MemoryArchiveScope, MemoryEvidenceDocumentMutation,
    MemoryLongTermPolicyRequest, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy, MemoryWriteRequest, RuntimeLifecycleModeInput,
};
#[cfg(feature = "sqlite-store")]
use bm_sdk::{MemoryStoreHandle, ProfileId, StoreBackendConfig};

#[cfg(feature = "sqlite-store")]
use support::open_memory_store;
use support::{empty_store_platform, test_runtime_with_identity_scope, test_runtime_with_scope};

const EVIDENCE_DOCUMENT_NAMESPACE: &str = "governed_evidence_documents";
const EVIDENCE_SOURCE_REF_NAMESPACE: &str = "governed_evidence_source_refs";
const CONTROL_SCOPE_MANIFEST_NAMESPACE: &str = "control_plane_scope_manifests";
const LONG_TERM_GOVERNANCE_POLICY_NAMESPACE: &str = "long_term_governance_policy";

fn llm_accept(target: MemoryCandidateTarget) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(target),
        reason: "llm_semantic_judgment".to_string(),
    }
}

fn scope(memory_space_id: &str) -> MemoryArchiveScope {
    MemoryArchiveScope::subject(memory_space_id, default_agent_subject_id("agent-main"))
        .expect("subject archive scope")
}

fn runtime_scope(runtime: &bm_sdk::MemoryRuntime) -> MemoryArchiveScope {
    MemoryArchiveScope::subject(runtime.memory_space_id(), runtime.subject_id())
        .expect("runtime subject archive scope")
}

fn write_project_candidate(
    runtime: &bm_sdk::MemoryRuntime,
    candidate_id: &str,
    topic: &str,
    body: &str,
) {
    write_project_candidate_with_privacy(
        runtime,
        candidate_id,
        topic,
        body,
        MemoryPrivacyClass::SharedWithSubject,
    );
}

fn write_project_candidate_with_privacy(
    runtime: &bm_sdk::MemoryRuntime,
    candidate_id: &str,
    topic: &str,
    body: &str,
    privacy: MemoryPrivacyClass,
) {
    runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![MemoryWriteCandidate {
                candidate_id: candidate_id.to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Project,
                    topic: topic.to_string(),
                },
                privacy,
                content: MemoryCandidateContent::Text {
                    topic: topic.to_string(),
                    body: body.to_string(),
                    keywords: vec![topic.to_string()],
                },
                evidence_refs: vec![format!("fixture:{candidate_id}")],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_accept(MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Project,
                    topic: topic.to_string(),
                })),
            }],
        })
        .expect("write project candidate");
}

fn write_subject_policy(runtime: &bm_sdk::MemoryRuntime) {
    runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(runtime.memory_space_id().to_string()),
                    subject_id: Some(runtime.subject_id().to_string()),
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: Some("temporary-*".to_string()),
                    source_chat_id: None,
                    source_scope: None,
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "archive_restore_policy".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("write subject policy");
}

fn assert_v6_long_term_closure(archive: &bm_sdk::MemorySpaceArchive) {
    for namespace in [
        LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
        LONG_TERM_HEAD_MANIFEST_NAMESPACE,
        LONG_TERM_VERSION_MATERIAL_NAMESPACE,
        CONTROL_SCOPE_MANIFEST_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
    ] {
        assert!(
            archive.contains_json_namespace(namespace),
            "archive missing v6 long-term closure namespace {namespace}"
        );
    }
    assert!(!archive.contains_json_namespace("long_term"));
}

#[test]
fn archive_scope_rejects_whitespace_aliases_on_export_and_import() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let canonical_scope = scope("space-canonical");
    let exported = export_memory_space(
        &store,
        MemorySpaceExportRequest {
            scope: canonical_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        },
    )
    .expect("canonical export");

    for aliased in [
        MemoryArchiveScope::Subject {
            memory_space_id: " space-canonical".to_string(),
            mounted_subject_id: canonical_scope
                .mounted_subject_id()
                .expect("subject scope")
                .to_string(),
        },
        MemoryArchiveScope::Subject {
            memory_space_id: canonical_scope.memory_space_id().to_string(),
            mounted_subject_id: format!(
                "{} ",
                canonical_scope.mounted_subject_id().expect("subject scope")
            ),
        },
    ] {
        assert_eq!(
            export_memory_space(
                &store,
                MemorySpaceExportRequest {
                    scope: aliased.clone(),
                    private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
                },
            )
            .expect_err("export must reject identity aliases")
            .stage(),
            "memory_archive_scope"
        );
        assert_eq!(
            import_memory_space(
                &store,
                MemorySpaceImportRequest {
                    scope: aliased,
                    expected_private_material_policy:
                        MemorySpacePrivateMaterialPolicy::ExcludePrivate,
                    archive: exported.archive.clone(),
                },
            )
            .expect_err("import must reject identity aliases")
            .stage(),
            "memory_archive_scope"
        );
    }
}

#[test]
fn public_archive_debug_does_not_expose_raw_snapshot_payloads() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let secret = "archive-debug-must-not-disclose-this-raw-payload";
    source
        .replay_harness()
        .session_store()
        .append("chat-debug-boundary", "user", secret)
        .expect("seed private archive payload");

    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: scope("space-debug-boundary"),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export raw archive");
    let public_debug = format!("{exported:?}");

    assert!(!public_debug.contains(secret));
    assert!(!public_debug.contains("json_docs: ["));
    assert!(!public_debug.contains("events: ["));
}

#[test]
fn typed_archive_import_requires_exact_scope_and_private_policy() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let source_scope = scope("space-main");
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("typed export");
    assert_eq!(exported.projection_scope.scope, source_scope);
    assert_eq!(
        exported.projection_scope.private_material_policy,
        MemorySpacePrivateMaterialPolicy::IncludePrivate
    );
    assert_eq!(exported.archive.root().scope, source_scope);
    assert_eq!(
        exported.archive.root().private_material_policy,
        MemorySpacePrivateMaterialPolicy::IncludePrivate
    );
    assert_eq!(
        exported.archive.root().json_doc_count,
        exported.archive.json_doc_count() as u64
    );
    assert_eq!(exported.archive.root().closure_sha256.len(), 64);

    let target = empty_store_platform(profile);
    let scope_error = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: scope("space-copy"),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive.clone(),
        },
    )
    .expect_err("direct import cannot relabel an archive");
    assert_eq!(scope_error.stage(), "memory_archive_scope");

    let policy_error = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: source_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: exported.archive.clone(),
        },
    )
    .expect_err("direct import must reject a private-material policy mismatch");
    assert_eq!(policy_error.stage(), "memory_space_import");

    let expected_root = exported.archive.root().clone();
    let imported = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: source_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("same-scope import");
    assert_eq!(imported.imported_scope, source_scope);
    assert_eq!(imported.archive_root, expected_root);
    assert_eq!(imported.inserted_json_docs, 0);
    assert_eq!(imported.deleted_json_docs, 0);
    assert_eq!(imported.inserted_events, 0);
    assert_eq!(imported.deleted_events, 0);
}

#[test]
fn same_scope_restore_preserves_v6_long_term_facet_and_control_closure() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(source.clone(), profile, "local", "chat-a");
    write_project_candidate(
        &runtime,
        "candidate-project",
        "sdk_readiness",
        "SDK host integration must use public memory contracts.",
    );
    write_subject_policy(&runtime);

    let archive_scope = runtime_scope(&runtime);
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: archive_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export v6 closure");
    assert!(exported
        .archive
        .contains_json_namespace("memory_facet_indexes"));
    assert_v6_long_term_closure(&exported.archive);
    let expected_root = exported.archive.root().clone();

    let target = empty_store_platform(profile);
    let imported = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: archive_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("restore v6 closure");
    assert_eq!(imported.archive_root, expected_root);
    assert!(imported.inserted_json_docs > 0);

    let restored = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: archive_scope,
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export restored closure");
    assert_eq!(restored.archive.root(), &expected_root);
    assert_v6_long_term_closure(&restored.archive);
}

#[test]
fn same_scope_restore_replaces_only_that_scope() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let source_runtime = test_runtime_with_scope(source.clone(), profile, "local", "source-a");
    write_project_candidate(
        &source_runtime,
        "candidate-source-current",
        "source_current",
        "Current source projection must replace the stale target projection.",
    );
    let source_scope = runtime_scope(&source_runtime);
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export source scope");
    let expected_source_root = exported.archive.root().clone();

    let target = empty_store_platform(profile);
    let stale_runtime = test_runtime_with_scope(target.clone(), profile, "local", "target-a");
    write_project_candidate(
        &stale_runtime,
        "candidate-target-stale",
        "target_stale",
        "This stale target projection must be replaced.",
    );
    let other_runtime = test_runtime_with_identity_scope(
        target.clone(),
        profile,
        "agent-main",
        "owner-other-space",
        "local",
        "target-b",
    );
    write_project_candidate(
        &other_runtime,
        "candidate-other-space",
        "other_space",
        "The unrelated memory space must survive scoped replacement.",
    );
    let other_scope = runtime_scope(&other_runtime);
    let stale_before = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export stale target scope");
    let other_before = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: other_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export unrelated scope before import");
    assert_ne!(stale_before.archive.root(), &expected_source_root);
    let other_root = other_before.archive.root().clone();

    let report = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: source_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("replace exact source scope");
    assert!(report.inserted_json_docs > 0);
    assert!(report.deleted_json_docs > 0);

    let source_after = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: source_scope,
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export replaced source scope");
    let other_after = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: other_scope,
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export unrelated scope after import");
    assert_eq!(source_after.archive.root(), &expected_source_root);
    assert_eq!(other_after.archive.root(), &other_root);
}

#[test]
fn runtime_rejects_cross_scope_export_and_restore_without_mutation() {
    let profile = support::host_test_profile();
    let shared_store = empty_store_platform(profile);
    let runtime_a = test_runtime_with_identity_scope(
        shared_store.clone(),
        profile,
        "agent-a",
        "owner-a",
        "local",
        "chat-a",
    );
    let runtime_b = test_runtime_with_identity_scope(
        shared_store,
        profile,
        "agent-b",
        "owner-b",
        "local",
        "chat-b",
    );
    write_project_candidate(&runtime_a, "space-a-entry", "space-a", "owned by runtime A");
    write_project_candidate(&runtime_b, "space-b-entry", "space-b", "owned by runtime B");

    let scope_a = runtime_scope(&runtime_a);
    let scope_b = runtime_scope(&runtime_b);
    let archive_b = runtime_b
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope_b.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        })
        .expect("runtime B export")
        .archive;
    let root_a_before = runtime_a
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope_a.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        })
        .expect("runtime A baseline export")
        .archive
        .root()
        .clone();

    let cross_export = runtime_a
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope_b.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        })
        .expect_err("runtime A must not export runtime B scope");
    assert_eq!(cross_export.stage(), "memory_runtime_archive_scope");

    let cross_import = runtime_a
        .import_memory_space(MemorySpaceImportRequest {
            scope: scope_b,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: archive_b.clone(),
        })
        .expect_err("runtime A must not replace runtime B scope");
    assert_eq!(cross_import.stage(), "memory_runtime_archive_scope");

    let disguised_archive = runtime_a
        .import_memory_space(MemorySpaceImportRequest {
            scope: scope_a.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: archive_b,
        })
        .expect_err("runtime A must reject an archive declared for runtime B");
    assert_eq!(disguised_archive.stage(), "memory_archive_scope");

    let root_a_after = runtime_a
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope_a,
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        })
        .expect("runtime A export after rejected imports")
        .archive
        .root()
        .clone();
    assert_eq!(root_a_after, root_a_before);
}

#[test]
fn public_archive_redacts_private_owner_and_keeps_v6_public_closure() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(source.clone(), profile, "local", "chat-redaction");
    write_project_candidate(
        &runtime,
        "candidate-public",
        "public-project",
        "Public project memory survives public archive projection.",
    );
    write_project_candidate_with_privacy(
        &runtime,
        "candidate-private",
        "private-project",
        "Private project memory must not survive public archive projection.",
        MemoryPrivacyClass::PrivateGarden,
    );
    write_subject_policy(&runtime);

    let archive_scope = runtime_scope(&runtime);
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: archive_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        },
    )
    .expect("public governed projection");
    assert_eq!(
        exported.archive.root().private_material_policy,
        MemorySpacePrivateMaterialPolicy::ExcludePrivate
    );
    assert_v6_long_term_closure(&exported.archive);

    let target = empty_store_platform(profile);
    import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: archive_scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: exported.archive,
        },
    )
    .expect("redacted governed projection remains importable");
    let snapshot = target
        .replay_harness()
        .export_store_snapshot()
        .expect("inspect imported projection");
    let material = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == LONG_TERM_VERSION_MATERIAL_NAMESPACE)
        .collect::<Vec<_>>();
    assert_eq!(material.len(), 1);
    assert!(material[0].value.to_string().contains("public-project"));
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| doc.value.to_string().contains("private-project")));
}

fn governed_evidence_draft(scope: &MemoryArchiveScope) -> Box<GovernedEvidenceDocumentDraft> {
    let document_id = "evidence:archive:public";
    let source_locator = "opaque://archive-contract/public-evidence";
    let canonical_evidence_group =
        canonical_recall_evidence_group("archive-contract:public-evidence");
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:archive".to_string(),
        ordinal: 0,
        body: "Governed evidence must retain its owner, source claim, and manifest.".to_string(),
    }];
    let body = "Governed public evidence remains closed after archive restore.";
    Box::new(GovernedEvidenceDocumentDraft {
        memory_space_id: scope.memory_space_id().to_string(),
        mounted_subject_id: scope
            .mounted_subject_id()
            .expect("subject archive scope")
            .to_string(),
        document_id: document_id.to_string(),
        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
        source_locator: source_locator.to_string(),
        canonical_evidence_group: canonical_evidence_group.clone(),
        evidence_family_group: None,
        source_revision: 1,
        body: body.to_string(),
        chunks: chunks.clone(),
        content_digest: governed_evidence_document_content_digest(
            source_locator,
            &canonical_evidence_group,
            None,
            body,
            &chunks,
        ),
        authority: MemoryEvidenceAuthority::WorldObservation,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        observed_at: 1_800_000_000,
    })
}

fn assert_governed_evidence_closure(
    snapshot: &StoreSnapshot,
    scope: &MemoryArchiveScope,
    max_scope_entries: usize,
) {
    let owners = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == EVIDENCE_DOCUMENT_NAMESPACE)
        .map(|doc| {
            serde_json::from_value::<GovernedEvidenceDocument>(doc.value.clone())
                .expect("decode governed evidence owner")
        })
        .collect::<Vec<_>>();
    let claims = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == EVIDENCE_SOURCE_REF_NAMESPACE)
        .map(|doc| {
            serde_json::from_value::<GovernedEvidenceSourceRef>(doc.value.clone())
                .expect("decode governed evidence source claim")
        })
        .collect::<Vec<_>>();
    let manifests = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE)
        .map(|doc| {
            serde_json::from_value::<GovernedEvidenceSourceClaimManifest>(doc.value.clone())
                .expect("decode governed evidence claim manifest")
        })
        .collect::<Vec<_>>();

    assert_eq!(owners.len(), 1);
    assert_eq!(claims.len(), 1);
    assert_eq!(manifests.len(), 1);
    assert_eq!(owners[0].memory_space_id, scope.memory_space_id());
    assert_eq!(
        owners[0].mounted_subject_id,
        scope.mounted_subject_id().expect("subject archive scope")
    );
    validate_governed_evidence_source_ref(&owners[0], &claims[0])
        .expect("evidence source claim closes to owner");
    assert_eq!(manifests[0].owner_claim_bindings.len(), 1);
    let binding = &manifests[0].owner_claim_bindings[0];
    assert_eq!(binding.owner_physical_key, owners[0].physical_key);
    assert_eq!(binding.claim_physical_key, claims[0].physical_key);
    binding
        .validate()
        .expect("evidence owner-claim binding digest");
    manifests[0]
        .validate_exact(
            scope.memory_space_id(),
            scope.mounted_subject_id().expect("subject archive scope"),
            manifests[0].owner_claim_bindings.clone(),
            max_scope_entries,
        )
        .expect("evidence claim manifest closes exact public scope");
}

#[test]
fn same_scope_restore_preserves_governed_evidence_closure() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(source.clone(), profile, "local", "evidence-a");
    let archive_scope = runtime_scope(&runtime);
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: governed_evidence_draft(&archive_scope),
            }],
        })
        .expect("seed governed evidence closure");

    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: archive_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        },
    )
    .expect("export governed evidence closure");
    for namespace in [
        EVIDENCE_DOCUMENT_NAMESPACE,
        EVIDENCE_SOURCE_REF_NAMESPACE,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
    ] {
        assert!(exported.archive.contains_json_namespace(namespace));
    }

    let target = empty_store_platform(profile);
    let report = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: archive_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: exported.archive,
        },
    )
    .expect("restore governed evidence closure");
    assert!(report.inserted_json_docs >= 3);
    let snapshot = target
        .replay_harness()
        .export_store_snapshot()
        .expect("inspect restored evidence closure");
    assert_governed_evidence_closure(&snapshot, &archive_scope, target.capacity().kv_max_entries);
}

#[cfg(feature = "sqlite-store")]
fn assert_backend_archive_restore(
    case_name: &str,
    profile: ProfileId,
    source: MemoryStoreHandle,
    target: MemoryStoreHandle,
) {
    let runtime = test_runtime_with_scope(source.clone(), profile, "local", "backend-a");
    write_project_candidate(
        &runtime,
        &format!("{case_name}-owner"),
        "backend_archive",
        "Every store backend restores the same governed archive closure.",
    );
    write_subject_policy(&runtime);
    let archive_scope = runtime_scope(&runtime);
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: archive_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export backend archive");
    assert_v6_long_term_closure(&exported.archive);
    let expected_root = exported.archive.root().clone();
    let report = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: archive_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("restore backend archive");
    assert_eq!(report.archive_root, expected_root);
    assert!(report.inserted_json_docs > 0);
    let restored = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: archive_scope,
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export restored backend archive");
    assert_eq!(restored.archive.root(), &expected_root);
}

#[cfg(feature = "sqlite-store")]
#[test]
fn same_scope_archive_restore_roundtrips_across_all_store_backends() {
    let profile = support::host_test_profile();
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-archive-matrix-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("create archive matrix root");
    let open = |config| open_memory_store(config).expect("open archive matrix store");

    assert_backend_archive_restore(
        "in-memory",
        profile,
        open(StoreBackendConfig::in_memory(profile).expect("in-memory source")),
        open(StoreBackendConfig::in_memory(profile).expect("in-memory target")),
    );
    let embedded_profile = ProfileId::EspStandaloneMemory;
    assert_backend_archive_restore(
        "embedded",
        embedded_profile,
        open(StoreBackendConfig::embedded(embedded_profile).expect("embedded source")),
        open(StoreBackendConfig::embedded(embedded_profile).expect("embedded target")),
    );
    assert_backend_archive_restore(
        "file",
        profile,
        open(StoreBackendConfig::file(root.join("file-source"), profile).expect("file source")),
        open(StoreBackendConfig::file(root.join("file-target"), profile).expect("file target")),
    );
    assert_backend_archive_restore(
        "sqlite",
        profile,
        open(
            StoreBackendConfig::sqlite(root.join("sqlite-source.db"), profile)
                .expect("sqlite source"),
        ),
        open(
            StoreBackendConfig::sqlite(root.join("sqlite-target.db"), profile)
                .expect("sqlite target"),
        ),
    );
    std::fs::remove_dir_all(root).expect("remove archive matrix root");
}
