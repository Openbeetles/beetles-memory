use super::*;
use bm_core::memory::*;

struct Clock;
impl crate::MemoryClock for Clock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

fn registry() -> AgentToolRegistrySnapshot {
    AgentToolRegistrySnapshot::compact(
        "pfi2-tools",
        "host",
        vec![bm_core::skills::AgentToolDescriptor::compact(
            "extract", "Extract", "schema-a",
        )],
        1_800_000_000,
    )
}

fn runtime(platform: &StorePlatform, actor: &str) -> MemoryRuntime {
    runtime_with_graph(platform, actor, None)
}

fn runtime_with_graph(
    platform: &StorePlatform,
    actor: &str,
    graph: Option<SubjectRelationshipGraph>,
) -> MemoryRuntime {
    runtime_in_chat(platform, actor, graph, "pfi2-chat")
}

fn runtime_in_chat(
    platform: &StorePlatform,
    actor: &str,
    graph: Option<SubjectRelationshipGraph>,
    chat: &str,
) -> MemoryRuntime {
    let mut scope =
        SubjectScopedRuntime::single_agent_default("pfi2-owner", "pfi2-agent", None).unwrap();
    scope.actor_subject_id = actor.into();
    let mut builder = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("pfi2-agent", "pfi2-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", chat).unwrap())
        .scoped_runtime(scope)
        .store(crate::MemoryStoreHandle::from_platform(platform.clone()))
        .clock(Arc::new(Clock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .agent_tool_registry(registry());
    if let Some(graph) = graph {
        builder = builder.subject_relationship_graph(graph);
    }
    builder.build().unwrap()
}

fn platform() -> StorePlatform {
    #[cfg(target_os = "macos")]
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    #[cfg(target_os = "linux")]
    let profile = ProfileId::DesktopLinuxEmbeddedSdk;
    #[cfg(target_os = "windows")]
    let profile = ProfileId::DesktopWindowsEmbeddedSdk;
    StorePlatform::open(crate::StoreBackendConfig::in_memory(profile).unwrap()).unwrap()
}

fn grant(governor: &MemoryRuntime) -> crate::MemoryProceduralProducerControlReport {
    governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "register-pfi2-witness".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: ProceduralProducerSpecV1 {
                binding_id: "witness-a".into(),
                scope: governor.procedural_producer_scope(),
                principal: ProceduralProducerPrincipalV1::LocalCapability {
                    capability_id: "executor-a".into(),
                },
                source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                claims: ProceduralProducerClaimsV1 {
                    execution_facts: true,
                    method_declarations: true,
                    usage_feedback: false,
                    source_classifications: vec![
                        ProceduralSourceSensitivity::NonPrivate,
                        ProceduralSourceSensitivity::Private,
                        ProceduralSourceSensitivity::Unknown,
                    ],
                },
                tools: vec![ProceduralProducerToolV1 {
                    registry_ref: registry().registry_ref(),
                    tool_id: "extract".into(),
                    schema_fingerprint: "schema-a".into(),
                }],
                source_config_ref: "host-executor-a".into(),
            },
        })
        .unwrap()
}

fn request(memory: &MemoryRuntime, id: &str) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: id.into(),
            conversation: ConversationScope {
                channel: "sdk.direct".into(),
                chat_id: "pfi2-chat".into(),
                conversation_id: Some("pfi2-chat".into()),
            },
            subject: memory.subject_id().into(),
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
            input_messages: vec![TranscriptInputMessage::user("synthetic extraction")],
            assistant_message: Some(TranscriptInputMessage::assistant("failed safely")),
            tool_observations: vec![ToolObservationDigest {
                observation_id: format!("obs-{id}"),
                call_id: format!("call-{id}"),
                tool_name: "extract".into(),
                summary: "synthetic result".into(),
                external_content: false,
            }],
            external_content_used: false,
            candidate_ids: vec![],
        },
        learning: PostTurnLearningInputV2 {
            tool_call_count: 1,
            agent_tool_feedback: vec![AgentToolUsageFeedbackV3 {
                registry_ref: registry().registry_ref(),
                tool_id: "extract".into(),
                schema_fingerprint: "schema-a".into(),
                execution_facts: vec![ToolExecutionFactV1 {
                    observation_id: format!("obs-{id}"),
                    call_id: format!("call-{id}"),
                    outcome: ToolExecutionOutcome::Failed,
                    source_sensitivity: ProceduralSourceSensitivity::Private,
                    started_at: Some(1_800_000_000),
                    completed_at: Some(1_800_000_000),
                }],
                method_evidence: vec![ToolMethodEvidenceV1 {
                    method_id: format!("method-{id}"),
                    task_signature: "extract-task".into(),
                    body:
                        "1. Synthetic private intake marker.\n2. Must never enter durable evidence."
                            .into(),
                    execution_refs: vec![format!("obs-{id}")],
                    source_sensitivity: ProceduralSourceSensitivity::Private,
                    external_content: false,
                }],
            }],
            ..PostTurnLearningInputV2::empty()
        },
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn image(
    platform: &StorePlatform,
) -> (
    Vec<crate::store_internal::StoreSnapshotJsonDoc>,
    Vec<crate::MemoryStoreEvent>,
) {
    (
        crate::store_internal::schema::store_json_namespaces()
            .flat_map(|namespace| platform.read_json_namespace(namespace).unwrap())
            .collect(),
        platform.read_events().unwrap(),
    )
}

#[test]
fn pfi2_missing_subject_root_cannot_be_recreated_by_a_new_scope() {
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-missing-root-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    std::fs::create_dir_all(&directory.0).unwrap();
    let database = directory.0.join("synthetic.db");
    let platform = StorePlatform::open(
        crate::StoreBackendConfig::sqlite(&database, platform().config().profile).unwrap(),
    )
    .unwrap();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let registered = grant(&governor);
    let other = runtime_in_chat(
        &platform,
        &system_governor_subject_id("pfi2-owner"),
        None,
        "pfi2-other-chat",
    );
    let key = ProceduralSubjectScopeV1 {
        memory_space_id: governor.memory_space_id().into(),
        mounted_subject_id: governor.subject_id().into(),
    }
    .root_key()
    .unwrap();
    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .execute(
                "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                rusqlite::params!["procedural_subject_validity_roots", key]
            )
            .unwrap(),
        1
    );
    drop(connection);
    let before = image(&platform);
    let mut spec = registered.binding.spec.clone();
    spec.scope = other.procedural_producer_scope();
    spec.binding_id = "new-scope-producer".into();
    let error = other
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "register-new-scope-after-corruption".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec,
        })
        .expect_err("an absent mutable root is not proof that the subject has never registered");
    let require_repair = |error: Error| {
        let Error::Other { source, .. } = error else {
            panic!("corruption must return a typed repair failure");
        };
        let typed = source
            .downcast_ref::<crate::ProceduralLearningSdkError>()
            .unwrap();
        assert_eq!(
            typed.disposition,
            crate::ProceduralLearningSdkErrorDisposition::RepairRequired
        );
    };
    require_repair(error);
    assert_eq!(
        image(&platform),
        before,
        "corruption cannot be repaired by dropping old scope ownership"
    );
    let error = governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "register-pfi2-witness".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: registered.binding.spec,
        })
        .expect_err("replaying the original operation must not bypass current closure health");
    require_repair(error);
    assert_eq!(image(&platform), before);
}

#[test]
fn pfi2_producer_registration_persists_subject_root_without_assets() {
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-root-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    let profile = platform().config().profile;
    for backend in ["memory", "file", "sqlite"] {
        let config = match backend {
            "memory" => crate::StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => crate::StoreBackendConfig::file(directory.0.join("file"), profile).unwrap(),
            "sqlite" => {
                crate::StoreBackendConfig::sqlite(directory.0.join("sqlite.db"), profile).unwrap()
            }
            _ => unreachable!(),
        };
        let platform = StorePlatform::open(config.clone()).unwrap();
        let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
        let registered = grant(&governor);
        let scope = ProceduralSubjectScopeV1 {
            memory_space_id: governor.memory_space_id().into(),
            mounted_subject_id: governor.subject_id().into(),
        };
        let key = scope.root_key().unwrap();
        let read = |platform: &StorePlatform| {
            let documents = platform
                .read_json_docs_by_keys(
                    "procedural_subject_validity_roots",
                    std::slice::from_ref(&key),
                )
                .unwrap();
            let document = documents
                .first()
                .expect("producer registration must atomically bind a subject root");
            let root: ProceduralSubjectValidityRootV1 =
                serde_json::from_value(document.value.clone()).unwrap();
            root.validate(MAX_PROCEDURAL_SUBJECT_SCOPES).unwrap();
            assert!(root
                .producer_scopes
                .contains(&registered.binding.spec.scope));
            root
        };
        let initial = read(&platform);
        let genesis_before = platform
            .read_json_namespace("procedural_subject_initializations")
            .unwrap();
        assert!(!genesis_before.is_empty());
        let mut usage = registered.binding.spec.clone();
        usage.binding_id = "usage-only".into();
        usage.claims = ProceduralProducerClaimsV1 {
            execution_facts: false,
            method_declarations: false,
            usage_feedback: true,
            source_classifications: vec![],
        };
        usage.tools.clear();
        let request = crate::MemoryProceduralProducerControlRequest {
            operation_id: "register-usage-only".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: usage,
        };
        governor
            .control_procedural_producer(request.clone())
            .unwrap();
        assert_eq!(
            read(&platform),
            initial,
            "another producer in the same scope must reuse its subject root"
        );
        assert!(platform
            .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
            .unwrap()
            .is_empty());
        assert!(platform
            .read_json_namespace("conversation_transcript")
            .unwrap()
            .is_empty());
        let before_retry = image(&platform);
        governor.control_procedural_producer(request).unwrap();
        assert_eq!(
            image(&platform),
            before_retry,
            "operation retry emits no duplicate root or event"
        );
        let other = runtime_in_chat(
            &platform,
            &system_governor_subject_id("pfi2-owner"),
            None,
            "second-chat",
        );
        let mut spec = registered.binding.spec.clone();
        spec.scope = other.procedural_producer_scope();
        spec.binding_id = "second-chat-producer".into();
        other
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "register-second-chat".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec,
            })
            .unwrap();
        let expected = read(&platform);
        assert!(expected
            .producer_scopes
            .contains(&other.procedural_producer_scope()));
        assert_eq!(expected.validity_epoch, initial.validity_epoch);
        assert_eq!(
            platform
                .read_json_namespace("procedural_subject_initializations")
                .unwrap(),
            genesis_before
        );
        drop(other);
        drop(governor);
        drop(platform);
        if backend != "memory" {
            let reopened = StorePlatform::open(config.clone()).unwrap();
            assert_eq!(read(&reopened), expected);
            assert_eq!(
                reopened
                    .read_json_namespace("procedural_subject_initializations")
                    .unwrap(),
                genesis_before
            );
            drop(reopened);
        }
        if backend == "file" {
            let namespace = "procedural_subject_validity_roots";
            let mut wrong_scope = registered.binding.spec.scope.clone();
            wrong_scope.chat_id = "unregistered-chat".into();
            let orphan = ProceduralSubjectValidityRootV1::initialize(
                scope,
                wrong_scope,
                MAX_PROCEDURAL_SUBJECT_SCOPES,
            )
            .unwrap();
            let before_value = serde_json::to_value(&expected).unwrap();
            let mut physical_path = None;
            for shard in
                std::fs::read_dir(directory.0.join("file/kv").join(namespace).join("_v2")).unwrap()
            {
                for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
                    let path = entry.unwrap().path();
                    let value: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                    if value == before_value {
                        assert!(physical_path.replace(path).is_none());
                    }
                }
            }
            let physical_path = physical_path.unwrap();
            let orphan_bytes = serde_json::to_vec(&orphan).unwrap();
            std::fs::write(&physical_path, &orphan_bytes).unwrap();
            assert!(
                StorePlatform::open(config).is_err(),
                "a canonical but orphaned root must fail Store-open closure"
            );
            assert_eq!(
                std::fs::read(physical_path).unwrap(),
                orphan_bytes,
                "reopen must not repair or overwrite synthetic corruption"
            );
        }
    }
}

#[test]
fn pfi2_public_store_append_cannot_copy_a_durable_producer_grant() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let registered = grant(&governor);
    let reference = registered.binding.revision_ref().unwrap();
    assert!(memory.procedural_submission_capability(&reference).is_err());
    let capability = governor
        .procedural_submission_capability(&reference)
        .unwrap();
    let before = image(&platform);
    assert!(memory
        .finalize_turn(request(&memory, "no-capability"))
        .is_err());
    assert_eq!(image(&platform), before);
    let report = memory
        .finalize_turn_with_procedural_evidence(&capability, request(&memory, "authorized"))
        .unwrap();
    assert!(report.transcript_commit.unwrap().committed);
    let source = platform
        .get_turn(
            &ConversationKey::new(memory.memory_space_id(), "sdk.direct", "pfi2-chat").unwrap(),
            memory.subject_id(),
            "authorized",
        )
        .unwrap()
        .unwrap();
    let admitted = &source
        .learning_evidence
        .as_ref()
        .unwrap()
        .agent_tool_feedback[0];
    assert_eq!(admitted.execution_facts.len(), 1);
    assert!(admitted.method_evidence.is_empty());
    assert!(!serde_json::to_string(&image(&platform).0)
        .unwrap()
        .contains("Synthetic private intake marker"));
    let forged_request = request(&memory, "copied-grant");
    let context = memory.procedural_submission_context(&capability).unwrap();
    let evidence = memory
        .build_post_turn_learning_evidence(&forged_request, Some(&context))
        .unwrap();
    let before = image(&platform);
    let plan = plan_canonical_turn_delta_with_transcript(
        platform.session_store().as_ref(),
        &platform,
        memory.memory_space_id(),
        &forged_request.turn,
        CanonicalTurnTranscriptCommitOptions {
            host_refs: vec![],
            learning_evidence: Some(evidence),
            conversation_alias: None,
            now_secs: 1_800_000_000,
        },
    )
    .unwrap();
    assert!(
        plan.commit_with(|intent| platform.append_canonical_turn_intent(intent))
            .is_err(),
        "copying a persisted producer grant must not grant public Store append authority"
    );
    assert_eq!(image(&platform), before);
}

#[test]
fn pfi2_intake_proof_is_exact_and_fenced_across_retry_and_revocation() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let mut ordinary = request(&memory, "ordinary-empty");
    ordinary.learning = PostTurnLearningInputV2::empty();
    ordinary.learning.tool_call_count = ordinary.turn.tool_observations.len() as u32;
    assert!(
        memory
            .finalize_turn(ordinary)
            .unwrap()
            .transcript_commit
            .unwrap()
            .committed
    );
    let registered = grant(&governor);
    let capability = governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    let make_plan = |id: &str| {
        let request = request(&memory, id);
        let context = memory.procedural_submission_context(&capability).unwrap();
        let evidence = memory
            .build_post_turn_learning_evidence(&request, Some(&context))
            .unwrap();
        let proof = crate::learning::ProceduralIntakeAuthorization {
            store_authority_digest: platform.learning_store_authority_digest(),
            current_head: context.head,
            evidence: evidence.clone(),
            store_incarnation: platform
                .procedural_store_incarnation(memory.memory_space_id())
                .unwrap(),
            source_preconditions: context.source_preconditions,
        };
        let plan = plan_canonical_turn_delta_with_transcript(
            platform.session_store().as_ref(),
            &platform,
            memory.memory_space_id(),
            &request.turn,
            CanonicalTurnTranscriptCommitOptions {
                host_refs: vec![],
                learning_evidence: Some(evidence),
                conversation_alias: None,
                now_secs: 1_800_000_000,
            },
        )
        .unwrap();
        (plan, proof)
    };
    let (_, proof_a) = make_plan("proof-a");
    let (plan_b, _) = make_plan("proof-b");
    let before = image(&platform);
    assert!(plan_b
        .commit_with(|intent| platform.append_authorized_canonical_turn(intent, &proof_a))
        .is_err());
    assert_eq!(image(&platform), before);
    let foreign_platform = self::platform();
    let foreign = runtime(&foreign_platform, &default_agent_subject_id("pfi2-agent"));
    let foreign_before = image(&foreign_platform);
    assert!(foreign
        .finalize_turn_with_procedural_evidence(&capability, request(&foreign, "foreign"))
        .is_err());
    assert_eq!(image(&foreign_platform), foreign_before);
    memory
        .finalize_turn_with_procedural_evidence(&capability, request(&memory, "retry"))
        .unwrap();
    let committed = image(&platform);
    let retry = memory
        .finalize_turn_with_procedural_evidence(&capability, request(&memory, "retry"))
        .unwrap();
    assert!(!retry.transcript_commit.unwrap().committed);
    assert_eq!(
        image(&platform),
        committed,
        "transport retry cannot append another turn/job/event"
    );
    let (stale_plan, stale_proof) = make_plan("revoke-race");
    governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "revoke-witness".into(),
            spec: registered.binding.spec.clone(),
            state: ProceduralProducerStateV1::Revoked,
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
        })
        .unwrap();
    let revoked = image(&platform);
    assert!(stale_plan
        .commit_with(|intent| platform.append_authorized_canonical_turn(intent, &stale_proof))
        .is_err());
    assert!(memory
        .finalize_turn_with_procedural_evidence(&capability, request(&memory, "after-revoke"))
        .is_err());
    assert_eq!(
        image(&platform),
        revoked,
        "revocation must fence Session, Transcript, jobs and events together"
    );
}

#[test]
fn pfi2_governed_source_requires_a_real_current_source_before_producer_creation() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let mut spec = grant(&governor).binding.spec;
    spec.binding_id = "governed-source".into();
    spec.claims.execution_facts = false;
    spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
        source_revision: GovernedOwnerRevisionRef::try_new(
            GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::EvidenceDocument,
                "missing-source",
            ),
            1,
        )
        .unwrap(),
    };
    let before = image(&platform);
    assert!(
        governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "register-missing-source".into(),
                spec,
                state: ProceduralProducerStateV1::Active,
                expected_revision: None,
            })
            .is_err(),
        "a canonical reference is not evidence that the governed source exists"
    );
    assert_eq!(image(&platform), before);
}

#[test]
fn pfi2_governed_source_revision_fences_intake_and_claimed_application() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let write_source = |revision: u64, authority: MemoryEvidenceAuthority, external: bool| {
        let body = format!(
            "1. Check the synthetic input.\n2. Validate the extracted fields. Revision {revision}."
        );
        let locator = if external {
            "synthetic://pfi2/external-source"
        } else {
            "synthetic://pfi2/source"
        };
        let group = canonical_recall_evidence_group(locator);
        let draft = GovernedEvidenceDocumentDraft {
            memory_space_id: memory.memory_space_id().into(),
            mounted_subject_id: memory.subject_id().into(),
            document_id: if external {
                "pfi2-external-source".into()
            } else {
                "pfi2-method-source".into()
            },
            source_kind: if !external {
                GovernedEvidenceDocumentSourceKind::StructuredMaterial
            } else {
                GovernedEvidenceDocumentSourceKind::ExternalContent
            },
            source_locator: locator.into(),
            canonical_evidence_group: group.clone(),
            evidence_family_group: None,
            source_revision: revision,
            content_digest: governed_evidence_document_content_digest(
                locator,
                &group,
                None,
                &body,
                &[],
            ),
            body,
            chunks: vec![],
            authority,
            privacy: MemoryPrivacyClass::SharedWithSubject,
            observed_at: 1_799_999_900 + revision,
        };
        memory
            .write(MemoryWriteRequest::GovernedEvidenceDocuments {
                mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                    draft: Box::new(draft),
                }],
            })
            .unwrap();
    };
    write_source(1, MemoryEvidenceAuthority::WorldObservation, false);
    let mut spec = grant(&governor).binding.spec;
    spec.binding_id = "real-governed-source".into();
    spec.claims.execution_facts = false;
    spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
        source_revision: GovernedOwnerRevisionRef::try_new(
            GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::EvidenceDocument,
                "pfi2-method-source",
            ),
            1,
        )
        .unwrap(),
    };
    let registered = governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "register-real-source".into(),
            spec,
            state: ProceduralProducerStateV1::Active,
            expected_revision: None,
        })
        .unwrap();
    let capability = governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    let candidate = |id: &str| {
        let mut value = request(&memory, id);
        let group = &mut value.learning.agent_tool_feedback[0];
        group.execution_facts.clear();
        let method = &mut group.method_evidence[0];
        method.body = "1. Check the synthetic input.\n2. Validate the extracted fields.".into();
        method.source_sensitivity = ProceduralSourceSensitivity::NonPrivate;
        method.execution_refs.clear();
        value
    };
    let positive = memory
        .finalize_turn_with_procedural_evidence(&capability, candidate("source-positive"))
        .unwrap();
    let first = memory
        .claim_due_procedural_feedback_job(
            &positive.procedural_learning.job_id.unwrap(),
            "source-worker",
            1_800_000_060,
        )
        .unwrap();
    memory
        .run_claimed_procedural_feedback_job(&first, "source-worker")
        .unwrap();
    let materials = platform
        .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
        .unwrap();
    assert!(
        materials.iter().any(|document| serde_json::from_value::<
            AgentToolExperienceRevisionMaterialV3,
        >(document.value.clone())
        .is_ok_and(|material| material.status
            == bm_core::skills::AgentToolExperienceStatus::Candidate
            && matches!(
                material.body,
                bm_core::skills::AgentToolExperienceBodyV1::Method { .. }
            ))),
        "a real governed method source must produce a candidate without inventing execution proof"
    );
    let submitted = memory
        .finalize_turn_with_procedural_evidence(&capability, candidate("source-before-change"))
        .unwrap();
    let claimed = memory
        .claim_due_procedural_feedback_job(
            &submitted.procedural_learning.job_id.unwrap(),
            "source-worker",
            1_800_000_060,
        )
        .unwrap();
    let pending = candidate("source-race");
    let context = memory.procedural_submission_context(&capability).unwrap();
    let evidence = memory
        .build_post_turn_learning_evidence(&pending, Some(&context))
        .unwrap();
    let proof = crate::learning::ProceduralIntakeAuthorization {
        store_authority_digest: platform.learning_store_authority_digest(),
        store_incarnation: capability.store_incarnation.clone(),
        current_head: context.head,
        evidence: evidence.clone(),
        source_preconditions: context.source_preconditions,
    };
    let plan = plan_canonical_turn_delta_with_transcript(
        platform.session_store().as_ref(),
        &platform,
        memory.memory_space_id(),
        &pending.turn,
        CanonicalTurnTranscriptCommitOptions {
            host_refs: vec![],
            learning_evidence: Some(evidence),
            conversation_alias: None,
            now_secs: 1_800_000_000,
        },
    )
    .unwrap();
    let subject_key = bm_core::memory::ProceduralSubjectScopeV1 {
        memory_space_id: memory.memory_space_id().into(),
        mounted_subject_id: memory.subject_id().into(),
    }
    .root_key()
    .unwrap();
    let read_root = || {
        serde_json::from_value::<bm_core::memory::ProceduralSubjectValidityRootV1>(
            platform
                .read_json_docs_by_keys(
                    crate::store_internal::schema::PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
                    std::slice::from_ref(&subject_key),
                )
                .unwrap()
                .pop()
                .unwrap()
                .value,
        )
        .unwrap()
    };
    let source_before = read_root();
    write_source(2, MemoryEvidenceAuthority::WorldObservation, false);
    let source_after = read_root();
    assert_eq!(source_after.validity_epoch, source_before.validity_epoch + 1,
            "source mutation must atomically fence existing learned candidates, not only reject future intake");
    assert!(!matches!(
        source_after.state,
        bm_core::memory::ProceduralSubjectValidityStateV1::Ready { .. }
    ));
    let before = image(&platform);
    assert!(memory
        .finalize_turn_with_procedural_evidence(&capability, candidate("source-after-change"))
        .is_err());
    assert!(
        plan.commit_with(|intent| platform.append_authorized_canonical_turn(intent, &proof))
            .is_err(),
        "an exact source read must be CAS-fenced against withdrawal before commit"
    );
    assert!(memory
        .run_claimed_procedural_feedback_job(&claimed, "source-worker")
        .is_err());
    assert_eq!(image(&platform), before);
    write_source(1, MemoryEvidenceAuthority::WorldObservation, true);
    let before = image(&platform);
    let mut external = registered.binding.spec.clone();
    external.binding_id = "external-kind-source".into();
    if let ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } =
        &mut external.source_authority
    {
        source_revision.owner_ref.owner_id = "pfi2-external-source".into();
    }
    assert!(
        governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "register-external-kind".into(),
                spec: external,
                state: ProceduralProducerStateV1::Active,
                expected_revision: None,
            })
            .is_err(),
        "an explicitly external source kind must not be laundered through a WorldObservation tag"
    );
    assert_eq!(image(&platform), before);
    governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "withdraw-invalid-source".into(),
            spec: registered.binding.spec.clone(),
            state: ProceduralProducerStateV1::Revoked,
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
        })
        .unwrap();
}

fn shared_source_runtime(platform: &StorePlatform, agent: &str, governor: bool) -> MemoryRuntime {
    let mut subjects = SubjectRegistry::single_agent_default("pfi2-owner", "pfi2-agent").unwrap();
    subjects
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("pfi2-agent-b"),
            "Synthetic B",
        ))
        .unwrap();
    subjects
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("pfi2-agent-c"),
            "Synthetic C",
        ))
        .unwrap();
    let mut scoped = SubjectScopedRuntime::single_agent_default("pfi2-owner", agent, None).unwrap();
    if governor {
        scoped.actor_subject_id = system_governor_subject_id("pfi2-owner");
    }
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent, "pfi2-owner").unwrap())
        .subject_registry(subjects)
        .scoped_runtime(scoped)
        .scope(MemoryScope::new("sdk.direct", format!("pfi2-chat-{agent}")).unwrap())
        .store(crate::MemoryStoreHandle::from_platform(platform.clone()))
        .clock(Arc::new(Clock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .agent_tool_registry(registry())
        .build()
        .unwrap()
}
fn shared_sources(memory: &MemoryRuntime, revision: u64) -> MemoryWriteRequest {
    MemoryWriteRequest::GovernedEvidenceDocuments {
        mutations: ["shared-method-a", "shared-method-b"]
            .into_iter()
            .map(|id| {
                let locator = format!("synthetic://pfi2/{id}");
                let group = canonical_recall_evidence_group(&locator);
                let body = format!("Synthetic method foundation {id}, revision {revision}");
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: Box::new(GovernedEvidenceDocumentDraft {
                        memory_space_id: memory.memory_space_id().into(),
                        mounted_subject_id: memory.subject_id().into(),
                        document_id: id.into(),
                        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
                        source_locator: locator.clone(),
                        canonical_evidence_group: group.clone(),
                        evidence_family_group: None,
                        source_revision: revision,
                        content_digest: governed_evidence_document_content_digest(
                            &locator,
                            &group,
                            None,
                            &body,
                            &[],
                        ),
                        body,
                        chunks: vec![],
                        authority: MemoryEvidenceAuthority::WorldObservation,
                        privacy: MemoryPrivacyClass::PublicRuntime,
                        observed_at: 1_799_999_900 + revision,
                    }),
                }
            })
            .collect(),
    }
}
#[test]
fn pfi2_shared_sources_fence_every_dependent_once_and_reopen() {
    shared_sources_fence_every_dependent_once_and_reopen("write");
}

#[test]
fn pfi2_archive_changed_sources_fence_every_dependent_once_and_reopen() {
    shared_sources_fence_every_dependent_once_and_reopen("archive_update");
}

#[test]
fn pfi2_archive_plain_tool_turn_replacement_preserves_other_subject_queries() {
    let directory = std::env::temp_dir().join(format!(
        "bm-pfi2-plain-archive-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let profile = platform().config().profile;
    for backend in ["memory", "file", "sqlite"] {
        let config = match backend {
            "memory" => crate::StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => crate::StoreBackendConfig::file(directory.join("file"), profile).unwrap(),
            "sqlite" => {
                crate::StoreBackendConfig::sqlite(directory.join("sqlite.db"), profile).unwrap()
            }
            _ => unreachable!(),
        };
        let platform = StorePlatform::open(config.clone()).unwrap();
        for agent in ["pfi2-agent", "pfi2-agent-b"] {
            let memory = shared_source_runtime(&platform, agent, false);
            let mut turn = request(&memory, "plain-tool-turn").turn;
            turn.conversation.chat_id = memory.scope().chat_id.clone();
            turn.conversation.conversation_id = Some(memory.scope().chat_id.clone());
            memory
                .commit_transcript(crate::MemoryTranscriptCommitRequest {
                    turn,
                    host_refs: Vec::new(),
                })
                .unwrap();
        }
        let records = platform
            .read_json_namespace("conversation_transcript")
            .unwrap();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(
                |doc| serde_json::from_value::<TranscriptTurnRecord>(doc.value.clone())
                    .is_ok_and(|record| record.learning_evidence.is_none()
                        && !record.tool_observations.is_empty())
            ));
        assert!(platform
            .read_json_namespace(crate::store_internal::schema::PROCEDURAL_FEEDBACK_JOB_NAMESPACE)
            .unwrap()
            .is_empty());
        let a = shared_source_runtime(&platform, "pfi2-agent", false);
        let scope =
            crate::MemoryArchiveScope::subject(a.memory_space_id(), a.subject_id()).unwrap();
        let archive = crate::export_memory_space_from_platform_with_budget(
            &platform,
            crate::MemorySpaceExportRequest {
                scope: scope.clone(),
                private_material_policy: crate::MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            },
            None,
        )
        .unwrap()
        .archive;
        crate::import_memory_space_from_platform_with_budget(
            &platform,
            crate::MemorySpaceImportRequest {
                scope,
                expected_private_material_policy:
                    crate::MemorySpacePrivateMaterialPolicy::ExcludePrivate,
                archive,
            },
            None,
        )
        .expect("plain tool observations are not protected learning authority");
        drop(a);
        let platform = if backend == "memory" {
            platform
        } else {
            drop(platform);
            StorePlatform::open(config).expect("other subject keyring survives reopen")
        };
        assert_eq!(
            platform
                .read_json_namespace("conversation_transcript")
                .unwrap()
                .len(),
            1
        );
        let b = shared_source_runtime(&platform, "pfi2-agent-b", false);
        let page = b
            .query_transcript_timeline(crate::MemoryTranscriptTimelineRequest {
                channel_id: b.scope().channel.clone(),
                conversation_id: b.scope().chat_id.clone(),
                anchor: TranscriptTimelineAnchor::Latest,
                limit: 8,
                cursor: None,
                view: TranscriptReplayView::HostUi,
            })
            .unwrap();
        assert_eq!(
            page.page.turns.len(),
            1,
            "other subject retains a real query positive"
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn pfi2_archive_removed_sources_fence_every_dependent_once_and_reopen() {
    shared_sources_fence_every_dependent_once_and_reopen("archive_delete");
}

fn shared_sources_fence_every_dependent_once_and_reopen(change: &str) {
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-shared-source-{change}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    let profile = platform().config().profile;
    for backend in ["memory", "file", "sqlite"] {
        let config = match backend {
            "memory" => crate::StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => crate::StoreBackendConfig::file(directory.0.join("file"), profile).unwrap(),
            "sqlite" => {
                crate::StoreBackendConfig::sqlite(directory.0.join("sqlite.db"), profile).unwrap()
            }
            _ => unreachable!(),
        };
        let platform = StorePlatform::open(config.clone()).unwrap();
        let a = shared_source_runtime(&platform, "pfi2-agent", false);
        a.write(shared_sources(&a, 1))
            .unwrap_or_else(|error| panic!("{backend}/{change}: source initialization: {error:?}"));
        let mut subject_epochs = BTreeMap::new();
        for agent in ["pfi2-agent", "pfi2-agent-b"] {
            let governor = shared_source_runtime(&platform, agent, true);
            let memory = shared_source_runtime(&platform, agent, false);
            let template = grant(&governor).binding.spec;
            for id in ["shared-method-a", "shared-method-b"] {
                let mut spec = template.clone();
                spec.binding_id = id.into();
                spec.claims.execution_facts = false;
                spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
                    source_revision: GovernedOwnerRevisionRef::try_new(
                        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::EvidenceDocument, id),
                        1,
                    )
                    .unwrap(),
                };
                let registered = governor
                    .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                        operation_id: format!("register-{id}"),
                        expected_revision: None,
                        state: ProceduralProducerStateV1::Active,
                        spec,
                    })
                    .unwrap_or_else(|error| {
                        panic!("{backend}/{change}/{agent}/{id}: source reference: {error:?}")
                    });
                let capability = governor
                    .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
                    .unwrap();
                let mut candidate = request(&memory, id);
                candidate.turn.conversation.chat_id = memory.scope().chat_id.clone();
                candidate.turn.conversation.conversation_id = Some(memory.scope().chat_id.clone());
                candidate.learning.agent_tool_feedback[0]
                    .execution_facts
                    .clear();
                candidate.learning.agent_tool_feedback[0].method_evidence[0]
                    .execution_refs
                    .clear();
                candidate.learning.agent_tool_feedback[0].method_evidence[0].source_sensitivity =
                    ProceduralSourceSensitivity::NonPrivate;
                candidate.learning.agent_tool_feedback[0].method_evidence[0].body =
                    "1. Check the synthetic input.\n2. Validate the extracted fields.".into();
                let report = memory
                    .finalize_turn_with_procedural_evidence(&capability, candidate)
                    .unwrap();
                let job = memory
                    .claim_due_procedural_feedback_job(
                        &report.procedural_learning.job_id.unwrap(),
                        "shared-source-worker",
                        1_800_000_060,
                    )
                    .unwrap();
                memory
                    .run_claimed_procedural_feedback_job(&job, "shared-source-worker")
                    .unwrap_or_else(|error| panic!("{backend}/{agent}/{id}: {error:?}"));
            }
            let key = ProceduralSubjectScopeV1 {
                memory_space_id: memory.memory_space_id().into(),
                mounted_subject_id: memory.subject_id().into(),
            }
            .root_key()
            .unwrap();
            let root: ProceduralSubjectValidityRootV1 = serde_json::from_value(
                platform
                    .read_json_docs_by_keys(
                        crate::store_internal::schema::PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
                        std::slice::from_ref(&key),
                    )
                    .unwrap()
                    .pop()
                    .unwrap()
                    .value,
            )
            .unwrap();
            assert!(root
                .permits_learning_reads(MAX_PROCEDURAL_SUBJECT_SCOPES)
                .unwrap());
            let learned = platform
                .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
                .unwrap();
            assert!(
                learned.iter().any(|doc| serde_json::from_value::<
                    AgentToolExperienceRevisionMaterialV3,
                >(doc.value.clone())
                .is_ok_and(|material| material.owning_scope
                    == bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
                        mounted_subject_id: memory.subject_id().into()
                    }
                    && matches!(
                        material.body,
                        bm_core::skills::AgentToolExperienceBodyV1::Method { .. }
                    ))),
                "every dependent must have actual learned material"
            );
            subject_epochs.insert(key, root.validity_epoch);
        }
        if change == "write" {
            a.write(shared_sources(&a, 2)).unwrap();
        } else {
            let scope =
                crate::MemoryArchiveScope::subject(a.memory_space_id(), a.subject_id()).unwrap();
            let redacted = crate::export_memory_space_from_platform_with_budget(
                &platform,
                crate::MemorySpaceExportRequest {
                    scope: scope.clone(),
                    private_material_policy:
                        crate::MemorySpacePrivateMaterialPolicy::ExcludePrivate,
                },
                None,
            )
            .unwrap()
            .archive;
            let before_rejected = image(&platform);
            let error = crate::import_memory_space_from_platform_with_budget(
                &platform,
                crate::MemorySpaceImportRequest {
                    scope: scope.clone(),
                    expected_private_material_policy:
                        crate::MemorySpacePrivateMaterialPolicy::ExcludePrivate,
                    archive: redacted,
                },
                None,
            )
            .expect_err("a public-only replacement cannot remove retained local learning sources");
            let Error::Other { source, .. } = error else {
                panic!("expected typed archive conflict: {error:?}")
            };
            assert_eq!(
                source.downcast_ref::<crate::MemorySpaceImportConflict>(),
                Some(&crate::MemorySpaceImportConflict::ProtectedTranscriptSourceMissing)
            );
            assert_eq!(
                image(&platform),
                before_rejected,
                "rejected public archive must leave every owner and event unchanged"
            );
            let export = |store: &StorePlatform| {
                crate::export_memory_space_from_platform_with_budget(
                    store,
                    crate::MemorySpaceExportRequest {
                        scope: scope.clone(),
                        private_material_policy:
                            crate::MemorySpacePrivateMaterialPolicy::IncludePrivate,
                    },
                    None,
                )
                .unwrap()
                .archive
            };
            let import = |store: &StorePlatform, archive| {
                crate::import_memory_space_from_platform_with_budget(
                    store,
                    crate::MemorySpaceImportRequest {
                        scope: scope.clone(),
                        expected_private_material_policy:
                            crate::MemorySpacePrivateMaterialPolicy::IncludePrivate,
                        archive,
                    },
                    None,
                )
            };
            let original = export(&platform);
            let original_transcripts = platform
                .read_json_namespace("conversation_transcript")
                .unwrap();
            assert!(original_transcripts.iter().any(|document| {
                serde_json::from_value::<TranscriptTurnRecord>(document.value.clone()).is_ok_and(
                    |record| record.subject == a.subject_id() && record.learning_evidence.is_some(),
                )
            }));
            for changed_field in ["body", "host_ref", "lifecycle"] {
                // Produce a completely valid alternative archive through the
                // real public runtime; no stale search/catalog hashes can
                // reject the fixture before the retained-source boundary.
                let alternative =
                    StorePlatform::open(crate::StoreBackendConfig::in_memory(profile).unwrap())
                        .unwrap();
                let alternate_runtime = shared_source_runtime(&alternative, "pfi2-agent", false);
                for id in ["shared-method-a", "shared-method-b"] {
                    let mut turn = request(&alternate_runtime, id).turn;
                    turn.conversation.chat_id = alternate_runtime.scope().chat_id.clone();
                    turn.conversation.conversation_id =
                        Some(alternate_runtime.scope().chat_id.clone());
                    let mut host_refs = Vec::new();
                    if changed_field == "body" {
                        turn.input_messages
                            .first_mut()
                            .unwrap()
                            .content
                            .push_str(" synthetic archive replacement");
                    } else if changed_field == "host_ref" {
                        host_refs.push(HostOpaqueRef {
                            host_kind: "synthetic-host".into(),
                            business_ref_type: "synthetic-record".into(),
                            business_ref_id: "changed-reference".into(),
                            relation: HostRefRelation::Related,
                            visibility: HostRefVisibility::HostUi,
                            label: None,
                        });
                    }
                    alternate_runtime
                        .commit_transcript(crate::MemoryTranscriptCommitRequest { turn, host_refs })
                        .unwrap();
                }
                if changed_field == "lifecycle" {
                    alternate_runtime
                        .request_transcript_lifecycle(crate::MemoryTranscriptLifecycleRequest {
                            memory_space_id: alternate_runtime.memory_space_id().into(),
                            channel_id: alternate_runtime.scope().channel.clone(),
                            conversation_id: alternate_runtime.scope().chat_id.clone(),
                            turn_id: None,
                            transition: TranscriptLifecycleTransition::Archive,
                            reason: "synthetic alternative archive lifecycle".into(),
                        })
                        .unwrap();
                }
                let changed_archive = export(&alternative);
                let before = image(&platform);
                let error = import(&platform, changed_archive)
                    .expect_err("archive cannot rewrite an existing transcript source");
                let Error::Other { source, .. } = error else {
                    panic!("{backend}/{changed_field}: expected typed source conflict: {error:?}")
                };
                assert_eq!(
                    source.downcast_ref::<crate::MemorySpaceImportConflict>(),
                    Some(&crate::MemorySpaceImportConflict::ExistingTranscriptDiffers),
                    "{backend}/{changed_field}",
                );
                assert_eq!(image(&platform), before, "{backend}/{changed_field}");
            }
            let prior_roots = platform
                .read_json_namespace(
                    crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE,
                )
                .unwrap();
            let prior_subjects = platform
                .read_json_namespace(
                    crate::store_internal::schema::PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
                )
                .unwrap();
            import(&platform, original.clone())
                .expect("identical archive preserves target authority");
            assert_eq!(
                platform
                    .read_json_namespace("conversation_transcript")
                    .unwrap(),
                original_transcripts,
                "same-store restore must preserve the actual complete evidence record"
            );
            assert_eq!(
                platform
                    .read_json_namespace(
                        crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE
                    )
                    .unwrap(),
                prior_roots
            );
            assert_eq!(
                platform
                    .read_json_namespace(
                        crate::store_internal::schema::PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE
                    )
                    .unwrap(),
                prior_subjects,
                "identical sources cannot advance subject epochs"
            );
            let donor = StorePlatform::open(crate::StoreBackendConfig::in_memory(profile).unwrap())
                .unwrap();
            import(&donor, original.clone()).unwrap();
            let donor_records = donor
                .read_json_namespace("conversation_transcript")
                .unwrap();
            assert!(!donor_records.is_empty());
            assert!(
                donor_records.iter().all(|document| {
                    serde_json::from_value::<TranscriptTurnRecord>(document.value.clone())
                        .is_ok_and(|record| {
                            record.learning_evidence.is_none()
                                && record.tool_observations.is_empty()
                        })
                }),
                "a fresh target cannot inherit the source store's execution authority"
            );
            assert!(donor
                .read_json_namespace(
                    crate::store_internal::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE
                )
                .unwrap()
                .is_empty());
            let donor_runtime = shared_source_runtime(&donor, "pfi2-agent", false);
            if change == "archive_delete" {
                donor_runtime
                    .write(MemoryWriteRequest::GovernedEvidenceDocuments {
                        mutations: ["shared-method-a", "shared-method-b"]
                            .into_iter()
                            .map(|id| MemoryEvidenceDocumentMutation::Delete {
                                document_id: id.into(),
                                expected_owner_revision: 1,
                            })
                            .collect(),
                    })
                    .unwrap();
            } else {
                donor_runtime
                    .write(shared_sources(&donor_runtime, 2))
                    .unwrap();
            }
            import(&platform, export(&donor))
                .expect("changed archive closes retained target dependents");
            let after = image(&platform);
            assert!(
                import(&platform, original).is_err(),
                "old archive must not resurrect or roll back a source identity"
            );
            assert_eq!(
                image(&platform),
                after,
                "rejected archive cannot partly replace root/job/material/event"
            );
        }
        let roots = platform
            .read_json_namespace(
                crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE,
            )
            .unwrap();
        assert!(!roots.is_empty());
        for document in &roots {
            let root: ProceduralSourceDependentsRootV1 =
                serde_json::from_value(document.value.clone()).unwrap();
            root.validate().unwrap();
            assert_eq!(
                root.dependents
                    .iter()
                    .map(|item| item.scope.mounted_subject_id.clone())
                    .collect::<BTreeSet<_>>(),
                [
                    default_agent_subject_id("pfi2-agent"),
                    default_agent_subject_id("pfi2-agent-b")
                ]
                .into_iter()
                .collect()
            );
        }
        let check = |platform: &StorePlatform| {
            for (key, epoch) in &subject_epochs {
                let root: ProceduralSubjectValidityRootV1 = serde_json::from_value(
                    platform
                        .read_json_docs_by_keys(
                            crate::store_internal::schema::PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
                            std::slice::from_ref(key),
                        )
                        .unwrap()
                        .pop()
                        .unwrap()
                        .value,
                )
                .unwrap();
                assert_eq!(
                    root.validity_epoch,
                    epoch + 1,
                    "two changed sources must cause exactly one subject epoch"
                );
                assert!(!root
                    .permits_learning_reads(MAX_PROCEDURAL_SUBJECT_SCOPES)
                    .unwrap());
                let job = crate::store_internal::procedural_feedback::read_job(
                    platform,
                    &root.retained_work.last().unwrap().job_id,
                )
                .unwrap()
                .unwrap();
                let ProceduralLearningWorkV1::Reconcile { source } = job.work else {
                    panic!("source change must create real reconciliation work")
                };
                let ProceduralReconciliationTriggerV1::SourceChange { changes, .. } =
                    source.trigger
                else {
                    panic!("wrong cause")
                };
                assert_eq!(
                    changes
                        .iter()
                        .map(|change| change.owner_ref.owner_id.as_str())
                        .collect::<Vec<_>>(),
                    vec!["shared-method-a", "shared-method-b"]
                );
            }
        };
        check(&platform);
        drop(a);
        let platform = if backend != "memory" {
            drop(platform);
            let reopened = StorePlatform::open(config).unwrap();
            check(&reopened);
            assert_eq!(
                reopened
                    .read_json_namespace(
                        crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE
                    )
                    .unwrap(),
                roots
            );
            reopened
        } else {
            platform
        };
        if change != "write" {
            for agent in ["pfi2-agent", "pfi2-agent-b"] {
                let memory = shared_source_runtime(&platform, agent, false);
                let mut completed = false;
                for _ in 0..64 {
                    let job = memory
                        .due_procedural_feedback_jobs(8)
                        .unwrap()
                        .into_iter()
                        .find(|job| matches!(job.work, ProceduralLearningWorkV1::Reconcile { .. }))
                        .expect("archive reconciliation must remain discoverable after reopen");
                    let claimed = memory
                        .claim_due_procedural_feedback_job(
                            &job.job_id,
                            "archive-worker",
                            1_800_000_060,
                        )
                        .unwrap();
                    let page = memory
                        .run_claimed_procedural_reconciliation_job(&claimed, "archive-worker")
                        .unwrap_or_else(|error| {
                            panic!("{backend}/{change}/{agent} archive reconciliation: {error:?}")
                        });
                    if page.completed {
                        completed = true;
                        break;
                    }
                }
                assert!(
                    completed,
                    "archive source withdrawal must complete actual bounded work"
                );
            }
        }
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn pfi2_skill_control_rejects_foreign_owner_before_reading_its_body() {
    use crate::runtime::recall_immutable_session_observer_tests::ObservedStoreEngine;
    use crate::store_internal::schema::RUNTIME_SKILL_RECORD_NAMESPACE;
    let base = platform();
    let a = shared_source_runtime(&base, "pfi2-agent", false);
    let b = shared_source_runtime(&base, "pfi2-agent-b", false);
    b.seed_runtime_skills_for_replay(
        vec![crate::GovernedRuntimeSkillWriteInput {
            write: bm_core::skills::RuntimeSkillWrite {
                name: "runtime_skill__foreign".into(),
                title: "Foreign skill".into(),
                topic: "private synthetic workflow".into(),
                summary: "Synthetic B method".into(),
                content: "1. Read the B-only synthetic input\n2. Validate the B-only output".into(),
                citations: vec!["synthetic-b".into()],
                source_chat_id: Some(b.scope().chat_id.clone()),
                observed_at: 1_800_000_000,
            },
            creation_ref: bm_core::skills::RuntimeSkillCreationRef::ReplayPromotion {
                candidate_ref: "synthetic-b".into(),
                verification_receipt_digest: format!("sha256:{}", "0".repeat(64)),
            },
            privacy_class: MemoryPrivacyClass::SharedWithSubject,
        }],
        bm_core::skills::RuntimeSkillOwningScope::Subject {
            mounted_subject_id: b.subject_id().into(),
        },
    )
    .unwrap();
    let foreign = base
        .read_json_namespace(RUNTIME_SKILL_RECORD_NAMESPACE)
        .unwrap()
        .pop()
        .expect("real foreign owner positive");
    let observer = Arc::new(ObservedStoreEngine::new(base.engine_for_test()));
    let observed = base.with_engine_for_test(observer.clone());
    let before = image(&base);
    observer.begin_source_read_observation();
    let result = observed.commit_governed_memory_transaction(crate::StoreMutationBatch {
        transaction_id: "foreign-skill-control".into(),
        operation: "runtime_skill.retire".into(),
        scope: a.memory_write_transaction_scope(),
        mutations: vec![crate::StoreMutation::DeleteJson {
            namespace: foreign.namespace.clone(),
            key: foreign.key.clone(),
            event_kind: crate::MemoryStoreEventKind::MemoryDelete,
            plane: "procedural_memory".into(),
            record_key: foreign.key.clone(),
        }],
    });
    assert!(result.is_err(), "foreign control must fail closed");
    assert!(
        !observer
            .requested_json_keys()
            .contains(&(foreign.namespace, foreign.key)),
        "foreign skill body must never be requested during planning"
    );
    assert_eq!(image(&base), before);
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn pfi2_corrupt_source_dependent_index_never_reads_other_subject_bodies() {
    use crate::runtime::recall_immutable_session_observer_tests::ObservedStoreEngine;
    use crate::store_internal::schema::{
        PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE as LEDGER,
        PROCEDURAL_FEEDBACK_JOB_NAMESPACE, PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE as INDEX,
    };
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-source-reads-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    let profile = platform().config().profile;
    for backend in ["memory", "file", "sqlite"] {
        let config = match backend {
            "memory" => crate::StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => crate::StoreBackendConfig::file(directory.0.join("file"), profile).unwrap(),
            "sqlite" => {
                crate::StoreBackendConfig::sqlite(directory.0.join("sqlite.db"), profile).unwrap()
            }
            _ => unreachable!(),
        };
        let base = StorePlatform::open(config).unwrap();
        let a = shared_source_runtime(&base, "pfi2-agent", false);
        a.write(shared_sources(&a, 1)).unwrap();
        let b_governor = shared_source_runtime(&base, "pfi2-agent-b", true);
        let mut spec = grant(&b_governor).binding.spec;
        spec.binding_id = "source-b".into();
        spec.claims.execution_facts = false;
        spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
            source_revision: GovernedOwnerRevisionRef::try_new(
                GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    "shared-method-a",
                ),
                1,
            )
            .unwrap(),
        };
        let b = b_governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "register-source-b".into(),
                spec,
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
            })
            .unwrap();
        let c = shared_source_runtime(&base, "pfi2-agent-c", false);
        let c_governor = shared_source_runtime(&base, "pfi2-agent-c", true);
        let c_producer = grant(&c_governor);
        let capability = c_governor
            .procedural_submission_capability(&c_producer.binding.revision_ref().unwrap())
            .unwrap();
        let mut turn = request(&c, "private-c-turn");
        turn.turn.conversation.chat_id = c.scope().chat_id.clone();
        turn.turn.conversation.conversation_id = Some(c.scope().chat_id.clone());
        let report = c
            .finalize_turn_with_procedural_evidence(&capability, turn)
            .unwrap();
        let claimed = c
            .claim_due_procedural_feedback_job(
                &report.procedural_learning.job_id.unwrap(),
                "private-c-worker",
                1_800_000_060,
            )
            .unwrap();
        c.run_claimed_procedural_feedback_job(&claimed, "private-c-worker")
            .unwrap();
        let completed =
            crate::store_internal::procedural_feedback::read_job(&base, &claimed.job_id)
                .unwrap()
                .unwrap();
        let c_materials = base
            .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
            .unwrap()
            .into_iter()
            .filter(|document| {
                serde_json::from_value::<AgentToolExperienceRevisionMaterialV3>(
                    document.value.clone(),
                )
                .unwrap()
                .owning_scope
                    == bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
                        mounted_subject_id: c.subject_id().into(),
                    }
            })
            .collect::<Vec<_>>();
        assert!(
            !c_materials.is_empty(),
            "foreign body negative needs a real learned positive"
        );
        let b_key = b.binding.spec.scope.scope_index_key().unwrap();
        let before_index = base
            .read_json_docs_by_keys(INDEX, std::slice::from_ref(&b_key))
            .unwrap()
            .pop()
            .unwrap()
            .value;
        let c_key = c_producer.binding.spec.scope.scope_index_key().unwrap();
        let c_index: ProceduralFeedbackScopeIndexV2 = serde_json::from_value(
            base.read_json_docs_by_keys(INDEX, std::slice::from_ref(&c_key))
                .unwrap()
                .pop()
                .unwrap()
                .value,
        )
        .unwrap();
        let mut corrupted: ProceduralFeedbackScopeIndexV2 =
            serde_json::from_value(before_index.clone()).unwrap();
        corrupted.recent_terminal_jobs.push(
            c_index
                .recent_terminal_jobs
                .iter()
                .find(|item| item.job_id == completed.job_id)
                .unwrap()
                .clone(),
        );
        corrupted.validate().unwrap();
        base.tamper_json_document_for_nonproduction_harness(
            INDEX,
            &b_key,
            serde_json::to_value(corrupted).unwrap(),
        )
        .unwrap();
        let observer = Arc::new(ObservedStoreEngine::new(base.engine_for_test()));
        let observed_platform = base.with_engine_for_test(observer.clone());
        let observed_a = shared_source_runtime(&observed_platform, "pfi2-agent", false);
        let before = image(&base);
        observer.begin_source_read_observation();
        let error = observed_a
            .write(shared_sources(&observed_a, 2))
            .expect_err("foreign dependency must fail before body read");
        assert_eq!(error.stage(), "procedural_source_closure_repair_required");
        let requested = observer.requested_json_keys();
        assert!(
            requested.contains(&(
                PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(),
                completed.job_id.clone()
            )),
            "test must reach the conflicting job identity"
        );
        let source = completed.feedback_source().unwrap();
        let transcript_key = crate::store_internal::transcript_turn_storage_key(
            &ConversationKey::new(
                &source.identity.memory_space_id,
                &source.identity.channel_id,
                &source.identity.conversation_id,
            )
            .unwrap(),
            &source.identity.mounted_subject_id,
            &source.identity.turn_id,
        );
        assert!(
            !requested.contains(&("conversation_transcript".into(), transcript_key)),
            "foreign transcript body was requested"
        );
        assert!(
            !requested.contains(&(LEDGER.into(), completed.job_id)),
            "foreign contribution ledger was requested"
        );
        for document in c_materials {
            assert!(
                !requested.contains(&(document.namespace, document.key)),
                "foreign learned body was requested"
            );
        }
        assert!(
            image(&base) == before,
            "read rejection cannot modify any owner or event"
        );
        base.tamper_json_document_for_nonproduction_harness(INDEX, &b_key, before_index)
            .unwrap();
        observed_a.write(shared_sources(&observed_a, 2)).unwrap();
    }
}

#[test]
fn pfi2_source_delete_is_atomic_irreversible_and_reopen_preserves_identity() {
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-source-delete-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    let profile = platform().config().profile;
    for backend in ["memory", "file", "sqlite"] {
        let config = match backend {
            "memory" => crate::StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => crate::StoreBackendConfig::file(directory.0.join("file"), profile).unwrap(),
            "sqlite" => {
                crate::StoreBackendConfig::sqlite(directory.0.join("sqlite.db"), profile).unwrap()
            }
            _ => unreachable!(),
        };
        let platform = StorePlatform::open(config.clone()).unwrap();
        let memory = shared_source_runtime(&platform, "pfi2-agent", false);
        memory.write(shared_sources(&memory, 1)).unwrap();
        let delete = |expected| MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: ["shared-method-a", "shared-method-b"]
                .into_iter()
                .map(|id| MemoryEvidenceDocumentMutation::Delete {
                    document_id: id.into(),
                    expected_owner_revision: expected,
                })
                .collect(),
        };
        let before = image(&platform);
        assert!(memory.write(delete(2)).is_err());
        assert_eq!(
            image(&platform),
            before,
            "stale source deletion cannot modify source/root/event"
        );
        memory.write(delete(1)).unwrap();
        assert!(platform
            .read_json_namespace(crate::store_internal::GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
            .unwrap()
            .is_empty());
        let roots = platform
            .read_json_namespace(
                crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE,
            )
            .unwrap();
        assert!(!roots.is_empty());
        for document in &roots {
            let root: ProceduralSourceDependentsRootV1 =
                serde_json::from_value(document.value.clone()).unwrap();
            root.validate().unwrap();
            assert_eq!(root.current, ProceduralSourcePostImageV1::Deleted);
            assert!(matches!(
                root.last_change.before,
                Some(ProceduralSourcePostImageV1::Present { .. })
            ));
        }
        drop(memory);
        let platform = if backend == "memory" {
            platform
        } else {
            drop(platform);
            StorePlatform::open(config).unwrap()
        };
        assert_eq!(
            platform
                .read_json_namespace(
                    crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE
                )
                .unwrap(),
            roots
        );
        let memory = shared_source_runtime(&platform, "pfi2-agent", false);
        let deleted = image(&platform);
        let error = memory
            .write(shared_sources(&memory, 2))
            .expect_err("deleted source ID cannot be recreated");
        let Error::Other { source, .. } = error else {
            panic!("source deletion must return a typed lifecycle error: {error:?}");
        };
        assert_eq!(
            source.downcast_ref::<GovernedSourceLifecycleError>(),
            Some(&GovernedSourceLifecycleError::DeletedIdentityCannotBeRecreated)
        );
        assert_eq!(
            image(&platform),
            deleted,
            "failed recreation must preserve tombstone, receipts and events"
        );
    }
}

#[test]
fn pfi2_long_term_source_respects_current_staleness_and_exact_correction() {
    for change in ["corrected", "stale", "revised"] {
        let corrected = change == "corrected";
        let platform = platform();
        let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
        let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
        let draft = LongTermMemoryDraft {
            kind: LongTermMemoryKind::Fact,
            topic: "synthetic method foundation".into(),
            content: "The synthetic extraction requires field validation.".into(),
            keywords: vec!["extraction".into()],
            privacy: MemoryPrivacyClass::SharedWithSubject,
            source_chat_id: Some("pfi2-chat".into()),
            source_type: None,
            source_scope: None,
            subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            provenance: LongTermMemoryProvenance::new(
                MemoryEvidenceAuthority::ProgramMemoryCanonical,
            ),
            confidence: None,
            freshness: None,
            stale_hint: None,
            supporting_citations: vec!["synthetic:pfi2-source".into()],
            canonical_entities: vec![],
            evidence_count: Some(1),
            observed_at: Some(1_799_999_900),
            source_revision: Some(1),
        };
        memory
            .write(MemoryWriteRequest::LongTermExtraction {
                extraction: ParsedLongTermMemoryExtraction {
                    upserts: vec![draft.clone()],
                    deletes: vec![],
                    skill_writes: vec![],
                },
            })
            .unwrap();
        let entry = memory
            .list_long_term_memory(MemoryLongTermListRequest {
                query: LongTermMemoryQuery::default(),
                cursor: None,
                limit: 10,
                view: MemoryLongTermControlView::HostUi,
            })
            .unwrap()
            .records
            .pop()
            .unwrap()
            .record;
        let mut spec = grant(&governor).binding.spec;
        spec.binding_id = "long-term-source".into();
        spec.claims.execution_facts = false;
        spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
            source_revision: GovernedOwnerRevisionRef::try_new(
                GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, &entry.id),
                entry.owner_revision,
            )
            .unwrap(),
        };
        let registered = governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "register-long-term-source".into(),
                spec,
                state: ProceduralProducerStateV1::Active,
                expected_revision: None,
            })
            .unwrap();
        let capability = governor
            .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
            .unwrap();
        assert!(memory.procedural_submission_context(&capability).is_ok());
        if change == "revised" {
            let mut updated = draft;
            updated.content =
                "The synthetic extraction requires field validation and a result check.".into();
            updated.source_revision = Some(2);
            memory
                .write(MemoryWriteRequest::LongTermExtraction {
                    extraction: ParsedLongTermMemoryExtraction {
                        upserts: vec![updated],
                        deletes: vec![],
                        skill_writes: vec![],
                    },
                })
                .unwrap();
            let updated = memory
                .list_long_term_memory(MemoryLongTermListRequest {
                    query: LongTermMemoryQuery::default(),
                    cursor: None,
                    limit: 10,
                    view: MemoryLongTermControlView::HostUi,
                })
                .unwrap()
                .records
                .into_iter()
                .find(|record| record.record.id == entry.id)
                .unwrap()
                .record;
            assert!(
                updated.owner_revision > entry.owner_revision,
                "the retained-source positive must advance the real owner"
            );
        } else {
            let operation = if corrected {
                let mut replacement = draft;
                replacement.content =
                    "The previous synthetic extraction requirement was incorrect.".into();
                MemoryLongTermMutation::Correct {
                    target: MemoryLongTermTarget::RecordId(entry.id.clone()),
                    replacement,
                }
            } else {
                MemoryLongTermMutation::MarkStale {
                    target: MemoryLongTermTarget::RecordId(entry.id.clone()),
                    stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
                }
            };
            memory
                .mutate_long_term_memory(MemoryLongTermMutationRequest {
                    operation,
                    reason: "synthetic source governance".into(),
                    dry_run: false,
                    mode_input: RuntimeLifecycleModeInput::default(),
                })
                .unwrap();
        }
        let before = image(&platform);
        assert_eq!(
            memory.procedural_submission_context(&capability).is_ok(),
            change == "revised",
            "retained material must use exact lifecycle semantics: {change}"
        );
        let mut next = registered.binding.spec.clone();
        next.binding_id = "regrant-old-long-term-source".into();
        assert_eq!(
            governor
                .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                    operation_id: "regrant-obsolete-source".into(),
                    spec: next,
                    state: ProceduralProducerStateV1::Active,
                    expected_revision: None,
                })
                .is_ok(),
            change == "revised"
        );
        if change != "revised" {
            assert_eq!(image(&platform), before);
        }
    }
}

#[test]
fn pfi2_human_confirmation_requires_the_actual_human_actor() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let human_id = governor
        .subject_registry()
        .primary_human_user()
        .unwrap()
        .subject_id
        .clone();
    let human = runtime(&platform, &human_id);
    let mut spec = grant(&governor).binding.spec;
    spec.binding_id = "human-declarations".into();
    spec.source_authority = ProceduralProducerSourceAuthorityV1::HumanUser {
        subject_id: human_id.clone(),
    };
    let registered = governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "register-human".into(),
            spec,
            state: ProceduralProducerStateV1::Active,
            expected_revision: None,
        })
        .unwrap();
    let capability = governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    let mut confirmed = request(&human, "real-human-confirmed");
    confirmed.learning.human_confirmation_operation_id = Some("confirm-real-human-evidence".into());
    human
        .finalize_turn_with_procedural_evidence(&capability, confirmed)
        .unwrap();
    let key = ConversationKey::new(human.memory_space_id(), "sdk.direct", "pfi2-chat").unwrap();
    let record = platform
        .get_turn(&key, human.subject_id(), "real-human-confirmed")
        .unwrap()
        .unwrap();
    let evidence = record.learning_evidence.unwrap();
    assert!(
        matches!(&evidence.authority, ProceduralFeedbackAuthorityV2::Producer {
            source_authority: ProceduralProducerSourceAuthorityV1::HumanUser { subject_id }, confirmation: Some(confirmation), ..
        } if subject_id == &human_id && confirmation.actor_subject_id == human_id
            && confirmation.operation_id == "confirm-real-human-evidence"
            && confirmation.evidence_digest == evidence.canonical_confirmation_payload_digest().unwrap())
    );
    let before = image(&platform);
    let mut forged = request(&memory, "forged-human-confirmation");
    forged.learning.human_confirmation_operation_id = Some("forged-confirmation".into());
    assert!(memory
        .finalize_turn_with_procedural_evidence(&capability, forged)
        .is_err());
    assert_eq!(image(&platform), before);
    let declared = request(&human, "human-declared-not-confirmed");
    human
        .finalize_turn_with_procedural_evidence(&capability, declared)
        .unwrap();
    let evidence = platform
        .get_turn(&key, human.subject_id(), "human-declared-not-confirmed")
        .unwrap()
        .unwrap()
        .learning_evidence
        .unwrap();
    assert!(matches!(
        evidence.authority,
        ProceduralFeedbackAuthorityV2::Producer {
            confirmation: None,
            ..
        }
    ));
}

#[test]
fn pfi2_corrupt_file_source_returns_only_safe_typed_error() {
    const PRIVATE_MARKER: &str = "synthetic-private-source-value-31415";
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-source-error-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    let platform = StorePlatform::open(
        crate::StoreBackendConfig::file(&root.0, platform().config().profile).unwrap(),
    )
    .unwrap();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let locator = "synthetic://pfi2/private-source-locator";
    let body = "1. Validate synthetic input.\n2. Check synthetic output.";
    let group = canonical_recall_evidence_group(locator);
    memory
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: Box::new(GovernedEvidenceDocumentDraft {
                    memory_space_id: memory.memory_space_id().into(),
                    mounted_subject_id: memory.subject_id().into(),
                    document_id: "pfi2-safe-error-source".into(),
                    source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
                    source_locator: locator.into(),
                    canonical_evidence_group: group.clone(),
                    evidence_family_group: None,
                    source_revision: 1,
                    content_digest: governed_evidence_document_content_digest(
                        locator,
                        &group,
                        None,
                        body,
                        &[],
                    ),
                    body: body.into(),
                    chunks: vec![],
                    authority: MemoryEvidenceAuthority::WorldObservation,
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    observed_at: 1_799_999_900,
                }),
            }],
        })
        .unwrap();
    let mut spec = grant(&governor).binding.spec;
    spec.binding_id = "safe-error-source".into();
    spec.claims.execution_facts = false;
    spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
        source_revision: GovernedOwnerRevisionRef::try_new(
            GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::EvidenceDocument,
                "pfi2-safe-error-source",
            ),
            1,
        )
        .unwrap(),
    };
    let registered = governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "register-safe-error-source".into(),
            spec,
            state: ProceduralProducerStateV1::Active,
            expected_revision: None,
        })
        .unwrap();
    let capability = governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    let candidate = |id: &str| {
        let mut input = request(&memory, id);
        input.learning.agent_tool_feedback[0]
            .execution_facts
            .clear();
        let method = &mut input.learning.agent_tool_feedback[0].method_evidence[0];
        method.execution_refs.clear();
        method.source_sensitivity = ProceduralSourceSensitivity::NonPrivate;
        method.body = body.into();
        input
    };
    assert!(
        memory
            .finalize_turn_with_procedural_evidence(&capability, candidate("safe-source-positive"))
            .unwrap()
            .transcript_commit
            .unwrap()
            .committed
    );
    let doc = platform
        .read_json_namespace(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
        .unwrap()
        .pop()
        .unwrap();
    let mut corrupted = doc.value.clone();
    corrupted["authority"] = serde_json::Value::String(PRIVATE_MARKER.into());
    let mut physical_path = None;
    for shard in std::fs::read_dir(
        root.0
            .join("kv")
            .join(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
            .join("_v2"),
    )
    .unwrap()
    {
        for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
            let path = entry.unwrap().path();
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if value == doc.value {
                assert!(physical_path.replace(path).is_none());
            }
        }
    }
    // Corrupt only the exact synthetic source, never a user-supplied Store.
    let physical_path = physical_path.unwrap();
    let corrupted_bytes = serde_json::to_vec(&corrupted).unwrap();
    std::fs::write(&physical_path, &corrupted_bytes).unwrap();
    let unaffected_image = || {
        (
            crate::store_internal::schema::store_json_namespaces()
                .filter(|namespace| *namespace != GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
                .flat_map(|namespace| platform.read_json_namespace(namespace).unwrap())
                .collect::<Vec<_>>(),
            platform.read_events().unwrap(),
        )
    };
    let before = unaffected_image();
    let error = memory
        .finalize_turn_with_procedural_evidence(&capability, candidate("corrupt-source-negative"))
        .err()
        .unwrap();
    let rendered = format!("{error}\n{error:?}");
    for denied in [PRIVATE_MARKER, locator, body, doc.key.as_str()] {
        assert!(
            !rendered.contains(denied),
            "source errors cannot disclose protected material or owner locators"
        );
    }
    let Error::Other { source, .. } = error else {
        panic!("the public failure must be typed");
    };
    let typed = source
        .downcast_ref::<crate::ProceduralLearningSdkError>()
        .unwrap();
    assert_eq!(
        typed.disposition,
        crate::ProceduralLearningSdkErrorDisposition::RepairRequired
    );
    assert!(std::error::Error::source(typed).is_none());
    assert_eq!(unaffected_image(), before);
    assert_eq!(std::fs::read(physical_path).unwrap(), corrupted_bytes);
}

#[test]
fn pfi2_producer_errors_identify_the_actual_public_operation() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let registered = grant(&governor);
    let mut spec = registered.binding.spec.clone();
    spec.scope.chat_id = "another-chat".into();
    let before = image(&platform);
    let error = governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "invalid-scope-control".into(),
            spec,
            state: ProceduralProducerStateV1::Active,
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
        })
        .unwrap_err();
    let Error::Other { source, .. } = error else {
        panic!("public control must return its typed error");
    };
    let typed = source
        .downcast_ref::<crate::ProceduralLearningSdkError>()
        .unwrap();
    assert_eq!(
        serde_json::to_value(typed.operation).unwrap(),
        "producer_control"
    );
    let error = memory
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .err()
        .unwrap();
    let Error::Other { source, .. } = error else {
        panic!("capability issue must return its typed error");
    };
    let typed = source
        .downcast_ref::<crate::ProceduralLearningSdkError>()
        .unwrap();
    assert_eq!(
        serde_json::to_value(typed.operation).unwrap(),
        "issue_submission_capability"
    );
    assert_eq!(
        typed.disposition,
        crate::ProceduralLearningSdkErrorDisposition::AuthorityRejected
    );
    assert_eq!(image(&platform), before);
}

#[test]
fn pfi2_public_procedural_file_errors_preserve_classification_without_leaking() {
    use crate::{ProceduralLearningErrorKeyV1 as Key, ProceduralLearningSdkOperation as Operation};
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = SyntheticDirectory(std::env::temp_dir().join(format!(
        "bm-pfi2-public-errors-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    let platform = StorePlatform::open(
        crate::StoreBackendConfig::file(&root.0, platform().config().profile).unwrap(),
    )
    .unwrap();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let reference = grant(&governor).binding.revision_ref().unwrap();
    let capability = governor
        .procedural_submission_capability(&reference)
        .unwrap();
    assert!(
        memory
            .finalize_turn_with_procedural_evidence(&capability, request(&memory, "file-positive"))
            .unwrap()
            .transcript_commit
            .unwrap()
            .committed
    );
    let before = image(&platform);
    let material = before
        .0
        .iter()
        .find(|doc| {
            doc.namespace == crate::store_internal::schema::PROCEDURAL_PRODUCER_BINDING_NAMESPACE
                && doc.key == reference.material_key()
        })
        .unwrap();
    let mut physical_path = None;
    for shard in std::fs::read_dir(root.0.join("kv").join(&material.namespace).join("_v2")).unwrap()
    {
        for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
            let path = entry.unwrap().path();
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if value == material.value {
                assert!(physical_path.replace(path).is_none());
            }
        }
    }
    let path = physical_path.unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let held = root.0.join("synthetic-held-producer");
    let mut observations = Vec::new();
    for (io_failure, expected) in [(true, Key::StoreUnavailable), (false, Key::RepairRequired)] {
        if io_failure {
            std::fs::rename(&path, &held).unwrap();
            std::fs::create_dir(&path).unwrap();
        } else {
            std::fs::write(&path, br#"{"synthetic-private-producer-marker":true}"#).unwrap();
        }
        let issued = governor
            .procedural_submission_capability(&reference)
            .err()
            .unwrap();
        let finalized = memory
            .finalize_turn_with_procedural_evidence(&capability, request(&memory, "file-negative"))
            .err()
            .unwrap();
        if io_failure {
            assert!(path.is_dir());
            std::fs::remove_dir(&path).unwrap();
            std::fs::rename(&held, &path).unwrap();
        } else {
            assert_eq!(
                std::fs::read(&path).unwrap(),
                br#"{"synthetic-private-producer-marker":true}"#
            );
            std::fs::write(&path, &bytes).unwrap();
        }
        assert_eq!(
            image(&platform),
            before,
            "failed public calls must leave the full Store unchanged"
        );
        observations.push((issued, Operation::IssueSubmissionCapability, expected));
        observations.push((finalized, Operation::FinalizeTurn, expected));
    }
    for (error, operation, key) in observations {
        let rendered = format!("{error}\n{error:?}");
        for denied in [
            "synthetic-private-producer-marker",
            path.to_str().unwrap(),
            reference.binding_key.as_str(),
        ] {
            assert!(
                !rendered.contains(denied),
                "public failure leaked Store content or locator"
            );
        }
        let Error::Other { source, .. } = error else {
            panic!("public error must be typed");
        };
        let typed = source
            .downcast_ref::<crate::ProceduralLearningSdkError>()
            .unwrap();
        assert_eq!(typed.operation, operation);
        assert_eq!(typed.key, key);
        assert!(std::error::Error::source(typed).is_none());
    }
    assert!(
        memory
            .finalize_turn_with_procedural_evidence(&capability, request(&memory, "file-negative"))
            .unwrap()
            .transcript_commit
            .unwrap()
            .committed
    );
}

#[test]
fn pfi2_public_finalize_expired_real_lease_is_safe_and_zero_write() {
    let platform = platform();
    let governor = shared_source_runtime(&platform, "pfi2-agent", true);
    let memory = shared_source_runtime(&platform, "pfi2-agent", false);
    memory.write(shared_sources(&memory, 1)).unwrap();
    let mut spec = grant(&governor).binding.spec;
    spec.binding_id = "expiry-source".into();
    spec.claims.execution_facts = false;
    spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
        source_revision: GovernedOwnerRevisionRef::try_new(
            GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::EvidenceDocument,
                "shared-method-a",
            ),
            1,
        )
        .unwrap(),
    };
    let registered = governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "register-expiry-source".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec,
        })
        .unwrap();
    let capability = governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    let mut candidate = request(&memory, "expired-lease-turn");
    candidate.turn.conversation.chat_id = memory.scope().chat_id.clone();
    candidate.turn.conversation.conversation_id = Some(memory.scope().chat_id.clone());
    candidate.learning.agent_tool_feedback[0]
        .execution_facts
        .clear();
    candidate.learning.agent_tool_feedback[0].method_evidence[0]
        .execution_refs
        .clear();
    let before = image(&platform);
    let lease = memory.acquire_runtime_budget_lease().unwrap();
    let deadline = lease.report().resource_snapshot.observed_at_unix_secs
        + lease.report().resource_snapshot.ttl_ms.div_ceil(1000);
    let error = memory
        .execute_with_runtime_budget_lease(&lease, || {
            // Let a genuinely issued lease expire while it is active; no forged report or clock.
            while current_resource_unix_secs() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            memory.finalize_turn_with_procedural_evidence(&capability, candidate.clone())
        })
        .err()
        .unwrap();
    assert_eq!(image(&platform), before);
    let Error::Other { source, .. } = error else {
        panic!("public error must be typed");
    };
    let typed = source
        .downcast_ref::<crate::ProceduralLearningSdkError>()
        .expect("raw admission errors cannot escape the public procedural boundary");
    assert_eq!(
        typed.operation,
        crate::ProceduralLearningSdkOperation::FinalizeTurn
    );
    assert_eq!(
        typed.key,
        crate::ProceduralLearningErrorKeyV1::StoreUnavailable
    );
    assert!(std::error::Error::source(typed).is_none());
    memory.refresh_runtime_resource_snapshot().unwrap();
    assert!(
        memory
            .finalize_turn_with_procedural_evidence(&capability, candidate)
            .unwrap()
            .transcript_commit
            .unwrap()
            .committed
    );
}

#[test]
fn pfi2_public_finalize_redacts_corrupt_transcript_intake_errors() {
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = SyntheticDirectory(std::env::temp_dir().join(format!(
        "bm-pfi2-transcript-error-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    )));
    let platform = StorePlatform::open(
        crate::StoreBackendConfig::file(&root.0, platform().config().profile).unwrap(),
    )
    .unwrap();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let reference = grant(&governor).binding.revision_ref().unwrap();
    let capability = governor
        .procedural_submission_capability(&reference)
        .unwrap();
    let candidate = request(&memory, "transcript-corruption");
    assert!(
        memory
            .finalize_turn_with_procedural_evidence(&capability, candidate.clone())
            .unwrap()
            .transcript_commit
            .unwrap()
            .committed
    );
    let before = image(&platform);
    let doc = before
        .0
        .iter()
        .find(|doc| doc.namespace == "conversation_transcript")
        .unwrap();
    let mut physical_path = None;
    for shard in std::fs::read_dir(root.0.join("kv").join(&doc.namespace).join("_v2")).unwrap() {
        for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
            let path = entry.unwrap().path();
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if value == doc.value {
                assert!(physical_path.replace(path).is_none());
            }
        }
    }
    let path = physical_path.unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut corrupted = doc.value.clone();
    const MARKER: &str = "synthetic-private-transcript-state-92653";
    corrupted["lifecycle_state"] = serde_json::Value::String(MARKER.into());
    let corrupted_bytes = serde_json::to_vec(&corrupted).unwrap();
    std::fs::write(&path, &corrupted_bytes).unwrap();
    let error = memory
        .finalize_turn_with_procedural_evidence(&capability, candidate)
        .err()
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), corrupted_bytes);
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(
        image(&platform),
        before,
        "intake errors cannot partially write"
    );
    assert!(
        !format!("{error}\n{error:?}").contains(MARKER),
        "raw transcript errors must not leave the public boundary"
    );
    let Error::Other { source, .. } = error else {
        panic!("public error must be typed");
    };
    let typed = source
        .downcast_ref::<crate::ProceduralLearningSdkError>()
        .unwrap();
    assert_eq!(
        typed.operation,
        crate::ProceduralLearningSdkOperation::FinalizeTurn
    );
    assert_eq!(
        typed.key,
        crate::ProceduralLearningErrorKeyV1::RepairRequired
    );
    assert!(std::error::Error::source(typed).is_none());
}

#[test]
fn pfi2_public_procedural_invalid_input_is_not_store_damage() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let reference = grant(&governor).binding.revision_ref().unwrap();
    let capability = governor
        .procedural_submission_capability(&reference)
        .unwrap();
    assert!(
        memory
            .finalize_turn_with_procedural_evidence(&capability, request(&memory, "input-positive"))
            .unwrap()
            .transcript_commit
            .unwrap()
            .committed
    );
    let before = image(&platform);
    let mut invalid_reference = reference;
    invalid_reference.binding_key.clear();
    let mut errors = vec![(
        governor
            .procedural_submission_capability(&invalid_reference)
            .err()
            .unwrap(),
        crate::ProceduralLearningSdkOperation::IssueSubmissionCapability,
        crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
        crate::ProceduralLearningSdkErrorDisposition::AuthorityRejected,
    )];
    for mismatch in ["chat", "subject", "actor"] {
        let mut candidate = request(&memory, "invalid-input");
        match mismatch {
            "chat" => candidate.turn.conversation.chat_id = "foreign-chat".into(),
            "subject" => candidate.turn.subject = "foreign-subject".into(),
            "actor" => {
                candidate.turn.actor = Some(bm_core::memory::ActorAttribution {
                    actor_subject_id: Some("foreign-actor".into()),
                    ..bm_core::memory::ActorAttribution::for_subject(memory.subject_id())
                })
            }
            _ => unreachable!(),
        }
        errors.push((
            memory
                .finalize_turn_with_procedural_evidence(&capability, candidate)
                .err()
                .unwrap(),
            crate::ProceduralLearningSdkOperation::FinalizeTurn,
            crate::ProceduralLearningErrorKeyV1::ScopeMismatch,
            crate::ProceduralLearningSdkErrorDisposition::ContractRejected,
        ));
    }
    assert_eq!(image(&platform), before);
    for (error, operation, key, disposition) in errors {
        let Error::Other { source, .. } = error else {
            panic!("public input failure must be typed");
        };
        let typed = source
            .downcast_ref::<crate::ProceduralLearningSdkError>()
            .unwrap();
        assert_eq!(typed.operation, operation);
        assert_eq!(typed.key, key);
        assert_eq!(typed.disposition, disposition);
    }
}

#[test]
fn pfi2_submission_rechecks_the_current_governing_relationship() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let registered = grant(&governor);
    let capability = governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    let positive = memory
        .finalize_turn_with_procedural_evidence(&capability, request(&memory, "governed-positive"))
        .unwrap();
    assert!(positive.transcript_commit.unwrap().committed);
    let positive_job = memory
        .claim_due_procedural_feedback_job(
            &positive.procedural_learning.job_id.unwrap(),
            "governance-worker",
            1_800_000_060,
        )
        .unwrap();
    memory
        .run_claimed_procedural_feedback_job(&positive_job, "governance-worker")
        .unwrap();
    assert!(!platform
        .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
        .unwrap()
        .is_empty());
    let queued = memory
        .finalize_turn_with_procedural_evidence(
            &capability,
            request(&memory, "governance-removed-before-apply"),
        )
        .unwrap();
    let claimed = memory
        .claim_due_procedural_feedback_job(
            &queued.procedural_learning.job_id.unwrap(),
            "governance-worker",
            1_800_000_060,
        )
        .unwrap();
    let mut graph = memory.subject_relationship_graph().clone();
    graph
        .edges
        .retain(|edge| edge.kind != SubjectRelationshipKind::Governs);
    let without_governance = runtime_with_graph(
        &platform,
        &default_agent_subject_id("pfi2-agent"),
        Some(graph),
    );
    assert_eq!(
        memory.governance_attachment_identity().unwrap(),
        without_governance.governance_attachment_identity().unwrap()
    );
    let before = image(&platform);
    assert!(
        without_governance
            .finalize_turn_with_procedural_evidence(
                &capability,
                request(&without_governance, "withdrawn-governance")
            )
            .is_err(),
        "a durable grant must not replace the current governing relationship"
    );
    assert_eq!(image(&platform), before);
    assert!(
        without_governance
            .run_claimed_procedural_feedback_job(&claimed, "governance-worker")
            .is_err(),
        "a previously claimed job must not replace the current governing relationship"
    );
    assert_eq!(image(&platform), before);
}

#[test]
fn pfi2_governor_can_revoke_after_the_tool_registry_is_unmounted() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let registered = grant(&governor);
    governor.delete_agent_tool_registry("pfi2-tools").unwrap();
    let revoked = governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "revoke-unmounted-witness".into(),
            spec: registered.binding.spec.clone(),
            state: ProceduralProducerStateV1::Revoked,
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
        })
        .expect("withdrawal must not require the withdrawn tool to be available");
    assert_eq!(revoked.binding.state, ProceduralProducerStateV1::Revoked);
    assert_eq!(revoked.binding.revision, 2);
}

#[test]
fn pfi2_failed_execution_applies_but_revocation_fences_a_claimed_job() {
    let platform = platform();
    let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
    let memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
    let registered = grant(&governor);
    let capability = governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    let claim = |id: &str| {
        let finalized = memory
            .finalize_turn_with_procedural_evidence(&capability, request(&memory, id))
            .unwrap();
        memory
            .claim_due_procedural_feedback_job(
                &finalized.procedural_learning.job_id.unwrap(),
                "synthetic-worker",
                1_800_000_060,
            )
            .unwrap()
    };
    let first = claim("failed-positive");
    memory
        .run_claimed_procedural_feedback_job(&first, "synthetic-worker")
        .unwrap();
    let materials = platform
        .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
        .unwrap();
    assert!(
        !materials.is_empty(),
        "failed execution is genuine evidence, not an empty success-only aggregate"
    );
    assert!(materials.iter().any(|document| serde_json::from_value::<AgentToolExperienceRevisionMaterialV3>(document.value.clone()).is_ok_and(|material|
            matches!(material.body, bm_core::skills::AgentToolExperienceBodyV1::Execution { counts, .. } if counts.failed == 1 && counts.succeeded == 0))));
    let second = claim("claimed-before-revoke");
    governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: "revoke-claimed-witness".into(),
            spec: registered.binding.spec.clone(),
            state: ProceduralProducerStateV1::Revoked,
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
        })
        .unwrap();
    let before = image(&platform);
    assert!(
        memory
            .run_claimed_procedural_feedback_job(&second, "synthetic-worker")
            .is_err(),
        "a claimed job must not turn revoked declarations into new experience"
    );
    assert_eq!(image(&platform), before);
}

#[test]
fn pfi2_capability_survives_reopen_but_not_store_recreation_at_the_same_path() {
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-incarnation-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    std::fs::create_dir_all(&root.0).unwrap();
    let profile = platform().config().profile;
    for sqlite in [false, true] {
        let directory = root.0.join(if sqlite { "sqlite" } else { "file" });
        std::fs::create_dir_all(&directory).unwrap();
        let config = if sqlite {
            crate::StoreBackendConfig::sqlite(directory.join("synthetic.db"), profile).unwrap()
        } else {
            crate::StoreBackendConfig::file(directory.join("synthetic-store"), profile).unwrap()
        };
        let (capability, original) = {
            let platform = StorePlatform::open(config.clone()).unwrap();
            let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
            let registered = grant(&governor);
            (
                governor
                    .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
                    .unwrap(),
                registered.binding,
            )
        };
        {
            let reopened = StorePlatform::open(config.clone()).unwrap();
            let memory = runtime(&reopened, &default_agent_subject_id("pfi2-agent"));
            assert!(
                memory
                    .finalize_turn_with_procedural_evidence(
                        &capability,
                        request(&memory, "reopen-positive")
                    )
                    .unwrap()
                    .transcript_commit
                    .unwrap()
                    .committed
            );
        }
        // Deliberately replace only this test-owned synthetic Store after all
        // handles close. No real or user-supplied path is accepted here.
        std::fs::remove_dir_all(&directory).unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        let recreated = StorePlatform::open(config).unwrap();
        let governor = runtime(&recreated, &system_governor_subject_id("pfi2-owner"));
        let memory = runtime(&recreated, &default_agent_subject_id("pfi2-agent"));
        let registered = grant(&governor);
        assert_eq!(
            registered.binding, original,
            "same-time reprovision must not accidentally distinguish the test by grant payload"
        );
        let before = image(&recreated);
        assert!(memory.finalize_turn_with_procedural_evidence(&capability, request(&memory, "stale-incarnation")).is_err(),
                "the old reporting capability must not authorize a new Store at the same filesystem path");
        assert_eq!(image(&recreated), before);
        let fresh = governor
            .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
            .unwrap();
        assert!(
            memory
                .finalize_turn_with_procedural_evidence(
                    &fresh,
                    request(&memory, "new-incarnation-positive")
                )
                .unwrap()
                .transcript_commit
                .unwrap()
                .committed
        );
    }
}

#[test]
fn pfi2_reconciliation_first_page_closes_real_sources() {
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-pages-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    let profile = platform().config().profile;
    for backend in ["memory", "file", "sqlite"] {
        let config = match backend {
            "memory" => crate::StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => crate::StoreBackendConfig::file(directory.0.join("file"), profile).unwrap(),
            "sqlite" => {
                crate::StoreBackendConfig::sqlite(directory.0.join("sqlite.db"), profile).unwrap()
            }
            _ => unreachable!(),
        };
        let mut platform = StorePlatform::open(config.clone()).unwrap();
        let mut governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
        let mut memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
        let registered = grant(&governor);
        let capability = governor
            .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
            .unwrap();
        let submitted = memory
            .finalize_turn_with_procedural_evidence(&capability, request(&memory, "page-source"))
            .unwrap();
        let source = memory
            .claim_due_procedural_feedback_job(
                &submitted.procedural_learning.job_id.unwrap(),
                "page-worker",
                1_800_000_060,
            )
            .unwrap();
        memory
            .run_claimed_procedural_feedback_job(&source, "page-worker")
            .unwrap();
        let revoked = governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "page-revoke".into(),
                expected_revision: Some(registered.binding.revision_ref().unwrap()),
                state: ProceduralProducerStateV1::Revoked,
                spec: registered.binding.spec.clone(),
            })
            .unwrap();
        let jobs = memory.due_procedural_feedback_jobs(8).unwrap();
        let job = jobs
            .iter()
            .find(|job| {
                matches!(
                    job.work,
                    bm_core::memory::ProceduralLearningWorkV1::Reconcile { .. }
                )
            })
            .unwrap();
        let claimed = memory
            .claim_due_procedural_feedback_job(&job.job_id, "page-worker", 1_800_000_060)
            .unwrap();
        let page = memory
            .run_claimed_procedural_reconciliation_job(&claimed, "page-worker")
            .unwrap();
        assert!(!page.completed);
        assert_eq!(
            page.job.status,
            ProceduralFeedbackJobStatusV1::ReadyToContinue
        );
        assert!(!page
            .job
            .checkpoint
            .as_ref()
            .unwrap()
            .verified_owners
            .is_empty());
        let producer_pin = page
            .job
            .checkpoint
            .as_ref()
            .unwrap()
            .dependencies
            .iter()
            .map(|pin| serde_json::to_value(pin).unwrap())
            .find(|pin| pin["kind"] == "producer")
            .unwrap();
        assert_eq!(producer_pin.get("original_revision"),
            Some(&serde_json::to_value(registered.binding.revision_ref().unwrap()).unwrap()),
            "a current producer revision must not replace the contribution's original source authority");
        assert_eq!(
            producer_pin["current_revision"],
            serde_json::to_value(revoked.binding.revision_ref().unwrap()).unwrap()
        );
        assert_eq!(
            producer_pin["permitted"], false,
            "the current revocation must remain explicit in the dependency proof"
        );
        let committed_image = image(&platform);
        let replay = memory
            .run_claimed_procedural_reconciliation_job(&claimed, "page-worker")
            .expect("a lost page response must replay before inspecting the new manifest");
        assert!(replay.replayed);
        assert_eq!(replay.receipt, page.receipt);
        assert_eq!(replay.job, page.job);
        assert_eq!(image(&platform), committed_image);
        let next_claim = memory
            .claim_due_procedural_feedback_job(&job.job_id, "next-page-worker", 1_800_000_060)
            .unwrap();
        let next = memory
            .run_claimed_procedural_reconciliation_job(&next_claim, "next-page-worker")
            .unwrap();
        let advanced_image = image(&platform);
        let old_replay = memory
            .run_claimed_procedural_reconciliation_job(&claimed, "page-worker")
            .unwrap();
        assert!(old_replay.replayed);
        assert_eq!(
            old_replay.receipt, page.receipt,
            "receipt identifies the original committed page"
        );
        assert_eq!(
            old_replay.job, next.job,
            "status is the actual durable progress, never a reconstructed old after-image"
        );
        assert_eq!(image(&platform), advanced_image);
        assert!(memory
            .run_claimed_procedural_reconciliation_job(&claimed, "other-worker")
            .is_err());
        assert_eq!(image(&platform), advanced_image);
        let other = runtime_in_chat(
            &platform,
            &system_governor_subject_id("pfi2-owner"),
            None,
            "new-chat-during-scan",
        );
        let mut spec = registered.binding.spec.clone();
        spec.scope = other.procedural_producer_scope();
        spec.binding_id = "producer-during-scan".into();
        other
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "add-scope-during-scan".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec,
            })
            .unwrap();
        let restart_claim = memory
            .claim_due_procedural_feedback_job(&job.job_id, "restart-worker", 1_800_000_060)
            .unwrap();
        let restarted = memory
            .run_claimed_procedural_reconciliation_job(&restart_claim, "restart-worker")
            .expect("a legitimate same-epoch root change must restart the exact durable scan");
        let checkpoint = restarted.job.checkpoint.as_ref().unwrap();
        assert_eq!(checkpoint.page_number, 0);
        assert!(checkpoint.verified_owners.is_empty() && checkpoint.dependencies.is_empty());
        assert_eq!(restarted.job.work, next.job.work);
        assert_eq!(restarted.job.attempt_count, next.job.attempt_count);
        assert!(restarted.job.state_revision > next.job.state_revision);
        assert!(!restarted.completed);
        let restart_image = image(&platform);
        let restart_replay = memory
            .run_claimed_procedural_reconciliation_job(&restart_claim, "restart-worker")
            .unwrap();
        assert!(restart_replay.replayed);
        assert_eq!(restart_replay.job, restarted.job);
        assert_eq!(restart_replay.receipt, restarted.receipt);
        assert_eq!(image(&platform), restart_image);
        let capacity_claim = memory
            .claim_due_procedural_feedback_job(&job.job_id, "capacity-worker", 1_800_000_060)
            .unwrap();
        let blocked = memory
            .retry_claimed_procedural_feedback_job(
                &capacity_claim,
                "capacity-worker",
                ProceduralFeedbackErrorClassV1::BudgetExceeded,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(blocked.status).unwrap(),
            "blocked_capacity",
            "deterministic reconciliation capacity failure must not consume transient retries"
        );
        assert_eq!(blocked.attempt_count, capacity_claim.attempt_count);
        assert!(
            blocked.next_attempt_at.is_none()
                && blocked.lease_owner.is_none()
                && blocked.terminal_at.is_none()
        );
        assert!(!memory
            .due_procedural_feedback_jobs(8)
            .unwrap()
            .iter()
            .any(|candidate| candidate.job_id == job.job_id));
        if backend != "memory" {
            drop(memory);
            drop(governor);
            drop(other);
            drop(platform);
            platform = StorePlatform::open(config.clone()).unwrap();
            governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
            memory = runtime(&platform, &default_agent_subject_id("pfi2-agent"));
            assert_eq!(
                crate::store_internal::procedural_feedback::read_job(&platform, &job.job_id)
                    .unwrap()
                    .unwrap(),
                blocked
            );
            assert!(
                !memory
                    .due_procedural_feedback_jobs(8)
                    .unwrap()
                    .iter()
                    .any(|candidate| candidate.job_id == job.job_id),
                "{backend}: runtime reopen must not implicitly resume capacity work"
            );
        }
        let resume_request = crate::MemoryProceduralReconciliationResumeRequest {
            operation_id: "explicit-capacity-resume".into(),
            job_id: job.job_id.clone(),
            expected_state_revision: blocked.state_revision,
        };
        let blocked_image = image(&platform);
        assert!(
            memory
                .resume_procedural_reconciliation(resume_request.clone())
                .is_err(),
            "a worker cannot upgrade itself to the SystemGovernor"
        );
        assert!(memory
            .claim_due_procedural_feedback_job(&job.job_id, "bypass-block", 1_800_000_060)
            .is_err());
        assert_eq!(image(&platform), blocked_image);
        let resumed = governor
            .resume_procedural_reconciliation(resume_request.clone())
            .unwrap();
        assert!(!resumed.replayed);
        assert_eq!(
            resumed.job.status,
            ProceduralFeedbackJobStatusV1::ReadyToContinue
        );
        assert_eq!(resumed.job.attempt_count, blocked.attempt_count);
        assert_eq!(resumed.job.work, blocked.work);
        assert_eq!(resumed.job.checkpoint.as_ref().unwrap().page_number, 0);
        let resumed_image = image(&platform);
        let resume_replay = governor
            .resume_procedural_reconciliation(resume_request)
            .unwrap();
        assert!(resume_replay.replayed);
        assert_eq!(resume_replay.job, resumed.job);
        assert_eq!(resume_replay.receipt, resumed.receipt);
        assert_eq!(image(&platform), resumed_image);
        let mut final_page = None;
        for _ in 0..16 {
            let claim = memory
                .claim_due_procedural_feedback_job(
                    &job.job_id,
                    "resumed-page-worker",
                    1_800_000_060,
                )
                .unwrap();
            let page = memory
                .run_claimed_procedural_reconciliation_job(&claim, "resumed-page-worker")
                .unwrap();
            assert_eq!(page.job.attempt_count, blocked.attempt_count);
            if page.completed {
                final_page = Some((claim, page));
                break;
            }
        }
        let (final_claim, final_page) =
            final_page.expect("resumed work must actually finish its complete inventory");
        assert!(!final_page
            .job
            .checkpoint
            .as_ref()
            .unwrap()
            .verified_owners
            .is_empty());
        let final_image = image(&platform);
        let final_replay = memory
            .run_claimed_procedural_reconciliation_job(&final_claim, "resumed-page-worker")
            .unwrap();
        assert!(final_replay.completed && final_replay.replayed);
        assert_eq!(final_replay.receipt, final_page.receipt);
        let old_replay = memory
            .run_claimed_procedural_reconciliation_job(&claimed, "page-worker")
            .unwrap();
        assert!(
            !old_replay.completed && old_replay.replayed,
            "later job completion is not completion by the old page operation"
        );
        assert_eq!(old_replay.job, final_page.job);
        assert_eq!(image(&platform), final_image);
        let mut fresh_spec = registered.binding.spec.clone();
        fresh_spec.binding_id = "capacity-supersession-producer".into();
        let fresh = governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "fresh-producer-for-capacity".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec: fresh_spec.clone(),
            })
            .unwrap();
        fresh_spec.claims.method_declarations = false;
        let narrowed = governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "new-epoch-for-capacity".into(),
                expected_revision: Some(fresh.binding.revision_ref().unwrap()),
                state: ProceduralProducerStateV1::Active,
                spec: fresh_spec.clone(),
            })
            .unwrap();
        let newer = memory
            .due_procedural_feedback_jobs(8)
            .unwrap()
            .into_iter()
            .find(|candidate| matches!(candidate.work, ProceduralLearningWorkV1::Reconcile { .. }))
            .unwrap();
        let blocked_claim = memory
            .claim_due_procedural_feedback_job(&newer.job_id, "blocked-epoch-worker", 1_800_000_060)
            .unwrap();
        let blocked_epoch = memory
            .retry_claimed_procedural_feedback_job(
                &blocked_claim,
                "blocked-epoch-worker",
                ProceduralFeedbackErrorClassV1::BudgetExceeded,
            )
            .unwrap();
        governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "revoke-while-capacity-blocked".into(),
                expected_revision: Some(narrowed.binding.revision_ref().unwrap()),
                state: ProceduralProducerStateV1::Revoked,
                spec: fresh_spec,
            })
            .expect("capacity exhaustion must not prevent a later authority revocation");
        let cancelled =
            crate::store_internal::procedural_feedback::read_job(&platform, &newer.job_id)
                .unwrap()
                .unwrap();
        assert_eq!(cancelled.status, ProceduralFeedbackJobStatusV1::Cancelled);
        assert_eq!(cancelled.work, blocked_epoch.work);
        let superseded_image = image(&platform);
        assert!(governor
            .resume_procedural_reconciliation(crate::MemoryProceduralReconciliationResumeRequest {
                operation_id: "must-not-resume-old-epoch".into(),
                job_id: newer.job_id,
                expected_state_revision: blocked_epoch.state_revision,
            })
            .is_err());
        assert_eq!(image(&platform), superseded_image);
        drop(memory);
        drop(governor);
        drop(platform);
        if backend != "memory" {
            let reopened = StorePlatform::open(config).unwrap();
            assert_eq!(
                crate::store_internal::procedural_feedback::read_job(&reopened, &job.job_id)
                    .unwrap()
                    .unwrap(),
                final_page.job
            );
            if backend == "sqlite" {
                let consumer = runtime(&reopened, &default_agent_subject_id("pfi2-agent"));
                let connection = rusqlite::Connection::open(directory.0.join("sqlite.db")).unwrap();
                assert_eq!(connection.execute("DELETE FROM bm_kv WHERE namespace IN ('memory_mutation_receipts','memory_mutation_audits') AND key = ?1",
                        [page.receipt.identity.storage_key()]).unwrap(), 2);
                let corrupt = image(&reopened);
                assert!(
                    consumer
                        .run_claimed_procedural_reconciliation_job(&claimed, "page-worker")
                        .is_err(),
                    "missing immutable old-page receipt must not allow re-planning an old claim"
                );
                assert!(
                    image(&reopened) == corrupt,
                    "old claim minted replacement proof"
                );
            }
        }
    }
}

#[test]
fn pfi2_authority_cancellation_is_atomic_and_survives_persistent_reopen() {
    struct NoProvider;
    impl crate::GovernanceExecutionPort for NoProvider {
        fn execute(
            &mut self,
            _: &crate::AuthorizedGovernanceEnvelope,
            _: &crate::ImmutableGovernanceExecutionBinding,
            _: &crate::GovernanceEgressAuthority,
            _: &mut dyn crate::GovernanceExecutionOperation,
        ) -> std::result::Result<(), crate::GovernanceExecutionPortFailure> {
            panic!("procedural cancellation must never invoke a Provider")
        }
    }
    struct SyntheticDirectory(std::path::PathBuf);
    impl Drop for SyntheticDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = SyntheticDirectory(std::env::temp_dir().join(
        format!("bm-pfi2-cancel-reopen-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
    ));
    let profile = platform().config().profile;
    for backend in ["memory", "file", "sqlite"] {
        let config = match backend {
            "memory" => crate::StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => crate::StoreBackendConfig::file(root.0.join("file"), profile).unwrap(),
            "sqlite" => {
                crate::StoreBackendConfig::sqlite(root.0.join("sqlite.db"), profile).unwrap()
            }
            _ => unreachable!(),
        };
        let platform = StorePlatform::open(config.clone()).unwrap();
        let governor = runtime(&platform, &system_governor_subject_id("pfi2-owner"));
        let memory = Arc::new(runtime(&platform, &default_agent_subject_id("pfi2-agent")));
        let registered = grant(&governor);
        let capability = governor
            .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
            .unwrap();
        let positive = memory
            .finalize_turn_with_procedural_evidence(
                &capability,
                request(&memory, "cancel-positive"),
            )
            .unwrap();
        let positive_job = memory
            .claim_due_procedural_feedback_job(
                &positive.procedural_learning.job_id.unwrap(),
                "positive-worker",
                1_800_000_060,
            )
            .unwrap();
        memory
            .run_claimed_procedural_feedback_job(&positive_job, "positive-worker")
            .unwrap();
        assert!(!platform
            .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
            .unwrap()
            .is_empty());
        let queued = memory
            .finalize_turn_with_procedural_evidence(
                &capability,
                request(&memory, "cancel-after-grant"),
            )
            .unwrap();
        let job_id = queued.procedural_learning.job_id.unwrap();
        governor
            .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
                operation_id: "cancel-producer-revoke".into(),
                expected_revision: Some(registered.binding.revision_ref().unwrap()),
                state: ProceduralProducerStateV1::Revoked,
                spec: registered.binding.spec,
            })
            .unwrap();
        let engine = crate::MemoryLearningEngine::attach(memory.clone()).unwrap();
        let mut cancellation = None;
        for _ in 0..16 {
            let before_materials = platform
                .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
                .unwrap();
            match engine
                .run_due_cycle(
                    crate::MemoryLearningCycleRequest {
                        lease_owner: "cancel-worker".into(),
                        lease_duration_secs: 60,
                    },
                    &mut NoProvider,
                )
                .unwrap()
            {
                crate::MemoryLearningCycleOutcome::ProceduralFailed(report) => {
                    cancellation = Some((report, before_materials));
                    break;
                }
                crate::MemoryLearningCycleOutcome::ProceduralReconciliation(page) => {
                    assert_eq!(page.job.attempt_count, 1);
                    assert!(!page.replayed);
                    assert!(page.job.checkpoint.is_some());
                }
                crate::MemoryLearningCycleOutcome::Blocked(report) => {
                    assert_eq!(
                        report.job.status,
                        crate::PostTurnGovernanceJobStatusV2::BlockedConfiguration
                    );
                    assert_eq!(report.reason, "governance_execution_binding_unavailable");
                    assert_eq!(
                        report
                            .job
                            .execution_block_authority
                            .as_ref()
                            .unwrap()
                            .typed_block_reason,
                        crate::PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable
                    );
                    assert_eq!(report.job.attempt_count, 0);
                }
                outcome => panic!(
                    "must commit exact reconciliation or cancel revoked feedback: {outcome:?}"
                ),
            }
        }
        let (report, before_materials) =
            cancellation.expect("bounded work must not starve cancelled feedback");
        drop(engine);
        assert_eq!(report.job.job_id, job_id);
        assert_eq!(report.job.status, ProceduralFeedbackJobStatusV1::Cancelled);
        assert_eq!(
            report.job.last_error_class,
            Some(ProceduralFeedbackErrorClassV1::AuthorityDenied)
        );
        assert!(
            report.job.next_attempt_at.is_none()
                && report.job.lease_owner.is_none()
                && report.job.receipt.is_none()
        );
        let verify = |platform: &StorePlatform| {
            let stored = crate::store_internal::procedural_feedback::read_job(platform, &job_id)
                .unwrap()
                .unwrap();
            assert_eq!(stored, report.job);
            let index = crate::store_internal::procedural_feedback::read_scope_index(
                platform,
                &stored.discovery_root_key,
            )
            .unwrap()
            .unwrap();
            assert!(!index.active_jobs.iter().any(|entry| entry.job_id == job_id));
            assert!(index
                .recent_terminal_jobs
                .iter()
                .any(|entry| *entry == ProceduralFeedbackJobRefV2::from_job(&stored)));
            assert_eq!(
                platform
                    .read_json_namespace(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
                    .unwrap(),
                before_materials
            );
        };
        verify(&platform);
        let before_retry = image(&platform);
        assert!(memory
            .terminate_claimed_procedural_feedback_job(
                &report.job,
                "cancel-worker",
                ProceduralFeedbackErrorClassV1::AuthorityDenied
            )
            .is_err());
        assert_eq!(
            image(&platform),
            before_retry,
            "expired terminal lease cannot emit another state/event"
        );
        drop(memory);
        drop(governor);
        drop(platform);
        if backend != "memory" {
            let reopened = StorePlatform::open(config).unwrap();
            verify(&reopened);
        }
    }
}
