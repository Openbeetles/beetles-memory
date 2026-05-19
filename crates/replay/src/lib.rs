//! Replay fixtures for Beetle Memory.

use bm_core::{
    MemoryPlane, ProjectionReport, ProjectionSurface, PromptRecallIntent, RecallSelectionReport,
    RecallWarning, RuntimeProfile, WriteCandidate, WriteDecision, WriteRejectReason, WriteReport,
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
    let archive_projection = runtime.project(&archive_recall, ProjectionSurface::Inspection);
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
