mod support;
use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    CanonicalTurnDelta, ConversationKey, DerivedMemoryPlane, DerivedMemoryRef, HostOpaqueRef,
    HostRefRelation, HostRefVisibility, MemoryEvidenceAuthority, MemoryTurnDeliveryStatus,
    MemoryTurnProtocol, MemoryTurnSource, TranscriptActivityQuery, TranscriptAttrEnvelope,
    TranscriptAttrGovernance, TranscriptAttrLink, TranscriptAttrRedactionPolicy,
    TranscriptAttrScope, TranscriptAttrSource, TranscriptAttrSourceKind, TranscriptAttrTarget,
    TranscriptAttrValueKind, TranscriptCatalogLifecycle, TranscriptCatalogQuery,
    TranscriptConversationAlias, TranscriptEvidenceRef, TranscriptInputMessage,
    TranscriptLifecycleRequest, TranscriptLifecycleState, TranscriptLifecycleTransition,
    TranscriptRepairIssueKind, TranscriptReplayView, TranscriptSearchLifecycle,
    TranscriptSearchNormalizerV1, TranscriptSearchQuery, TranscriptSearchScope,
    TranscriptSearchSort, TranscriptTimelineAnchor, TranscriptTimelineQuery, TranscriptTurnRecord,
    TranscriptUtcRange,
};
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{
    StoreBackendConfig, StoreCapacityBudget, StoreEventScope,
};
use serde_json::json;
use std::collections::BTreeMap;

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-transcript-store-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn rewrite_file_manifest_schema(root: &std::path::Path, schema_id: &str, version: u32) {
    let manifest_path = root.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["schema_id"] = json!(schema_id);
    manifest["schema_version"] = json!(version);
    std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}

fn file_tree_bytes(root: &std::path::Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(
        root: &std::path::Path,
        path: &std::path::Path,
        files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        let mut entries = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                visit(root, &entry, files);
            } else {
                files.insert(
                    entry.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(entry).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn transcript_query_indexes_cover_catalog_timeline_search_activity_and_tamper() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    for sequence in 1..=17 {
        let mut record = transcript_record(
            &key,
            &format!("turn-query-{sequence:02}"),
            &format!("cobalt 检索消息 {sequence}"),
        );
        record.sequence = sequence;
        record.created_at = 1_800_000_000 + sequence;
        record.updated_at = 1_800_000_000 + sequence;
        for message in record.input_messages.iter_mut().chain(
            record
                .assistant_messages
                .iter_mut()
                .map(|item| &mut item.message),
        ) {
            message.observed_at = 1_800_000_000 + sequence;
            message.created_at = 1_800_000_000 + sequence;
        }
        store.append_turn(&record).unwrap();
    }

    let governance = governance_context_digest("store-contract-query");
    let catalog = store
        .list_conversation_catalog(
            "subject-store",
            &TranscriptCatalogQuery {
                memory_space_id: "space-store".to_string(),
                governance_context_digest: governance.clone(),
                channel_id: Some("llm.gateway".to_string()),
                lifecycle: TranscriptCatalogLifecycle::ActiveOnly,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(catalog.heads.len(), 1);
    assert_eq!(catalog.heads[0].turn_count, 17);
    assert_eq!(catalog.heads[0].lifecycle.active.message_count, 34);

    let latest = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(
        latest
            .turns
            .iter()
            .map(|turn| turn.sequence)
            .collect::<Vec<_>>(),
        (10..=17).collect::<Vec<_>>()
    );
    let older_cursor = latest.older_cursor.expect("latest page has older cursor");
    let older = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: Some(older_cursor),
            },
        )
        .unwrap();
    assert_eq!(
        older
            .turns
            .iter()
            .map(|turn| turn.sequence)
            .collect::<Vec<_>>(),
        (2..=9).collect::<Vec<_>>()
    );
    let newer_cursor = older
        .newer_cursor
        .clone()
        .expect("middle page has newer cursor");
    let newest_again = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: Some(newer_cursor),
            },
        )
        .unwrap();
    assert_eq!(
        newest_again
            .turns
            .iter()
            .map(|turn| turn.sequence)
            .collect::<Vec<_>>(),
        (10..=17).collect::<Vec<_>>()
    );

    let oldest = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: older.older_cursor,
            },
        )
        .unwrap();
    assert_eq!(
        oldest
            .turns
            .iter()
            .map(|turn| turn.sequence)
            .collect::<Vec<_>>(),
        vec![1]
    );
    let middle_again = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: Some(oldest.newer_cursor.expect("oldest page has newer cursor")),
            },
        )
        .unwrap();
    assert_eq!(
        middle_again
            .turns
            .iter()
            .map(|turn| turn.sequence)
            .collect::<Vec<_>>(),
        (2..=9).collect::<Vec<_>>()
    );

    let search_query = TranscriptSearchQuery {
        scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
        governance_context_digest: governance,
        query: TranscriptSearchNormalizerV1::normalize("cobalt").unwrap(),
        sort: TranscriptSearchSort::ObservedAtDescending,
        lifecycle: TranscriptSearchLifecycle::ActiveOnly,
        limit: 8,
        cursor: None,
    };
    let search = store
        .search_transcript("subject-store", &search_query)
        .unwrap();
    assert_eq!(search.candidates.len(), 8);
    assert!(search.has_more);
    assert_eq!(search.candidates[0].record.sequence, 17);
    let mut encoded = search.next_cursor.unwrap().as_str().to_string();
    let replacement = if encoded.ends_with('0') { '1' } else { '0' };
    encoded.pop();
    encoded.push(replacement);
    let mut tampered_query = search_query;
    tampered_query.cursor =
        Some(bm_core::memory::TranscriptQueryCursor::try_from_encoded(encoded).unwrap());
    assert!(store
        .search_transcript("subject-store", &tampered_query)
        .is_err());

    let activity = store
        .query_transcript_activity(
            "subject-store",
            &TranscriptActivityQuery {
                key,
                ranges: vec![TranscriptUtcRange::new(1_800_000_000, 1_800_000_100).unwrap()],
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
            },
        )
        .unwrap();
    assert_eq!(activity.buckets.len(), 1);
    assert_eq!(activity.buckets[0].candidates.len(), 34);
}

#[test]
fn transcript_query_indexes_exclude_private_authority_before_candidate_hydration() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let mut record = transcript_record(&key, "turn-private-authority", "private-cobalt-secret");
    record.input_messages[0].authority = MemoryEvidenceAuthority::PrivateGardenInternal;
    record.input_messages[0].observed_at = 1_800_000_001;
    record.assistant_messages[0].message.content = "public-amber-sentinel".to_string();
    record.assistant_messages[0].message.observed_at = 1_800_000_002;
    let public_message_id = record.assistant_messages[0].message.message_id.clone();
    record.created_at = 1_800_000_001;
    record.updated_at = 1_800_000_002;
    store.append_turn(&record).unwrap();

    let governance = governance_context_digest("store-contract-private-authority");
    for private_term in ["private-cobalt-secret", "cobalt"] {
        let private = store
            .search_transcript(
                "subject-store",
                &TranscriptSearchQuery {
                    scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
                    governance_context_digest: governance.clone(),
                    query: TranscriptSearchNormalizerV1::normalize(private_term).unwrap(),
                    sort: TranscriptSearchSort::ObservedAtDescending,
                    lifecycle: TranscriptSearchLifecycle::ActiveOnly,
                    limit: 8,
                    cursor: None,
                },
            )
            .unwrap();
        assert!(private.candidates.is_empty());
        assert!(!private.has_more);
        assert!(private.next_cursor.is_none());
    }

    let public = store
        .search_transcript(
            "subject-store",
            &TranscriptSearchQuery {
                scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
                governance_context_digest: governance.clone(),
                query: TranscriptSearchNormalizerV1::normalize("amber").unwrap(),
                sort: TranscriptSearchSort::ObservedAtDescending,
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(public.candidates.len(), 1);
    assert_eq!(public.candidates[0].message_id, public_message_id);

    let activity = store
        .query_transcript_activity(
            "subject-store",
            &TranscriptActivityQuery {
                key,
                ranges: vec![TranscriptUtcRange::new(1_800_000_000, 1_800_000_010).unwrap()],
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
            },
        )
        .unwrap();
    assert_eq!(activity.buckets[0].candidates.len(), 1);
    assert_eq!(
        activity.buckets[0].candidates[0].message_id,
        public_message_id
    );
}

#[test]
fn query_keyring_rotates_without_append_keeps_grace_then_prunes_expired_previous() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    for sequence in 1..=2 {
        let mut record = transcript_record(
            &key,
            &format!("turn-rotation-{sequence}"),
            "rotation evidence",
        );
        record.sequence = sequence;
        store.append_turn(&record).unwrap();
    }
    let governance = governance_context_digest("store-contract-key-rotation");
    let old_cursor = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 1,
                cursor: None,
            },
        )
        .unwrap()
        .older_cursor
        .expect("two turns require an older cursor");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut snapshot = platform.export_store_snapshot().unwrap();
    let keyring = snapshot
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == "conversation_transcript_query_keyring_private")
        .expect("private keyring owner");
    let old_key_id = keyring.value["current"]["key_id"]
        .as_str()
        .unwrap()
        .to_string();
    keyring.value["current"]["expires_at"] = json!(now + 604_800);
    platform.import_store_snapshot(&snapshot).unwrap();

    let historical = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 1,
                cursor: Some(old_cursor.clone()),
            },
        )
        .unwrap();
    assert_eq!(historical.turns[0].sequence, 1);
    let mut rotated = platform.export_store_snapshot().unwrap();
    let keyring = rotated
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == "conversation_transcript_query_keyring_private")
        .expect("rotated keyring owner");
    assert_ne!(keyring.value["current"]["key_id"], json!(old_key_id));
    assert_eq!(keyring.value["previous"]["key_id"], json!(old_key_id));

    keyring.value["previous"]["created_at"] = json!(1);
    keyring.value["previous"]["expires_at"] = json!(2);
    platform.import_store_snapshot(&rotated).unwrap();
    store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 1,
                cursor: None,
            },
        )
        .unwrap();
    let pruned = platform.export_store_snapshot().unwrap();
    let keyring = pruned
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_transcript_query_keyring_private")
        .expect("pruned keyring owner");
    assert!(keyring.value["previous"].is_null());
    assert!(store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key,
                governance_context_digest: governance,
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 1,
                cursor: Some(old_cursor),
            },
        )
        .is_err());
}

#[test]
fn file_store_reopens_with_transcript_query_cursor_authority_and_indexes() {
    let root = temp_root("query-reopen");
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let governance = governance_context_digest("store-contract-query-reopen");
    let cursor = {
        let platform = support::open_store(config.clone()).unwrap();
        let store = platform.conversation_transcript_store();
        for sequence in 1..=9 {
            let mut record = transcript_record(
                &key,
                &format!("turn-reopen-{sequence:02}"),
                &format!("持久检索 cobalt {sequence}"),
            );
            record.sequence = sequence;
            record.created_at = 1_800_100_000 + sequence;
            record.updated_at = 1_800_100_000 + sequence;
            for message in record.input_messages.iter_mut().chain(
                record
                    .assistant_messages
                    .iter_mut()
                    .map(|item| &mut item.message),
            ) {
                message.observed_at = 1_800_100_000 + sequence;
                message.created_at = 1_800_100_000 + sequence;
            }
            store.append_turn(&record).unwrap();
        }
        store
            .query_transcript_timeline(
                "subject-store",
                &TranscriptTimelineQuery {
                    key: key.clone(),
                    governance_context_digest: governance.clone(),
                    anchor: TranscriptTimelineAnchor::Latest,
                    limit: 8,
                    cursor: None,
                },
            )
            .unwrap()
            .older_cursor
            .expect("nine turns require an older cursor")
    };

    let reopened = support::open_store(config).unwrap();
    let store = reopened.conversation_transcript_store();
    let older = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: Some(cursor),
            },
        )
        .unwrap();
    assert_eq!(older.turns.len(), 1);
    assert_eq!(older.turns[0].sequence, 1);
    let search = store
        .search_transcript(
            "subject-store",
            &TranscriptSearchQuery {
                scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
                governance_context_digest: governance,
                query: TranscriptSearchNormalizerV1::normalize("持久检索").unwrap(),
                sort: TranscriptSearchSort::RelevanceThenObservedAt,
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    assert!(!search.candidates.is_empty());
}

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_core::memory::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3".to_string()),
        request_id: Some("req-store".to_string()),
        client_conversation_hint: Some("window-store".to_string()),
    }
}

fn transcript_record(key: &ConversationKey, turn_id: &str, user: &str) -> TranscriptTurnRecord {
    TranscriptTurnRecord::from_delta(key, 1, &delta(turn_id, user), Vec::new(), 10).unwrap()
}

fn transcript_record_for_subject(
    key: &ConversationKey,
    subject: &str,
    turn_id: &str,
    user: &str,
) -> TranscriptTurnRecord {
    let mut delta = delta(turn_id, user);
    delta.subject = subject.to_string();
    TranscriptTurnRecord::from_delta(key, 1, &delta, Vec::new(), 10).unwrap()
}

fn model_usage_attr(
    key: &ConversationKey,
    turn_id: &str,
    message_id: &str,
) -> TranscriptAttrEnvelope {
    TranscriptAttrEnvelope {
        attr_id: format!("usage-{turn_id}-{message_id}"),
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
            "input_tokens": 42,
            "output_tokens": 9,
            "usage_source": "provider_reported"
        }),
        visibility: HostRefVisibility::HostUi,
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
            ref_id: "model-1".to_string(),
        }],
        created_at: 1_800_000_010,
        updated_at: 1_800_000_010,
    }
}

fn derived_ref_for_turn(
    key: &ConversationKey,
    turn_id: &str,
    message_id: &str,
) -> DerivedMemoryRef {
    DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: format!("long_term:{turn_id}"),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: turn_id.to_string(),
            message_id: Some(message_id.to_string()),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 1_800_000_010,
    }
}

fn delta(turn_id: &str, user: &str) -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: turn_id.to_string(),
        conversation: bm_core::memory::ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "legacy-chat-a".to_string(),
            conversation_id: Some("conversation-store".to_string()),
        },
        subject: "subject-store".to_string(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: turn_source(),
        actor: None,
        input_messages: vec![TranscriptInputMessage::user(user).with_speaker("owner", "human")],
        assistant_messages: vec![bm_core::memory::TranscriptAssistantMessage::complete(
            TranscriptInputMessage::assistant("noted"),
        )],
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    }
}

#[test]
fn same_timestamp_assistant_messages_keep_turn_order_in_search_and_activity() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let mut turn = delta("ordered-assistants", "请回答");
    turn.assistant_messages = vec![
        bm_core::memory::TranscriptAssistantMessage::complete(TranscriptInputMessage::assistant(
            "cobalt 第一条",
        )),
        bm_core::memory::TranscriptAssistantMessage::complete(TranscriptInputMessage::assistant(
            "cobalt 第二条",
        )),
    ];
    let record = (0..100)
        .find_map(|suffix| {
            turn.assistant_messages[1].message.content = format!("cobalt 第二条 {suffix}");
            let record =
                TranscriptTurnRecord::from_delta(&key, 1, &turn, Vec::new(), 1_800_500_000)
                    .unwrap();
            (record.assistant_messages[0].message.message_id
                > record.assistant_messages[1].message.message_id)
                .then_some(record)
        })
        .expect("find message IDs whose lexical order opposes turn order");
    let expected = record
        .assistant_messages
        .iter()
        .rev()
        .map(|item| item.message.message_id.clone())
        .collect::<Vec<_>>();
    store.append_turn(&record).unwrap();
    let search = store
        .search_transcript(
            "subject-store",
            &TranscriptSearchQuery {
                scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
                governance_context_digest: governance_context_digest("same-time-order"),
                query: TranscriptSearchNormalizerV1::normalize("cobalt").unwrap(),
                sort: TranscriptSearchSort::ObservedAtDescending,
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
                limit: 1,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(search.candidates[0].message_id, expected[0]);
    let next = store
        .search_transcript(
            "subject-store",
            &TranscriptSearchQuery {
                scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
                governance_context_digest: governance_context_digest("same-time-order"),
                query: TranscriptSearchNormalizerV1::normalize("cobalt").unwrap(),
                sort: TranscriptSearchSort::ObservedAtDescending,
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
                limit: 1,
                cursor: search.next_cursor,
            },
        )
        .unwrap();
    assert_eq!(next.candidates[0].message_id, expected[1]);
    let activity = store
        .query_transcript_activity(
            "subject-store",
            &TranscriptActivityQuery {
                key,
                ranges: vec![TranscriptUtcRange::new(1_800_500_000, 1_800_500_001).unwrap()],
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
            },
        )
        .unwrap();
    let observed = activity.buckets[0]
        .candidates
        .iter()
        .map(|candidate| candidate.message_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        record
            .input_messages
            .iter()
            .chain(record.assistant_messages.iter().map(|item| &item.message))
            .map(|message| message.message_id.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn store_rejects_mutated_assistant_content_and_authority_at_public_append_boundary() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let original = transcript_record(&key, "turn-invalid", "prompt");
    let mut blank = original.clone();
    blank.assistant_messages[0].message.content.clear();
    assert!(store.append_turn(&blank).is_err());
    let mut forged = original.clone();
    forged.assistant_messages[0].message.authority = MemoryEvidenceAuthority::UserAsserted;
    forged.assistant_messages[0].message.actor.speaker_kind = "human".into();
    assert!(store.append_turn(&forged).is_err());
    assert!(store
        .get_turn(&key, "subject-store", "turn-invalid")
        .unwrap()
        .is_none());
}

#[test]
fn public_append_rejects_forged_raw_deleted_record_with_retained_text() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let mut forged = transcript_record(&key, "forged-deleted", "secret raw text");
    forged.lifecycle_state = TranscriptLifecycleState::RawDeleted;
    forged.redaction_state = bm_core::memory::TranscriptRedactionState::RawDeleted;
    assert!(store.append_turn(&forged).is_err());
    assert!(store
        .get_turn(&key, "subject-store", "forged-deleted")
        .unwrap()
        .is_none());
}

#[test]
fn delete_raw_fails_closed_when_session_shadow_id_has_divergent_content() {
    let scope = StoreEventScope::new("agent-a", "owner-a", "llm.gateway", "legacy-chat-a")
        .with_memory_space("space-store")
        .with_subject("subject-store")
        .with_conversation("conversation-store");
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap())
            .unwrap()
            .with_event_scope(scope),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "divergent-shadow", "original secret");
    let message = &record.input_messages[0];
    platform
        .session_store()
        .append_batch(
            "legacy-chat-a",
            &[bm_core::memory::SessionMessage::new(
                &message.message_id,
                &message.role,
                "drifted original secret",
                message.observed_at,
                message.created_at,
                &message.actor.speaker_id,
                &message.actor.speaker_kind,
            )],
        )
        .unwrap();
    store.append_turn(&record).unwrap();
    let error = store
        .apply_lifecycle_request(
            "subject-store",
            &TranscriptLifecycleRequest {
                key: key.clone(),
                turn_id: Some(record.turn_id.clone()),
                transition: TranscriptLifecycleTransition::DeleteRaw,
                reason: "privacy".into(),
                requested_by: "owner".into(),
                requested_at: 20,
            },
        )
        .unwrap_err();
    assert_eq!(error.stage(), "transcript_lifecycle_plan");
    assert_eq!(
        store
            .get_turn(&key, "subject-store", &record.turn_id)
            .unwrap()
            .unwrap()
            .lifecycle_state,
        TranscriptLifecycleState::Active
    );
}

#[test]
fn delete_raw_clears_session_shadow_attrs_and_host_labels_without_touching_next_turn() {
    let scope = StoreEventScope::new("agent-a", "owner-a", "llm.gateway", "legacy-chat-a")
        .with_memory_space("space-store")
        .with_subject("subject-store")
        .with_conversation("conversation-store");
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap())
            .unwrap()
            .with_event_scope(scope),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let secret = "private verbatim assistant text";
    let mut first = delta("turn-private", "first user text");
    first.assistant_messages[0].message.content = secret.into();
    let refs = vec![HostOpaqueRef {
        host_kind: "ui".into(),
        business_ref_type: "card".into(),
        business_ref_id: "ref-1".into(),
        relation: HostRefRelation::Origin,
        visibility: HostRefVisibility::HostUi,
        label: Some(secret.into()),
    }];
    let record = TranscriptTurnRecord::from_delta(&key, 1, &first, refs, 1_800_600_000).unwrap();
    let next = TranscriptTurnRecord::from_delta(
        &key,
        2,
        &delta("turn-next", "keep this turn"),
        Vec::new(),
        1_800_600_001,
    )
    .unwrap();
    let session_messages = [&record, &next]
        .into_iter()
        .flat_map(|turn| {
            turn.input_messages
                .iter()
                .chain(turn.assistant_messages.iter().map(|item| &item.message))
        })
        .map(|message| {
            bm_core::memory::SessionMessage::new(
                &message.message_id,
                &message.role,
                &message.content,
                message.observed_at,
                message.created_at,
                &message.actor.speaker_id,
                &message.actor.speaker_kind,
            )
        })
        .collect::<Vec<_>>();
    platform
        .session_store()
        .append_batch("legacy-chat-a", &session_messages)
        .unwrap();
    platform
        .session_summary_store()
        .set("legacy-chat-a", secret)
        .unwrap();
    store.append_turn(&record).unwrap();
    store.append_turn(&next).unwrap();
    let mut attr = model_usage_attr(
        &key,
        "turn-private",
        &record.assistant_messages[0].message.message_id,
    );
    attr.value = json!({"source_excerpt": secret});
    store
        .upsert_transcript_attrs(&key, "subject-store", &[attr.clone()])
        .unwrap();
    store
        .apply_lifecycle_request(
            "subject-store",
            &TranscriptLifecycleRequest {
                key: key.clone(),
                turn_id: Some("turn-private".into()),
                transition: TranscriptLifecycleTransition::DeleteRaw,
                reason: "privacy".into(),
                requested_by: "owner".into(),
                requested_at: 1_800_600_010,
            },
        )
        .unwrap();
    let recent = platform
        .session_store()
        .load_recent("legacy-chat-a", 10)
        .unwrap();
    assert_eq!(recent.len(), session_messages.len());
    assert!(recent[..2].iter().all(|message| message.content.is_empty()));
    assert_eq!(recent[2..], session_messages[2..]);
    assert!(platform
        .session_summary_store()
        .get("legacy-chat-a")
        .unwrap()
        .is_none());
    let deleted = store
        .get_turn(&key, "subject-store", "turn-private")
        .unwrap()
        .unwrap();
    assert!(deleted
        .host_refs
        .iter()
        .all(|reference| reference.label.is_none()));
    assert!(attr.validate_for_record(&deleted).is_err());
    assert!(store
        .list_transcript_attrs(&key, "subject-store", Some("turn-private"))
        .unwrap()
        .is_empty());
    let report = store
        .upsert_transcript_attrs(&key, "subject-store", &[attr])
        .unwrap();
    assert!(report.accepted_attrs.is_empty());
    assert_eq!(report.rejected_attrs.len(), 1);
    // A later explicit summary write is a separate owner action; DeleteRaw
    // clears the summary that existed when the lifecycle transaction ran.
    platform
        .session_summary_store()
        .set("legacy-chat-a", "fresh independent summary")
        .unwrap();
    assert_eq!(
        platform
            .session_summary_store()
            .get("legacy-chat-a")
            .unwrap()
            .as_deref(),
        Some("fresh independent summary")
    );
}

#[test]
fn file_reopen_preserves_delete_raw_session_shadow_erasure() {
    let root = temp_root("delete-raw-session-file");
    let scope = StoreEventScope::new("agent-a", "owner-a", "llm.gateway", "legacy-chat-a")
        .with_memory_space("space-store")
        .with_subject("subject-store")
        .with_conversation("conversation-store");
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile())
        .unwrap()
        .with_event_scope(scope);
    assert_delete_raw_session_shadow_reopen(config);
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_delete_raw_session_shadow_erasure() {
    let root = temp_root("delete-raw-session-sqlite");
    let scope = StoreEventScope::new("agent-a", "owner-a", "llm.gateway", "legacy-chat-a")
        .with_memory_space("space-store")
        .with_subject("subject-store")
        .with_conversation("conversation-store");
    let config = StoreBackendConfig::sqlite(
        root.join("memory.sqlite3"),
        support::native_persistent_profile(),
    )
    .unwrap()
    .with_event_scope(scope);
    assert_delete_raw_session_shadow_reopen(config);
}

fn assert_delete_raw_session_shadow_reopen(config: StoreBackendConfig) {
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let secret = "persistent private assistant text";
    {
        let platform = support::open_store(config.clone()).unwrap();
        let store = platform.conversation_transcript_store();
        let mut turn = delta("turn-private", "persistent private user text");
        turn.assistant_messages[0].message.content = secret.into();
        let record =
            TranscriptTurnRecord::from_delta(&key, 1, &turn, Vec::new(), 1_800_600_000).unwrap();
        let session_messages = record
            .input_messages
            .iter()
            .chain(record.assistant_messages.iter().map(|item| &item.message))
            .map(|message| {
                bm_core::memory::SessionMessage::new(
                    &message.message_id,
                    &message.role,
                    &message.content,
                    message.observed_at,
                    message.created_at,
                    &message.actor.speaker_id,
                    &message.actor.speaker_kind,
                )
            })
            .collect::<Vec<_>>();
        platform
            .session_store()
            .append_batch("legacy-chat-a", &session_messages)
            .unwrap();
        platform
            .session_summary_store()
            .set("legacy-chat-a", secret)
            .unwrap();
        store.append_turn(&record).unwrap();
        store
            .apply_lifecycle_request(
                "subject-store",
                &TranscriptLifecycleRequest {
                    key: key.clone(),
                    turn_id: Some("turn-private".into()),
                    transition: TranscriptLifecycleTransition::DeleteRaw,
                    reason: "privacy".into(),
                    requested_by: "owner".into(),
                    requested_at: 1_800_600_010,
                },
            )
            .unwrap();
    }
    let reopened = support::open_store(config).unwrap();
    assert!(reopened
        .session_store()
        .load_recent("legacy-chat-a", 10)
        .unwrap()
        .iter()
        .all(|message| message.content.is_empty()));
    assert!(reopened
        .session_summary_store()
        .get("legacy-chat-a")
        .unwrap()
        .is_none());
    let record = reopened
        .conversation_transcript_store()
        .get_turn(&key, "subject-store", "turn-private")
        .unwrap()
        .unwrap();
    assert_eq!(record.lifecycle_state, TranscriptLifecycleState::RawDeleted);
}

#[test]
fn cancelled_multi_message_turn_indexes_and_erases_every_settlement() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let mut turn = delta("cancelled-with-speech", "请继续");
    turn.delivery_status = MemoryTurnDeliveryStatus::Cancelled;
    turn.assistant_messages = vec![
        bm_core::memory::TranscriptAssistantMessage::complete(TranscriptInputMessage::assistant(
            "第一条已完成",
        )),
        bm_core::memory::TranscriptAssistantMessage::interrupted_delivered_prefix(
            TranscriptInputMessage::assistant("第二条已交付前缀"),
        ),
    ];
    let record = TranscriptTurnRecord::from_delta(&key, 1, &turn, Vec::new(), 10).unwrap();
    let expected_ids = record
        .assistant_messages
        .iter()
        .map(|item| item.message.message_id.clone())
        .collect::<Vec<_>>();
    let mut corrupt = record.clone();
    corrupt.assistant_messages[1].message.message_id = expected_ids[0].clone();
    assert!(store.append_turn(&corrupt).is_err());
    assert!(store
        .get_turn(&key, "subject-store", &turn.turn_id)
        .unwrap()
        .is_none());
    store.append_turn(&record).unwrap();
    let loaded = store
        .get_turn(&key, "subject-store", &turn.turn_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.assistant_messages.len(), 2);
    assert_eq!(
        loaded.assistant_messages[0].message.message_id,
        expected_ids[0]
    );
    assert_eq!(
        loaded.assistant_messages[1].settlement,
        bm_core::memory::AssistantMessageSettlement::InterruptedDeliveredPrefix
    );

    let catalog = store
        .list_conversation_catalog(
            "subject-store",
            &TranscriptCatalogQuery {
                memory_space_id: "space-store".into(),
                governance_context_digest: governance_context_digest("cancelled-multi-message"),
                channel_id: Some("llm.gateway".into()),
                lifecycle: TranscriptCatalogLifecycle::ActiveOnly,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(catalog.heads[0].message_count, 3);

    let report = store
        .apply_lifecycle_request(
            "subject-store",
            &TranscriptLifecycleRequest {
                key: key.clone(),
                turn_id: Some(turn.turn_id.clone()),
                transition: TranscriptLifecycleTransition::DeleteRaw,
                reason: "user_redaction_request".into(),
                requested_by: "owner".into(),
                requested_at: 20,
            },
        )
        .unwrap();
    assert_eq!(report.affected_message_ids.len(), 3);
    assert!(expected_ids
        .iter()
        .all(|id| report.affected_message_ids.contains(id)));
    let deleted = store
        .get_turn(&key, "subject-store", &turn.turn_id)
        .unwrap()
        .unwrap();
    assert!(deleted
        .assistant_messages
        .iter()
        .all(|item| item.message.content.is_empty()));
}

#[test]
fn file_store_reopens_cancelled_multi_message_turn_without_losing_order() {
    let root = temp_root("cancelled-multi-message-reopen");
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let mut turn = delta("cancelled-reopen", "你说到哪里了");
    turn.delivery_status = MemoryTurnDeliveryStatus::Cancelled;
    turn.assistant_messages = vec![
        bm_core::memory::TranscriptAssistantMessage::complete(TranscriptInputMessage::assistant(
            "第一条完整发言",
        )),
        bm_core::memory::TranscriptAssistantMessage::interrupted_delivered_prefix(
            TranscriptInputMessage::assistant("第二条已交付前缀"),
        ),
    ];
    let record = TranscriptTurnRecord::from_delta(&key, 1, &turn, Vec::new(), 10).unwrap();
    {
        let platform = support::open_store(config.clone()).unwrap();
        platform
            .conversation_transcript_store()
            .append_turn(&record)
            .unwrap();
    }
    let reopened = support::open_store(config).unwrap();
    let loaded = reopened
        .conversation_transcript_store()
        .get_turn(&key, "subject-store", &turn.turn_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.assistant_messages, record.assistant_messages);
    let replay = reopened
        .conversation_transcript_store()
        .redacted_replay(&key, "subject-store", 8, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns[0].assistant_messages.len(), 2);
    assert_eq!(
        replay.turns[0].assistant_messages[1].settlement,
        bm_core::memory::AssistantMessageSettlement::InterruptedDeliveredPrefix
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_reopens_cancelled_multi_message_turn_without_losing_order() {
    let root = temp_root("cancelled-multi-message-sqlite");
    let config = StoreBackendConfig::sqlite(
        root.join("store.sqlite"),
        support::native_persistent_profile(),
    )
    .unwrap();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let mut turn = delta("cancelled-sqlite", "继续");
    turn.delivery_status = MemoryTurnDeliveryStatus::Cancelled;
    turn.assistant_messages = vec![
        bm_core::memory::TranscriptAssistantMessage::complete(TranscriptInputMessage::assistant(
            "第一条完整发言",
        )),
        bm_core::memory::TranscriptAssistantMessage::interrupted_delivered_prefix(
            TranscriptInputMessage::assistant("第二条已交付前缀"),
        ),
    ];
    let record = TranscriptTurnRecord::from_delta(&key, 1, &turn, Vec::new(), 10).unwrap();
    {
        let platform = support::open_store(config.clone()).unwrap();
        platform
            .conversation_transcript_store()
            .append_turn(&record)
            .unwrap();
    }
    let reopened = support::open_store(config).unwrap();
    let loaded = reopened
        .conversation_transcript_store()
        .get_turn(&key, "subject-store", &turn.turn_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.assistant_messages, record.assistant_messages);
    let _ = std::fs::remove_dir_all(root);
}

fn governance_context_digest(label: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(label.as_bytes()))
}

#[test]
fn store_persists_transcript_by_memory_space_channel_and_conversation() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let host_ref = HostOpaqueRef {
        host_kind: "host".to_string(),
        business_ref_type: "ticket".to_string(),
        business_ref_id: "T-1".to_string(),
        relation: HostRefRelation::EvidenceFor,
        visibility: HostRefVisibility::OperatorAudit,
        label: None,
    };
    let record =
        TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "hello"), vec![host_ref], 10)
            .unwrap();

    let report = store.append_turn(&record).unwrap();
    assert!(report.committed);
    assert_eq!(report.sequence, 1);

    let loaded = store
        .get_turn(&key, "subject-store", "turn-1")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.key, key);
    assert_eq!(loaded.turn_id, "turn-1");
    assert_eq!(loaded.host_refs[0].business_ref_id, "T-1");

    let turns = store.list_turns(&key, "subject-store", 10).unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].input_messages[0].actor.speaker_id, "owner");

    let replay = store
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("hello")
    );
}

#[test]
fn conversation_manifest_isolates_identical_conversation_and_turn_ids_by_subject() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let subject_a = transcript_record_for_subject(&key, "subject-a", "turn-shared", "from a");
    let subject_b = transcript_record_for_subject(&key, "subject-b", "turn-shared", "from b");

    store.append_turn(&subject_a).unwrap();
    store.append_turn(&subject_b).unwrap();

    let turns_a = store.list_turns(&key, "subject-a", 10).unwrap();
    let turns_b = store.list_turns(&key, "subject-b", 10).unwrap();
    assert_eq!(turns_a.len(), 1);
    assert_eq!(turns_b.len(), 1);
    assert_eq!(turns_a[0].subject, "subject-a");
    assert_eq!(turns_b[0].subject, "subject-b");
    assert_eq!(turns_a[0].input_messages[0].content, "from a");
    assert_eq!(turns_b[0].input_messages[0].content, "from b");
    assert!(store.list_turns(&key, "subject-c", 10).unwrap().is_empty());

    let snapshot = platform.export_store_snapshot().unwrap();
    let manifests = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "conversation_recall_manifests")
        .collect::<Vec<_>>();
    assert_eq!(manifests.len(), 2);
    assert!(manifests
        .iter()
        .any(|doc| doc.value["mounted_subject_id"] == json!("subject-a")));
    assert!(manifests
        .iter()
        .any(|doc| doc.value["mounted_subject_id"] == json!("subject-b")));
    assert_ne!(manifests[0].key, manifests[1].key);
}

#[test]
fn transcript_append_materializes_bounded_head_and_pages() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();

    for sequence in 1..=65 {
        let mut record = transcript_record(
            &key,
            &format!("turn-{sequence:04}"),
            &format!("message {sequence}"),
        );
        record.sequence = sequence;
        store.append_turn(&record).unwrap();
    }

    let snapshot = platform.export_store_snapshot().unwrap();
    let head = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_recall_manifests")
        .expect("typed conversation transcript head");
    assert_eq!(head.value["turn_count"], json!(65));
    assert_eq!(head.value["last_sequence"], json!(65));
    assert_eq!(head.value["page_count"], json!(2));
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "conversation_transcript_pages")
            .count(),
        2
    );
}

#[test]
fn transcript_append_and_tail_read_scale_past_legacy_manifest_ceiling() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .unwrap()
    .try_with_nonproduction_store_budget_limit(StoreCapacityBudget::full().into_runtime_budget())
    .unwrap();
    let platform = support::open_store_in_memory(config).unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-scale", "llm.gateway", "conversation-store").unwrap();

    for sequence in 1..=1_000 {
        if sequence % 200 == 1 {
            platform
                .refresh_runtime_resource_snapshot_for_nonproduction_harness()
                .unwrap();
        }
        let mut record = transcript_record(
            &key,
            &format!("turn-{sequence:04}"),
            &format!("message {sequence}"),
        );
        record.sequence = sequence;
        let report = store.append_turn(&record).unwrap();
        assert_eq!(report.sequence, sequence);
        assert_eq!(report.after_count, sequence as usize);
    }

    let tail = store.list_turns(&key, "subject-store", 10).unwrap();
    assert_eq!(tail.len(), 10);
    assert_eq!(tail.first().unwrap().sequence, 991);
    assert_eq!(tail.last().unwrap().sequence, 1_000);
}

#[test]
fn transcript_tail_and_forward_page_do_not_read_unrelated_history_pages() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    for sequence in 1..=130 {
        let mut record = transcript_record(
            &key,
            &format!("turn-{sequence:04}"),
            &format!("message {sequence}"),
        );
        record.sequence = sequence;
        store.append_turn(&record).unwrap();
    }
    let first_doc = platform
        .export_store_snapshot()
        .unwrap()
        .json_docs
        .into_iter()
        .find(|doc| {
            doc.namespace == "conversation_transcript" && doc.value["turn_id"] == json!("turn-0001")
        })
        .expect("first transcript owner");
    let first_key = first_doc.key;
    let mut corrupt = first_doc.value;
    corrupt["subject"] = json!("wrong-subject");
    platform
        .tamper_json_document_for_nonproduction_harness(
            "conversation_transcript",
            &first_key,
            corrupt,
        )
        .unwrap();

    let tail = store.list_turns(&key, "subject-store", 10).unwrap();
    assert_eq!(tail.first().unwrap().sequence, 121);
    assert_eq!(tail.last().unwrap().sequence, 130);
    assert!(store
        .list_turns_page(&key, "subject-store", None, 10)
        .is_err());
}

#[test]
fn transcript_cursor_is_typed_tamper_evident_and_scope_bound() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    for sequence in 1..=2 {
        let mut record = transcript_record_for_subject(
            &key,
            "subject-store",
            &format!("turn-{sequence}"),
            "cursor",
        );
        record.sequence = sequence;
        store.append_turn(&record).unwrap();
    }
    let first = store
        .list_turns_page(&key, "subject-store", None, 1)
        .unwrap();
    let cursor = first.next_cursor.expect("bounded first page cursor");
    let mut tampered = cursor.clone().into_bytes();
    let last = tampered.last_mut().expect("cursor byte");
    *last = if *last == b'0' { b'1' } else { b'0' };
    let tampered = String::from_utf8(tampered).unwrap();
    assert!(store
        .list_turns_page(&key, "subject-store", Some(&tampered), 1)
        .is_err());
    assert!(store
        .list_turns_page(&key, "another-subject", Some(&cursor), 1)
        .is_err());
}

#[test]
fn transcript_per_turn_aux_scales_with_one_thousand_turns() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .unwrap()
    .try_with_nonproduction_store_budget_limit(StoreCapacityBudget::full().into_runtime_budget())
    .unwrap();
    let platform = support::open_store_in_memory(config).unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-aux", "llm.gateway", "conversation-store").unwrap();

    for sequence in 1..=1_000 {
        if sequence % 200 == 1 {
            platform
                .refresh_runtime_resource_snapshot_for_nonproduction_harness()
                .unwrap();
        }
        let turn_id = format!("turn-{sequence:04}");
        let mut record = transcript_record(&key, &turn_id, "bounded aux");
        record.sequence = sequence;
        let message_id = record.assistant_messages[0].message.message_id.clone();
        store.append_turn(&record).unwrap();
        store
            .upsert_transcript_attrs(
                &key,
                "subject-store",
                &[model_usage_attr(&key, &turn_id, &message_id)],
            )
            .unwrap();
        store
            .append_derived_memory_ref(&key, &derived_ref_for_turn(&key, &turn_id, &message_id))
            .unwrap();
    }

    platform
        .refresh_runtime_resource_snapshot_for_nonproduction_harness()
        .unwrap();
    let snapshot = platform.export_store_snapshot().unwrap();
    let head = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_recall_manifests")
        .expect("bounded conversation head");
    assert_eq!(head.value["turn_count"], json!(1_000));
    assert_eq!(head.value["page_count"], json!(16));
    assert!(head.value.get("entries").is_none());
    assert!(
        serde_json::to_vec(&head.value).unwrap().len() < 1_024,
        "conversation head must stay constant-size"
    );
    let pages = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "conversation_transcript_pages")
        .collect::<Vec<_>>();
    assert_eq!(pages.len(), 16);
    assert!(pages.iter().all(|page| {
        page.value["entries"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty() && entries.len() <= 64)
    }));
    let aux = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "conversation_transcript_aux_manifests")
        .collect::<Vec<_>>();
    assert_eq!(aux.len(), 1_000);
    assert!(aux.iter().all(|manifest| {
        manifest.value["entries"]
            .as_array()
            .is_some_and(|entries| entries.len() == 2)
    }));

    let replay = store
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns.len(), 10);
    assert_eq!(
        replay
            .turns
            .iter()
            .map(|turn| turn.attrs.len())
            .sum::<usize>(),
        0
    );
    assert_eq!(
        replay
            .turns
            .iter()
            .filter_map(|turn| turn.assistant_messages.first().map(|item| &item.message))
            .map(|message| message.attrs.len())
            .sum::<usize>(),
        10
    );
    let repair = store.repair_report(&key, "subject-store").unwrap();
    assert!(!repair.fail_closed);
    assert_eq!(repair.checked_turns, 1_000);
    assert_eq!(repair.checked_attrs, 1_000);
    assert_eq!(repair.checked_derived_refs, 1_000);
}

#[test]
fn conversation_manifest_identity_tampering_fails_closed() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    platform
        .conversation_transcript_store()
        .append_turn(&transcript_record_for_subject(
            &key,
            "subject-a",
            "turn-a",
            "owned by a",
        ))
        .unwrap();

    let mut snapshot = platform.export_store_snapshot().unwrap();
    let manifest = snapshot
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == "conversation_recall_manifests")
        .expect("conversation manifest");
    manifest.value["mounted_subject_id"] = json!("subject-b");

    let target = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let error = target
        .import_store_snapshot(&snapshot)
        .expect_err("tampered manifest identity must fail closed at admission");
    assert_eq!(error.stage(), "store_snapshot_import");
}

#[test]
fn store_persists_transcript_attrs_and_replays_visible_message_attrs() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "turn-attr", "count this model reply");
    let message_id = record.assistant_messages[0].message.message_id.clone();
    store.append_turn(&record).unwrap();

    let attr = model_usage_attr(&key, "turn-attr", &message_id);
    let report = store
        .upsert_transcript_attrs(&key, "subject-store", std::slice::from_ref(&attr))
        .unwrap();

    assert_eq!(report.accepted_attrs, vec![attr.clone()]);
    assert!(report.rejected_attrs.is_empty());
    let replay = store
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(
        replay.turns[0].assistant_messages[0].message.attrs,
        vec![attr]
    );
}

#[test]
fn store_rejects_transcript_attrs_when_target_turn_or_message_is_missing() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "turn-attr-target", "target exists");
    let message_id = record.assistant_messages[0].message.message_id.clone();
    store.append_turn(&record).unwrap();

    let missing_turn = model_usage_attr(&key, "missing-turn", &message_id);
    let mut missing_message = model_usage_attr(&key, "turn-attr-target", &message_id);
    missing_message.attr_id = "usage-missing-message".to_string();
    missing_message.target.message_id = Some("missing-message".to_string());
    let report = store
        .upsert_transcript_attrs(&key, "subject-store", &[missing_turn, missing_message])
        .unwrap();

    assert!(report.accepted_attrs.is_empty());
    assert_eq!(report.rejected_attrs.len(), 2);
    assert!(store
        .list_transcript_attrs(&key, "subject-store", None)
        .unwrap()
        .is_empty());
    let repair = store.repair_report(&key, "subject-store").unwrap();
    assert_eq!(repair.checked_attrs, 0);
    assert!(!repair.fail_closed);
}

#[test]
fn store_replay_fails_closed_while_repair_reports_corrupt_transcript_attr_records() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "turn-corrupt-attr", "target exists");
    let message_id = record.assistant_messages[0].message.message_id.clone();
    store.append_turn(&record).unwrap();
    let valid_attr = model_usage_attr(&key, "turn-corrupt-attr", &message_id);
    store
        .upsert_transcript_attrs(&key, "subject-store", &[valid_attr])
        .unwrap();

    let snapshot = platform.export_store_snapshot().unwrap();
    let attr_doc = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_transcript_attr")
        .expect("indexed transcript attr owner");
    let attr_key = attr_doc.key.clone();
    let mut corrupt_attr = attr_doc.value.clone();
    corrupt_attr["visibility"] = json!("not_a_visibility");
    let target = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    target.import_store_snapshot(&snapshot).unwrap();
    target
        .tamper_json_document_for_nonproduction_harness(
            "conversation_transcript_attr",
            &attr_key,
            corrupt_attr,
        )
        .expect("inject post-admission corruption for repair inspection");
    let target_store = target.conversation_transcript_store();

    let replay_error = target_store
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .expect_err("corrupt governed attrs must prevent replay rendering");
    assert!(replay_error.to_string().contains("not_a_visibility"));
    let repair = target_store.repair_report(&key, "subject-store").unwrap();
    assert!(repair.fail_closed);
    assert_eq!(repair.checked_attrs, 1);
    assert!(repair.issues.iter().any(|issue| {
        issue.kind == TranscriptRepairIssueKind::CorruptTranscriptAttrRecord
            && issue.turn_id == "turn-corrupt-attr"
    }));
}

#[test]
fn store_resolves_subject_owned_conversation_transcript_alias_by_chat_id() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    store
        .remember_conversation_alias(
            &TranscriptConversationAlias::new(
                "space-store",
                "subject-store",
                "llm.gateway",
                "legacy-chat-a",
                "conversation-store",
                10,
            )
            .unwrap(),
        )
        .unwrap();
    store
        .remember_conversation_alias(
            &TranscriptConversationAlias::new(
                "space-store",
                "subject-other",
                "llm.gateway",
                "legacy-chat-a",
                "conversation-other",
                11,
            )
            .unwrap(),
        )
        .unwrap();

    assert_eq!(
        store
            .resolve_conversation_alias(
                "space-store",
                "subject-store",
                "llm.gateway",
                "legacy-chat-a",
            )
            .unwrap()
            .as_deref(),
        Some("conversation-store")
    );
    assert_eq!(
        store
            .resolve_conversation_alias(
                "space-store",
                "subject-other",
                "llm.gateway",
                "legacy-chat-a",
            )
            .unwrap()
            .as_deref(),
        Some("conversation-other")
    );
    assert!(store
        .resolve_conversation_alias(
            "space-store",
            "subject-store",
            "llm.gateway",
            "missing-chat",
        )
        .unwrap()
        .is_none());
}

#[test]
fn lifecycle_request_masks_raw_transcript_content_but_keeps_audit_key() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record =
        TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "mask me"), Vec::new(), 10)
            .unwrap();
    store.append_turn(&record).unwrap();

    let report = store
        .apply_lifecycle_request(
            "subject-store",
            &TranscriptLifecycleRequest {
                key: key.clone(),
                turn_id: Some("turn-1".to_string()),
                transition: TranscriptLifecycleTransition::DeleteRaw,
                reason: "user_redaction_request".to_string(),
                requested_by: "owner".to_string(),
                requested_at: 20,
            },
        )
        .unwrap();

    assert_eq!(report.affected_turns, 1);
    assert_eq!(report.affected_turn_ids, vec!["turn-1".to_string()]);
    assert_eq!(report.affected_message_ids.len(), 2);
    assert!(report.affected_host_refs.is_empty());
    assert!(report.derived_memory_refs.is_empty());
    let loaded = store
        .get_turn(&key, "subject-store", "turn-1")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.lifecycle_state, TranscriptLifecycleState::RawDeleted);

    let replay = store
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns[0].input_messages[0].content, None);
    assert_eq!(replay.audit.redacted_messages, 2);
    assert_eq!(replay.redactions.len(), 2);
}

#[test]
fn lifecycle_report_includes_memory_owned_derived_refs_for_affected_turns() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record =
        TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "derive me"), Vec::new(), 10)
            .unwrap();
    let message = record.input_messages[0].clone();
    store.append_turn(&record).unwrap();

    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: "long_term:preference:response_style".to_string(),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: "turn-1".to_string(),
            message_id: Some(message.message_id),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 11,
    };
    store.append_derived_memory_ref(&key, &derived).unwrap();

    let report = store
        .apply_lifecycle_request(
            "subject-store",
            &TranscriptLifecycleRequest {
                key: key.clone(),
                turn_id: Some("turn-1".to_string()),
                transition: TranscriptLifecycleTransition::Mask,
                reason: "review_derived_memory".to_string(),
                requested_by: "owner".to_string(),
                requested_at: 20,
            },
        )
        .unwrap();

    assert_eq!(report.affected_turn_ids, vec!["turn-1".to_string()]);
    assert_eq!(report.derived_memory_refs, vec![derived]);
}

#[test]
fn transcript_list_page_returns_bounded_cursor_pages() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    store
        .append_turn(
            &TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "first"), Vec::new(), 10)
                .unwrap(),
        )
        .unwrap();
    store
        .append_turn(
            &TranscriptTurnRecord::from_delta(&key, 2, &delta("turn-2", "second"), Vec::new(), 11)
                .unwrap(),
        )
        .unwrap();

    let first = store
        .list_turns_page(&key, "subject-store", None, 1)
        .unwrap();
    assert_eq!(first.turns[0].turn_id, "turn-1");
    assert!(first.has_more);
    let second = store
        .list_turns_page(&key, "subject-store", first.next_cursor.as_deref(), 1)
        .unwrap();
    assert_eq!(second.turns[0].turn_id, "turn-2");
    assert!(!second.has_more);
    assert!(second.next_cursor.is_none());
}

#[test]
fn transcript_repair_report_flags_missing_derived_source_turn() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: "long_term:missing-source".to_string(),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: "missing-turn".to_string(),
            message_id: Some("missing-message".to_string()),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 11,
    };
    store.append_derived_memory_ref(&key, &derived).unwrap();

    let report = store.repair_report(&key, "subject-store").unwrap();
    assert!(report.fail_closed);
    assert_eq!(report.checked_turns, 0);
    assert_eq!(report.checked_derived_refs, 1);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(
        report.issues[0].kind,
        TranscriptRepairIssueKind::MissingSourceTurn
    );
    assert_eq!(report.issues[0].turn_id, "missing-turn");
}

#[test]
fn transcript_repair_report_flags_missing_derived_source_message() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "turn-without-message", "present message");
    store.append_turn(&record).unwrap();
    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: "long_term:missing-message".to_string(),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: "turn-without-message".to_string(),
            message_id: Some("missing-message".to_string()),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 11,
    };
    store.append_derived_memory_ref(&key, &derived).unwrap();

    let report = store.repair_report(&key, "subject-store").unwrap();

    assert!(report.fail_closed);
    assert_eq!(report.checked_turns, 1);
    assert_eq!(report.checked_derived_refs, 1);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(
        report.issues[0].kind,
        TranscriptRepairIssueKind::MissingSourceMessage
    );
    assert_eq!(
        report.issues[0].message_id.as_deref(),
        Some("missing-message")
    );
}

#[test]
fn transcript_write_events_preserve_memory_space_scope_and_record_key() {
    let scope = StoreEventScope::new("agent-a", "owner-a", "llm.gateway", "legacy-chat-a")
        .with_memory_space("space-store")
        .with_subject("subject-store")
        .with_conversation("conversation-store");
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .unwrap()
    .with_event_scope(scope);
    let platform = support::open_store_in_memory(config).unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record =
        TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "event"), Vec::new(), 10)
            .unwrap();

    store.append_turn(&record).unwrap();
    let events = platform.read_events().unwrap();
    let event = events
        .iter()
        .find(|event| event.kind_name == "memory.write" && event.plane == "conversation_transcript")
        .expect("transcript write event");

    assert_eq!(event.scope.memory_space_id, "space-store");
    assert_eq!(event.scope.subject_id, "subject-store");
    assert_eq!(
        event.scope.conversation_id.as_deref(),
        Some("conversation-store")
    );
    assert!(event.record_key.contains("space-store"));
    assert!(event.record_key.contains("conversation-store"));
    let snapshot = platform.export_store_snapshot().unwrap();
    let transcript_doc = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_transcript")
        .expect("transcript owner in snapshot");
    assert_eq!(event.record_key, transcript_doc.key);
    let persisted: TranscriptTurnRecord =
        serde_json::from_value(transcript_doc.value.clone()).expect("typed transcript owner");
    assert_eq!(persisted.turn_id, "turn-1");
}

#[test]
fn file_store_persists_transcript_across_reopen() {
    let root = temp_root("file-reopen");
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap();
    let key = ConversationKey::new("space-file", "llm.gateway", "conversation-store").unwrap();

    {
        let platform = support::open_store(config.clone()).unwrap();
        platform
            .conversation_transcript_store()
            .append_turn(&transcript_record(&key, "turn-file", "persist me"))
            .unwrap();
    }

    let reopened = support::open_store(config).unwrap();
    let replay = reopened
        .conversation_transcript_store()
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("persist me")
    );
}

#[test]
fn file_store_persists_long_transcript_keys_and_attrs_across_reopen() {
    let root = temp_root("file-long-transcript-key");
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap();
    let key = ConversationKey::new(
        "space:local-user-with-default-desktop-memory",
        "llm.gateway",
        format!("work-room:{}", "long-input-segment-".repeat(16)),
    )
    .unwrap();
    let mut turn_delta = delta("turn-long-file-key", "persist long key");
    turn_delta.conversation.channel = key.channel_id.clone();
    turn_delta.conversation.conversation_id = Some(key.conversation_id.clone());
    let record = TranscriptTurnRecord::from_delta(&key, 1, &turn_delta, Vec::new(), 10).unwrap();
    let message_id = record.assistant_messages[0].message.message_id.clone();
    let attr = model_usage_attr(&key, "turn-long-file-key", &message_id);

    {
        let platform = support::open_store(config.clone()).unwrap();
        let store = platform.conversation_transcript_store();
        store.append_turn(&record).unwrap();
        store
            .upsert_transcript_attrs(&key, "subject-store", std::slice::from_ref(&attr))
            .unwrap();
    }

    let reopened = support::open_store(config).unwrap();
    let replay = reopened
        .conversation_transcript_store()
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .unwrap();

    assert_eq!(replay.turns.len(), 1);
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("persist long key")
    );
    assert_eq!(
        replay.turns[0].assistant_messages[0].message.attrs,
        vec![attr]
    );
}

#[test]
fn snapshot_export_import_carries_conversation_transcript_namespace() {
    let source = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let key = ConversationKey::new("space-snapshot", "llm.gateway", "conversation-store").unwrap();
    let store = source.conversation_transcript_store();
    let record = transcript_record(&key, "turn-snapshot", "snapshot me");
    let message = record.input_messages[0].clone();
    store.append_turn(&record).unwrap();
    let attr = model_usage_attr(&key, "turn-snapshot", &message.message_id);
    store
        .upsert_transcript_attrs(&key, "subject-store", std::slice::from_ref(&attr))
        .unwrap();
    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::ArchiveEvidence,
        store_key: "archive:conversation-store:turn-snapshot".to_string(),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: "turn-snapshot".to_string(),
            message_id: Some(message.message_id),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 12,
    };
    store.append_derived_memory_ref(&key, &derived).unwrap();

    let snapshot = source.export_store_snapshot().unwrap();
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"));
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript_attr"));
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript_derived_ref"));

    let target = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    target.import_store_snapshot(&snapshot).unwrap();
    let replay = target
        .conversation_transcript_store()
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("snapshot me")
    );
    assert_eq!(replay.turns[0].input_messages[0].attrs, vec![attr]);
    let derived_refs = target
        .conversation_transcript_store()
        .list_derived_memory_refs(&key, "subject-store", Some("turn-snapshot"))
        .unwrap();
    assert_eq!(derived_refs, vec![derived]);
}

#[test]
fn file_snapshot_export_import_preserves_long_transcript_keys_and_attrs() {
    let source_root = temp_root("file-snapshot-source-long-key");
    let target_root = temp_root("file-snapshot-target-long-key");
    let source = support::open_store(
        StoreBackendConfig::file(&source_root, support::native_persistent_profile()).unwrap(),
    )
    .unwrap();
    let key = ConversationKey::new(
        "space:local-user-with-default-desktop-memory",
        "llm.gateway",
        format!("work-room:{}", "long-input-segment-".repeat(16)),
    )
    .unwrap();
    let mut turn_delta = delta("turn-long-file-snapshot", "snapshot long key");
    turn_delta.conversation.channel = key.channel_id.clone();
    turn_delta.conversation.conversation_id = Some(key.conversation_id.clone());
    let record = TranscriptTurnRecord::from_delta(&key, 1, &turn_delta, Vec::new(), 10).unwrap();
    let message_id = record.assistant_messages[0].message.message_id.clone();
    let attr = model_usage_attr(&key, "turn-long-file-snapshot", &message_id);
    let source_store = source.conversation_transcript_store();
    source_store.append_turn(&record).unwrap();
    source_store
        .upsert_transcript_attrs(&key, "subject-store", std::slice::from_ref(&attr))
        .unwrap();

    let snapshot = source.export_store_snapshot().unwrap();
    assert!(snapshot.json_docs.iter().any(|doc| {
        doc.namespace == "conversation_transcript"
            && serde_json::from_value::<TranscriptTurnRecord>(doc.value.clone())
                .is_ok_and(|turn| turn.key == key && turn.turn_id == "turn-long-file-snapshot")
    }));
    assert!(snapshot.json_docs.iter().any(|doc| {
        doc.namespace == "conversation_transcript_attr"
            && serde_json::from_value::<TranscriptAttrEnvelope>(doc.value.clone())
                .is_ok_and(|attr| attr.target.key == key)
    }));

    let target = support::open_store(
        StoreBackendConfig::file(&target_root, support::native_persistent_profile()).unwrap(),
    )
    .unwrap();
    target.import_store_snapshot(&snapshot).unwrap();
    let target_store = target.conversation_transcript_store();
    let turns = target_store.list_turns(&key, "subject-store", 10).unwrap();
    assert_eq!(turns.len(), 1);

    let replay = target_store
        .redacted_replay(&key, "subject-store", 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("snapshot long key")
    );
    assert_eq!(
        replay.turns[0].assistant_messages[0].message.attrs,
        vec![attr]
    );
}

#[test]
fn independent_persistent_platforms_conflict_then_explicitly_replan_transcript_append() {
    let root = temp_root("independent-platform-transcript-cas");
    let profile = support::native_persistent_profile();
    let configs = vec![(
        "file",
        StoreBackendConfig::file(root.join("file"), profile).expect("file config"),
    )];
    #[cfg(feature = "sqlite-store")]
    let configs = configs
        .into_iter()
        .chain(std::iter::once((
            "sqlite",
            StoreBackendConfig::sqlite(root.join("sqlite.db"), profile).expect("sqlite config"),
        )))
        .collect::<Vec<_>>();

    for (backend, config) in configs {
        let first_platform = support::open_store(config.clone())
            .unwrap_or_else(|error| panic!("open first {backend} platform: {error}"));
        let second_platform = support::open_store(config)
            .unwrap_or_else(|error| panic!("open second {backend} platform: {error}"));
        let key = ConversationKey::new(
            format!("space-{backend}-concurrent"),
            "llm.gateway",
            "conversation-store",
        )
        .unwrap();
        let mut first_record = transcript_record(&key, "turn-first", "first concurrent append");
        first_record.sequence = 0;
        let mut second_record = transcript_record(&key, "turn-second", "second concurrent append");
        second_record.sequence = 0;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let first_store = first_platform.conversation_transcript_store();
        let first_barrier = std::sync::Arc::clone(&barrier);
        let first_candidate = first_record.clone();
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            first_store
                .append_turn(&first_candidate)
                .map(|report| ("turn-first", report))
                .map_err(|error| ("turn-first", error))
        });
        let second_store = second_platform.conversation_transcript_store();
        let second_barrier = std::sync::Arc::clone(&barrier);
        let second_candidate = second_record.clone();
        let second = std::thread::spawn(move || {
            second_barrier.wait();
            second_store
                .append_turn(&second_candidate)
                .map(|report| ("turn-second", report))
                .map_err(|error| ("turn-second", error))
        });
        let outcomes = [
            first.join().expect("first append thread"),
            second.join().expect("second append thread"),
        ];
        let committed = outcomes
            .iter()
            .filter_map(|outcome| outcome.as_ref().ok())
            .collect::<Vec<_>>();
        let conflicted = outcomes
            .iter()
            .filter_map(|outcome| outcome.as_ref().err())
            .collect::<Vec<_>>();
        assert_eq!(committed.len(), 1, "backend={backend}");
        assert_eq!(committed[0].1.sequence, 1, "backend={backend}");
        assert_eq!(conflicted.len(), 1, "backend={backend}");
        assert_eq!(
            conflicted[0].1.stage(),
            "memory_write_transaction_precondition_failed",
            "backend={backend}: {}",
            conflicted[0].1
        );

        let before_retry = first_platform
            .conversation_transcript_store()
            .list_turns(&key, "subject-store", 10)
            .unwrap_or_else(|error| panic!("read {backend} before retry: {error}"));
        assert_eq!(before_retry.len(), 1, "backend={backend}");
        let conflicted_record = match conflicted[0].0 {
            "turn-first" => &first_record,
            "turn-second" => &second_record,
            other => panic!("unexpected conflicted turn {other}"),
        };
        let retry = first_platform
            .conversation_transcript_store()
            .append_turn(conflicted_record)
            .unwrap_or_else(|error| panic!("explicitly replan {backend} append: {error}"));
        assert_eq!(retry.sequence, 2, "backend={backend}");

        let turns = second_platform
            .conversation_transcript_store()
            .list_turns(&key, "subject-store", 10)
            .unwrap_or_else(|error| panic!("read {backend} after retry: {error}"));
        assert_eq!(
            turns.iter().map(|turn| turn.sequence).collect::<Vec<_>>(),
            vec![1, 2],
            "backend={backend}"
        );
        let turn_ids = turns
            .iter()
            .map(|turn| turn.turn_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            turn_ids,
            std::collections::BTreeSet::from(["turn-first", "turn-second"]),
            "backend={backend}"
        );
        let transcript_events = first_platform
            .read_events()
            .expect("read transcript events")
            .into_iter()
            .filter(|event| {
                event.kind_name == "memory.write" && event.plane == "conversation_transcript"
            })
            .collect::<Vec<_>>();
        assert_eq!(transcript_events.len(), 2, "backend={backend}");
    }
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_reopens_with_transcript_catalog_timeline_search_and_activity() {
    let root = temp_root("query-sqlite-reopen");
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile()).unwrap();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let governance = governance_context_digest("store-contract-query-sqlite");
    {
        let platform = support::open_store(config.clone()).unwrap();
        let store = platform.conversation_transcript_store();
        for sequence in 1..=9 {
            let mut record = transcript_record(
                &key,
                &format!("turn-sqlite-{sequence:02}"),
                &format!("sqlite 持久检索 cobalt {sequence}"),
            );
            record.sequence = sequence;
            record.created_at = 1_800_200_000 + sequence;
            record.updated_at = 1_800_200_000 + sequence;
            for message in record.input_messages.iter_mut().chain(
                record
                    .assistant_messages
                    .iter_mut()
                    .map(|item| &mut item.message),
            ) {
                message.observed_at = 1_800_200_000 + sequence;
                message.created_at = 1_800_200_000 + sequence;
            }
            store.append_turn(&record).unwrap();
        }
    }

    let reopened = support::open_store(config).unwrap();
    let store = reopened.conversation_transcript_store();
    let catalog = store
        .list_conversation_catalog(
            "subject-store",
            &TranscriptCatalogQuery {
                memory_space_id: "space-store".to_string(),
                governance_context_digest: governance.clone(),
                channel_id: None,
                lifecycle: TranscriptCatalogLifecycle::ActiveOnly,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(catalog.heads.len(), 1);
    assert_eq!(catalog.heads[0].turn_count, 9);
    let timeline = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(timeline.turns.len(), 8);
    assert_eq!(timeline.turns[0].sequence, 2);
    let search = store
        .search_transcript(
            "subject-store",
            &TranscriptSearchQuery {
                scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
                governance_context_digest: governance,
                query: TranscriptSearchNormalizerV1::normalize("sqlite 持久检索").unwrap(),
                sort: TranscriptSearchSort::RelevanceThenObservedAt,
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(search.candidates.len(), 8);
    let activity = store
        .query_transcript_activity(
            "subject-store",
            &TranscriptActivityQuery {
                key,
                ranges: vec![TranscriptUtcRange::new(1_800_200_000, 1_800_200_100).unwrap()],
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
            },
        )
        .unwrap();
    assert_eq!(activity.buckets[0].candidates.len(), 18);
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_unsupported_schemas_without_mutation() {
    for (schema_id, version) in [
        ("beetle_memory_store_schema_v11", 11),
        ("beetle_memory_store_schema_v13", 13),
        ("beetle_memory_store_schema_v16", 16),
        ("unknown_memory_store_schema_v999", 999),
    ] {
        let root = temp_root(&format!("query-sqlite-v{version}-clean-break"));
        let path = root.join("memory.sqlite3");
        let config =
            StoreBackendConfig::sqlite(&path, support::native_persistent_profile()).unwrap();
        {
            let platform = support::open_store(config.clone()).unwrap();
            let key =
                ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
            platform
                .conversation_transcript_store()
                .append_turn(&transcript_record(&key, "turn-old", "preserved state"))
                .unwrap();
        }
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            let raw: String = connection
                .query_row("SELECT manifest_json FROM bm_schema", [], |row| row.get(0))
                .unwrap();
            let mut manifest: serde_json::Value = serde_json::from_str(&raw).unwrap();
            manifest["schema_id"] = json!(schema_id);
            manifest["schema_version"] = json!(version);
            connection.execute("DELETE FROM bm_schema", []).unwrap();
            connection
            .execute(
                "INSERT INTO bm_schema(schema_id, schema_version, manifest_json) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    schema_id,
                    version,
                    serde_json::to_string(&manifest).unwrap()
                ],
            )
            .unwrap();
        }
        let before = file_tree_bytes(&root);
        let error = match support::open_store(config) {
            Ok(_) => panic!("v17 clean break must reject SQLite schema {schema_id}"),
            Err(error) => error,
        };
        assert_eq!(error.stage(), "store_rebuild_required", "{schema_id}");
        assert_eq!(file_tree_bytes(&root), before, "{schema_id}");
    }
}

#[test]
fn file_store_rejects_unsupported_schemas_without_mutation() {
    for (schema_id, version) in [
        ("beetle_memory_store_schema_v11", 11),
        ("beetle_memory_store_schema_v13", 13),
        ("beetle_memory_store_schema_v16", 16),
        ("unknown_memory_store_schema_v999", 999),
    ] {
        let root = temp_root(&format!("query-file-v{version}-clean-break"));
        let config = StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap();
        {
            let platform = support::open_store(config.clone()).unwrap();
            let key =
                ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
            platform
                .conversation_transcript_store()
                .append_turn(&transcript_record(&key, "turn-v11", "preserved v11 state"))
                .unwrap();
        }
        rewrite_file_manifest_schema(&root, schema_id, version);
        let before = file_tree_bytes(&root);
        let error = match support::open_store(config) {
            Ok(_) => panic!("v17 clean break must reject File schema {schema_id}"),
            Err(error) => error,
        };
        assert_eq!(error.stage(), "store_rebuild_required", "{schema_id}");
        assert_eq!(file_tree_bytes(&root), before, "{schema_id}");
    }
}

#[test]
fn file_store_open_fails_closed_when_transcript_query_closure_is_missing() {
    let root = temp_root("query-file-open-closure");
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    {
        let platform = support::open_store(config.clone()).unwrap();
        platform
            .conversation_transcript_store()
            .append_turn(&transcript_record(&key, "turn-closure", "closure evidence"))
            .unwrap();
    }
    std::fs::remove_dir_all(
        root.join("kv")
            .join("conversation_transcript_search_message_manifests"),
    )
    .unwrap();
    let error = match support::open_store(config) {
        Ok(_) => panic!("missing CTQ owner closure must fail open"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("transcript message has no search manifest"));
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_open_fails_closed_when_transcript_query_closure_is_missing() {
    let root = temp_root("query-sqlite-open-closure");
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile()).unwrap();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    {
        let platform = support::open_store(config.clone()).unwrap();
        platform
            .conversation_transcript_store()
            .append_turn(&transcript_record(&key, "turn-closure", "closure evidence"))
            .unwrap();
    }
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "DELETE FROM bm_kv WHERE namespace = 'conversation_transcript_search_message_manifests'",
                [],
            )
            .unwrap();
    }
    let error = match support::open_store(config) {
        Ok(_) => panic!("missing CTQ owner closure must fail open"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("transcript message has no search manifest"));
}

#[test]
fn transcript_lifecycle_exact_zeroes_catalog_timeline_search_and_activity_indexes() {
    for transition in [
        TranscriptLifecycleTransition::Mask,
        TranscriptLifecycleTransition::DeleteRaw,
    ] {
        let platform = support::open_store_in_memory(
            StoreBackendConfig::in_memory(
                ProfileId::native_dev_full().expect("native dev-full profile"),
            )
            .unwrap(),
        )
        .unwrap();
        let store = platform.conversation_transcript_store();
        let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
        let mut record = transcript_record(&key, "turn-private", "private cobalt 私密消息");
        record.created_at = 1_800_300_001;
        record.updated_at = 1_800_300_001;
        for message in record.input_messages.iter_mut().chain(
            record
                .assistant_messages
                .iter_mut()
                .map(|item| &mut item.message),
        ) {
            message.observed_at = 1_800_300_001;
            message.created_at = 1_800_300_001;
        }
        store.append_turn(&record).unwrap();
        store
            .apply_lifecycle_request(
                "subject-store",
                &TranscriptLifecycleRequest {
                    key: key.clone(),
                    turn_id: Some(record.turn_id.clone()),
                    transition,
                    reason: "privacy contract".to_string(),
                    requested_by: "owner".to_string(),
                    requested_at: 1_800_300_010,
                },
            )
            .unwrap();
        let governance = governance_context_digest("store-contract-lifecycle-zero");
        let catalog = store
            .list_conversation_catalog(
                "subject-store",
                &TranscriptCatalogQuery {
                    memory_space_id: "space-store".to_string(),
                    governance_context_digest: governance.clone(),
                    channel_id: None,
                    lifecycle: TranscriptCatalogLifecycle::ActiveAndArchived,
                    limit: 8,
                    cursor: None,
                },
            )
            .unwrap();
        assert!(catalog.heads.is_empty());
        let timeline = store
            .query_transcript_timeline(
                "subject-store",
                &TranscriptTimelineQuery {
                    key: key.clone(),
                    governance_context_digest: governance.clone(),
                    anchor: TranscriptTimelineAnchor::Latest,
                    limit: 8,
                    cursor: None,
                },
            )
            .unwrap();
        assert!(timeline.turns.is_empty());
        let search = store
            .search_transcript(
                "subject-store",
                &TranscriptSearchQuery {
                    scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
                    governance_context_digest: governance,
                    query: TranscriptSearchNormalizerV1::normalize("cobalt").unwrap(),
                    sort: TranscriptSearchSort::RelevanceThenObservedAt,
                    lifecycle: TranscriptSearchLifecycle::ActiveAndArchived,
                    limit: 8,
                    cursor: None,
                },
            )
            .unwrap();
        assert!(search.candidates.is_empty());
        let activity = store
            .query_transcript_activity(
                "subject-store",
                &TranscriptActivityQuery {
                    key: key.clone(),
                    ranges: vec![TranscriptUtcRange::new(1_800_300_000, 1_800_300_100).unwrap()],
                    lifecycle: TranscriptSearchLifecycle::ActiveAndArchived,
                },
            )
            .unwrap();
        assert!(activity.buckets[0].candidates.is_empty());
    }
}

#[test]
fn transcript_query_continuation_pages_are_lossless_beyond_128_entries() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    for sequence in 1..=129 {
        let mut record = transcript_record(
            &key,
            &format!("turn-continuation-{sequence:03}"),
            &format!("cobalt continuation evidence {sequence}"),
        );
        record.sequence = sequence;
        record.created_at = 1_800_400_000 + sequence;
        record.updated_at = 1_800_400_000 + sequence;
        for message in record.input_messages.iter_mut().chain(
            record
                .assistant_messages
                .iter_mut()
                .map(|item| &mut item.message),
        ) {
            message.observed_at = 1_800_400_000 + sequence;
            message.created_at = 1_800_400_000 + sequence;
        }
        store.append_turn(&record).unwrap();
    }
    let governance = governance_context_digest("store-contract-continuation");
    let mut timeline_cursor = None;
    let mut timeline_sequences = std::collections::BTreeSet::new();
    loop {
        let page = store
            .query_transcript_timeline(
                "subject-store",
                &TranscriptTimelineQuery {
                    key: key.clone(),
                    governance_context_digest: governance.clone(),
                    anchor: TranscriptTimelineAnchor::Latest,
                    limit: 64,
                    cursor: timeline_cursor,
                },
            )
            .unwrap();
        timeline_sequences.extend(page.turns.iter().map(|turn| turn.sequence));
        if !page.has_older {
            break;
        }
        timeline_cursor = page.older_cursor;
    }
    assert_eq!(timeline_sequences.len(), 129);
    assert_eq!(timeline_sequences.first().copied(), Some(1));
    assert_eq!(timeline_sequences.last().copied(), Some(129));

    let mut search_query = TranscriptSearchQuery {
        scope: TranscriptSearchScope::ExactConversation { key: key.clone() },
        governance_context_digest: governance,
        query: TranscriptSearchNormalizerV1::normalize("cobalt").unwrap(),
        sort: TranscriptSearchSort::ObservedAtDescending,
        lifecycle: TranscriptSearchLifecycle::ActiveOnly,
        limit: 64,
        cursor: None,
    };
    let mut search_turns = std::collections::BTreeSet::new();
    loop {
        let page = store
            .search_transcript("subject-store", &search_query)
            .unwrap();
        search_turns.extend(
            page.candidates
                .iter()
                .map(|candidate| candidate.record.turn_id.clone()),
        );
        if !page.has_more {
            break;
        }
        search_query.cursor = page.next_cursor;
    }
    assert_eq!(search_turns.len(), 129);
    let activity = store
        .query_transcript_activity(
            "subject-store",
            &TranscriptActivityQuery {
                key,
                ranges: vec![TranscriptUtcRange::new(1_800_400_000, 1_800_401_000).unwrap()],
                lifecycle: TranscriptSearchLifecycle::ActiveOnly,
            },
        )
        .unwrap();
    assert_eq!(activity.buckets[0].candidates.len(), 258);
}

#[test]
fn transcript_catalog_continuation_is_lossless_for_129_conversations() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    for index in 1..=129 {
        let conversation_id = format!("conversation-catalog-{index:03}");
        let key = ConversationKey::new("space-store", "llm.gateway", &conversation_id).unwrap();
        let mut input = delta(&format!("turn-catalog-{index:03}"), "catalog evidence");
        input.conversation.chat_id = conversation_id.clone();
        input.conversation.conversation_id = Some(conversation_id);
        let record =
            TranscriptTurnRecord::from_delta(&key, 1, &input, Vec::new(), 1_800_500_000 + index)
                .unwrap();
        store.append_turn(&record).unwrap();
    }
    let mut query = TranscriptCatalogQuery {
        memory_space_id: "space-store".to_string(),
        governance_context_digest: governance_context_digest("store-contract-catalog-pages"),
        channel_id: Some("llm.gateway".to_string()),
        lifecycle: TranscriptCatalogLifecycle::ActiveOnly,
        limit: 64,
        cursor: None,
    };
    let mut conversations = Vec::new();
    loop {
        let page = store
            .list_conversation_catalog("subject-store", &query)
            .unwrap();
        conversations.extend(
            page.heads
                .iter()
                .map(|head| (head.updated_at, head.key.conversation_id.clone())),
        );
        if !page.has_more {
            break;
        }
        query.cursor = page.next_cursor;
    }
    assert_eq!(conversations.len(), 129);
    assert_eq!(
        conversations
            .iter()
            .map(|(_, conversation)| conversation)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        129
    );
    assert!(conversations.windows(2).all(|pair| pair[0] > pair[1]));
}

#[test]
fn timeline_cursor_survives_append_but_stales_on_lifecycle_and_hides_gap_flags() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    for sequence in 1..=9 {
        let mut record = transcript_record(
            &key,
            &format!("turn-snapshot-{sequence:02}"),
            &format!("snapshot {sequence}"),
        );
        record.sequence = sequence;
        store.append_turn(&record).unwrap();
    }
    let governance = governance_context_digest("store-contract-timeline-snapshot");
    let first = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: None,
            },
        )
        .unwrap();
    let old_cursor = first.older_cursor.expect("nine turns require cursor");
    let mut appended = transcript_record(&key, "turn-snapshot-10", "new tail");
    appended.sequence = 10;
    store.append_turn(&appended).unwrap();
    let historical = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key: key.clone(),
                governance_context_digest: governance.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: Some(old_cursor.clone()),
            },
        )
        .unwrap();
    assert_eq!(historical.turns.len(), 1);
    assert_eq!(historical.turns[0].sequence, 1);
    assert!(historical.turns.iter().all(|turn| turn.sequence <= 9));

    store
        .apply_lifecycle_request(
            "subject-store",
            &TranscriptLifecycleRequest {
                key: key.clone(),
                turn_id: Some("turn-snapshot-01".to_string()),
                transition: TranscriptLifecycleTransition::Mask,
                reason: "invalidate historical cursor".to_string(),
                requested_by: "owner".to_string(),
                requested_at: 20,
            },
        )
        .unwrap();
    let stale = store.query_transcript_timeline(
        "subject-store",
        &TranscriptTimelineQuery {
            key: key.clone(),
            governance_context_digest: governance.clone(),
            anchor: TranscriptTimelineAnchor::Latest,
            limit: 8,
            cursor: Some(old_cursor),
        },
    );
    assert!(stale.is_err());

    for sequence in 2..=10 {
        if sequence != 5 {
            store
                .apply_lifecycle_request(
                    "subject-store",
                    &TranscriptLifecycleRequest {
                        key: key.clone(),
                        turn_id: Some(format!("turn-snapshot-{sequence:02}")),
                        transition: TranscriptLifecycleTransition::Mask,
                        reason: "privacy gap".to_string(),
                        requested_by: "owner".to_string(),
                        requested_at: 20 + sequence,
                    },
                )
                .unwrap();
        }
    }
    let only_visible = store
        .query_transcript_timeline(
            "subject-store",
            &TranscriptTimelineQuery {
                key,
                governance_context_digest: governance,
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 1,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(only_visible.turns.len(), 1);
    assert_eq!(only_visible.turns[0].sequence, 5);
    assert!(!only_visible.has_older);
    assert!(!only_visible.has_newer);
    assert!(only_visible.older_cursor.is_none());
    assert!(only_visible.newer_cursor.is_none());
}
