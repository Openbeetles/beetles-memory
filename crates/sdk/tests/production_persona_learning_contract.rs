mod support;

use bm_sdk::*;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};

const OBSERVATION: &str =
    "In our conversation you admitted uncertainty instead of inventing a confident answer.";
const GROWTH: &str = "grounded candor before certainty";
static TEMP: AtomicU64 = AtomicU64::new(0);

struct LearningModel {
    calls: AtomicUsize,
    growth: bool,
    saw_verified_growth: AtomicBool,
}
impl LearningModel {
    fn new(growth: bool) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            growth,
            saw_verified_growth: AtomicBool::new(false),
        }
    }
}
impl LlmClient for LearningModel {
    fn chat(
        &self,
        _: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        _: Option<&[ToolSpec]>,
        _: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        assert!(
            !messages
                .iter()
                .any(|message| message.content.contains("now=")),
            "official background model request contains a competing epoch clock"
        );
        let content = if system.contains("persona interpretation") {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let input: serde_json::Value = serde_json::from_str(&messages[0].content).unwrap();
            assert!(messages
                .iter()
                .any(|message| message.content.contains(OBSERVATION)));
            assert!(
                !messages
                    .iter()
                    .any(|message| message.content.contains(GROWTH)),
                "model must interpret canonical observations, not its own previous interpretation"
            );
            if self.growth {
                serde_json::json!({
                    "decision": "growth", "scope": "subject", "priority_order": [], "value_orientation": GROWTH,
                    "relationship_posture": "", "supporting_observations": [{
                        "message_id": input["observations"][0]["message_id"], "quote": OBSERVATION,
                    }], "counter_observations": [],
                })
                .to_string()
            } else {
                r#"{"decision":"operational_only"}"#.into()
            }
        } else if system == bm_core::memory::SELF_MODEL_SYSTEM_PROMPT {
            assert!(messages
                .iter()
                .any(|message| message.content.contains(GROWTH)));
            assert!(
                !messages
                    .iter()
                    .any(|message| message.content.contains(OBSERVATION)),
                "raw turn must not be promoted into the initial self model"
            );
            serde_json::json!({"continuity_anchor":"I preserve grounded candor before certainty", "value_orientation":GROWTH}).to_string()
        } else if system.contains("private garden") {
            "null".into()
        } else if system.contains("inward autonomy runtime") {
            if messages.iter().any(|message| {
                message.content.contains("Subject Growth Evidence")
                    && message.content.contains(GROWTH)
            }) {
                self.saw_verified_growth.store(true, Ordering::SeqCst);
            }
            "{}".into()
        } else {
            "[]".into()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

struct Port<'a>(&'a LearningModel);
impl GovernanceExecutionPort for Port<'_> {
    fn execute(
        &mut self,
        _: &AuthorizedGovernanceEnvelope,
        _: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        egress
            .revalidate_before_egress()
            .map_err(GovernanceExecutionPortFailure::Other)?;
        operation
            .run(&mut support::StaticHttpClient, self.0)
            .map_err(|error| {
                eprintln!("synthetic persona governance owner failure: {error:?}");
                GovernanceExecutionPortFailure::Other(error)
            })
    }
}

fn runtime_for_store(store: MemoryStoreHandle, agent: &str) -> Arc<MemoryRuntime> {
    Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new(agent, "persona-learning-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", format!("learning-chat-{agent}")).unwrap())
            .store(store)
            .clock(support::fixed_clock(1_800_000_000))
            .user_local_time_provider(Arc::new(UnavailableUserLocalTimeProvider))
            .build()
            .unwrap(),
    )
}

const RELATIONSHIP: &str = "relationship:persona-review";
const RELATIONSHIP_FLOOR: &str = "respect explicit refusal";

fn review_runtime_for_store(store: MemoryStoreHandle) -> Arc<MemoryRuntime> {
    let owner = "persona-learning-owner";
    let agent = "review-persona";
    let mounted = default_agent_subject_id(agent);
    let human = primary_human_subject_id(owner);
    let registry = SubjectRegistry::single_agent_default(owner, agent).unwrap();
    let mut graph = SubjectRelationshipGraph::single_agent_default(&registry).unwrap();
    for edge in &mut graph.edges {
        if (edge.from_subject_id == mounted && edge.to_subject_id == human)
            || (edge.from_subject_id == human && edge.to_subject_id == mounted)
        {
            edge.relationship_id = Some(RELATIONSHIP.into());
        }
    }
    Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new(agent, owner).unwrap())
            .scope(MemoryScope::new("sdk.direct", "learning-chat-review-persona").unwrap())
            .store(store)
            .subject_registry(registry)
            .subject_relationship_graph(graph)
            .scoped_runtime(SubjectScopedRuntime {
                memory_space_id: default_memory_space_id(owner),
                mounted_subject_id: mounted.clone(),
                actor_subject_id: mounted,
                agent_id: agent.into(),
                relationship_scope: Some(bm_core::memory::RelationshipScope {
                    relationship_id: RELATIONSHIP.into(),
                    channel: "sdk.direct".into(),
                    conversation_id: Some("learning-chat-review-persona".into()),
                }),
                projection_policy: "subject_aware_default".into(),
                write_policy: "subject_candidate_then_space_governance".into(),
            })
            .clock(support::fixed_clock(1_800_000_000))
            .build()
            .unwrap(),
    )
}

fn install_relationship_floor(runtime: &MemoryRuntime) {
    let human = primary_human_subject_id("persona-learning-owner");
    runtime
        .control_relationship_source(RelationshipSourceControlIntentV1 {
            operation_id: "relationship-review-boundary".into(),
            memory_space_id: runtime.memory_space_id().into(),
            relationship_id: RELATIONSHIP.into(),
            mounted_subject_id: runtime.subject_id().into(),
            counterparty_subject_ids: vec![human.clone()],
            expected_state: runtime
                .relationship_source_pristine_expected_state(RELATIONSHIP)
                .unwrap(),
            authority: RelationshipSourceControlAuthorityV1::HumanUser {
                actor_subject_id: human,
            },
            action: RelationshipSourceControlIntentActionV1::Create {
                clauses: RelationshipSourceClausesV1 {
                    disclosure_ceiling: RelationshipDisclosureCeilingV1::GovernedSummary,
                    access_constraints: vec![
                        RelationshipAccessConstraintV1::NoPrivateRaw,
                        RelationshipAccessConstraintV1::GovernedDisclosureOnly,
                    ],
                    truth_commitments: vec!["state uncertainty".into()],
                    mutual_boundary_commitments: vec![RELATIONSHIP_FLOOR.into()],
                    repair_commitments: vec!["repair before escalation".into()],
                },
                source_asserted_at: Some(1_800_000_000),
                evidence_digest: "c".repeat(64),
            },
        })
        .unwrap();
}

fn install(runtime: &MemoryRuntime) {
    runtime
        .install_governance_binding(MemoryGovernanceBindingInstallRequest {
            source_owner_id: "test-deployment".into(),
            source_config_id: "single-provider".into(),
            source_revision: 1,
            protocol: PostTurnGovernanceProviderProtocolV1::OllamaNative,
            endpoint: "http://127.0.0.1:11434/api".into(),
            model_id: "synthetic".into(),
            credential_reference: None,
            request_timeout_ms: 30_000,
            max_input_tokens: 8192,
            max_output_tokens: 2048,
            provider_permission_generation: 1,
        })
        .unwrap();
}

fn signed_persona_request(runtime: &MemoryRuntime, turn_id: &str) -> MemoryTurnFinalizeRequest {
    let projected = runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: turn_id.into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: OBSERVATION.into(),
            system_max_len: 8192,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![],
        })
        .unwrap();
    let mut learning = PostTurnLearningInputV3::empty();
    learning.selection_receipt = Some(projected.report().selection_receipt().unwrap().clone());
    assert!(learning
        .selection_receipt
        .as_ref()
        .unwrap()
        .soul_source_admission
        .is_some());
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.into(),
            conversation: ConversationScope {
                channel: runtime.scope().channel.clone(),
                chat_id: runtime.scope().chat_id.clone(),
                conversation_id: Some(runtime.scope().chat_id.clone()),
            },
            subject: runtime.subject_id().into(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: IngressKind::User,
                channel: "sdk.direct".into(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: None,
                model_alias: None,
                model_resolved: None,
                request_id: None,
                client_conversation_hint: None,
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user(OBSERVATION)],
            assistant_messages: vec![bm_sdk::TranscriptAssistantMessage::complete(
                TranscriptInputMessage::assistant(
                    "I will distinguish the evidence from uncertainty.",
                ),
            )],
            tool_observations: vec![],
            external_content_used: false,
            candidate_ids: vec![],
        },
        learning,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn run_turn(runtime: &Arc<MemoryRuntime>, turn_id: &str, model: &LearningModel) {
    runtime
        .finalize_turn(signed_persona_request(runtime, turn_id))
        .unwrap();
    let engine = MemoryLearningEngine::attach(runtime.clone()).unwrap();
    let outcome = engine
        .run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "production-persona-learning-test".into(),
                lease_duration_secs: 60,
            },
            &mut Port(model),
        )
        .unwrap();
    assert!(
        model.calls.load(Ordering::SeqCst) > 0,
        "public learning engine never requested persona interpretation: {outcome:?}"
    );
    let MemoryLearningCycleOutcome::Completed(report) = outcome else {
        panic!("expected semantic completion: {outcome:?}");
    };
    assert!(report.job.receipt.is_some());
}

#[test]
fn public_engine_interprets_signed_canonical_turn_without_host_persona_ledger() {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let runtime = runtime_for_store(store, "persona-agent");
    install(&runtime);
    run_turn(&runtime, "observation-turn", &LearningModel::new(false));
}

#[test]
fn cancelled_interrupted_prefix_is_recalled_without_persona_growth() {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let runtime = runtime_for_store(store, "cancelled-persona-source");
    install(&runtime);
    let mut request = signed_persona_request(&runtime, "cancelled-after-visible-prefix");
    request.turn.delivery_status = MemoryTurnDeliveryStatus::Cancelled;
    request.turn.assistant_messages =
        vec![TranscriptAssistantMessage::interrupted_delivered_prefix(
            TranscriptInputMessage::assistant("I began to distinguish the evidence"),
        )];
    runtime.finalize_turn(request).unwrap();
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: runtime.scope().chat_id.clone(),
            limit: 8,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    assert_eq!(replay.slice.turns[0].assistant_messages.len(), 1);
    assert_eq!(
        replay.slice.turns[0].assistant_messages[0].settlement,
        AssistantMessageSettlement::InterruptedDeliveredPrefix,
    );
    let model = LearningModel::new(true);
    let engine = MemoryLearningEngine::attach(runtime.clone()).unwrap();
    engine
        .run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "cancelled-persona-source-test".into(),
                lease_duration_secs: 60,
            },
            &mut Port(&model),
        )
        .unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        runtime
            .read_subject_soul(SubjectSoulReadRequestV1 {
                target_subject_id: runtime.subject_id().into(),
                selector: SubjectSoulReadSelectorV1::Current,
                view: SubjectSoulReadViewV1::OperatorSafe,
            })
            .unwrap(),
        SubjectSoulReadOutcomeV1::ImplicitUnseeded { .. }
    ));
}

fn assert_metadata_private(runtime: &MemoryRuntime) {
    let outcome = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.subject_id().into(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .unwrap();
    let safe = serde_json::to_string(&outcome).unwrap();
    assert!(!safe.contains(GROWTH));
    assert!(!safe.contains(OBSERVATION));
    assert!(
        matches!(outcome, SubjectSoulReadOutcomeV1::Verified { view } if view.state == SubjectSoulLifecycleStateV1::Unseeded && view.revision.is_none()),
        "insufficient independent evidence must not fabricate a personality"
    );
}

fn projection_request(
    temporal_operation: MemoryRecallTemporalOperation,
) -> MemoryProjectionRequest {
    MemoryProjectionRequest {
        binding: ProceduralProjectionBindingV1::Preview,
        temporal_operation,
        user_query: "How do you approach uncertain claims?".into(),
        system_max_len: 8192,
        recent_messages_limit: 8,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
        structured_query_facets: vec![],
        tool_registry_refs: vec![],
    }
}

fn reopen_proof(config: StoreBackendConfig, persistent: bool) {
    let store = support::open_memory_store(config.clone()).unwrap();
    let first = runtime_for_store(store.clone(), "persona-agent");
    install(&first);
    for index in 0..2 {
        run_turn(
            &first,
            &format!("founding-observation-{index}"),
            &LearningModel::new(true),
        );
    }
    assert_metadata_private(&first);
    drop(first);
    let store = if persistent {
        drop(store);
        support::open_memory_store(config).unwrap()
    } else {
        store
    };
    let reopened = runtime_for_store(store.clone(), "persona-agent");
    // The host reinstalls its current execution binding; immutable snapshots
    // persist, but the SDK does not guess which deployment binding is current.
    install(&reopened);
    let second = LearningModel::new(false);
    run_turn(&reopened, "later-operational-turn", &second);
    assert!(
        second.saw_verified_growth.load(Ordering::SeqCst),
        "reopened official governance did not consume the persisted source-verified observation"
    );
    assert_metadata_private(&reopened);
    drop(reopened);
    let other = runtime_for_store(store, "other-persona");
    install(&other);
    let foreign = LearningModel::new(false);
    run_turn(&other, "other-subject-turn", &foreign);
    assert!(
        !foreign.saw_verified_growth.load(Ordering::SeqCst),
        "another subject consumed private personality evidence"
    );
}

#[test]
fn in_memory_persona_evidence_survives_runtime_reopen_without_public_disclosure() {
    reopen_proof(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        false,
    );
}

#[test]
fn archiving_a_canonical_source_preserves_its_lawful_persona_evidence() {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let runtime = runtime_for_store(store, "archived-persona-source");
    install(&runtime);
    for index in 0..2 {
        run_turn(
            &runtime,
            &format!("archive-source-{index}"),
            &LearningModel::new(true),
        );
    }
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: runtime.scope().chat_id.clone(),
            turn_id: Some("archive-source-0".into()),
            transition: TranscriptLifecycleTransition::Archive,
            reason: "archive_without_withdrawing_source".into(),
        })
        .unwrap();
    let later = LearningModel::new(false);
    run_turn(&runtime, "after-source-archive", &later);
    assert!(
        later.saw_verified_growth.load(Ordering::SeqCst),
        "presentation archive must not invalidate canonical content evidence"
    );
    assert_metadata_private(&runtime);
}

#[test]
fn file_persona_evidence_survives_store_reopen_without_public_disclosure() {
    let path = std::env::temp_dir().join(format!(
        "beetle-persona-file-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    reopen_proof(
        StoreBackendConfig::file(&path, support::host_test_profile()).unwrap(),
        true,
    );
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_persona_evidence_survives_store_reopen_without_public_disclosure() {
    let root = std::env::temp_dir().join(format!(
        "beetle-persona-sqlite-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    reopen_proof(
        StoreBackendConfig::sqlite(root.join("memory.sqlite"), support::host_test_profile())
            .unwrap(),
        true,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn four_independent_production_observations_form_governed_core_and_project() {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let runtime = runtime_for_store(store, "growing-persona");
    install(&runtime);
    for index in 0..4 {
        run_turn(
            &runtime,
            &format!("independent-observation-{index}"),
            &LearningModel::new(true),
        );
        if index < 3 {
            assert_metadata_private(&runtime);
        }
    }
    let outcome = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.subject_id().into(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .unwrap();
    assert!(
        matches!(outcome, SubjectSoulReadOutcomeV1::Verified { view }
        if view.state == SubjectSoulLifecycleStateV1::Active && view.revision == Some(1)
            && view.origin == Some(SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap)
            && view.source_read_availability.permits_content())
    );
    let projected = runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "How do you approach uncertain claims?".into(),
            system_max_len: 8192,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![],
        })
        .unwrap();
    assert!(
        projected
            .provider_payload()
            .system_memory_block()
            .contains(GROWTH),
        "governed production growth never reached model projection"
    );
    let block = projected.provider_payload().system_memory_block();
    assert!(
        block.contains("Situated now:"),
        "must exercise a nonempty owned Subject Shell"
    );
    assert!(
        !block.contains("now="),
        "missing local time must not expose an epoch clock: {block}"
    );
    assert!(!block.contains("User device local time:"));
}

fn source_review_recovery_proof(config: StoreBackendConfig, persistent: bool) {
    let store = support::open_memory_store(config.clone()).unwrap();
    let mut runtime = review_runtime_for_store(store.clone());
    install(&runtime);
    for index in 0..4 {
        run_turn(
            &runtime,
            &format!("review-source-{index}"),
            &LearningModel::new(true),
        );
    }
    install_relationship_floor(&runtime);
    const INDEPENDENT_FACT: &str = "The user calls their review workspace Cedar Observatory.";
    // Keep this accepted memory independent of every Soul observation being withdrawn.
    // Recall delivery requires an actual evidence citation, not just a successfully stored row.
    let independent_turn_id = "independent-shared-observation";
    let independent = runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: CanonicalTurnDelta {
                turn_id: independent_turn_id.into(),
                conversation: ConversationScope {
                    channel: runtime.scope().channel.clone(),
                    chat_id: runtime.scope().chat_id.clone(),
                    conversation_id: Some(runtime.scope().chat_id.clone()),
                },
                subject: runtime.subject_id().into(),
                delivery_status: MemoryTurnDeliveryStatus::Delivered,
                source: MemoryTurnSource {
                    ingress: IngressKind::User,
                    channel: runtime.scope().channel.clone(),
                    provider: None,
                    protocol: MemoryTurnProtocol::Native,
                    endpoint: None,
                    model_alias: None,
                    model_resolved: None,
                    request_id: None,
                    client_conversation_hint: None,
                },
                actor: None,
                input_messages: vec![TranscriptInputMessage::user(INDEPENDENT_FACT)],
                assistant_messages: vec![bm_sdk::TranscriptAssistantMessage::complete(
                    TranscriptInputMessage::assistant("I will use that workspace name."),
                )],
                tool_observations: vec![],
                external_content_used: false,
                candidate_ids: vec![],
            },
            host_refs: vec![],
        })
        .unwrap();
    assert!(independent.transcript_commit.as_ref().unwrap().committed);
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: runtime.scope().channel.clone(),
            conversation_id: runtime.scope().chat_id.clone(),
            limit: 8,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    let source_turn = replay
        .slice
        .turns
        .iter()
        .find(|turn| turn.turn_id == independent_turn_id)
        .unwrap();
    let source_message = &source_turn.input_messages[0];
    assert_eq!(source_message.content.as_deref(), Some(INDEPENDENT_FACT));
    let independent_citation = TranscriptEvidenceRef {
        memory_space_id: independent.key.memory_space_id,
        channel_id: independent.key.channel_id,
        conversation_id: independent.key.conversation_id,
        turn_id: source_turn.turn_id.clone(),
        message_id: Some(source_message.message_id.clone()),
        subject_id: Some(runtime.subject_id().into()),
        authority: Some(MemoryEvidenceAuthority::UserAsserted),
    }
    .display_citation();
    let written = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Profile,
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    topic: "cedar_observatory".into(),
                    content: INDEPENDENT_FACT.into(),
                    keywords: vec![
                        "Cedar".into(),
                        "Observatory".into(),
                        "review station".into(),
                    ],
                    source_chat_id: Some(runtime.scope().chat_id.clone()),
                    source_type: None,
                    source_scope: None,
                    subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
                    provenance: LongTermMemoryProvenance::new(
                        MemoryEvidenceAuthority::UserAsserted,
                    ),
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec![independent_citation],
                    canonical_entities: vec![],
                    evidence_count: None,
                    observed_at: Some(1_800_000_000),
                    source_revision: None,
                }],
                deletes: vec![],
                skill_writes: vec![],
            },
        })
        .unwrap();
    assert_eq!(written.changed, 1);
    assert!(written.accepted);
    assert_eq!(written.shared_fact_governance.as_ref().unwrap().accepted, 1);

    let mut shared_memory_query = projection_request(MemoryRecallTemporalOperation::Current);
    shared_memory_query.user_query = format!("{OBSERVATION} Cedar Observatory review station");
    let shared_before = runtime.project(shared_memory_query).unwrap();
    assert!(
        shared_before.provider_payload().system_memory_block().contains(INDEPENDENT_FACT),
        "independent accepted long-term memory must be visible for the same query before source withdrawal"
    );

    assert!(
        runtime
            .project(projection_request(MemoryRecallTemporalOperation::Current))
            .unwrap()
            .provider_payload()
            .system_memory_block()
            .contains(RELATIONSHIP_FLOOR),
        "active relationship floor positive control"
    );
    let before = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.subject_id().into(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .unwrap();
    let SubjectSoulReadOutcomeV1::Verified { view: before } = before else {
        panic!("growth must produce an owned Soul");
    };
    assert_eq!(before.revision, Some(1));
    let exact_before = runtime
        .project_with_subject_soul_selector(
            projection_request(MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_800_000_000,
            }),
            Some(SubjectSoulReadSelectorV1::Exact {
                generation: before.generation,
                revision: before.revision.unwrap(),
                material_digest: before.material_digest.clone().unwrap(),
            }),
        )
        .unwrap();
    assert!(
        exact_before
            .provider_payload()
            .system_memory_block()
            .contains(GROWTH),
        "the exact historical Soul revision remains a positive control before withdrawal"
    );
    assert!(
        !exact_before
            .provider_payload()
            .system_memory_block()
            .contains(RELATIONSHIP_FLOOR),
        "an exact Soul selector must not borrow a current relationship source"
    );
    assert!(runtime
        .project(projection_request(MemoryRecallTemporalOperation::Current))
        .unwrap()
        .provider_payload()
        .system_memory_block()
        .contains(GROWTH));
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: runtime.scope().chat_id.clone(),
            turn_id: Some("review-source-0".into()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "withdraw_observation_source".into(),
        })
        .unwrap();
    drop(runtime);
    let store = if persistent {
        drop(store);
        support::open_memory_store(config.clone()).unwrap()
    } else {
        store
    };
    runtime = review_runtime_for_store(store.clone());
    install(&runtime);
    let held = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.subject_id().into(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .unwrap();
    assert!(matches!(held, SubjectSoulReadOutcomeV1::Verified { view }
        if !view.source_read_availability.permits_content() && view.runtime_private_core.is_none()));
    let pending = runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: "review-new-observation".into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: format!("{OBSERVATION} Cedar Observatory review station"),
            system_max_len: 8192,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![],
        })
        .unwrap();
    assert!(!pending
        .provider_payload()
        .system_memory_block()
        .contains(GROWTH));
    assert!(pending.provider_payload().system_memory_block().contains(RELATIONSHIP_FLOOR),
        "independent HumanUser relationship floor must remain enforced while Soul awaits source review");
    assert!(
        pending
            .provider_payload()
            .system_memory_block()
            .contains(INDEPENDENT_FACT),
        "Soul source-review pending must preserve independent accepted shared long-term memory"
    );
    assert!(matches!(
        pending
            .report()
            .selection_receipt()
            .unwrap()
            .soul_source_admission
            .as_ref()
            .unwrap()
            .purpose,
        bm_core::memory::SoulTurnSourcePurposeV1::SourceReview { .. }
    ));
    run_turn(
        &runtime,
        "review-new-observation",
        &LearningModel::new(true),
    );
    drop(runtime);
    let store = if persistent {
        drop(store);
        support::open_memory_store(config.clone()).unwrap()
    } else {
        store
    };
    runtime = review_runtime_for_store(store.clone());
    install(&runtime);
    let recovered = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.subject_id().into(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .unwrap();
    assert!(
        matches!(recovered, SubjectSoulReadOutcomeV1::Verified { view }
        if view.source_read_availability.permits_content() && view.generation == before.generation
            && view.revision == Some(2))
    );
    assert!(
        runtime
            .project(projection_request(MemoryRecallTemporalOperation::Current))
            .unwrap()
            .provider_payload()
            .system_memory_block()
            .contains(GROWTH),
        "recovered same-generation Soul must really project"
    );
    let exact = SubjectSoulReadSelectorV1::Exact {
        generation: before.generation,
        revision: before.revision.unwrap(),
        material_digest: before.material_digest.unwrap(),
    };
    let historical = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.subject_id().into(),
            selector: exact.clone(),
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .unwrap();
    assert!(
        matches!(historical, SubjectSoulReadOutcomeV1::Verified { view }
        if !view.source_read_availability.permits_content() && view.runtime_private_core.is_none())
    );
    let historical_projection = runtime
        .project_with_subject_soul_selector(
            projection_request(MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_800_000_000,
            }),
            Some(exact),
        )
        .unwrap();
    assert!(
        !historical_projection
            .provider_payload()
            .system_memory_block()
            .contains(GROWTH),
        "an exact historical revision cannot bypass source withdrawal"
    );
    assert!(
        !historical_projection
            .provider_payload()
            .system_memory_block()
            .contains(RELATIONSHIP_FLOOR),
        "current relationship floors must not cross into historical Soul projections"
    );
    // A repaired Soul remains usable; historical audit lineage must not poison
    // the next ordinary source or reenter its model-visible governance context.
    run_turn(
        &runtime,
        "post-review-operational-turn",
        &LearningModel::new(false),
    );
}

#[test]
fn withdrawn_source_holds_soul_and_real_new_turn_recovers_same_generation_without_history_leak() {
    source_review_recovery_proof(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        false,
    );
}

#[test]
fn file_source_review_pending_and_recovery_survive_store_reopen() {
    let path = std::env::temp_dir().join(format!(
        "beetle-persona-review-file-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    source_review_recovery_proof(
        StoreBackendConfig::file(&path, support::host_test_profile()).unwrap(),
        true,
    );
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_source_review_pending_and_recovery_survive_store_reopen() {
    let root = std::env::temp_dir().join(format!(
        "beetle-persona-review-sqlite-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    source_review_recovery_proof(
        StoreBackendConfig::sqlite(root.join("memory.sqlite"), support::host_test_profile())
            .unwrap(),
        true,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pre_reset_signed_and_queued_turns_never_reauthorize_soul_but_shared_facts_complete() {
    const FACT: &str =
        "The user reported that uncertainty was admitted instead of a fabricated confident answer.";
    struct SharedFactModel;
    impl LlmClient for SharedFactModel {
        fn chat(
            &self,
            _: &mut dyn LlmHttpClient,
            system: &str,
            _: &[Message],
            _: Option<&[ToolSpec]>,
            _: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            assert!(
                !system.contains("persona interpretation"),
                "reset reauthorized old canonical personality evidence"
            );
            assert!(
                !system.contains("private garden"),
                "reset reauthorized old private governance"
            );
            assert!(
                !system.contains("inward autonomy runtime"),
                "reset reauthorized old Soul cycle"
            );
            let content = if system.contains("You extract durable memory updates") {
                serde_json::json!([{"plane":"factual","op":"upsert","kind":"profile",
                    "source_authority":"user_asserted","topic":"reset_independent_fact","content":FACT,
                    "keywords":["uncertainty","reset_independent_fact"]}]).to_string()
            } else {
                "[]".into()
            };
            Ok(LlmResponse {
                content,
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            })
        }
    }
    struct SharedPort;
    impl GovernanceExecutionPort for SharedPort {
        fn execute(
            &mut self,
            _: &AuthorizedGovernanceEnvelope,
            _: &ImmutableGovernanceExecutionBinding,
            egress: &GovernanceEgressAuthority,
            operation: &mut dyn GovernanceExecutionOperation,
        ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
            egress
                .revalidate_before_egress()
                .map_err(GovernanceExecutionPortFailure::Other)?;
            operation
                .run(&mut support::StaticHttpClient, &SharedFactModel)
                .map_err(GovernanceExecutionPortFailure::Other)
        }
    }
    for queued_before_reset in [false, true] {
        let store = support::open_memory_store(
            StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        )
        .unwrap();
        let runtime = runtime_for_store(store, "reset-persona");
        install(&runtime);
        for index in 0..4 {
            run_turn(
                &runtime,
                &format!("prior-growth-{index}"),
                &LearningModel::new(true),
            );
        }
        assert!(runtime
            .project(projection_request(MemoryRecallTemporalOperation::Current))
            .unwrap()
            .provider_payload()
            .system_memory_block()
            .contains(GROWTH));
        let old_request = signed_persona_request(&runtime, "old-execution-across-reset");
        let mut old_job = if queued_before_reset {
            runtime
                .finalize_turn(old_request.clone())
                .unwrap()
                .memory_consolidation
                .job_id
        } else {
            None
        };
        let SubjectSoulReadOutcomeV1::Verified { view } = runtime
            .read_subject_soul(SubjectSoulReadRequestV1 {
                target_subject_id: runtime.subject_id().into(),
                selector: SubjectSoulReadSelectorV1::Current,
                view: SubjectSoulReadViewV1::OperatorSafe,
            })
            .unwrap()
        else {
            panic!("grown Soul must be nonempty before reset")
        };
        let generation = view.generation;
        runtime
            .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
                operation_id: "reset-after-signed-execution".into(),
                target_subject_id: runtime.subject_id().into(),
                expected_state: SubjectSoulExpectedStateV1::Exact {
                    generation,
                    revision: view.revision,
                    lifecycle_state: view.state,
                    head_digest: view.head_digest,
                    manifest_digest: view.manifest_digest,
                },
                authority: SubjectSoulLifecycleAuthorityV1::Destructive {
                    system_actor_subject_id: system_governor_subject_id("persona-learning-owner"),
                    human_confirmation: HumanSoulLifecycleConfirmationV1 {
                        human_subject_id: primary_human_subject_id("persona-learning-owner"),
                        target_subject_id: runtime.subject_id().into(),
                        expected_generation: generation,
                        action: SubjectSoulTerminalActionV1::Reset,
                        reason_code: "explicit_reset".into(),
                        confirmed_at: 1_800_000_000,
                        evidence_digest: "d".repeat(64),
                    },
                },
                action: SubjectSoulLifecycleActionV1::Reset {
                    reason_code: "explicit_reset".into(),
                },
            })
            .unwrap();
        if !queued_before_reset {
            old_job = runtime
                .finalize_turn(old_request.clone())
                .unwrap()
                .memory_consolidation
                .job_id;
        }
        let outcome = MemoryLearningEngine::attach(runtime.clone())
            .unwrap()
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "reset-source-test".into(),
                    lease_duration_secs: 60,
                },
                &mut SharedPort,
            )
            .unwrap();
        let MemoryLearningCycleOutcome::Completed(report) = outcome else {
            panic!("independent memory lane must complete: {outcome:?}")
        };
        assert_eq!(Some(report.job.job_id), old_job);
        assert_eq!(report.semantic_governance.accepted_count, 1);
        assert!(
            matches!(runtime.read_subject_soul(SubjectSoulReadRequestV1 { target_subject_id: runtime.subject_id().into(),
            selector: SubjectSoulReadSelectorV1::Current, view: SubjectSoulReadViewV1::OperatorSafe }).unwrap(),
            SubjectSoulReadOutcomeV1::Verified { view } if view.generation == generation + 1 && view.revision.is_none() && view.state == SubjectSoulLifecycleStateV1::Unseeded)
        );
        let mut query = projection_request(MemoryRecallTemporalOperation::Current);
        query.user_query = "reset_independent_fact uncertainty".into();
        let projected = runtime.project(query).unwrap();
        assert!(
            projected
                .provider_payload()
                .system_memory_block()
                .contains(FACT),
            "reset incorrectly disabled shared long-term memory"
        );
        assert!(!projected
            .provider_payload()
            .system_memory_block()
            .contains(GROWTH));
    }
}
