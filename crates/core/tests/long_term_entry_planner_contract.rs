use bm_core::memory::{
    canonical_evidence_ref_from_source, plan_long_term_memory_owner_mutation,
    plan_long_term_memory_upsert, scoped_long_term_memory_storage_key, CanonicalEntityKey,
    CanonicalEntityKind, CanonicalEntityRef, CanonicalRecallEvidenceFamilyGroup,
    LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryEntryPlan,
    LongTermMemoryEntryRejection, LongTermMemoryFreshness, LongTermMemoryKind,
    LongTermMemoryOwnerMutation, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryStaleHint, MemoryPrivacyClass,
};

#[test]
fn shared_factual_owner_physical_key_is_memory_space_scoped() {
    let first =
        scoped_long_term_memory_storage_key("space-a", "owner-1").expect("scoped long-term key");
    assert_eq!(
        first,
        scoped_long_term_memory_storage_key("space-a", "owner-1").expect("deterministic key")
    );
    assert_ne!(
        first,
        scoped_long_term_memory_storage_key("space-b", "owner-1").expect("space isolation")
    );
    assert!(scoped_long_term_memory_storage_key("", "owner-1").is_err());
    assert!(scoped_long_term_memory_storage_key("space-a", "").is_err());
}

const NOW: u64 = 1_900_000_000;

fn evidence(source_ref: &str) -> bm_core::memory::CanonicalEvidenceRef {
    canonical_evidence_ref_from_source(source_ref).expect("canonical evidence")
}

fn entity(
    kind: CanonicalEntityKind,
    canonical_id: &str,
    display_label: &str,
    aliases: &[&str],
    source_ref: &str,
) -> CanonicalEntityRef {
    CanonicalEntityRef {
        key: CanonicalEntityKey {
            kind,
            canonical_id: canonical_id.to_string(),
        },
        display_label: Some(display_label.to_string()),
        aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
        evidence_refs: vec![evidence(source_ref)],
    }
}

fn draft(content: &str, source_revision: Option<u64>) -> LongTermMemoryDraft {
    let citation = "transcript:space-a:chat-a:turn-1";
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Project,
        topic: "typed-entity-project".to_string(),
        content: content.to_string(),
        keywords: vec!["typed entity".to_string()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("chat-a".to_string()),
        source_type: Some(LongTermMemorySourceType::Conversation),
        source_scope: Some(LongTermMemorySourceScope::User),
        subject_visibility: bm_core::memory::MemorySubjectVisibilityPolicy::AllSubjects,
        provenance: bm_core::memory::LongTermMemoryProvenance {
            source_authority: bm_core::memory::MemoryEvidenceAuthority::UserAsserted,
            semantic_judgment_source: None,
        },
        confidence: Some(LongTermMemoryConfidence::High),
        freshness: Some(LongTermMemoryFreshness::Dynamic),
        stale_hint: Some(LongTermMemoryStaleHint::ReviewBeforeUse),
        supporting_citations: vec![citation.to_string()],
        canonical_entities: vec![entity(
            CanonicalEntityKind::Project,
            " Beetle-Memory ",
            "Beetle Memory",
            &["Beetle", " beetle "],
            citation,
        )],
        evidence_count: Some(1),
        observed_at: Some(NOW - 10),
        source_revision,
    }
}

fn created(plan: LongTermMemoryEntryPlan) -> bm_core::memory::LongTermMemoryEntry {
    match plan {
        LongTermMemoryEntryPlan::Created(entry) => entry,
        other => panic!("expected Created, got {other:?}"),
    }
}

fn expect_updated(plan: LongTermMemoryEntryPlan) -> bm_core::memory::LongTermMemoryEntry {
    match plan {
        LongTermMemoryEntryPlan::Updated(entry) => entry,
        other => panic!("expected Updated, got {other:?}"),
    }
}

#[test]
fn create_sets_owner_revision_one_and_preserves_optional_source_revision() {
    let entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));

    assert_eq!(entry.owner_revision, 1);
    assert_eq!(entry.source_revision, Some(7));
    assert_eq!(entry.canonical_entities.len(), 1);
    assert_eq!(
        entry.canonical_entities[0].key.canonical_id,
        "beetle-memory"
    );
    assert_eq!(entry.canonical_entities[0].aliases, vec!["Beetle"]);
}

#[test]
fn canonical_entity_preserves_only_structured_optional_evidence_family() {
    let mut governed = draft("v1", Some(7));
    let family =
        CanonicalRecallEvidenceFamilyGroup::from_structured_identity("transcript-session:chat-a")
            .expect("structured family")
            .into_string();
    governed.canonical_entities[0].evidence_refs[0].evidence_family_group = Some(family.clone());

    let entry = created(plan_long_term_memory_upsert(None, &governed, NOW));
    assert_eq!(
        entry.canonical_entities[0].evidence_refs[0].evidence_family_group,
        Some(family)
    );

    let mut invalid = draft("v1", Some(7));
    invalid.canonical_entities[0].evidence_refs[0].evidence_family_group =
        Some("chat-a".to_string());
    assert_eq!(
        plan_long_term_memory_upsert(None, &invalid, NOW),
        LongTermMemoryEntryPlan::Rejected(LongTermMemoryEntryRejection::InvalidCanonicalEntity)
    );
}

#[test]
fn exact_source_replay_is_noop_and_does_not_advance_owner_revision() {
    let entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));

    assert!(matches!(
        plan_long_term_memory_upsert(Some(&entry), &draft("v1", Some(7)), NOW + 100),
        LongTermMemoryEntryPlan::Noop
    ));
    assert_eq!(entry.owner_revision, 1);
}

#[test]
fn same_lineage_rejects_older_revision_and_same_revision_payload_conflict() {
    let entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));

    assert!(matches!(
        plan_long_term_memory_upsert(Some(&entry), &draft("older", Some(6)), NOW + 1),
        LongTermMemoryEntryPlan::Rejected(LongTermMemoryEntryRejection::OlderSourceRevision)
    ));
    assert!(matches!(
        plan_long_term_memory_upsert(Some(&entry), &draft("conflict", Some(7)), NOW + 1),
        LongTermMemoryEntryPlan::Rejected(LongTermMemoryEntryRejection::SourceRevisionConflict)
    ));
}

#[test]
fn higher_source_revision_updates_payload_and_advances_owner_once() {
    let entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));
    let mut incoming = draft("v2", Some(8));
    incoming.canonical_entities = vec![entity(
        CanonicalEntityKind::Product,
        "beetle-memory-sdk",
        "Beetle Memory SDK",
        &["BM SDK"],
        "transcript:space-a:chat-a:turn-1",
    )];

    let updated = expect_updated(plan_long_term_memory_upsert(
        Some(&entry),
        &incoming,
        NOW + 1,
    ));

    assert_eq!(updated.owner_revision, 2);
    assert_eq!(updated.source_revision, Some(8));
    assert_eq!(updated.content, "v2");
    assert_eq!(updated.canonical_entities.len(), 1);
    assert_eq!(
        updated.canonical_entities[0].key.kind,
        CanonicalEntityKind::Product
    );
}

#[test]
fn absent_revision_preserves_same_lineage_but_binds_none_to_new_lineage() {
    let entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));
    let mut same_lineage = draft("v2", None);
    same_lineage.observed_at = Some(NOW + 1);
    let updated = expect_updated(plan_long_term_memory_upsert(
        Some(&entry),
        &same_lineage,
        NOW + 1,
    ));
    assert_eq!(updated.source_revision, Some(7));

    let mut new_lineage = draft("v3", None);
    new_lineage.source_chat_id = Some("chat-b".to_string());
    new_lineage.observed_at = Some(NOW + 2);
    let rebound = expect_updated(plan_long_term_memory_upsert(
        Some(&updated),
        &new_lineage,
        NOW + 2,
    ));
    assert_eq!(rebound.source_revision, None);
    assert_eq!(rebound.owner_revision, 3);
}

#[test]
fn unspecified_lineage_fields_preserve_existing_source_lineage() {
    let entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));
    let mut incoming = draft("v2", None);
    incoming.source_chat_id = None;
    incoming.source_type = None;
    incoming.source_scope = None;
    incoming.observed_at = Some(NOW + 1);

    let updated = expect_updated(plan_long_term_memory_upsert(
        Some(&entry),
        &incoming,
        NOW + 1,
    ));

    assert_eq!(updated.source_chat_id, entry.source_chat_id);
    assert_eq!(updated.source_type, entry.source_type);
    assert_eq!(updated.source_scope, entry.source_scope);
    assert_eq!(updated.source_revision, Some(7));
}

#[test]
fn entity_display_label_is_optional_and_blank_normalizes_to_none() {
    let mut incoming = draft("v1", None);
    incoming.canonical_entities[0].display_label = Some("   ".to_string());

    let entry = created(plan_long_term_memory_upsert(None, &incoming, NOW));

    assert_eq!(entry.canonical_entities[0].display_label, None);
}

#[test]
fn same_content_reinforcement_unions_entity_aliases_and_evidence() {
    let mut initial = draft("v1", None);
    initial
        .supporting_citations
        .push("archive:release#turn=7".to_string());
    let entry = created(plan_long_term_memory_upsert(None, &initial, NOW));
    let mut reinforcement = draft("v1", None);
    reinforcement.observed_at = Some(NOW + 1);
    reinforcement
        .supporting_citations
        .push("archive:release#turn=7".to_string());
    reinforcement.canonical_entities = vec![entity(
        CanonicalEntityKind::Project,
        "beetle-memory",
        "Beetle Memory",
        &["BM"],
        "archive:release#turn=7",
    )];

    let updated = expect_updated(plan_long_term_memory_upsert(
        Some(&entry),
        &reinforcement,
        NOW + 1,
    ));

    assert_eq!(updated.owner_revision, 2);
    assert_eq!(updated.canonical_entities.len(), 1);
    assert_eq!(updated.canonical_entities[0].aliases, vec!["Beetle", "BM"]);
    assert_eq!(updated.canonical_entities[0].evidence_refs.len(), 2);
}

#[test]
fn draft_rejects_forged_or_cross_draft_entity_evidence_and_label_conflicts() {
    let mut forged = draft("v1", None);
    forged.canonical_entities[0].evidence_refs[0].source_kind = "forged".to_string();
    assert!(matches!(
        plan_long_term_memory_upsert(None, &forged, NOW),
        LongTermMemoryEntryPlan::Rejected(LongTermMemoryEntryRejection::InvalidCanonicalEntity)
    ));

    let mut forged_group = draft("v1", None);
    forged_group.canonical_entities[0].evidence_refs[0].canonical_evidence_group =
        "forged-group".to_string();
    assert!(matches!(
        plan_long_term_memory_upsert(None, &forged_group, NOW),
        LongTermMemoryEntryPlan::Rejected(LongTermMemoryEntryRejection::InvalidCanonicalEntity)
    ));

    let mut forged_authority = draft("v1", None);
    forged_authority.canonical_entities[0].evidence_refs[0].source_authority_score += 1;
    assert!(matches!(
        plan_long_term_memory_upsert(None, &forged_authority, NOW),
        LongTermMemoryEntryPlan::Rejected(LongTermMemoryEntryRejection::InvalidCanonicalEntity)
    ));

    let mut cross_draft = draft("v1", None);
    cross_draft.canonical_entities[0].evidence_refs = vec![evidence("archive:other#turn=9")];
    assert!(matches!(
        plan_long_term_memory_upsert(None, &cross_draft, NOW),
        LongTermMemoryEntryPlan::Rejected(LongTermMemoryEntryRejection::InvalidCanonicalEntity)
    ));

    let mut conflicting = draft("v1", None);
    conflicting.canonical_entities.push(entity(
        CanonicalEntityKind::Project,
        "beetle-memory",
        "Different Project",
        &[],
        "transcript:space-a:chat-a:turn-1",
    ));
    assert!(matches!(
        plan_long_term_memory_upsert(None, &conflicting, NOW),
        LongTermMemoryEntryPlan::Rejected(LongTermMemoryEntryRejection::EntityLabelConflict)
    ));
}

#[test]
fn old_entry_schema_and_unknown_entity_kind_do_not_decode() {
    let entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));
    let mut old_entry = serde_json::to_value(entry).expect("serialize entry");
    let object = old_entry.as_object_mut().expect("entry object");
    object.remove("owner_revision");
    object.remove("canonical_entities");
    assert!(serde_json::from_value::<bm_core::memory::LongTermMemoryEntry>(old_entry).is_err());

    assert!(serde_json::from_value::<CanonicalEntityKind>(serde_json::json!("custom")).is_err());
}

#[test]
fn owner_control_mutations_preserve_source_revision_and_noop_on_same_value() {
    let entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));

    assert!(matches!(
        plan_long_term_memory_owner_mutation(
            &entry,
            &LongTermMemoryOwnerMutation::ChangePrivacy(entry.privacy),
            NOW + 1,
        ),
        LongTermMemoryEntryPlan::Noop
    ));

    let updated = expect_updated(plan_long_term_memory_owner_mutation(
        &entry,
        &LongTermMemoryOwnerMutation::MarkStale(LongTermMemoryStaleHint::VerifyAgainstCurrentState),
        NOW + 1,
    ));
    assert_eq!(updated.owner_revision, 2);
    assert_eq!(updated.source_revision, Some(7));
}

#[test]
fn evidence_compaction_is_an_owner_mutation_not_a_source_replay() {
    let mut entry = created(plan_long_term_memory_upsert(
        None,
        &draft("v1", Some(7)),
        NOW,
    ));
    entry.supporting_citations = vec![
        "turn-2".to_string(),
        "turn-1".to_string(),
        "turn-1".to_string(),
    ];
    entry.evidence_count = 9;
    entry.last_confirmed_at = Some(NOW);

    let updated = expect_updated(plan_long_term_memory_owner_mutation(
        &entry,
        &LongTermMemoryOwnerMutation::CompactEvidenceMetadata {
            supporting_citations: vec!["turn-1".to_string(), "turn-2".to_string()],
            evidence_count: 2,
        },
        NOW + 1,
    ));

    assert_eq!(updated.supporting_citations, vec!["turn-1", "turn-2"]);
    assert_eq!(updated.evidence_count, 2);
    assert_eq!(updated.last_confirmed_at, Some(NOW));
    assert_eq!(updated.source_revision, Some(7));
    assert_eq!(updated.owner_revision, entry.owner_revision + 1);
}
