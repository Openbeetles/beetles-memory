use bm_core::{
    MemoryPlane, ProjectionSurface, PromptAssemblyReport, PromptRecallIntent,
    RecallAssemblyRequest, RuntimeProfile,
};

#[test]
fn s7_contract_names_recall_assembly_budget_sanitizer_and_groups() {
    let request = RecallAssemblyRequest::new(
        "agent:s7",
        "task:s7",
        "继续 S7",
        ProjectionSurface::Prompt,
        RuntimeProfile::DevFull,
    )
    .active_task("继续落实 S7 recall projection")
    .recent_grounding("S6 procedural skill memory is already landed")
    .redact_fragment("SECRET-S7-PRIVATE")
    .limit(6);

    assert_eq!(request.identity, "agent:s7");
    assert_eq!(request.scope, "task:s7");
    assert_eq!(request.raw_query, "继续 S7");
    assert_eq!(request.surface, ProjectionSurface::Prompt);
    assert_eq!(request.profile, RuntimeProfile::DevFull);
    assert_eq!(request.limits.per_plane_limit, 6);
    assert_eq!(request.redaction.private_fragments[0], "SECRET-S7-PRIVATE");

    let budget = RuntimeProfile::DevFull.projection_budget_profile(ProjectionSurface::Prompt);
    let compact = RuntimeProfile::EspCompact.projection_budget_profile(ProjectionSurface::Prompt);
    assert!(budget.total_bytes > compact.total_bytes);
    assert!(budget.governed_memory_bytes >= budget.active_task_bytes);

    let report = PromptAssemblyReport::empty(request, PromptRecallIntent::Continuity);
    assert_eq!(report.router.intent, PromptRecallIntent::Continuity);
    assert!(report.groups.active_task_context.is_none());
    assert!(report.plane_reports.is_empty());
    assert_eq!(report.sanitizer.redacted_fragments, 0);
}

#[test]
fn s7_contract_keeps_subject_and_soul_out_of_raw_candidate_mix() {
    let request = RecallAssemblyRequest::new(
        "agent:s7",
        "task:s7",
        "检查主体挂载",
        ProjectionSurface::Adapter,
        RuntimeProfile::DevFull,
    );
    let report = PromptAssemblyReport::empty(request, PromptRecallIntent::Continuity);

    assert!(report
        .router
        .active_task_order
        .contains(&MemoryPlane::SubjectProjection));
    assert!(!report
        .router
        .governed_memory_order
        .contains(&MemoryPlane::SoulGovernance));
}
