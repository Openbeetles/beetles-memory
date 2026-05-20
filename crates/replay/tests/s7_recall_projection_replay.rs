use bm_core::{MemoryPlane, PromptRecallIntent, RuntimeProfile};

#[test]
fn s7_replay_covers_recall_projection_paths() {
    let report = bm_replay::run_s7_replay();

    assert!(report.prompt_assemblies >= 3);
    assert!(report.projection_blocks >= 3);
    assert!(report.sanitized_fragments >= 1);
    assert!(report.budget_trimmed);
    assert!(!report.raw_private_exposed);
    assert!(report.intents.contains(&PromptRecallIntent::Continuity));
    assert!(report.selected_planes.contains(&MemoryPlane::Procedural));
    assert!(report
        .selected_planes
        .contains(&MemoryPlane::ArchiveEvidence));
    assert!(report.profiles.contains(&RuntimeProfile::EspCompact));
}
