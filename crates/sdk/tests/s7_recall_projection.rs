use bm_core::{
    MemoryPlane, ProjectionSurface, PromptRecallIntent, RecallAssemblyRequest, RuntimeProfile,
    WriteCandidate,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn assemble_recall_routes_reranks_budgets_and_sanitizes_prompt_context() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            "task:s7",
            "S7 当前阶段是 recall orchestration 和 prompt projection",
        )
        .source("operator")
        .plane_hint(MemoryPlane::SharedFactual),
    );
    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            "task:s7",
            "archive turn: prompt budget 曾经导致上下文裁剪错误",
        )
        .source("archive:turn:s7")
        .plane_hint(MemoryPlane::ArchiveEvidence),
    );
    let skill = runtime.write(
        WriteCandidate::new(
            "agent:s7",
            "task:s7",
            "下次落实阶段内核时，先补 core/sdk/replay 红灯测试，再改 SDK 编排。",
        )
        .source("task-learning:s7")
        .plane_hint(MemoryPlane::Procedural),
    );
    if let Some(record_id) = skill.record_id.as_ref() {
        runtime.record_procedural_skill_outcome(
            std::slice::from_ref(record_id),
            bm_core::ProceduralSkillReuseOutcome::Succeeded,
            20,
            "validated by S7 prompt assembly test",
        );
    }
    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            "task:s7",
            "继续 S7：当前 active work 是把多平面召回组装成 prompt assembly report",
        )
        .source("runtime:continuity")
        .plane_hint(MemoryPlane::ContinuityCapsule),
    );
    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            "task:s7",
            "当前主体挂载只允许摘要进入前台，SECRET-S7-PRIVATE 不得回显",
        )
        .source("runtime:subject")
        .plane_hint(MemoryPlane::SubjectProjection),
    );

    let request = RecallAssemblyRequest::new(
        "agent:s7",
        "task:s7",
        "继续",
        ProjectionSurface::Prompt,
        RuntimeProfile::DevFull,
    )
    .active_task("继续落实 S7")
    .recent_grounding("S5/S6 已落地")
    .redact_fragment("SECRET-S7-PRIVATE")
    .limit(4);

    let assembly = runtime.assemble_recall(request);

    assert_eq!(assembly.router.intent, PromptRecallIntent::Continuity);
    assert!(assembly
        .router
        .signals
        .iter()
        .any(|signal| signal.plane == MemoryPlane::ContinuityCapsule && signal.score > 0));
    assert!(assembly
        .plane_reports
        .iter()
        .any(|report| report.plane == MemoryPlane::Procedural && report.selected_count > 0));
    assert!(assembly
        .plane_reports
        .iter()
        .any(|report| report.plane == MemoryPlane::TaskRecall && report.miss_reason.is_some()));
    assert!(assembly
        .groups
        .active_task_context
        .as_deref()
        .is_some_and(|text| text.contains("继续落实 S7")));
    assert!(assembly
        .groups
        .governed_memory_evidence
        .as_deref()
        .is_some_and(|text| text.contains("Procedural skill hint")));
    assert!(assembly
        .rerank
        .top_candidates
        .iter()
        .any(|candidate| candidate.plane == MemoryPlane::ContinuityCapsule));
    assert!(assembly
        .rerank
        .top_candidates
        .iter()
        .all(|candidate| candidate.selected && candidate.rerank_score >= candidate.original_score));
    assert!(assembly
        .rerank
        .top_planes
        .iter()
        .all(|signal| signal.candidate_count >= signal.selected_count));
    assert!(!assembly.rerank.skipped_candidates.is_empty());
    assert!(assembly.sanitizer.redacted_fragments >= 1);
    assert!(!assembly
        .blocks
        .iter()
        .any(|block| block.content.contains("SECRET-S7-PRIVATE")));
    assert!(
        assembly.budget.governed_memory.after_bytes <= assembly.budget.governed_memory.max_bytes
    );
}

#[test]
fn project_context_adapter_surface_is_report_first_and_does_not_leak_private_raw() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    runtime.write(
        WriteCandidate::new(
            "agent:s7",
            "task:s7-adapter",
            "主体私域只允许 presence/report，SECRET-ADAPTER-PRIVATE 不得输出",
        )
        .source("runtime:subject")
        .plane_hint(MemoryPlane::SubjectProjection),
    );

    let request = RecallAssemblyRequest::new(
        "agent:s7",
        "task:s7-adapter",
        "检查主体",
        ProjectionSurface::Adapter,
        RuntimeProfile::DevFull,
    )
    .intent_hint(PromptRecallIntent::Continuity)
    .redact_fragment("SECRET-ADAPTER-PRIVATE");

    let projection = runtime.project_context(request);

    assert_eq!(projection.surface, ProjectionSurface::Adapter);
    assert_eq!(projection.privacy_filtered_count, projection.blocks.len());
    assert!(projection
        .blocks
        .iter()
        .all(|block| !block.content.contains("SECRET-ADAPTER-PRIVATE")));
    assert!(projection
        .warnings
        .iter()
        .any(|warning| warning.contains("prompt_assembly")));
}
