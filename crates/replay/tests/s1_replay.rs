use bm_core::{MemoryPlane, RuntimeProfile, WriteRejectReason};
use bm_replay::{run_s1_replay, S1ReplayPath};

#[test]
fn s1_replay_fixture_reports_required_capability_and_projection_paths() {
    let report = run_s1_replay();

    assert!(report.accepted >= 5);
    assert_eq!(report.rejected, 1);
    assert!(report.selected >= 5);
    assert!(report.projected >= 4);
    assert!(!report.warnings.is_empty());

    let paths: Vec<_> = report.paths.iter().map(|path| path.path).collect();
    assert!(paths.contains(&S1ReplayPath::FactualPromptProjection));
    assert!(paths.contains(&S1ReplayPath::ProceduralAdapterProjection));
    assert!(paths.contains(&S1ReplayPath::ArchiveEvidenceNeedsDistillation));
    assert!(paths.contains(&S1ReplayPath::SoulGovernanceSubjectProjection));
    assert!(paths.contains(&S1ReplayPath::EspCompactProjectionTrim));
}

#[test]
fn archive_evidence_recall_does_not_become_canonical_factual_write() {
    let report = run_s1_replay();
    let archive = report
        .paths
        .iter()
        .find(|path| path.path == S1ReplayPath::ArchiveEvidenceNeedsDistillation)
        .expect("archive evidence replay path");

    assert_eq!(archive.selected_planes, vec![MemoryPlane::ArchiveEvidence]);
    assert_eq!(
        archive.rejected_reasons,
        vec![WriteRejectReason::NeedsDistillation]
    );
    assert!(!archive.canonical_projection);
}

#[test]
fn soul_governance_projection_never_exposes_raw_private_text() {
    let report = run_s1_replay();
    let soul = report
        .paths
        .iter()
        .find(|path| path.path == S1ReplayPath::SoulGovernanceSubjectProjection)
        .expect("soul governance replay path");

    assert_eq!(soul.projected_planes, vec![MemoryPlane::SubjectProjection]);
    assert!(soul.privacy_filtered);
    assert!(!soul.raw_private_exposed);
}

#[test]
fn esp_compact_replay_records_profile_budget_trim() {
    let report = run_s1_replay();
    let esp = report
        .paths
        .iter()
        .find(|path| path.path == S1ReplayPath::EspCompactProjectionTrim)
        .expect("esp compact replay path");

    assert_eq!(esp.profile, RuntimeProfile::EspCompact);
    assert!(esp.projection_trimmed);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("EspCompact projection trimmed")));
}
