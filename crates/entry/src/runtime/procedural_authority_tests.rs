use super::*;
use crate::{EntryAuthConfig, EntryBearerPrincipal, EntryOperationCapability};
use bm_sdk::*;

fn fixture() -> (
    EntryRuntime,
    MemoryRuntime,
    MemoryProceduralProducerControlReport,
    MemoryProceduralSubmissionCapability,
) {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    capability.maintenance_enabled = false;
    #[cfg(target_os = "macos")]
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    #[cfg(target_os = "linux")]
    let profile = ProfileId::DesktopLinuxEmbeddedSdk;
    #[cfg(target_os = "windows")]
    let profile = ProfileId::DesktopWindowsEmbeddedSdk;
    let entry = EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "entry-agent".into(),
            owner_id: "entry-owner".into(),
        },
        scope: EntryScope {
            channel: "entry-test".into(),
            chat_id: "entry-chat".into(),
            conversation_id: None,
        },
        store: StoreBackendConfig::in_memory(profile).unwrap(),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::required_bearer_principal(
            "synthetic-token",
            EntryBearerPrincipal::new(
                "executor-principal",
                "entry-owner",
                [EntryOperationCapability::FinalizeTurn],
            ),
        ),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .unwrap();
    let tools = AgentToolRegistrySnapshot::compact(
        "entry-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "inspect", "Inspect", "schema-1",
        )],
        entry.runtime.config().clock.now_secs(),
    );
    entry
        .runtime
        .upsert_agent_tool_registry(tools.clone())
        .unwrap();
    let governor = governor_control_runtime(&entry.runtime, entry.store.clone()).unwrap();
    let registered = entry
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "register-entry-executor".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: ProceduralProducerSpecV1 {
                binding_id: "entry-executor".into(),
                scope: ProceduralProducerScopeV1 {
                    memory_space_id: entry.runtime.memory_space_id().into(),
                    mounted_subject_id: entry.runtime.subject_id().into(),
                    channel_id: "entry-test".into(),
                    chat_id: "entry-chat".into(),
                },
                principal: ProceduralProducerPrincipalV1::EntryPrincipal {
                    principal_id: "executor-principal".into(),
                    owner_id: "entry-owner".into(),
                },
                source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                claims: ProceduralProducerClaimsV1 {
                    execution_facts: true,
                    method_declarations: true,
                    usage_feedback: false,
                    source_classifications: vec![
                        ProceduralSourceSensitivity::NonPrivate,
                        ProceduralSourceSensitivity::Private,
                    ],
                },
                tools: vec![ProceduralProducerToolV1 {
                    registry_ref: tools.registry_ref(),
                    tool_id: "inspect".into(),
                    schema_fingerprint: "schema-1".into(),
                }],
                source_config_ref: "trusted-entry-executor".into(),
            },
        })
        .unwrap();
    let submission = entry
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    (entry, governor, registered, submission)
}

fn request(entry: &EntryRuntime, id: &str, feedback: bool) -> AdapterCommand {
    let now = entry.runtime.config().clock.now_secs();
    let mut learning = PostTurnLearningInputV2::empty();
    learning.tool_call_count = 1;
    if feedback {
        learning.agent_tool_feedback.push(AgentToolUsageFeedbackV3 {
            registry_ref: entry.runtime.agent_tool_registries()[0].registry_ref(),
            tool_id: "inspect".into(),
            schema_fingerprint: "schema-1".into(),
            execution_facts: vec![ToolExecutionFactV1 {
                observation_id: format!("obs-{id}"),
                call_id: format!("call-{id}"),
                outcome: ToolExecutionOutcome::Failed,
                source_sensitivity: ProceduralSourceSensitivity::Private,
                started_at: Some(now),
                completed_at: Some(now),
            }],
            method_evidence: vec![ToolMethodEvidenceV1 {
                method_id: format!("method-{id}"),
                task_signature: "private-input".into(),
                body: "PRIVATE_ENTRY_METHOD_MUST_NOT_PERSIST".into(),
                execution_refs: vec![format!("obs-{id}")],
                source_sensitivity: ProceduralSourceSensitivity::Private,
                external_content: false,
            }],
        });
    }
    AdapterCommand::FinalizeTurn(Box::new(MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: id.into(),
            conversation: ConversationScope {
                channel: "entry-test".into(),
                chat_id: "entry-chat".into(),
                conversation_id: Some("entry-chat".into()),
            },
            subject: entry.runtime.subject_id().into(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: IngressKind::User,
                channel: "entry-test".into(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: None,
                model_alias: None,
                model_resolved: None,
                request_id: None,
                client_conversation_hint: None,
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("synthetic entry request")],
            assistant_message: Some(TranscriptInputMessage::assistant("finished safely")),
            tool_observations: vec![ToolObservationDigest {
                observation_id: format!("obs-{id}"),
                call_id: format!("call-{id}"),
                tool_name: "inspect".into(),
                summary: "synthetic safe execution result".into(),
                external_content: false,
            }],
            external_content_used: false,
            candidate_ids: vec![],
        },
        learning,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }))
}

fn context(entry: &EntryRuntime, id: &str) -> crate::EntryTransportContext {
    context_with_authorization(entry, id, Some("Bearer synthetic-token"))
}

fn context_with_authorization(
    entry: &EntryRuntime,
    id: &str,
    authorization: Option<&str>,
) -> crate::EntryTransportContext {
    // Actual accepted socket and the configured verifier, not a forged auth bool.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let accepted = crate::EntryAcceptedTcpStream::accept(&listener).unwrap();
    let auth = entry.config.auth.authenticate_accepted_tcp_stream(
        &accepted,
        authorization,
        "untrusted-loopback",
    );
    drop(client);
    crate::EntryTransportContext::new(
        id,
        bm_adapter::TransportKind::Http,
        bm_adapter::TransportMode::Server,
        bm_adapter::AdapterOperation::FinalizeTurn,
        "remote-test",
        "http_client",
        id,
        format!("audit-{id}"),
        auth,
    )
}

#[cfg(feature = "nonproduction-replay-harness")]
mod additional_boundaries {
    use super::*;

    struct NoProvider;

    impl GovernanceExecutionPort for NoProvider {
        fn execute(
            &mut self,
            _: &AuthorizedGovernanceEnvelope,
            _: &ImmutableGovernanceExecutionBinding,
            _: &GovernanceEgressAuthority,
            _: &mut dyn GovernanceExecutionOperation,
        ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
            panic!("synthetic procedural evidence must never call a Provider")
        }
    }

    fn scoped_worker(entry: &EntryRuntime, chat: &str) -> Arc<MemoryRuntime> {
        let memory = &entry.runtime;
        let mut policy = memory.config().capability_policy.clone();
        policy.maintenance_enabled = true;
        Arc::new(
            MemoryRuntime::builder()
                .identity(memory.identity().clone())
                .scope(
                    MemoryScope::new("entry-test", chat)
                        .unwrap()
                        .with_conversation_id("entry-chat")
                        .unwrap(),
                )
                .subject_registry(memory.subject_registry().clone())
                .subject_relationship_graph(memory.subject_relationship_graph().clone())
                .scoped_runtime(memory.scoped_runtime().clone())
                .store(entry.store.clone())
                .clock(memory.config().clock.clone())
                .capability_policy(policy)
                .privacy_policy(memory.config().privacy_policy.clone())
                .audit_sink(memory.config().audit_sink.clone())
                .agent_tool_registries(memory.agent_tool_registries())
                .build()
                .unwrap(),
        )
    }

    fn seed_real_experience(
        entry: &EntryRuntime,
        capability: &MemoryProceduralSubmissionCapability,
    ) {
        let id = "selection-source";
        let mut command = request(entry, id, true);
        let AdapterCommand::FinalizeTurn(input) = &mut command else {
            unreachable!()
        };
        let now = entry.runtime.config().clock.now_secs();
        let facts = ["a", "b"]
            .into_iter()
            .map(|suffix| ToolExecutionFactV1 {
                observation_id: format!("obs-{id}-{suffix}"),
                call_id: format!("call-{id}-{suffix}"),
                outcome: ToolExecutionOutcome::Succeeded,
                source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
                started_at: Some(now),
                completed_at: Some(now),
            })
            .collect::<Vec<_>>();
        input.turn.tool_observations = facts
            .iter()
            .map(|fact| ToolObservationDigest {
                observation_id: fact.observation_id.clone(),
                call_id: fact.call_id.clone(),
                tool_name: "inspect".into(),
                summary: "Synthetic release artifact inspection completed".into(),
                external_content: false,
            })
            .collect();
        input.learning.tool_call_count = 2;
        let group = &mut input.learning.agent_tool_feedback[0];
        group.method_evidence = vec![ToolMethodEvidenceV1 {
            method_id: "inspect-release-artifact".into(),
            task_signature: "inspect_release_artifact".into(),
            body: "1. Inspect the release artifact manifest\n2. Validate each artifact against the manifest\n3. Verify the inspection results before delivery".into(),
            execution_refs: facts.iter().map(|fact| fact.observation_id.clone()).collect(),
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
            external_content: false,
        }];
        group.execution_facts = facts;
        let response = entry
            .handle_with_services(
                context(entry, id),
                command,
                AdapterRuntimeServices {
                    http: None,
                    llm: None,
                    procedural_submission: Some(capability),
                },
            )
            .unwrap();
        let bm_adapter::AdapterResponse::Accepted {
            report: bm_adapter::AdapterSdkReport::FinalizeTurn(report),
            ..
        } = response.adapter
        else {
            panic!("authorized source turn must commit")
        };
        let job_id = report.procedural_learning.job_id.unwrap();
        // Keep Entry's background service disabled. Only the official SDK cycle
        // runs explicitly, with the same Store, registry and mounted subject.
        let engine = MemoryLearningEngine::attach(scoped_worker(entry, "entry-chat")).unwrap();
        let outcome = engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "entry-synthetic-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap();
        let MemoryLearningCycleOutcome::ProceduralCompleted(applied) = outcome else {
            panic!("the actual procedural job must apply, got {outcome:?}")
        };
        assert_eq!(applied.job.job_id, job_id);
        assert!(applied.receipt.changed_count > 0);
    }

    fn signed_selection(memory: &MemoryRuntime, turn_id: &str) -> ProceduralSelectionReceiptV1 {
        let output = memory
            .project(MemoryProjectionRequest {
                binding: ProceduralProjectionBindingV1::Turn {
                    turn_id: turn_id.into(),
                },
                temporal_operation: MemoryRecallTemporalOperation::Current,
                user_query: "inspect release artifact".into(),
                system_max_len: 16_384,
                recent_messages_limit: 4,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: Vec::new(),
                tool_registry_refs: memory
                    .agent_tool_registries()
                    .iter()
                    .map(|tools| tools.registry_ref())
                    .collect(),
            })
            .unwrap();
        assert!(
            !output.provider_payload().agent_tool_hints().is_empty(),
            "selection must represent actual delivered experience"
        );
        let receipt = output.report().selection_receipt().unwrap().clone();
        assert!(!receipt.agent_tool_experiences.is_empty());
        receipt
    }

    fn selection_only(
        entry: &EntryRuntime,
        id: &str,
        receipt: ProceduralSelectionReceiptV1,
    ) -> AdapterCommand {
        let mut command = request(entry, id, false);
        let AdapterCommand::FinalizeTurn(input) = &mut command else {
            unreachable!()
        };
        input.turn.tool_observations.clear();
        input.learning.tool_call_count = 0;
        input.learning.selection_receipt = Some(receipt);
        assert!(!input.learning.has_feedback());
        command
    }

    #[test]
    fn pfi2_entry_signed_selection_only_needs_no_producer_but_checks_signature_and_scope() {
        let (entry, governor, registered, capability) = fixture();
        seed_real_experience(&entry, &capability);
        let first = signed_selection(&entry.runtime, "selection-no-cap");
        let stale = signed_selection(&entry.runtime, "selection-revoked-cap");
        let valid_for_tamper = signed_selection(&entry.runtime, "selection-tamper");
        let foreign =
            signed_selection(&scoped_worker(&entry, "other-chat"), "selection-cross-chat");
        assert_eq!(foreign.identity.chat_id, "other-chat");
        assert_eq!(foreign.identity.conversation_id, "entry-chat");

        let response = entry
            .handle(
                context(&entry, "selection-no-cap"),
                selection_only(&entry, "selection-no-cap", first.clone()),
            )
            .unwrap();
        let bm_adapter::AdapterResponse::Accepted {
            report: bm_adapter::AdapterSdkReport::FinalizeTurn(report),
            ..
        } = response.adapter
        else {
            panic!("signed selection without feedback needs no grant")
        };
        assert!(report.session_committed && report.transcript_committed);
        assert!(report.procedural_learning.job_id.is_none());

        governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "selection-revoke".into(),
                expected_revision: Some(registered.binding.revision_ref().unwrap()),
                state: ProceduralProducerStateV1::Revoked,
                spec: registered.binding.spec,
            })
            .unwrap();
        let response = entry
            .handle_with_services(
                context(&entry, "selection-revoked-cap"),
                selection_only(&entry, "selection-revoked-cap", stale.clone()),
                AdapterRuntimeServices {
                    http: None,
                    llm: None,
                    procedural_submission: Some(&capability),
                },
            )
            .unwrap();
        let bm_adapter::AdapterResponse::Accepted {
            report: bm_adapter::AdapterSdkReport::FinalizeTurn(report),
            ..
        } = response.adapter
        else {
            panic!("a revoked capability must not reject selection-only intake")
        };
        assert!(report.transcript_committed && report.procedural_learning.job_id.is_none());
        let snapshot = entry.store.export_replay_snapshot().unwrap();
        for (id, receipt) in [
            ("selection-no-cap", first),
            ("selection-revoked-cap", stale),
        ] {
            let turn = snapshot
                .json_docs
                .iter()
                .find(|doc| {
                    doc.namespace == "conversation_transcript" && doc.value["turn_id"] == id
                })
                .unwrap();
            let evidence: PostTurnLearningEvidenceV2 =
                serde_json::from_value(turn.value["learning_evidence"].clone()).unwrap();
            assert!(matches!(
                evidence.authority,
                ProceduralFeedbackAuthorityV2::Empty
            ));
            assert_eq!(evidence.selection_receipt, Some(receipt));
        }
        let mut tampered = valid_for_tamper;
        tampered.delivery_digest = format!("sha256:{}", "f".repeat(64));
        tampered.receipt_ref = tampered.canonical_receipt_ref().unwrap();
        assert!(
            tampered.validate_contract(),
            "attack must reach signature verification"
        );
        for (id, receipt, expected) in [
            (
                "selection-tamper",
                tampered,
                ProceduralLearningErrorKeyV1::ReceiptInvalid,
            ),
            (
                "selection-cross-chat",
                foreign,
                ProceduralLearningErrorKeyV1::ScopeMismatch,
            ),
        ] {
            let before = entry.store.export_replay_snapshot().unwrap();
            let error = entry
                .handle(context(&entry, id), selection_only(&entry, id, receipt))
                .err()
                .expect("receipt authority must reject before intake");
            let typed = std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<ProceduralLearningSdkError>())
                .expect("typed receipt authority rejection");
            assert_eq!(typed.key, expected);
            assert_eq!(entry.store.export_replay_snapshot().unwrap(), before);
        }
    }

    fn wire_body(command: AdapterCommand) -> serde_json::Value {
        let AdapterCommand::FinalizeTurn(input) = command else {
            unreachable!()
        };
        serde_json::json!({ "turn": input.turn, "learning": input.learning })
    }

    #[test]
    fn pfi2_entry_json_decoder_and_bearer_cannot_mint_procedural_authority() {
        let (entry, _, _, _) = fixture();
        let ordinary = wire_body(request(&entry, "wire-ordinary", false));
        let command = bm_adapter::decode_json_adapter_command(
            bm_adapter::AdapterOperation::FinalizeTurn,
            &ordinary.to_string(),
        )
        .unwrap();
        let response = entry
            .handle(context(&entry, "wire-ordinary"), command)
            .unwrap();
        assert!(matches!(
            response.adapter,
            bm_adapter::AdapterResponse::Accepted { .. }
        ));
        let before = entry.store.export_replay_snapshot().unwrap();
        assert!(before
            .json_docs
            .iter()
            .any(|doc| doc.namespace == "conversation_transcript"
                && doc.value["turn_id"] == "wire-ordinary"));
        for field in ["authority", "producer", "procedural_submission"] {
            let mut body = wire_body(request(&entry, field, true));
            if field == "procedural_submission" {
                body[field] = serde_json::json!({"binding_id": "entry-executor"});
            } else {
                body["learning"][field] = serde_json::json!({"kind": "host_runtime_observation"});
            }
            let error = bm_adapter::decode_json_adapter_command(
                bm_adapter::AdapterOperation::FinalizeTurn,
                &body.to_string(),
            )
            .expect_err("wire authority fields are forbidden");
            assert_eq!(error.stage(), "adapter_json_command");
            assert_eq!(entry.store.export_replay_snapshot().unwrap(), before);
        }
        let body = wire_body(request(&entry, "wire-feedback-no-cap", true));
        let command = bm_adapter::decode_json_adapter_command(
            bm_adapter::AdapterOperation::FinalizeTurn,
            &body.to_string(),
        )
        .unwrap();
        let error = entry
            .handle(context(&entry, "wire-feedback-no-cap"), command)
            .err()
            .expect("a decoded typed body is not a submission capability");
        assert_eq!(error.stage(), "post_turn_learning_evidence");
        assert!(!error
            .to_string()
            .contains("PRIVATE_ENTRY_METHOD_MUST_NOT_PERSIST"));
        assert_eq!(entry.store.export_replay_snapshot().unwrap(), before);
        for (id, auth) in [
            ("wire-missing-bearer", None),
            ("wire-wrong-bearer", Some("Bearer wrong-token")),
        ] {
            let body = wire_body(request(&entry, id, false));
            let command = bm_adapter::decode_json_adapter_command(
                bm_adapter::AdapterOperation::FinalizeTurn,
                &body.to_string(),
            )
            .unwrap();
            let response = entry
                .handle(context_with_authorization(&entry, id, auth), command)
                .unwrap();
            assert!(matches!(
                response.adapter,
                bm_adapter::AdapterResponse::Rejected { .. }
            ));
            assert_eq!(entry.store.export_replay_snapshot().unwrap(), before);
        }
    }
}

#[test]
fn pfi2_authenticated_entry_handoff_is_explicit_and_revocation_fenced() {
    let (entry, governor, registered, capability) = fixture();
    let response = entry
        .handle_with_services(
            context(&entry, "positive"),
            request(&entry, "positive", true),
            AdapterRuntimeServices {
                http: None,
                llm: None,
                procedural_submission: Some(&capability),
            },
        )
        .unwrap();
    assert!(matches!(
        response.adapter,
        bm_adapter::AdapterResponse::Accepted {
            report: bm_adapter::AdapterSdkReport::FinalizeTurn(_),
            ..
        }
    ));
    assert!(entry
        .handle(
            context(&entry, "no-grant"),
            request(&entry, "no-grant", true)
        )
        .is_err());
    governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "revoke-entry".into(),
            spec: registered.binding.spec.clone(),
            state: ProceduralProducerStateV1::Revoked,
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
        })
        .unwrap();
    assert!(entry
        .handle_with_services(
            context(&entry, "revoked"),
            request(&entry, "revoked", true),
            AdapterRuntimeServices {
                http: None,
                llm: None,
                procedural_submission: Some(&capability)
            }
        )
        .is_err());
    let ordinary = entry
        .handle_with_services(
            context(&entry, "ordinary"),
            request(&entry, "ordinary", false),
            AdapterRuntimeServices {
                http: None,
                llm: None,
                procedural_submission: Some(&capability),
            },
        )
        .unwrap();
    assert!(matches!(
        ordinary.adapter,
        bm_adapter::AdapterResponse::Accepted { .. }
    ));
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_entry_rejects_cross_principal_and_forged_confirmation_without_writes() {
    let (entry, _, registered, capability) = fixture();
    entry
        .handle_with_services(
            context(&entry, "privacy-positive"),
            request(&entry, "privacy-positive", true),
            AdapterRuntimeServices {
                http: None,
                llm: None,
                procedural_submission: Some(&capability),
            },
        )
        .unwrap();
    let positive = entry.store.export_replay_snapshot().unwrap();
    assert!(positive
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"));
    assert!(!serde_json::to_string(&positive)
        .unwrap()
        .contains("PRIVATE_ENTRY_METHOD_MUST_NOT_PERSIST"));

    for (index, principal) in [
        ProceduralProducerPrincipalV1::EntryPrincipal {
            principal_id: "other-principal".into(),
            owner_id: "entry-owner".into(),
        },
        ProceduralProducerPrincipalV1::EntryPrincipal {
            principal_id: "executor-principal".into(),
            owner_id: "other-owner".into(),
        },
        ProceduralProducerPrincipalV1::LocalCapability {
            capability_id: "local-only".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut spec = registered.binding.spec.clone();
        spec.binding_id = format!("wrong-principal-{index}");
        spec.principal = principal;
        let grant = entry
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: format!("register-wrong-principal-{index}"),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec,
            })
            .unwrap();
        let wrong = entry
            .procedural_submission_capability(&grant.binding.revision_ref().unwrap())
            .unwrap();
        let before = entry.store.export_replay_snapshot().unwrap();
        let id = format!("denied-principal-{index}");
        let denied = entry
            .handle_with_services(
                context(&entry, &id),
                request(&entry, &id, true),
                AdapterRuntimeServices {
                    http: None,
                    llm: None,
                    procedural_submission: Some(&wrong),
                },
            )
            .unwrap();
        assert!(matches!(
            denied.adapter,
            bm_adapter::AdapterResponse::Rejected {
                error_key: bm_adapter::AdapterErrorKey::Forbidden,
                ..
            }
        ));
        assert_eq!(entry.store.export_replay_snapshot().unwrap(), before);
    }
    let before = entry.store.export_replay_snapshot().unwrap();
    let mut forged = request(&entry, "forged-human", true);
    let AdapterCommand::FinalizeTurn(input) = &mut forged else {
        unreachable!()
    };
    input.learning.human_confirmation_operation_id = Some("body-cannot-confirm".into());
    let error = entry
        .handle_with_services(
            context(&entry, "forged-human"),
            forged,
            AdapterRuntimeServices {
                http: None,
                llm: None,
                procedural_submission: Some(&capability),
            },
        )
        .err()
        .expect("body cannot grant Human confirmation");
    assert!(!error
        .to_string()
        .contains("PRIVATE_ENTRY_METHOD_MUST_NOT_PERSIST"));
    assert_eq!(entry.store.export_replay_snapshot().unwrap(), before);
    let error = entry
        .handle(
            context(&entry, "missing-producer"),
            request(&entry, "missing-producer", true),
        )
        .err()
        .expect("missing producer must reject");
    assert!(!error
        .to_string()
        .contains("PRIVATE_ENTRY_METHOD_MUST_NOT_PERSIST"));
    assert_eq!(entry.store.export_replay_snapshot().unwrap(), before);
}
