use bm_core::{
    EvidenceState, MemoryPlane, MentalPrivacyLayer, ProjectionSurface, PromptRecallIntent,
    RecallQuery, RuntimeProfile, SubjectAssemblySource, WriteCandidate, WriteDecision,
    WriteRejectReason,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn private_material_is_rejected_before_it_can_reach_prompt_projection() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();
    let raw_private = "RAW PRIVATE MATERIAL SHOULD NOT MOVE";

    let private = runtime.write(
        WriteCandidate::new("agent:s3", "task:s3:private", raw_private)
            .source("operator:private-note")
            .plane_hint(MemoryPlane::SubjectProjection)
            .privacy_layer(MentalPrivacyLayer::Private),
    );

    assert_eq!(private.decision, WriteDecision::Rejected);
    assert_eq!(
        private.governance.reject_reason,
        Some(WriteRejectReason::RawPrivateRejected)
    );

    let recall = runtime.recall(
        RecallQuery::new("task:s3:private")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Prompt);

    assert!(projection.blocks.is_empty());
    assert_eq!(projection.privacy_filtered_count, 0);
}

#[test]
fn soul_governance_summary_supports_subject_assembly_without_raw_prompt_exposure() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();
    let summary = "relationship boundary summary: answer from governed shared commitments";

    let soul = runtime.write(
        WriteCandidate::new("agent:s3", "task:s3:soul", summary)
            .source("operator:soul-summary")
            .plane_hint(MemoryPlane::SoulGovernance)
            .privacy_layer(MentalPrivacyLayer::Relational)
            .evidence(EvidenceState::Supported)
            .canonical(true),
    );

    assert_eq!(soul.decision, WriteDecision::Accepted);

    let recall = runtime.recall(
        RecallQuery::new("task:s3:soul")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SoulGovernance),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Prompt);

    assert_eq!(projection.blocks.len(), 1);
    assert_eq!(projection.privacy_filtered_count, 1);
    assert!(!projection.blocks[0].content.contains(summary));
    assert!(projection.blocks[0].content.contains("presence"));
    let assembly = projection
        .subject_assembly
        .expect("soul governance should produce subject assembly report");
    assert!(assembly.mounted);
    assert!(assembly
        .sources_used
        .iter()
        .any(|source| source.source == SubjectAssemblySource::SelfCore));
}

#[test]
fn esp_compact_rejects_soul_governance_but_projects_trimmed_subject_projection() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::EspCompact)
        .store(store)
        .build();

    let soul = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:esp",
            "compact must not carry thick soul state",
        )
        .source("operator:soul-summary")
        .plane_hint(MemoryPlane::SoulGovernance)
        .privacy_layer(MentalPrivacyLayer::Shared),
    );
    assert_eq!(soul.decision, WriteDecision::Rejected);
    assert_eq!(
        soul.governance.reject_reason,
        Some(WriteRejectReason::ProfileRejected)
    );

    let subject = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:esp",
            "compact subject projection ".repeat(80),
        )
        .source("replay:subject-frame")
        .plane_hint(MemoryPlane::SubjectProjection)
        .privacy_layer(MentalPrivacyLayer::Shared)
        .evidence(EvidenceState::Supported),
    );
    assert_eq!(subject.decision, WriteDecision::Accepted);

    let recall = runtime.recall(
        RecallQuery::new("task:s3:esp")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Prompt);

    assert_eq!(projection.blocks.len(), 1);
    assert_eq!(projection.blocks[0].plane, MemoryPlane::SubjectProjection);
    assert!(
        projection.blocks[0].content.len() <= RuntimeProfile::EspCompact.projection_budget_bytes()
    );
}

#[test]
fn archive_evidence_cannot_become_soul_governance_without_distillation() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let archive_to_soul = runtime.write(
        WriteCandidate::new("agent:s3", "task:s3:archive", "archive evidence says maybe")
            .source("archive:conversation-1")
            .plane_hint(MemoryPlane::SoulGovernance)
            .evidence(EvidenceState::ArchiveOnly)
            .canonical(true),
    );

    assert_eq!(archive_to_soul.decision, WriteDecision::Rejected);
    assert_eq!(
        archive_to_soul.governance.reject_reason,
        Some(WriteRejectReason::NeedsDistillation)
    );
}

#[test]
fn program_memory_supports_subject_assembly_without_becoming_soul_core() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let procedural = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:program",
            "下次处理 S3 主体挂载时，先检查 soul governance 再组装 subject projection。",
        )
        .source("task-learning:s3")
        .evidence(EvidenceState::Supported),
    );
    assert_eq!(procedural.decision, WriteDecision::Accepted);

    let program_to_soul = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:program",
            "下次处理主体挂载时，把程序证据直接写成灵魂核心。",
        )
        .source("task-learning:s3")
        .plane_hint(MemoryPlane::SoulGovernance)
        .evidence(EvidenceState::Supported),
    );
    assert_eq!(program_to_soul.decision, WriteDecision::Rejected);
    assert_eq!(
        program_to_soul.governance.reject_reason,
        Some(WriteRejectReason::NeedsDistillation)
    );

    let recall = runtime.recall(
        RecallQuery::new("task:s3:program")
            .intent(PromptRecallIntent::Procedural)
            .plane(MemoryPlane::Procedural),
    );
    let projection = runtime.project(&recall, ProjectionSurface::ToolContext);
    let assembly = projection
        .subject_assembly
        .expect("program evidence should support subject assembly report");

    assert!(assembly
        .sources_used
        .iter()
        .any(|source| source.source == SubjectAssemblySource::ProgramMemory));
    assert!(assembly
        .sources_used
        .iter()
        .all(|source| source.source != SubjectAssemblySource::SelfCore));
}
