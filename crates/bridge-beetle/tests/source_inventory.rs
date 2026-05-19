use bm_bridge_beetle::{
    BeetleLongTermKind, BeetleMemorySource, BeetleOriginKind, ContentHandling, MigrationPlanner,
};
use bm_core::{EvidenceState, Freshness, MemoryPlane, MentalPrivacyLayer};

#[test]
fn long_term_fact_inventory_keeps_origin_and_canonical_metadata() {
    let source =
        BeetleMemorySource::new("beetle-long-term", "lt-1", "user project is Beetle Memory")
            .origin_path("src/memory/long_term.rs")
            .origin_kind(BeetleOriginKind::LongTerm {
                kind: BeetleLongTermKind::SharedFact,
                freshness: Freshness::Current,
                evidence: EvidenceState::Canonical,
            });

    let plan = MigrationPlanner::default().plan(source);

    assert_eq!(plan.origin_path, "src/memory/long_term.rs");
    assert_eq!(
        plan.origin_kind,
        BeetleOriginKind::LongTerm {
            kind: BeetleLongTermKind::SharedFact,
            freshness: Freshness::Current,
            evidence: EvidenceState::Canonical,
        }
    );
    assert!(plan.canonical);
    assert_eq!(plan.privacy_layer, MentalPrivacyLayer::Shared);
    assert_eq!(plan.target_plane, MemoryPlane::SharedFactual);
    assert_eq!(plan.candidate.source.as_deref(), Some("beetle:lt-1"));
}

#[test]
fn inventory_routes_runtime_skill_archive_subject_and_soul_sources() {
    let planner = MigrationPlanner::default();

    let skill = planner.plan(
        BeetleMemorySource::new("beetle-runtime-skill", "skill-1", "when X fails, run Y")
            .origin_path("src/skills/runtime.rs")
            .origin_kind(BeetleOriginKind::RuntimeSkill),
    );
    assert_eq!(skill.target_plane, MemoryPlane::Procedural);
    assert!(skill.canonical);
    assert_eq!(skill.privacy_layer, MentalPrivacyLayer::Shared);
    assert_eq!(skill.content_handling, ContentHandling::MigrateContent);

    let archive = planner.plan(
        BeetleMemorySource::new("beetle-archive", "archive-1", "historical evidence")
            .origin_path("archive evidence")
            .origin_kind(BeetleOriginKind::ArchiveEvidence),
    );
    assert_eq!(archive.target_plane, MemoryPlane::ArchiveEvidence);
    assert!(!archive.canonical);
    assert_eq!(archive.content_handling, ContentHandling::MigrateContent);

    let subject = planner.plan(
        BeetleMemorySource::new("beetle-subject", "subject-1", "current subject summary")
            .origin_path("src/memory/subject_shell.rs")
            .origin_kind(BeetleOriginKind::SubjectProjection),
    );
    assert_eq!(subject.target_plane, MemoryPlane::SubjectProjection);
    assert!(!subject.canonical);
    assert_eq!(subject.content_handling, ContentHandling::FixtureOnly);

    let soul = planner.plan(
        BeetleMemorySource::new("beetle-soul", "soul-1", "relationship boundary v3")
            .origin_path("src/memory/self_authored_core.rs")
            .origin_kind(BeetleOriginKind::SoulGovernance),
    );
    assert_eq!(soul.target_plane, MemoryPlane::SoulGovernance);
    assert_eq!(soul.privacy_layer, MentalPrivacyLayer::Relational);
}

#[test]
fn private_sources_register_presence_without_migrating_raw_material() {
    let source = BeetleMemorySource::new(
        "beetle-private-garden",
        "private-1",
        "RAW PRIVATE MATERIAL SHOULD NOT MOVE",
    )
    .origin_path("src/memory/private_garden.rs")
    .origin_kind(BeetleOriginKind::PrivatePresence);

    let plan = MigrationPlanner::default().plan(source);

    assert_eq!(plan.target_plane, MemoryPlane::SoulGovernance);
    assert!(!plan.canonical);
    assert_eq!(plan.privacy_layer, MentalPrivacyLayer::Private);
    assert_eq!(plan.content_handling, ContentHandling::PresenceOnly);
    assert!(!plan.candidate.content.contains("RAW PRIVATE MATERIAL"));
    assert!(plan.candidate.content.contains("private source present"));
}
