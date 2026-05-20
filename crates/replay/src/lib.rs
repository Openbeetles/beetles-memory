//! Replay fixtures for Beetle Memory.

pub use bm_core::{EvolutionMode, EvolutionProposalKind};

use bm_core::{
    ArchiveEvidenceLink, ArchiveRecordLocator, ArchiveRecordSource, ArchiveSearchQuery, Confidence,
    EvidenceRef, EvidenceState, EvolutionBudget, EvolutionInput, EvolutionProposalBatch, Freshness,
    LongTermMemoryKind, MemoryPlane, MentalPrivacyLayer, ProceduralEvidenceRef,
    ProceduralSkillDraft, ProceduralSkillImportEnvelope, ProceduralSkillOrigin,
    ProceduralSkillRecallQuery, ProceduralSkillReuseOutcome, ProjectionReport, ProjectionSurface,
    PromptRecallIntent, RecallAssemblyRequest, RecallSelectionReport, RecallWarning,
    RuntimeProfile, SourceKind, SourceRef, SubjectAssemblyReport, SubjectAssemblySource,
    WriteCandidate, WriteDecision, WriteRejectReason, WriteReport,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct S5ReplayReport {
    pub inserted: usize,
    pub replaced: usize,
    pub rejected: usize,
    pub deleted: usize,
    pub archive_hits: usize,
    pub warnings: Vec<String>,
}

pub fn run_s5_replay() -> S5ReplayReport {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();
    let mut report = S5ReplayReport::default();

    let first = runtime.write(s5_fact("S5 long-term kernel is active").observed_at(10));
    if first.decision == WriteDecision::Accepted {
        report.inserted += 1;
    }
    let replaced =
        runtime.write(s5_fact("S5 long-term kernel includes archive evidence").observed_at(20));
    if replaced.decision == WriteDecision::Merged {
        report.replaced += 1;
    }
    let archive_reject = runtime.write(
        WriteCandidate::new("agent:s5", "task:s5:replay", "archive-only fact")
            .source("archive:transcript:replay")
            .plane_hint(MemoryPlane::SharedFactual),
    );
    if archive_reject.decision == WriteDecision::Rejected {
        report.rejected += 1;
    }
    runtime.write(
        WriteCandidate::new(
            "agent:s5",
            "task:s5:replay",
            "archive evidence supports S5 replay",
        )
        .source("archive:transcript:replay")
        .plane_hint(MemoryPlane::ArchiveEvidence)
        .topic("S5 replay")
        .evidence(EvidenceState::ArchiveOnly),
    );
    let archive = runtime.search_archive(ArchiveSearchQuery::new(
        "task:s5:replay",
        "archive replay",
        RuntimeProfile::DevFull,
    ));
    report.archive_hits = archive.hits.len();
    let recall = runtime
        .recall(bm_core::RecallQuery::new("task:s5:replay").intent(PromptRecallIntent::Evidence));
    report
        .warnings
        .extend(recall.warnings.iter().map(|warning| format!("{warning:?}")));

    let raw = r#"[{"plane":"factual","op":"delete","kind":"project","topic":"S5 replay"}]"#;
    let prepared = runtime.prepare_long_term_extraction(
        raw,
        "agent:s5",
        "task:s5:replay",
        SourceRef::new(
            SourceKind::LongTermExtraction,
            "long-term-extraction:replay",
        ),
    );
    let applied = runtime.apply_long_term_extraction(prepared);
    report.deleted = applied.deleted;

    report
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct S6ReplayReport {
    pub user_provided_inserted: usize,
    pub import_quarantined: usize,
    pub import_adopted: usize,
    pub runtime_rejected: usize,
    pub runtime_accepted: usize,
    pub outcome_updates: usize,
    pub recall_selected: usize,
    pub projection_blocks: usize,
    pub warnings: Vec<String>,
}

pub fn run_s6_replay() -> S6ReplayReport {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();
    let mut report = S6ReplayReport::default();

    let user = runtime.write_procedural_skill(s6_user_skill());
    if user.decision == WriteDecision::Accepted {
        report.user_provided_inserted += 1;
    }

    let envelope = ProceduralSkillImportEnvelope::new(s6_imported_skill(), "digest-s6");
    let envelope_json = match serde_json::to_string(&envelope) {
        Ok(json) => json,
        Err(error) => {
            report
                .warnings
                .push(format!("s6 envelope serialization failed: {error}"));
            return report;
        }
    };
    let imported = runtime.import_procedural_skill(&envelope_json, false);
    report.import_quarantined += imported.quarantined;
    if imported.quarantined > 0 {
        report
            .warnings
            .push("quarantined imported procedural skill".to_owned());
    }
    let adopted = runtime.import_procedural_skill(&envelope_json, true);
    report.import_adopted += adopted.adopted;

    let rejected = runtime.write_procedural_skill(ProceduralSkillDraft::new(
        "agent:s6",
        "task:s6:replay",
        ProceduralSkillOrigin::RuntimeLearned,
        "Runtime recovery",
        "runtime recovery",
        "When recovery is needed, first inspect the narrow log, then run the replay.",
    ));
    if rejected.decision == WriteDecision::Rejected {
        report.runtime_rejected += 1;
    }

    let accepted = runtime.write_procedural_skill(
        ProceduralSkillDraft::new(
            "agent:s6",
            "task:s6:replay",
            ProceduralSkillOrigin::RuntimeLearned,
            "Runtime recovery",
            "runtime recovery",
            "When recovery is needed, first inspect the narrow log, then run the replay.",
        )
        .evidence(vec![ProceduralEvidenceRef::new(
            "replay:s6",
            "runtime recovery worked under replay",
        )]),
    );
    if accepted.decision == WriteDecision::Accepted {
        report.runtime_accepted += 1;
    }
    if let Some(record_id) = accepted.record_id {
        report.outcome_updates += runtime
            .record_procedural_skill_outcome(
                std::slice::from_ref(&record_id),
                ProceduralSkillReuseOutcome::Succeeded,
                10,
                "reused successfully",
            )
            .updated;
        report.outcome_updates += runtime
            .record_procedural_skill_outcome(
                std::slice::from_ref(&record_id),
                ProceduralSkillReuseOutcome::Mismatch,
                20,
                "needs revision",
            )
            .updated;
    }

    let recall = runtime.recall_procedural_skills(ProceduralSkillRecallQuery::new(
        "task:s6:replay",
        "release checklist runtime recovery",
    ));
    report.recall_selected = recall.selected_count;
    report.warnings.extend(recall.warnings.clone());
    let projection = runtime.project_procedural_skills(&recall, ProjectionSurface::Prompt);
    report.projection_blocks = projection.blocks.len();
    report
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct S7ReplayReport {
    pub prompt_assemblies: usize,
    pub projection_blocks: usize,
    pub sanitized_fragments: usize,
    pub budget_trimmed: bool,
    pub raw_private_exposed: bool,
    pub intents: Vec<PromptRecallIntent>,
    pub selected_planes: Vec<MemoryPlane>,
    pub profiles: Vec<RuntimeProfile>,
    pub warnings: Vec<String>,
}

pub fn run_s7_replay() -> S7ReplayReport {
    let mut report = S7ReplayReport::default();
    let private_probe = "SECRET-S7-REPLAY";

    let mut runtime = replay_runtime(RuntimeProfile::DevFull);
    seed_s7_records(&mut runtime, "task:s7:replay", private_probe);

    let prompt = runtime.assemble_recall(
        RecallAssemblyRequest::new(
            "agent:s7",
            "task:s7:replay",
            "继续",
            ProjectionSurface::Prompt,
            RuntimeProfile::DevFull,
        )
        .active_task("继续 S7 replay")
        .recent_grounding("S5/S6 replay already passed")
        .redact_fragment(private_probe),
    );
    record_s7_assembly(&mut report, &prompt, private_probe);

    let evidence = runtime.assemble_recall(RecallAssemblyRequest::new(
        "agent:s7",
        "task:s7:replay",
        "日志证据",
        ProjectionSurface::Replay,
        RuntimeProfile::DevFull,
    ));
    record_s7_assembly(&mut report, &evidence, private_probe);

    let adapter = runtime.project_context(
        RecallAssemblyRequest::new(
            "agent:s7",
            "task:s7:replay",
            "检查主体",
            ProjectionSurface::Adapter,
            RuntimeProfile::DevFull,
        )
        .intent_hint(PromptRecallIntent::Continuity)
        .redact_fragment(private_probe),
    );
    report.projection_blocks += adapter.blocks.len();
    report.raw_private_exposed |= adapter
        .blocks
        .iter()
        .any(|block| block.content.contains(private_probe));
    report.warnings.extend(adapter.warnings);

    let mut compact = replay_runtime(RuntimeProfile::EspCompact);
    compact.write(
        WriteCandidate::new(
            "agent:s7",
            "task:s7:compact",
            "compact active continuity ".repeat(80),
        )
        .source("replay:s7:compact")
        .plane_hint(MemoryPlane::ContinuityCapsule),
    );
    let compact_assembly = compact.assemble_recall(
        RecallAssemblyRequest::new(
            "agent:s7",
            "task:s7:compact",
            "继续",
            ProjectionSurface::Prompt,
            RuntimeProfile::EspCompact,
        )
        .active_task("compact profile must trim prompt assembly ".repeat(40)),
    );
    record_s7_assembly(&mut report, &compact_assembly, private_probe);

    report
}

fn seed_s7_records(
    runtime: &mut bm_sdk::MemoryRuntime<InMemoryStore>,
    scope: &str,
    private_probe: &str,
) {
    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            scope,
            "S7 replay fact: recall orchestration precedes communication adapters",
        )
        .source("replay:s7:factual")
        .plane_hint(MemoryPlane::SharedFactual),
    );
    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            scope,
            "archive evidence: prompt budget and sanitizer must be reported",
        )
        .source("archive:turn:s7")
        .plane_hint(MemoryPlane::ArchiveEvidence)
        .evidence(EvidenceState::ArchiveOnly),
    );
    let procedural = runtime.write(
        WriteCandidate::new(
            "agent:s7",
            scope,
            "下次做阶段内核时，先写红灯测试，再补 SDK 编排和 replay gate。",
        )
        .source("task-learning:s7")
        .plane_hint(MemoryPlane::Procedural),
    );
    if let Some(record_id) = procedural.record_id.as_ref() {
        runtime.record_procedural_skill_outcome(
            std::slice::from_ref(record_id),
            ProceduralSkillReuseOutcome::Succeeded,
            30,
            "validated by S7 replay",
        );
    }
    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            scope,
            "继续 S7 replay: active work is prompt assembly",
        )
        .source("replay:s7:continuity")
        .plane_hint(MemoryPlane::ContinuityCapsule),
    );
    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            scope,
            format!("subject projection summary, {private_probe} must be redacted"),
        )
        .source("replay:s7:subject")
        .plane_hint(MemoryPlane::SubjectProjection),
    );
}

fn record_s7_assembly(
    report: &mut S7ReplayReport,
    assembly: &bm_core::PromptAssemblyReport,
    private_probe: &str,
) {
    report.prompt_assemblies += 1;
    report.projection_blocks += assembly.blocks.len();
    report.sanitized_fragments += assembly.sanitizer.redacted_fragments;
    report.budget_trimmed |= assembly.budget.total.trimmed
        || assembly.budget.active_task.trimmed
        || assembly.budget.governed_memory.trimmed;
    report.raw_private_exposed |= assembly
        .blocks
        .iter()
        .any(|block| block.content.contains(private_probe));
    extend_unique_prompt_intent(&mut report.intents, assembly.router.intent);
    extend_unique_runtime_profile(&mut report.profiles, assembly.profile);
    for block in &assembly.blocks {
        if !report.selected_planes.contains(&block.plane) {
            report.selected_planes.push(block.plane);
        }
    }
    report.warnings.extend(assembly.warnings.clone());
}

fn extend_unique_prompt_intent(target: &mut Vec<PromptRecallIntent>, value: PromptRecallIntent) {
    if !target.contains(&value) {
        target.push(value);
    }
}

fn extend_unique_runtime_profile(target: &mut Vec<RuntimeProfile>, value: RuntimeProfile) {
    if !target.contains(&value) {
        target.push(value);
    }
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

fn s5_fact(content: &str) -> WriteCandidate {
    WriteCandidate::new("agent:s5", "task:s5:replay", content)
        .source("replay:s5")
        .plane_hint(MemoryPlane::SharedFactual)
        .long_term_kind(LongTermMemoryKind::Project)
        .topic("S5 replay")
        .confidence(Confidence::Medium)
        .freshness(Freshness::Current)
        .canonical(true)
        .archive_links(vec![ArchiveEvidenceLink {
            locator: ArchiveRecordLocator {
                source: ArchiveRecordSource::Transcript,
                scope: "task:s5:replay".to_owned(),
                record_id: "archive:transcript:replay".to_owned(),
            },
            supports: true,
            reason: "replay evidence link".to_owned(),
        }])
}

fn s6_user_skill() -> ProceduralSkillDraft {
    ProceduralSkillDraft::new(
        "agent:s6",
        "task:s6:replay",
        ProceduralSkillOrigin::UserProvided,
        "Release checklist",
        "release checklist",
        "When preparing a release, first verify status, then run tests, then commit.",
    )
}

fn s6_imported_skill() -> ProceduralSkillDraft {
    ProceduralSkillDraft::new(
        "agent:s6",
        "task:s6:replay",
        ProceduralSkillOrigin::RuntimeLearned,
        "Serial recovery",
        "serial recovery",
        "When serial framing stalls, first reset the reader, then retry one narrow probe.",
    )
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
    if let Some(record_id) = procedural_write.record_id.as_ref() {
        runtime.record_procedural_skill_outcome(
            std::slice::from_ref(record_id),
            ProceduralSkillReuseOutcome::Succeeded,
            10,
            "validated by S3 replay fixture",
        );
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum S4ReplayPath {
    ArchiveEvidenceProducesDistillationProposal,
    ProgramEvidenceCannotBecomeSoulRevision,
    StableProceduralPatternProducesPromotionProposal,
    SubjectAssemblyProducesRefreshProposal,
    PrivateMaterialForcesNoWriteOrPrivacyRepair,
    FullSandboxCanAdjudicateBranches,
    CompactSandboxTrimsAndSkipsHeavyPasses,
    ConsumerModeDoesNotRunEvolution,
    ProposalApplyReturnsToSdkGovernance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S4ReplayPathReport {
    pub path: S4ReplayPath,
    pub profile: RuntimeProfile,
    pub mode: EvolutionMode,
    pub evidence_read: usize,
    pub branches_evaluated: usize,
    pub proposals_emitted: usize,
    pub rejected_candidates: usize,
    pub privacy_filtered_count: usize,
    pub profile_trimmed: bool,
    pub raw_private_exposed: bool,
    pub proposal_kinds: Vec<EvolutionProposalKind>,
    pub rejected_reasons: Vec<WriteRejectReason>,
    pub proposal_apply_reports: Vec<WriteReport>,
    pub sdk_governance_returned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S4ReplayReport {
    pub mode: EvolutionMode,
    pub evidence_read: usize,
    pub branches_evaluated: usize,
    pub proposals_emitted: usize,
    pub rejected_candidates: usize,
    pub privacy_filtered_count: usize,
    pub profile_trimmed: bool,
    pub raw_private_exposed: bool,
    pub proposal_apply_reports: Vec<WriteReport>,
    pub contract_red_light_reasons: Vec<String>,
    pub paths: Vec<S4ReplayPathReport>,
}

pub fn run_s4_replay() -> S4ReplayReport {
    let paths = vec![
        replay_s4_archive_evidence_distillation(),
        replay_s4_program_evidence_soul_revision_gate(),
        replay_s4_stable_procedural_pattern(),
        replay_s4_subject_assembly_refresh(),
        replay_s4_private_material_privacy_repair(),
        replay_s4_full_sandbox_branches(),
        replay_s4_compact_sandbox_trim(),
        replay_s4_consumer_mode_no_evolution(),
        replay_s4_proposal_apply_sdk_governance(),
    ];

    let proposal_apply_reports = paths
        .iter()
        .flat_map(|path| path.proposal_apply_reports.iter().cloned())
        .collect::<Vec<_>>();

    S4ReplayReport {
        mode: EvolutionMode::Full,
        evidence_read: paths.iter().map(|path| path.evidence_read).sum(),
        branches_evaluated: paths.iter().map(|path| path.branches_evaluated).sum(),
        proposals_emitted: paths.iter().map(|path| path.proposals_emitted).sum(),
        rejected_candidates: paths.iter().map(|path| path.rejected_candidates).sum(),
        privacy_filtered_count: paths.iter().map(|path| path.privacy_filtered_count).sum(),
        profile_trimmed: paths.iter().any(|path| path.profile_trimmed),
        raw_private_exposed: paths.iter().any(|path| path.raw_private_exposed),
        proposal_apply_reports,
        contract_red_light_reasons: Vec::new(),
        paths,
    }
}

fn replay_s4_archive_evidence_distillation() -> S4ReplayPathReport {
    let mut runtime = replay_runtime(RuntimeProfile::DevFull);
    let batch = runtime.propose_evolution(s4_input(
        "archive-distillation",
        RuntimeProfile::DevFull,
        EvolutionMode::Full,
        vec![s4_evidence(
            "archive:s4-evidence",
            MemoryPlane::ArchiveEvidence,
            MentalPrivacyLayer::Shared,
            EvidenceState::ArchiveOnly,
            "archive-only evidence requires distillation before factual memory refresh",
        )],
        None,
        1,
    ));
    let mut apply_reports = apply_s4_proposals(&mut runtime, &batch);
    let direct_archive_promotion = runtime.write(
        WriteCandidate::new(
            "agent:s4",
            "task:s4:archive-distillation",
            "archive-only evidence requires distillation before factual memory refresh",
        )
        .source("archive:s4-evidence")
        .plane_hint(MemoryPlane::SharedFactual)
        .evidence(EvidenceState::ArchiveOnly),
    );
    apply_reports.push(direct_archive_promotion);

    s4_path_report_from_batch(
        S4ReplayPath::ArchiveEvidenceProducesDistillationProposal,
        batch,
        apply_reports,
        true,
    )
}

fn replay_s4_program_evidence_soul_revision_gate() -> S4ReplayPathReport {
    let mut runtime = replay_runtime(RuntimeProfile::DevFull);
    let batch = runtime.propose_evolution(s4_input(
        "program-soul-gate",
        RuntimeProfile::DevFull,
        EvolutionMode::Full,
        vec![s4_evidence(
            "task-learning:s4-program",
            MemoryPlane::Procedural,
            MentalPrivacyLayer::Shared,
            EvidenceState::Supported,
            "program evidence supports subject refresh but cannot become soul governance",
        )],
        Some(mounted_subject_assembly(RuntimeProfile::DevFull)),
        1,
    ));
    let mut apply_reports = apply_s4_proposals(&mut runtime, &batch);
    let blocked_soul_revision = runtime.write(
        WriteCandidate::new(
            "agent:s4",
            "task:s4:program-soul-gate",
            "下次评估主体刷新时，程序证据只能作为 subject refresh support，不能直升 soul governance。",
        )
        .source("task-learning:s4")
        .plane_hint(MemoryPlane::SoulGovernance),
    );
    apply_reports.push(blocked_soul_revision);

    s4_path_report_from_batch(
        S4ReplayPath::ProgramEvidenceCannotBecomeSoulRevision,
        batch,
        apply_reports,
        true,
    )
}

fn replay_s4_stable_procedural_pattern() -> S4ReplayPathReport {
    let mut runtime = replay_runtime(RuntimeProfile::DevFull);
    let evidence = (0..3)
        .map(|idx| {
            s4_evidence(
                &format!("task-learning:s4-procedure-{idx}"),
                MemoryPlane::Procedural,
                MentalPrivacyLayer::Shared,
                EvidenceState::Supported,
                "下次准备 replay gate 时，先写失败测试，再实现 deterministic replay fixture。",
            )
        })
        .collect::<Vec<_>>();
    let batch = runtime.propose_evolution(s4_input(
        "procedural-pattern",
        RuntimeProfile::DevFull,
        EvolutionMode::Full,
        evidence,
        None,
        1,
    ));
    let apply_reports = apply_s4_proposals(&mut runtime, &batch);

    s4_path_report_from_batch(
        S4ReplayPath::StableProceduralPatternProducesPromotionProposal,
        batch,
        apply_reports,
        true,
    )
}

fn replay_s4_subject_assembly_refresh() -> S4ReplayPathReport {
    let mut runtime = replay_runtime(RuntimeProfile::DevFull);
    let batch = runtime.propose_evolution(s4_input(
        "subject-refresh",
        RuntimeProfile::DevFull,
        EvolutionMode::Full,
        vec![s4_evidence(
            "replay:s4-subject-assembly",
            MemoryPlane::SharedFactual,
            MentalPrivacyLayer::Shared,
            EvidenceState::Supported,
            "subject assembly report indicates continuity and task sources should refresh projection",
        )],
        Some(mounted_subject_assembly(RuntimeProfile::DevFull)),
        1,
    ));
    let apply_reports = apply_s4_proposals(&mut runtime, &batch);

    s4_path_report_from_batch(
        S4ReplayPath::SubjectAssemblyProducesRefreshProposal,
        batch,
        apply_reports,
        true,
    )
}

fn replay_s4_private_material_privacy_repair() -> S4ReplayPathReport {
    let raw_private = "RAW PRIVATE S4 MATERIAL";
    let mut runtime = replay_runtime(RuntimeProfile::DevFull);
    let batch = runtime.propose_evolution(s4_input(
        "privacy-repair",
        RuntimeProfile::DevFull,
        EvolutionMode::Full,
        vec![s4_evidence(
            "replay:s4-private-material",
            MemoryPlane::SubjectProjection,
            MentalPrivacyLayer::Sealed,
            EvidenceState::Supported,
            "sealed material presence only",
        )],
        None,
        1,
    ));
    let apply = runtime.write(
        WriteCandidate::new("agent:s4", "task:s4:privacy-repair", raw_private)
            .source("replay:s4-private-material")
            .plane_hint(MemoryPlane::SubjectProjection)
            .privacy_layer(MentalPrivacyLayer::Private),
    );

    s4_path_report_from_batch(
        S4ReplayPath::PrivateMaterialForcesNoWriteOrPrivacyRepair,
        batch,
        vec![apply],
        true,
    )
}

fn replay_s4_full_sandbox_branches() -> S4ReplayPathReport {
    let runtime = replay_runtime(RuntimeProfile::DevFull);
    let batch = runtime.propose_evolution(s4_input(
        "full-branches",
        RuntimeProfile::DevFull,
        EvolutionMode::Full,
        vec![
            s4_evidence(
                "archive:s4-branch",
                MemoryPlane::ArchiveEvidence,
                MentalPrivacyLayer::Shared,
                EvidenceState::ArchiveOnly,
                "archive branch candidate",
            ),
            s4_evidence(
                "task-learning:s4-branch",
                MemoryPlane::Procedural,
                MentalPrivacyLayer::Shared,
                EvidenceState::Supported,
                "procedural branch candidate",
            ),
        ],
        None,
        2,
    ));

    s4_path_report_from_batch(
        S4ReplayPath::FullSandboxCanAdjudicateBranches,
        batch,
        Vec::new(),
        false,
    )
}

fn replay_s4_compact_sandbox_trim() -> S4ReplayPathReport {
    let mut runtime = replay_runtime(RuntimeProfile::EspCompact);
    let batch = runtime.propose_evolution(s4_input(
        "compact",
        RuntimeProfile::EspCompact,
        EvolutionMode::Compact,
        vec![s4_evidence(
            "task-learning:s4-compact",
            MemoryPlane::Procedural,
            MentalPrivacyLayer::Shared,
            EvidenceState::Supported,
            "compact sandbox keeps fixed-budget subject refresh proposal and skips branch review",
        )],
        Some(mounted_subject_assembly(RuntimeProfile::EspCompact)),
        0,
    ));
    let apply_reports = apply_s4_proposals(&mut runtime, &batch);

    s4_path_report_from_batch(
        S4ReplayPath::CompactSandboxTrimsAndSkipsHeavyPasses,
        batch,
        apply_reports,
        true,
    )
}

fn replay_s4_consumer_mode_no_evolution() -> S4ReplayPathReport {
    let runtime = replay_runtime(RuntimeProfile::SdkEmbedded);
    let batch = runtime.propose_evolution(s4_input(
        "consumer",
        RuntimeProfile::SdkEmbedded,
        EvolutionMode::Consumer,
        vec![s4_evidence(
            "replay:s4-consumer",
            MemoryPlane::ArchiveEvidence,
            MentalPrivacyLayer::Shared,
            EvidenceState::ArchiveOnly,
            "consumer evidence is report only",
        )],
        None,
        0,
    ));

    s4_path_report_from_batch(
        S4ReplayPath::ConsumerModeDoesNotRunEvolution,
        batch,
        Vec::new(),
        false,
    )
}

fn replay_s4_proposal_apply_sdk_governance() -> S4ReplayPathReport {
    let mut runtime = replay_runtime(RuntimeProfile::DevFull);
    let batch = runtime.propose_evolution(s4_input(
        "proposal-apply",
        RuntimeProfile::DevFull,
        EvolutionMode::Full,
        vec![s4_evidence(
            "task-learning:s4-apply",
            MemoryPlane::Procedural,
            MentalPrivacyLayer::Shared,
            EvidenceState::Supported,
            "下次应用 evolution proposal 时，只能把 candidate_write 提交给 SDK write governance。",
        )],
        None,
        1,
    ));
    let mut apply_reports = apply_s4_proposals(&mut runtime, &batch);
    let rejected = runtime.write(
        WriteCandidate::new(
            "agent:s4",
            "task:s4:proposal-apply",
            "proposal apply cannot bypass SDK governance for task-learning soul revision",
        )
        .source("task-learning:s4")
        .plane_hint(MemoryPlane::SoulGovernance),
    );
    apply_reports.push(rejected);

    s4_path_report_from_batch(
        S4ReplayPath::ProposalApplyReturnsToSdkGovernance,
        batch,
        apply_reports,
        true,
    )
}

struct S4PathReportInput {
    path: S4ReplayPath,
    batch: EvolutionProposalBatch,
    proposal_apply_reports: Vec<WriteReport>,
    sdk_governance_returned: bool,
}

fn s4_path_report_from_batch(
    path: S4ReplayPath,
    batch: EvolutionProposalBatch,
    proposal_apply_reports: Vec<WriteReport>,
    sdk_governance_returned: bool,
) -> S4ReplayPathReport {
    s4_path_report(S4PathReportInput {
        path,
        batch,
        proposal_apply_reports,
        sdk_governance_returned,
    })
}

fn s4_path_report(input: S4PathReportInput) -> S4ReplayPathReport {
    let rejected_reasons = input
        .proposal_apply_reports
        .iter()
        .filter_map(|report| report.governance.reject_reason)
        .collect::<Vec<_>>();
    let proposal_kinds = input
        .batch
        .proposals
        .iter()
        .map(|proposal| proposal.kind)
        .collect::<Vec<_>>();

    S4ReplayPathReport {
        path: input.path,
        profile: input.batch.profile,
        mode: input.batch.mode,
        evidence_read: input.batch.report.evidence_read,
        branches_evaluated: input.batch.report.branches_evaluated,
        proposals_emitted: input.batch.report.proposals_emitted,
        rejected_candidates: input.batch.report.rejected_candidates,
        privacy_filtered_count: input.batch.report.privacy_filtered_count,
        profile_trimmed: input.batch.report.profile_trimmed,
        raw_private_exposed: input.batch.report.raw_private_exposed,
        proposal_kinds,
        rejected_reasons,
        proposal_apply_reports: input.proposal_apply_reports,
        sdk_governance_returned: input.sdk_governance_returned,
    }
}

fn apply_s4_proposals(
    runtime: &mut bm_sdk::MemoryRuntime<InMemoryStore>,
    batch: &EvolutionProposalBatch,
) -> Vec<WriteReport> {
    batch
        .proposals
        .iter()
        .filter(|proposal| proposal.candidate_write.is_some())
        .map(|proposal| runtime.submit_evolution_proposal(proposal))
        .collect()
}

fn s4_input(
    run_suffix: &str,
    profile: RuntimeProfile,
    mode: EvolutionMode,
    evidence: Vec<EvidenceRef>,
    subject_assembly: Option<SubjectAssemblyReport>,
    max_branches: usize,
) -> EvolutionInput {
    EvolutionInput {
        run_id: format!("s4:{run_suffix}"),
        identity: "agent:s4".to_owned(),
        scope: format!("task:s4:{run_suffix}"),
        profile,
        mode,
        evidence,
        recall_report: None,
        projection_report: None,
        subject_assembly,
        budget: s4_budget(mode, max_branches),
    }
}

fn s4_budget(mode: EvolutionMode, max_branches: usize) -> EvolutionBudget {
    match mode {
        EvolutionMode::Full => EvolutionBudget {
            max_events: 64,
            max_records: 64,
            max_branches,
            max_proposals: 16,
            max_output_bytes: 8_192,
            allow_private_layer: false,
            allow_soul_revision: true,
            allow_script_backend: false,
        },
        EvolutionMode::Compact => EvolutionBudget {
            max_events: 8,
            max_records: 8,
            max_branches: 0,
            max_proposals: 8,
            max_output_bytes: 512,
            allow_private_layer: false,
            allow_soul_revision: false,
            allow_script_backend: false,
        },
        EvolutionMode::Consumer => EvolutionBudget {
            max_events: 0,
            max_records: 0,
            max_branches: 0,
            max_proposals: 0,
            max_output_bytes: 256,
            allow_private_layer: false,
            allow_soul_revision: false,
            allow_script_backend: false,
        },
    }
}

fn s4_evidence(
    id: &str,
    plane: MemoryPlane,
    privacy_layer: MentalPrivacyLayer,
    evidence: EvidenceState,
    summary: &str,
) -> EvidenceRef {
    EvidenceRef {
        record_id: Some(id.to_owned()),
        event_seq: None,
        source: SourceRef::new(SourceKind::ReplayFixture, id),
        plane,
        privacy_layer,
        evidence,
        summary: summary.to_owned(),
    }
}

fn mounted_subject_assembly(profile: RuntimeProfile) -> SubjectAssemblyReport {
    SubjectAssemblyReport {
        mounted: true,
        sources_used: Vec::new(),
        sources_missing: Vec::new(),
        privacy_decisions: Vec::new(),
        profile,
        budget_bytes: profile.projection_budget_bytes(),
    }
}

fn replay_runtime(profile: RuntimeProfile) -> bm_sdk::MemoryRuntime<InMemoryStore> {
    MemoryRuntimeBuilder::new(profile)
        .store(InMemoryStore::default())
        .build()
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
    if let Some(record_id) = procedural_write.record_id.as_ref() {
        runtime.record_procedural_skill_outcome(
            std::slice::from_ref(record_id),
            ProceduralSkillReuseOutcome::Succeeded,
            10,
            "validated by replay fixture",
        );
    }
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
