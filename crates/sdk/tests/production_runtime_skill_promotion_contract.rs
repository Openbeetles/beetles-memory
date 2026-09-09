#![cfg(feature = "sqlite-store")]

mod support;

use bm_sdk::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

const METHOD: &str = "1. Inspect the archive manifest and validate target paths\n2. Unpack archive into the selected workspace\n3. Verify extracted files against the manifest";

struct Clock(AtomicU64);
impl MemoryClock for Clock {
    fn now_secs(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct NoProvider;
impl GovernanceExecutionPort for NoProvider {
    fn execute(
        &mut self,
        _: &AuthorizedGovernanceEnvelope,
        _: &ImmutableGovernanceExecutionBinding,
        _: &GovernanceEgressAuthority,
        _: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        panic!("deterministic procedural promotion must not invoke a Provider")
    }
}

fn registry() -> AgentToolRegistrySnapshot {
    AgentToolRegistrySnapshot::compact(
        "promotion-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "archive.unpack",
            "Unpack archive",
            "schema-archive-v1",
        )],
        1_800_000_000,
    )
}

fn runtime(store: MemoryStoreHandle, clock: Arc<Clock>) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "promotion-chat").unwrap())
        .store(store)
        .clock(clock)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .agent_tool_registry(registry())
        .build()
        .unwrap()
}

fn request(runtime: &MemoryRuntime, turn_id: &str) -> MemoryTurnFinalizeRequest {
    let now = runtime.config().clock.now_secs();
    let observations = ["first-call", "second-call"]
        .into_iter()
        .map(|call| AgentToolObservationDigest {
            observation_id: format!("{turn_id}-{call}-observation"),
            registry_id: "promotion-tools".into(),
            tool_id: "archive.unpack".into(),
            schema_fingerprint: "schema-archive-v1".into(),
            call_id: Some(format!("{turn_id}-{call}")),
            task_signature: "unpack archive".into(),
            summary: METHOD.into(),
            outcome: AgentToolOutcome::Succeeded,
            error_code: None,
            external_content: false,
            private_content_used: false,
            permission_tags: Vec::new(),
            risk_tags: Vec::new(),
            started_at: Some(now),
            completed_at: Some(now),
        })
        .collect::<Vec<_>>();
    let canonical_observations = observations
        .iter()
        .map(|observation| ToolObservationDigest {
            observation_id: observation.observation_id.clone(),
            tool_name: observation.tool_id.clone(),
            summary: observation.summary.clone(),
            external_content: false,
        })
        .collect();
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.into(),
            conversation: ConversationScope {
                channel: "sdk.direct".into(),
                chat_id: "promotion-chat".into(),
                conversation_id: Some("promotion-chat".into()),
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
            input_messages: vec![TranscriptInputMessage::user("unpack archive")],
            assistant_message: Some(TranscriptInputMessage::assistant("Archive verified")),
            tool_observations: canonical_observations,
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        learning: PostTurnLearningInputV1 {
            tool_call_count: 2,
            agent_tool_feedback: vec![AgentToolUsageFeedbackV2 {
                registry_ref: registry().registry_ref(),
                tool_id: "archive.unpack".into(),
                schema_fingerprint: "schema-archive-v1".into(),
                observations,
                outcome: ProceduralExecutionOutcomeV1::Succeeded,
                user_visible_result_summary: Some("Archive verified".into()),
                operator_note: None,
            }],
            ..PostTurnLearningInputV1::empty()
        },
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn project(runtime: &MemoryRuntime, turn_id: &str) -> MemoryProjectionOutput {
    runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: turn_id.into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "unpack archive".into(),
            system_max_len: 16_384,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: vec![registry().registry_ref()],
        })
        .unwrap()
}

fn skills(runtime: &MemoryRuntime) -> Vec<RuntimeSkillSummary> {
    runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: runtime.subject_id().into(),
            },
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 10,
        })
        .unwrap()
        .skills
}

fn complete_request(
    memory: &Arc<MemoryRuntime>,
    input: MemoryTurnFinalizeRequest,
    accepted: u32,
) -> (String, u32) {
    let finalized = memory.finalize_turn(input).unwrap();
    let expected_job = finalized.procedural_learning.job_id.unwrap();
    let outcome = MemoryLearningEngine::attach(memory.clone())
        .unwrap()
        .run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "promotion-worker".into(),
                lease_duration_secs: 60,
            },
            &mut NoProvider,
        )
        .unwrap();
    let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
        panic!("official procedural completion required: {outcome:?}");
    };
    assert_eq!(report.job.job_id, expected_job);
    assert_eq!(report.receipt.accepted_count, accepted);
    (expected_job, report.receipt.changed_count)
}

fn method_request(
    runtime: &MemoryRuntime,
    turn_id: &str,
    method: &str,
) -> MemoryTurnFinalizeRequest {
    let mut input = request(runtime, turn_id);
    for observation in &mut input.turn.tool_observations {
        observation.summary = method.into();
    }
    for feedback in &mut input.learning.agent_tool_feedback {
        for observation in &mut feedback.observations {
            observation.summary = method.into();
        }
    }
    input
}

fn test_directory(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "bm-runtime-promotion-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn two_accepted_production_turns_create_one_runtime_skill_and_project_it_after_reopen() {
    let eligibility = bm_core::skills::govern_runtime_skill_write_shapes(
        &[RuntimeSkillWrite {
            name: String::new(),
            topic: "unpack archive".into(),
            title: "Inspect the archive manifest and validate target paths".into(),
            summary: "Inspect the archive manifest and validate target paths".into(),
            content: METHOD.into(),
            citations: Vec::new(),
            source_chat_id: Some("promotion-chat".into()),
            observed_at: 1_800_000_000,
        }],
        bm_core::skills::RuntimeSkillWriteSource::Extraction,
    );
    assert_eq!(
        eligibility.accepted, 1,
        "actual observation method must pass the unchanged Core structure gate: {eligibility:?}"
    );
    let path = std::env::temp_dir().join(format!(
        "bm-production-runtime-promotion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    let config =
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap();
    let store = support::open_memory_store(config.clone()).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = Arc::new(runtime(store.clone(), clock.clone()));
    assert!(
        skills(&memory).is_empty(),
        "the production Store starts empty"
    );
    for turn_id in ["method-source-one", "method-source-two"] {
        let finalized = memory.finalize_turn(request(&memory, turn_id)).unwrap();
        let expected_job = finalized.procedural_learning.job_id.unwrap();
        let engine = MemoryLearningEngine::attach(memory.clone()).unwrap();
        let outcome = engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "promotion-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap();
        let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
            panic!("official procedural worker must complete: {outcome:?}");
        };
        assert_eq!(report.job.job_id, expected_job);
        assert_eq!(report.receipt.accepted_count, 1);
        assert!(
            report.receipt.changed_count > 0,
            "real accepted owner mutation"
        );
        clock.0.fetch_add(2, Ordering::SeqCst);
        let delivered = project(&memory, "next-execution");
        assert!(
            !delivered.provider_payload().agent_tool_hints().is_empty(),
            "each source produces real governed AgentTool experience, not an empty fixture"
        );
        if turn_id == "method-source-one" {
            assert!(
                skills(&memory).is_empty(),
                "one turn is not repeated evidence"
            );
        }
    }
    let promoted = skills(&memory);
    assert_eq!(promoted.len(), 1, "two accepted independent turns must create one Runtime owner without a seed or public create");
    let locator = promoted[0].locator.clone();
    assert_eq!(locator.owner_revision(), 1);
    assert!(promoted[0].enabled);
    let delivered = project(&memory, "use-promoted-method");
    let receipt = delivered.report().selection_receipt().unwrap();
    assert!(receipt
        .runtime_skills
        .iter()
        .any(|selected| selected.locator == locator));
    assert!(delivered
        .provider_payload()
        .system_memory_block()
        .contains("Verify extracted files"));
    complete_request(&memory, request(&memory, "method-source-three"), 1);
    assert_eq!(
        skills(&memory)[0].locator,
        locator,
        "a third source never creates or rewrites revision one"
    );
    clock.0.fetch_add(2, Ordering::SeqCst);
    let use_projection = project(&memory, "promoted-usage");
    let selection = use_projection.report().selection_receipt().unwrap().clone();
    let selected = selection
        .runtime_skills
        .iter()
        .find(|selected| selected.locator == locator)
        .unwrap()
        .clone();
    let mut use_request = request(&memory, "promoted-usage");
    use_request.turn.tool_observations.clear();
    use_request.learning = PostTurnLearningInputV1 {
        selection_receipt: Some(selection),
        runtime_skill_feedback: vec![RuntimeSkillUsageFeedbackV1 {
            locator: selected.locator,
            selected_content_digest: selected.content_digest,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "execution-promoted-usage".into(),
        }],
        ..PostTurnLearningInputV1::empty()
    };
    assert_eq!(complete_request(&memory, use_request, 1).1, 1);
    let used = skills(&memory).remove(0);
    assert_eq!(used.locator.owner_revision(), locator.owner_revision() + 1);
    assert_eq!(used.validated_success_count, 1);
    drop(memory);
    drop(store);
    let reopened_store = support::open_memory_store(config).unwrap();
    let reopened = runtime(reopened_store.clone(), clock);
    assert_eq!(skills(&reopened)[0].locator, used.locator);
    let reopened_projection = project(&reopened, "use-after-reopen");
    assert!(reopened_projection
        .report()
        .selection_receipt()
        .unwrap()
        .runtime_skills
        .iter()
        .any(|selected| selected.locator == used.locator));
    drop(reopened);
    drop(reopened_store);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn production_promotion_keeps_one_owner_and_its_sources_on_three_backends() {
    let path = test_directory("backends");
    let profile = support::host_test_profile();
    for config in [
        StoreBackendConfig::in_memory(profile).unwrap(),
        StoreBackendConfig::file(path.join("file"), profile).unwrap(),
        StoreBackendConfig::sqlite(path.join("sqlite.db"), profile).unwrap(),
    ] {
        let store = support::open_memory_store(config.clone()).unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let memory = Arc::new(runtime(store.clone(), clock.clone()));
        assert!(skills(&memory).is_empty());
        for turn in ["backend-first", "backend-second"] {
            let (_, changed) = complete_request(&memory, request(&memory, turn), 1);
            assert!(changed > 0);
            clock.0.fetch_add(2, Ordering::SeqCst);
        }
        let before = skills(&memory);
        assert_eq!(
            before.len(),
            1,
            "each backend materializes the same real production owner"
        );
        assert_eq!(before[0].locator.owner_revision(), 1);
        complete_request(&memory, request(&memory, "backend-third"), 1);
        assert_eq!(skills(&memory)[0].locator, before[0].locator);
        if config.backend() != StoreBackendKind::InMemory {
            drop(memory);
            drop(store);
            let reopened = support::open_memory_store(config).unwrap();
            let memory = runtime(reopened.clone(), clock);
            assert_eq!(skills(&memory)[0].locator, before[0].locator);
            assert!(skills(&memory)[0].enabled);
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn weak_different_method_model_and_private_sources_do_not_create_runtime_skills() {
    let path = test_directory("negative");
    for case in ["weak", "different-method", "model", "private", "external"] {
        let config = StoreBackendConfig::sqlite(
            path.join(format!("{case}.db")),
            support::host_test_profile(),
        )
        .unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let memory = Arc::new(runtime(
            support::open_memory_store(config).unwrap(),
            clock.clone(),
        ));
        // An unrelated valid source is a real nonempty experience control, but
        // cannot satisfy the other case's exact task/authority/method evidence.
        complete_request(&memory, request(&memory, "positive-control"), 1);
        assert!(!project(&memory, "before-negative")
            .provider_payload()
            .agent_tool_hints()
            .is_empty());
        assert!(skills(&memory).is_empty());
        for turn in ["negative-one", "negative-two"] {
            clock.0.fetch_add(2, Ordering::SeqCst);
            let method = if case == "weak" {
                "archive unpack completed successfully"
            } else if case == "different-method" && turn == "negative-one" {
                "1. Read the zip manifest\n2. Extract zip entries into a sandbox\n3. Check each extracted checksum"
            } else {
                METHOD
            };
            let mut input = method_request(&memory, turn, method);
            for feedback in &mut input.learning.agent_tool_feedback {
                for observation in &mut feedback.observations {
                    observation.task_signature = "negative-specific-task".into();
                    observation.private_content_used = case == "private";
                    observation.external_content = case == "external";
                }
            }
            for observation in &mut input.turn.tool_observations {
                observation.external_content = case == "external";
            }
            input.turn.external_content_used = case == "external";
            if case == "model" {
                input.learning.authority = ProceduralFeedbackAuthorityInputV1::ModelInferred;
            }
            complete_request(&memory, input, if case == "private" { 0 } else { 1 });
        }
        assert!(
            skills(&memory).is_empty(),
            "{case} cannot acquire promotion authority"
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn withdrawing_an_earlier_promotion_source_retires_current_and_historical_projection() {
    let path = test_directory("withdrawal");
    let config =
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap();
    let store = support::open_memory_store(config.clone()).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = Arc::new(runtime(store.clone(), clock.clone()));
    for turn in ["old-source", "promotion-source"] {
        complete_request(&memory, request(&memory, turn), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    let positive = project(&memory, "before-withdrawal");
    assert!(!positive
        .report()
        .selection_receipt()
        .unwrap()
        .runtime_skills
        .is_empty());
    let before = skills(&memory).remove(0);
    let as_of_time = clock.now_secs();
    let archive_scope =
        MemoryArchiveScope::subject(memory.memory_space_id(), memory.subject_id()).unwrap();
    let archive = memory
        .export_memory_space(MemorySpaceExportRequest {
            scope: archive_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .unwrap()
        .archive;
    clock.0.fetch_add(2, Ordering::SeqCst);
    memory
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: memory.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: "promotion-chat".into(),
            turn_id: Some("old-source".into()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "synthetic earlier-source withdrawal".into(),
        })
        .unwrap();
    let retired = skills(&memory).remove(0);
    assert!(!retired.enabled);
    assert!(retired.locator.owner_revision() > before.locator.owner_revision());
    memory
        .import_memory_space(MemorySpaceImportRequest {
            scope: archive_scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive,
        })
        .unwrap();
    assert_eq!(skills(&memory)[0].locator, retired.locator);
    for temporal_operation in [
        MemoryRecallTemporalOperation::Current,
        MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
    ] {
        let denied = memory
            .project(MemoryProjectionRequest {
                binding: match temporal_operation {
                    MemoryRecallTemporalOperation::Current => ProceduralProjectionBindingV1::Turn {
                        turn_id: "after-withdrawal".into(),
                    },
                    MemoryRecallTemporalOperation::HistoricalAsOf { .. } => {
                        ProceduralProjectionBindingV1::Preview
                    }
                },
                temporal_operation,
                user_query: "unpack archive".into(),
                system_max_len: 16_384,
                recent_messages_limit: 0,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: Vec::new(),
                tool_registry_refs: vec![registry().registry_ref()],
            })
            .unwrap();
        assert!(denied
            .report()
            .selection_receipt()
            .is_none_or(|receipt| receipt.runtime_skills.is_empty()));
        assert!(!denied
            .provider_payload()
            .system_memory_block()
            .contains("Verify extracted files"));
    }
    drop(memory);
    drop(store);
    let reopened = support::open_memory_store(config).unwrap();
    let memory = runtime(reopened.clone(), clock);
    assert_eq!(skills(&memory)[0].locator, retired.locator);
    assert!(!skills(&memory)[0].enabled);
    drop(memory);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn scoped_production_promotions_keep_project_workspace_and_conversation_boundaries() {
    let path = test_directory("scope");
    for scope in [
        AgentToolRegistryScope::Project {
            project_id: "project-a".into(),
        },
        AgentToolRegistryScope::Workspace {
            workspace_id: "workspace-a".into(),
        },
        AgentToolRegistryScope::Conversation {
            conversation_id: "conversation-a".into(),
        },
    ] {
        let mut scoped_registry = registry();
        scoped_registry.scope = scope.clone();
        scoped_registry.fingerprint =
            bm_core::skills::fingerprint_agent_tool_registry(&scoped_registry);
        let label = match scope {
            AgentToolRegistryScope::Project { .. } => "project",
            AgentToolRegistryScope::Workspace { .. } => "workspace",
            _ => "conversation",
        };
        let store = support::open_memory_store(
            StoreBackendConfig::sqlite(
                path.join(format!("{label}.db")),
                support::host_test_profile(),
            )
            .unwrap(),
        )
        .unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let build = |matching: bool| {
            let conversation = if matching {
                "conversation-a"
            } else {
                "conversation-b"
            };
            MemoryRuntime::builder()
                .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
                .scope(
                    MemoryScope::new("sdk.direct", "promotion-chat")
                        .unwrap()
                        .with_conversation_id(conversation)
                        .unwrap(),
                )
                .store(store.clone())
                .clock(clock.clone())
                .capability_policy(MemoryCapabilityPolicy::strict_profile())
                .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
                .agent_tool_registry(scoped_registry.clone())
                .procedural_applicability_context(
                    ProceduralApplicabilityContextV1::try_new(
                        Some(if matching { "project-a" } else { "project-b" }.into()),
                        Some(
                            if matching {
                                "workspace-a"
                            } else {
                                "workspace-b"
                            }
                            .into(),
                        ),
                        Some(conversation.into()),
                    )
                    .unwrap(),
                )
                .build()
                .unwrap()
        };
        let memory = Arc::new(build(true));
        for turn in ["scope-first", "scope-second"] {
            let mut input = request(&memory, turn);
            input.turn.conversation.conversation_id = Some("conversation-a".into());
            input.learning.agent_tool_feedback[0].registry_ref = scoped_registry.registry_ref();
            complete_request(&memory, input, 1);
            clock.0.fetch_add(2, Ordering::SeqCst);
        }
        let created = skills(&memory);
        assert_eq!(
            created.len(),
            1,
            "typed {label} owner exists before delivery checks"
        );
        let scoped_project = |runtime: &MemoryRuntime| {
            runtime
                .project(MemoryProjectionRequest {
                    binding: ProceduralProjectionBindingV1::Turn {
                        turn_id: "scope-use".into(),
                    },
                    temporal_operation: MemoryRecallTemporalOperation::Current,
                    user_query: "unpack archive".into(),
                    system_max_len: 16_384,
                    recent_messages_limit: 0,
                    pressure: PressureLevel::Normal,
                    mode_input: RuntimeLifecycleModeInput::default(),
                    structured_query_facets: Vec::new(),
                    tool_registry_refs: vec![scoped_registry.registry_ref()],
                })
                .unwrap()
        };
        let positive = scoped_project(&memory);
        assert!(
            positive
                .report()
                .selection_receipt()
                .unwrap()
                .runtime_skills
                .iter()
                .any(|selection| selection.locator == created[0].locator),
            "real matching {label} context must deliver the production Runtime owner"
        );
        let denied = scoped_project(&build(false));
        assert!(
            denied
                .report()
                .selection_receipt()
                .is_none_or(|receipt| receipt.runtime_skills.is_empty()),
            "wrong {label} cannot receive the owner"
        );
        assert!(!denied
            .provider_payload()
            .system_memory_block()
            .contains("Verify extracted files"));
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn production_sources_from_different_subjects_never_combine_into_a_runtime_skill() {
    let path = test_directory("subjects");
    let store = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let mut subjects =
        SubjectRegistry::single_agent_default("promotion-owner", "promotion-agent").unwrap();
    subjects
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Agent B",
        ))
        .unwrap();
    let build = |agent: &str, chat: &str| {
        Arc::new(
            MemoryRuntime::builder()
                .identity(MemoryIdentity::new(agent, "promotion-owner").unwrap())
                .scope(MemoryScope::new("sdk.direct", chat).unwrap())
                .subject_registry(subjects.clone())
                .store(store.clone())
                .clock(clock.clone())
                .capability_policy(MemoryCapabilityPolicy::strict_profile())
                .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
                .agent_tool_registry(registry())
                .build()
                .unwrap(),
        )
    };
    let a = build("promotion-agent", "promotion-chat");
    let b = build("agent-b", "other-chat");
    complete_request(&a, request(&a, "a-first"), 1);
    let mut b_input = request(&b, "b-first");
    b_input.turn.conversation.chat_id = "other-chat".into();
    b_input.turn.conversation.conversation_id = Some("other-chat".into());
    complete_request(&b, b_input, 1);
    assert!(skills(&a).is_empty());
    assert!(
        skills(&b).is_empty(),
        "two subjects' independent accepted turns do not form repeated evidence"
    );
    clock.0.fetch_add(2, Ordering::SeqCst);
    complete_request(&a, request(&a, "a-second"), 1);
    assert_eq!(
        skills(&a).len(),
        1,
        "same-subject second source is a nonempty positive control"
    );
    assert!(skills(&b).is_empty());
    assert!(project(&b, "b-use")
        .report()
        .selection_receipt()
        .is_none_or(|receipt| receipt.runtime_skills.is_empty()));
    drop(a);
    drop(b);
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn runtime_builder_rejects_duplicate_project_workspace_conversation_authorities_before_writes() {
    for target in [
        RuntimeSkillApplicabilityTarget::Project {
            project_id: "project-a".into(),
        },
        RuntimeSkillApplicabilityTarget::Workspace {
            workspace_id: "workspace-a".into(),
        },
        RuntimeSkillApplicabilityTarget::Conversation {
            conversation_id: "conversation-a".into(),
        },
    ] {
        let store = support::empty_store_platform(support::host_test_profile());
        let before = store.export_replay_snapshot().unwrap();
        let result = MemoryRuntime::builder()
            .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "promotion-chat").unwrap())
            .store(store.clone())
            .runtime_skill_applicability_context(
                RuntimeSkillApplicabilityContext::try_new(vec![target.clone()]).unwrap(),
            )
            .build();
        assert!(result.is_err(), "{target:?} has only the procedural applicability owner, not a second Runtime builder authority");
        assert_eq!(
            store.export_replay_snapshot().unwrap(),
            before,
            "invalid constructor must not initialize signing keys or any owner"
        );
        let valid = MemoryRuntime::builder()
            .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "promotion-chat").unwrap())
            .store(store)
            .runtime_skill_applicability_context(
                RuntimeSkillApplicabilityContext::try_new(vec![
                    RuntimeSkillApplicabilityTarget::User {
                        user_ref: "human-owner".into(),
                    },
                    RuntimeSkillApplicabilityTarget::Organization {
                        organization_id: "organization-a".into(),
                    },
                    RuntimeSkillApplicabilityTarget::Device {
                        device_id: "device-a".into(),
                    },
                ])
                .unwrap(),
            )
            .build();
        assert!(
            valid.is_ok(),
            "remaining typed Runtime applicability remains usable"
        );
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn file_and_sqlite_reopen_reject_forged_runtime_promotion_source_commitments() {
    use bm_core::skills::{
        RuntimeSkillOwnerBinding, RuntimeSkillOwnerRecord, RuntimeSkillScopeManifest,
    };
    let root = test_directory("source-open");
    let profile = support::host_test_profile();
    for backend in ["file", "sqlite"] {
        for corruption in [
            "unknown-source",
            "other-subject-source",
            "generic-source-kind",
        ] {
            let case = root.join(format!("{backend}-{corruption}"));
            std::fs::create_dir_all(&case).unwrap();
            let store_path = if backend == "file" {
                case.join("store")
            } else {
                case.join("store.db")
            };
            let config = if backend == "file" {
                StoreBackendConfig::file(&store_path, profile).unwrap()
            } else {
                StoreBackendConfig::sqlite(&store_path, profile).unwrap()
            };
            let store = support::open_memory_store(config.clone()).unwrap();
            let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
            let memory = Arc::new(runtime(store.clone(), clock.clone()));
            for turn in ["open-first", "open-second"] {
                complete_request(&memory, request(&memory, turn), 1);
                clock.0.fetch_add(2, Ordering::SeqCst);
            }
            assert_eq!(
                skills(&memory).len(),
                1,
                "real production promotion positive"
            );
            let mut subjects =
                SubjectRegistry::single_agent_default("promotion-owner", "promotion-agent")
                    .unwrap();
            subjects
                .upsert_subject(SubjectDescriptor::agent_persona(
                    default_agent_subject_id("agent-b"),
                    "Agent B",
                ))
                .unwrap();
            let other = Arc::new(
                MemoryRuntime::builder()
                    .identity(MemoryIdentity::new("agent-b", "promotion-owner").unwrap())
                    .scope(MemoryScope::new("sdk.direct", "other-chat").unwrap())
                    .subject_registry(subjects)
                    .store(store.clone())
                    .clock(clock.clone())
                    .capability_policy(MemoryCapabilityPolicy::strict_profile())
                    .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
                    .agent_tool_registry(registry())
                    .build()
                    .unwrap(),
            );
            let mut other_input = request(&other, "other-subject-source");
            other_input.turn.conversation.chat_id = "other-chat".into();
            other_input.turn.conversation.conversation_id = Some("other-chat".into());
            let other_job_id = complete_request(&other, other_input, 1).0;
            let snapshot = store.export_replay_snapshot().unwrap();
            let owner_doc = snapshot
                .json_docs
                .iter()
                .find(|doc| doc.namespace == "runtime_skill_records")
                .unwrap();
            let owner: RuntimeSkillOwnerRecord =
                serde_json::from_value(owner_doc.value.clone()).unwrap();
            let manifest_doc = snapshot
                .json_docs
                .iter()
                .find(|doc| {
                    doc.namespace == "runtime_skill_scope_manifests"
                        && doc.value["memory_space_id"] == owner.memory_space_id
                        && doc.value["owning_scope"]
                            == serde_json::to_value(&owner.owning_scope).unwrap()
                })
                .unwrap();
            let manifest: RuntimeSkillScopeManifest =
                serde_json::from_value(manifest_doc.value.clone()).unwrap();
            let mut intrinsic = owner.intrinsic_contract.clone();
            match corruption {
                "unknown-source" => {
                    intrinsic.evidence_bindings[0].safe_ref =
                        format!("procedural_feedback_job:sha256:{}", "f".repeat(64))
                }
                "other-subject-source" => {
                    let other_job = snapshot
                        .json_docs
                        .iter()
                        .find(|doc| {
                            doc.namespace == "procedural_feedback_jobs" && doc.key == other_job_id
                        })
                        .unwrap();
                    intrinsic.evidence_bindings[0].safe_ref = other_job_id;
                    intrinsic.evidence_bindings[0].source_digest = other_job.value
                        ["learning_evidence_digest"]
                        .as_str()
                        .unwrap()
                        .into();
                }
                _ => {
                    intrinsic.evidence_bindings[0].kind = RuntimeSkillEvidenceKind::GovernedEvidence
                }
            }
            let forged = RuntimeSkillOwnerRecord::build(
                &owner.memory_space_id,
                owner.owning_scope.clone(),
                owner.creation_ref.clone(),
                owner.owner_revision,
                intrinsic,
                owner.procedural_content.clone(),
                owner.lifecycle.clone(),
                owner.privacy_class,
            )
            .unwrap();
            let forged_manifest = RuntimeSkillScopeManifest::build(
                manifest.revision,
                &manifest.memory_space_id,
                manifest.owning_scope.clone(),
                vec![RuntimeSkillOwnerBinding::from_record(&forged).unwrap()],
                memory
                    .runtime_budget()
                    .governed_state_budget
                    .max_retained_runtime_skill_owners_per_scope,
            )
            .unwrap();
            let replacements = [
                (
                    owner_doc.namespace.clone(),
                    owner_doc.key.clone(),
                    owner_doc.value.clone(),
                    serde_json::to_value(forged).unwrap(),
                ),
                (
                    manifest_doc.namespace.clone(),
                    manifest_doc.key.clone(),
                    manifest_doc.value.clone(),
                    serde_json::to_value(forged_manifest).unwrap(),
                ),
            ];
            drop(other);
            drop(memory);
            drop(store);
            let positive_reopen = support::open_memory_store(config.clone()).unwrap();
            assert!(positive_reopen
                .export_replay_snapshot()
                .unwrap()
                .json_docs
                .iter()
                .any(|doc| doc.namespace == "runtime_skill_records"));
            drop(positive_reopen);
            // Deliberate corruption of only this synthetic backend's two exact
            // current-owner documents. Historical ledger/MOR/source rows stay intact.
            if backend == "sqlite" {
                let connection = rusqlite::Connection::open(&store_path).unwrap();
                for (namespace, key, _, after) in &replacements {
                    assert_eq!(connection.execute("UPDATE bm_kv SET value_json = ?1 WHERE namespace = ?2 AND key = ?3",
                        rusqlite::params![serde_json::to_string(after).unwrap(), namespace, key]).unwrap(), 1);
                }
            } else {
                for (namespace, _, before, after) in &replacements {
                    let mut exact_path = None;
                    for shard in
                        std::fs::read_dir(store_path.join("kv").join(namespace).join("_v2"))
                            .unwrap()
                    {
                        for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
                            let path = entry.unwrap().path();
                            let value: serde_json::Value =
                                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                            if value == *before {
                                assert!(
                                    exact_path.replace(path).is_none(),
                                    "one exact physical owner address"
                                );
                            }
                        }
                    }
                    std::fs::write(
                        exact_path.expect("validated exact synthetic document"),
                        serde_json::to_vec(after).unwrap(),
                    )
                    .unwrap();
                }
            }
            let result = support::open_memory_store(config);
            assert!(result.is_err(), "{backend} open must reject {corruption} even with canonical current owner/manifest digests and intact historical ledgers");
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
