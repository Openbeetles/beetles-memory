mod support;

use bm_sdk::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const MATERIAL: &str = "Calculate the file checksum and compare it with an authenticated expected checksum; reject on mismatch.";

struct LearningModel(AtomicUsize);
impl LlmClient for LearningModel {
    fn chat(
        &self,
        _: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        _: Option<&[ToolSpec]>,
        _: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let content = if system.contains("method learning extraction") {
            self.0.fetch_add(1, Ordering::SeqCst);
            assert!(messages
                .iter()
                .any(|message| message.content.contains(MATERIAL)));
            r#"[{"task_signature":"verify_file","procedure":"1. Calculate the file checksum.\n2. Compare it with the authenticated expected checksum; reject on mismatch.","constraints":["Use an authenticated expected checksum."],"source_material_ids":["observed-strategy"]}]"#
        } else if system.contains("private garden") {
            "null"
        } else if system.contains("inward autonomy runtime") {
            "{}"
        } else if system.contains("persona interpretation") {
            r#"{"decision":"operational_only"}"#
        } else {
            "[]"
        };
        Ok(LlmResponse {
            content: content.into(),
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
            .map_err(GovernanceExecutionPortFailure::Other)
    }
}

fn production_learning(config: StoreBackendConfig, persistent: bool, failed_task: bool) {
    production_learning_with_active_receipt(config, persistent, failed_task, false, false);
}

fn production_learning_with_active_receipt(
    config: StoreBackendConfig,
    persistent: bool,
    failed_task: bool,
    active_receipt: bool,
    successive_source_deletion: bool,
) {
    let store = support::open_memory_store(config.clone()).unwrap();
    let registry = AgentToolRegistrySnapshot::compact(
        "production-tools",
        "executor",
        vec![AgentToolDescriptor::compact(
            "verify_file",
            "Verify file",
            "schema-checksum",
        )],
        1_800_000_000,
    );
    let runtime = Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("learning-agent", "learning-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "learning-chat").unwrap())
            .store(store.clone())
            .clock(support::fixed_clock(1_800_000_000))
            .agent_tool_registry(registry.clone())
            .build()
            .unwrap(),
    );
    let governor = support::procedural::governor(&runtime, store.clone());
    let mut spec = support::procedural::runtime_observer_spec(&runtime, "strategy-observer");
    spec.claims.method_learning_materials = true;
    spec.claims.method_declarations = false;
    spec.claims.method_adoptions = true;
    spec.claims.method_task_evaluations = true;
    spec.method_evaluators = vec![MethodTaskEvaluatorV1 {
        id: "checksum-verifier".into(),
        revision: 1,
    }];
    let capability =
        support::procedural::register_and_issue(&governor, spec, "register-strategy-observer");
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
    let mut learning = PostTurnLearningInputV3::empty();
    learning.method_learning_inputs.push(MethodLearningInputV1 {
        registry_ref: registry.registry_ref(),
        tool_id: "verify_file".into(),
        schema_fingerprint: "schema-checksum".into(),
        materials: vec![MethodLearningMaterialV1 {
            material_id: "observed-strategy".into(),
            purpose: MethodLearningMaterialPurposeV1::TaskConstraint,
            source: MethodLearningMaterialSourceV1::CanonicalInputMessage { input_index: 0 },
            text: MATERIAL.into(),
            sensitivity: ProceduralSourceSensitivity::NonPrivate,
            external_content: false,
        }],
    });
    runtime
        .finalize_turn_with_procedural_evidence(
            &capability,
            MemoryTurnFinalizeRequest {
                turn: CanonicalTurnDelta {
                    turn_id: "strategy-turn".into(),
                    conversation: ConversationScope {
                        channel: "sdk.direct".into(),
                        chat_id: "learning-chat".into(),
                        conversation_id: Some("learning-chat".into()),
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
                    input_messages: vec![TranscriptInputMessage::user(MATERIAL)],
                    assistant_messages: vec![bm_sdk::TranscriptAssistantMessage::complete(
                        TranscriptInputMessage::assistant("I verified the file."),
                    )],
                    tool_observations: vec![],
                    external_content_used: false,
                    candidate_ids: vec![],
                },
                learning,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .unwrap();
    let model = LearningModel(AtomicUsize::new(0));
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: "learning-chat".into(),
            turn_id: Some("strategy-turn".into()),
            transition: TranscriptLifecycleTransition::Archive,
            reason: "archive_without_changing_evidence".into(),
        })
        .unwrap();
    let engine = MemoryLearningEngine::attach(runtime.clone()).unwrap();
    let outcome = engine
        .run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "production-learning-test".into(),
                lease_duration_secs: 60,
            },
            &mut Port(&model),
        )
        .unwrap();
    assert!(
        model.0.load(Ordering::SeqCst) > 0,
        "public learning engine never requested extraction: {outcome:?}"
    );
    let MemoryLearningCycleOutcome::Completed(report) = outcome else {
        panic!("expected semantic completion: {outcome:?}");
    };
    assert!(matches!(report.job.receipt.unwrap().method_application,
        PostTurnGovernanceMethodApplicationV1::Applied { owner_bindings, .. } if !owner_bindings.is_empty()));
    let candidates = runtime
        .discover_method_trials(MemoryMethodTrialRequest {
            tool: bm_core::memory::ProceduralProducerToolV1 {
                registry_ref: registry.registry_ref(),
                tool_id: "verify_file".into(),
                schema_fingerprint: "schema-checksum".into(),
            },
            max_candidates: 4,
        })
        .unwrap();
    assert!(!candidates.candidates.is_empty());
    let candidate = &candidates.candidates[0];
    assert_eq!(candidate.status, AgentToolExperienceStatus::Candidate);
    assert!(candidate.procedure.contains("reject on mismatch"));
    assert_eq!(candidate.method.owner_revision.owner_revision, 1);
    let projection = runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "verify_file".into(),
            system_max_len: 8192,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .unwrap();
    assert!(!projection
        .provider_payload()
        .system_memory_block()
        .contains(&candidate.procedure));
    let original = candidate.method.clone();
    let procedure = candidate.procedure.clone();
    for index in 0..if failed_task || active_receipt { 3 } else { 2 } {
        let turn_id = format!("apply-method-{index}");
        let active_projection = if active_receipt && index == 2 {
            Some(
                runtime
                    .project(MemoryProjectionRequest {
                        binding: ProceduralProjectionBindingV1::Turn {
                            turn_id: turn_id.clone(),
                        },
                        temporal_operation: MemoryRecallTemporalOperation::Current,
                        user_query: "verify_file".into(),
                        system_max_len: 16_384,
                        recent_messages_limit: 8,
                        pressure: PressureLevel::Normal,
                        mode_input: RuntimeLifecycleModeInput::default(),
                        structured_query_facets: vec![],
                        tool_registry_refs: vec![registry.registry_ref()],
                    })
                    .unwrap(),
            )
        } else {
            None
        };
        let method_reference = if let Some(projected) = &active_projection {
            let receipt = projected
                .report()
                .selection_receipt()
                .expect("actual Turn projection must carry signed selection authority");
            let selected = receipt
                .agent_tool_experiences
                .iter()
                .find(|binding| binding.tool_id == "verify_file")
                .expect(
                    "nonempty active-method projection must expose its exact method commitment",
                );
            assert!(
                selected.experience_revision > 1,
                "fresh consumer must obtain the Active revision, not reuse the Candidate"
            );
            let mut foreign = selected.clone();
            foreign.experience_revision += 1;
            assert!(
                MethodContentRefV1::from_selection(receipt, &foreign).is_err(),
                "consumer cannot substitute an unselected revision"
            );
            let mut tampered_receipt = receipt.clone();
            tampered_receipt.selection_digest = "sha256:forged".into();
            assert!(
                MethodContentRefV1::from_selection(&tampered_receipt, selected).is_err(),
                "noncanonical receipt cannot mint a method reference"
            );
            MethodContentRefV1::from_selection(receipt, selected).unwrap()
        } else {
            original.clone()
        };
        let fact = ToolExecutionFactV1 {
            observation_id: format!("observation-{index}"),
            call_id: format!("call-{index}"),
            outcome: ToolExecutionOutcome::Succeeded,
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
            started_at: Some(1_800_000_000),
            completed_at: Some(1_800_000_000),
        };
        let refs = vec![fact.observation_id.clone()];
        let mut learning = PostTurnLearningInputV3::empty();
        learning.tool_call_count = 1;
        learning.selection_receipt = active_projection
            .as_ref()
            .and_then(|projection| projection.report().selection_receipt())
            .cloned();
        learning.agent_tool_feedback.push(AgentToolUsageFeedbackV3 {
            registry_ref: registry.registry_ref(),
            tool_id: "verify_file".into(),
            schema_fingerprint: "schema-checksum".into(),
            execution_facts: vec![fact.clone()],
            method_evidence: vec![],
        });
        learning.method_adoptions.push(MethodAdoptionEvidenceV1 {
            method: method_reference.clone(),
            task_id: turn_id.clone(),
            adoption: MethodAdoptionStatusV1::Adopted,
            observation_refs: refs.clone(),
        });
        learning
            .method_task_evaluations
            .push(MethodTaskEvaluationV1 {
                method: method_reference.clone(),
                task_id: turn_id.clone(),
                task_root_id: format!("independent-root-{index}"),
                independence_group: format!("independent-scenario-{index}"),
                evaluator_id: "checksum-verifier".into(),
                evaluator_revision: 1,
                outcome: if failed_task && index == 2 {
                    MethodTaskOutcomeV1::Failed
                } else {
                    MethodTaskOutcomeV1::Passed
                },
                observation_refs: refs,
            });
        runtime
            .finalize_turn_with_procedural_evidence(
                &capability,
                MemoryTurnFinalizeRequest {
                    turn: CanonicalTurnDelta {
                        turn_id,
                        conversation: ConversationScope {
                            channel: "sdk.direct".into(),
                            chat_id: "learning-chat".into(),
                            conversation_id: Some("learning-chat".into()),
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
                        input_messages: vec![TranscriptInputMessage::user(
                            "Verify this file using the selected method.",
                        )],
                        assistant_messages: vec![bm_sdk::TranscriptAssistantMessage::complete(
                            TranscriptInputMessage::assistant(
                                if failed_task && index == 2 { "The verifier rejected the result despite successful tool execution." } else { "The independent checksum verifier accepted the result." },
                            ),
                        )],
                        tool_observations: support::procedural::canonical_observations(
                            "verify_file",
                            &[fact],
                        ),
                        external_content_used: false,
                        candidate_ids: vec![],
                    },
                    learning,
                    pressure: PressureLevel::Normal,
                    mode_input: RuntimeLifecycleModeInput::default(),
                },
            )
            .unwrap();
        let mut applied = false;
        for _ in 0..3 {
            let outcome = engine
                .run_due_cycle(
                    MemoryLearningCycleRequest {
                        lease_owner: "production-learning-test".into(),
                        lease_duration_secs: 60,
                    },
                    &mut Port(&model),
                )
                .unwrap();
            match outcome {
                MemoryLearningCycleOutcome::ProceduralCompleted(_) => applied = true,
                MemoryLearningCycleOutcome::Completed(_) => {}
                MemoryLearningCycleOutcome::Idle { .. } => break,
                other => {
                    panic!("task application must complete through the official engine: {other:?}")
                }
            }
        }
        assert!(
            applied,
            "real adoption and independent evaluation were never applied"
        );
        let projection = runtime
            .project(MemoryProjectionRequest {
                binding: ProceduralProjectionBindingV1::Preview,
                temporal_operation: MemoryRecallTemporalOperation::Current,
                user_query: "verify_file".into(),
                system_max_len: 16_384,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: vec![],
                tool_registry_refs: vec![registry.registry_ref()],
            })
            .unwrap();
        if index == 0 || (failed_task && index == 2) {
            assert!(!projection
                .provider_payload()
                .system_memory_block()
                .contains(&procedure));
        } else {
            assert!(
                projection
                    .provider_payload()
                    .system_memory_block()
                    .contains("reject on mismatch"),
                "validated method never reached governed projection"
            );
            assert!(
                !runtime
                    .list_runtime_skills(RuntimeSkillListRequest {
                        owning_scope: RuntimeSkillOwningScope::Subject {
                            mounted_subject_id: runtime.subject_id().into()
                        },
                        query: None,
                        include_disabled: false,
                        include_retired: false,
                        limit: 10,
                    })
                    .unwrap()
                    .skills
                    .is_empty(),
                "successful independent tasks never produced a real skill positive control"
            );
        }
    }

    drop(projection);
    drop(engine);
    drop(governor);
    drop(runtime);
    let reopened_store = if persistent {
        drop(store);
        support::open_memory_store(config.clone()).unwrap()
    } else {
        store
    };
    let reopened = Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("learning-agent", "learning-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "learning-chat").unwrap())
            .store(reopened_store.clone())
            .clock(support::fixed_clock(1_800_000_001))
            .agent_tool_registry(registry.clone())
            .build()
            .unwrap(),
    );
    let after = reopened
        .discover_method_trials(MemoryMethodTrialRequest {
            tool: bm_core::memory::ProceduralProducerToolV1 {
                registry_ref: registry.registry_ref(),
                tool_id: "verify_file".into(),
                schema_fingerprint: "schema-checksum".into(),
            },
            max_candidates: 4,
        })
        .unwrap();
    assert!(
        after.candidates.is_empty(),
        "an active method must no longer be offered as an unverified trial"
    );
    let projection = reopened
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "verify_file".into(),
            system_max_len: 16_384,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .unwrap();
    assert_eq!(
        projection
            .provider_payload()
            .system_memory_block()
            .contains("reject on mismatch"),
        !failed_task,
        "reopen must retain the real positive and negative task result"
    );
    reopened
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: reopened.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: "learning-chat".into(),
            turn_id: Some("strategy-turn".into()),
            transition: TranscriptLifecycleTransition::Restore,
            reason: "restore_without_changing_evidence".into(),
        })
        .unwrap();
    let skills = reopened
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: reopened.subject_id().into(),
            },
            query: None,
            include_disabled: false,
            include_retired: false,
            limit: 10,
        })
        .unwrap();
    assert!(
        skills.skills.is_empty() == failed_task,
        "negative task evidence must suppress the previously proven runtime skill after reopen"
    );
    let other = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("other-agent", "learning-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "learning-chat").unwrap())
        .store(reopened_store.clone())
        .clock(support::fixed_clock(1_800_000_001))
        .agent_tool_registry(registry.clone())
        .build()
        .unwrap();
    let denied = other
        .discover_method_trials(MemoryMethodTrialRequest {
            tool: bm_core::memory::ProceduralProducerToolV1 {
                registry_ref: registry.registry_ref(),
                tool_id: "verify_file".into(),
                schema_fingerprint: "schema-checksum".into(),
            },
            max_candidates: 4,
        })
        .unwrap();
    assert!(
        denied.candidates.is_empty(),
        "another mounted subject must not see method trial content"
    );
    let denied_projection = other
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "verify_file".into(),
            system_max_len: 16_384,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .unwrap();
    assert!(!denied_projection
        .provider_payload()
        .system_memory_block()
        .contains("reject on mismatch"));
    let denied_skills = other
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: other.subject_id().into(),
            },
            query: None,
            include_disabled: false,
            include_retired: false,
            limit: 10,
        })
        .unwrap();
    assert!(denied_skills.skills.is_empty());
    reopened
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: reopened.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: "learning-chat".into(),
            turn_id: Some("strategy-turn".into()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "withdraw_learned_method_source".into(),
        })
        .unwrap();
    if successive_source_deletion {
        #[cfg(feature = "nonproduction-replay-harness")]
        {
            let erased = reopened_store.export_replay_snapshot().unwrap();
            assert!(
                erased
                    .json_docs
                    .iter()
                    .all(|doc| doc.namespace != "runtime_skill_records"),
                "erasing the actual semantic creation source must erase its promoted raw copy"
            );
            assert!(!serde_json::to_string(&erased).unwrap().contains(&procedure),
                "semantic creation DeleteRaw must remove the derived method body, not merely hide its projection");
        }
        reopened
            .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
                memory_space_id: reopened.memory_space_id().into(),
                channel_id: "sdk.direct".into(),
                conversation_id: "learning-chat".into(),
                turn_id: Some("apply-method-1".into()),
                transition: TranscriptLifecycleTransition::DeleteRaw,
                reason: "delete remaining task source after semantic creation was already erased"
                    .into(),
            })
            .expect(
                "exact erased semantic creation must permit deleting its remaining task source",
            );
    }
    let withdrawn = reopened
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "verify_file".into(),
            system_max_len: 16_384,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .unwrap();
    assert!(
        !withdrawn
            .provider_payload()
            .system_memory_block()
            .contains("reject on mismatch"),
        "withdrawn method source must disappear before reconciliation runs"
    );
    let withdrawn_skills = reopened
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: reopened.subject_id().into(),
            },
            query: None,
            include_disabled: false,
            include_retired: false,
            limit: 10,
        })
        .unwrap();
    assert!(
        withdrawn_skills.skills.is_empty(),
        "a promoted skill cannot launder a withdrawn method source"
    );

    let reconcile_engine = MemoryLearningEngine::attach(reopened.clone()).unwrap();
    let mut reconciled = false;
    for _ in 0..32 {
        match reconcile_engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "method-source-withdrawal-reconcile".into(),
                    lease_duration_secs: 60,
                },
                &mut Port(&model),
            )
            .unwrap()
        {
            MemoryLearningCycleOutcome::ProceduralReconciliation(report) => {
                if report.completed {
                    reconciled = true;
                    break;
                }
            }
            MemoryLearningCycleOutcome::Completed(_)
            | MemoryLearningCycleOutcome::ProceduralCompleted(_) => {}
            other => {
                panic!("withdrawn source must run through real durable reconciliation: {other:?}")
            }
        }
    }
    assert!(
        reconciled,
        "source invalidation never completed its official reconciliation lane"
    );
    assert_withdrawn_method_unavailable(&reopened, &registry);
    drop(reconcile_engine);
    drop(other);
    drop(reopened);
    let final_store = if persistent {
        drop(reopened_store);
        support::open_memory_store(config).unwrap()
    } else {
        reopened_store
    };
    let final_runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("learning-agent", "learning-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "learning-chat").unwrap())
        .store(final_store)
        .clock(support::fixed_clock(1_800_000_002))
        .agent_tool_registry(registry.clone())
        .build()
        .unwrap();
    assert_withdrawn_method_unavailable(&final_runtime, &registry);
}

fn assert_withdrawn_method_unavailable(
    runtime: &MemoryRuntime,
    registry: &AgentToolRegistrySnapshot,
) {
    let trials = runtime
        .discover_method_trials(MemoryMethodTrialRequest {
            tool: ProceduralProducerToolV1 {
                registry_ref: registry.registry_ref(),
                tool_id: "verify_file".into(),
                schema_fingerprint: "schema-checksum".into(),
            },
            max_candidates: 4,
        })
        .unwrap();
    assert!(
        trials.candidates.is_empty(),
        "reconciliation/reopen resurrected a withdrawn method as a trial"
    );
    let projected = runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "verify_file".into(),
            system_max_len: 16_384,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .unwrap();
    assert!(
        !projected
            .provider_payload()
            .system_memory_block()
            .contains("reject on mismatch"),
        "reconciliation/reopen restored a source-withdrawn method to the actual model payload"
    );
    assert!(
        runtime
            .list_runtime_skills(RuntimeSkillListRequest {
                owning_scope: RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: runtime.subject_id().into()
                },
                query: None,
                include_disabled: false,
                include_retired: false,
                limit: 10,
            })
            .unwrap()
            .skills
            .is_empty(),
        "reconciliation/reopen restored a source-withdrawn runtime skill"
    );
}

#[test]
fn public_engine_extracts_candidate_from_admitted_material_without_host_method_declaration() {
    production_learning(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        false,
        false,
    );
}

static TEMP: AtomicUsize = AtomicUsize::new(0);
#[test]
fn file_method_extraction_and_trial_survive_store_reopen() {
    let root = std::env::temp_dir().join(format!(
        "beetle-method-file-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    production_learning(
        StoreBackendConfig::file(&root, support::host_test_profile()).unwrap(),
        true,
        false,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_method_extraction_and_trial_survive_store_reopen() {
    let root = std::env::temp_dir().join(format!(
        "beetle-method-sqlite-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    production_learning(
        StoreBackendConfig::sqlite(root.join("memory.sqlite"), support::host_test_profile())
            .unwrap(),
        true,
        false,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_task_then_source_withdrawal_remains_closed_in_memory() {
    production_learning(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        false,
        true,
    );
}

#[test]
fn failed_task_then_source_withdrawal_remains_closed_after_file_reopen() {
    let root = std::env::temp_dir().join(format!(
        "beetle-method-negative-file-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    production_learning(
        StoreBackendConfig::file(&root, support::host_test_profile()).unwrap(),
        true,
        true,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "sqlite-store")]
#[test]
fn failed_task_then_source_withdrawal_remains_closed_after_sqlite_reopen() {
    let root = std::env::temp_dir().join(format!(
        "beetle-method-negative-sqlite-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    production_learning(
        StoreBackendConfig::sqlite(root.join("memory.sqlite"), support::host_test_profile())
            .unwrap(),
        true,
        true,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn fresh_consumer_adopts_active_method_from_signed_projection_receipt() {
    production_learning_with_active_receipt(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        false,
        false,
        true,
        false,
    );
}

#[test]
#[cfg(all(feature = "sqlite-store", feature = "nonproduction-replay-harness"))]
fn semantic_creation_and_task_successive_raw_deletion_survives_reopen() {
    let root = std::env::temp_dir().join(format!(
        "beetle-semantic-successive-delete-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    production_learning_with_active_receipt(
        StoreBackendConfig::sqlite(root.join("memory.sqlite"), support::host_test_profile())
            .unwrap(),
        true,
        false,
        false,
        true,
    );
    std::fs::remove_dir_all(root).unwrap();
}
