mod support;

use bm_core::platform::Platform as _;
use bm_sdk::{
    CanonicalTurnDelta, ConversationKey, ConversationScope, DerivedMemoryPlane, DerivedMemoryRef,
    HostOpaqueRef, HostRefRelation, HostRefVisibility, MemoryEvidenceAuthority,
    MemoryProjectionRequest, MemoryTranscriptCommitRequest, MemoryTranscriptLifecycleRequest,
    MemoryTranscriptRepairRequest, MemoryTranscriptReplayRequest, MemoryTurnDeliveryStatus,
    MemoryTurnProtocol, MemoryTurnSource, PressureLevel, ProfileId, RuntimeLifecycleModeInput,
    TranscriptEvidenceRef, TranscriptInputMessage, TranscriptLifecycleTransition,
    TranscriptRedactionReason, TranscriptReplayView,
};

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

#[test]
fn projection_render_limit_does_not_cut_source_recall() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let projection = runtime
        .project(MemoryProjectionRequest {
            user_query: "release artifact safety".to_string(),
            system_max_len: 64,
            recent_messages_limit: 1,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(projection.system_memory_block.len() <= 64);
    assert!(
        projection
            .context
            .long_term_memory_text
            .as_deref()
            .unwrap_or_default()
            .contains("Verify release artifacts before publishing."),
        "source assembly must still recall memory when render budget is tiny"
    );
}

#[test]
fn runtime_exposes_compiled_budget_report() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxMemoryGateway);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxMemoryGateway);
    let budget = runtime.runtime_budget();

    assert_eq!(budget.profile, ProfileId::ServerLinuxMemoryGateway);
    assert!(budget.projection_source_budget.context_assembly_max_chars > 0);
    assert!(budget.projection_render_budget.system_block_max_chars > 0);
    assert!(budget.adapter_budget.http_body_max_bytes > 0);
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
    assert!(compact_budget.redaction_items_per_page > 0);
    assert!(compact_budget.derived_refs_per_report > 0);
    assert!(compact_budget.repair_issues_per_report > 0);
    assert!(compact_budget.transcript_page_size < server_budget.transcript_page_size);
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
    let store = platform.conversation_transcript_store();
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
fn projection_exposes_runtime_awareness_without_archive_backend_trace() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    platform
        .memory_store()
        .write_daily_note(
            "2026-05-23.md",
            "Archive note: release artifact safety passed after checklist verification.",
        )
        .expect("seed archive note");
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let projection = runtime
        .project(MemoryProjectionRequest {
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
