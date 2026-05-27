mod support;

use bm_core::memory::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemoryWriteCandidate,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    apply_memory_space_migration, export_memory_space, import_memory_space,
    preview_memory_space_migration, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewRequest, MemoryWriteRequest,
    ProfileId,
};

use support::{empty_store_platform, test_runtime_with_scope};

fn llm_accept(target: MemoryCandidateTarget) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(target),
        reason: "llm_semantic_judgment".to_string(),
    }
}

#[test]
fn memory_space_export_preview_apply_and_import_use_public_sdk_contract() {
    let profile = ProfileId::ServerLinuxDevFull;
    let source = empty_store_platform(profile);
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
                semantic_judgment: Some(llm_accept(MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Project,
                    topic: "sdk_readiness".to_string(),
                })),
            }],
        })
        .expect("write candidate");

    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            memory_space_id: "space-main".to_string(),
            include_private: true,
        },
    )
    .expect("export");
    assert_eq!(exported.memory_space_id, "space-main");
    assert!(exported.export_report.json_docs > 0 || exported.export_report.events > 0);

    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: "space-main".to_string(),
        target_memory_space_id: "space-copy".to_string(),
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        snapshot: exported.snapshot.clone(),
    });
    assert!(!preview.loss_risk);
    assert_eq!(preview.manifest.source_memory_space_id, "space-main");
    assert_eq!(preview.manifest.target_memory_space_id, "space-copy");
    assert!(preview.manifest.whole_space_snapshot);
    assert!(preview.manifest.subject_remap.required);
    assert!(!preview.manifest.subject_remap.applied);
    assert!(preview
        .manifest
        .planes
        .iter()
        .any(|plane| plane.plane == "long_term" && plane.records > 0));
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

    let target = empty_store_platform(profile);
    let apply_report = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest {
            target_memory_space_id: "space-copy".to_string(),
            snapshot: exported.snapshot.clone(),
            preflight: preview.vault_preflight.clone(),
        },
    )
    .expect("apply");
    assert_eq!(apply_report.target_memory_space_id, "space-copy");
    assert_eq!(
        apply_report.import_report.state_fingerprint,
        exported.export_report.state_fingerprint
    );

    let imported = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            memory_space_id: "space-copy".to_string(),
            snapshot: exported.snapshot,
        },
    )
    .expect("import");
    assert_eq!(imported.memory_space_id, "space-copy");
}

#[test]
fn memory_space_apply_fails_closed_when_target_capability_preflight_fails() {
    let profile = ProfileId::ServerLinuxDevFull;
    let source = empty_store_platform(profile);
    source
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
            memory_space_id: "space-private".to_string(),
            include_private: true,
        },
    )
    .expect("export");
    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: "space-private".to_string(),
        target_memory_space_id: "space-esp".to_string(),
        source_profile: profile,
        target_profile: ProfileId::EspEmbeddedSdk,
        snapshot: exported.snapshot.clone(),
    });
    assert!(!preview.vault_preflight.passed);
    assert!(!preview.vault_preflight.capability_allowed);
    assert!(preview.vault_preflight.privacy_allowed);
    assert!(!preview.vault_redaction.redacted_refs.is_empty());
    assert_eq!(preview.vault_redaction.raw_private_leak_count, 0);

    let target = empty_store_platform(ProfileId::EspEmbeddedSdk);
    let before = target.export_store_snapshot().expect("before");
    let apply = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest {
            target_memory_space_id: "space-esp".to_string(),
            snapshot: exported.snapshot,
            preflight: preview.vault_preflight,
        },
    );
    assert!(apply.is_err());
    let after = target.export_store_snapshot().expect("after");
    assert_eq!(before.state_fingerprint(), after.state_fingerprint());
    assert_eq!(before.event_fingerprint(), after.event_fingerprint());
}

#[test]
fn memory_space_apply_rejects_stale_or_mismatched_vault_preflight() {
    let profile = ProfileId::ServerLinuxDevFull;
    let source = empty_store_platform(profile);
    source
        .session_store()
        .append("chat-a", "user", "first snapshot")
        .expect("seed first");
    let first = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            memory_space_id: "space-a".to_string(),
            include_private: false,
        },
    )
    .expect("first export");
    let first_preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: "space-a".to_string(),
        target_memory_space_id: "space-b".to_string(),
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosStandaloneMemory,
        snapshot: first.snapshot,
    });
    assert!(first_preview.vault_preflight.passed);

    source
        .session_store()
        .append("chat-a", "assistant", "second snapshot")
        .expect("seed second");
    let second = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            memory_space_id: "space-a".to_string(),
            include_private: false,
        },
    )
    .expect("second export");
    assert_ne!(
        first_preview.vault_manifest.snapshot_fingerprint,
        second.export_report.state_fingerprint
    );

    let target = empty_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let apply = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest {
            target_memory_space_id: "space-b".to_string(),
            snapshot: second.snapshot,
            preflight: first_preview.vault_preflight,
        },
    );
    assert!(apply.is_err());
}

#[test]
fn memory_runtime_exposes_vault_migration_preview_and_apply_methods() {
    let profile = ProfileId::ServerLinuxDevFull;
    let source = empty_store_platform(profile);
    source
        .session_store()
        .append("chat-a", "user", "runtime migration export")
        .expect("seed session");
    let source_runtime = test_runtime_with_scope(source, profile, "local", "chat-a");
    let exported = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            memory_space_id: "space-runtime".to_string(),
            include_private: false,
        })
        .expect("runtime export");
    let preview = source_runtime
        .preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
            source_memory_space_id: "space-runtime".to_string(),
            target_memory_space_id: "space-runtime-copy".to_string(),
            source_profile: profile,
            target_profile: ProfileId::DesktopMacosStandaloneMemory,
            snapshot: exported.snapshot.clone(),
        })
        .expect("runtime preview");
    assert!(preview.vault_preflight.passed);

    let target_runtime = test_runtime_with_scope(
        empty_store_platform(ProfileId::DesktopMacosStandaloneMemory),
        ProfileId::DesktopMacosStandaloneMemory,
        "local",
        "chat-a",
    );
    let applied = target_runtime
        .apply_memory_space_migration(MemorySpaceMigrateApplyRequest {
            target_memory_space_id: "space-runtime-copy".to_string(),
            snapshot: exported.snapshot,
            preflight: preview.vault_preflight,
        })
        .expect("runtime apply");
    assert_eq!(applied.target_memory_space_id, "space-runtime-copy");
}

#[test]
fn memory_space_export_without_private_redacts_private_layers() {
    let profile = ProfileId::ServerLinuxDevFull;
    let source = empty_store_platform(profile);
    source
        .private_garden_store()
        .write("chat-a", "journal/note.md", "private note", 1_800_000_000)
        .expect("private garden write");

    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            memory_space_id: "space-main".to_string(),
            include_private: false,
        },
    )
    .expect("export");

    assert!(exported.privacy_redactions > 0);
    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: "space-main".to_string(),
        target_memory_space_id: "space-public".to_string(),
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        snapshot: exported.snapshot.clone(),
    });
    assert_eq!(preview.privacy_redactions, 0);
    assert!(preview.vault_preflight.passed);
    assert!(preview
        .manifest
        .privacy
        .iter()
        .all(|entry| entry.privacy_class != "private"));
    assert!(exported
        .snapshot
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "private_garden"));
    assert!(exported
        .snapshot
        .events
        .iter()
        .all(|event| event.plane != "private_garden"));
    assert_eq!(
        exported.export_report.json_docs,
        exported.snapshot.json_docs.len()
    );
}
