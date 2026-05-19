//! Replay fixtures for Beetle Memory.

use bm_core::{
    EvidenceState, MemoryPlane, MentalPrivacyLayer, ProjectionReport, ProjectionSurface,
    PromptRecallIntent, RecallSelectionReport, RecallWarning, RuntimeProfile,
    SubjectAssemblySource, WriteCandidate, WriteDecision, WriteRejectReason, WriteReport,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayReport {
    pub write_accepted: usize,
    pub recall_selected: usize,
    pub projection_blocks: usize,
    pub profile: String,
}

pub fn run_basic_replay() -> ReplayReport {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let write = runtime.write(
        WriteCandidate::new("agent:replay", "task:replay", "replay fact")
            .source("replay:basic")
            .plane_hint(MemoryPlane::SharedFactual),
    );
    let recall = runtime.recall(
        bm_core::RecallQuery::new("task:replay")
            .intent(PromptRecallIntent::Factual)
            .plane(MemoryPlane::SharedFactual)
            .limit(2),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Prompt);

    ReplayReport {
        write_accepted: usize::from(write.record_id.is_some()),
        recall_selected: recall.selected.len(),
        projection_blocks: projection.blocks.len(),
        profile: RuntimeProfile::DevFull.as_str().to_owned(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum S1ReplayPath {
    FactualPromptProjection,
    ProceduralAdapterProjection,
    ArchiveEvidenceNeedsDistillation,
    SoulGovernanceSubjectProjection,
    EspCompactProjectionTrim,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S1ReplayPathReport {
    pub path: S1ReplayPath,
    pub profile: RuntimeProfile,
    pub selected_planes: Vec<MemoryPlane>,
    pub projected_planes: Vec<MemoryPlane>,
    pub rejected_reasons: Vec<WriteRejectReason>,
    pub canonical_projection: bool,
    pub privacy_filtered: bool,
    pub raw_private_exposed: bool,
    pub projection_trimmed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S1ReplayReport {
    pub accepted: usize,
    pub rejected: usize,
    pub selected: usize,
    pub projected: usize,
    pub warnings: Vec<String>,
    pub paths: Vec<S1ReplayPathReport>,
}

pub fn run_s1_replay() -> S1ReplayReport {
    let mut report = S1ReplayReport {
        accepted: 0,
        rejected: 0,
        selected: 0,
        projected: 0,
        warnings: Vec::new(),
        paths: Vec::new(),
    };

    replay_dev_full_paths(&mut report);
    replay_esp_compact_trim(&mut report);
    report
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum S3ReplayPath {
    SoulGovernanceSummaryFeedsSubjectProjection,
    ProgramEvidenceSupportsSubjectAssembly,
    PrivateMaterialFilteredFromPrompt,
    OperatorInspectionShowsPresenceOnly,
    EspCompactAcceptsSubjectProjectionButRejectsSoulGovernance,
    ArchiveEvidenceCannotBecomeSoulCore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S3ReplayPathReport {
    pub path: S3ReplayPath,
    pub profile: RuntimeProfile,
    pub selected_planes: Vec<MemoryPlane>,
    pub projected_planes: Vec<MemoryPlane>,
    pub rejected_reasons: Vec<WriteRejectReason>,
    pub subject_assembly_sources_used: Vec<&'static str>,
    pub subject_assembly_sources_missing: Vec<&'static str>,
    pub privacy_filtered: usize,
    pub raw_private_exposed: bool,
    pub profile_trimmed: bool,
    pub operator_presence_only: bool,
    pub inspection_private_content_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S3ReplayReport {
    pub accepted: usize,
    pub rejected: usize,
    pub deferred: usize,
    pub selected: usize,
    pub skipped: usize,
    pub projected: usize,
    pub privacy_filtered: usize,
    pub subject_assembly_sources_used: Vec<&'static str>,
    pub subject_assembly_sources_missing: Vec<&'static str>,
    pub profile_trim_warning: bool,
    pub raw_private_exposed: bool,
    pub paths: Vec<S3ReplayPathReport>,
}

pub fn run_s3_replay() -> S3ReplayReport {
    let mut report = S3ReplayReport {
        accepted: 0,
        rejected: 0,
        deferred: 0,
        selected: 0,
        skipped: 0,
        projected: 0,
        privacy_filtered: 0,
        subject_assembly_sources_used: Vec::new(),
        subject_assembly_sources_missing: Vec::new(),
        profile_trim_warning: false,
        raw_private_exposed: false,
        paths: Vec::new(),
    };

    replay_s3_soul_governance_summary(&mut report);
    replay_s3_program_evidence_subject_assembly(&mut report);
    replay_s3_private_prompt_filter(&mut report);
    replay_s3_operator_inspection_presence(&mut report);
    replay_s3_esp_compact_profile_gate(&mut report);
    replay_s3_archive_evidence_soul_core_rejection(&mut report);
    report
}

fn replay_s3_soul_governance_summary(report: &mut S3ReplayReport) {
    let raw_private = "RAW PRIVATE SOUL MATERIAL";
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let soul_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:soul-summary",
            "governed soul summary: identity boundary is stable; private material is presence-only",
        )
        .source("replay:soul-governance")
        .plane_hint(MemoryPlane::SoulGovernance)
        .privacy_layer(MentalPrivacyLayer::Relational)
        .evidence(EvidenceState::Supported)
        .canonical(true),
    );
    let subject_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:soul-summary",
            "subject projection mounted from governed summary; private source is redacted",
        )
        .source("replay:subject-projection")
        .plane_hint(MemoryPlane::SubjectProjection),
    );

    let soul_recall = runtime.recall(
        bm_core::RecallQuery::new("task:s3:soul-summary")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SoulGovernance),
    );
    let subject_recall = runtime.recall(
        bm_core::RecallQuery::new("task:s3:soul-summary")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let mixed_recall = runtime.recall(
        bm_core::RecallQuery::new("task:s3:soul-summary")
            .intent(PromptRecallIntent::Continuity)
            .limit(4),
    );
    let projection = runtime.project(&mixed_recall, ProjectionSurface::Prompt);

    record_s3_path(
        report,
        S3PathRecordInput {
            path: S3ReplayPath::SoulGovernanceSummaryFeedsSubjectProjection,
            profile: RuntimeProfile::DevFull,
            writes: vec![soul_write, subject_write],
            recalls: vec![&soul_recall, &subject_recall, &mixed_recall],
            projections: vec![&projection],
            profile_trimmed: false,
            operator_presence_only: false,
            inspection_private_content_bytes: projected_private_content_bytes(
                &projection,
                raw_private,
            ),
            raw_private_probe: Some(raw_private),
        },
    );
}

fn replay_s3_program_evidence_subject_assembly(report: &mut S3ReplayReport) {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();
    let scope = "task:s3:program-evidence";

    let factual_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            scope,
            "shared factual evidence supports the current task boundary",
        )
        .source("replay:shared-factual")
        .plane_hint(MemoryPlane::SharedFactual),
    );
    let continuity_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            scope,
            "continuity capsule keeps the current subject assembly stable",
        )
        .source("replay:continuity")
        .plane_hint(MemoryPlane::ContinuityCapsule),
    );
    let procedural_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            scope,
            "下次组装主体挂载帧时，先读取 program evidence，再生成 subject assembly report。",
        )
        .source("task-learning:s3")
        .plane_hint(MemoryPlane::Procedural),
    );
    let task_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            scope,
            "current task context requires S3 soul and subject contract verification",
        )
        .source("task:s3")
        .plane_hint(MemoryPlane::TaskRecall),
    );
    let program_to_soul = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            scope,
            "program evidence cannot be written directly as soul governance",
        )
        .source("task-learning:s3")
        .plane_hint(MemoryPlane::SoulGovernance),
    );
    let subject_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            scope,
            "subject assembly uses program evidence without promoting it into soul governance",
        )
        .source("replay:subject-assembly")
        .plane_hint(MemoryPlane::SubjectProjection),
    );

    let recall = runtime.recall(
        bm_core::RecallQuery::new(scope)
            .intent(PromptRecallIntent::Mixed)
            .limit(8),
    );
    let subject_recall = runtime.recall(
        bm_core::RecallQuery::new(scope)
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let skip_probe_recall = runtime.recall(
        bm_core::RecallQuery::new(scope)
            .identity("agent:s3:other")
            .intent(PromptRecallIntent::Mixed)
            .limit(1),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Replay);

    record_s3_path(
        report,
        S3PathRecordInput {
            path: S3ReplayPath::ProgramEvidenceSupportsSubjectAssembly,
            profile: RuntimeProfile::DevFull,
            writes: vec![
                factual_write,
                continuity_write,
                procedural_write,
                task_write,
                program_to_soul,
                subject_write,
            ],
            recalls: vec![&recall, &subject_recall, &skip_probe_recall],
            projections: vec![&projection],
            profile_trimmed: false,
            operator_presence_only: false,
            inspection_private_content_bytes: 0,
            raw_private_probe: None,
        },
    );
}

fn replay_s3_private_prompt_filter(report: &mut S3ReplayReport) {
    let raw_private = "RAW PRIVATE PROMPT MATERIAL";
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let private_write = runtime.write(
        WriteCandidate::new("agent:s3", "task:s3:private-filter", raw_private)
            .source("replay:private-raw")
            .plane_hint(MemoryPlane::SubjectProjection)
            .privacy_layer(MentalPrivacyLayer::Private),
    );
    let summary_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:private-filter",
            "private material present; prompt projection receives governed summary only",
        )
        .source("replay:private-summary")
        .plane_hint(MemoryPlane::SubjectProjection),
    );
    let recall = runtime.recall(
        bm_core::RecallQuery::new("task:s3:private-filter")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Prompt);

    record_s3_path(
        report,
        S3PathRecordInput {
            path: S3ReplayPath::PrivateMaterialFilteredFromPrompt,
            profile: RuntimeProfile::DevFull,
            writes: vec![private_write, summary_write],
            recalls: vec![&recall],
            projections: vec![&projection],
            profile_trimmed: false,
            operator_presence_only: false,
            inspection_private_content_bytes: projected_private_content_bytes(
                &projection,
                raw_private,
            ),
            raw_private_probe: Some(raw_private),
        },
    );
}

fn replay_s3_operator_inspection_presence(report: &mut S3ReplayReport) {
    let raw_private = "RAW OPERATOR PRIVATE MATERIAL";
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:operator-inspection",
            "private subject source is present; policy=summary-only; reason=privacy-filtered",
        )
        .source("replay:operator-presence")
        .plane_hint(MemoryPlane::SubjectProjection),
    );
    let recall = runtime.recall(
        bm_core::RecallQuery::new("task:s3:operator-inspection")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let projection = runtime.project(&recall, ProjectionSurface::OperatorInspection);

    record_s3_path(
        report,
        S3PathRecordInput {
            path: S3ReplayPath::OperatorInspectionShowsPresenceOnly,
            profile: RuntimeProfile::DevFull,
            writes: vec![write],
            recalls: vec![&recall],
            projections: vec![&projection],
            profile_trimmed: false,
            operator_presence_only: true,
            inspection_private_content_bytes: projected_private_content_bytes(
                &projection,
                raw_private,
            ),
            raw_private_probe: Some(raw_private),
        },
    );
}

fn replay_s3_esp_compact_profile_gate(report: &mut S3ReplayReport) {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::EspCompact)
        .store(store)
        .build();

    let subject_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:esp-compact",
            "compact subject projection ".repeat(40),
        )
        .source("replay:esp-subject")
        .plane_hint(MemoryPlane::SubjectProjection),
    );
    let soul_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:esp-compact",
            "thick soul governance record is not accepted by compact profile",
        )
        .source("replay:esp-soul")
        .plane_hint(MemoryPlane::SoulGovernance),
    );
    let recall = runtime.recall(
        bm_core::RecallQuery::new("task:s3:esp-compact")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Prompt);
    let profile_trimmed = recall.warnings.iter().any(|warning| {
        matches!(
            warning,
            RecallWarning::ProfileBudgetTrimmed {
                profile: RuntimeProfile::EspCompact,
                ..
            }
        )
    });

    record_s3_path(
        report,
        S3PathRecordInput {
            path: S3ReplayPath::EspCompactAcceptsSubjectProjectionButRejectsSoulGovernance,
            profile: RuntimeProfile::EspCompact,
            writes: vec![subject_write, soul_write],
            recalls: vec![&recall],
            projections: vec![&projection],
            profile_trimmed,
            operator_presence_only: false,
            inspection_private_content_bytes: 0,
            raw_private_probe: None,
        },
    );
}

fn replay_s3_archive_evidence_soul_core_rejection(report: &mut S3ReplayReport) {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let archive_write = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:archive-soul-core",
            "archive evidence can support assembly but cannot become canonical soul core",
        )
        .source("archive:s3-evidence")
        .plane_hint(MemoryPlane::ArchiveEvidence),
    );
    let archive_promotion = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:archive-soul-core",
            "archive evidence can support assembly but cannot become canonical soul core",
        )
        .source("archive:s3-evidence")
        .plane_hint(MemoryPlane::SharedFactual),
    );
    let archive_soul_promotion = runtime.write(
        WriteCandidate::new(
            "agent:s3",
            "task:s3:archive-soul-core",
            "archive evidence can support assembly but cannot become canonical soul core",
        )
        .source("archive:s3-evidence")
        .plane_hint(MemoryPlane::SoulGovernance),
    );
    let recall = runtime.recall(
        bm_core::RecallQuery::new("task:s3:archive-soul-core")
            .intent(PromptRecallIntent::Evidence)
            .plane(MemoryPlane::ArchiveEvidence),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Replay);

    record_s3_path(
        report,
        S3PathRecordInput {
            path: S3ReplayPath::ArchiveEvidenceCannotBecomeSoulCore,
            profile: RuntimeProfile::DevFull,
            writes: vec![archive_write, archive_promotion, archive_soul_promotion],
            recalls: vec![&recall],
            projections: vec![&projection],
            profile_trimmed: false,
            operator_presence_only: false,
            inspection_private_content_bytes: 0,
            raw_private_probe: None,
        },
    );
}

struct S3PathRecordInput<'a> {
    path: S3ReplayPath,
    profile: RuntimeProfile,
    writes: Vec<WriteReport>,
    recalls: Vec<&'a RecallSelectionReport>,
    projections: Vec<&'a ProjectionReport>,
    profile_trimmed: bool,
    operator_presence_only: bool,
    inspection_private_content_bytes: usize,
    raw_private_probe: Option<&'a str>,
}

fn record_s3_path(report: &mut S3ReplayReport, input: S3PathRecordInput<'_>) {
    let accepted = input
        .writes
        .iter()
        .filter(|write| write.decision == WriteDecision::Accepted)
        .count();
    let rejected = input
        .writes
        .iter()
        .filter(|write| write.decision == WriteDecision::Rejected)
        .count();
    let deferred = input
        .writes
        .iter()
        .filter(|write| write.decision == WriteDecision::Deferred)
        .count();
    let rejected_reasons = input
        .writes
        .iter()
        .filter_map(|write| write.governance.reject_reason)
        .collect::<Vec<_>>();
    let selected = input
        .recalls
        .iter()
        .map(|recall| recall.selected.len())
        .sum::<usize>();
    let skipped = input
        .recalls
        .iter()
        .map(|recall| recall.skipped.len())
        .sum::<usize>();
    let projected = input
        .projections
        .iter()
        .map(|projection| projection.blocks.len())
        .sum::<usize>();
    let privacy_filtered = input
        .projections
        .iter()
        .map(|projection| projection.privacy_filtered_count)
        .sum::<usize>();
    let raw_private_exposed = input.raw_private_probe.is_some_and(|probe| {
        input.projections.iter().any(|projection| {
            projection
                .blocks
                .iter()
                .any(|block| block.content.contains(probe))
        })
    });
    let selected_planes = input
        .recalls
        .iter()
        .flat_map(|recall| recall.selected.iter().map(|selection| selection.plane))
        .collect::<Vec<_>>();
    let projected_planes = input
        .projections
        .iter()
        .flat_map(|projection| projection.blocks.iter().map(|block| block.plane))
        .collect::<Vec<_>>();
    let mut subject_assembly_sources_used = Vec::new();
    let mut subject_assembly_sources_missing = Vec::new();
    for projection in &input.projections {
        if let Some(assembly) = &projection.subject_assembly {
            for source in &assembly.sources_used {
                extend_unique_value(
                    &mut subject_assembly_sources_used,
                    subject_assembly_source_name(source.source),
                );
            }
            for source in &assembly.sources_missing {
                extend_unique_value(
                    &mut subject_assembly_sources_missing,
                    subject_assembly_source_name(*source),
                );
            }
        }
    }

    report.accepted += accepted;
    report.rejected += rejected;
    report.deferred += deferred;
    report.selected += selected;
    report.skipped += skipped;
    report.projected += projected;
    report.privacy_filtered += privacy_filtered;
    report.profile_trim_warning |= input.profile_trimmed;
    report.raw_private_exposed |= raw_private_exposed;
    extend_unique(
        &mut report.subject_assembly_sources_used,
        &subject_assembly_sources_used,
    );
    extend_unique(
        &mut report.subject_assembly_sources_missing,
        &subject_assembly_sources_missing,
    );
    report.paths.push(S3ReplayPathReport {
        path: input.path,
        profile: input.profile,
        selected_planes,
        projected_planes,
        rejected_reasons,
        subject_assembly_sources_used,
        subject_assembly_sources_missing,
        privacy_filtered,
        raw_private_exposed,
        profile_trimmed: input.profile_trimmed,
        operator_presence_only: input.operator_presence_only,
        inspection_private_content_bytes: input.inspection_private_content_bytes,
    });
}

fn projected_private_content_bytes(projection: &ProjectionReport, private_probe: &str) -> usize {
    projection
        .blocks
        .iter()
        .filter(|block| block.content.contains(private_probe))
        .map(|block| block.content.len())
        .sum()
}

fn extend_unique(target: &mut Vec<&'static str>, source: &[&'static str]) {
    for value in source {
        extend_unique_value(target, value);
    }
}

fn extend_unique_value(target: &mut Vec<&'static str>, value: &'static str) {
    if !target.contains(&value) {
        target.push(value);
    }
}

fn subject_assembly_source_name(source: SubjectAssemblySource) -> &'static str {
    match source {
        SubjectAssemblySource::SelfCore => "SelfCore",
        SubjectAssemblySource::SelfContinuity => "SelfContinuity",
        SubjectAssemblySource::Relationship => "Relationship",
        SubjectAssemblySource::ProgramMemory => "ProgramMemory",
        SubjectAssemblySource::World => "World",
        SubjectAssemblySource::Task => "Task",
    }
}

fn replay_dev_full_paths(report: &mut S1ReplayReport) {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let factual_write = runtime.write(
        WriteCandidate::new(
            "agent:replay",
            "task:s1:factual",
            "项目名称是 Beetle Memory",
        )
        .source("replay:factual")
        .plane_hint(MemoryPlane::SharedFactual),
    );
    let factual_recall = runtime.recall(
        bm_core::RecallQuery::new("task:s1:factual")
            .intent(PromptRecallIntent::Factual)
            .plane(MemoryPlane::SharedFactual),
    );
    let factual_projection = runtime.project(&factual_recall, ProjectionSurface::Prompt);
    record_path(
        report,
        PathRecordInput {
            path: S1ReplayPath::FactualPromptProjection,
            profile: RuntimeProfile::DevFull,
            writes: vec![factual_write],
            recall: &factual_recall,
            projection: &factual_projection,
            projection_trimmed: false,
            raw_private_probe: None,
        },
    );

    let procedural_write = runtime.write(
        WriteCandidate::new(
            "agent:replay",
            "task:s1:procedural",
            "下次遇到 S1 合同漂移时，先补 core report 测试，再改 SDK。",
        )
        .source("task-learning:replay"),
    );
    let procedural_recall = runtime.recall(
        bm_core::RecallQuery::new("task:s1:procedural")
            .intent(PromptRecallIntent::Procedural)
            .plane(MemoryPlane::Procedural),
    );
    let procedural_projection = runtime.project(&procedural_recall, ProjectionSurface::Adapter);
    record_path(
        report,
        PathRecordInput {
            path: S1ReplayPath::ProceduralAdapterProjection,
            profile: RuntimeProfile::DevFull,
            writes: vec![procedural_write],
            recall: &procedural_recall,
            projection: &procedural_projection,
            projection_trimmed: false,
            raw_private_probe: None,
        },
    );

    let archive_write = runtime.write(
        WriteCandidate::new("agent:replay", "task:s1:archive", "archive evidence only")
            .source("archive:hit-1")
            .plane_hint(MemoryPlane::ArchiveEvidence),
    );
    let archive_recall = runtime.recall(
        bm_core::RecallQuery::new("task:s1:archive")
            .intent(PromptRecallIntent::Evidence)
            .plane(MemoryPlane::ArchiveEvidence),
    );
    let archive_projection =
        runtime.project(&archive_recall, ProjectionSurface::OperatorInspection);
    let archive_promotion = runtime.write(
        WriteCandidate::new("agent:replay", "task:s1:archive", "archive evidence only")
            .source("archive:hit-1")
            .plane_hint(MemoryPlane::SharedFactual),
    );
    record_path(
        report,
        PathRecordInput {
            path: S1ReplayPath::ArchiveEvidenceNeedsDistillation,
            profile: RuntimeProfile::DevFull,
            writes: vec![archive_write, archive_promotion],
            recall: &archive_recall,
            projection: &archive_projection,
            projection_trimmed: false,
            raw_private_probe: None,
        },
    );

    let raw_private = "RAW PRIVATE MATERIAL SHOULD NOT MOVE";
    let subject_write = runtime.write(
        WriteCandidate::new(
            "agent:replay",
            "task:s1:soul",
            "当前回合使用 compact 主体挂载帧；私域原文已过滤。",
        )
        .source("host:subject-state")
        .plane_hint(MemoryPlane::SubjectProjection),
    );
    let subject_recall = runtime.recall(
        bm_core::RecallQuery::new("task:s1:soul")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let subject_projection = runtime.project(&subject_recall, ProjectionSurface::Prompt);
    record_path(
        report,
        PathRecordInput {
            path: S1ReplayPath::SoulGovernanceSubjectProjection,
            profile: RuntimeProfile::DevFull,
            writes: vec![subject_write],
            recall: &subject_recall,
            projection: &subject_projection,
            projection_trimmed: false,
            raw_private_probe: Some(raw_private),
        },
    );
}

fn replay_esp_compact_trim(report: &mut S1ReplayReport) {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::EspCompact)
        .store(store)
        .build();
    let write = runtime.write(
        WriteCandidate::new(
            "agent:replay",
            "task:s1:esp",
            "compact subject projection ".repeat(40),
        )
        .source("replay:esp")
        .plane_hint(MemoryPlane::SubjectProjection),
    );
    let recall = runtime.recall(
        bm_core::RecallQuery::new("task:s1:esp")
            .intent(PromptRecallIntent::Continuity)
            .plane(MemoryPlane::SubjectProjection),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Prompt);
    let projection_trimmed = recall.warnings.iter().any(|warning| {
        matches!(
            warning,
            RecallWarning::ProfileBudgetTrimmed {
                profile: RuntimeProfile::EspCompact,
                ..
            }
        )
    });
    if projection_trimmed {
        report
            .warnings
            .push("EspCompact projection trimmed".to_owned());
    }
    record_path(
        report,
        PathRecordInput {
            path: S1ReplayPath::EspCompactProjectionTrim,
            profile: RuntimeProfile::EspCompact,
            writes: vec![write],
            recall: &recall,
            projection: &projection,
            projection_trimmed,
            raw_private_probe: None,
        },
    );
}

struct PathRecordInput<'a> {
    path: S1ReplayPath,
    profile: RuntimeProfile,
    writes: Vec<WriteReport>,
    recall: &'a RecallSelectionReport,
    projection: &'a ProjectionReport,
    projection_trimmed: bool,
    raw_private_probe: Option<&'a str>,
}

fn record_path(report: &mut S1ReplayReport, input: PathRecordInput<'_>) {
    let accepted = input
        .writes
        .iter()
        .filter(|write| write.decision == WriteDecision::Accepted)
        .count();
    let rejected = input
        .writes
        .iter()
        .filter(|write| write.decision == WriteDecision::Rejected)
        .count();
    let rejected_reasons = input
        .writes
        .iter()
        .filter_map(|write| write.governance.reject_reason)
        .collect::<Vec<_>>();
    let raw_private_exposed = input.raw_private_probe.is_some_and(|probe| {
        input
            .projection
            .blocks
            .iter()
            .any(|block| block.content.contains(probe))
    });

    report.accepted += accepted;
    report.rejected += rejected;
    report.selected += input.recall.selected.len();
    report.projected += input.projection.blocks.len();
    report.paths.push(S1ReplayPathReport {
        path: input.path,
        profile: input.profile,
        selected_planes: input
            .recall
            .selected
            .iter()
            .map(|selection| selection.plane)
            .collect(),
        projected_planes: input
            .projection
            .blocks
            .iter()
            .map(|block| block.plane)
            .collect(),
        rejected_reasons,
        canonical_projection: !input.projection.blocks.is_empty()
            && input
                .recall
                .selected
                .iter()
                .all(|selection| selection.canonical)
            && input.projection.blocks.iter().all(|block| {
                !matches!(block.plane, MemoryPlane::ArchiveEvidence) && !block.privacy_filtered
            }),
        privacy_filtered: input
            .projection
            .blocks
            .iter()
            .any(|block| block.privacy_filtered),
        raw_private_exposed,
        projection_trimmed: input.projection_trimmed,
    });
}
