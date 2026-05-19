use bm_core::{MemoryPlane, RuntimeProfile, WriteRejectReason};
use bm_replay::{run_s3_replay, S3ReplayPath};

#[test]
fn s3_replay_reports_all_required_soul_subject_paths() {
    let report = run_s3_replay();

    let paths: Vec<_> = report.paths.iter().map(|path| path.path).collect();
    assert!(paths.contains(&S3ReplayPath::SoulGovernanceSummaryFeedsSubjectProjection));
    assert!(paths.contains(&S3ReplayPath::ProgramEvidenceSupportsSubjectAssembly));
    assert!(paths.contains(&S3ReplayPath::PrivateMaterialFilteredFromPrompt));
    assert!(paths.contains(&S3ReplayPath::OperatorInspectionShowsPresenceOnly));
    assert!(
        paths.contains(&S3ReplayPath::EspCompactAcceptsSubjectProjectionButRejectsSoulGovernance)
    );
    assert!(paths.contains(&S3ReplayPath::ArchiveEvidenceCannotBecomeSoulCore));
}

#[test]
fn s3_replay_aggregates_required_gate_counts_without_raw_private_exposure() {
    let report = run_s3_replay();

    assert!(report.accepted >= 6);
    assert!(report.rejected >= 2);
    assert_eq!(report.deferred, 0);
    assert!(report.selected >= 6);
    assert!(report.skipped >= 1);
    assert!(report.projected >= 4);
    assert!(report.privacy_filtered >= 2);
    assert!(!report.subject_assembly_sources_used.is_empty());
    assert!(!report.raw_private_exposed);
}

#[test]
fn soul_governance_summary_feeds_subject_projection_without_prompting_raw_private() {
    let report = run_s3_replay();
    let path = report
        .paths
        .iter()
        .find(|path| path.path == S3ReplayPath::SoulGovernanceSummaryFeedsSubjectProjection)
        .expect("soul governance summary replay path");

    assert_eq!(path.profile, RuntimeProfile::DevFull);
    assert!(path.selected_planes.contains(&MemoryPlane::SoulGovernance));
    assert!(path
        .projected_planes
        .contains(&MemoryPlane::SubjectProjection));
    assert!(has_source(&path.subject_assembly_sources_used, "SelfCore"));
    assert!(has_source(
        &path.subject_assembly_sources_used,
        "SelfContinuity"
    ));
    assert!(path.privacy_filtered > 0);
    assert!(!path.raw_private_exposed);
}

#[test]
fn program_evidence_supports_subject_assembly_report() {
    let report = run_s3_replay();
    let path = report
        .paths
        .iter()
        .find(|path| path.path == S3ReplayPath::ProgramEvidenceSupportsSubjectAssembly)
        .expect("program evidence subject assembly path");

    assert!(path.selected_planes.contains(&MemoryPlane::SharedFactual));
    assert!(path
        .selected_planes
        .contains(&MemoryPlane::ContinuityCapsule));
    assert!(path.selected_planes.contains(&MemoryPlane::Procedural));
    assert!(has_source(
        &path.subject_assembly_sources_used,
        "ProgramMemory"
    ));
    assert!(has_source(
        &path.subject_assembly_sources_used,
        "SelfContinuity"
    ));
    assert!(has_source(&path.subject_assembly_sources_used, "Task"));
    assert!(path
        .rejected_reasons
        .contains(&WriteRejectReason::NeedsDistillation));
}

#[test]
fn private_material_is_filtered_from_prompt_projection() {
    let report = run_s3_replay();
    let path = report
        .paths
        .iter()
        .find(|path| path.path == S3ReplayPath::PrivateMaterialFilteredFromPrompt)
        .expect("private material prompt filter path");

    assert_eq!(path.projected_planes, vec![MemoryPlane::SubjectProjection]);
    assert!(path.privacy_filtered > 0);
    assert!(!path.raw_private_exposed);
}

#[test]
fn operator_inspection_reports_presence_only() {
    let report = run_s3_replay();
    let path = report
        .paths
        .iter()
        .find(|path| path.path == S3ReplayPath::OperatorInspectionShowsPresenceOnly)
        .expect("operator inspection path");

    assert!(path.operator_presence_only);
    assert!(path.inspection_private_content_bytes == 0);
    assert!(path.privacy_filtered > 0);
    assert!(!path.raw_private_exposed);
}

#[test]
fn esp_compact_accepts_subject_projection_but_rejects_soul_governance() {
    let report = run_s3_replay();
    let path = report
        .paths
        .iter()
        .find(|path| {
            path.path == S3ReplayPath::EspCompactAcceptsSubjectProjectionButRejectsSoulGovernance
        })
        .expect("esp compact profile replay path");

    assert_eq!(path.profile, RuntimeProfile::EspCompact);
    assert!(path
        .projected_planes
        .contains(&MemoryPlane::SubjectProjection));
    assert_eq!(
        path.rejected_reasons,
        vec![WriteRejectReason::ProfileRejected]
    );
    assert!(path.profile_trimmed);
}

#[test]
fn archive_evidence_cannot_become_soul_core() {
    let report = run_s3_replay();
    let path = report
        .paths
        .iter()
        .find(|path| path.path == S3ReplayPath::ArchiveEvidenceCannotBecomeSoulCore)
        .expect("archive evidence cannot become soul core path");

    assert!(path.selected_planes.contains(&MemoryPlane::ArchiveEvidence));
    assert!(!path.projected_planes.contains(&MemoryPlane::SoulGovernance));
    assert!(path
        .rejected_reasons
        .contains(&WriteRejectReason::NeedsDistillation));
    assert!(has_source(
        &path.subject_assembly_sources_missing,
        "SelfCore"
    ));
}

fn has_source(sources: &[&str], expected: &str) -> bool {
    sources.contains(&expected)
}
