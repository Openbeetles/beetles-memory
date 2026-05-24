mod support;

use bm_core::memory::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemoryWriteCandidate,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    apply_memory_space_migration, export_memory_space, import_memory_space,
    preview_memory_space_migration, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewRequest, MemoryWriteRequest,
    ProfileId,
};

use support::{empty_store_platform, test_runtime_with_scope};

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
        snapshot: exported.snapshot.clone(),
    });
    assert!(!preview.loss_risk);
    assert_eq!(
        preview.state_fingerprint,
        exported.export_report.state_fingerprint
    );

    let target = empty_store_platform(profile);
    let apply_report = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest {
            target_memory_space_id: "space-copy".to_string(),
            snapshot: exported.snapshot.clone(),
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
