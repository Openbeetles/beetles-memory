use bm_core::memory::{
    allocate_recall_delivery_candidates, build_governed_evidence_document_facet_index_doc,
    build_long_term_memory_facet_index_doc as try_build_long_term_memory_facet_index_doc,
    canonical_recall_evidence_group, governed_evidence_document_content_digest,
    governed_long_term_owner_evidence_bindings, governed_memory_recall_candidate_id,
    memory_facet_manifest_key, memory_facet_posting_key, scoped_memory_facet_owner_storage_key,
    score_recall_delivery_texts, validate_memory_facet_manifest, validate_memory_facet_posting,
    validate_memory_facet_read_chain, CanonicalEntityKey, CanonicalEntityKind, CanonicalEntityRef,
    CanonicalEvidenceRef, CanonicalRecallEvidenceFamilyGroup, FacetReportAudience,
    GovernedEvidenceBinding, GovernedEvidenceDocument, GovernedEvidenceDocumentChunk,
    GovernedEvidenceDocumentSourceKind, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    HumanFacetSuggestion, LongTermMemoryConfidence, LongTermMemoryEntry, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemorySourceScope, LongTermMemorySourceType,
    MemoryEvidenceAuthority, MemoryFacetIndexDoc, MemoryFacetIndexManifest, MemoryFacetNamespace,
    MemoryFacetOwnerVersion, MemoryFacetPostingDoc, MemoryFacetPostingRevision, MemoryFacetStatus,
    MemoryFacetValidationError, MemoryPrivacyClass, QueryFacetInput, QueryFacetParser,
    RecallDeliveryCandidate, RecallDeliveryOrderingPolicy, RecallDeliverySelectionDropReason,
    RecallDeliveryText, StructuredFacetParser, TemporalAnchor, TemporalAnchorKind,
    TemporalAnchorPrecision, MEMORY_FACET_SCHEMA_VERSION,
};

fn build_long_term_memory_facet_index_doc(
    entry: &LongTermMemoryEntry,
    memory_space_id: impl Into<String>,
    subject_ids: Vec<String>,
    facet_index_revision: u64,
) -> MemoryFacetIndexDoc {
    try_build_long_term_memory_facet_index_doc(
        entry,
        memory_space_id,
        subject_ids,
        facet_index_revision,
    )
    .expect("fixture long-term owner must produce a valid governed facet document")
}

fn delivery_bindings(sources: &[&str], family: Option<&str>) -> Vec<GovernedEvidenceBinding> {
    let family = family.map(|identity| {
        CanonicalRecallEvidenceFamilyGroup::from_structured_identity(identity)
            .expect("structured family")
            .into_string()
    });
    sources
        .iter()
        .map(|source| {
            GovernedEvidenceBinding::try_new(
                *source,
                canonical_recall_evidence_group(source),
                family.clone(),
            )
            .expect("canonical owner evidence binding")
        })
        .collect()
}

#[test]
fn facet_owner_physical_key_is_scoped_before_any_owner_read() {
    let long_term = GovernedMemoryOwnerRef {
        owner_plane: GovernedMemoryOwnerPlane::LongTerm,
        owner_id: "owner-1".to_string(),
    };
    let evidence = GovernedMemoryOwnerRef {
        owner_plane: GovernedMemoryOwnerPlane::EvidenceDocument,
        owner_id: "owner-1".to_string(),
    };
    let base = scoped_memory_facet_owner_storage_key("space-a", "subject-a", &long_term)
        .expect("scoped facet owner key");
    assert_eq!(
        base,
        scoped_memory_facet_owner_storage_key("space-a", "subject-a", &long_term)
            .expect("deterministic key")
    );
    assert_ne!(
        base,
        scoped_memory_facet_owner_storage_key("space-b", "subject-a", &long_term)
            .expect("space isolation")
    );
    assert_ne!(
        base,
        scoped_memory_facet_owner_storage_key("space-a", "subject-b", &long_term)
            .expect("subject isolation")
    );
    assert_ne!(
        base,
        scoped_memory_facet_owner_storage_key("space-a", "subject-a", &evidence)
            .expect("owner plane isolation")
    );
}

#[test]
fn governed_recall_candidate_id_never_collides_across_planes() {
    let owner_id = "shared-owner-id";
    let owner_planes = [
        GovernedMemoryOwnerPlane::LongTerm,
        GovernedMemoryOwnerPlane::EvidenceDocument,
        GovernedMemoryOwnerPlane::ConversationTranscript,
        GovernedMemoryOwnerPlane::MemoryGraph,
        GovernedMemoryOwnerPlane::RuntimeSkill,
    ];

    let candidate_ids = owner_planes
        .into_iter()
        .map(|owner_plane| {
            let candidate_id = governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
                owner_plane,
                owner_id,
            ));
            assert_ne!(candidate_id, owner_id);
            assert!(candidate_id.starts_with(&format!("owner:{}:", owner_plane.as_str())));
            candidate_id
        })
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(candidate_ids.len(), owner_planes.len());
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

fn fixture_evidence_document() -> GovernedEvidenceDocument {
    let source_locator = "opaque://must-not-become-a-facet/free-tag".to_string();
    let canonical_evidence_group =
        bm_core::memory::canonical_recall_evidence_group("evidence:release:p7.4.1");
    let body = "Typed metadata and bounded lexical indexing close release acceptance.".to_string();
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:acceptance".to_string(),
        ordinal: 0,
        body: "Facet posting manifest verified".to_string(),
    }];
    GovernedEvidenceDocument {
        schema_version: bm_core::memory::GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION,
        physical_key: bm_core::memory::scoped_governed_evidence_document_key(
            "space:main",
            "shared-owner-id",
        )
        .expect("evidence owner key"),
        memory_space_id: "space:main".to_string(),
        mounted_subject_id: "subject:user".to_string(),
        document_id: "shared-owner-id".to_string(),
        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
        source_locator: source_locator.clone(),
        canonical_evidence_group: canonical_evidence_group.clone(),
        evidence_family_group: None,
        source_revision: 7,
        owner_revision: 3,
        content_digest: governed_evidence_document_content_digest(
            &source_locator,
            &canonical_evidence_group,
            None,
            &body,
            &chunks,
        ),
        body,
        chunks,
        authority: MemoryEvidenceAuthority::UserAsserted,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        observed_at: 1_800_000_010,
        created_at: 1_800_000_010,
        updated_at: 1_800_000_030,
    }
}

#[test]
fn delivery_allocator_preserves_distinct_evidence_groups_before_duplicate_rank() {
    let candidates = vec![
        RecallDeliveryCandidate {
            candidate_id: "candidate-shared-high".to_string(),
            evidence_bindings: delivery_bindings(&["evidence:shared"], Some("family:shared")),
            owner_available: true,
            governed_binding_eligible: true,
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
            evidence_bindings: delivery_bindings(&["evidence:shared"], Some("family:shared")),
            owner_available: true,
            governed_binding_eligible: true,
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
            evidence_bindings: delivery_bindings(&["evidence:distinct"], Some("family:distinct")),
            owner_available: true,
            governed_binding_eligible: true,
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
            evidence_bindings: delivery_bindings(&["evidence:private"], Some("family:private")),
            owner_available: true,
            governed_binding_eligible: true,
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
            evidence_bindings: delivery_bindings(&["evidence:stale"], Some("family:stale")),
            owner_available: true,
            governed_binding_eligible: true,
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
            evidence_bindings: delivery_bindings(&["evidence:missing"], Some("family:missing")),
            owner_available: false,
            governed_binding_eligible: true,
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
            evidence_bindings: delivery_bindings(&["evidence:governed"], Some("family:governed")),
            owner_available: true,
            governed_binding_eligible: true,
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
        evidence_bindings: delivery_bindings(&[group], Some(&format!("family:{group}"))),
        owner_available: true,
        governed_binding_eligible: true,
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
        evidence_bindings: delivery_bindings(&["evidence:shared"], Some("family:shared")),
        owner_available: true,
        governed_binding_eligible: true,
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
fn delivery_allocator_preserves_new_groups_from_partial_overlap() {
    let candidate = |id: &str, groups: &[&str], rank: usize| RecallDeliveryCandidate {
        candidate_id: id.to_string(),
        evidence_bindings: delivery_bindings(groups, Some(&format!("family:{id}"))),
        owner_available: true,
        governed_binding_eligible: true,
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

    assert_eq!(
        report.selected_candidate_ids,
        vec!["a-b-first", "b-c-second"]
    );
    let second = report
        .decisions
        .iter()
        .find(|decision| decision.candidate_id == "b-c-second")
        .expect("second candidate decision");
    assert!(second.selected);
    assert_eq!(
        second
            .evidence_bindings
            .iter()
            .map(GovernedEvidenceBinding::canonical_evidence_group)
            .collect::<Vec<_>>(),
        vec![
            canonical_recall_evidence_group("evidence:b"),
            canonical_recall_evidence_group("evidence:c")
        ]
    );
    assert_eq!(
        second
            .renderable_evidence_bindings
            .iter()
            .map(GovernedEvidenceBinding::canonical_evidence_group)
            .collect::<Vec<_>>(),
        vec![canonical_recall_evidence_group("evidence:c")]
    );
    assert_eq!(second.drop_reason, None);
    assert_eq!(
        report.covered_evidence_groups,
        vec![
            canonical_recall_evidence_group("evidence:a"),
            canonical_recall_evidence_group("evidence:b"),
            canonical_recall_evidence_group("evidence:c")
        ]
    );
}

#[test]
fn delivery_allocator_preserves_group_to_family_binding_through_partial_overlap() {
    let candidate = |id: &str,
                     bindings: Vec<GovernedEvidenceBinding>,
                     rank: usize|
     -> RecallDeliveryCandidate {
        RecallDeliveryCandidate {
            candidate_id: id.to_string(),
            evidence_bindings: bindings,
            owner_available: true,
            governed_binding_eligible: true,
            citation_eligible: true,
            privacy_eligible: true,
            temporal_eligible: true,
            source_rank: Some(rank),
            expanded_rank: Some(rank),
            reranked_rank: rank,
            relevance_score: 100_u32.saturating_sub(rank as u32),
            authority_score: 100,
        }
    };
    let first = [
        delivery_bindings(&["evidence:a"], Some("family:a")).remove(0),
        delivery_bindings(&["evidence:b"], Some("family:b")).remove(0),
    ];
    let second = [
        delivery_bindings(&["evidence:b"], Some("family:b")).remove(0),
        delivery_bindings(&["evidence:c"], Some("family:c")).remove(0),
    ];

    let report = allocate_recall_delivery_candidates(
        &[
            candidate("a-b-first", first.into(), 1),
            candidate("b-c-second", second.into(), 2),
        ],
        2,
        RecallDeliveryOrderingPolicy::RelevanceRank,
    );
    let second_decision = report
        .decisions
        .iter()
        .find(|decision| decision.candidate_id == "b-c-second")
        .expect("second candidate decision");
    let rendered_families = second_decision
        .renderable_evidence_bindings
        .iter()
        .map(GovernedEvidenceBinding::effective_evidence_family_group)
        .collect::<Vec<_>>();
    let family_c = delivery_bindings(&["evidence:c"], Some("family:c"))
        .remove(0)
        .effective_evidence_family_group()
        .to_string();

    assert_eq!(second_decision.renderable_evidence_bindings.len(), 1);
    assert_eq!(rendered_families, vec![family_c.as_str()]);
    assert_eq!(report.selected_owner_evidence_family_groups.len(), 3);
    assert_eq!(report.renderable_evidence_family_groups.len(), 3);
}

#[test]
fn delivery_allocator_decides_every_candidate_and_types_invalid_governed_binding() {
    let candidate =
        |id: &str, governed_binding_eligible: bool, rank: usize| RecallDeliveryCandidate {
            candidate_id: id.to_string(),
            evidence_bindings: delivery_bindings(&[&format!("evidence:{id}")], None),
            owner_available: true,
            governed_binding_eligible,
            citation_eligible: true,
            privacy_eligible: true,
            temporal_eligible: true,
            source_rank: Some(rank),
            expanded_rank: Some(rank),
            reranked_rank: rank,
            relevance_score: 100,
            authority_score: 100,
        };
    let candidates = vec![
        candidate("invalid-binding", false, 1),
        candidate("selected", true, 2),
        candidate("budgeted", true, 3),
    ];

    let report = allocate_recall_delivery_candidates(
        &candidates,
        1,
        RecallDeliveryOrderingPolicy::RelevanceRank,
    );

    assert_eq!(report.decisions.len(), candidates.len());
    assert_eq!(report.selected_candidate_ids, vec!["selected"]);
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "invalid-binding"
            && decision.drop_reason
                == Some(RecallDeliverySelectionDropReason::GovernedBindingInvalid)
    }));
    assert!(report.decisions.iter().any(|decision| {
        decision.candidate_id == "budgeted"
            && decision.drop_reason
                == Some(RecallDeliverySelectionDropReason::ProfileBudgetExhausted)
    }));
    assert_eq!(
        RecallDeliverySelectionDropReason::GovernedBindingInvalid.label(),
        "governed_binding_invalid"
    );
}

#[test]
fn evidence_family_rotation_round_robins_only_within_equal_utility() {
    let candidate = |id: &str, group: &str, family: &str, rank: usize| RecallDeliveryCandidate {
        candidate_id: id.to_string(),
        evidence_bindings: delivery_bindings(&[group], Some(family)),
        owner_available: true,
        governed_binding_eligible: true,
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
        evidence_bindings: if citation_eligible {
            delivery_bindings(&[&format!("evidence:{id}")], None)
        } else {
            Vec::new()
        },
        owner_available: true,
        governed_binding_eligible: true,
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

    assert_eq!(
        doc.owner_ref,
        GovernedMemoryOwnerRef {
            owner_plane: GovernedMemoryOwnerPlane::LongTerm,
            owner_id: "ltm:project:agent-memory-w4".to_string(),
        }
    );
    assert_eq!(doc.schema_version, MEMORY_FACET_SCHEMA_VERSION);
    assert_eq!(MEMORY_FACET_SCHEMA_VERSION, 4);
    assert_eq!(doc.facet_index_revision, 3);
    assert_eq!(doc.status, MemoryFacetStatus::Active);
    assert_eq!(doc.memory_space_id, "space:main");
    assert_eq!(doc.subject_ids, vec!["subject:user"]);
    assert_eq!(doc.owner_revision, 5);
    let serialized = serde_json::to_value(&doc).expect("serialize typed facet owner");
    assert!(serialized.get("owner_ref").is_some());
    assert!(serialized.get("owner_record_id").is_none());
    assert!(serialized.get("owner_plane").is_none());

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
fn evidence_document_facets_use_typed_metadata_and_bounded_lexical_input_only() {
    let evidence_owner = fixture_evidence_document();
    let material = evidence_owner
        .owner_material()
        .expect("governed evidence owner material");
    let doc = build_governed_evidence_document_facet_index_doc(
        &evidence_owner,
        vec!["subject:user".to_string()],
        4,
    )
    .expect("valid evidence facet owner");

    assert_eq!(
        doc.owner_ref,
        GovernedMemoryOwnerRef {
            owner_plane: GovernedMemoryOwnerPlane::EvidenceDocument,
            owner_id: "shared-owner-id".to_string(),
        }
    );
    assert!(doc
        .exact_facets
        .iter()
        .any(|facet| facet.namespace == MemoryFacetNamespace::SourceType));
    assert!(doc
        .exact_facets
        .iter()
        .any(|facet| facet.namespace == MemoryFacetNamespace::Evidence));
    assert!(doc
        .exact_facets
        .iter()
        .any(|facet| facet.namespace == MemoryFacetNamespace::Temporal));
    assert!(doc
        .exact_facets
        .iter()
        .any(|facet| facet.namespace == MemoryFacetNamespace::Keyword));
    assert_eq!(doc.canonical_evidence_refs.len(), 1);
    let binding = &material.evidence_bindings()[0];
    assert_eq!(
        doc.canonical_evidence_refs[0].source_ref,
        binding.safe_evidence_ref()
    );
    assert_eq!(
        doc.canonical_evidence_refs[0].canonical_evidence_group,
        binding.canonical_evidence_group()
    );
    assert!(
        doc.exact_facets.iter().any(|facet| {
            matches!(
                &facet.value,
                bm_core::memory::MemoryFacetValue::Keyword { normalized }
                    if normalized == "verified"
            )
        }),
        "chunk-only governed lexical content must reach facet indexing"
    );

    let serialized = serde_json::to_string(&doc).expect("serialize evidence facet doc");
    assert!(!serialized.contains("must-not-become-a-facet"));
    assert!(!serialized.contains("free-tag"));
    assert!(
        doc.exact_facets.len() <= 68,
        "lexical facet input must be bounded"
    );
}

#[test]
fn same_owner_id_in_different_planes_has_distinct_facet_ids() {
    let mut long_term = fixture_long_term_entry();
    long_term.id = "shared-owner-id".to_string();
    let long_term_doc = build_long_term_memory_facet_index_doc(
        &long_term,
        "space:main",
        vec!["subject:user".to_string()],
        4,
    );
    let evidence_doc = build_governed_evidence_document_facet_index_doc(
        &fixture_evidence_document(),
        vec!["subject:user".to_string()],
        4,
    )
    .expect("valid evidence facet owner");

    let long_term_ids = long_term_doc
        .exact_facets
        .iter()
        .map(|facet| &facet.facet_id)
        .collect::<std::collections::BTreeSet<_>>();
    let evidence_ids = evidence_doc
        .exact_facets
        .iter()
        .map(|facet| &facet.facet_id)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(long_term_ids.is_disjoint(&evidence_ids));
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
        owner_ref: owner.owner_ref.clone(),
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
        evidence_family_group: None,
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
fn facet_evidence_identity_tamper_cannot_replace_long_term_owner_truth() {
    let mut entry = fixture_long_term_entry();
    let owner_evidence =
        bm_core::memory::canonical_evidence_ref_from_source("archive:release#turn=7")
            .expect("owner evidence");
    entry.canonical_entities = vec![CanonicalEntityRef {
        key: CanonicalEntityKey {
            kind: CanonicalEntityKind::Repository,
            canonical_id: "agent-memory".to_string(),
        },
        display_label: None,
        aliases: Vec::new(),
        evidence_refs: vec![owner_evidence.clone()],
    }];
    let owner_bindings = governed_long_term_owner_evidence_bindings(&entry)
        .expect("owner closes canonical evidence binding");
    let mut facet = build_long_term_memory_facet_index_doc(
        &entry,
        "space:main",
        vec!["subject:user".to_string()],
        3,
    );
    facet.canonical_evidence_refs[0].source_ref = "forged:facet:identity".to_string();
    facet.canonical_evidence_refs[0].canonical_evidence_group =
        canonical_recall_evidence_group("forged:facet:identity");

    let owner_binding = owner_bindings
        .iter()
        .find(|binding| binding.safe_evidence_ref() == owner_evidence.source_ref)
        .expect("owner evidence binding remains authoritative");
    assert_eq!(owner_binding.safe_evidence_ref(), owner_evidence.source_ref);
    assert_eq!(
        owner_binding.canonical_evidence_group(),
        owner_evidence.canonical_evidence_group
    );
    assert_ne!(
        owner_binding.canonical_evidence_group(),
        facet.canonical_evidence_refs[0].canonical_evidence_group
    );
}

#[test]
fn long_term_owner_evidence_family_binding_is_order_invariant() {
    let mut entry = fixture_long_term_entry();
    entry.supporting_citations.clear();
    let base = bm_core::memory::canonical_evidence_ref_from_source("archive:family#turn=7")
        .expect("canonical evidence");
    let family = CanonicalRecallEvidenceFamilyGroup::from_structured_identity("session:family")
        .expect("canonical family")
        .into_string();
    let mut explicit = base.clone();
    explicit.evidence_family_group = Some(family.clone());
    let implicit = base.clone();

    let entity = |evidence_refs| CanonicalEntityRef {
        key: CanonicalEntityKey {
            kind: CanonicalEntityKind::Repository,
            canonical_id: "agent-memory".to_string(),
        },
        display_label: None,
        aliases: Vec::new(),
        evidence_refs,
    };
    entry.canonical_entities = vec![entity(vec![implicit.clone(), explicit.clone()])];
    let forward = governed_long_term_owner_evidence_bindings(&entry)
        .expect("forward evidence family binding");
    entry.canonical_entities = vec![entity(vec![explicit, implicit])];
    let reverse = governed_long_term_owner_evidence_bindings(&entry)
        .expect("reverse evidence family binding");

    assert_eq!(forward, reverse);
    assert_eq!(forward.len(), 1);
    assert_eq!(forward[0].evidence_family_group(), Some(family.as_str()));
}

#[test]
fn long_term_owner_rejects_conflicting_explicit_evidence_families() {
    let mut entry = fixture_long_term_entry();
    entry.supporting_citations.clear();
    let base = bm_core::memory::canonical_evidence_ref_from_source("archive:family#turn=7")
        .expect("canonical evidence");
    let mut first = base.clone();
    first.evidence_family_group = Some(
        CanonicalRecallEvidenceFamilyGroup::from_structured_identity("session:first")
            .expect("first family")
            .into_string(),
    );
    let mut second = base;
    second.evidence_family_group = Some(
        CanonicalRecallEvidenceFamilyGroup::from_structured_identity("session:second")
            .expect("second family")
            .into_string(),
    );
    entry.canonical_entities = vec![CanonicalEntityRef {
        key: CanonicalEntityKey {
            kind: CanonicalEntityKind::Repository,
            canonical_id: "agent-memory".to_string(),
        },
        display_label: None,
        aliases: Vec::new(),
        evidence_refs: vec![first, second],
    }];

    let error = governed_long_term_owner_evidence_bindings(&entry)
        .expect_err("conflicting family authorities must fail closed");
    assert!(error
        .to_string()
        .contains("one canonical evidence group maps to multiple evidence families"));
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
    assert_eq!(report.owner_ref, None);
    assert!(!report.owner_token.contains(&doc.owner_ref.owner_id));
    assert_eq!(report.visible_canonical_evidence_groups.len(), 0);
    assert_eq!(
        report.redacted_canonical_evidence_group_count,
        doc.canonical_evidence_refs.len()
    );

    let owner_report = doc.report_view(FacetReportAudience::OwnerRaw);
    assert!(!owner_report.redacted_sensitive_metadata);
    assert_eq!(owner_report.owner_ref.as_ref(), Some(&doc.owner_ref));
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
        owner_ref: GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::LongTerm,
            "ltm:project:agent-memory-w4",
        ),
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
