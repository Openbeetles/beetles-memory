use bm_core::memory::{
    commit_canonical_turn_delta, commit_canonical_turn_delta_with_transcript,
    transcript_cursor_governance_context_digest, transcript_message_is_query_index_eligible,
    ActorAttribution, CanonicalTurnDelta, ConversationCatalogHead, ConversationKey,
    ConversationTranscriptStore, DerivedMemoryPlane, DerivedMemoryRef, HostOpaqueRef,
    HostRefRelation, HostRefVisibility, MemoryEvidenceAuthority, MemoryTurnDeliveryStatus,
    MemoryTurnProtocol, MemoryTurnSource, RedactedTranscriptSlice, SessionMessage, SessionStore,
    ToolObservationDigest, TranscriptActivityBucket, TranscriptAnchor, TranscriptAppendIntent,
    TranscriptAttrEnvelope, TranscriptAttrGovernance, TranscriptAttrLink,
    TranscriptAttrRedactionPolicy, TranscriptAttrScope, TranscriptAttrSource,
    TranscriptAttrSourceKind, TranscriptAttrTarget, TranscriptAttrValueKind,
    TranscriptCommitReport, TranscriptConversationAlias, TranscriptCursorDisclosurePolicyV1,
    TranscriptCursorOperationKind, TranscriptEvidenceRef, TranscriptInputMessage,
    TranscriptLifecycleReport, TranscriptLifecycleRequest, TranscriptLifecycleState,
    TranscriptQueryCursor, TranscriptRedactionReason, TranscriptRedactionState,
    TranscriptRepairIssueKind, TranscriptReplayView, TranscriptSearchCandidate,
    TranscriptSearchCandidatePage, TranscriptSearchNormalizerV1, TranscriptSearchQuery,
    TranscriptSearchScope, TranscriptSearchSort, TranscriptTurnRecord, TranscriptUtcRange,
};
use bm_core::Result;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_core::memory::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3".to_string()),
        request_id: Some("req-1".to_string()),
        client_conversation_hint: Some("window-a".to_string()),
    }
}

fn delivered_delta(turn_id: &str) -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: turn_id.to_string(),
        conversation: bm_core::memory::ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "legacy-chat-a".to_string(),
            conversation_id: Some("conversation-a".to_string()),
        },
        subject: "subject-qingchuan".to_string(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: turn_source(),
        actor: None,
        input_messages: vec![TranscriptInputMessage::user("请把这个事项挂到宿主任务上")
            .with_observed_at(1_800_000_001)
            .with_speaker("owner-human", "human")],
        assistant_message: Some(
            TranscriptInputMessage::assistant("已记录证据，但不会解释宿主任务状态。")
                .with_observed_at(1_800_000_002)
                .with_speaker("assistant-main", "llm_agent"),
        ),
        tool_observations: vec![ToolObservationDigest {
            observation_id: "tool-1".to_string(),
            tool_name: "host.lookup".to_string(),
            summary: "host returned opaque ref".to_string(),
            external_content: true,
        }],
        external_content_used: true,
        candidate_ids: vec!["candidate-1".to_string()],
    }
}

fn host_ref(id: &str, visibility: HostRefVisibility) -> HostOpaqueRef {
    HostOpaqueRef {
        host_kind: "generic-host".to_string(),
        business_ref_type: "ticket".to_string(),
        business_ref_id: id.to_string(),
        relation: HostRefRelation::Related,
        visibility,
        label: Some(format!("opaque {id}")),
    }
}

fn governance_context_digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}

#[test]
fn transcript_query_index_eligibility_is_canonical_and_excludes_private_authorities() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut record =
        TranscriptTurnRecord::from_delta(&key, 1, &delivered_delta("turn-index"), Vec::new(), 10)
            .unwrap();
    assert!(record
        .input_messages
        .iter()
        .chain(record.assistant_message.iter())
        .all(transcript_message_is_query_index_eligible));
    for authority in [
        MemoryEvidenceAuthority::PrivateGardenInternal,
        MemoryEvidenceAuthority::SoulGovernance,
        MemoryEvidenceAuthority::OperatorDiagnostic,
    ] {
        record.input_messages[0].authority = authority;
        assert!(!transcript_message_is_query_index_eligible(
            &record.input_messages[0]
        ));
    }
    record.input_messages[0].authority = MemoryEvidenceAuthority::UserAsserted;
    record.input_messages[0].role = "tool".to_string();
    assert!(!transcript_message_is_query_index_eligible(
        &record.input_messages[0]
    ));
}

fn message_usage_attr(
    key: &ConversationKey,
    turn_id: &str,
    message_id: &str,
    visibility: HostRefVisibility,
) -> TranscriptAttrEnvelope {
    TranscriptAttrEnvelope {
        attr_id: format!("attr-{turn_id}-{message_id}"),
        target: TranscriptAttrTarget {
            key: key.clone(),
            scope: TranscriptAttrScope::Message,
            turn_id: turn_id.to_string(),
            message_id: Some(message_id.to_string()),
        },
        key: "host.beetle_agent.model_usage".to_string(),
        value_kind: TranscriptAttrValueKind::JsonObject,
        schema_ref: Some("beetle-agent.model-usage.v1".to_string()),
        value: json!({
            "status": "measured",
            "input_tokens": 12,
            "output_tokens": 5,
            "usage_source": "provider_reported"
        }),
        visibility,
        source: TranscriptAttrSource {
            writer: "beetle-agent".to_string(),
            source_kind: TranscriptAttrSourceKind::ProviderReported,
            written_at: 1_800_000_010,
            audit_reason: "model invocation completed".to_string(),
        },
        governance: TranscriptAttrGovernance {
            max_value_bytes: 4096,
            redaction_policy: TranscriptAttrRedactionPolicy::MetadataSurvivesMask,
            export_allowed: false,
        },
        links: vec![TranscriptAttrLink {
            relation: "model_invocation".to_string(),
            ref_kind: "model_invocation_id".to_string(),
            ref_id: "model-invocation-1".to_string(),
        }],
        created_at: 1_800_000_010,
        updated_at: 1_800_000_010,
    }
}

#[test]
fn conversation_key_separates_memory_space_channel_and_conversation() {
    let left = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let right = ConversationKey::new("space-b", "llm.gateway", "conversation-a").unwrap();

    assert_ne!(left, right);
    assert_ne!(left.storage_key(), right.storage_key());
    assert!(left.storage_key().contains("space-a"));
    assert!(left.storage_key().contains("conversation-a"));
}

#[test]
fn conversation_key_storage_key_is_not_ambiguous_when_components_contain_separators() {
    let left = ConversationKey::new("space__a", "channel", "conversation").unwrap();
    let right = ConversationKey::new("space", "a__channel", "conversation").unwrap();

    assert_ne!(left.storage_key(), right.storage_key());
}

#[test]
fn transcript_query_contract_is_scope_bound_and_cursor_stays_opaque() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let locator = bm_core::memory::TranscriptLocator::new(
        key.clone(),
        "subject-qingchuan",
        "turn-9",
        Some("message-9".to_string()),
        9,
        1_700_000_009,
    )
    .unwrap();
    let anchor = TranscriptAnchor::new(locator.clone(), 17, "sha256:catalog-head").unwrap();
    assert_eq!(anchor.locator, locator);

    let head = ConversationCatalogHead {
        key,
        mounted_subject_id: "subject-qingchuan".to_string(),
        revision: 17,
        head_digest: "sha256:catalog-head".to_string(),
        turn_count: 9,
        message_count: 18,
        lifecycle: bm_core::memory::TranscriptLifecycleAggregate {
            active: bm_core::memory::TranscriptLifecycleStats {
                turn_count: 9,
                message_count: 18,
                first_observed_at: Some(1_700_000_001),
                last_observed_at: Some(1_700_000_009),
            },
            archived: bm_core::memory::TranscriptLifecycleStats::default(),
            masked: bm_core::memory::TranscriptLifecycleStats::default(),
            raw_deleted: bm_core::memory::TranscriptLifecycleStats::default(),
        },
        first_sequence: Some(1),
        last_sequence: Some(9),
        content_generation: 17,
        index_generation: 17,
        updated_at: 1_700_000_010,
    };
    head.validate().unwrap();

    let cursor = TranscriptQueryCursor::try_from_encoded("btq1:opaque-token").unwrap();
    assert_eq!(cursor.as_str(), "btq1:opaque-token");
    assert_eq!(format!("{cursor:?}"), "TranscriptQueryCursor([REDACTED])");
    assert!(TranscriptQueryCursor::try_from_encoded("opaque-token").is_err());
    assert!(TranscriptQueryCursor::try_from_encoded(format!("btq1:{}", "x".repeat(4096))).is_err());
}

#[test]
fn catalog_lifecycle_stats_preserve_message_counts_and_time_bounds_per_state() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut head = ConversationCatalogHead {
        key,
        mounted_subject_id: "subject-qingchuan".to_string(),
        revision: 3,
        head_digest: "sha256:mixed-head".to_string(),
        turn_count: 3,
        message_count: 5,
        lifecycle: bm_core::memory::TranscriptLifecycleAggregate {
            active: bm_core::memory::TranscriptLifecycleStats {
                turn_count: 1,
                message_count: 2,
                first_observed_at: Some(100),
                last_observed_at: Some(101),
            },
            archived: bm_core::memory::TranscriptLifecycleStats {
                turn_count: 1,
                message_count: 2,
                first_observed_at: Some(80),
                last_observed_at: Some(81),
            },
            masked: bm_core::memory::TranscriptLifecycleStats {
                turn_count: 1,
                message_count: 1,
                first_observed_at: Some(60),
                last_observed_at: Some(60),
            },
            raw_deleted: bm_core::memory::TranscriptLifecycleStats::default(),
        },
        first_sequence: Some(1),
        last_sequence: Some(3),
        content_generation: 3,
        index_generation: 3,
        updated_at: 110,
    };
    head.validate().unwrap();

    head.lifecycle.masked.message_count = 2;
    assert!(head.validate().is_err());
}

#[test]
fn catalog_and_mounted_subject_search_are_exact_memory_space_scoped() {
    let catalog = bm_core::memory::TranscriptCatalogQuery {
        memory_space_id: "space-a".to_string(),
        governance_context_digest: governance_context_digest(),
        channel_id: None,
        lifecycle: bm_core::memory::TranscriptCatalogLifecycle::ActiveOnly,
        limit: 8,
        cursor: None,
    };
    catalog.validate().unwrap();

    let search = TranscriptSearchQuery {
        scope: TranscriptSearchScope::MountedSubject {
            memory_space_id: "space-a".to_string(),
            channel_id: None,
        },
        governance_context_digest: governance_context_digest(),
        query: TranscriptSearchNormalizerV1::normalize("青川").unwrap(),
        sort: TranscriptSearchSort::ObservedAtDescending,
        lifecycle: bm_core::memory::TranscriptSearchLifecycle::ActiveOnly,
        limit: 8,
        cursor: None,
    };
    search.validate().unwrap();

    let mut cross_space_delta = delivered_delta("turn-cross-space");
    cross_space_delta.conversation.conversation_id = Some("conversation-b".to_string());
    let record = TranscriptTurnRecord::from_delta(
        &ConversationKey::new("space-b", "llm.gateway", "conversation-b").unwrap(),
        1,
        &cross_space_delta,
        Vec::new(),
        1_800_000_010,
    )
    .unwrap();
    let page = TranscriptSearchCandidatePage {
        candidates: vec![TranscriptSearchCandidate {
            message_id: record.input_messages[0].message_id.clone(),
            record,
            score: 10,
            head_revision: 1,
            head_digest: "sha256:space-b-head".to_string(),
        }],
        next_cursor: None,
        has_more: false,
        budget_applied: false,
    };
    assert!(page
        .validate_for_query("subject-qingchuan", &search)
        .is_err());
}

#[test]
fn cross_conversation_search_candidates_carry_each_head_identity() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-search-head"),
        Vec::new(),
        1_800_000_010,
    )
    .unwrap();
    let candidate = TranscriptSearchCandidate {
        message_id: record.input_messages[0].message_id.clone(),
        record,
        score: 10,
        head_revision: 7,
        head_digest: "sha256:conversation-a-head".to_string(),
    };
    candidate.validate().unwrap();

    let mut invalid = candidate.clone();
    invalid.head_revision = 0;
    assert!(invalid.validate().is_err());
}

#[test]
fn transcript_search_normalizer_is_canonical_and_highlight_is_unicode_safe() {
    let normalized = TranscriptSearchNormalizerV1::normalize("  青川，HELLO！青川  ").unwrap();
    assert_eq!(normalized.normalized, "青川 hello 青川");
    assert!(normalized.terms.iter().any(|term| term == "青川"));
    assert!(normalized.terms.iter().any(|term| term == "hello"));

    let excerpt = TranscriptSearchNormalizerV1::excerpt(
        "开头🙂这里记录了青川，HELLO！最后一段",
        &normalized,
        18,
    )
    .unwrap();
    assert!(!excerpt.text.is_empty());
    assert!(!excerpt.highlights.is_empty());
    for highlight in &excerpt.highlights {
        assert!(highlight.start_char < highlight.end_char);
        assert!(highlight.end_char <= excerpt.text.chars().count());
    }

    assert!(TranscriptSearchNormalizerV1::normalize(" \n，！ ").is_err());
    assert!(TranscriptSearchNormalizerV1::normalize(&"x".repeat(1025)).is_err());
}

#[test]
fn transcript_document_indexing_is_not_limited_or_silently_truncated_like_a_query() {
    let long_document = (0..80)
        .map(|index| format!("document_term_{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(long_document.len() > bm_core::memory::MAX_TRANSCRIPT_SEARCH_QUERY_BYTES);
    assert!(TranscriptSearchNormalizerV1::normalize(&long_document).is_err());

    let terms = TranscriptSearchNormalizerV1::index_terms(&long_document, 128).unwrap();
    assert!(terms.len() > bm_core::memory::MAX_TRANSCRIPT_SEARCH_TERMS);
    assert!(TranscriptSearchNormalizerV1::index_terms(&long_document, 24).is_err());

    let control_normalized = TranscriptSearchNormalizerV1::index_terms("hello\0world", 8).unwrap();
    assert!(control_normalized.iter().any(|term| term == "hello"));
    assert!(control_normalized.iter().any(|term| term == "world"));
}

#[test]
fn transcript_document_indexing_allows_valid_content_without_searchable_terms() {
    for content in ["好", "🙂", "……！？"] {
        let terms = TranscriptSearchNormalizerV1::index_terms(content, 8).unwrap();
        assert!(
            terms.is_empty(),
            "{content} must remain valid but unindexed"
        );

        let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
        let mut delta = delivered_delta("turn-unindexable-content");
        delta.input_messages = vec![TranscriptInputMessage::user(content)];
        delta.assistant_message = None;
        let record = TranscriptTurnRecord::from_delta(&key, 1, &delta, Vec::new(), 10).unwrap();
        TranscriptAppendIntent {
            record,
            conversation_alias: None,
        }
        .validate()
        .unwrap();
    }

    assert!(TranscriptSearchNormalizerV1::normalize("好").is_err());
    assert!(TranscriptSearchNormalizerV1::normalize("🙂").is_err());
    assert!(TranscriptSearchNormalizerV1::normalize("……！？").is_err());
}

#[test]
fn transcript_utc_ranges_are_half_open_and_activity_buckets_validate_canonically() {
    let first = TranscriptUtcRange::new(1_700_000_000, 1_700_082_800).unwrap();
    let second = TranscriptUtcRange::new(1_700_082_800, 1_700_172_800).unwrap();
    assert!(first.contains(1_700_000_000));
    assert!(!first.contains(1_700_082_800));
    assert!(TranscriptUtcRange::new(10, 10).is_err());
    assert!(TranscriptUtcRange::validate_sorted_non_overlapping(&[first, second]).is_ok());
    assert!(TranscriptUtcRange::validate_sorted_non_overlapping(&[second, first]).is_err());

    let bucket = TranscriptActivityBucket {
        range: first,
        visible_message_count: 0,
        first_visible_anchor: None,
        last_visible_anchor: None,
    };
    bucket.validate().unwrap();

    bm_core::memory::TranscriptActivityQuery {
        key: ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap(),
        ranges: vec![first],
        lifecycle: bm_core::memory::TranscriptSearchLifecycle::ActiveOnly,
    }
    .validate()
    .unwrap();

    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    bm_core::memory::TranscriptTimelineQuery {
        key: key.clone(),
        governance_context_digest: governance_context_digest(),
        anchor: bm_core::memory::TranscriptTimelineAnchor::AroundSequence(9),
        limit: 8,
        cursor: None,
    }
    .validate()
    .unwrap();
    let _ = key;
}

#[test]
fn cursor_queries_require_exact_opaque_governance_context_digest() {
    let mut catalog = bm_core::memory::TranscriptCatalogQuery {
        memory_space_id: "space-a".to_string(),
        governance_context_digest: governance_context_digest(),
        channel_id: None,
        lifecycle: bm_core::memory::TranscriptCatalogLifecycle::ActiveOnly,
        limit: 8,
        cursor: None,
    };
    catalog.validate().unwrap();
    catalog.governance_context_digest = "sha256:not-64-hex".to_string();
    assert!(catalog.validate().is_err());

    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut timeline = bm_core::memory::TranscriptTimelineQuery {
        key: key.clone(),
        governance_context_digest: governance_context_digest(),
        anchor: bm_core::memory::TranscriptTimelineAnchor::Latest,
        limit: 8,
        cursor: None,
    };
    timeline.validate().unwrap();
    timeline.governance_context_digest = format!("sha256:{}", "g".repeat(64));
    assert!(timeline.validate().is_err());

    let mut search = TranscriptSearchQuery {
        scope: TranscriptSearchScope::ExactConversation { key },
        governance_context_digest: governance_context_digest(),
        query: TranscriptSearchNormalizerV1::normalize("青川").unwrap(),
        sort: TranscriptSearchSort::ObservedAtDescending,
        lifecycle: bm_core::memory::TranscriptSearchLifecycle::ActiveOnly,
        limit: 8,
        cursor: None,
    };
    search.validate().unwrap();
    search.governance_context_digest = format!("sha256:{}", "a".repeat(63));
    assert!(search.validate().is_err());
}

#[test]
fn cursor_governance_context_digest_is_deterministic_and_binds_kind_view_and_capability() {
    let policy =
        TranscriptCursorDisclosurePolicyV1::new(1, format!("sha256:{}", "b".repeat(64))).unwrap();
    let same_left = transcript_cursor_governance_context_digest(
        TranscriptCursorOperationKind::Catalog,
        TranscriptReplayView::HostUi,
        &policy,
    )
    .unwrap();
    let same_right = transcript_cursor_governance_context_digest(
        TranscriptCursorOperationKind::Catalog,
        TranscriptReplayView::HostUi,
        &policy,
    )
    .unwrap();
    assert_eq!(same_left, same_right);
    assert_eq!(same_left.len(), "sha256:".len() + 64);
    assert!(same_left
        .strip_prefix("sha256:")
        .unwrap()
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert_ne!(
        same_left,
        transcript_cursor_governance_context_digest(
            TranscriptCursorOperationKind::Search,
            TranscriptReplayView::HostUi,
            &policy,
        )
        .unwrap()
    );
    assert_ne!(
        same_left,
        transcript_cursor_governance_context_digest(
            TranscriptCursorOperationKind::Catalog,
            TranscriptReplayView::ModelContext,
            &policy,
        )
        .unwrap()
    );

    let other_capability =
        TranscriptCursorDisclosurePolicyV1::new(1, format!("sha256:{}", "c".repeat(64))).unwrap();
    assert_ne!(
        same_left,
        transcript_cursor_governance_context_digest(
            TranscriptCursorOperationKind::Catalog,
            TranscriptReplayView::HostUi,
            &other_capability,
        )
        .unwrap()
    );
}

#[test]
fn transcript_search_and_activity_exclude_masked_or_raw_deleted_records() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut record =
        TranscriptTurnRecord::from_delta(&key, 1, &delivered_delta("turn-1"), Vec::new(), 10)
            .unwrap();
    assert!(record.is_searchable_for_presentation());
    assert!(record.contributes_to_presentation_activity());

    record.apply_lifecycle_transition(bm_core::memory::TranscriptLifecycleTransition::Archive, 11);
    assert!(record.is_searchable_for_presentation());
    assert!(record.contributes_to_presentation_activity());

    record.apply_lifecycle_transition(bm_core::memory::TranscriptLifecycleTransition::Mask, 12);
    assert!(!record.is_searchable_for_presentation());
    assert!(!record.contributes_to_presentation_activity());

    let mut deleted =
        TranscriptTurnRecord::from_delta(&key, 2, &delivered_delta("turn-2"), Vec::new(), 10)
            .unwrap();
    deleted.apply_lifecycle_transition(
        bm_core::memory::TranscriptLifecycleTransition::DeleteRaw,
        13,
    );
    assert!(!deleted.is_searchable_for_presentation());
    assert!(!deleted.contributes_to_presentation_activity());
}

#[test]
fn transcript_turn_record_keeps_actor_attribution_and_host_refs_opaque() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let host_ref = HostOpaqueRef {
        host_kind: "beetle-agent".to_string(),
        business_ref_type: "task".to_string(),
        business_ref_id: "task-42".to_string(),
        relation: HostRefRelation::Related,
        visibility: HostRefVisibility::HostUi,
        label: Some("宿主任务引用".to_string()),
    };

    let record =
        TranscriptTurnRecord::from_delta(&key, 7, &delivered_delta("turn-7"), vec![host_ref], 10)
            .unwrap();

    assert_eq!(record.key, key);
    assert_eq!(record.turn_id, "turn-7");
    assert_eq!(record.sequence, 7);
    assert_eq!(record.subject, "subject-qingchuan");
    assert_eq!(
        record.actor.subject_id.as_deref(),
        Some("subject-qingchuan")
    );
    assert_eq!(record.input_messages[0].actor.speaker_id, "owner-human");
    assert_eq!(record.input_messages[0].actor.speaker_kind, "human");
    assert_eq!(
        record.assistant_message.as_ref().unwrap().actor.speaker_id,
        "assistant-main"
    );
    assert_eq!(record.host_refs.len(), 1);
    assert_eq!(record.host_refs[0].business_ref_type, "task");
    assert_eq!(record.host_refs[0].business_ref_id, "task-42");
    assert_eq!(record.lifecycle_state, TranscriptLifecycleState::Active);
    assert_eq!(
        record.redaction_state,
        TranscriptRedactionState::RawAvailable
    );
}

#[test]
fn transcript_turn_record_uses_host_provided_actor_attribution() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut delta = delivered_delta("turn-host-actor");
    delta.actor = Some(ActorAttribution {
        speaker_id: "runtime-dispatcher".to_string(),
        speaker_kind: "runtime".to_string(),
        subject_id: Some("subject-qingchuan".to_string()),
        actor_subject_id: Some("subject-human-owner".to_string()),
        mounted_subject_id: Some("subject-agent-mounted".to_string()),
        agent_id: Some("agent-alpha".to_string()),
        triggered_by: Some("host:event:abc".to_string()),
    });

    let record = TranscriptTurnRecord::from_delta(&key, 2, &delta, Vec::new(), 10).unwrap();

    assert_eq!(record.actor.speaker_id, "runtime-dispatcher");
    assert_eq!(
        record.actor.actor_subject_id.as_deref(),
        Some("subject-human-owner")
    );
    assert_eq!(
        record.actor.mounted_subject_id.as_deref(),
        Some("subject-agent-mounted")
    );
    assert_eq!(record.actor.agent_id.as_deref(), Some("agent-alpha"));
    assert_eq!(record.actor.triggered_by.as_deref(), Some("host:event:abc"));
    assert_eq!(
        record.input_messages[0].actor.actor_subject_id.as_deref(),
        Some("subject-human-owner")
    );
    assert_eq!(
        record.input_messages[0].actor.mounted_subject_id.as_deref(),
        Some("subject-agent-mounted")
    );
    assert_eq!(
        record
            .assistant_message
            .as_ref()
            .unwrap()
            .actor
            .agent_id
            .as_deref(),
        Some("agent-alpha")
    );
}

#[test]
fn transcript_turn_record_serialization_roundtrips_contract_fields() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record =
        TranscriptTurnRecord::from_delta(&key, 3, &delivered_delta("turn-json"), Vec::new(), 10)
            .unwrap();

    let json = serde_json::to_string(&record).unwrap();
    let decoded: TranscriptTurnRecord = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, record);
    assert_eq!(decoded.key.memory_space_id, "space-a");
    assert_eq!(decoded.turn_id, "turn-json");
    assert_eq!(
        decoded.input_messages[0].authority,
        MemoryEvidenceAuthority::UserAsserted
    );
}

#[test]
fn transcript_turn_record_from_delta_does_not_record_non_delivered_assistant_message() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut delta = delivered_delta("turn-incomplete");
    delta.delivery_status = MemoryTurnDeliveryStatus::IncompleteStream;

    let record = TranscriptTurnRecord::from_delta(&key, 3, &delta, Vec::new(), 10).unwrap();

    assert_eq!(
        record.delivery_status,
        MemoryTurnDeliveryStatus::IncompleteStream
    );
    assert_eq!(record.input_messages.len(), 1);
    assert!(record.assistant_message.is_none());
}

#[test]
fn redacted_replay_keeps_attribution_and_refs_without_raw_deleted_content() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut record =
        TranscriptTurnRecord::from_delta(&key, 1, &delivered_delta("turn-1"), Vec::new(), 10)
            .unwrap();
    record.redaction_state = TranscriptRedactionState::RawDeleted;
    record.lifecycle_state = TranscriptLifecycleState::RawDeleted;
    record.actor = ActorAttribution::for_subject("subject-qingchuan");

    let replay = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::HostUi,
        &[record.clone()],
    );

    assert_eq!(replay.key, key);
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(replay.turns[0].turn_id, "turn-1");
    assert_eq!(replay.turns[0].input_messages[0].content, None);
    assert!(replay.turns[0].input_messages[0].redacted);
    assert_eq!(
        replay.turns[0].input_messages[0]
            .actor
            .subject_id
            .as_deref(),
        Some("subject-qingchuan")
    );
    assert_eq!(replay.audit.redacted_messages, 2);
}

#[test]
fn non_owner_replay_views_redact_private_internal_authority() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut delta = delivered_delta("turn-private");
    delta.input_messages = vec![TranscriptInputMessage::new(
        "assistant",
        "private scratch",
        MemoryEvidenceAuthority::PrivateGardenInternal,
    )];
    delta.assistant_message = None;
    let record = TranscriptTurnRecord::from_delta(&key, 1, &delta, Vec::new(), 10).unwrap();

    let replay =
        RedactedTranscriptSlice::from_records(key, TranscriptReplayView::HostUi, &[record]);

    assert_eq!(replay.turns[0].input_messages[0].content, None);
    assert!(replay.turns[0].input_messages[0].redacted);
    assert_eq!(replay.audit.redacted_messages, 1);
}

#[test]
fn host_ref_visibility_is_enforced_and_reported_per_replay_view() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-visibility"),
        vec![
            host_ref("internal", HostRefVisibility::Internal),
            host_ref("host", HostRefVisibility::HostUi),
            host_ref("model", HostRefVisibility::ModelContext),
            host_ref("operator", HostRefVisibility::OperatorAudit),
            host_ref("export", HostRefVisibility::Export),
        ],
        10,
    )
    .unwrap();

    let host_ui = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::HostUi,
        std::slice::from_ref(&record),
    );
    assert_eq!(
        host_ui.turns[0]
            .host_refs
            .iter()
            .map(|host_ref| host_ref.business_ref_id.as_str())
            .collect::<Vec<_>>(),
        vec!["host", "export"]
    );
    assert_eq!(host_ui.audit.redacted_host_refs, 3);
    assert!(host_ui.redactions.iter().any(|item| {
        item.reason == TranscriptRedactionReason::HostRefVisibility
            && item.host_ref_index == Some(0)
            && item.turn_id == "turn-visibility"
    }));

    let model = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::ModelContext,
        std::slice::from_ref(&record),
    );
    assert_eq!(model.turns[0].host_refs.len(), 1);
    assert_eq!(model.turns[0].host_refs[0].business_ref_id, "model");

    let export =
        RedactedTranscriptSlice::from_records(key, TranscriptReplayView::Export, &[record]);
    assert_eq!(export.turns[0].host_refs.len(), 1);
    assert_eq!(export.turns[0].host_refs[0].business_ref_id, "export");
}

#[test]
fn host_ref_label_is_redacted_for_non_owner_views() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-host-ref-label"),
        vec![
            host_ref("host", HostRefVisibility::HostUi),
            host_ref("model", HostRefVisibility::ModelContext),
            host_ref("operator", HostRefVisibility::OperatorAudit),
            host_ref("export", HostRefVisibility::Export),
        ],
        10,
    )
    .unwrap();

    let owner = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::RawOwnerOnly,
        std::slice::from_ref(&record),
    );
    assert!(owner.turns[0]
        .host_refs
        .iter()
        .all(|host_ref| host_ref.label.is_some()));

    let host_ui = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::HostUi,
        std::slice::from_ref(&record),
    );
    assert_eq!(host_ui.turns[0].host_refs[0].business_ref_id, "host");
    assert_eq!(
        host_ui.turns[0].host_refs[0].label.as_deref(),
        Some("opaque host")
    );
    assert_eq!(host_ui.turns[0].host_refs[1].business_ref_id, "export");
    assert!(host_ui.turns[0].host_refs[1].label.is_none());

    let model = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::ModelContext,
        std::slice::from_ref(&record),
    );
    assert_eq!(model.turns[0].host_refs[0].business_ref_id, "model");
    assert!(model.turns[0].host_refs[0].label.is_none());

    let operator = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::OperatorAudit,
        std::slice::from_ref(&record),
    );
    assert!(operator.turns[0]
        .host_refs
        .iter()
        .all(|host_ref| host_ref.label.is_none()));

    let export =
        RedactedTranscriptSlice::from_records(key, TranscriptReplayView::Export, &[record]);
    assert_eq!(export.turns[0].host_refs[0].business_ref_id, "export");
    assert!(export.turns[0].host_refs[0].label.is_none());
}

#[test]
fn transcript_attrs_are_filtered_per_replay_view_and_attached_to_message() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-attr-visibility"),
        Vec::new(),
        10,
    )
    .unwrap();
    let assistant_message_id = record
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    let host_ui_attr = message_usage_attr(
        &key,
        "turn-attr-visibility",
        &assistant_message_id,
        HostRefVisibility::HostUi,
    );
    let model_attr = TranscriptAttrEnvelope {
        attr_id: "attr-model-context".to_string(),
        visibility: HostRefVisibility::ModelContext,
        ..message_usage_attr(
            &key,
            "turn-attr-visibility",
            &assistant_message_id,
            HostRefVisibility::ModelContext,
        )
    };

    let host_ui = RedactedTranscriptSlice::from_records_with_attrs(
        key.clone(),
        TranscriptReplayView::HostUi,
        std::slice::from_ref(&record),
        &[host_ui_attr.clone(), model_attr.clone()],
    );

    let assistant = host_ui.turns[0].assistant_message.as_ref().unwrap();
    assert_eq!(assistant.attrs, vec![host_ui_attr]);
    assert!(host_ui.redactions.iter().any(|item| {
        item.reason == TranscriptRedactionReason::AttrVisibility
            && item.attr_id.as_deref() == Some("attr-model-context")
    }));

    let model = RedactedTranscriptSlice::from_records_with_attrs(
        key,
        TranscriptReplayView::ModelContext,
        &[record],
        &[model_attr],
    );
    assert_eq!(
        model.turns[0].assistant_message.as_ref().unwrap().attrs[0].visibility,
        HostRefVisibility::ModelContext
    );
}

#[test]
fn transcript_attrs_obey_mask_and_delete_raw_lifecycle_policies() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-attr-lifecycle"),
        Vec::new(),
        10,
    )
    .unwrap();
    let assistant_message_id = record
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    let metadata_attr = message_usage_attr(
        &key,
        "turn-attr-lifecycle",
        &assistant_message_id,
        HostRefVisibility::HostUi,
    );

    let mut masked_record = record.clone();
    masked_record.lifecycle_state = TranscriptLifecycleState::Masked;
    masked_record.redaction_state = TranscriptRedactionState::Masked;
    let masked = RedactedTranscriptSlice::from_records_with_attrs(
        key.clone(),
        TranscriptReplayView::HostUi,
        std::slice::from_ref(&masked_record),
        std::slice::from_ref(&metadata_attr),
    );
    assert_eq!(
        masked.turns[0].assistant_message.as_ref().unwrap().attrs,
        vec![metadata_attr.clone()]
    );

    let mut raw_deleted_record = record.clone();
    raw_deleted_record.lifecycle_state = TranscriptLifecycleState::RawDeleted;
    raw_deleted_record.redaction_state = TranscriptRedactionState::RawDeleted;
    let raw_deleted = RedactedTranscriptSlice::from_records_with_attrs(
        key.clone(),
        TranscriptReplayView::HostUi,
        std::slice::from_ref(&raw_deleted_record),
        std::slice::from_ref(&metadata_attr),
    );
    assert!(raw_deleted.turns[0]
        .assistant_message
        .as_ref()
        .unwrap()
        .attrs
        .is_empty());
    assert!(raw_deleted.redactions.iter().any(|item| {
        item.reason == TranscriptRedactionReason::AttrLifecyclePolicy
            && item.attr_id.as_deref() == Some(metadata_attr.attr_id.as_str())
    }));

    let mut audit_attr = message_usage_attr(
        &key,
        "turn-attr-lifecycle",
        &assistant_message_id,
        HostRefVisibility::OperatorAudit,
    );
    audit_attr.attr_id = "attr-operator-audit-only".to_string();
    audit_attr.governance.redaction_policy =
        TranscriptAttrRedactionPolicy::OperatorAuditOnlyAfterMask;
    audit_attr.value = json!({"secret": "provider body must not survive raw delete"});
    let operator = RedactedTranscriptSlice::from_records_with_attrs(
        key,
        TranscriptReplayView::OperatorAudit,
        std::slice::from_ref(&raw_deleted_record),
        std::slice::from_ref(&audit_attr),
    );
    let visible_attr = &operator.turns[0].assistant_message.as_ref().unwrap().attrs[0];
    assert_ne!(visible_attr.value, audit_attr.value);
    assert_eq!(visible_attr.value["redacted"], json!(true));
    assert_eq!(
        visible_attr.schema_ref.as_deref(),
        Some("memory.transcript.attr.redacted.v1")
    );
}

#[test]
fn transcript_attr_validation_rejects_unscoped_keys_and_bad_message_targets() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-attr-validation"),
        Vec::new(),
        10,
    )
    .unwrap();
    let assistant_message_id = record
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    let mut invalid_key = message_usage_attr(
        &key,
        "turn-attr-validation",
        &assistant_message_id,
        HostRefVisibility::HostUi,
    );
    invalid_key.key = "usage".to_string();
    assert!(invalid_key.validate_for_record(&record).is_err());

    let mut missing_message = message_usage_attr(
        &key,
        "turn-attr-validation",
        &assistant_message_id,
        HostRefVisibility::HostUi,
    );
    missing_message.target.message_id = None;
    assert!(missing_message.validate_for_record(&record).is_err());

    let mut wrong_message = message_usage_attr(
        &key,
        "turn-attr-validation",
        &assistant_message_id,
        HostRefVisibility::HostUi,
    );
    wrong_message.target.message_id = Some("missing-message".to_string());
    assert!(wrong_message.validate_for_record(&record).is_err());
}

#[test]
fn redaction_report_records_message_reasons() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut delta = delivered_delta("turn-private-report");
    delta.input_messages = vec![TranscriptInputMessage::new(
        "assistant",
        "private scratch",
        MemoryEvidenceAuthority::PrivateGardenInternal,
    )];
    delta.assistant_message = None;
    let record = TranscriptTurnRecord::from_delta(&key, 1, &delta, Vec::new(), 10).unwrap();

    let replay =
        RedactedTranscriptSlice::from_records(key, TranscriptReplayView::HostUi, &[record]);

    assert_eq!(replay.audit.redacted_messages, 1);
    assert!(replay.redactions.iter().any(|item| {
        item.reason == TranscriptRedactionReason::PrivateAuthority
            && item.message_id.is_some()
            && item.source_authority == Some(MemoryEvidenceAuthority::PrivateGardenInternal)
    }));
}

#[derive(Default)]
struct RepairFixtureStore {
    turns: Vec<TranscriptTurnRecord>,
    derived_refs: Vec<DerivedMemoryRef>,
    appended_intents: Mutex<Vec<TranscriptAppendIntent>>,
}

impl ConversationTranscriptStore for RepairFixtureStore {
    fn append_turn_intent(
        &self,
        intent: &TranscriptAppendIntent,
    ) -> Result<TranscriptCommitReport> {
        intent.validate()?;
        let mut appended = self.appended_intents.lock().unwrap();
        let before_count = appended.len();
        appended.push(intent.clone());
        Ok(TranscriptCommitReport {
            key: intent.record.key.clone(),
            turn_id: intent.record.turn_id.clone(),
            sequence: u64::try_from(before_count).unwrap_or(u64::MAX) + 1,
            committed: true,
            before_count,
            after_count: before_count + 1,
            skipped_reason: None,
        })
    }

    fn remember_conversation_alias(&self, _alias: &TranscriptConversationAlias) -> Result<()> {
        unimplemented!("repair fixture is read-only")
    }

    fn resolve_conversation_alias(
        &self,
        _memory_space_id: &str,
        _mounted_subject_id: &str,
        _channel_id: &str,
        _chat_id: &str,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    fn get_turn(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>> {
        Ok(self
            .appended_intents
            .lock()
            .unwrap()
            .iter()
            .find(|intent| {
                intent.record.key == *key
                    && intent.record.subject == mounted_subject_id
                    && intent.record.turn_id == turn_id
            })
            .map(|intent| intent.record.clone()))
    }

    fn list_turns(
        &self,
        _key: &ConversationKey,
        _mounted_subject_id: &str,
        _limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        Ok(self.turns.clone())
    }

    fn turn_count(&self, _key: &ConversationKey, _mounted_subject_id: &str) -> Result<usize> {
        Ok(self.turns.len() + self.appended_intents.lock().unwrap().len())
    }

    fn list_turns_page(
        &self,
        key: &ConversationKey,
        _mounted_subject_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<bm_core::memory::TranscriptTurnPage> {
        bm_core::memory::TranscriptTurnPage::from_records(key.clone(), &self.turns, cursor, limit)
    }

    fn list_conversation_catalog(
        &self,
        _mounted_subject_id: &str,
        _query: &bm_core::memory::TranscriptCatalogQuery,
    ) -> Result<bm_core::memory::ConversationCatalogCandidatePage> {
        unimplemented!("repair fixture is read-only")
    }

    fn query_transcript_timeline(
        &self,
        _mounted_subject_id: &str,
        _query: &bm_core::memory::TranscriptTimelineQuery,
    ) -> Result<bm_core::memory::TranscriptTimelineCandidatePage> {
        unimplemented!("repair fixture is read-only")
    }

    fn search_transcript(
        &self,
        _mounted_subject_id: &str,
        _query: &bm_core::memory::TranscriptSearchQuery,
    ) -> Result<bm_core::memory::TranscriptSearchCandidatePage> {
        unimplemented!("repair fixture is read-only")
    }

    fn query_transcript_activity(
        &self,
        _mounted_subject_id: &str,
        _query: &bm_core::memory::TranscriptActivityQuery,
    ) -> Result<bm_core::memory::TranscriptActivityCandidateReport> {
        unimplemented!("repair fixture is read-only")
    }

    fn append_derived_memory_ref(
        &self,
        _key: &ConversationKey,
        _derived: &DerivedMemoryRef,
    ) -> Result<()> {
        unimplemented!("repair fixture is read-only")
    }

    fn list_derived_memory_refs(
        &self,
        _key: &ConversationKey,
        _mounted_subject_id: &str,
        _turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>> {
        Ok(self.derived_refs.clone())
    }

    fn apply_lifecycle_request(
        &self,
        _mounted_subject_id: &str,
        _request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport> {
        unimplemented!("repair fixture is read-only")
    }
}

#[derive(Default)]
struct TranscriptSessionStore {
    messages: Mutex<BTreeMap<String, Vec<SessionMessage>>>,
}

impl SessionStore for TranscriptSessionStore {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.messages
            .lock()
            .unwrap()
            .entry(chat_id.to_string())
            .or_default()
            .push(SessionMessage::synthetic(role, content));
        Ok(())
    }

    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        self.messages
            .lock()
            .unwrap()
            .entry(chat_id.to_string())
            .or_default()
            .extend_from_slice(messages);
        Ok(())
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let mut messages = self
            .messages
            .lock()
            .unwrap()
            .get(chat_id)
            .cloned()
            .unwrap_or_default();
        if messages.len() > n {
            messages = messages[messages.len() - n..].to_vec();
        }
        Ok(messages)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.messages.lock().unwrap().remove(chat_id);
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        Ok(self.messages.lock().unwrap().keys().cloned().collect())
    }
}

#[test]
fn canonical_turn_commit_passes_conversation_alias_in_same_append_intent() {
    let session_store = TranscriptSessionStore::default();
    let transcript_store = RepairFixtureStore::default();
    let delta = delivered_delta("turn-alias-intent");
    let alias = TranscriptConversationAlias::new(
        "space-a",
        "subject-qingchuan",
        "llm.gateway",
        "legacy-chat-a",
        "conversation-a",
        1_800_000_010,
    )
    .unwrap();

    let report = commit_canonical_turn_delta_with_transcript(
        &session_store,
        &transcript_store,
        "space-a",
        &delta,
        Vec::new(),
        Some(alias.clone()),
        1_800_000_010,
    )
    .unwrap();

    assert!(report.session_commit.committed);
    assert!(report.transcript_commit.unwrap().committed);
    let intents = transcript_store.appended_intents.lock().unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].conversation_alias.as_ref(), Some(&alias));
}

#[test]
fn canonical_turn_backfill_passes_conversation_alias_in_same_append_intent() {
    let session_store = TranscriptSessionStore::default();
    let transcript_store = RepairFixtureStore::default();
    let delta = delivered_delta("turn-alias-backfill");
    assert!(
        commit_canonical_turn_delta(&session_store, &delta)
            .unwrap()
            .committed
    );
    let alias = TranscriptConversationAlias::new(
        "space-a",
        "subject-qingchuan",
        "llm.gateway",
        "legacy-chat-a",
        "conversation-a",
        1_800_000_010,
    )
    .unwrap();

    let report = commit_canonical_turn_delta_with_transcript(
        &session_store,
        &transcript_store,
        "space-a",
        &delta,
        Vec::new(),
        Some(alias.clone()),
        1_800_000_010,
    )
    .unwrap();

    assert!(!report.session_commit.committed);
    assert!(report.transcript_commit.unwrap().committed);
    let intents = transcript_store.appended_intents.lock().unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].conversation_alias.as_ref(), Some(&alias));
}

#[test]
fn mismatched_alias_fails_before_session_or_transcript_mutation() {
    let session_store = TranscriptSessionStore::default();
    let transcript_store = RepairFixtureStore::default();
    let delta = delivered_delta("turn-alias-mismatch");
    let alias = TranscriptConversationAlias::new(
        "space-b",
        "subject-qingchuan",
        "llm.gateway",
        "legacy-chat-a",
        "conversation-a",
        1_800_000_010,
    )
    .unwrap();

    assert!(commit_canonical_turn_delta_with_transcript(
        &session_store,
        &transcript_store,
        "space-a",
        &delta,
        Vec::new(),
        Some(alias),
        1_800_000_010,
    )
    .is_err());
    assert_eq!(session_store.message_count("legacy-chat-a").unwrap(), 0);
    assert!(transcript_store.appended_intents.lock().unwrap().is_empty());
}

#[test]
fn transcript_repair_report_flags_mismatched_orphan_duplicate_and_corrupt_records() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut first = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-repair-a"),
        Vec::new(),
        10,
    )
    .unwrap();
    let mut duplicate_sequence = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-repair-b"),
        Vec::new(),
        11,
    )
    .unwrap();
    duplicate_sequence.sequence = first.sequence;
    first.key = ConversationKey::new("other-space", "llm.gateway", "conversation-a").unwrap();
    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: String::new(),
        subject_id: Some("subject-default".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: "other-space".to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: "turn-repair-a".to_string(),
            message_id: None,
            subject_id: Some("subject-default".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 12,
    };
    let store = RepairFixtureStore {
        turns: vec![first, duplicate_sequence],
        derived_refs: vec![derived],
        appended_intents: Mutex::new(Vec::new()),
    };

    let report = store.repair_report(&key, "subject-agent").unwrap();
    let kinds = report
        .issues
        .iter()
        .map(|issue| issue.kind)
        .collect::<Vec<_>>();

    assert!(report.fail_closed);
    assert!(kinds.contains(&TranscriptRepairIssueKind::CorruptRecord));
    assert!(kinds.contains(&TranscriptRepairIssueKind::DuplicateTurnCursor));
    assert!(kinds.contains(&TranscriptRepairIssueKind::OrphanDerivedRef));
    assert!(kinds.contains(&TranscriptRepairIssueKind::MismatchedSourceKey));
}
