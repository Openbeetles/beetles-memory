use bm_core::memory::{
    allocate_recall_delivery_candidates, build_long_term_memory_facet_index_doc,
    memory_facet_manifest_key, memory_facet_posting_key, scoped_memory_facet_owner_storage_key,
    score_recall_delivery_texts, validate_memory_facet_manifest, validate_memory_facet_posting,
    validate_memory_facet_read_chain, CanonicalEntityKey, CanonicalEntityKind, CanonicalEntityRef,
    CanonicalEvidenceRef, FacetReportAudience, HumanFacetSuggestion, LongTermMemoryConfidence,
    LongTermMemoryEntry, LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemorySourceScope,
    LongTermMemorySourceType, MemoryFacetIndexDoc, MemoryFacetIndexManifest, MemoryFacetNamespace,
    MemoryFacetOwnerPlane, MemoryFacetOwnerVersion, MemoryFacetPostingDoc,
    MemoryFacetPostingRevision, MemoryFacetStatus, MemoryFacetValidationError, MemoryPrivacyClass,
    QueryFacetInput, QueryFacetParser, RecallDeliveryCandidate, RecallDeliveryOrderingPolicy,
    RecallDeliverySelectionDropReason, RecallDeliveryText, StructuredFacetParser, TemporalAnchor,
    TemporalAnchorKind, TemporalAnchorPrecision, MEMORY_FACET_SCHEMA_VERSION,
};

#[test]
fn facet_owner_physical_key_is_scoped_before_any_owner_read() {
    let base = scoped_memory_facet_owner_storage_key("space-a", "subject-a", "owner-1")
        .expect("scoped facet owner key");
    assert_eq!(
        base,
        scoped_memory_facet_owner_storage_key("space-a", "subject-a", "owner-1")
            .expect("deterministic key")
    );
    assert_ne!(
        base,
        scoped_memory_facet_owner_storage_key("space-b", "subject-a", "owner-1")
            .expect("space isolation")
    );
    assert_ne!(
        base,
        scoped_memory_facet_owner_storage_key("space-a", "subject-b", "owner-1")
            .expect("subject isolation")
    );
}

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
        privacy: MemoryPrivacyClass::SharedWithSubject,
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
        canonical_entities: Vec::new(),
        evidence_count: 3,
        created_at: 1_800_000_000,
        updated_at: 1_800_000_030,
        observed_at: 1_800_000_010,
        last_confirmed_at: 1_800_000_020,
        source_revision: Some(42),
        owner_revision: 5,
        last_used_at: 0,
    }
}

#[test]
fn delivery_allocator_preserves_distinct_evidence_groups_before_duplicate_rank() {
    let candidates = vec![
        RecallDeliveryCandidate {
            candidate_id: "candidate-shared-high".to_string(),
            canonical_evidence_groups: vec!["evidence:shared".to_string()],
            evidence_family_groups: vec!["family:shared".to_string()],
            owner_available: true,
            citation_eligible: true,
            privacy_eligible: true,
            temporal_eligible: true,
            source_rank: Some(1),
            expanded_rank: Some(1),
            reranked_rank: 1,
            relevance_score: 100,
            authority_score: 100,
        },
        RecallDeliveryCandidate {
            candidate_id: "candidate-shared-second".to_string(),
            canonical_evidence_groups: vec!["evidence:shared".to_string()],
            evidence_family_groups: vec!["family:shared".to_string()],
            owner_available: true,
            citation_eligible: true,
            privacy_eligible: true,
            temporal_eligible: true,
            source_rank: Some(2),
            expanded_rank: Some(2),
            reranked_rank: 2,
            relevance_score: 90,
            authority_score: 100,
        },
        RecallDeliveryCandidate {
            candidate_id: "candidate-distinct".to_string(),
            canonical_evidence_groups: vec!["evidence:distinct".to_string()],
            evidence_family_groups: vec!["family:distinct".to_string()],
            owner_available: true,
            citation_eligible: true,
            privacy_eligible: true,
            temporal_eligible: true,
            source_rank: Some(3),
            expanded_rank: Some(3),
            reranked_rank: 3,
            relevance_score: 80,
            authority_score: 100,
        },
        RecallDeliveryCandidate {
            candidate_id: "candidate-private".to_string(),
            canonical_evidence_groups: vec!["evidence:private".to_string()],
            evidence_family_groups: vec!["family:private".to_string()],
            owner_available: true,
            citation_eligible: true,
            privacy_eligible: false,
            temporal_eligible: true,
            source_rank: Some(4),
            expanded_rank: Some(4),
            reranked_rank: 4,
            relevance_score: 70,
            authority_score: 100,
        },
        RecallDeliveryCandidate {
            candidate_id: "candidate-superseded".to_string(),
            canonical_evidence_groups: vec!["evidence:stale".to_string()],
            evidence_family_groups: vec!["family:stale".to_string()],
            owner_available: true,
            citation_eligible: true,
            privacy_eligible: true,
            temporal_eligible: false,
            source_rank: Some(5),
            expanded_rank: Some(5),
            reranked_rank: 5,
            relevance_score: 60,
            authority_score: 100,
        },
    ];

    let report = allocate_recall_delivery_candidates(
        &candidates,
        2,
        RecallDeliveryOrderingPolicy::EvidenceFamilyRotationWithinEqualUtility,
    );

    assert_eq!(
        report.selected_candidate_ids,
        vec![
            "candidate-shared-high".to_string(),
            "candidate-distinct".to_string()
        ]
    );
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "candidate-shared-second"
            && decision.drop_reason
                == Some(RecallDeliverySelectionDropReason::DuplicateEvidenceGroup)
    }));
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "candidate-private"
            && decision.drop_reason == Some(RecallDeliverySelectionDropReason::PrivacyScopeBlocked)
    }));
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "candidate-superseded"
            && decision.drop_reason == Some(RecallDeliverySelectionDropReason::TemporalSuperseded)
    }));
}

#[test]
fn delivery_allocator_rejects_missing_owner_before_it_consumes_budget() {
    let candidates = vec![
        RecallDeliveryCandidate {
            candidate_id: "missing-owner".to_string(),
            canonical_evidence_groups: vec!["evidence:missing".to_string()],
            evidence_family_groups: vec!["family:missing".to_string()],
            owner_available: false,
            citation_eligible: true,
            privacy_eligible: false,
            temporal_eligible: true,
            source_rank: Some(1),
            expanded_rank: Some(1),
            reranked_rank: 1,
            relevance_score: 1_000,
            authority_score: 1_000,
        },
        RecallDeliveryCandidate {
            candidate_id: "governed-owner".to_string(),
            canonical_evidence_groups: vec!["evidence:governed".to_string()],
            evidence_family_groups: vec!["family:governed".to_string()],
            owner_available: true,
            citation_eligible: true,
            privacy_eligible: true,
            temporal_eligible: true,
            source_rank: Some(2),
            expanded_rank: Some(2),
            reranked_rank: 2,
            relevance_score: 10,
            authority_score: 10,
        },
    ];

    let report = allocate_recall_delivery_candidates(
        &candidates,
        1,
        RecallDeliveryOrderingPolicy::EvidenceFamilyRotationWithinEqualUtility,
    );

    assert_eq!(report.selected_candidate_ids, vec!["governed-owner"]);
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "missing-owner"
            && decision.drop_reason
                == Some(RecallDeliverySelectionDropReason::OwnerRecordUnavailable)
    }));
}

#[test]
fn evidence_family_rotation_off_keeps_exact_group_deduplication_enabled() {
    let candidate = |id: &str, group: &str, rank: usize| RecallDeliveryCandidate {
        candidate_id: id.to_string(),
        canonical_evidence_groups: vec![group.to_string()],
        evidence_family_groups: vec![format!("family:{group}")],
        owner_available: true,
        citation_eligible: true,
        privacy_eligible: true,
        temporal_eligible: true,
        source_rank: Some(rank),
        expanded_rank: Some(rank),
        reranked_rank: rank,
        relevance_score: 100_u32.saturating_sub(rank as u32),
        authority_score: 100,
    };
    let candidates = vec![
        candidate("shared-first", "evidence:shared", 1),
        candidate("shared-second", "evidence:shared", 2),
        candidate("distinct", "evidence:distinct", 3),
    ];

    let report = allocate_recall_delivery_candidates(
        &candidates,
        2,
        RecallDeliveryOrderingPolicy::RelevanceRank,
    );

    assert_eq!(
        report.selected_candidate_ids,
        vec!["shared-first", "distinct"]
    );
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "shared-second"
            && decision.drop_reason
                == Some(RecallDeliverySelectionDropReason::DuplicateEvidenceGroup)
    }));
}

#[test]
fn delivery_allocator_never_backfills_an_exact_group_duplicate() {
    let candidate = |id: &str, rank: usize| RecallDeliveryCandidate {
        candidate_id: id.to_string(),
        canonical_evidence_groups: vec!["evidence:shared".to_string()],
        evidence_family_groups: vec!["family:shared".to_string()],
        owner_available: true,
        citation_eligible: true,
        privacy_eligible: true,
        temporal_eligible: true,
        source_rank: Some(rank),
        expanded_rank: Some(rank),
        reranked_rank: rank,
        relevance_score: 100_u32.saturating_sub(rank as u32),
        authority_score: 100,
    };
    let candidates = vec![candidate("primary", 1), candidate("duplicate", 2)];

    let report = allocate_recall_delivery_candidates(
        &candidates,
        8,
        RecallDeliveryOrderingPolicy::EvidenceFamilyRotationWithinEqualUtility,
    );

    assert_eq!(report.selected_candidate_ids, vec!["primary"]);
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "duplicate"
            && decision.drop_reason
                == Some(RecallDeliverySelectionDropReason::DuplicateEvidenceGroup)
    }));
}

#[test]
fn delivery_allocator_rejects_partial_exact_group_overlap() {
    let candidate = |id: &str, groups: &[&str], rank: usize| RecallDeliveryCandidate {
        candidate_id: id.to_string(),
        canonical_evidence_groups: groups.iter().map(|group| group.to_string()).collect(),
        evidence_family_groups: vec![format!("family:{id}")],
        owner_available: true,
        citation_eligible: true,
        privacy_eligible: true,
        temporal_eligible: true,
        source_rank: Some(rank),
        expanded_rank: Some(rank),
        reranked_rank: rank,
        relevance_score: 100_u32.saturating_sub(rank as u32),
        authority_score: 100,
    };
    let candidates = vec![
        candidate("a-b-first", &["evidence:a", "evidence:b"], 1),
        candidate("b-c-second", &["evidence:b", "evidence:c"], 2),
    ];

    let report = allocate_recall_delivery_candidates(
        &candidates,
        2,
        RecallDeliveryOrderingPolicy::RelevanceRank,
    );

    assert_eq!(report.selected_candidate_ids, vec!["a-b-first"]);
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "b-c-second"
            && !decision.selected
            && decision.drop_reason
                == Some(RecallDeliverySelectionDropReason::DuplicateEvidenceGroup)
    }));
    let selected_groups = report
        .decisions
        .iter()
        .filter(|decision| decision.selected)
        .flat_map(|decision| decision.canonical_evidence_groups.iter())
        .collect::<Vec<_>>();
    assert_eq!(
        selected_groups.len(),
        selected_groups
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
}

#[test]
fn evidence_family_rotation_round_robins_only_within_equal_utility() {
    let candidate = |id: &str, group: &str, family: &str, rank: usize| RecallDeliveryCandidate {
        candidate_id: id.to_string(),
        canonical_evidence_groups: vec![group.to_string()],
        evidence_family_groups: vec![family.to_string()],
        owner_available: true,
        citation_eligible: true,
        privacy_eligible: true,
        temporal_eligible: true,
        source_rank: Some(rank),
        expanded_rank: Some(rank),
        reranked_rank: rank,
        relevance_score: 100_u32.saturating_sub(rank as u32),
        authority_score: 100,
    };
    let candidates = vec![
        candidate("family-a-1", "a:1", "family:a", 1),
        candidate("family-a-2", "a:2", "family:a", 1),
        candidate("family-a-3", "a:3", "family:a", 1),
        candidate("family-b-1", "b:1", "family:b", 1),
    ];

    let covered = allocate_recall_delivery_candidates(
        &candidates,
        3,
        RecallDeliveryOrderingPolicy::EvidenceFamilyRotationWithinEqualUtility,
    );
    let rank_only = allocate_recall_delivery_candidates(
        &candidates,
        3,
        RecallDeliveryOrderingPolicy::RelevanceRank,
    );

    assert_eq!(
        covered.selected_candidate_ids,
        vec![
            "family-a-1".to_string(),
            "family-b-1".to_string(),
            "family-a-2".to_string(),
        ]
    );
    assert_eq!(
        rank_only.selected_candidate_ids,
        vec![
            "family-a-1".to_string(),
            "family-a-2".to_string(),
            "family-a-3".to_string(),
        ]
    );
}

#[test]
fn delivery_rejects_missing_governed_citation_before_budget() {
    let candidate = |id: &str, citation_eligible: bool, rank: usize| RecallDeliveryCandidate {
        candidate_id: id.to_string(),
        canonical_evidence_groups: if citation_eligible {
            vec![format!("evidence:{id}")]
        } else {
            Vec::new()
        },
        evidence_family_groups: Vec::new(),
        owner_available: true,
        citation_eligible,
        privacy_eligible: true,
        temporal_eligible: true,
        source_rank: Some(rank),
        expanded_rank: Some(rank),
        reranked_rank: rank,
        relevance_score: 1_000_u32.saturating_sub(rank as u32),
        authority_score: 1_000,
    };
    let candidates = vec![
        candidate("missing-citation", false, 1),
        candidate("governed-citation", true, 2),
    ];

    let report = allocate_recall_delivery_candidates(
        &candidates,
        1,
        RecallDeliveryOrderingPolicy::RelevanceRank,
    );

    assert_eq!(report.selected_candidate_ids, vec!["governed-citation"]);
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "missing-citation"
            && decision.drop_reason == Some(RecallDeliverySelectionDropReason::CitationMissing)
    }));
    assert_eq!(
        RecallDeliverySelectionDropReason::CitationMissing.label(),
        "citation_missing"
    );
}

#[test]
fn delivery_bm25_with_word_ngrams_prefers_query_specific_owner_content() {
    let scores = score_recall_delivery_texts(
        "When did Melanie paint a sunrise?",
        &[
            RecallDeliveryText {
                candidate_id: "generic",
                text: "Melanie went to a community event and talked with friends.",
            },
            RecallDeliveryText {
                candidate_id: "specific",
                text: "Melanie painted a sunrise during an early morning art class.",
            },
            RecallDeliveryText {
                candidate_id: "unrelated",
                text: "Caroline discussed her university plans.",
            },
        ],
    );
    let score = |candidate_id: &str| {
        scores
            .iter()
            .find(|score| score.candidate_id == candidate_id)
            .expect("candidate score")
            .score
    };

    assert!(score("specific") > score("generic"));
    assert!(score("generic") > score("unrelated"));
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
    assert_eq!(doc.schema_version, MEMORY_FACET_SCHEMA_VERSION);
    assert_eq!(doc.facet_index_revision, 3);
    assert_eq!(doc.status, MemoryFacetStatus::Active);
    assert_eq!(doc.memory_space_id, "space:main");
    assert_eq!(doc.subject_ids, vec!["subject:user"]);
    assert_eq!(doc.owner_revision, 5);

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
fn exact_postings_exclude_evidence_metadata_and_weak_identifier_fragments() {
    let doc = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );
    let posting_keys = doc
        .posting_keys_for_subject("subject:user")
        .expect("mounted subject posting keys");
    let keyword = QueryFacetParser::parse(QueryFacetInput::Keyword("recall-quality".to_string()));
    let keyword = keyword.facets.first().expect("typed keyword facet");

    assert!(posting_keys.contains(
        &memory_facet_posting_key("space:main", "subject:user", keyword).expect("posting key")
    ));
    for forbidden in [
        "external",
        "eval",
        "d1",
        "12",
        "session",
        "session_1",
        "external_eval:d1:12",
    ] {
        let query = QueryFacetParser::parse(QueryFacetInput::Keyword(forbidden.to_string()));
        let query = query.facets.first().expect("typed forbidden keyword");
        assert!(
            !posting_keys.contains(
                &memory_facet_posting_key("space:main", "subject:user", query)
                    .expect("posting key")
            ),
            "weak evidence metadata became an exact posting: {forbidden}"
        );
    }
}

#[test]
fn typed_posting_keys_separate_namespace_and_topic_match_kind() {
    let parse_one = |input| {
        let parsed = QueryFacetParser::parse(input);
        assert!(parsed.accepted, "{}", parsed.reason);
        parsed.facets.into_iter().next().expect("query facet")
    };
    let kind = parse_one(QueryFacetInput::Kind(LongTermMemoryKind::Project));
    let keyword = parse_one(QueryFacetInput::Keyword("project".to_string()));
    let topic_full = parse_one(QueryFacetInput::TopicFull("project".to_string()));
    let topic_segment = parse_one(QueryFacetInput::TopicSegments(vec!["project".to_string()]));

    let keys = [kind, keyword, topic_full, topic_segment]
        .iter()
        .map(|facet| {
            memory_facet_posting_key("space:main", "subject:user", facet).expect("posting key")
        })
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(keys.len(), 4);
}

#[test]
fn facet_keys_are_isolated_by_mounted_subject_and_reject_empty_subject() {
    let facet = QueryFacetParser::parse(QueryFacetInput::Keyword("project".to_string()))
        .facets
        .into_iter()
        .next()
        .expect("typed query facet");

    let subject_a =
        memory_facet_posting_key("space:main", "subject:a", &facet).expect("subject a posting key");
    let subject_b =
        memory_facet_posting_key("space:main", "subject:b", &facet).expect("subject b posting key");
    assert_ne!(subject_a, subject_b);
    assert_ne!(
        memory_facet_manifest_key("space:main", "subject:a").expect("subject a manifest key"),
        memory_facet_manifest_key("space:main", "subject:b").expect("subject b manifest key")
    );
    assert_eq!(
        memory_facet_posting_key("space:main", " ", &facet),
        Err(MemoryFacetValidationError::SubjectIdEmpty)
    );
    assert_eq!(
        memory_facet_manifest_key("space:main", ""),
        Err(MemoryFacetValidationError::SubjectIdEmpty)
    );
}

#[test]
fn facet_index_doc_generates_typed_posting_keys_only_for_a_mounted_subject() {
    let doc = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );

    assert!(!doc
        .posting_keys_for_subject("subject:user")
        .expect("mounted subject posting keys")
        .is_empty());
    assert_eq!(
        doc.posting_keys_for_subject("subject:other"),
        Err(MemoryFacetValidationError::SubjectNotMounted)
    );
    assert_eq!(
        doc.posting_keys_for_subject(" "),
        Err(MemoryFacetValidationError::SubjectIdEmpty)
    );
}

fn fixture_facet_read_chain() -> (
    MemoryFacetIndexManifest,
    MemoryFacetPostingDoc,
    MemoryFacetIndexDoc,
) {
    let owner = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );
    let posting_key = owner
        .posting_keys_for_subject("subject:user")
        .expect("posting keys")
        .into_iter()
        .next()
        .expect("posting key");
    let owner_version = MemoryFacetOwnerVersion {
        owner_record_id: owner.owner_record_id.clone(),
        owner_revision: owner.owner_revision,
        facet_index_revision: owner.facet_index_revision,
    };
    let posting = MemoryFacetPostingDoc {
        schema_version: MEMORY_FACET_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        subject_id: "subject:user".to_string(),
        posting_key: posting_key.clone(),
        revision: 7,
        owner_versions: vec![owner_version.clone()],
    };
    let manifest = MemoryFacetIndexManifest {
        schema_version: MEMORY_FACET_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        subject_id: "subject:user".to_string(),
        owner_doc_count: 1,
        posting_doc_count: 1,
        revision: 11,
        owner_versions: vec![owner_version],
        posting_revisions: vec![MemoryFacetPostingRevision {
            posting_key,
            revision: posting.revision,
        }],
    };

    (manifest, posting, owner)
}

#[test]
fn facet_read_chain_rejects_stale_posting_revision() {
    let (manifest, mut posting, owner) = fixture_facet_read_chain();
    posting.revision -= 1;

    assert_eq!(
        validate_memory_facet_read_chain("space:main", "subject:user", &manifest, &posting, &owner,),
        Err(MemoryFacetValidationError::PostingRevisionMismatch)
    );
}

#[test]
fn facet_read_chain_rejects_stale_owner_facet_revision() {
    let (manifest, posting, mut owner) = fixture_facet_read_chain();
    owner.facet_index_revision -= 1;

    assert_eq!(
        validate_memory_facet_read_chain("space:main", "subject:user", &manifest, &posting, &owner,),
        Err(MemoryFacetValidationError::OwnerVersionMismatch)
    );
}

#[test]
fn facet_read_chain_rejects_owner_revision_drift_independently_of_facet_revision() {
    let (manifest, posting, mut owner) = fixture_facet_read_chain();
    owner.owner_revision += 1;

    assert_eq!(
        validate_memory_facet_read_chain("space:main", "subject:user", &manifest, &posting, &owner,),
        Err(MemoryFacetValidationError::OwnerVersionMismatch)
    );
}

#[test]
fn old_facet_owner_schema_without_owner_revision_does_not_decode() {
    let (_manifest, _posting, owner) = fixture_facet_read_chain();
    let mut value = serde_json::to_value(owner).expect("serialize owner doc");
    value
        .as_object_mut()
        .expect("owner object")
        .remove("owner_revision");
    value["source_revision"] = serde_json::json!(42);

    assert!(serde_json::from_value::<MemoryFacetIndexDoc>(value).is_err());
}

#[test]
fn facet_manifest_rejects_count_membership_mismatch() {
    let (mut manifest, _posting, _owner) = fixture_facet_read_chain();
    manifest.owner_doc_count = 2;
    assert_eq!(
        validate_memory_facet_manifest("space:main", "subject:user", &manifest),
        Err(MemoryFacetValidationError::ManifestOwnerCountMismatch)
    );

    manifest.owner_doc_count = manifest.owner_versions.len();
    manifest.posting_doc_count = 2;
    assert_eq!(
        validate_memory_facet_manifest("space:main", "subject:user", &manifest),
        Err(MemoryFacetValidationError::ManifestPostingCountMismatch)
    );
}

#[test]
fn facet_manifest_rejects_missing_membership_even_when_counts_are_zero() {
    let (mut manifest, _posting, _owner) = fixture_facet_read_chain();
    manifest.owner_versions.clear();
    manifest.owner_doc_count = 0;
    assert_eq!(
        validate_memory_facet_manifest("space:main", "subject:user", &manifest),
        Err(MemoryFacetValidationError::ManifestOwnerMembershipMissing)
    );

    let (mut manifest, _posting, _owner) = fixture_facet_read_chain();
    manifest.posting_revisions.clear();
    manifest.posting_doc_count = 0;
    assert_eq!(
        validate_memory_facet_manifest("space:main", "subject:user", &manifest),
        Err(MemoryFacetValidationError::ManifestPostingMembershipMissing)
    );
}

#[test]
fn facet_read_chain_rejects_missing_membership_and_scope_mismatch() {
    let (manifest, mut posting, _owner) = fixture_facet_read_chain();
    posting.owner_versions.clear();
    assert_eq!(
        validate_memory_facet_posting("space:main", "subject:user", &manifest, &posting),
        Err(MemoryFacetValidationError::PostingOwnerMembershipMissing)
    );

    let (manifest, mut posting, owner) = fixture_facet_read_chain();
    posting.subject_id = "subject:other".to_string();
    assert_eq!(
        validate_memory_facet_read_chain("space:main", "subject:user", &manifest, &posting, &owner,),
        Err(MemoryFacetValidationError::PostingScopeMismatch)
    );
}

#[test]
fn legacy_posting_without_subject_and_revision_membership_is_rejected() {
    let legacy = serde_json::json!({
        "schema_version": 2,
        "memory_space_id": "space:main",
        "posting_key": "facet-posting:legacy",
        "owner_record_ids": ["ltm:project:agent-memory-w4"]
    });

    let error = serde_json::from_value::<MemoryFacetPostingDoc>(legacy)
        .expect_err("legacy posting must not decode as the current schema");
    assert!(error.to_string().contains("subject_id"));
}

#[test]
fn owner_facets_match_only_parser_owned_typed_query_facets() {
    let doc = build_long_term_memory_facet_index_doc(
        &fixture_long_term_entry(),
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );
    let parse_one = |input| {
        QueryFacetParser::parse(input)
            .facets
            .into_iter()
            .next()
            .expect("typed query facet")
    };
    let cases = [
        parse_one(QueryFacetInput::Kind(LongTermMemoryKind::Project)),
        parse_one(QueryFacetInput::TopicFull(
            "agent-memory/w4/facet-index".to_string(),
        )),
        parse_one(QueryFacetInput::TopicSegments(vec!["w4".to_string()])),
        parse_one(QueryFacetInput::Keyword("recall-quality".to_string())),
        parse_one(QueryFacetInput::SourceScope(
            LongTermMemorySourceScope::User,
        )),
        parse_one(QueryFacetInput::SourceType(
            LongTermMemorySourceType::Conversation,
        )),
        parse_one(QueryFacetInput::Freshness(LongTermMemoryFreshness::Dynamic)),
    ];

    for query in cases {
        assert!(
            doc.exact_facets
                .iter()
                .chain(doc.expanded_facets.iter())
                .any(|facet| facet.matches_query_facet(&query)),
            "typed query facet did not match owner facet: {query:?}"
        );
    }

    let wrong_namespace = parse_one(QueryFacetInput::Keyword("project".to_string()));
    let kind = doc
        .exact_facets
        .iter()
        .find(|facet| facet.namespace == MemoryFacetNamespace::Kind)
        .expect("kind facet");
    assert!(!kind.matches_query_facet(&wrong_namespace));
}

#[test]
fn query_facet_parser_preserves_whole_cjk_keyword_without_ngrams() {
    let parsed = QueryFacetParser::parse(QueryFacetInput::Keyword("记忆召回".to_string()));

    assert!(parsed.accepted);
    assert_eq!(parsed.facets.len(), 1);
    assert_eq!(parsed.facets[0].canonical_value(), "记忆召回");
}

#[test]
fn unresolved_entity_and_temporal_query_facets_fail_closed() {
    let entity = QueryFacetParser::parse(QueryFacetInput::UnresolvedEntity("Alice".to_string()));
    let temporal =
        QueryFacetParser::parse(QueryFacetInput::UnresolvedTemporal("last week".to_string()));

    assert!(!entity.accepted);
    assert_eq!(entity.reason, "entity_query_facet_requires_typed_anchor");
    assert!(entity.facets.is_empty());
    assert!(!temporal.accepted);
    assert_eq!(
        temporal.reason,
        "temporal_query_facet_requires_typed_anchor"
    );
    assert!(temporal.facets.is_empty());
}

#[test]
fn malformed_typed_entity_and_temporal_query_facets_fail_closed() {
    let evidence = CanonicalEvidenceRef {
        source_ref: "turn:1".to_string(),
        canonical_evidence_group: "turn:1".to_string(),
        source_kind: "turn".to_string(),
        source_authority_score: 1,
    };
    let entity = QueryFacetParser::parse(QueryFacetInput::Entity(CanonicalEntityKey {
        kind: CanonicalEntityKind::Person,
        canonical_id: String::new(),
    }));
    let temporal = QueryFacetParser::parse(QueryFacetInput::Temporal(TemporalAnchor {
        anchor_kind: TemporalAnchorKind::ObservedAt,
        epoch_secs: 0,
        precision: TemporalAnchorPrecision::Second,
        evidence_ref: evidence,
    }));

    assert!(!entity.accepted);
    assert_eq!(entity.reason, "entity_query_facet_typed_anchor_invalid");
    assert!(entity.facets.is_empty());
    assert!(!temporal.accepted);
    assert_eq!(temporal.reason, "temporal_query_facet_typed_anchor_invalid");
    assert!(temporal.facets.is_empty());
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

    assert_eq!(groups.len(), 3);
    assert!(groups
        .iter()
        .all(|group| group.starts_with("opaque:recall-group:sha256:")));
    assert_eq!(
        groups
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert!(groups
        .iter()
        .all(|group| { !group.contains("external_eval") && !group.contains("archive:release") }));
}

#[test]
fn canonical_entity_key_is_exact_and_alias_is_provenance_backed_expansion_only() {
    let mut entry = fixture_long_term_entry();
    let evidence = bm_core::memory::canonical_evidence_ref_from_source("archive:release#turn=7")
        .expect("entity evidence");
    let key = CanonicalEntityKey {
        kind: CanonicalEntityKind::Repository,
        canonical_id: "agent-memory".to_string(),
    };
    entry.canonical_entities = vec![CanonicalEntityRef {
        key: key.clone(),
        display_label: Some("Agent Memory".to_string()),
        aliases: vec!["memory repo".to_string()],
        evidence_refs: vec![evidence],
    }];

    let doc = build_long_term_memory_facet_index_doc(
        &entry,
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );
    let query = QueryFacetParser::parse(QueryFacetInput::Entity(key));
    assert!(query.accepted);
    let exact = doc
        .exact_facets
        .iter()
        .find(|facet| facet.namespace == MemoryFacetNamespace::Entity)
        .expect("exact entity facet");
    let alias = doc
        .expanded_facets
        .iter()
        .find(|facet| {
            matches!(
                &facet.value,
                bm_core::memory::MemoryFacetValue::EntityAlias { alias, .. }
                    if alias == "memory repo"
            )
        })
        .expect("expanded entity alias");

    assert!(exact.matches_query_facet(&query.facets[0]));
    assert!(!alias.matches_query_facet(&query.facets[0]));
    assert_eq!(alias.source_evidence_refs.len(), 1);
    assert_eq!(
        alias.expansion_rule_id.as_deref(),
        Some("canonical_entity_alias_v1")
    );
}

#[test]
fn facet_parser_never_constructs_entity_from_strings_or_regex() {
    let entity = StructuredFacetParser::parse_entity_anchor("person:/alice.*/", "turn:1");
    assert!(!entity.accepted);
    assert_eq!(entity.reason, "entity_facet_requires_canonical_entity_ref");

    let plain = StructuredFacetParser::parse_entity_anchor("person:alice", "turn:1");
    assert!(!plain.accepted);
    assert_eq!(plain.reason, "entity_facet_requires_canonical_entity_ref");

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
