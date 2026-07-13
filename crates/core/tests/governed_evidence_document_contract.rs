use bm_core::memory::{
    governed_evidence_document_content_digest, plan_governed_evidence_document_upsert,
    scoped_governed_evidence_document_key, validate_governed_evidence_document,
    validate_governed_evidence_document_draft, GovernedEvidenceDocumentChunk,
    GovernedEvidenceDocumentDraft, GovernedEvidenceDocumentPlan, GovernedEvidenceDocumentRejection,
    GovernedEvidenceDocumentSourceKind, MemoryEvidenceAuthority, MemoryPrivacyClass,
    MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES, MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES,
    MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS, MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES,
};

const NOW: u64 = 1_900_000_000;

fn fixture_draft() -> GovernedEvidenceDocumentDraft {
    let source_locator = "opaque://导入批次/α-7".to_string();
    let canonical_evidence_group = "opaque:recall-group:sha256:group-a".to_string();
    let body = "# 发布证据\n\n状态: 已完成\n负责人: 张三".to_string();
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:验收".to_string(),
        ordinal: 0,
        body: "- cargo test 通过\n- 设备验收通过".to_string(),
    }];
    let content_digest = governed_evidence_document_content_digest(
        &source_locator,
        &canonical_evidence_group,
        &body,
        &chunks,
    );
    GovernedEvidenceDocumentDraft {
        memory_space_id: "space-a".to_string(),
        mounted_subject_id: "subject-a".to_string(),
        document_id: "release/P7.4.1/evidence.json".to_string(),
        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
        source_locator,
        canonical_evidence_group,
        source_revision: 7,
        body,
        chunks,
        content_digest,
        authority: MemoryEvidenceAuthority::UserAsserted,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        observed_at: NOW - 20,
    }
}

fn refresh_digest(draft: &mut GovernedEvidenceDocumentDraft) {
    draft.content_digest = governed_evidence_document_content_digest(
        &draft.source_locator,
        &draft.canonical_evidence_group,
        &draft.body,
        &draft.chunks,
    );
}

fn create_fixture() -> bm_core::memory::GovernedEvidenceDocument {
    let GovernedEvidenceDocumentPlan::Created(document) =
        plan_governed_evidence_document_upsert(None, &fixture_draft(), NOW)
    else {
        panic!("fixture must create");
    };
    document
}

#[test]
fn physical_key_is_owned_by_memory_space_and_document_not_mounted_subject() {
    let key = scoped_governed_evidence_document_key("space-a", "doc-a").expect("valid scoped key");
    assert_eq!(
        key,
        scoped_governed_evidence_document_key("space-a", "doc-a")
            .expect("deterministic scoped key")
    );
    assert_ne!(
        key,
        scoped_governed_evidence_document_key("space-b", "doc-a").expect("space-isolated key")
    );

    let mut other_subject = fixture_draft();
    other_subject.mounted_subject_id = "subject-b".to_string();
    refresh_digest(&mut other_subject);
    let GovernedEvidenceDocumentPlan::Created(document) =
        plan_governed_evidence_document_upsert(None, &other_subject, NOW)
    else {
        panic!("other subject fixture must create");
    };
    assert_eq!(document.physical_key, create_fixture().physical_key);
}

#[test]
fn structured_utf8_material_creates_complete_governed_owner() {
    let draft = fixture_draft();
    assert_eq!(validate_governed_evidence_document_draft(&draft), Ok(()));

    let document = create_fixture();
    assert_eq!(document.owner_revision, 1);
    assert_eq!(document.source_revision, 7);
    assert_eq!(document.document_id, draft.document_id);
    assert_eq!(document.mounted_subject_id, draft.mounted_subject_id);
    assert_eq!(document.source_locator, draft.source_locator);
    assert_eq!(
        document.canonical_evidence_group,
        draft.canonical_evidence_group
    );
    assert_eq!(document.observed_at, draft.observed_at);
    assert_eq!(document.created_at, NOW);
    assert_eq!(document.updated_at, NOW);
    assert!(!document.shared_fact_surface_allowed());
    assert_eq!(validate_governed_evidence_document(&document), Ok(()));
}

#[test]
fn identity_body_digest_and_metadata_drift_fail_closed() {
    let mut draft = fixture_draft();
    draft.document_id = "  ".to_string();
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::EmptyDocumentId)
    );

    for field in ["memory_space_id", "mounted_subject_id", "document_id"] {
        let mut draft = fixture_draft();
        match field {
            "memory_space_id" => draft.memory_space_id = " space-a".to_string(),
            "mounted_subject_id" => draft.mounted_subject_id = "subject-a ".to_string(),
            "document_id" => draft.document_id = " release/P7.4.1/evidence.json ".to_string(),
            _ => unreachable!(),
        }
        assert_eq!(
            validate_governed_evidence_document_draft(&draft),
            Err(GovernedEvidenceDocumentRejection::NonCanonicalIdentity),
            "stable owner identity must reject whitespace aliases: {field}"
        );
    }

    let mut draft = fixture_draft();
    draft.body.clear();
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::EmptyBody)
    );

    let mut draft = fixture_draft();
    draft.content_digest = "0".repeat(64);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::DigestMismatch)
    );

    let existing = create_fixture();
    let original_digest = fixture_draft().content_digest;
    for drift in ["source_locator", "canonical_evidence_group", "observed_at"] {
        let mut changed = fixture_draft();
        match drift {
            "source_locator" => changed.source_locator.push_str("/drift"),
            "canonical_evidence_group" => changed.canonical_evidence_group.push_str(":drift"),
            "observed_at" => changed.observed_at += 1,
            _ => unreachable!(),
        }
        refresh_digest(&mut changed);
        if drift != "observed_at" {
            assert_ne!(
                original_digest, changed.content_digest,
                "lineage metadata must be covered by the digest: {drift}"
            );
        }
        assert_eq!(
            plan_governed_evidence_document_upsert(Some(&existing), &changed, NOW + 1),
            GovernedEvidenceDocumentPlan::Rejected(
                GovernedEvidenceDocumentRejection::SourceRevisionConflict
            ),
            "same revision metadata drift must conflict: {drift}"
        );
    }
}

#[test]
fn authority_uses_an_explicit_allowlist_and_privacy_uses_projection_gate() {
    for authority in [
        MemoryEvidenceAuthority::UserAsserted,
        MemoryEvidenceAuthority::RuntimeObservation,
        MemoryEvidenceAuthority::WorldObservation,
        MemoryEvidenceAuthority::ArchiveEvidence,
        MemoryEvidenceAuthority::ExternalContent,
        MemoryEvidenceAuthority::LegacyTranscript,
    ] {
        let mut draft = fixture_draft();
        draft.authority = authority;
        assert_eq!(validate_governed_evidence_document_draft(&draft), Ok(()));
    }

    for authority in [
        MemoryEvidenceAuthority::AssistantUtterance,
        MemoryEvidenceAuthority::AssistantSelfClaim,
        MemoryEvidenceAuthority::ProgramMemoryCanonical,
        MemoryEvidenceAuthority::SubjectProjection,
        MemoryEvidenceAuthority::SoulGovernance,
        MemoryEvidenceAuthority::PrivateGardenInternal,
        MemoryEvidenceAuthority::OperatorDiagnostic,
    ] {
        let mut draft = fixture_draft();
        draft.authority = authority;
        assert_eq!(
            validate_governed_evidence_document_draft(&draft),
            Err(GovernedEvidenceDocumentRejection::InvalidAuthority)
        );
    }

    for privacy in [
        MemoryPrivacyClass::PublicRuntime,
        MemoryPrivacyClass::SharedWithSubject,
    ] {
        let mut draft = fixture_draft();
        draft.privacy = privacy;
        assert_eq!(validate_governed_evidence_document_draft(&draft), Ok(()));
        let GovernedEvidenceDocumentPlan::Created(document) =
            plan_governed_evidence_document_upsert(None, &draft, NOW)
        else {
            panic!("projection-visible privacy must create");
        };
        assert!(!document.shared_fact_surface_allowed());
    }

    for privacy in [
        MemoryPrivacyClass::PrivateGarden,
        MemoryPrivacyClass::SoulPrivate,
        MemoryPrivacyClass::OperatorDiagnostic,
    ] {
        let mut draft = fixture_draft();
        draft.privacy = privacy;
        assert_eq!(
            validate_governed_evidence_document_draft(&draft),
            Err(GovernedEvidenceDocumentRejection::PrivacyNotProjectionVisible)
        );
    }
}

#[test]
fn chunks_are_ordered_unique_and_all_size_boundaries_are_enforced() {
    let mut draft = fixture_draft();
    draft.chunks.push(GovernedEvidenceDocumentChunk {
        identity: draft.chunks[0].identity.clone(),
        ordinal: 1,
        body: "duplicate".to_string(),
    });
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::DuplicateChunkIdentity)
    );

    let mut draft = fixture_draft();
    draft.chunks[0].ordinal = 1;
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::InvalidChunkOrdinal)
    );

    let mut reordered = fixture_draft();
    reordered.chunks = vec![
        GovernedEvidenceDocumentChunk {
            identity: "first".to_string(),
            ordinal: 0,
            body: "a".to_string(),
        },
        GovernedEvidenceDocumentChunk {
            identity: "second".to_string(),
            ordinal: 1,
            body: "b".to_string(),
        },
    ];
    refresh_digest(&mut reordered);
    let ordered_digest = reordered.content_digest.clone();
    reordered.chunks.swap(0, 1);
    assert_ne!(
        ordered_digest,
        governed_evidence_document_content_digest(
            &reordered.source_locator,
            &reordered.canonical_evidence_group,
            &reordered.body,
            &reordered.chunks
        )
    );

    let mut draft = fixture_draft();
    draft.body = "x".repeat(MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES + 1);
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::BodyTooLarge)
    );

    let mut draft = fixture_draft();
    draft.chunks[0].body = "x".repeat(MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES + 1);
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::ChunkTooLarge)
    );

    let mut draft = fixture_draft();
    draft.chunks = (0..=MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS)
        .map(|ordinal| GovernedEvidenceDocumentChunk {
            identity: format!("chunk-{ordinal}"),
            ordinal: ordinal as u32,
            body: "x".to_string(),
        })
        .collect();
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::TooManyChunks)
    );

    let mut draft = fixture_draft();
    draft.body = "x".repeat(MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES);
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::DocumentTooLarge)
    );
}

#[test]
fn revisions_and_timestamps_are_linear_and_source_lineage_is_stable() {
    let existing = create_fixture();
    let draft = fixture_draft();
    assert_eq!(
        plan_governed_evidence_document_upsert(Some(&existing), &draft, NOW + 1),
        GovernedEvidenceDocumentPlan::Noop
    );

    let mut conflict = draft.clone();
    conflict.body.push_str("\n冲突内容");
    refresh_digest(&mut conflict);
    assert_eq!(
        plan_governed_evidence_document_upsert(Some(&existing), &conflict, NOW + 1),
        GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::SourceRevisionConflict
        )
    );

    let mut newer = conflict;
    newer.source_revision += 1;
    newer.observed_at = NOW + 5;
    refresh_digest(&mut newer);
    let GovernedEvidenceDocumentPlan::Updated(updated) =
        plan_governed_evidence_document_upsert(Some(&existing), &newer, NOW + 2)
    else {
        panic!("newer source revision must update");
    };
    assert_eq!(updated.owner_revision, existing.owner_revision + 1);
    assert_eq!(updated.created_at, existing.created_at);
    assert!(updated.updated_at > existing.updated_at);
    assert!(updated.updated_at >= updated.observed_at);

    let mut different_lineage = newer.clone();
    different_lineage.source_locator.push_str("/other");
    refresh_digest(&mut different_lineage);
    assert_eq!(
        plan_governed_evidence_document_upsert(Some(&existing), &different_lineage, NOW + 10),
        GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::SourceLineageMismatch
        )
    );

    let mut older = draft;
    older.source_revision -= 1;
    assert_eq!(
        plan_governed_evidence_document_upsert(Some(&existing), &older, NOW + 1),
        GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::OlderSourceRevision
        )
    );
}

#[test]
fn persisted_document_rejects_invalid_owner_and_time_state() {
    let mut document = create_fixture();
    document.owner_revision = 0;
    assert_eq!(
        validate_governed_evidence_document(&document),
        Err(GovernedEvidenceDocumentRejection::InvalidOwnerRevision)
    );

    let mut document = create_fixture();
    document.updated_at = document.created_at - 1;
    assert_eq!(
        validate_governed_evidence_document(&document),
        Err(GovernedEvidenceDocumentRejection::InvalidTimestamps)
    );

    let mut document = create_fixture();
    document.updated_at = document.observed_at - 1;
    assert_eq!(
        validate_governed_evidence_document(&document),
        Err(GovernedEvidenceDocumentRejection::InvalidTimestamps)
    );
}
