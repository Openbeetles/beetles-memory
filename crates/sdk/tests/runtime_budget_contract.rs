#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::{memory::MemoryStore as _, platform::Platform as _};
use bm_sdk::{
    compile_runtime_budget, CanonicalTurnDelta, ConversationKey, ConversationScope,
    DerivedMemoryPlane, DerivedMemoryRef, HostOpaqueRef, HostRefRelation, HostRefVisibility,
    MemoryEvidenceAuthority, MemoryProjectionRequest, MemoryTranscriptAttrWriteRequest,
    MemoryTranscriptCommitRequest, MemoryTranscriptLifecycleRequest, MemoryTranscriptRepairRequest,
    MemoryTranscriptReplayRequest, MemoryTurnDeliveryStatus, MemoryTurnProtocol, MemoryTurnSource,
    PressureLevel, ProfileId, ProviderModelContextLimit, RuntimeBudgetInput,
    RuntimeLifecycleModeInput, TranscriptAttrEnvelope, TranscriptAttrGovernance,
    TranscriptAttrLink, TranscriptAttrRedactionPolicy, TranscriptAttrScope, TranscriptAttrSource,
    TranscriptAttrSourceKind, TranscriptAttrTarget, TranscriptAttrValueKind, TranscriptEvidenceRef,
    TranscriptInputMessage, TranscriptLifecycleTransition, TranscriptRedactionReason,
    TranscriptReplayView,
};
use serde_json::json;

use support::{
    empty_store_platform, seeded_store_platform, test_runtime, test_runtime_with_scope_and_subject,
    test_runtime_with_scope_subject_and_budget,
};

fn transcript_budget_turn(turn_id: &str) -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: turn_id.to_string(),
        conversation: ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "budget-chat".to_string(),
            conversation_id: Some("budget-conversation".to_string()),
        },
        subject: "subject-budget".to_string(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: bm_sdk::IngressKind::User,
            channel: "llm.gateway".to_string(),
            provider: Some("sdk-test".to_string()),
            protocol: MemoryTurnProtocol::OllamaChat,
            endpoint: Some("/api/chat".to_string()),
            model_alias: Some("qwen".to_string()),
            model_resolved: Some("qwen3".to_string()),
            request_id: Some(format!("req-{turn_id}")),
            client_conversation_hint: Some("budget-window".to_string()),
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user(format!(
            "budget user {turn_id}"
        ))],
        assistant_message: Some(TranscriptInputMessage::assistant(format!(
            "budget assistant {turn_id}"
        ))),
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    }
}

fn transcript_budget_attr(
    key: ConversationKey,
    turn_id: &str,
    message_id: &str,
    index: usize,
) -> TranscriptAttrEnvelope {
    TranscriptAttrEnvelope {
        attr_id: format!("budget-attr-{index}"),
        target: TranscriptAttrTarget {
            key,
            scope: TranscriptAttrScope::Message,
            turn_id: turn_id.to_string(),
            message_id: Some(message_id.to_string()),
        },
        key: format!("host.budget.model_usage_{index}"),
        value_kind: TranscriptAttrValueKind::JsonObject,
        schema_ref: Some("budget.model-usage.v1".to_string()),
        value: json!({
            "input_tokens": 10 + index,
            "output_tokens": 2,
            "usage_source": "provider_reported"
        }),
        visibility: HostRefVisibility::HostUi,
        source: TranscriptAttrSource {
            writer: "budget-test".to_string(),
            source_kind: TranscriptAttrSourceKind::ProviderReported,
            written_at: 1_800_000_000 + index as u64,
            audit_reason: "budget attr test".to_string(),
        },
        governance: TranscriptAttrGovernance {
            max_value_bytes: 4096,
            redaction_policy: TranscriptAttrRedactionPolicy::MetadataSurvivesMask,
            export_allowed: false,
        },
        links: vec![TranscriptAttrLink {
            relation: "model_invocation".to_string(),
            ref_kind: "model_invocation_id".to_string(),
            ref_id: format!("budget-model-{index}"),
        }],
        created_at: 1_800_000_000 + index as u64,
        updated_at: 1_800_000_000 + index as u64,
    }
}

#[test]
fn projection_render_limit_does_not_cut_source_recall() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "release artifact safety".to_string(),
            system_max_len: 64,
            recent_messages_limit: 1,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(projection.system_memory_block.len() <= 64);
    assert!(projection.context.long_term_memory_text.is_none());
    assert!(!projection
        .recall_delivery_report
        .selected_candidate_ids
        .is_empty());
    assert!(projection
        .recall_delivery_report
        .rendered_capsules
        .is_empty());
}

#[test]
fn runtime_exposes_compiled_budget_report() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxMemoryGateway);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxMemoryGateway);
    let budget = runtime.runtime_budget();

    assert_eq!(budget.profile, ProfileId::ServerLinuxMemoryGateway);
    assert!(budget.projection_source_budget.context_assembly_max_chars > 0);
    assert!(budget.projection_render_budget.system_block_max_chars > 0);
    assert!(budget.facet_recall_budget.max_query_facets > 0);
    assert!(budget.facet_recall_budget.max_facet_index_docs_read > 0);
    assert!(budget.facet_recall_budget.max_facet_anchor_candidates > 0);
    assert!(budget.facet_recall_budget.max_facet_expanded_candidates > 0);
    assert!(budget.adapter_budget.http_body_max_bytes > 0);
}

#[test]
fn graph_expansion_budget_is_profile_owned_and_not_provider_render_owned() {
    let esp_embedded = bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::EspEmbeddedSdk)
        .graph_expansion_budget;
    let esp_standalone =
        bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::EspStandaloneMemory)
            .graph_expansion_budget;
    let linux_device =
        bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::LinuxDeviceStandaloneMemory)
            .graph_expansion_budget;
    let server_gateway =
        bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::ServerLinuxMemoryGateway)
            .graph_expansion_budget;
    let dev_full = bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::ServerLinuxDevFull)
        .graph_expansion_budget;

    assert_eq!(esp_embedded.max_hops, 1);
    assert_eq!(esp_standalone.max_hops, 1);
    assert_eq!(linux_device.max_hops, 1);
    assert!(!esp_embedded.default_recall_multi_hop_allowed);
    assert!(!esp_standalone.default_recall_multi_hop_allowed);
    assert!(!linux_device.default_recall_multi_hop_allowed);

    assert_eq!(server_gateway.max_hops, 2);
    assert_eq!(dev_full.max_hops, 2);
    assert!(server_gateway.eval_recall_multi_hop_allowed);
    assert!(dev_full.eval_recall_multi_hop_allowed);
    assert!(esp_embedded.max_expanded_candidates < server_gateway.max_expanded_candidates);
    assert!(linux_device.max_graph_nodes_loaded < dev_full.max_graph_nodes_loaded);

    let mut provider_limited_input =
        RuntimeBudgetInput::static_for_profile(ProfileId::ServerLinuxDevFull);
    provider_limited_input.provider_model_context_limit = Some(ProviderModelContextLimit {
        provider: Some("local".to_string()),
        model: Some("tiny-render".to_string()),
        max_context_tokens: None,
        max_prompt_chars: Some(512),
    });
    let provider_limited = compile_runtime_budget(provider_limited_input);

    assert_eq!(provider_limited.graph_expansion_budget, dev_full);
    assert_eq!(
        provider_limited
            .projection_render_budget
            .system_block_max_chars,
        512
    );
}

#[test]
fn facet_recall_budget_is_profile_owned_and_not_graph_or_render_owned() {
    let esp_embedded = bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::EspEmbeddedSdk)
        .facet_recall_budget;
    let linux_device =
        bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::LinuxDeviceStandaloneMemory)
            .facet_recall_budget;
    let server_gateway =
        bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::ServerLinuxMemoryGateway)
            .facet_recall_budget;
    let dev_full = bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::ServerLinuxDevFull)
        .facet_recall_budget;

    assert!(esp_embedded.max_query_facets > 0);
    assert!(esp_embedded.max_facet_index_docs_read > 0);
    assert!(esp_embedded.max_facet_anchor_candidates > 0);
    assert!(esp_embedded.max_facet_expanded_candidates > 0);
    assert!(esp_embedded.max_facet_index_docs_read < server_gateway.max_facet_index_docs_read);
    assert!(linux_device.max_facet_index_docs_read < dev_full.max_facet_index_docs_read);
    assert!(linux_device.max_facet_anchor_candidates <= dev_full.max_facet_anchor_candidates);
    assert!(linux_device.max_facet_expanded_candidates <= dev_full.max_facet_expanded_candidates);

    let mut provider_limited_input =
        RuntimeBudgetInput::static_for_profile(ProfileId::ServerLinuxDevFull);
    provider_limited_input.provider_model_context_limit = Some(ProviderModelContextLimit {
        provider: Some("local".to_string()),
        model: Some("tiny-render".to_string()),
        max_context_tokens: None,
        max_prompt_chars: Some(512),
    });
    let provider_limited = compile_runtime_budget(provider_limited_input);

    assert_eq!(provider_limited.facet_recall_budget, dev_full);
    assert_eq!(
        provider_limited
            .projection_render_budget
            .system_block_max_chars,
        512
    );
}

#[test]
fn recall_delivery_budget_is_profile_owned_and_not_provider_or_graph_owned() {
    let compact = bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::EspEmbeddedSdk)
        .recall_delivery_budget;
    let device =
        bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::LinuxDeviceStandaloneMemory)
            .recall_delivery_budget;
    let full = bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::ServerLinuxDevFull)
        .recall_delivery_budget;
    let full_graph = bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::ServerLinuxDevFull)
        .graph_expansion_budget;

    assert!(compact.max_selected_candidates > 0);
    assert!(compact.max_rendered_capsules > 0);
    assert!(compact.max_capsule_chars > 0);
    assert!(compact.max_loss_ledger_entries > 0);
    assert!(compact.max_selected_candidates < full.max_selected_candidates);
    assert!(device.max_rendered_capsules < full.max_rendered_capsules);
    assert_ne!(full.max_selected_candidates, full_graph.max_seed_candidates);

    let mut provider_limited_input =
        RuntimeBudgetInput::static_for_profile(ProfileId::ServerLinuxDevFull);
    provider_limited_input.provider_model_context_limit = Some(ProviderModelContextLimit {
        provider: Some("local".to_string()),
        model: Some("tiny-render".to_string()),
        max_context_tokens: None,
        max_prompt_chars: Some(512),
    });
    let provider_limited = compile_runtime_budget(provider_limited_input);

    assert_eq!(provider_limited.recall_delivery_budget, full);
    assert_eq!(
        provider_limited
            .projection_render_budget
            .system_block_max_chars,
        512
    );
}

#[test]
fn transcript_governance_budget_is_profile_owned_and_runtime_enforced() {
    let compact_budget = bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::EspEmbeddedSdk)
        .transcript_governance_budget;
    let server_budget =
        bm_sdk::RuntimeBudgetReport::static_for_profile(ProfileId::ServerLinuxDevFull)
            .transcript_governance_budget;

    assert!(compact_budget.transcript_page_size > 0);
    assert!(compact_budget.host_refs_per_turn > 0);
    assert!(compact_budget.max_attrs_per_turn > 0);
    assert!(compact_budget.max_attrs_per_message > 0);
    assert!(compact_budget.redaction_items_per_page > 0);
    assert!(compact_budget.derived_refs_per_report > 0);
    assert!(compact_budget.repair_issues_per_report > 0);
    assert!(compact_budget.transcript_page_size < server_budget.transcript_page_size);
    assert!(compact_budget.max_attrs_per_turn < server_budget.max_attrs_per_turn);
    assert!(compact_budget.max_attrs_per_message < server_budget.max_attrs_per_message);
    assert!(compact_budget.derived_refs_per_report < server_budget.derived_refs_per_report);

    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "budget-chat",
        "subject-budget",
    );
    let page_size = runtime
        .runtime_budget()
        .transcript_governance_budget
        .transcript_page_size;
    let turn_count = page_size.saturating_add(2);
    for index in 0..turn_count {
        runtime
            .commit_transcript(MemoryTranscriptCommitRequest {
                turn: transcript_budget_turn(&format!("turn-{index:03}")),
                host_refs: Vec::new(),
            })
            .expect("commit transcript");
    }

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "budget-conversation".to_string(),
            limit: turn_count.saturating_mul(2),
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .expect("budgeted replay");

    assert_eq!(replay.slice.turns.len(), page_size);
    assert!(replay.has_more);
}

#[test]
fn transcript_report_budgets_limit_derived_refs_and_repair_issues() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut runtime_budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    runtime_budget
        .transcript_governance_budget
        .derived_refs_per_report = 1;
    runtime_budget
        .transcript_governance_budget
        .repair_issues_per_report = 1;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform.clone(),
        profile,
        "llm.gateway",
        "budget-chat",
        "subject-budget",
        runtime_budget,
    );
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: transcript_budget_turn("turn-derived"),
            host_refs: Vec::new(),
        })
        .expect("commit transcript");

    let key = ConversationKey::new(
        runtime.memory_space_id().to_string(),
        "llm.gateway".to_string(),
        "budget-conversation".to_string(),
    )
    .expect("conversation key");
    let store = platform.replay_harness().conversation_transcript_store();
    for index in 0..2 {
        store
            .append_derived_memory_ref(
                &key,
                &DerivedMemoryRef {
                    plane: DerivedMemoryPlane::LongTerm,
                    store_key: format!("long_term:budget:{index}"),
                    subject_id: Some("subject-budget".to_string()),
                    source: TranscriptEvidenceRef {
                        memory_space_id: runtime.memory_space_id().to_string(),
                        channel_id: "llm.gateway".to_string(),
                        conversation_id: "budget-conversation".to_string(),
                        turn_id: "turn-derived".to_string(),
                        message_id: None,
                        subject_id: Some("subject-budget".to_string()),
                        authority: Some(MemoryEvidenceAuthority::UserAsserted),
                    },
                    created_at: 1_800_000_000,
                },
            )
            .expect("append derived ref");
    }
    for index in 0..2 {
        store
            .append_derived_memory_ref(
                &key,
                &DerivedMemoryRef {
                    plane: DerivedMemoryPlane::ProceduralSkill,
                    store_key: format!("runtime_skill:missing:{index}"),
                    subject_id: Some("subject-budget".to_string()),
                    source: TranscriptEvidenceRef {
                        memory_space_id: runtime.memory_space_id().to_string(),
                        channel_id: "llm.gateway".to_string(),
                        conversation_id: "budget-conversation".to_string(),
                        turn_id: format!("missing-turn-{index}"),
                        message_id: Some(format!("missing-message-{index}")),
                        subject_id: Some("subject-budget".to_string()),
                        authority: Some(MemoryEvidenceAuthority::RuntimeObservation),
                    },
                    created_at: 1_800_000_000,
                },
            )
            .expect("append missing derived ref");
    }

    let lifecycle = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "budget-conversation".to_string(),
            turn_id: Some("turn-derived".to_string()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "budgeted lifecycle report".to_string(),
        })
        .expect("lifecycle report");
    assert_eq!(lifecycle.transcript.derived_memory_refs.len(), 1);
    assert!(lifecycle.transcript.profile_budget_applied);

    let repair = runtime
        .repair_transcript(MemoryTranscriptRepairRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "budget-conversation".to_string(),
        })
        .expect("repair report");
    assert_eq!(repair.transcript.checked_derived_refs, 4);
    assert_eq!(repair.transcript.issues.len(), 1);
    assert!(repair.transcript.profile_budget_applied);
    assert!(repair.transcript.fail_closed);
}

#[test]
fn transcript_replay_budget_limits_visible_host_refs_and_redaction_items() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut runtime_budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    runtime_budget
        .transcript_governance_budget
        .host_refs_per_turn = 1;
    runtime_budget
        .transcript_governance_budget
        .redaction_items_per_page = 1;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "budget-chat",
        "subject-budget",
        runtime_budget,
    );
    let host_refs = (0..3)
        .map(|index| HostOpaqueRef {
            host_kind: "generic-host".to_string(),
            business_ref_type: "ticket".to_string(),
            business_ref_id: format!("T-{index}"),
            relation: HostRefRelation::Related,
            visibility: HostRefVisibility::HostUi,
            label: Some(format!("ticket {index}")),
        })
        .collect::<Vec<_>>();
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: transcript_budget_turn("turn-host-ref-budget"),
            host_refs,
        })
        .expect("commit transcript");

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "budget-conversation".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .expect("replay transcript");

    assert_eq!(replay.slice.turns[0].host_refs.len(), 1);
    assert_eq!(replay.slice.redactions.len(), 1);
    assert_eq!(replay.slice.audit.redacted_host_refs, 2);
    assert!(replay
        .slice
        .audit
        .redaction_reasons
        .contains(&TranscriptRedactionReason::ProfileBudget));
}

#[test]
fn transcript_attr_budget_limits_visible_message_attrs_and_reports_redaction() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut runtime_budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    runtime_budget
        .transcript_governance_budget
        .max_attrs_per_message = 1;
    runtime_budget
        .transcript_governance_budget
        .redaction_items_per_page = 10;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "budget-chat",
        "subject-budget",
        runtime_budget,
    );
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: transcript_budget_turn("turn-attr-budget"),
            host_refs: Vec::new(),
        })
        .expect("commit transcript");
    let raw = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "budget-conversation".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .expect("raw replay");
    let turn = &raw.slice.turns[0];
    let message_id = turn
        .assistant_message
        .as_ref()
        .expect("assistant message")
        .message_id
        .clone();
    let attrs = (0..3)
        .map(|index| {
            transcript_budget_attr(raw.slice.key.clone(), &turn.turn_id, &message_id, index)
        })
        .collect::<Vec<_>>();
    runtime
        .record_transcript_attrs(MemoryTranscriptAttrWriteRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "budget-conversation".to_string(),
            attrs,
            idempotency_key: Some("budget-attr-write".to_string()),
            dry_run: false,
        })
        .expect("record attrs");

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "budget-conversation".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .expect("budgeted replay");

    let assistant = replay.slice.turns[0]
        .assistant_message
        .as_ref()
        .expect("assistant message");
    assert_eq!(assistant.attrs.len(), 1);
    assert!(replay.slice.redactions.iter().any(|redaction| {
        redaction.attr_id.as_deref() == Some("budget-attr-1")
            && redaction.attr_key.as_deref() == Some("host.budget.model_usage_1")
            && redaction.reason == TranscriptRedactionReason::AttrValueBudget
    }));
    assert!(replay
        .slice
        .audit
        .redaction_reasons
        .contains(&TranscriptRedactionReason::AttrValueBudget));
    assert!(replay
        .slice
        .audit
        .redaction_reasons
        .contains(&TranscriptRedactionReason::ProfileBudget));
}

#[test]
fn projection_exposes_runtime_awareness_without_archive_backend_trace() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    platform
        .replay_harness()
        .write_daily_note(
            "2026-05-23.md",
            "Archive note: release artifact safety passed after checklist verification.",
        )
        .expect("seed archive note");
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "release artifact safety".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Cautious,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    let block = projection.system_memory_block;
    assert!(block.contains("## Runtime Constraints"), "{block}");
    assert!(block.contains("Resource pressure: cautious"), "{block}");
    assert!(block.contains("Beetle Memory"), "{block}");
    assert!(block.contains("## Governed Memory Evidence"), "{block}");
    assert!(
        block.contains("world_snapshot [public_grounding"),
        "{block}"
    );
    assert!(block.contains("release artifact safety"), "{block}");
    for forbidden in [
        "IndexedHybrid",
        "backend=",
        "Backend names",
        "selector=",
        "selectors",
        "store paths",
        "trace counters",
        "candidate_count",
        "candidates=",
        "hits=",
        "primary quota pass",
        "model trained on IndexedHybrid",
        "Private internal layers",
    ] {
        assert!(
            !block.contains(forbidden),
            "projection leaked diagnostic term {forbidden}: {block}"
        );
    }
}
