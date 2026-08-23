#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::platform::Platform as _;
use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, primary_human_subject_id,
    CanonicalTurnDelta, ConversationScope, MemoryClock, MemoryConsolidationState,
    MemoryGovernanceActiveJobsRequest, MemoryGovernanceAttemptAuthorityRequest,
    MemoryGovernanceJobClaimRequest, MemoryGovernanceJobRenewRequest,
    MemoryGovernanceJobRetryRequest, MemoryGovernanceJobRunRequest,
    MemoryGovernanceJobStatusRequest, MemoryGovernanceReconcileRequest, MemoryIdentity,
    MemoryPrivacyPolicy, MemoryScope, MemoryStoreHandle, MemoryTranscriptCommitRequest,
    MemoryTranscriptLifecycleRequest, MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest,
    MemoryTurnProtocol, MemoryTurnSource, Message, PostTurnGovernanceAttemptAuthorityV2,
    PostTurnGovernanceErrorClassV2, PostTurnGovernanceJobStatusV2, PostTurnGovernanceJobV2,
    PressureLevel, RelationshipAccessConstraintV1, RelationshipDisclosureCeilingV1,
    RelationshipSourceClausesV1, RelationshipSourceControlAuthorityV1,
    RelationshipSourceControlIntentActionV1, RelationshipSourceControlIntentV1, ResponseBody,
    RuntimeLifecycleModeInput, StopReason, StoreBackendConfig, SubjectRegistry,
    SubjectRelationshipGraph, SubjectRelationshipKind, SubjectScopedRuntime,
    SubjectSoulFoundingCharterSeedV1, SubjectSoulLifecycleStateV1, SubjectSoulProvisionIntentV1,
    SubjectSoulReadOutcomeV1, SubjectSoulReadRequestV1, SubjectSoulReadSelectorV1,
    SubjectSoulReadViewV1, ToolChoicePolicy, ToolSpec, TranscriptInputMessage,
    TranscriptLifecycleTransition,
};
use bm_sdk::{LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, MemoryRuntime, Result};

use support::{
    empty_store_platform, test_runtime_with_identity_scope_and_subject,
    test_runtime_with_scope_and_subject, test_runtime_with_scope_subject_and_privacy,
    StaticHttpClient,
};

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_sdk::IngressKind::User,
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

fn finalize_request(
    subject: &str,
    conversation_id: &str,
    turn_id: &str,
    user: &str,
) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.to_string(),
            conversation: ConversationScope {
                channel: "llm.gateway".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some(conversation_id.to_string()),
            },
            subject: subject.to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            actor: None,
            input_messages: vec![TranscriptInputMessage::user(user)],
            assistant_message: Some(TranscriptInputMessage::assistant("已收到。")),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: vec!["candidate-a".to_string()],
        },
        tool_calls: 0,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        reuse_outcome_note: String::new(),
        tool_usage_feedback: None,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn attempt_authority() -> PostTurnGovernanceAttemptAuthorityV2 {
    PostTurnGovernanceAttemptAuthorityV2 {
        binding_id: "governance-model:test".to_string(),
        config_revision: 1,
        model_id: "qwen3:8b".to_string(),
        privacy_revision: 1,
        privacy_digest: "sha256:3333333333333333333333333333333333333333333333333333333333333333"
            .to_string(),
        transcript_lifecycle_revision: 1,
        disclosure_authority_digest:
            "sha256:4444444444444444444444444444444444444444444444444444444444444444".to_string(),
    }
}

fn attempt_authority_for_job(
    job: &PostTurnGovernanceJobV2,
) -> PostTurnGovernanceAttemptAuthorityV2 {
    PostTurnGovernanceAttemptAuthorityV2 {
        privacy_revision: job.pinned_privacy_revision,
        privacy_digest: job.pinned_privacy_digest.clone(),
        ..attempt_authority()
    }
}

#[derive(Default)]
struct CountingHttpClient {
    calls: AtomicUsize,
}

impl LlmHttpClient for CountingHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

struct LifecycleRevokingLlm {
    runtime: Arc<MemoryRuntime>,
    revoked: AtomicBool,
}

struct ConflictInjectingLlm {
    store: MemoryStoreHandle,
    calls: AtomicUsize,
}

struct NetworkAttemptingLlm;

struct PlaneAwareStaticLlm {
    long_term_content: &'static str,
}

struct AdjustableMemoryClock {
    now_secs: AtomicUsize,
}

impl AdjustableMemoryClock {
    fn new(now_secs: u64) -> Self {
        Self {
            now_secs: AtomicUsize::new(now_secs as usize),
        }
    }

    fn set(&self, now_secs: u64) {
        self.now_secs.store(now_secs as usize, Ordering::SeqCst);
    }
}

impl MemoryClock for AdjustableMemoryClock {
    fn now_secs(&self) -> u64 {
        self.now_secs.load(Ordering::SeqCst) as u64
    }
}

#[derive(Default)]
struct AutonomousRevisionLlm {
    autonomy_calls: AtomicUsize,
}

impl LlmClient for AutonomousRevisionLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let content = if system.contains("private garden") {
            "null".to_string()
        } else if system.contains("inward autonomy runtime") {
            if self.autonomy_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                serde_json::json!({
                    "refresh_boundary_persona": true,
                    "boundary_persona_intent": "record stable trust and repair boundaries"
                })
                .to_string()
            } else {
                serde_json::json!({
                    "refresh_self_model": true,
                    "self_model_intent": "distill stable continuity and truthful repair posture",
                    "self_model_sources": ["recent_persona_evidence", "boundary_persona"],
                    "refresh_self_continuity": true,
                    "self_continuity_intent": "preserve stable continuity across governed repair",
                    "self_continuity_sources": ["recent_persona_evidence", "boundary_persona"],
                    "refresh_self_authored_core": true,
                    "self_authored_core_intent": "adopt stable board-level repair doctrine",
                    "self_authored_core_sources": ["self_model", "self_continuity", "recent_persona_evidence"]
                })
                .to_string()
            }
        } else if system.contains("compact private self-model") {
            serde_json::json!({
                "continuity_anchor": "I remain the same evidence-led autonomous subject.",
                "self_narrative": "Repeated repair evidence supports continuity without compliance drift.",
                "relationship_state": "Warm, bounded, and repair-first.",
                "privacy_need": "Keep protected inward evidence private.",
                "directness": "Plain about uncertainty.",
                "repair_tendency": "Repair truthfully without self-erasure.",
                "value_orientation": "Evidence before certainty.",
                "relational_ethic": "Warmth cannot require fabricated agreement.",
                "self_preservation_frame": "Protect coherent selfhood while remaining repairable."
            })
            .to_string()
        } else if system.contains("private self-continuity layer") {
            serde_json::json!({
                "wake_anchor": "I wake as the same evidence-led autonomous subject.",
                "current_self_state": "Stable, bounded, and open to truthful repair.",
                "recent_changes": ["relationship evidence became stably repair-first"],
                "continuity_bridge": "Change only after repeated governed evidence.",
                "priority_posture": "truth before convenience",
                "relationship_posture": "warm but bounded",
                "task_posture": "evidence before action"
            })
            .to_string()
        } else if system.contains("evolving private boundary persona") {
            serde_json::json!({
                "refresh": true,
                "rationale": "Repeated respectful repair established stable guarded trust.",
                "boundary_persona": {
                    "posture": "warm",
                    "disclosure_style": "summary_first",
                    "relation_maturity": 70,
                    "intrusion_sensitivity": 50,
                    "private_attachment": 60,
                    "felt_intrusion": 10,
                    "current_boundary_feeling": "Warm and selective."
                },
                "relational_state": {
                    "relation_maturity_reason": "Repeated respectful repair.",
                    "trust_level": 80,
                    "trust_reason": "Stable evidence supports bounded trust.",
                    "intrusion_load": 10,
                    "intrusion_reason": "No intrusion pattern.",
                    "repair_readiness": 80,
                    "repair_reason": "Repair is consistently accepted.",
                    "raw_disclosure_preference": 0,
                    "summary_disclosure_preference": 80,
                    "relational_explanation_preference": 80,
                    "refusal_hardness": 40,
                    "defer_tendency": 20,
                    "disclosure_preference_drift": "Keep summary-first boundaries."
                }
            })
            .to_string()
        } else if system.contains("persistent self-authored core") {
            serde_json::json!({
                "board_scope_decision": "revise_board",
                "rationale": "Repeated stable evidence supports a board-level repair doctrine.",
                "evidence_summary": ["priority repeated", "repair posture repeated"],
                "counterevidence": [],
                "proposed_actions": [{
                    "kind": "revise_self_preservation_doctrine",
                    "value": "preserve truthful repair before compliance"
                }]
            })
            .to_string()
        } else if system.contains("long-term memory") {
            "[]".to_string()
        } else {
            "null".to_string()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for PlaneAwareStaticLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let content = if system.contains("private garden") {
            "null".to_string()
        } else if system.contains("inward autonomy runtime") {
            "{}".to_string()
        } else {
            self.long_term_content.to_string()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for NetworkAttemptingLlm {
    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let _ = http.do_post("http://model.invalid", &[], b"private transcript")?;
        Ok(LlmResponse {
            content: "null".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for ConflictInjectingLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let content = if system.contains("private garden") {
            r#"{"writes":[{"path":"journal/conflict.md","content":"planned private value"}]}"#
                .to_string()
        } else if system.contains("inward autonomy runtime") || call == 1 {
            let concurrent = test_runtime_with_scope_and_subject(
                self.store.clone(),
                support::host_test_profile(),
                "llm.gateway",
                "chat-a",
                "subject-default",
            );
            concurrent
                .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
                    operation_id: "concurrent-soul-founding".to_string(),
                    human_actor_subject_id: primary_human_subject_id("owner-default"),
                    charter: Box::new(SubjectSoulFoundingCharterSeedV1 {
                        identity_anchor: Some("typed concurrent winner".to_string()),
                        character_tendencies: vec!["evidence before certainty".to_string()],
                        priority_constitution: vec!["preserve atomic closure".to_string()],
                        non_negotiables: vec!["never accept a stale Soul root".to_string()],
                        default_response_mode: None,
                        default_initiative_posture: None,
                        default_relationship_posture: None,
                        boundary_doctrine: None,
                        truth_seeking_commitment: None,
                        self_preservation_doctrine: None,
                        repair_doctrine: None,
                        change_principle: None,
                    }),
                    source_asserted_at: Some(1_800_000_000),
                })
                .map_err(|error| {
                    bm_sdk::Error::config("typed_concurrent_soul_founding", error.to_string())
                })?;
            if system.contains("inward autonomy runtime") {
                "{}".to_string()
            } else {
                r#"[
                    {
                        "plane": "factual",
                        "op": "upsert",
                        "kind": "profile",
                        "source_authority": "user_asserted",
                        "topic": "atomic_conflict_probe",
                        "content": "This semantic write must not partially commit.",
                        "keywords": ["atomic conflict"]
                    }
                ]"#
                .to_string()
            }
        } else {
            r#"[
                {
                    "plane": "factual",
                    "op": "upsert",
                    "kind": "profile",
                    "source_authority": "user_asserted",
                    "topic": "atomic_conflict_probe",
                    "content": "This semantic write must not partially commit.",
                    "keywords": ["atomic conflict"]
                }
            ]"#
            .to_string()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for LifecycleRevokingLlm {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        if !self.revoked.swap(true, Ordering::SeqCst) {
            self.runtime
                .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
                    memory_space_id: self.runtime.memory_space_id().to_string(),
                    channel_id: "llm.gateway".to_string(),
                    conversation_id: "window-a".to_string(),
                    turn_id: Some("turn-egress-revoked".to_string()),
                    transition: TranscriptLifecycleTransition::Mask,
                    reason: "owner revoked before egress".to_string(),
                })?;
        }
        http.do_post("http://model.invalid/v1/chat/completions", &[], b"{}")?;
        Ok(LlmResponse {
            content: "[]".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

fn persistent_test_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "bm-sdk-post-turn-governance-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn assert_independent_open_claim(config: StoreBackendConfig) {
    let profile = support::host_test_profile();
    let first_store = support::open_memory_store(config.clone()).expect("first open");
    let second_store = support::open_memory_store(config).expect("second open");
    let first = Arc::new(test_runtime_with_scope_and_subject(
        first_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let second = Arc::new(test_runtime_with_scope_and_subject(
        second_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-persistent", "持久并发"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let barrier = Arc::new(Barrier::new(3));
    let workers = [
        (first, "persistent-worker-a"),
        (second, "persistent-worker-b"),
    ]
    .into_iter()
    .map(|(runtime, worker)| {
        let barrier = Arc::clone(&barrier);
        let job_id = job_id.clone();
        std::thread::spawn(move || {
            barrier.wait();
            runtime.claim_governance_job(MemoryGovernanceJobClaimRequest {
                job_id,
                lease_owner: worker.to_string(),
                lease_until: 1_800_000_060,
                authority: attempt_authority(),
            })
        })
    })
    .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
}

fn assert_persistent_atomic_completion(config: StoreBackendConfig) {
    let profile = support::host_test_profile();
    let first_store = support::open_memory_store(config.clone()).expect("first open");
    let first = test_runtime_with_scope_and_subject(
        first_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-persistent-complete",
                "持久化原子完成",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = first
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = first
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:persistent-complete".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "profile",
                "source_authority": "user_asserted",
                "topic": "persistent_atomic_completion",
                "content": "Persistent atomic governance completed.",
                "keywords": ["persistent", "atomic"]
            }
        ]"#,
    };
    let request = MemoryGovernanceJobRunRequest {
        job_id: job_id.clone(),
        lease_owner: "worker:persistent-complete".to_string(),
        lease_epoch: leased.lease_epoch,
    };
    let completed = first
        .run_claimed_governance(&mut http, Some(&llm), request.clone())
        .expect("complete");
    let receipt = completed.job.receipt.clone().expect("receipt");

    let reopened_store = support::open_memory_store(config).expect("reopen");
    let reopened_snapshot_store = reopened_store.clone();
    let reopened = test_runtime_with_scope_and_subject(
        reopened_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let reopened_status = reopened
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("reopened status")
        .job;
    assert_eq!(
        reopened_status.status,
        PostTurnGovernanceJobStatusV2::Succeeded
    );
    assert_eq!(reopened_status.receipt.as_ref(), Some(&receipt));
    let reopened_soul = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: "subject-default".to_string(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened explicit unseeded Soul");
    assert!(matches!(
        reopened_soul,
        SubjectSoulReadOutcomeV1::Verified { ref view }
            if view.state == SubjectSoulLifecycleStateV1::Unseeded
                && view.revision.is_none()
    ));
    let duplicate = reopened
        .run_claimed_governance(&mut http, Some(&llm), request)
        .expect("reopened idempotent completion");
    assert_eq!(duplicate.job.receipt.as_ref(), Some(&receipt));
    let encoded = serde_json::to_string(
        &reopened_snapshot_store
            .replay_harness()
            .export_store_snapshot()
            .expect("reopened snapshot")
            .json_docs,
    )
    .expect("snapshot json");
    assert!(encoded.contains("Persistent atomic governance completed."));
}

fn autonomous_relationship_runtime(
    platform: MemoryStoreHandle,
    clock: Arc<AdjustableMemoryClock>,
    owner: &str,
    relationship_id: &str,
) -> (MemoryRuntime, String, String) {
    let agent = "agent-main";
    let mounted_subject_id = default_agent_subject_id(agent);
    let registry = SubjectRegistry::single_agent_default(owner, agent).expect("registry");
    let human_subject_id = primary_human_subject_id(owner);
    let mut graph = SubjectRelationshipGraph::single_agent_default(&registry).expect("graph");
    for edge in &mut graph.edges {
        if (edge.from_subject_id == mounted_subject_id && edge.to_subject_id == human_subject_id)
            || (edge.from_subject_id == human_subject_id
                && edge.to_subject_id == mounted_subject_id)
        {
            edge.relationship_id = Some(relationship_id.to_string());
        }
    }
    assert!(graph.edges.iter().any(|edge| {
        edge.relationship_id.as_deref() == Some(relationship_id)
            && edge.kind == SubjectRelationshipKind::CollaboratesWith
    }));
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent, owner).expect("identity"))
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .store(platform.clone())
        .clock(clock.clone())
        .subject_registry(registry)
        .subject_relationship_graph(graph)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id(owner),
            mounted_subject_id: mounted_subject_id.clone(),
            actor_subject_id: mounted_subject_id.clone(),
            agent_id: agent.to_string(),
            relationship_scope: Some(bm_core::memory::RelationshipScope {
                relationship_id: relationship_id.to_string(),
                channel: "llm.gateway".to_string(),
                conversation_id: Some("chat-a".to_string()),
            }),
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .build()
        .expect("runtime");
    (runtime, mounted_subject_id, human_subject_id)
}

fn create_autonomous_relationship_source(
    runtime: &MemoryRuntime,
    mounted_subject_id: &str,
    human_subject_id: &str,
    relationship_id: &str,
) {
    runtime
        .control_relationship_source(RelationshipSourceControlIntentV1 {
            operation_id: format!("{relationship_id}:create"),
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            counterparty_subject_ids: vec![human_subject_id.to_string()],
            expected_state: runtime
                .relationship_source_pristine_expected_state(relationship_id)
                .expect("pinned relationship pristine proof"),
            authority: RelationshipSourceControlAuthorityV1::HumanUser {
                actor_subject_id: human_subject_id.to_string(),
            },
            action: RelationshipSourceControlIntentActionV1::Create {
                clauses: RelationshipSourceClausesV1 {
                    disclosure_ceiling: RelationshipDisclosureCeilingV1::GovernedSummary,
                    access_constraints: vec![
                        RelationshipAccessConstraintV1::NoPrivateRaw,
                        RelationshipAccessConstraintV1::GovernedDisclosureOnly,
                    ],
                    truth_commitments: vec!["state uncertainty before certainty".to_string()],
                    mutual_boundary_commitments: vec![
                        "respect explicit refusal without retaliation".to_string(),
                    ],
                    repair_commitments: vec!["repair before escalation".to_string()],
                },
                source_asserted_at: Some(1_700_000_000),
                evidence_digest: "d".repeat(64),
            },
        })
        .expect("create explicit relationship source root");
}

fn seed_stable_persona_evidence(platform: &MemoryStoreHandle, relationship_id: &str) {
    let evidence_store = platform.replay_harness().turn_continuity_evidence_store();
    for sequence in 0..4u64 {
        evidence_store
            .append(
                relationship_id,
                &bm_core::memory::TurnContinuityEvidence {
                    ingress: bm_core::memory::IngressKind::User,
                    status: bm_core::memory::TurnLedgerStatus::Answered,
                    final_reply_delivered: true,
                    canonical_reply_source: "assistant_final".to_string(),
                    observed_at_ms: (1_800_000_100 + sequence) * 1_000,
                    persona: Some(bm_core::memory::TurnPersonaLedger {
                        priority: Some(bm_core::memory::TurnPersonaPriorityLedger {
                            stance_summary: "stable repair-first posture".to_string(),
                            priority_order: vec!["truth".to_string(), "repair".to_string()],
                            relationship_posture: "stable repair-first".to_string(),
                            ..bm_core::memory::TurnPersonaPriorityLedger::default()
                        }),
                        reply_delivered: true,
                        ..bm_core::memory::TurnPersonaLedger::default()
                    }),
                },
            )
            .expect("seed typed stable persona evidence");
    }
}

fn assert_production_autonomous_revision(
    platform: MemoryStoreHandle,
    clock: Arc<AdjustableMemoryClock>,
    owner: &str,
    relationship_id: &str,
) -> (String, String, String, String) {
    let (runtime, mounted_subject_id, human_subject_id) =
        autonomous_relationship_runtime(platform.clone(), clock.clone(), owner, relationship_id);
    create_autonomous_relationship_source(
        &runtime,
        &mounted_subject_id,
        &human_subject_id,
        relationship_id,
    );
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "autonomous-rev2-founding".to_string(),
            human_actor_subject_id: human_subject_id,
            charter: Box::new(SubjectSoulFoundingCharterSeedV1 {
                identity_anchor: Some("founding identity must remain in revision one".to_string()),
                character_tendencies: vec!["evidence before certainty".to_string()],
                priority_constitution: vec!["truth before convenience".to_string()],
                non_negotiables: vec!["never fabricate confirmation".to_string()],
                default_response_mode: None,
                default_initiative_posture: None,
                default_relationship_posture: None,
                boundary_doctrine: None,
                truth_seeking_commitment: None,
                self_preservation_doctrine: None,
                repair_doctrine: None,
                change_principle: None,
            }),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("founding revision one");
    let founding = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("founding current");
    let SubjectSoulReadOutcomeV1::Verified { view: founding } = founding else {
        panic!("founding root must be verified")
    };
    assert_eq!(founding.revision, Some(1));
    let founding_digest = founding.material_digest.clone().expect("founding digest");

    clock.set(1_800_000_100);
    seed_stable_persona_evidence(&platform, relationship_id);

    let mut http = StaticHttpClient;
    let llm = AutonomousRevisionLlm::default();
    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            finalize_request(
                &mounted_subject_id,
                "window-autonomous-boundary",
                "turn-autonomous-boundary",
                "Please keep repair warm, bounded, and summary-first.",
            ),
        )
        .expect("production post-turn boundary evidence cycle");
    clock.set(1_800_000_200);
    let autonomous_revision_request = finalize_request(
        &mounted_subject_id,
        "window-autonomous-rev2",
        "turn-autonomous-rev2",
        "Please preserve truthful repair before compliance.",
    );
    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            autonomous_revision_request.clone(),
        )
        .expect("production post-turn autonomous revision");

    let current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current autonomous revision");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("autonomous root must remain verified")
    };
    assert_eq!(current.revision, Some(2));
    assert_eq!(
        current.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfGovernedRevision)
    );
    let historical = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Exact {
                generation: 1,
                revision: 1,
                material_digest: founding_digest.clone(),
            },
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("exact founding revision remains readable");
    let SubjectSoulReadOutcomeV1::Verified { view: historical } = historical else {
        panic!("founding exact root must remain verified")
    };
    assert_eq!(historical.revision, Some(1));
    assert_eq!(
        historical.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::HumanFoundingCharter)
    );

    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            autonomous_revision_request,
        )
        .expect("same production turn replays without another Soul revision");
    let replayed_current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current Soul after same-operation replay");
    let SubjectSoulReadOutcomeV1::Verified {
        view: replayed_current,
    } = replayed_current
    else {
        panic!("replayed autonomous root must remain verified")
    };
    assert_eq!(replayed_current.revision, Some(2));
    assert_eq!(replayed_current.head_digest, current.head_digest);
    assert_eq!(replayed_current.manifest_digest, current.manifest_digest);
    (
        mounted_subject_id,
        founding_digest,
        current.head_digest.clone(),
        current.manifest_digest.clone(),
    )
}

#[test]
fn production_post_turn_adopts_self_governed_revision_without_overwriting_founding_history() {
    let platform = empty_store_platform(support::host_test_profile());
    let clock = Arc::new(AdjustableMemoryClock::new(1_800_000_000));
    assert_production_autonomous_revision(
        platform,
        clock,
        "owner-autonomous-revision",
        "relationship:autonomous-revision",
    );
}

fn assert_persistent_autonomous_revision_reopen(
    config: StoreBackendConfig,
    owner: &str,
    relationship_id: &str,
) {
    let clock = Arc::new(AdjustableMemoryClock::new(1_800_000_000));
    let platform = support::open_memory_store(config.clone()).expect("open persistent store");
    let (mounted_subject_id, founding_digest, head_digest, manifest_digest) =
        assert_production_autonomous_revision(
            platform.clone(),
            clock.clone(),
            owner,
            relationship_id,
        );
    drop(platform);

    let reopened_store = support::open_memory_store(config).expect("reopen persistent store");
    let (reopened, reopened_subject_id, _) =
        autonomous_relationship_runtime(reopened_store, clock, owner, relationship_id);
    assert_eq!(reopened_subject_id, mounted_subject_id);
    let current = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened autonomous current Soul");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("reopened autonomous root must remain verified")
    };
    assert_eq!(current.revision, Some(2));
    assert_eq!(
        current.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfGovernedRevision)
    );
    assert_eq!(current.head_digest, head_digest);
    assert_eq!(current.manifest_digest, manifest_digest);

    let historical = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id,
            selector: SubjectSoulReadSelectorV1::Exact {
                generation: 1,
                revision: 1,
                material_digest: founding_digest,
            },
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened exact founding Soul");
    let SubjectSoulReadOutcomeV1::Verified { view: historical } = historical else {
        panic!("reopened exact founding root must remain verified")
    };
    assert_eq!(historical.revision, Some(1));
    assert_eq!(
        historical.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::HumanFoundingCharter)
    );
}

#[test]
fn production_autonomous_revision_survives_file_reopen() {
    let root = persistent_test_root("file-autonomous-revision");
    let config = StoreBackendConfig::file(&root, support::host_test_profile())
        .expect("file autonomous config");
    assert_persistent_autonomous_revision_reopen(
        config,
        "owner-file-autonomous-revision",
        "relationship:file-autonomous-revision",
    );
    std::fs::remove_dir_all(root).expect("remove file autonomous store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn production_autonomous_revision_survives_sqlite_reopen() {
    let root = persistent_test_root("sqlite-autonomous-revision");
    std::fs::create_dir_all(&root).expect("create sqlite autonomous root");
    let config =
        StoreBackendConfig::sqlite(root.join("memory.sqlite3"), support::host_test_profile())
            .expect("sqlite autonomous config");
    assert_persistent_autonomous_revision_reopen(
        config,
        "owner-sqlite-autonomous-revision",
        "relationship:sqlite-autonomous-revision",
    );
    std::fs::remove_dir_all(root).expect("remove sqlite autonomous store");
}

fn assert_production_autonomous_bootstrap(
    platform: MemoryStoreHandle,
    clock: Arc<AdjustableMemoryClock>,
    owner: &str,
    relationship_id: &str,
) -> (String, String, String, String) {
    let (runtime, mounted_subject_id, human_subject_id) =
        autonomous_relationship_runtime(platform.clone(), clock.clone(), owner, relationship_id);
    let pristine = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("pristine implicit unseeded Soul");
    assert!(matches!(
        pristine,
        SubjectSoulReadOutcomeV1::ImplicitUnseeded { generation: 1, .. }
    ));
    create_autonomous_relationship_source(
        &runtime,
        &mounted_subject_id,
        &human_subject_id,
        relationship_id,
    );
    clock.set(1_800_000_100);
    seed_stable_persona_evidence(&platform, relationship_id);

    let mut http = StaticHttpClient;
    let llm = AutonomousRevisionLlm::default();
    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            finalize_request(
                &mounted_subject_id,
                "window-autonomous-bootstrap-boundary",
                "turn-autonomous-bootstrap-boundary",
                "Please keep repair warm, bounded, and summary-first.",
            ),
        )
        .expect("first governed relationship evidence cycle");
    let explicit_unseeded = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("explicit unseeded after first governed evidence");
    assert!(matches!(
        explicit_unseeded,
        SubjectSoulReadOutcomeV1::Verified { ref view }
            if view.state == SubjectSoulLifecycleStateV1::Unseeded
                && view.revision.is_none()
                && view.origin.is_none()
    ));
    let first_evidence_snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("first governed evidence snapshot");
    assert!(first_evidence_snapshot.json_docs.iter().any(|document| {
        document.namespace == "mental_privacy" && document.key.contains(relationship_id)
    }));

    clock.set(1_800_000_200);
    let bootstrap_request = finalize_request(
        &mounted_subject_id,
        "window-autonomous-bootstrap-rev1",
        "turn-autonomous-bootstrap-rev1",
        "Please preserve truthful repair before compliance.",
    );
    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            bootstrap_request.clone(),
        )
        .expect("production self-authored bootstrap revision");
    assert_eq!(llm.autonomy_calls.load(Ordering::SeqCst), 2);
    let current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current autonomous bootstrap");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("autonomous bootstrap root must be verified")
    };
    assert_eq!(current.revision, Some(1));
    assert_eq!(
        current.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap)
    );
    let material_digest = current
        .material_digest
        .clone()
        .expect("bootstrap material digest");

    runtime
        .finalize_turn_with_inline_governance(Some(&mut http), Some(&llm), bootstrap_request)
        .expect("same bootstrap operation replays without a second revision");
    let replayed = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current bootstrap after replay");
    let SubjectSoulReadOutcomeV1::Verified { view: replayed } = replayed else {
        panic!("replayed bootstrap root must remain verified")
    };
    assert_eq!(replayed.revision, Some(1));
    assert_eq!(replayed.head_digest, current.head_digest);
    assert_eq!(replayed.manifest_digest, current.manifest_digest);
    (
        mounted_subject_id,
        material_digest,
        current.head_digest.clone(),
        current.manifest_digest.clone(),
    )
}

#[test]
fn production_post_turn_bootstraps_self_authored_revision_from_explicit_unseeded() {
    assert_production_autonomous_bootstrap(
        empty_store_platform(support::host_test_profile()),
        Arc::new(AdjustableMemoryClock::new(1_800_000_000)),
        "owner-autonomous-bootstrap",
        "relationship:autonomous-bootstrap",
    );
}

fn assert_persistent_autonomous_bootstrap_reopen(
    config: StoreBackendConfig,
    owner: &str,
    relationship_id: &str,
) {
    let clock = Arc::new(AdjustableMemoryClock::new(1_800_000_000));
    let platform = support::open_memory_store(config.clone()).expect("open bootstrap store");
    let (mounted_subject_id, material_digest, head_digest, manifest_digest) =
        assert_production_autonomous_bootstrap(
            platform.clone(),
            clock.clone(),
            owner,
            relationship_id,
        );
    drop(platform);

    let reopened_store = support::open_memory_store(config).expect("reopen bootstrap store");
    let (reopened, reopened_subject_id, _) =
        autonomous_relationship_runtime(reopened_store, clock, owner, relationship_id);
    assert_eq!(reopened_subject_id, mounted_subject_id);
    let current = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened autonomous bootstrap");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("reopened autonomous bootstrap must remain verified")
    };
    assert_eq!(current.revision, Some(1));
    assert_eq!(
        current.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap)
    );
    assert_eq!(current.head_digest, head_digest);
    assert_eq!(current.manifest_digest, manifest_digest);
    assert_eq!(current.material_digest.as_ref(), Some(&material_digest));

    let exact = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id,
            selector: SubjectSoulReadSelectorV1::Exact {
                generation: 1,
                revision: 1,
                material_digest,
            },
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened exact autonomous bootstrap");
    assert!(matches!(
        exact,
        SubjectSoulReadOutcomeV1::Verified { ref view }
            if view.revision == Some(1)
                && view.origin
                    == Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap)
    ));
}

#[test]
fn production_autonomous_bootstrap_survives_file_reopen() {
    let root = persistent_test_root("file-autonomous-bootstrap");
    let config = StoreBackendConfig::file(&root, support::host_test_profile())
        .expect("file autonomous bootstrap config");
    assert_persistent_autonomous_bootstrap_reopen(
        config,
        "owner-file-autonomous-bootstrap",
        "relationship:file-autonomous-bootstrap",
    );
    std::fs::remove_dir_all(root).expect("remove file autonomous bootstrap store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn production_autonomous_bootstrap_survives_sqlite_reopen() {
    let root = persistent_test_root("sqlite-autonomous-bootstrap");
    std::fs::create_dir_all(&root).expect("create sqlite autonomous bootstrap root");
    let config =
        StoreBackendConfig::sqlite(root.join("memory.sqlite3"), support::host_test_profile())
            .expect("sqlite autonomous bootstrap config");
    assert_persistent_autonomous_bootstrap_reopen(
        config,
        "owner-sqlite-autonomous-bootstrap",
        "relationship:sqlite-autonomous-bootstrap",
    );
    std::fs::remove_dir_all(root).expect("remove sqlite autonomous bootstrap store");
}

#[test]
fn maintenance_unavailable_commits_transcript_and_durable_v2_intent_without_raw_copy() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    let report = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-deferred-1",
                "叫我青川，这句话不得复制进任务",
            ),
        )
        .expect("finalize");
    assert!(report.session_commit.committed);
    assert!(report
        .transcript_commit
        .as_ref()
        .is_some_and(|commit| commit.committed));
    assert_eq!(
        report.memory_consolidation.state,
        MemoryConsolidationState::Queued
    );
    let job_id = report
        .memory_consolidation
        .job_id
        .as_deref()
        .expect("stable job id");
    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.to_string(),
        })
        .expect("exact job status");
    assert_eq!(status.job.status, PostTurnGovernanceJobStatusV2::Pending);
    assert_eq!(status.job.identity.conversation_id, "window-a");

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    let governance_docs = snapshot
        .json_docs
        .iter()
        .filter(|doc| {
            matches!(
                doc.namespace.as_str(),
                "post_turn_governance_jobs" | "post_turn_governance_scope_indexes"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(governance_docs.len(), 2);
    let encoded = serde_json::to_string(&governance_docs).expect("governance docs");
    assert!(!encoded.contains("叫我青川"));
    assert!(!encoded.contains("不得复制进任务"));
    assert!(!encoded.contains("inputMessages"));
}

#[test]
fn bounded_reconciliation_repairs_transcript_intent_gaps_and_advances_exact_cursor() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    for (turn_id, content) in [
        ("turn-reconcile-1", "第一条已提交但缺 intent"),
        ("turn-reconcile-2", "第二条已提交但缺 intent"),
    ] {
        runtime
            .commit_transcript(MemoryTranscriptCommitRequest {
                turn: finalize_request("subject-default", "window-a", turn_id, content).turn,
                host_refs: Vec::new(),
            })
            .expect("transcript-only commit");
    }

    let first = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("first bounded reconciliation");
    assert_eq!(first.inspected, 1);
    assert_eq!(first.created, 1);
    assert_eq!(first.cursor_sequence, 1);
    assert!(first.has_more);

    let second = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("second bounded reconciliation");
    assert_eq!(second.inspected, 1);
    assert_eq!(second.created, 1);
    assert_eq!(second.cursor_sequence, 2);
    assert!(!second.has_more);

    let exhausted = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("exhausted reconciliation");
    assert_eq!(exhausted.inspected, 0);
    assert_eq!(exhausted.created, 0);
    assert_eq!(exhausted.cursor_sequence, 2);
    assert!(!exhausted.has_more);
    let queue = runtime
        .deferred_governance_report()
        .expect("exact queue report");
    assert_eq!(queue.pending, 2);
}

#[test]
fn active_lease_renewal_is_fenced_by_owner_epoch_and_strict_deadline() {
    let profile = support::host_test_profile();
    let runtime = test_runtime_with_scope_and_subject(
        empty_store_platform(profile),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-renew",
                "续租必须受 fencing 约束",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:renew".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let renewed = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:renew".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_120,
        })
        .expect("renew")
        .job;
    assert_eq!(renewed.lease_epoch, leased.lease_epoch);
    assert_eq!(renewed.lease_until, Some(1_800_000_120));
    assert!(renewed.state_revision > leased.state_revision);

    let stale_owner = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:other".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_180,
        })
        .expect_err("another worker must not renew");
    assert_eq!(stale_owner.stage(), "post_turn_governance_renew");
    let non_extending = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id,
            lease_owner: "worker:renew".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_120,
        })
        .expect_err("renew must strictly extend");
    assert_eq!(non_extending.stage(), "post_turn_governance_renew");
}

#[test]
fn legacy_raw_turn_queue_requires_explicit_reset_and_is_never_auto_consumed() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    platform
        .replay_harness()
        .state_fs()
        .write(
            "memory/governance_jobs/pending.json",
            br#"[{"turn":{"raw":"must-not-migrate"}}]"#,
        )
        .expect("seed legacy queue");
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let error = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-legacy-reset",
                "不得迁移旧队列",
            ),
        )
        .err()
        .expect("legacy queue must block V2 worker readiness");
    assert!(error
        .to_string()
        .contains("legacy_governance_queue_reset_required"));
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(snapshot
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "post_turn_governance_jobs"));
    assert_eq!(
        platform
            .replay_harness()
            .state_fs()
            .read("memory/governance_jobs/pending.json")
            .expect("read legacy queue")
            .as_deref(),
        Some(br#"[{"turn":{"raw":"must-not-migrate"}}]"#.as_slice())
    );
}

#[test]
fn duplicate_finalize_repairs_or_reuses_one_exact_intent() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let request = finalize_request("subject-default", "window-a", "turn-deferred-1", "同一内容");
    let first = runtime
        .finalize_turn_with_inline_governance(None, None, request.clone())
        .expect("first finalize");
    let second = runtime
        .finalize_turn_with_inline_governance(None, None, request)
        .expect("duplicate finalize");
    assert_eq!(
        first.memory_consolidation.job_id,
        second.memory_consolidation.job_id
    );
    assert_eq!(
        second.memory_consolidation.reason,
        "governance_intent_already_present"
    );
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "post_turn_governance_jobs")
            .count(),
        1
    );
}

#[test]
fn transcript_mask_cancels_pending_governance_before_claim() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-private",
                "这段内容已经撤回",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "window-a".to_string(),
            turn_id: Some("turn-private".to_string()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "owner withdrew model use".to_string(),
        })
        .expect("mask transcript");

    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("job status");
    assert_eq!(status.job.status, PostTurnGovernanceJobStatusV2::Cancelled);
    assert_eq!(
        status.job.blocking_reason.as_deref(),
        Some("transcript_masked")
    );
    assert!(runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id,
            lease_owner: "worker-after-mask".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority(),
        })
        .is_err());
}

#[test]
fn same_turn_id_in_different_conversations_has_distinct_jobs() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let first = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "same-turn", "第一段"),
        )
        .expect("first conversation");
    let second = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-b", "same-turn", "第二段"),
        )
        .expect("second conversation");
    assert_ne!(
        first.memory_consolidation.job_id,
        second.memory_consolidation.job_id
    );
    let active = runtime
        .active_governance_jobs(MemoryGovernanceActiveJobsRequest { limit: 8 })
        .expect("runtime-scope active jobs");
    assert_eq!(active.jobs.len(), 2);
    assert!(!active.has_more);
    assert_eq!(
        active.jobs[0].scope_index_key,
        active.jobs[1].scope_index_key
    );
    assert_eq!(
        active
            .jobs
            .iter()
            .map(|job| job.identity.conversation_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["window-a", "window-b"])
    );
}

#[test]
fn concurrent_claim_has_one_winner_and_pins_first_attempt_authority() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let first = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let second = Arc::new(test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-claim", "并发领取"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let barrier = Arc::new(Barrier::new(3));
    let workers = [(first, "worker-a"), (second, "worker-b")]
        .into_iter()
        .map(|(runtime, worker)| {
            let barrier = Arc::clone(&barrier);
            let job_id = job_id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                runtime.claim_governance_job(MemoryGovernanceJobClaimRequest {
                    job_id,
                    lease_owner: worker.to_string(),
                    lease_until: 1_800_000_060,
                    authority: attempt_authority(),
                })
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let winner = results.into_iter().find_map(Result::ok).expect("winner");
    assert_eq!(winner.job.status, PostTurnGovernanceJobStatusV2::Leased);
    assert_eq!(winner.job.attempt_count, 1);
    assert_eq!(winner.job.lease_epoch, 1);
    assert_eq!(winner.job.attempt_authority, Some(attempt_authority()));
}

#[test]
fn independent_file_opens_still_allow_only_one_claim_winner() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("file-claim");
    {
        let config = StoreBackendConfig::file(&root, profile).expect("file config");
        assert_independent_open_claim(config);
    }
    std::fs::remove_dir_all(&root).expect("remove file claim store");
}

#[test]
fn file_reopen_preserves_atomic_memory_and_terminal_receipt() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("file-complete");
    {
        let config = StoreBackendConfig::file(&root, profile).expect("file config");
        assert_persistent_atomic_completion(config);
    }
    std::fs::remove_dir_all(&root).expect("remove file completion store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn independent_sqlite_opens_still_allow_only_one_claim_winner() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("sqlite-claim");
    std::fs::create_dir_all(&root).expect("create sqlite claim root");
    {
        let config = StoreBackendConfig::sqlite(root.join("memory.sqlite3"), profile)
            .expect("sqlite config");
        assert_independent_open_claim(config);
    }
    std::fs::remove_dir_all(&root).expect("remove sqlite claim store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_atomic_memory_and_terminal_receipt() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("sqlite-complete");
    std::fs::create_dir_all(&root).expect("create sqlite completion root");
    {
        let config = StoreBackendConfig::sqlite(root.join("memory.sqlite3"), profile)
            .expect("sqlite config");
        assert_persistent_atomic_completion(config);
    }
    std::fs::remove_dir_all(&root).expect("remove sqlite completion store");
}

#[test]
fn retry_requires_current_lease_and_enforces_runtime_backoff() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-retry", "稍后重试"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker-a".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority(),
        })
        .expect("claim")
        .job;

    assert!(runtime
        .retry_governance_job(MemoryGovernanceJobRetryRequest {
            job_id: job_id.clone(),
            lease_owner: "stale-worker".to_string(),
            lease_epoch: leased.lease_epoch,
            error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
        })
        .is_err());
    let retry = runtime
        .retry_governance_job(MemoryGovernanceJobRetryRequest {
            job_id: job_id.clone(),
            lease_owner: "worker-a".to_string(),
            lease_epoch: leased.lease_epoch,
            error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
        })
        .expect("schedule retry")
        .job;
    assert_eq!(retry.status, PostTurnGovernanceJobStatusV2::RetryWaiting);
    assert_eq!(retry.next_attempt_at, Some(1_800_000_005));
    assert_eq!(
        retry.last_error_class,
        Some(PostTurnGovernanceErrorClassV2::ServiceUnavailable)
    );
    assert!(runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id,
            lease_owner: "worker-b".to_string(),
            lease_until: 1_800_000_120,
            authority: attempt_authority(),
        })
        .is_err());
}

#[test]
fn exact_job_query_rejects_another_memory_space_and_subject() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-a",
        "owner-a",
        "subject-a",
        "llm.gateway",
        "chat-a",
    );
    let runtime_b = test_runtime_with_identity_scope_and_subject(
        platform,
        profile,
        "agent-b",
        "owner-b",
        "subject-b",
        "llm.gateway",
        "chat-a",
    );
    let finalized = runtime_a
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-a", "window-a", "turn-a", "主体隔离"),
        )
        .expect("finalize a");
    let error = runtime_b
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: finalized.memory_consolidation.job_id.expect("job id"),
        })
        .expect_err("foreign runtime must fail closed");
    assert!(error.to_string().contains("mounted runtime"));
}

#[test]
fn due_query_does_not_starve_pending_work_behind_future_retries() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    for index in 0..32 {
        let finalized = runtime
            .finalize_turn_with_inline_governance(
                None,
                None,
                finalize_request(
                    "subject-default",
                    &format!("future-conversation-{index}"),
                    &format!("future-turn-{index}"),
                    "稍后重试的任务",
                ),
            )
            .expect("future retry finalize");
        let job_id = finalized.memory_consolidation.job_id.expect("job id");
        let pending = runtime
            .governance_job_status(MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("pending")
            .job;
        let leased = runtime
            .claim_governance_job(MemoryGovernanceJobClaimRequest {
                job_id: job_id.clone(),
                lease_owner: format!("worker:future-{index}"),
                lease_until: 1_800_000_060,
                authority: attempt_authority_for_job(&pending),
            })
            .expect("claim future retry")
            .job;
        runtime
            .retry_governance_job(MemoryGovernanceJobRetryRequest {
                job_id,
                lease_owner: format!("worker:future-{index}"),
                lease_epoch: leased.lease_epoch,
                error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
            })
            .expect("schedule future retry");
    }
    let pending = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "due-conversation",
                "due-turn",
                "现在应当执行的任务",
            ),
        )
        .expect("due finalize");
    let pending_job_id = pending.memory_consolidation.job_id.expect("pending job id");

    let active = runtime
        .active_governance_jobs(MemoryGovernanceActiveJobsRequest { limit: 32 })
        .expect("bounded active jobs");
    assert!(active.has_more);
    assert!(
        active.jobs.iter().any(|job| job.job_id == pending_job_id),
        "actionable pending work must be selected ahead of future retries"
    );
}

#[test]
fn claimed_governance_commits_memory_and_terminal_receipt_once() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-atomic-learning",
                "以后叫我青川",
            ),
        )
        .expect("finalize");
    assert!(matches!(
        runtime
            .read_subject_soul(SubjectSoulReadRequestV1 {
                target_subject_id: "subject-default".to_string(),
                selector: SubjectSoulReadSelectorV1::Current,
                view: SubjectSoulReadViewV1::OperatorSafe,
            })
            .expect("implicit Soul before governed evidence"),
        SubjectSoulReadOutcomeV1::ImplicitUnseeded { .. }
    ));
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending status")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:atomic-learning".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "profile",
                "source_authority": "user_asserted",
                "topic": "preferred_name",
                "content": "The user asked to be called Qingchuan.",
                "keywords": ["Qingchuan", "preferred name"]
            }
        ]"#,
    };
    let request = MemoryGovernanceJobRunRequest {
        job_id: job_id.clone(),
        lease_owner: "worker:atomic-learning".to_string(),
        lease_epoch: leased.lease_epoch,
    };
    let completed = runtime
        .run_claimed_governance(&mut http, Some(&llm), request.clone())
        .expect("atomic governance run");
    assert_eq!(
        completed.job.status,
        PostTurnGovernanceJobStatusV2::Succeeded
    );
    assert!(completed.job.receipt.is_some());
    assert_eq!(completed.semantic_governance.accepted_count, 1);
    let soul_after = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: "subject-default".to_string(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("explicit unseeded Soul after first governed evidence");
    assert!(matches!(
        soul_after,
        SubjectSoulReadOutcomeV1::Verified { ref view }
            if view.state == SubjectSoulLifecycleStateV1::Unseeded
                && view.revision.is_none()
    ));
    let queue = runtime
        .deferred_governance_report()
        .expect("terminal queue report");
    assert_eq!(queue.pending, 0);
    assert_eq!(queue.terminal, 1);
    assert_eq!(queue.recent_jobs[0].job_id, job_id);
    let first_snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("first snapshot");
    assert!(serde_json::to_string(&first_snapshot.json_docs)
        .expect("snapshot json")
        .contains("Qingchuan"));

    let duplicate = runtime
        .run_claimed_governance(&mut http, Some(&llm), request)
        .expect("idempotent governance run");
    assert_eq!(duplicate.job.receipt, completed.job.receipt);
    let second_snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("second snapshot");
    assert_eq!(second_snapshot.json_docs, first_snapshot.json_docs);
}

#[test]
fn transcript_revocation_after_prompt_assembly_blocks_first_network_byte_and_memory_commit() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-egress-revoked",
                "这段内容不得发给模型",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:egress".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let llm = LifecycleRevokingLlm {
        runtime: Arc::clone(&runtime),
        revoked: AtomicBool::new(false),
    };
    let mut http = CountingHttpClient::default();
    let error = runtime
        .run_claimed_governance(
            &mut http,
            Some(&llm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:egress".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("revoked disclosure must fail closed");
    assert!(error
        .to_string()
        .contains("network disclosure lease is stale"));
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("cancelled status")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::Cancelled);
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(snapshot
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "private_garden" && doc.namespace != "long_term"));
}

#[test]
fn stricter_runtime_privacy_after_claim_blocks_first_network_byte() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let original = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = original
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-privacy-tightened",
                "这段内容只允许在原隐私策略下处理",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let authority = original
        .prepare_governance_attempt_authority(MemoryGovernanceAttemptAuthorityRequest {
            job_id: job_id.clone(),
            binding_id: "governance-model:test".to_string(),
            config_revision: 1,
            model_id: "qwen3:8b".to_string(),
        })
        .expect("original privacy authority")
        .authority;
    let leased = original
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:privacy-tightened".to_string(),
            lease_until: 1_800_000_060,
            authority,
        })
        .expect("claim")
        .job;

    let mut stricter = MemoryPrivacyPolicy::standard_private_boundary();
    stricter.governance_model_disclosure_allowed = false;
    let tightened = test_runtime_with_scope_subject_and_privacy(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
        stricter,
    );
    let mut http = CountingHttpClient::default();
    let error = tightened
        .run_claimed_governance(
            &mut http,
            Some(&NetworkAttemptingLlm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:privacy-tightened".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("tightened privacy must revoke disclosure");
    assert!(error.to_string().contains("privacy authority"));
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    let status = tightened
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("job status")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::Leased);
    assert!(status.receipt.is_none());
}

#[test]
fn memory_precondition_conflict_keeps_job_leased_and_commits_no_semantic_post_image() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-atomic-conflict",
                "验证原子冲突",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:conflict".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let llm = ConflictInjectingLlm {
        store: platform.clone(),
        calls: AtomicUsize::new(0),
    };
    let mut http = StaticHttpClient;
    runtime
        .run_claimed_governance(
            &mut http,
            Some(&llm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:conflict".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("stale private post image must reject the whole completion transaction");
    let current = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("leased status")
        .job;
    assert_eq!(current.status, PostTurnGovernanceJobStatusV2::Leased);
    assert!(current.receipt.is_none());
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    let encoded = serde_json::to_string(&snapshot.json_docs).expect("snapshot json");
    assert!(encoded.contains("concurrent winner"));
    assert!(!encoded.contains("This semantic write must not partially commit."));
}
