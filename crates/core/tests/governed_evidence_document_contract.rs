use bm_core::memory::{
    governed_evidence_document_content_digest, governed_evidence_source_ref_from_document,
    plan_governed_evidence_document_delete, plan_governed_evidence_document_upsert,
    scoped_governed_evidence_document_key, scoped_governed_evidence_source_ref_key,
    validate_governed_evidence_document, validate_governed_evidence_document_draft,
    validate_governed_evidence_source_ref, CanonicalRecallEvidenceFamilyGroup,
    GovernedEvidenceDocument, GovernedEvidenceDocumentChunk, GovernedEvidenceDocumentDeletePlan,
    GovernedEvidenceDocumentDraft, GovernedEvidenceDocumentPlan, GovernedEvidenceDocumentRejection,
    GovernedEvidenceDocumentSourceKind, MemoryEvidenceAuthority, MemoryPrivacyClass,
    MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES, MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES,
    MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS, MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES,
};

const NOW: u64 = 1_900_000_000;

fn fixture_draft() -> GovernedEvidenceDocumentDraft {
    let source_locator = "opaque://导入批次/α-7".to_string();
    let canonical_evidence_group =
        bm_core::memory::canonical_recall_evidence_group("external:evidence:group-a");
    let evidence_family_group = Some(
        CanonicalRecallEvidenceFamilyGroup::from_structured_identity("transcript:session-alpha")
            .expect("structured evidence family")
            .into_string(),
    );
    let body = "# 发布证据\n\n状态: 已完成\n负责人: 张三".to_string();
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:验收".to_string(),
        ordinal: 0,
        body: "- cargo test 通过\n- 设备验收通过".to_string(),
    }];
    let content_digest = governed_evidence_document_content_digest(
        &source_locator,
        &canonical_evidence_group,
        evidence_family_group.as_deref(),
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
        evidence_family_group,
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
        draft.evidence_family_group.as_deref(),
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
    assert_eq!(document.evidence_family_group, draft.evidence_family_group);
    assert_eq!(document.observed_at, draft.observed_at);
    assert_eq!(document.created_at, NOW);
    assert_eq!(document.updated_at, NOW);
    assert!(!document.shared_fact_surface_allowed());
    assert_eq!(validate_governed_evidence_document(&document), Ok(()));
}

#[test]
fn owner_material_closes_safe_binding_family_and_chunk_excerpt_once() {
    let document = create_fixture();
    let material = document.owner_material().expect("governed owner material");
    assert_eq!(material.owner_ref().owner_id, document.document_id);
    assert_eq!(material.evidence_bindings().len(), 1);
    let binding = &material.evidence_bindings()[0];
    assert_ne!(binding.safe_evidence_ref(), document.source_locator);
    assert!(!binding.safe_evidence_ref().contains("导入批次"));
    assert_eq!(
        binding.canonical_evidence_group(),
        document.canonical_evidence_group
    );
    assert_eq!(
        binding.evidence_family_group(),
        document.evidence_family_group.as_deref()
    );
    assert_eq!(
        binding.effective_evidence_family_group(),
        document.evidence_family_group.as_deref().expect("family")
    );

    let excerpt = document
        .select_lexical_excerpt("设备验收", 64)
        .expect("excerpt selection")
        .expect("bounded excerpt");
    assert_eq!(excerpt.content_digest(), document.content_digest);
    assert_eq!(excerpt.segment_identity(), "section:验收");
    assert!(excerpt.text().contains("设备验收通过"));
    assert!(excerpt.text().chars().count() <= 64);

    let mut no_family = fixture_draft();
    no_family.evidence_family_group = None;
    refresh_digest(&mut no_family);
    let GovernedEvidenceDocumentPlan::Created(no_family) =
        plan_governed_evidence_document_upsert(None, &no_family, NOW)
    else {
        panic!("family-less owner must create");
    };
    let no_family_material = no_family.owner_material().expect("family-less material");
    let no_family_binding = &no_family_material.evidence_bindings()[0];
    assert_eq!(no_family_binding.evidence_family_group(), None);
    assert_eq!(
        no_family_binding.effective_evidence_family_group(),
        no_family_binding.canonical_evidence_group()
    );
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
    draft.canonical_evidence_group = "not-a-canonical-group".to_string();
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::InvalidCanonicalEvidenceGroup)
    );

    let mut draft = fixture_draft();
    draft.evidence_family_group = Some("session-alpha".to_string());
    refresh_digest(&mut draft);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::InvalidEvidenceFamilyGroup)
    );

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
    for drift in [
        "source_locator",
        "canonical_evidence_group",
        "evidence_family_group",
        "observed_at",
    ] {
        let mut changed = fixture_draft();
        match drift {
            "source_locator" => changed.source_locator.push_str("/drift"),
            "canonical_evidence_group" => {
                changed.canonical_evidence_group =
                    bm_core::memory::canonical_recall_evidence_group("external:evidence:drift")
            }
            "evidence_family_group" => {
                changed.evidence_family_group = Some(
                    CanonicalRecallEvidenceFamilyGroup::from_structured_identity(
                        "transcript:session-beta",
                    )
                    .expect("different structured evidence family")
                    .into_string(),
                )
            }
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
            reordered.evidence_family_group.as_deref(),
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

    let mut different_subject = newer.clone();
    different_subject.mounted_subject_id = "agent:other-subject".to_string();
    assert_eq!(
        plan_governed_evidence_document_upsert(Some(&existing), &different_subject, NOW + 10),
        GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::MountedSubjectMismatch
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
    document.schema_version = 0;
    assert_eq!(
        validate_governed_evidence_document(&document),
        Err(GovernedEvidenceDocumentRejection::InvalidSchemaVersion)
    );

    let mut old_shape = serde_json::to_value(create_fixture()).expect("serialize evidence owner");
    old_shape
        .as_object_mut()
        .expect("evidence owner object")
        .remove("schema_version");
    assert!(serde_json::from_value::<GovernedEvidenceDocument>(old_shape).is_err());

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

#[test]
fn delete_requires_a_valid_exact_owner_revision() {
    let existing = create_fixture();

    assert_eq!(
        plan_governed_evidence_document_delete(None, existing.owner_revision),
        GovernedEvidenceDocumentDeletePlan::Rejected(
            GovernedEvidenceDocumentRejection::OwnerDocumentMissing
        )
    );
    assert_eq!(
        plan_governed_evidence_document_delete(Some(&existing), 0),
        GovernedEvidenceDocumentDeletePlan::Rejected(
            GovernedEvidenceDocumentRejection::InvalidOwnerRevision
        )
    );
    assert_eq!(
        plan_governed_evidence_document_delete(Some(&existing), existing.owner_revision + 1),
        GovernedEvidenceDocumentDeletePlan::Rejected(
            GovernedEvidenceDocumentRejection::OwnerRevisionConflict
        )
    );
    assert_eq!(
        plan_governed_evidence_document_delete(Some(&existing), existing.owner_revision),
        GovernedEvidenceDocumentDeletePlan::Deleted
    );
}

#[test]
fn derived_source_ref_is_typed_owner_bound_and_locator_free() {
    let document = create_fixture();
    let source_ref =
        governed_evidence_source_ref_from_document(&document).expect("typed evidence source ref");

    assert_eq!(source_ref.owner_ref.owner_id, document.document_id);
    assert_eq!(source_ref.owner_revision, document.owner_revision);
    assert_eq!(source_ref.source_revision, document.source_revision);
    assert_eq!(source_ref.content_digest, document.content_digest);
    assert_eq!(
        source_ref.physical_key,
        scoped_governed_evidence_source_ref_key(
            &document.memory_space_id,
            &document.mounted_subject_id,
            document.source_kind,
            &document.source_locator,
            document.source_revision,
        )
        .expect("source ref key")
    );
    assert_eq!(
        validate_governed_evidence_source_ref(&document, &source_ref),
        Ok(())
    );
    let encoded = serde_json::to_value(&source_ref).expect("source ref json");
    assert_eq!(
        encoded["source_locator_digest"]
            .as_str()
            .expect("source locator digest")
            .len(),
        64
    );
    assert!(encoded.get("source_locator").is_none());
    assert!(!encoded.to_string().contains(&document.source_locator));
}
