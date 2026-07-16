#![cfg(feature = "nonproduction-replay-harness")]

mod support;

#[cfg(feature = "sqlite-store")]
use bm_core::memory::{
    canonical_recall_evidence_group, governed_evidence_document_content_digest,
    relationship_scope_id, validate_governed_evidence_source_ref, GovernedEvidenceDocument,
    GovernedEvidenceDocumentChunk, GovernedEvidenceDocumentDraft,
    GovernedEvidenceDocumentSourceKind, GovernedEvidenceSourceRef, MentalPrivacyState,
    RelationshipPortfolio, RelationshipPortfolioEntry, SelfAuthoredCore, SelfContinuity, SelfModel,
};
use bm_core::memory::{
    LongTermMemoryDraft, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemorySemanticJudgmentSource,
    MemoryWriteCandidate, ParsedLongTermMemoryExtraction,
};
#[cfg(feature = "sqlite-store")]
use bm_core::memory::{
    LongTermMemoryQuery, MemoryGovernancePolicyMutation, MemoryGovernanceSelector,
    MemoryGovernanceSuppressionDuration, MemoryLongTermControlView, MemoryLongTermMutation,
    MemoryLongTermTarget,
};
use bm_core::platform::Platform as _;
#[cfg(feature = "sqlite-store")]
use bm_sdk::nonproduction_replay_harness::{
    GovernedEvidenceSourceClaimManifest, StoreSnapshot,
    GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
};
#[cfg(feature = "sqlite-store")]
use bm_sdk::RuntimeLifecycleModeInput;
#[cfg(feature = "sqlite-store")]
use bm_sdk::StoreBackendConfig;
use bm_sdk::{
    apply_memory_space_migration, default_agent_subject_id, export_memory_space,
    import_memory_space, preview_memory_space_migration, MemorySpaceExportRequest,
    MemorySpaceImportRequest, MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewRequest,
    MemorySpacePrivateMaterialPolicy, MemorySpaceScope, MemoryStoreHandle, MemoryWriteRequest,
    ProfileId,
};
#[cfg(feature = "sqlite-store")]
use bm_sdk::{
    MemoryEvidenceDocumentMutation, MemoryLongTermListRequest, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest,
};

#[cfg(feature = "sqlite-store")]
use support::open_memory_store;
#[cfg(feature = "sqlite-store")]
use support::test_runtime_with_identity_scope_and_subject;
use support::{empty_store_platform, test_runtime_with_identity_scope, test_runtime_with_scope};

#[cfg(feature = "sqlite-store")]
const EVIDENCE_DOCUMENT_NAMESPACE: &str = "governed_evidence_documents";
#[cfg(feature = "sqlite-store")]
const EVIDENCE_SOURCE_REF_NAMESPACE: &str = "governed_evidence_source_refs";

fn llm_accept(target: MemoryCandidateTarget) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(target),
        reason: "llm_semantic_judgment".to_string(),
    }
}

fn scope(memory_space_id: &str) -> MemorySpaceScope {
    MemorySpaceScope {
        memory_space_id: memory_space_id.to_string(),
        mounted_subject_id: default_agent_subject_id("agent-main"),
    }
}

fn runtime_scope(runtime: &bm_sdk::MemoryRuntime) -> MemorySpaceScope {
    MemorySpaceScope {
        memory_space_id: runtime.memory_space_id().to_string(),
        mounted_subject_id: runtime.subject_id().to_string(),
    }
}

#[test]
fn memory_space_identity_rejects_whitespace_aliases_on_every_public_boundary() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let canonical_scope = scope("space-canonical");
    let exported = export_memory_space(
        &store,
        MemorySpaceExportRequest {
            scope: canonical_scope.clone(),
            include_private: false,
        },
    )
    .expect("canonical export");

    for aliased in [
        MemorySpaceScope {
            memory_space_id: " space-canonical".to_string(),
            mounted_subject_id: canonical_scope.mounted_subject_id.clone(),
        },
        MemorySpaceScope {
            memory_space_id: canonical_scope.memory_space_id.clone(),
            mounted_subject_id: format!("{} ", canonical_scope.mounted_subject_id),
        },
    ] {
        assert_eq!(
            export_memory_space(
                &store,
                MemorySpaceExportRequest {
                    scope: aliased.clone(),
                    include_private: false,
                },
            )
            .expect_err("export must reject identity aliases")
            .stage(),
            "memory_space_export"
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
            "memory_space_import"
        );
    }

    let preview_error = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_scope: canonical_scope.clone(),
        target_scope: MemorySpaceScope {
            memory_space_id: format!("{} ", canonical_scope.memory_space_id),
            mounted_subject_id: canonical_scope.mounted_subject_id,
        },
        expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        source_profile: profile,
        target_profile: profile,
        archive: exported.archive,
    })
    .expect_err("preview must reject target identity aliases");
    assert_eq!(preview_error.stage(), "memory_space_migration_preview");
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
            include_private: true,
        },
    )
    .expect("export raw archive");
    let public_debug = format!("{exported:?}");

    assert!(!public_debug.contains(secret));
    assert!(!public_debug.contains("json_docs: ["));
    assert!(!public_debug.contains("events: ["));

    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_scope: scope("space-debug-boundary"),
        target_scope: scope("space-debug-boundary-copy"),
        expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        source_profile: profile,
        target_profile: profile,
        archive: exported.archive,
    })
    .expect("preview migration");
    let preview_debug = format!("{preview:?}");
    assert!(!preview_debug.contains(secret));
    assert!(!preview_debug.contains("json_docs: ["));

    let apply_debug = format!(
        "{:?}",
        MemorySpaceMigrateApplyRequest { plan: preview.plan }
    );
    assert!(!apply_debug.contains(secret));
    assert!(!apply_debug.contains("json_docs: ["));
}

#[test]
fn memory_space_export_is_typed_and_import_requires_exact_space_identity() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    source
        .replay_harness()
        .session_store()
        .append("chat-a", "user", "sdk migration contract")
        .expect("seed session");

    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: scope("space-main"),
            include_private: true,
        },
    )
    .expect("export");
    assert_eq!(
        exported.projection_scope.scope.memory_space_id,
        "space-main"
    );
    assert_eq!(exported.export_report.json_docs, 0);
    assert_eq!(exported.export_report.events, 0);

    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_scope: scope("space-main"),
        target_scope: scope("space-copy"),
        expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        archive: exported.archive.clone(),
    })
    .expect("preview migration");
    assert!(!preview.loss_risk);
    assert_eq!(
        preview.manifest.projection_scope.scope.memory_space_id,
        "space-main"
    );
    assert_eq!(preview.manifest.target_scope.memory_space_id, "space-copy");
    assert!(preview.manifest.identity_remap.required);
    assert!(!preview.manifest.identity_remap.applied);
    assert!(preview.manifest.planes.is_empty());
    assert_eq!(
        preview.state_fingerprint,
        exported.export_report.state_fingerprint
    );
    assert_eq!(
        preview.vault_manifest.snapshot_fingerprint,
        exported.export_report.state_fingerprint
    );
    assert_eq!(preview.vault_preflight.source_profile, profile);
    assert!(preview.vault_preflight.lineage_allowed);
    let policy_mismatch_preview =
        preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
            source_scope: scope("space-main"),
            target_scope: scope("space-main"),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            source_profile: profile,
            target_profile: profile,
            archive: exported.archive.clone(),
        })
        .expect("preview migration");
    assert!(!policy_mismatch_preview.vault_preflight.passed);
    let target = empty_store_platform(profile);
    let apply_error = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest {
            plan: preview.plan.clone(),
        },
    )
    .expect_err("migration without an explicit space remap must fail closed");
    assert_eq!(apply_error.stage(), "memory_space_migration");

    let mismatch = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: scope("space-copy"),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive.clone(),
        },
    )
    .expect_err("direct import cannot relabel an archive");
    assert_eq!(mismatch.stage(), "memory_space_import");

    let policy_mismatch = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: scope("space-main"),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: exported.archive.clone(),
        },
    )
    .expect_err("direct import must reject a private-material policy mismatch");
    assert_eq!(policy_mismatch.stage(), "memory_space_import");

    let imported = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: scope("space-main"),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("same-space import");
    assert_eq!(imported.imported_scope.memory_space_id, "space-main");
}

fn assert_same_scope_facet_closure_migrates(
    profile: ProfileId,
    source: MemoryStoreHandle,
    target: MemoryStoreHandle,
    direct_target: MemoryStoreHandle,
) {
    let runtime = test_runtime_with_scope(source.clone(), profile, "local", "chat-a");
    runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-project".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Project,
                    topic: "sdk_readiness".to_string(),
                },
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::Text {
                    topic: "sdk_readiness".to_string(),
                    body: "SDK host integration must use public memory contracts.".to_string(),
                    keywords: vec!["sdk".to_string(), "readiness".to_string()],
                },
                evidence_refs: vec!["fixture:generic-rust-host".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_accept(MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Project,
                    topic: "sdk_readiness".to_string(),
                })),
            }],
        })
        .expect("write candidate");

    let source_scope = MemorySpaceScope {
        memory_space_id: runtime.memory_space_id().to_string(),
        mounted_subject_id: runtime.subject_id().to_string(),
    };
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: true,
        },
    )
    .expect("export");
    assert!(exported
        .archive
        .contains_json_namespace("memory_facet_indexes"));
    assert!(exported.archive.contains_json_namespace("long_term"));

    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_scope: source_scope.clone(),
        target_scope: source_scope.clone(),
        expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        source_profile: profile,
        target_profile: profile,
        archive: exported.archive.clone(),
    })
    .expect("preview migration");
    assert!(preview.vault_preflight.passed);
    assert!(preview
        .manifest
        .planes
        .iter()
        .any(|plane| plane.plane == "memory_facet_indexes" && plane.records > 0));

    let applied = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest { plan: preview.plan },
    )
    .expect("same-scope facet migration");
    assert_eq!(applied.target_scope, source_scope);
    assert!(applied.import_report.json_docs > 0);
    let imported_projection = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: true,
        },
    )
    .expect("export applied projection");
    assert_eq!(
        imported_projection.export_report.state_fingerprint,
        exported.export_report.state_fingerprint
    );

    let imported = import_memory_space(
        &direct_target,
        MemorySpaceImportRequest {
            scope: source_scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("direct same-scope facet import");
    assert!(imported.import_report.json_docs > 0);
}

#[cfg(feature = "sqlite-store")]
struct MigrationBackendCase {
    source: MemoryStoreHandle,
    migration_target: MemoryStoreHandle,
    direct_target: MemoryStoreHandle,
    replacement_target: MemoryStoreHandle,
    public_target: MemoryStoreHandle,
    fail_closed_target: MemoryStoreHandle,
}

#[cfg(feature = "sqlite-store")]
fn open_backend_case(make_config: impl Fn(&str) -> StoreBackendConfig) -> MigrationBackendCase {
    let open = |role: &str| {
        open_memory_store(make_config(role))
            .unwrap_or_else(|error| panic!("open {role} migration matrix store: {error:?}"))
    };
    MigrationBackendCase {
        source: open("source"),
        migration_target: open("migration-target"),
        direct_target: open("direct-target"),
        replacement_target: open("replacement-target"),
        public_target: open("public-target"),
        fail_closed_target: open("fail-closed-target"),
    }
}

#[cfg(feature = "sqlite-store")]
fn store_fingerprints(store: &MemoryStoreHandle) -> (String, String) {
    let snapshot = store
        .export_replay_snapshot()
        .expect("export migration matrix snapshot");
    (snapshot.state_fingerprint(), snapshot.event_fingerprint())
}

#[cfg(feature = "sqlite-store")]
fn assert_control_plane_roundtrip(
    case_name: &str,
    profile: ProfileId,
    source: MemoryStoreHandle,
    target: MemoryStoreHandle,
) {
    let source_runtime = test_runtime_with_scope(source.clone(), profile, "local", "control-a");
    write_project_candidate(
        &source_runtime,
        &format!("{case_name}-control-owner"),
        "control_roundtrip",
        "Control-plane migration must preserve governed maintenance state.",
    );
    let record_id = source_runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Project),
                limit: 10,
                ..LongTermMemoryQuery::default()
            },
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list control migration owner")
        .records
        .into_iter()
        .find(|record| record.record.topic == "control_roundtrip")
        .expect("control migration owner")
        .record
        .id;
    source_runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                replacement: LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "control_roundtrip".to_string(),
                    content: "Corrected control-plane owner before deletion.".to_string(),
                    keywords: vec!["control".to_string(), "roundtrip".to_string()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("control-a".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["test:control-roundtrip".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_900_000_010),
                    last_confirmed_at: Some(1_900_000_010),
                    source_revision: Some(2),
                },
            },
            reason: "control_roundtrip_correction".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("correct control migration owner");
    source_runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
            },
            reason: "control_roundtrip_delete".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete control migration owner");
    source_runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(source_runtime.memory_space_id().to_string()),
                    subject_id: Some(source_runtime.subject_id().to_string()),
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: Some("temporary-*".to_string()),
                    source_chat_id: None,
                    source_scope: None,
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "control_roundtrip_policy".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("seed control migration policy");

    let scope = runtime_scope(&source_runtime);
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: scope.clone(),
            include_private: true,
        },
    )
    .expect("export complete control-plane projection");
    for namespace in [
        "long_term_control_revision",
        "long_term_control_tombstone",
        "long_term_governance_policy",
        "long_term_control_audit",
        "control_plane_scope_manifests",
    ] {
        assert!(
            exported.archive.contains_json_namespace(namespace),
            "{case_name} export missing {namespace}"
        );
    }
    import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("import complete control-plane projection");
    let target_runtime = test_runtime_with_scope(target, profile, "local", "control-a");
    assert_eq!(runtime_scope(&target_runtime), scope);
    let control = target_runtime
        .replay_harness()
        .scoped_long_term_memory_control_read_store(&scope.memory_space_id)
        .expect("open restored scoped control store");
    assert!(
        !control
            .list_long_term_control_revisions(&record_id, 10)
            .expect("restored revisions")
            .is_empty(),
        "{case_name} lost revisions"
    );
    assert!(
        control
            .get_long_term_control_tombstone(&record_id)
            .expect("restored tombstone")
            .is_some(),
        "{case_name} lost tombstone"
    );
    assert!(
        !control
            .list_long_term_governance_policies(10)
            .expect("restored policies")
            .is_empty(),
        "{case_name} lost governance policy"
    );
    assert!(
        control
            .list_long_term_control_audit(10)
            .expect("restored audit")
            .len()
            >= 3,
        "{case_name} lost audit lineage"
    );
}

#[cfg(feature = "sqlite-store")]
fn governed_evidence_draft(scope: &MemorySpaceScope) -> Box<GovernedEvidenceDocumentDraft> {
    let document_id = "evidence:migration:public";
    let source_locator = "opaque://migration-contract/public-evidence";
    let canonical_evidence_group =
        canonical_recall_evidence_group("migration-contract:public-evidence");
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:migration".to_string(),
        ordinal: 0,
        body: "Governed public evidence must retain its owner, source claim, and manifest."
            .to_string(),
    }];
    let body = "Governed public evidence remains closed after private archive redaction.";
    Box::new(GovernedEvidenceDocumentDraft {
        memory_space_id: scope.memory_space_id.clone(),
        mounted_subject_id: scope.mounted_subject_id.clone(),
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

#[cfg(feature = "sqlite-store")]
fn assert_governed_evidence_closure(
    snapshot: &StoreSnapshot,
    scope: &MemorySpaceScope,
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
    assert_eq!(owners[0].memory_space_id, scope.memory_space_id);
    assert_eq!(owners[0].mounted_subject_id, scope.mounted_subject_id);
    validate_governed_evidence_source_ref(&owners[0], &claims[0])
        .expect("evidence source claim closes to owner");
    assert_eq!(manifests[0].owner_claim_bindings.len(), 1);
    let binding = &manifests[0].owner_claim_bindings[0];
    assert_eq!(binding.owner_physical_key, owners[0].physical_key);
    assert_eq!(binding.claim_physical_key, claims[0].physical_key);
    assert_eq!(binding.owner_revision, owners[0].owner_revision);
    assert_eq!(binding.source_revision, owners[0].source_revision);
    assert_eq!(binding.content_digest, owners[0].content_digest);
    binding
        .validate()
        .expect("evidence owner-claim binding digest");
    manifests[0]
        .validate_exact(
            &scope.memory_space_id,
            &scope.mounted_subject_id,
            manifests[0].owner_claim_bindings.clone(),
            max_scope_entries,
        )
        .expect("evidence claim manifest closes exact public scope");
}

#[cfg(feature = "sqlite-store")]
fn assert_backend_migration_matrix(
    case_name: &str,
    profile: ProfileId,
    stores: MigrationBackendCase,
) {
    let MigrationBackendCase {
        source,
        migration_target,
        direct_target,
        replacement_target,
        public_target,
        fail_closed_target,
    } = stores;
    assert_same_scope_facet_closure_migrates(
        profile,
        source.clone(),
        migration_target,
        direct_target,
    );

    let source_runtime = test_runtime_with_scope(source.clone(), profile, "local", "source-a");
    let source_scope = MemorySpaceScope {
        memory_space_id: source_runtime.memory_space_id().to_string(),
        mounted_subject_id: source_runtime.subject_id().to_string(),
    };
    let source_core = SelfAuthoredCore {
        identity_anchor: "canonical source soul".to_string(),
        updated_at: 1_900_000_000,
        ..SelfAuthoredCore::default()
    };
    let source_model = SelfModel {
        continuity_anchor: "canonical source identity".to_string(),
        updated_at: 1_900_000_001,
        ..SelfModel::default()
    };
    let source_continuity = SelfContinuity {
        wake_anchor: "canonical source continuity".to_string(),
        updated_at: 1_900_000_002,
        ..SelfContinuity::default()
    };
    let source_privacy = MentalPrivacyState {
        updated_at: 1_900_000_003,
        ..MentalPrivacyState::default()
    };
    let relationship_id = relationship_scope_id("local", "source-a");
    let source_portfolio = RelationshipPortfolio {
        entries: vec![RelationshipPortfolioEntry {
            scope_id: relationship_id.clone(),
            channel: "local".to_string(),
            chat_id: "source-a".to_string(),
            source_updated_at: 1_900_000_003,
            ..RelationshipPortfolioEntry::default()
        }],
        updated_at: 1_900_000_003,
    };
    source
        .replay_harness()
        .self_authored_core_store()
        .set(&source_scope.mounted_subject_id, &source_core)
        .expect("seed source scoped Soul owner");
    source
        .replay_harness()
        .self_model_store()
        .set(&source_scope.mounted_subject_id, &source_model)
        .expect("seed source identity owner");
    source
        .replay_harness()
        .self_continuity_store()
        .set(&source_scope.mounted_subject_id, &source_continuity)
        .expect("seed source continuity owner");
    source
        .replay_harness()
        .relationship_portfolio_store()
        .set(&source_scope.mounted_subject_id, &source_portfolio)
        .expect("seed source relationship owner manifest");
    source
        .replay_harness()
        .mental_privacy_store()
        .set(&relationship_id, &source_privacy)
        .expect("seed source privacy owner");
    let replacement_archive = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: true,
        },
    )
    .expect("export source projection for scoped replacement");

    let stale_runtime =
        test_runtime_with_scope(replacement_target.clone(), profile, "local", "target-a");
    replacement_target
        .replay_harness()
        .self_authored_core_store()
        .set(
            &source_scope.mounted_subject_id,
            &SelfAuthoredCore {
                identity_anchor: "stale target soul must be replaced".to_string(),
                updated_at: 1,
                ..SelfAuthoredCore::default()
            },
        )
        .expect("seed stale target Soul owner in import scope");
    replacement_target
        .replay_harness()
        .self_model_store()
        .set(
            &source_scope.mounted_subject_id,
            &SelfModel {
                continuity_anchor: "stale target identity".to_string(),
                updated_at: 1,
                ..SelfModel::default()
            },
        )
        .expect("seed stale target identity owner in import scope");
    replacement_target
        .replay_harness()
        .self_continuity_store()
        .set(
            &source_scope.mounted_subject_id,
            &SelfContinuity {
                wake_anchor: "stale target continuity".to_string(),
                updated_at: 1,
                ..SelfContinuity::default()
            },
        )
        .expect("seed stale target continuity owner in import scope");
    replacement_target
        .replay_harness()
        .relationship_portfolio_store()
        .set(&source_scope.mounted_subject_id, &source_portfolio)
        .expect("seed stale target relationship owner manifest");
    replacement_target
        .replay_harness()
        .mental_privacy_store()
        .set(
            &relationship_id,
            &MentalPrivacyState {
                updated_at: 1,
                ..MentalPrivacyState::default()
            },
        )
        .expect("seed stale target privacy owner in import scope");
    write_project_candidate(
        &stale_runtime,
        "candidate-target-stale",
        "target_stale",
        "This stale target projection must be replaced.",
    );
    let other_runtime = test_runtime_with_identity_scope_and_subject(
        replacement_target.clone(),
        profile,
        "agent-main",
        "owner-other-space",
        "subject-other-space",
        "local",
        "target-b",
    );
    write_project_candidate(
        &other_runtime,
        "candidate-other-space",
        "other_space",
        "The unrelated memory space must survive scoped replacement.",
    );
    let other_scope = MemorySpaceScope {
        memory_space_id: other_runtime.memory_space_id().to_string(),
        mounted_subject_id: other_runtime.subject_id().to_string(),
    };
    let other_before = export_memory_space(
        &replacement_target,
        MemorySpaceExportRequest {
            scope: other_scope.clone(),
            include_private: true,
        },
    )
    .expect("export unrelated scope before replacement");
    import_memory_space(
        &replacement_target,
        MemorySpaceImportRequest {
            scope: source_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: replacement_archive.archive.clone(),
        },
    )
    .expect("replace only the target scope");
    let replaced = export_memory_space(
        &replacement_target,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: true,
        },
    )
    .expect("export replaced target scope");
    let other_after = export_memory_space(
        &replacement_target,
        MemorySpaceExportRequest {
            scope: other_scope,
            include_private: true,
        },
    )
    .expect("export unrelated scope after replacement");
    assert_eq!(
        replaced.export_report.state_fingerprint,
        replacement_archive.export_report.state_fingerprint
    );
    assert_eq!(
        replacement_target
            .replay_harness()
            .self_authored_core_store()
            .get(&source_scope.mounted_subject_id)
            .expect("read replaced Soul owner"),
        Some(source_core.clone()),
        "{case_name} must atomically replace the stale Soul owner"
    );
    assert_eq!(
        replacement_target
            .replay_harness()
            .self_model_store()
            .get(&source_scope.mounted_subject_id)
            .expect("read replaced identity owner"),
        Some(source_model.clone()),
        "{case_name} must atomically replace the stale identity owner"
    );
    assert_eq!(
        replacement_target
            .replay_harness()
            .self_continuity_store()
            .get(&source_scope.mounted_subject_id)
            .expect("read replaced continuity owner"),
        Some(source_continuity.clone()),
        "{case_name} must atomically replace the stale continuity owner"
    );
    assert_eq!(
        replacement_target
            .replay_harness()
            .mental_privacy_store()
            .get(&relationship_id)
            .expect("read replaced privacy owner"),
        Some(source_privacy.clone()),
        "{case_name} must atomically replace the stale privacy owner"
    );
    assert_eq!(
        other_after.export_report.state_fingerprint,
        other_before.export_report.state_fingerprint
    );
    assert_eq!(
        other_after.export_report.event_fingerprint,
        other_before.export_report.event_fingerprint
    );

    write_project_candidate_with_privacy(
        &source_runtime,
        "candidate-private",
        "private-project",
        "Private owner must be removed without orphaning the governed public closure.",
        MemoryPrivacyClass::PrivateGarden,
    );
    source_runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: governed_evidence_draft(&source_scope),
            }],
        })
        .expect("seed governed evidence closure");
    let public_archive = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: false,
        },
    )
    .expect("export public governed projection");
    assert!(public_archive.privacy_redactions > 0);
    for private_namespace in [
        "self_authored_core",
        "self_model",
        "self_continuity",
        "mental_privacy",
    ] {
        assert!(
            !public_archive
                .archive
                .contains_json_namespace(private_namespace),
            "{case_name} public archive must omit {private_namespace}"
        );
    }
    assert!(public_archive
        .archive
        .contains_json_namespace(EVIDENCE_DOCUMENT_NAMESPACE));
    assert!(public_archive
        .archive
        .contains_json_namespace(EVIDENCE_SOURCE_REF_NAMESPACE));
    assert!(public_archive
        .archive
        .contains_json_namespace(GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE));
    public_target
        .replay_harness()
        .self_authored_core_store()
        .set(&source_scope.mounted_subject_id, &source_core)
        .expect("seed public-target stale Soul owner");
    public_target
        .replay_harness()
        .self_model_store()
        .set(&source_scope.mounted_subject_id, &source_model)
        .expect("seed public-target stale identity owner");
    public_target
        .replay_harness()
        .self_continuity_store()
        .set(&source_scope.mounted_subject_id, &source_continuity)
        .expect("seed public-target stale continuity owner");
    public_target
        .replay_harness()
        .relationship_portfolio_store()
        .set(&source_scope.mounted_subject_id, &source_portfolio)
        .expect("seed public-target relationship owner manifest");
    public_target
        .replay_harness()
        .mental_privacy_store()
        .set(&relationship_id, &source_privacy)
        .expect("seed public-target stale privacy owner");
    import_memory_space(
        &public_target,
        MemorySpaceImportRequest {
            scope: source_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: public_archive.archive.clone(),
        },
    )
    .expect("import public governed projection");
    assert!(public_target
        .replay_harness()
        .self_authored_core_store()
        .get(&source_scope.mounted_subject_id)
        .expect("read public-target Soul owner")
        .is_none());
    assert!(public_target
        .replay_harness()
        .self_model_store()
        .get(&source_scope.mounted_subject_id)
        .expect("read public-target identity owner")
        .is_none());
    assert!(public_target
        .replay_harness()
        .self_continuity_store()
        .get(&source_scope.mounted_subject_id)
        .expect("read public-target continuity owner")
        .is_none());
    assert!(public_target
        .replay_harness()
        .mental_privacy_store()
        .get(&relationship_id)
        .expect("read public-target privacy owner")
        .is_none());
    let public_snapshot = public_target
        .export_replay_snapshot()
        .expect("inspect public governed projection");
    assert_governed_evidence_closure(
        &public_snapshot,
        &source_scope,
        public_target.capacity().kv_max_entries,
    );
    assert!(public_snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "memory_facet_indexes"));
    assert!(!public_snapshot
        .json_docs
        .iter()
        .any(|doc| doc.value.to_string().contains("private-project")));

    let before_cross_scope = store_fingerprints(&fail_closed_target);
    let cross_scope_error = import_memory_space(
        &fail_closed_target,
        MemorySpaceImportRequest {
            scope: MemorySpaceScope {
                memory_space_id: "space:cross-scope".to_string(),
                mounted_subject_id: "subject:cross-scope".to_string(),
            },
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: public_archive.archive,
        },
    )
    .expect_err("cross-scope archive must fail closed");
    assert_eq!(cross_scope_error.stage(), "memory_space_import");
    assert_eq!(store_fingerprints(&fail_closed_target), before_cross_scope);

    let valid_snapshot = source
        .export_replay_snapshot()
        .expect("export source snapshot for schema rejection");
    for rejected_schema in [
        "beetle_memory_store_schema_v3",
        "unknown_memory_store_schema_v999",
    ] {
        let mut rejected_snapshot = valid_snapshot.clone();
        rejected_snapshot.schema_id = rejected_schema.to_string();
        rejected_snapshot.schema_manifest.schema_id = rejected_schema.to_string();
        let before_schema_error = store_fingerprints(&fail_closed_target);
        let schema_error = fail_closed_target
            .import_replay_snapshot(&rejected_snapshot)
            .expect_err("old or unknown schema must fail closed");
        assert_eq!(schema_error.stage(), "store_snapshot_import");
        assert_eq!(store_fingerprints(&fail_closed_target), before_schema_error);
    }
}

#[test]
fn memory_space_migration_imports_a_valid_same_scope_facet_closure() {
    let profile = support::host_test_profile();
    assert_same_scope_facet_closure_migrates(
        profile,
        empty_store_platform(profile),
        empty_store_platform(profile),
        empty_store_platform(profile),
    );
}

#[cfg(feature = "sqlite-store")]
#[test]
fn same_scope_facet_closure_migrates_across_all_store_backends() {
    let profile = support::host_test_profile();
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-migration-matrix-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("create migration matrix root");
    assert_backend_migration_matrix(
        "in-memory",
        profile,
        open_backend_case(|_| {
            StoreBackendConfig::in_memory(profile).expect("in-memory matrix config")
        }),
    );
    let embedded_profile = ProfileId::EspStandaloneMemory;
    assert_backend_migration_matrix(
        "embedded",
        embedded_profile,
        open_backend_case(|_| {
            StoreBackendConfig::embedded(embedded_profile).expect("embedded matrix config")
        }),
    );
    assert_backend_migration_matrix(
        "file",
        profile,
        open_backend_case(|role| {
            StoreBackendConfig::file(root.join(format!("file-{role}")), profile)
                .expect("file matrix config")
        }),
    );
    assert_backend_migration_matrix(
        "sqlite",
        profile,
        open_backend_case(|role| {
            StoreBackendConfig::sqlite(root.join(format!("sqlite-{role}.db")), profile)
                .expect("sqlite matrix config")
        }),
    );
    std::fs::remove_dir_all(root).expect("remove migration matrix root");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn scoped_control_plane_roundtrips_across_all_store_backends() {
    let profile = support::host_test_profile();
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-control-migration-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("create control migration root");
    let open = |config| open_memory_store(config).expect("open control migration store");
    assert_control_plane_roundtrip(
        "in-memory",
        profile,
        open(StoreBackendConfig::in_memory(profile).expect("in-memory source")),
        open(StoreBackendConfig::in_memory(profile).expect("in-memory target")),
    );
    let embedded_profile = ProfileId::EspStandaloneMemory;
    assert_control_plane_roundtrip(
        "embedded",
        embedded_profile,
        open(StoreBackendConfig::embedded(embedded_profile).expect("embedded source")),
        open(StoreBackendConfig::embedded(embedded_profile).expect("embedded target")),
    );
    assert_control_plane_roundtrip(
        "file",
        profile,
        open(StoreBackendConfig::file(root.join("file-source"), profile).expect("file source")),
        open(StoreBackendConfig::file(root.join("file-target"), profile).expect("file target")),
    );
    assert_control_plane_roundtrip(
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
    std::fs::remove_dir_all(root).expect("remove control migration root");
}

#[test]
fn same_scope_import_replaces_only_that_scope_and_preserves_other_spaces() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let source_runtime = test_runtime_with_scope(source.clone(), profile, "local", "source-a");
    write_project_candidate(
        &source_runtime,
        "candidate-source-current",
        "source_current",
        "Current source projection must replace the stale target projection.",
    );
    let source_scope = MemorySpaceScope {
        memory_space_id: source_runtime.memory_space_id().to_string(),
        mounted_subject_id: source_runtime.subject_id().to_string(),
    };
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: true,
        },
    )
    .expect("export source scope");

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
    let other_scope = MemorySpaceScope {
        memory_space_id: other_runtime.memory_space_id().to_string(),
        mounted_subject_id: other_runtime.subject_id().to_string(),
    };
    let stale_before = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: true,
        },
    )
    .expect("export stale target scope");
    let other_before = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: other_scope.clone(),
            include_private: true,
        },
    )
    .expect("export other scope before import");
    assert_ne!(
        stale_before.export_report.state_fingerprint,
        exported.export_report.state_fingerprint
    );

    import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: source_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("replace exact source scope");

    let source_after = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: source_scope,
            include_private: true,
        },
    )
    .expect("export replaced source scope");
    let other_after = export_memory_space(
        &target,
        MemorySpaceExportRequest {
            scope: other_scope,
            include_private: true,
        },
    )
    .expect("export other scope after import");
    assert_eq!(
        source_after.export_report.state_fingerprint,
        exported.export_report.state_fingerprint
    );
    assert_eq!(
        other_after.export_report.state_fingerprint,
        other_before.export_report.state_fingerprint
    );
    assert_eq!(
        other_after.export_report.event_fingerprint,
        other_before.export_report.event_fingerprint
    );
}

#[test]
fn memory_space_export_omits_unowned_private_records_before_migration() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    source
        .replay_harness()
        .private_garden_store()
        .write(
            "chat-a",
            "journal/raw.md",
            "private raw note",
            1_800_000_000,
        )
        .expect("private garden write");
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: scope("space-private"),
            include_private: true,
        },
    )
    .expect("export");
    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_scope: scope("space-private"),
        target_scope: scope("space-esp"),
        expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        source_profile: profile,
        target_profile: ProfileId::EspEmbeddedSdk,
        archive: exported.archive.clone(),
    })
    .expect("preview migration");
    assert!(!preview.vault_preflight.passed);
    assert!(!preview.vault_preflight.capability_allowed);
    assert!(preview.vault_preflight.privacy_allowed);
    assert!(preview.vault_redaction.redacted_refs.is_empty());
    assert_eq!(preview.vault_redaction.raw_private_leak_count, 0);

    let target = empty_store_platform(ProfileId::EspEmbeddedSdk);
    let before = target
        .replay_harness()
        .export_store_snapshot()
        .expect("before");
    let apply = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest { plan: preview.plan },
    );
    assert!(apply.is_err());
    let after = target
        .replay_harness()
        .export_store_snapshot()
        .expect("after");
    assert_eq!(before.state_fingerprint(), after.state_fingerprint());
    assert_eq!(before.event_fingerprint(), after.event_fingerprint());
}

#[test]
fn memory_space_plan_remains_bound_to_the_previewed_archive() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    source
        .replay_harness()
        .session_store()
        .append("chat-a", "user", "first snapshot")
        .expect("seed first");
    let first = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: scope("space-a"),
            include_private: false,
        },
    )
    .expect("first export");
    let first_preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_scope: scope("space-a"),
        target_scope: scope("space-b"),
        expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosStandaloneMemory,
        archive: first.archive,
    })
    .expect("preview migration");
    assert!(!first_preview.vault_preflight.passed);

    source
        .replay_harness()
        .session_store()
        .append("chat-a", "assistant", "second snapshot")
        .expect("seed second");
    let second = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: scope("space-a"),
            include_private: false,
        },
    )
    .expect("second export");
    assert_eq!(
        first_preview.vault_manifest.snapshot_fingerprint,
        second.export_report.state_fingerprint
    );

    let target = empty_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let apply = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest {
            plan: first_preview.plan,
        },
    )
    .expect_err("space remap is not implemented by snapshot import");
    assert_eq!(apply.stage(), "memory_space_migration");
}

#[test]
fn memory_runtime_exposes_vault_migration_preview_and_apply_methods() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    source
        .replay_harness()
        .session_store()
        .append("chat-a", "user", "runtime migration export")
        .expect("seed session");
    let source_runtime = test_runtime_with_scope(source, profile, "local", "chat-a");
    let source_scope = runtime_scope(&source_runtime);
    let exported = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: false,
        })
        .expect("runtime export");
    let preview = source_runtime
        .preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
            source_scope,
            target_scope: scope("space-runtime-copy"),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            source_profile: profile,
            target_profile: ProfileId::DesktopMacosStandaloneMemory,
            archive: exported.archive.clone(),
        })
        .expect("runtime preview");
    assert!(!preview.vault_preflight.passed);

    let target_runtime = test_runtime_with_scope(
        empty_store_platform(ProfileId::DesktopMacosStandaloneMemory),
        ProfileId::DesktopMacosStandaloneMemory,
        "local",
        "chat-a",
    );
    let apply_error = target_runtime
        .apply_memory_space_migration(MemorySpaceMigrateApplyRequest { plan: preview.plan })
        .expect_err("runtime apply must not target another mounted memory-space");
    assert_eq!(apply_error.stage(), "memory_runtime_memory_space_scope");
}

#[test]
fn memory_runtime_rejects_cross_space_export_and_import_before_replacement() {
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
            include_private: false,
        })
        .expect("runtime B export")
        .archive;
    let state_a_before = runtime_a
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope_a.clone(),
            include_private: false,
        })
        .expect("runtime A baseline export")
        .export_report
        .state_fingerprint;

    let cross_export = runtime_a
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope_b.clone(),
            include_private: false,
        })
        .expect_err("runtime A must not export runtime B scope");
    assert_eq!(cross_export.stage(), "memory_runtime_memory_space_scope");

    let cross_import = runtime_a
        .import_memory_space(MemorySpaceImportRequest {
            scope: scope_b,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: archive_b.clone(),
        })
        .expect_err("runtime A must not replace runtime B scope");
    assert_eq!(cross_import.stage(), "memory_runtime_memory_space_scope");

    let disguised_archive = runtime_a
        .import_memory_space(MemorySpaceImportRequest {
            scope: scope_a.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: archive_b,
        })
        .expect_err("runtime A must reject an archive declared for runtime B");
    assert_eq!(disguised_archive.stage(), "memory_space_import");

    let state_a_after = runtime_a
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope_a,
            include_private: false,
        })
        .expect("runtime A export after rejected imports")
        .export_report
        .state_fingerprint;
    assert_eq!(state_a_after, state_a_before);
}

#[test]
fn memory_space_export_without_private_redacts_private_layers() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    source
        .replay_harness()
        .private_garden_store()
        .write("chat-a", "journal/note.md", "private note", 1_800_000_000)
        .expect("private garden write");

    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: scope("space-main"),
            include_private: false,
        },
    )
    .expect("export");

    assert_eq!(exported.privacy_redactions, 0);
    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_scope: scope("space-main"),
        target_scope: scope("space-public"),
        expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        archive: exported.archive.clone(),
    })
    .expect("preview migration");
    assert_eq!(preview.privacy_redactions, 0);
    assert!(!preview.vault_preflight.passed);
    assert!(preview
        .manifest
        .privacy
        .iter()
        .all(|entry| entry.privacy_class != "private"));
    assert!(!exported.archive.contains_json_namespace("private_garden"));
    assert!(!exported.archive.contains_event_plane("private_garden"));
    assert_eq!(
        exported.export_report.json_docs,
        exported.archive.json_doc_count()
    );
}

#[test]
fn public_memory_space_export_rebuilds_governed_closure_after_private_owner_redaction() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(source.clone(), profile, "local", "chat-redaction");
    write_project_candidate(
        &runtime,
        "candidate-public",
        "public-project",
        "Public project memory survives public archive projection.",
    );
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "private-project".to_string(),
                    content: "Private project memory must not survive public archive projection."
                        .to_string(),
                    keywords: vec!["private-project".to_string()],
                    privacy: MemoryPrivacyClass::PrivateGarden,
                    source_chat_id: Some("chat-redaction".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["fixture:private-project".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed private long-term owner");
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: MemorySpaceScope {
                memory_space_id: runtime.memory_space_id().to_string(),
                mounted_subject_id: runtime.subject_id().to_string(),
            },
            include_private: false,
        },
    )
    .expect("public governed projection");
    assert!(exported.privacy_redactions > 0);

    let target = empty_store_platform(profile);
    import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: MemorySpaceScope {
                memory_space_id: runtime.memory_space_id().to_string(),
                mounted_subject_id: runtime.subject_id().to_string(),
            },
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: exported.archive,
        },
    )
    .expect("redacted governed projection remains importable");
    let snapshot = target
        .replay_harness()
        .export_store_snapshot()
        .expect("inspect imported projection");
    let long_term = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "long_term")
        .collect::<Vec<_>>();
    assert_eq!(long_term.len(), 1);
    assert!(long_term[0].value.to_string().contains("public-project"));
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| doc.value.to_string().contains("private-project")));
}
