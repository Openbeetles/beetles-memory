use bm_core::budget::{
    compile_runtime_budget, ProviderModelContextLimit, RuntimeBudgetInput, RuntimeBudgetReport,
    RuntimeStoreMedium, StaticPlatformManifest,
};
use bm_core::feature_gate::{ProfileId, TargetFeature};
use bm_core::memory::{
    canonical_recall_evidence_group, governed_evidence_document_content_digest,
    validate_governed_evidence_document_draft, GovernedEvidenceDocumentChunk,
    GovernedEvidenceDocumentDraft, GovernedEvidenceDocumentRejection,
    GovernedEvidenceDocumentSourceKind, MemoryEvidenceAuthority, MemoryPrivacyClass,
    MAX_EVIDENCE_DOCUMENT_FACET_LEXICAL_TERMS, MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES,
    MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES, MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS,
    MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES,
};
use bm_core::orchestrator::PressureLevel;
use bm_core::resource::{
    RuntimeResourceProbeSource, RuntimeResourceSnapshot, RuntimeResourceUnavailableReason,
};
use bm_core::EvidenceDocumentRuntimeBudget;

const PROFILES: [ProfileId; 11] = [
    ProfileId::EspStandaloneMemory,
    ProfileId::EspEmbeddedSdk,
    ProfileId::LinuxDeviceStandaloneMemory,
    ProfileId::DesktopMacosStandaloneMemory,
    ProfileId::DesktopMacosEmbeddedSdk,
    ProfileId::DesktopMacosDevFull,
    ProfileId::DesktopLinuxEmbeddedSdk,
    ProfileId::DesktopWindowsEmbeddedSdk,
    ProfileId::DesktopWindowsDevFull,
    ProfileId::ServerLinuxMemoryGateway,
    ProfileId::ServerLinuxDevFull,
];

fn compiler_fixture(profile: ProfileId) -> RuntimeBudgetInput {
    let medium = if profile.target() == TargetFeature::Esp {
        RuntimeStoreMedium::EmbeddedFlash
    } else {
        RuntimeStoreMedium::VolatileMemory
    };
    RuntimeBudgetInput {
        profile,
        resource_snapshot: RuntimeResourceSnapshot::unavailable(
            10,
            RuntimeResourceProbeSource::StaticManifest,
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        ),
        static_platform_manifest: StaticPlatformManifest::for_profile(profile, medium),
        provider_model_context_limit: None,
    }
}

fn evidence_document_fixture() -> GovernedEvidenceDocumentDraft {
    let source_locator = "opaque://导入批次/alpha".to_string();
    let canonical_evidence_group = canonical_recall_evidence_group("external:evidence:budget");
    let body = "正文-预算".to_string();
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "chunk:验收".to_string(),
        ordinal: 0,
        body: "分块正文".to_string(),
    }];
    let content_digest = governed_evidence_document_content_digest(
        &source_locator,
        &canonical_evidence_group,
        None,
        &body,
        &chunks,
    );
    GovernedEvidenceDocumentDraft {
        memory_space_id: "space-甲".to_string(),
        mounted_subject_id: "subject-乙".to_string(),
        document_id: "document-丙".to_string(),
        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
        source_locator,
        canonical_evidence_group,
        evidence_family_group: None,
        source_revision: 1,
        body,
        chunks,
        content_digest,
        authority: MemoryEvidenceAuthority::UserAsserted,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        observed_at: 1_900_000_000,
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

fn runtime_budget_at_pressure(profile: ProfileId, pressure: PressureLevel) -> RuntimeBudgetReport {
    let mut input = compiler_fixture(profile);
    input.resource_snapshot.pressure = pressure;
    input.resource_snapshot.memory_available_bytes = Some(
        input
            .static_platform_manifest
            .memory_floor_bytes
            .saturating_mul(3),
    );
    input.resource_snapshot.storage_available_bytes = Some(
        input
            .static_platform_manifest
            .storage_floor_bytes
            .saturating_mul(3),
    );
    input.resource_snapshot.unavailable_reason = None;
    input.resource_snapshot.unavailable_detail = None;
    compile_runtime_budget(input)
}

#[test]
fn every_profile_explicitly_owns_schema_bounded_evidence_document_budget() {
    for profile in PROFILES {
        let report = runtime_budget_at_pressure(profile, PressureLevel::Normal);
        let budget = report.evidence_document_budget;

        assert_eq!(
            budget.max_document_body_bytes,
            budget
                .max_total_bytes_per_transaction
                .min(MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES)
                .min(MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES),
            "document body budget must come from profile store capacity for {profile:?}",
        );
        assert_eq!(
            budget.max_chunk_bytes,
            budget
                .max_document_body_bytes
                .min(MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES),
            "chunk budget must remain below the document and schema ceilings for {profile:?}",
        );
        assert_eq!(
            budget.max_chunks_per_document,
            report
                .memory_core_budget
                .recall_working_set_max_items
                .min(MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS),
            "chunk count must come from the profile working set for {profile:?}",
        );
        let derived_entries_per_document = MAX_EVIDENCE_DOCUMENT_FACET_LEXICAL_TERMS
            .saturating_add(MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS)
            .saturating_add(1);
        assert_eq!(
            budget.max_documents_per_transaction,
            report
                .runtime_job_budget
                .maintenance_batch_max_items
                .min(report.store_budget.event_log_max_items)
                .min(report.store_budget.kv_max_entries / derived_entries_per_document)
                .max(1),
            "transaction document count must reserve profile kv capacity for {profile:?}",
        );
        assert_eq!(
            budget.max_documents_per_read,
            report
                .facet_recall_budget
                .max_facet_index_docs_read
                .min(budget.max_documents_per_transaction),
            "batch evidence reads must share the profile recall and owner ceilings for {profile:?}",
        );
        assert_eq!(
            budget.max_total_bytes_per_transaction,
            report.store_budget.snapshot_max_bytes / 2,
            "evidence input may consume at most half the profile snapshot for {profile:?}",
        );
        assert!(
            report
                .store_budget
                .snapshot_max_bytes
                .saturating_sub(budget.max_total_bytes_per_transaction)
                >= budget.max_total_bytes_per_transaction,
            "derived state must retain at least a one-to-one byte reserve for {profile:?}",
        );
        assert!(budget.max_document_body_bytes > 0);
        assert!(budget.max_chunk_bytes > 0);
        assert!(budget.max_chunks_per_document > 0);
        assert!(budget.max_documents_per_transaction > 0);
        assert!(budget.max_documents_per_read > 0);
        assert!(
            budget.max_total_bytes_per_transaction >= budget.max_document_body_bytes,
            "one allowed document must fit in a transaction for {profile:?}",
        );
    }
}

#[test]
fn evidence_document_budget_is_exported_from_the_bm_core_root() {
    fn assert_public_contract(_: EvidenceDocumentRuntimeBudget) {}

    assert_public_contract(
        runtime_budget_at_pressure(ProfileId::EspEmbeddedSdk, PressureLevel::Normal)
            .evidence_document_budget,
    );
}

#[test]
fn evidence_document_budget_tracks_resources_but_not_provider_context() {
    let normal = runtime_budget_at_pressure(ProfileId::EspEmbeddedSdk, PressureLevel::Normal);
    let critical = runtime_budget_at_pressure(ProfileId::EspEmbeddedSdk, PressureLevel::Critical);

    assert!(
        critical.evidence_document_budget.max_document_body_bytes
            < normal.evidence_document_budget.max_document_body_bytes
    );
    assert!(
        critical.evidence_document_budget.max_chunks_per_document
            < normal.evidence_document_budget.max_chunks_per_document
    );
    assert!(
        critical
            .evidence_document_budget
            .max_documents_per_transaction
            < normal
                .evidence_document_budget
                .max_documents_per_transaction
    );
    assert!(
        critical
            .evidence_document_budget
            .max_total_bytes_per_transaction
            < normal
                .evidence_document_budget
                .max_total_bytes_per_transaction
    );

    let mut provider_limited = compiler_fixture(ProfileId::ServerLinuxDevFull);
    let static_budget = compile_runtime_budget(provider_limited.clone());
    provider_limited.provider_model_context_limit = Some(ProviderModelContextLimit {
        provider: Some("local".to_string()),
        model: Some("bounded".to_string()),
        max_context_tokens: Some(128),
        max_prompt_chars: Some(512),
    });
    let provider_limited = compile_runtime_budget(provider_limited);

    assert_eq!(
        provider_limited.evidence_document_budget,
        static_budget.evidence_document_budget,
    );
    assert_ne!(
        provider_limited.projection_render_budget,
        static_budget.projection_render_budget,
    );
}

#[test]
fn exact_meter_counts_every_caller_controlled_persisted_utf8_field() {
    let draft = evidence_document_fixture();
    let expected = [
        draft.memory_space_id.as_str(),
        draft.mounted_subject_id.as_str(),
        draft.document_id.as_str(),
        draft.source_locator.as_str(),
        draft.canonical_evidence_group.as_str(),
        draft.body.as_str(),
        draft.content_digest.as_str(),
        draft.chunks[0].identity.as_str(),
        draft.chunks[0].body.as_str(),
    ]
    .into_iter()
    .map(str::len)
    .try_fold(0usize, usize::checked_add)
    .expect("fixture byte count must fit usize");

    assert_eq!(
        draft.checked_caller_controlled_persisted_bytes(0),
        Some(expected),
    );
    assert_eq!(
        draft.checked_caller_controlled_persisted_bytes(7),
        expected.checked_add(7),
        "the same function must support checked transaction accumulation",
    );
    assert_eq!(
        draft.checked_caller_controlled_persisted_bytes(usize::MAX),
        None,
        "transaction accumulation must fail closed on arithmetic overflow",
    );
    assert!(
        expected
            > draft.body.len()
                + draft
                    .chunks
                    .iter()
                    .map(|chunk| chunk.identity.len() + chunk.body.len())
                    .sum::<usize>(),
        "the legacy body/chunk-only meter must not remain the contract",
    );
}

#[test]
fn transaction_budget_reuses_the_exact_document_meter() {
    let first = evidence_document_fixture();
    let mut second = evidence_document_fixture();
    second.document_id = "document-丁".to_string();
    second.source_locator = "opaque://导入批次/beta".to_string();
    refresh_digest(&mut second);

    let first_bytes = first
        .checked_caller_controlled_persisted_bytes(0)
        .expect("first document byte count");
    let second_bytes = second
        .checked_caller_controlled_persisted_bytes(0)
        .expect("second document byte count");
    let expected = first_bytes
        .checked_add(second_bytes)
        .expect("expected transaction byte count");

    assert_eq!(
        second.checked_caller_controlled_persisted_bytes(first_bytes),
        Some(expected),
        "transaction accumulation must equal the checked sum of exact document bytes",
    );
}

#[test]
fn every_caller_controlled_string_is_bounded_before_persistence() {
    let cases = [
        (
            "memory_space_id",
            GovernedEvidenceDocumentDraft::MAX_MEMORY_SPACE_ID_BYTES,
            GovernedEvidenceDocumentRejection::MemorySpaceIdTooLarge,
        ),
        (
            "mounted_subject_id",
            GovernedEvidenceDocumentDraft::MAX_MOUNTED_SUBJECT_ID_BYTES,
            GovernedEvidenceDocumentRejection::MountedSubjectIdTooLarge,
        ),
        (
            "document_id",
            GovernedEvidenceDocumentDraft::MAX_DOCUMENT_ID_BYTES,
            GovernedEvidenceDocumentRejection::DocumentIdTooLarge,
        ),
        (
            "source_locator",
            GovernedEvidenceDocumentDraft::MAX_SOURCE_LOCATOR_BYTES,
            GovernedEvidenceDocumentRejection::SourceLocatorTooLarge,
        ),
        (
            "canonical_evidence_group",
            GovernedEvidenceDocumentDraft::MAX_CANONICAL_EVIDENCE_GROUP_BYTES,
            GovernedEvidenceDocumentRejection::CanonicalEvidenceGroupTooLarge,
        ),
        (
            "body",
            MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES,
            GovernedEvidenceDocumentRejection::BodyTooLarge,
        ),
        (
            "content_digest",
            GovernedEvidenceDocumentDraft::CONTENT_DIGEST_BYTES,
            GovernedEvidenceDocumentRejection::InvalidContentDigest,
        ),
        (
            "chunk_identity",
            GovernedEvidenceDocumentDraft::MAX_CHUNK_IDENTITY_BYTES,
            GovernedEvidenceDocumentRejection::ChunkIdentityTooLarge,
        ),
        (
            "chunk_body",
            MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES,
            GovernedEvidenceDocumentRejection::ChunkTooLarge,
        ),
    ];

    for (field, max_bytes, expected) in cases {
        let mut draft = evidence_document_fixture();
        let oversized = "x".repeat(max_bytes + 1);
        match field {
            "memory_space_id" => draft.memory_space_id = oversized,
            "mounted_subject_id" => draft.mounted_subject_id = oversized,
            "document_id" => draft.document_id = oversized,
            "source_locator" => draft.source_locator = oversized,
            "canonical_evidence_group" => draft.canonical_evidence_group = oversized,
            "body" => draft.body = oversized,
            "content_digest" => draft.content_digest = oversized,
            "chunk_identity" => draft.chunks[0].identity = oversized,
            "chunk_body" => draft.chunks[0].body = oversized,
            _ => unreachable!(),
        }
        if field != "content_digest" {
            refresh_digest(&mut draft);
        }
        assert_eq!(
            validate_governed_evidence_document_draft(&draft),
            Err(expected),
            "{field} must fail at core validation before store persistence",
        );
    }

    let mut utf8 = evidence_document_fixture();
    utf8.source_locator =
        "界".repeat(GovernedEvidenceDocumentDraft::MAX_SOURCE_LOCATOR_BYTES / 3 + 1);
    refresh_digest(&mut utf8);
    assert!(
        utf8.source_locator.chars().count()
            < GovernedEvidenceDocumentDraft::MAX_SOURCE_LOCATOR_BYTES
    );
    assert!(utf8.source_locator.len() > GovernedEvidenceDocumentDraft::MAX_SOURCE_LOCATOR_BYTES);
    assert_eq!(
        validate_governed_evidence_document_draft(&utf8),
        Err(GovernedEvidenceDocumentRejection::SourceLocatorTooLarge),
        "field limits must use UTF-8 bytes rather than Unicode scalar counts",
    );
}

#[test]
fn schema_document_limit_uses_the_exact_caller_controlled_meter() {
    let mut draft = evidence_document_fixture();
    let metadata_bytes = draft
        .checked_caller_controlled_persisted_bytes(0)
        .expect("fixture byte count")
        - draft.body.len();
    draft.body = "x".repeat(MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES - metadata_bytes + 1);
    refresh_digest(&mut draft);

    assert!(draft.body.len() <= MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES);
    assert_eq!(
        validate_governed_evidence_document_draft(&draft),
        Err(GovernedEvidenceDocumentRejection::DocumentTooLarge),
    );
}
