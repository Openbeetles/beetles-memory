use bm_core::memory::{
    build_long_term_memory_facet_index_doc, FacetReportAudience, HumanFacetSuggestion,
    LongTermMemoryConfidence, LongTermMemoryEntry, LongTermMemoryFreshness, LongTermMemoryKind,
    LongTermMemorySourceScope, LongTermMemorySourceType, MemoryFacetNamespace,
    MemoryFacetOwnerPlane, MemoryFacetStatus, StructuredFacetParser,
};

fn fixture_long_term_entry() -> LongTermMemoryEntry {
    LongTermMemoryEntry {
        id: "ltm:project:agent-memory-w4".to_string(),
        kind: LongTermMemoryKind::Project,
        topic: "agent-memory/w4/facet-index".to_string(),
        content: "Facet retrieval must use governed typed evidence rather than regex tags."
            .to_string(),
        keywords: vec![
            "facet-index".to_string(),
            "recall-quality".to_string(),
            "governed-memory".to_string(),
        ],
        source_chat_id: Some("chat-alpha".to_string()),
        source_type: LongTermMemorySourceType::Conversation,
        source_scope: LongTermMemorySourceScope::User,
        confidence: LongTermMemoryConfidence::High,
        freshness: LongTermMemoryFreshness::Dynamic,
        stale_hint: Default::default(),
        supporting_citations: vec![
            "external_eval:D1:12|session_1".to_string(),
            "external_eval:D1:13|session_1".to_string(),
            "archive:release#turn=7".to_string(),
        ],
        evidence_count: 3,
        created_at: 1_800_000_000,
        updated_at: 1_800_000_030,
        observed_at: 1_800_000_010,
        last_confirmed_at: 1_800_000_020,
        source_revision: 42,
        last_used_at: 0,
    }
}

#[test]
fn long_term_memory_generates_governed_facets_from_accepted_fields() {
    let doc = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );

    assert_eq!(doc.owner_record_id, "ltm:project:agent-memory-w4");
    assert_eq!(doc.owner_plane, MemoryFacetOwnerPlane::LongTerm);
    assert_eq!(doc.schema_version, 1);
    assert_eq!(doc.facet_index_revision, 3);
    assert_eq!(doc.status, MemoryFacetStatus::Active);
    assert_eq!(doc.memory_space_id, "space:main");
    assert_eq!(doc.subject_ids, vec!["subject:user"]);
    assert_eq!(doc.source_revision, 42);

    for namespace in [
        MemoryFacetNamespace::Kind,
        MemoryFacetNamespace::Topic,
        MemoryFacetNamespace::Keyword,
        MemoryFacetNamespace::SourceScope,
        MemoryFacetNamespace::SourceType,
        MemoryFacetNamespace::Freshness,
        MemoryFacetNamespace::Temporal,
        MemoryFacetNamespace::Evidence,
    ] {
        assert!(
            doc.exact_facets
                .iter()
                .any(|facet| facet.namespace == namespace),
            "missing governed facet namespace {namespace:?}"
        );
    }
}

#[test]
fn facet_index_keeps_exact_and_expanded_hierarchical_facets_separate() {
    let doc = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );

    let exact_topic = doc
        .exact_facets
        .iter()
        .find(|facet| facet.namespace == MemoryFacetNamespace::Topic)
        .expect("exact topic facet");
    assert!(exact_topic.derived_from_exact_facet_id.is_none());

    let expanded_topic = doc
        .expanded_facets
        .iter()
        .find(|facet| {
            facet.namespace == MemoryFacetNamespace::Topic
                && facet.derived_from_exact_facet_id.as_deref()
                    == Some(exact_topic.facet_id.as_str())
        })
        .expect("expanded topic facet");

    assert_ne!(exact_topic.facet_id, expanded_topic.facet_id);
    assert!(expanded_topic.expansion_rule_id.is_some());
}

#[test]
fn facet_index_uses_canonical_evidence_group_without_collapsing_distinct_sources() {
    let doc = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );

    let groups = doc
        .canonical_evidence_refs
        .iter()
        .map(|evidence| evidence.canonical_evidence_group.as_str())
        .collect::<Vec<_>>();

    assert!(groups.contains(&"external_eval:d1:12"));
    assert!(groups.contains(&"external_eval:d1:13"));
    assert!(groups.contains(&"archive:release#turn=7"));
}

#[test]
fn facet_parser_rejects_regex_only_entity_and_time_facets() {
    let entity = StructuredFacetParser::parse_entity_anchor("person:/alice.*/", "turn:1");
    assert!(!entity.accepted);
    assert_eq!(entity.reason, "regex_only_entity_facet_rejected");

    let temporal = StructuredFacetParser::parse_temporal_anchor("2026-.*", "turn:1");
    assert!(!temporal.accepted);
    assert_eq!(temporal.reason, "regex_only_temporal_facet_rejected");
}

#[test]
fn facet_value_contract_uses_typed_values_not_display_string_splitting() {
    let doc = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );
    let topic = doc
        .exact_facets
        .iter()
        .find(|facet| facet.namespace == MemoryFacetNamespace::Topic)
        .expect("topic facet");
    let value = serde_json::to_value(&topic.value).expect("facet value json");

    assert_eq!(value["kind"], "topic");
    assert_eq!(value["normalized"], "agent-memory/w4/facet-index");
    assert_eq!(
        value["segments"],
        serde_json::json!(["agent-memory", "w4", "facet-index"])
    );
    assert!(value.get("display_string").is_none());
}

#[test]
fn facet_report_view_redacts_sensitive_metadata_by_default() {
    let doc = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );
    let report = doc.report_view(FacetReportAudience::HostUi);

    assert!(report.redacted_sensitive_metadata);
    assert_eq!(report.visible_canonical_evidence_groups.len(), 0);
    assert_eq!(
        report.redacted_canonical_evidence_group_count,
        doc.canonical_evidence_refs.len()
    );

    let owner_report = doc.report_view(FacetReportAudience::OwnerRaw);
    assert!(!owner_report.redacted_sensitive_metadata);
    assert_eq!(
        owner_report.visible_canonical_evidence_groups.len(),
        doc.canonical_evidence_refs.len()
    );
}

#[test]
fn human_facet_suggestion_requires_governed_proposal() {
    let suggestion = HumanFacetSuggestion {
        suggestion_id: "suggestion:raw-tag".to_string(),
        suggested_by: "operator".to_string(),
        owner_record_id: "ltm:project:agent-memory-w4".to_string(),
        proposed_facets: vec!["tag:obsidian-style".to_string()],
        governed_proposal_id: None,
    };

    let validation = suggestion.validate_contract();
    assert!(!validation.accepted);
    assert_eq!(
        validation.reason,
        "human_facet_suggestion_requires_governed_proposal"
    );

    let governed = HumanFacetSuggestion {
        governed_proposal_id: Some("proposal:facet:1".to_string()),
        ..suggestion
    };
    assert!(governed.validate_contract().accepted);
}
