use super::*;
use crate::{
    CanonicalTurnDelta, ConversationScope, MemoryTurnDeliveryStatus, MemoryTurnProtocol,
    MemoryTurnSource, PostTurnLearningInputV2, StoreBackendConfig, ToolObservationDigest,
    TranscriptInputMessage,
};

struct Clock;
impl crate::MemoryClock for Clock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

fn registry() -> AgentToolRegistrySnapshot {
    AgentToolRegistrySnapshot::compact(
        "promotion-test-tools",
        "host",
        vec![bm_core::skills::AgentToolDescriptor::compact(
            "archive.unpack",
            "Unpack archive",
            "schema-v1",
        )],
        1_800_000_000,
    )
}

fn request(memory: &MemoryRuntime, turn: &str) -> MemoryTurnFinalizeRequest {
    let observations = ["first", "second"]
        .into_iter()
        .map(|call| bm_core::memory::ToolExecutionFactV1 {
            observation_id: format!("{turn}-{call}"),
            call_id: format!("call-{turn}-{call}"),
            outcome: bm_core::memory::ToolExecutionOutcome::Succeeded,
            source_sensitivity: bm_core::memory::ProceduralSourceSensitivity::NonPrivate,
            started_at: Some(1_800_000_000),
            completed_at: Some(1_800_000_000),
        })
        .collect::<Vec<_>>();
    MemoryTurnFinalizeRequest {
            turn: CanonicalTurnDelta {
                turn_id: turn.into(),
                conversation: ConversationScope {
                    channel: "sdk.direct".into(),
                    chat_id: "atomic-promotion-chat".into(),
                    conversation_id: Some("atomic-promotion-chat".into()),
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
                input_messages: vec![TranscriptInputMessage::user("unpack archive")],
                assistant_message: Some(TranscriptInputMessage::assistant("Archive verified")),
                tool_observations: observations
                    .iter()
                    .map(|value| ToolObservationDigest {
                        observation_id: value.observation_id.clone(),
                        call_id: value.call_id.clone(),
                        tool_name: "archive.unpack".into(),
                        summary: "synthetic successful archive operation".into(),
                        external_content: false,
                    })
                    .collect(),
                external_content_used: false,
                candidate_ids: vec![],
            },
            learning: PostTurnLearningInputV2 {
                tool_call_count: 2,
                agent_tool_feedback: vec![bm_core::memory::AgentToolUsageFeedbackV3 {
                    registry_ref: registry().registry_ref(),
                    tool_id: "archive.unpack".into(),
                    schema_fingerprint: "schema-v1".into(),
                    method_evidence: vec![bm_core::memory::ToolMethodEvidenceV1 {
                        method_id: format!("method-{turn}"), task_signature: "unpack archive".into(),
                        body: "1. Inspect the archive manifest\n2. Extract entries into the workspace\n3. Verify output checksums".into(),
                        execution_refs: observations.iter().map(|fact| fact.observation_id.clone()).collect(),
                        source_sensitivity: bm_core::memory::ProceduralSourceSensitivity::NonPrivate, external_content: false,
                    }],
                    execution_facts: observations,
                }],
                ..PostTurnLearningInputV2::empty()
            },
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        }
}

fn fixture(
    config: StoreBackendConfig,
) -> (
    StorePlatform,
    MemoryRuntime,
    ProceduralFeedbackCompletionInput,
) {
    fixture_at_times(config, 1_800_000_000, 1_800_000_000)
}

fn fixture_at_times(
    config: StoreBackendConfig,
    first_time: u64,
    second_time: u64,
) -> (
    StorePlatform,
    MemoryRuntime,
    ProceduralFeedbackCompletionInput,
) {
    struct AdjustableClock(std::sync::atomic::AtomicU64);
    impl crate::MemoryClock for AdjustableClock {
        fn now_secs(&self) -> u64 {
            self.0.load(std::sync::atomic::Ordering::SeqCst)
        }
    }
    let clock = Arc::new(AdjustableClock(std::sync::atomic::AtomicU64::new(
        first_time,
    )));
    let platform = StorePlatform::open(config).unwrap();
    let memory = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("atomic-agent", "atomic-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "atomic-promotion-chat").unwrap())
        .store(crate::MemoryStoreHandle::from_platform(platform.clone()))
        .clock(clock.clone())
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .agent_tool_registry(registry())
        .build()
        .unwrap();
    let capability = submission_capability(&platform, &memory, false);
    let first = memory
        .finalize_turn_with_procedural_evidence(&capability, request(&memory, "first-source"))
        .unwrap();
    let first = memory
        .claim_due_procedural_feedback_job(
            &first.procedural_learning.job_id.unwrap(),
            "atomic-worker",
            1_800_000_060,
        )
        .unwrap();
    memory
        .run_claimed_procedural_feedback_job(&first, "atomic-worker")
        .unwrap();
    clock
        .0
        .store(second_time, std::sync::atomic::Ordering::SeqCst);
    let second = memory
        .finalize_turn_with_procedural_evidence(&capability, request(&memory, "second-source"))
        .unwrap();
    let job = memory
        .claim_due_procedural_feedback_job(
            &second.procedural_learning.job_id.unwrap(),
            "atomic-worker",
            1_800_000_060,
        )
        .unwrap();
    let input = completion_input(&platform, &memory, &job);
    (platform, memory, input)
}

fn completion_input(
    platform: &StorePlatform,
    memory: &MemoryRuntime,
    job: &ProceduralFeedbackJobV2,
) -> ProceduralFeedbackCompletionInput {
    let key = ConversationKey::new(
        memory.memory_space_id(),
        "sdk.direct",
        "atomic-promotion-chat",
    )
    .unwrap();
    let record = platform
        .get_turn(&key, memory.subject_id(), "second-source")
        .unwrap()
        .unwrap();
    let mut planned = memory
        .plan_agent_tool_feedback_application(
            job,
            &record,
            record.learning_evidence.as_ref().unwrap(),
            memory.config.clock.now_secs(),
        )
        .unwrap();
    assert!(planned.owner_bindings.iter().any(|binding| matches!(binding, bm_core::memory::ProceduralAppliedOwnerBindingV1::RuntimeSkill { .. })),
            "real two-source production plan must include the Runtime promotion before failure injection");
    planned.preconditions.push(StoreJsonPrecondition::Exact {
        namespace: "conversation_transcript".into(),
        key: crate::store_internal::transcript_turn_storage_key(
            &key,
            memory.subject_id(),
            "second-source",
        ),
        value: serde_json::to_value(record).unwrap(),
    });
    ProceduralFeedbackCompletionInput {
        job_id: job.job_id.clone(),
        lease_owner: "atomic-worker".into(),
        lease_epoch: job.lease_epoch,
        operation_id: format!("procedural-feedback-{}", job.job_id),
        actor_subject_id: memory.config.scoped_runtime.actor_subject_id.clone(),
        experience_mutations: planned.mutations,
        experience_preconditions: planned.preconditions,
        applied_owner_bindings: planned.owner_bindings,
        accepted_count: planned.accepted_count,
        partially_accepted_count: planned.partially_accepted_count,
        method_dispositions: planned.method_dispositions,
        deferred_count: planned.deferred_count,
        rejected_count: planned.rejected_count,
        changed_count: planned.changed_count,
        reason_digest: sha256_field_digest(&[b"synthetic-promotion-completion"]),
        completed_at: memory.config.clock.now_secs(),
    }
}

fn complete(
    platform: &StorePlatform,
    memory: &MemoryRuntime,
    input: ProceduralFeedbackCompletionInput,
) -> Result<ProceduralFeedbackCompletionOutcome> {
    complete_procedural_feedback_job(
        platform,
        memory.memory_write_transaction_scope(),
        &memory.runtime_budget(),
        input,
    )
}

fn submission_capability(
    platform: &StorePlatform,
    memory: &MemoryRuntime,
    model: bool,
) -> crate::MemoryProceduralSubmissionCapability {
    use bm_core::memory::*;
    let mut scoped = memory.scoped_runtime().clone();
    scoped.actor_subject_id = memory
        .subject_registry()
        .system_governor()
        .unwrap()
        .subject_id
        .clone();
    let governor = MemoryRuntime::builder()
        .identity(memory.identity().clone())
        .scope(memory.scope().clone())
        .store(crate::MemoryStoreHandle::from_platform(platform.clone()))
        .clock(Arc::new(Clock))
        .subject_registry(memory.subject_registry().clone())
        .scoped_runtime(scoped)
        .agent_tool_registry(registry())
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .build()
        .unwrap();
    let id = if model {
        "synthetic-model"
    } else {
        "synthetic-executor"
    };
    let spec = ProceduralProducerSpecV1 {
        binding_id: id.into(),
        scope: governor.procedural_producer_scope(),
        principal: ProceduralProducerPrincipalV1::LocalCapability {
            capability_id: id.into(),
        },
        source_authority: if model {
            ProceduralProducerSourceAuthorityV1::ModelInferred {
                subject_id: memory.subject_id().into(),
            }
        } else {
            ProceduralProducerSourceAuthorityV1::RuntimeObservation
        },
        claims: ProceduralProducerClaimsV1 {
            execution_facts: !model,
            method_declarations: true,
            usage_feedback: false,
            source_classifications: vec![ProceduralSourceSensitivity::NonPrivate],
        },
        tools: vec![ProceduralProducerToolV1 {
            registry_ref: registry().registry_ref(),
            tool_id: "archive.unpack".into(),
            schema_fingerprint: "schema-v1".into(),
        }],
        source_config_ref: id.into(),
    };
    let binding = governor
        .control_procedural_producer(crate::MemoryProceduralProducerControlRequest {
            operation_id: format!("register-{id}"),
            spec,
            state: ProceduralProducerStateV1::Active,
            expected_revision: None,
        })
        .unwrap()
        .binding;
    governor
        .procedural_submission_capability(&binding.revision_ref().unwrap())
        .unwrap()
}

fn fill_with_durable_observation_jobs(
    platform: &StorePlatform,
    memory: &MemoryRuntime,
    minimum_ledgers: usize,
    minimum_entries: usize,
) {
    let capability = submission_capability(platform, memory, true);
    let mut index = 0;
    loop {
        // This long-running synthetic host owns resource refresh, just as
        // Entry does in production. Request paths must not probe live state.
        memory.refresh_runtime_resource_snapshot().unwrap();
        let snapshot = platform.export_store_snapshot().unwrap();
        let ledgers = snapshot.json_docs.iter().filter(|doc| doc.namespace == crate::store_internal::schema::PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE).count();
        if ledgers >= minimum_ledgers
            && snapshot.json_docs.len() + snapshot.blobs.len() >= minimum_entries
        {
            break;
        }
        let mut input = request(memory, &format!("capacity-evidence-{index}"));
        input.learning.agent_tool_feedback[0]
            .execution_facts
            .clear();
        input.learning.agent_tool_feedback[0].method_evidence[0]
            .execution_refs
            .clear();
        input.learning.agent_tool_feedback[0].method_evidence[0].task_signature =
            format!("synthetic capacity observation {index}");
        let job_id = memory
            .finalize_turn_with_procedural_evidence(&capability, input)
            .unwrap()
            .procedural_learning
            .job_id
            .unwrap();
        let claimed = memory
            .claim_due_procedural_feedback_job(&job_id, "capacity-worker", 1_800_000_060)
            .unwrap();
        memory
            .run_claimed_procedural_feedback_job(&claimed, "capacity-worker")
            .unwrap();
        index += 1;
    }
}

fn minimum_valid_store_entries() -> usize {
    bm_core::memory::MAX_EVIDENCE_DOCUMENT_FACET_LEXICAL_TERMS
        + bm_core::memory::MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS
        + 1
}

#[test]
fn production_promotion_source_cas_rejects_withdrawal_without_any_partial_commit() {
    let config = StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap();
    let (platform, memory, input) = fixture(config);
    memory
        .request_transcript_lifecycle(crate::MemoryTranscriptLifecycleRequest {
            memory_space_id: memory.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: "atomic-promotion-chat".into(),
            turn_id: Some("first-source".into()),
            transition: bm_core::memory::TranscriptLifecycleTransition::DeleteRaw,
            reason: "synthetic source CAS race".into(),
        })
        .unwrap();
    let before = platform.export_store_snapshot().unwrap();
    assert!(complete(&platform, &memory, input).is_err());
    assert_eq!(
        platform.export_store_snapshot().unwrap(),
        before,
        "source drift rejects Runtime/material/head/manifest/ledger/receipt/audit/event together"
    );
}

#[test]
#[cfg(feature = "sqlite-store")]
fn production_promotion_completion_preserves_sqlite_closure_across_turn_times() {
    let root = std::env::temp_dir().join(format!(
        "bm-embedded-promotion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (platform, memory, input) = fixture_at_times(
        StoreBackendConfig::sqlite(
            root.join("synthetic.db"),
            ProfileId::native_dev_full().unwrap(),
        )
        .unwrap(),
        1_800_000_000,
        1_800_000_002,
    );
    complete(&platform, &memory, input)
        .expect("real SQLite promotion post-image across turn times");
    drop(memory);
    drop(platform);
    StorePlatform::open(
        StoreBackendConfig::sqlite(
            root.join("synthetic.db"),
            ProfileId::native_dev_full().unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pfi2_store_rejects_resealed_terminal_skill_resurrection() {
    use crate::store_internal::schema::{
        RUNTIME_SKILL_RECORD_NAMESPACE as OWNER, RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE as MANIFEST,
    };
    use bm_core::skills::{
        RuntimeSkillAvailability, RuntimeSkillLifecycleState, RuntimeSkillOwnerBinding,
        RuntimeSkillOwnerRecord, RuntimeSkillScopeManifest,
    };
    let root = std::env::temp_dir().join(format!(
        "bm-pfi2-terminal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (backend, primitive) in ["memory", "file"]
        .into_iter()
        .chain(cfg!(feature = "sqlite-store").then_some("sqlite"))
        .flat_map(|backend| [false, true].map(|primitive| (backend, primitive)))
    {
        let profile = ProfileId::native_dev_full().unwrap();
        let config = match backend {
            "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => {
                StoreBackendConfig::file(root.join(format!("file-{primitive}")), profile).unwrap()
            }
            "sqlite" => {
                StoreBackendConfig::sqlite(root.join(format!("sqlite-{primitive}.db")), profile)
                    .unwrap()
            }
            _ => unreachable!(),
        };
        let (platform, memory, input) = fixture(config.clone());
        let scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: memory.subject_id().into(),
        };
        if primitive {
            memory.seed_runtime_skills_for_replay(vec![crate::GovernedRuntimeSkillWriteInput {
                    write: bm_core::skills::RuntimeSkillWrite { name: "runtime_skill__terminal_primitive".into(), title: "Terminal primitive fixture".into(),
                        topic: "archive workflow".into(), summary: "Synthetic standalone skill".into(),
                        content: "1. Inspect archive entries\n2. Extract the selected entries\n3. Verify extracted output".into(),
                        citations: vec!["synthetic-only".into()], source_chat_id: Some(memory.scope().chat_id.clone()), observed_at: 1_800_000_000 },
                    creation_ref: bm_core::skills::RuntimeSkillCreationRef::ReplayPromotion { candidate_ref: "synthetic-static".into(),
                        verification_receipt_digest: format!("sha256:{}", "0".repeat(64)) }, privacy_class: MemoryPrivacyClass::SharedWithSubject,
                }], scope.clone()).unwrap();
        } else {
            complete(&platform, &memory, input).unwrap();
        }
        let positive = memory
            .list_runtime_skills(crate::RuntimeSkillListRequest {
                owning_scope: scope.clone(),
                query: None,
                include_disabled: true,
                include_retired: true,
                limit: 10,
            })
            .unwrap();
        assert!(!positive.skills.is_empty());
        memory
            .retire_runtime_skill(crate::RuntimeSkillRetireRequest {
                locator: positive.skills[0].locator.clone(),
                observed_at: 1_800_000_100,
            })
            .unwrap();
        let owner_doc = platform.read_json_namespace(OWNER).unwrap().pop().unwrap();
        let old: RuntimeSkillOwnerRecord = serde_json::from_value(owner_doc.value.clone()).unwrap();
        assert_eq!(old.lifecycle.state, RuntimeSkillLifecycleState::Retired);
        let manifest_doc = platform
            .read_json_namespace(MANIFEST)
            .unwrap()
            .pop()
            .unwrap();
        let manifest: RuntimeSkillScopeManifest =
            serde_json::from_value(manifest_doc.value.clone()).unwrap();
        let mut forged = old.clone();
        forged.owner_revision += 1;
        forged.lifecycle.updated_at += 1;
        forged.lifecycle.lineage.predecessor =
            Some(RuntimeSkillOwnerBinding::from_record(&old).unwrap());
        forged.lifecycle.state = RuntimeSkillLifecycleState::Active;
        forged.lifecycle.availability = RuntimeSkillAvailability::Enabled;
        forged.content_digest = forged.canonical_content_digest().unwrap();
        assert!(
            forged.validate_contract().accepted,
            "the attack has a valid standalone post-image"
        );
        let next_manifest = RuntimeSkillScopeManifest::build(
            manifest.revision + 1,
            memory.memory_space_id(),
            scope,
            [RuntimeSkillOwnerBinding::from_record(&forged).unwrap()],
            16,
        )
        .unwrap();
        let docs = [
            (owner_doc, serde_json::to_value(&forged).unwrap()),
            (manifest_doc, serde_json::to_value(next_manifest).unwrap()),
        ];
        let conditions = docs
            .iter()
            .map(|(doc, _)| StoreJsonPrecondition::Exact {
                namespace: doc.namespace.clone(),
                key: doc.key.clone(),
                value: doc.value.clone(),
            })
            .collect::<Vec<_>>();
        let mutations = docs
            .iter()
            .map(|(doc, value)| StoreMutation::PutJson {
                namespace: doc.namespace.clone(),
                key: doc.key.clone(),
                value: value.clone(),
                event_kind: crate::MemoryStoreEventKind::MemoryControl,
                plane: "procedural_memory".into(),
                record_key: doc.key.clone(),
            })
            .collect();
        let before = platform.export_store_snapshot().unwrap();
        let rejected = if primitive {
            let request = crate::store_internal::StoreTransactionRequest::new(
                format!("primitive-resurrection-{backend}"),
                conditions,
                docs.iter()
                    .map(
                        |(doc, value)| crate::store_internal::StoreEngineMutation::PutJson {
                            namespace: doc.namespace.clone(),
                            key: doc.key.clone(),
                            value: value.clone(),
                        },
                    )
                    .collect(),
                None,
            );
            platform
                .engine_for_test()
                .commit_transaction(&request)
                .is_err()
        } else {
            platform
                .commit_governed_memory_transaction_with_runtime_budget_at(
                    crate::StoreMutationBatch {
                        transaction_id: format!("forged-resurrection-{backend}"),
                        operation: "runtime_skill.control".into(),
                        scope: memory.memory_write_transaction_scope(),
                        mutations,
                    },
                    &conditions,
                    &memory.runtime_budget(),
                    forged.lifecycle.updated_at,
                )
                .is_err()
        };
        assert!(rejected, "{backend}/primitive={primitive}: standalone-valid terminal resurrection must fail in the actual Store transaction");
        assert_eq!(
            platform.export_store_snapshot().unwrap(),
            before,
            "owner/manifest/fence/jobs/events must all remain unchanged"
        );
        drop(memory);
        drop(platform);
        if backend != "memory" {
            let reopened = StorePlatform::open(config).unwrap();
            assert_eq!(
                reopened.read_json_namespace(OWNER).unwrap()[0].value,
                serde_json::to_value(old).unwrap()
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn production_promotion_rejects_missing_or_forged_source_authority_without_writes() {
    let (platform, memory, input) =
        fixture(StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap());
    for corruption in ["missing-source-cas", "generic-kind", "rebound-source-job"] {
        let mut bad = input.clone();
        if corruption == "missing-source-cas" {
            bad.experience_preconditions.retain(|condition| !matches!(condition,
                    StoreJsonPrecondition::Exact { namespace, value, .. } if namespace == "conversation_transcript" && value["turn_id"] == "first-source"));
        } else {
            for mutation in &mut bad.experience_mutations {
                let StoreMutation::PutJson {
                    namespace, value, ..
                } = mutation
                else {
                    continue;
                };
                if namespace != crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE {
                    continue;
                }
                let original: RuntimeSkillOwnerRecord =
                    serde_json::from_value(value.clone()).unwrap();
                let mut intrinsic = original.intrinsic_contract.clone();
                if corruption == "generic-kind" {
                    intrinsic.evidence_bindings[0].kind =
                        RuntimeSkillEvidenceKind::GovernedEvidence;
                } else {
                    intrinsic.evidence_bindings[0].safe_ref =
                        format!("procedural_feedback_job:sha256:{}", "f".repeat(64));
                }
                let forged = RuntimeSkillOwnerRecord::build(
                    &original.memory_space_id,
                    original.owning_scope,
                    original.creation_ref,
                    original.owner_revision,
                    intrinsic,
                    original.procedural_content,
                    original.lifecycle,
                    original.privacy_class,
                )
                .unwrap();
                for binding in &mut bad.applied_owner_bindings {
                    if let bm_core::memory::ProceduralAppliedOwnerBindingV1::RuntimeSkill {
                        binding,
                    } = binding
                    {
                        *binding = RuntimeSkillOwnerBinding::from_record(&forged).unwrap();
                    }
                }
                *value = serde_json::to_value(forged).unwrap();
            }
        }
        let before = platform.export_store_snapshot().unwrap();
        assert!(
            complete(&platform, &memory, bad).is_err(),
            "{corruption} must fail actual Store completion admission"
        );
        assert_eq!(
            platform.export_store_snapshot().unwrap(),
            before,
            "{corruption} must have no partial writes"
        );
    }
    complete(&platform, &memory, input).unwrap();
    assert!(
        !platform
            .read_json_namespace(crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE)
            .unwrap()
            .is_empty(),
        "the unchanged real promotion plan is a nonempty committed positive control"
    );
}

#[test]
fn procedural_ledger_reads_reject_current_request_key_and_byte_budget_overruns() {
    use bm_core::budget::{
        compile_nonproduction_runtime_budget, NonproductionRuntimeBudgetLimits, RuntimeBudgetInput,
    };
    let (platform, memory, input) =
        fixture(StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap());
    complete(&platform, &memory, input).unwrap();
    fill_with_durable_observation_jobs(&platform, &memory, minimum_valid_store_entries() + 1, 0);
    let report = memory.runtime_budget();
    let ledger_keys = platform
        .export_store_snapshot()
        .unwrap()
        .json_docs
        .into_iter()
        .filter(|document| {
            document.namespace
                == crate::store_internal::schema::PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE
        })
        .map(|document| document.key)
        .collect::<Vec<_>>();
    let positive = platform
        .read_procedural_application_ledgers_with_runtime_budget(&report, &ledger_keys)
        .unwrap();
    assert!(
        positive.len() >= 2,
        "real accepted production ledgers are present"
    );
    for limited_by in ["keys", "bytes"] {
        let mut capacity = report.store_budget;
        if limited_by == "keys" {
            capacity.kv_max_entries = minimum_valid_store_entries();
        } else {
            capacity.snapshot_max_bytes = 2;
        }
        let limited = compile_nonproduction_runtime_budget(
            RuntimeBudgetInput {
                profile: report.profile,
                resource_snapshot: report.resource_snapshot.clone(),
                static_platform_manifest: report.static_platform_manifest.clone(),
                provider_model_context_limit: report.provider_model_context_limit.clone(),
            },
            NonproductionRuntimeBudgetLimits::new()
                .try_with_store_budget_limit(capacity)
                .unwrap(),
        )
        .unwrap();
        let result = platform
            .read_procedural_application_ledgers_with_runtime_budget(&limited, &ledger_keys);
        assert!(
            result.is_err(),
            "{limited_by} request limit must reject before returning excess ledger material"
        );
        assert_eq!(result.unwrap_err().stage(), "store_budget_exceeded");
    }
}

#[test]
fn production_promotion_completion_budget_failure_is_atomic_and_recoverable() {
    let root = std::env::temp_dir().join(format!(
        "bm-promotion-budget-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = StoreBackendConfig::file(&root, ProfileId::native_dev_full().unwrap()).unwrap();
    let (platform, memory, input) = fixture(config.clone());
    fill_with_durable_observation_jobs(&platform, &memory, 0, minimum_valid_store_entries());
    // Filling is a real mutation of the shared experience manifest. Replan
    // the same leased job against that state so this test reaches capacity,
    // not an unrelated stale-manifest CAS rejection.
    let job = crate::store_internal::procedural_feedback::read_job(&platform, &input.job_id)
        .unwrap()
        .unwrap();
    let input = completion_input(&platform, &memory, &job);
    let before = platform.export_store_snapshot().unwrap();
    let scope = memory.memory_write_transaction_scope();
    let mut capacity = memory.runtime_budget().store_budget;
    capacity.kv_max_entries = before.json_docs.len() + before.blobs.len();
    let constrained = config
        .clone()
        .try_with_nonproduction_store_budget_limit(capacity)
        .unwrap();
    drop(memory);
    drop(platform);
    let limited = StorePlatform::open(constrained).unwrap();
    let before_commit = limited.export_store_snapshot().unwrap();
    // Opening is setup and has its own lifecycle event. The rejected
    // completion must leave this already-open Store completely unchanged.
    let limited_budget = limited.current_runtime_budget(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );
    let error =
        complete_procedural_feedback_job(&limited, scope.clone(), &limited_budget, input.clone())
            .unwrap_err();
    assert_eq!(
        error.stage(),
        "memory_write_transaction_preflight_failed",
        "actual post-image capacity must reject this otherwise valid plan: {error}"
    );
    let after_rejection = limited.export_store_snapshot().unwrap();
    assert!(
        before_commit == after_rejection,
        "budget-rejected completion changed Store data, events or metadata"
    );
    drop(limited);
    let restored = StorePlatform::open(config).unwrap();
    let budget = restored.current_runtime_budget(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );
    complete_procedural_feedback_job(&restored, scope, &budget, input).unwrap();
    assert!(!restored
        .read_json_namespace(crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE)
        .unwrap()
        .is_empty());
    drop(restored);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(feature = "sqlite-store")]
fn pfi2_real_sqlite_writer_lock_remains_retryable_without_partial_completion() {
    let root = std::env::temp_dir().join(format!(
        "bm-pfi2-busy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("synthetic.db");
    let config = StoreBackendConfig::sqlite(&database, ProfileId::native_dev_full().unwrap())
        .unwrap()
        .with_lock_timeout(std::time::Duration::from_millis(40));
    let (platform, memory, input) = fixture(config);
    let before = platform.export_store_snapshot().unwrap();
    let contender = rusqlite::Connection::open(&database).unwrap();
    contender.execute_batch("BEGIN IMMEDIATE").unwrap();
    let error = complete(&platform, &memory, input.clone()).unwrap_err();
    assert_eq!(error.stage(), "store_transaction_busy");
    let safe = crate::ProceduralLearningSdkError::from_owner_error(
        crate::ProceduralLearningSdkOperation::ApplyFeedback,
        &error,
    );
    assert_eq!(
        safe.key,
        crate::ProceduralLearningErrorKeyV1::StoreUnavailable
    );
    assert_eq!(
        safe.disposition,
        crate::ProceduralLearningSdkErrorDisposition::StoreCommitRejected
    );
    assert!(std::error::Error::source(&safe).is_none());
    contender.execute_batch("ROLLBACK").unwrap();
    assert!(
        platform.export_store_snapshot().unwrap() == before,
        "busy completion changed durable state"
    );
    complete(&platform, &memory, input).unwrap();
    assert!(!platform
        .read_json_namespace(crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE)
        .unwrap()
        .is_empty());
    drop(contender);
    drop(memory);
    drop(platform);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pfi2_real_capacity_exhaustion_blocks_worker_until_explicit_reopen_resume() {
    use bm_core::memory::{ProceduralFeedbackErrorClassV1, ProceduralFeedbackJobStatusV1};
    struct NoProvider;
    impl crate::GovernanceExecutionPort for NoProvider {
        fn execute(
            &mut self,
            _: &crate::AuthorizedGovernanceEnvelope,
            _: &crate::ImmutableGovernanceExecutionBinding,
            _: &crate::GovernanceEgressAuthority,
            _: &mut dyn crate::GovernanceExecutionOperation,
        ) -> std::result::Result<(), crate::GovernanceExecutionPortFailure> {
            panic!("synthetic reconciliation must not invoke Provider")
        }
    }
    let root = std::env::temp_dir().join(format!(
        "bm-pfi2-real-capacity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let build = |platform: &StorePlatform, governor: bool| {
        let mut scoped =
            SubjectScopedRuntime::single_agent_default("atomic-owner", "atomic-agent", None)
                .unwrap();
        if governor {
            scoped.actor_subject_id = bm_core::memory::system_governor_subject_id("atomic-owner");
        }
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("atomic-agent", "atomic-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "atomic-promotion-chat").unwrap())
            .scoped_runtime(scoped)
            .store(crate::MemoryStoreHandle::from_platform(platform.clone()))
            .clock(Arc::new(Clock))
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
            .agent_tool_registry(registry())
            .build()
            .unwrap()
    };
    for backend in ["memory", "file"]
        .into_iter()
        .chain(cfg!(feature = "sqlite-store").then_some("sqlite"))
    {
        let profile = ProfileId::native_dev_full().unwrap();
        let config = match backend {
            "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => StoreBackendConfig::file(root.join("file"), profile).unwrap(),
            "sqlite" => StoreBackendConfig::sqlite(root.join("sqlite.db"), profile).unwrap(),
            _ => unreachable!(),
        };
        let (platform, memory, input) = fixture(config.clone());
        complete(&platform, &memory, input).unwrap();
        assert!(!platform
            .read_json_namespace(crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE)
            .unwrap()
            .is_empty());
        fill_with_durable_observation_jobs(&platform, &memory, 0, minimum_valid_store_entries());
        memory
            .request_transcript_lifecycle(crate::MemoryTranscriptLifecycleRequest {
                memory_space_id: memory.memory_space_id().into(),
                channel_id: "sdk.direct".into(),
                conversation_id: "atomic-promotion-chat".into(),
                turn_id: Some("first-source".into()),
                transition: bm_core::memory::TranscriptLifecycleTransition::DeleteRaw,
                reason: "synthetic capacity source withdrawal".into(),
            })
            .unwrap();
        let snapshot = platform.export_store_snapshot().unwrap();
        let mut capacity = memory.runtime_budget().store_budget;
        capacity.kv_max_entries = snapshot.json_docs.len() + snapshot.blobs.len();
        let limited_config = config
            .clone()
            .try_with_nonproduction_store_budget_limit(capacity)
            .unwrap();
        drop(memory);
        drop(platform);
        let limited = StorePlatform::open(limited_config.clone()).unwrap();
        if backend == "memory" {
            limited.import_store_snapshot(&snapshot).unwrap();
        }
        let runtime = Arc::new(build(&limited, false));
        let engine = crate::MemoryLearningEngine::attach(runtime.clone()).unwrap();
        let before = limited.export_store_snapshot().unwrap();
        let outcome = engine
            .run_due_cycle(
                crate::MemoryLearningCycleRequest {
                    lease_owner: "actual-capacity-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap();
        let crate::MemoryLearningCycleOutcome::ProceduralBlocked(report) = outcome else {
            panic!("{backend}: actual Store capacity must block official worker: {outcome:?}");
        };
        assert_eq!(
            report.job.status,
            ProceduralFeedbackJobStatusV1::BlockedCapacity
        );
        assert_eq!(
            report.job.last_error_class,
            Some(ProceduralFeedbackErrorClassV1::BudgetExceeded)
        );
        let after = limited.export_store_snapshot().unwrap();
        let stable = |snapshot: &crate::store_internal::StoreSnapshot| {
            snapshot
                .json_docs
                .iter()
                .filter(|doc| {
                    !matches!(
                        doc.namespace.as_str(),
                        crate::store_internal::schema::PROCEDURAL_FEEDBACK_JOB_NAMESPACE
                            | crate::store_internal::schema::PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE
                    )
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        assert!(
            stable(&after) == stable(&before),
            "failed page cannot publish assets/material/manifest/receipt/audit"
        );
        assert_eq!(after.blobs, before.blobs);
        assert!(!runtime
            .due_procedural_feedback_jobs(256)
            .unwrap()
            .iter()
            .any(|job| job.job_id == report.job.job_id));
        drop(engine);
        drop(runtime);
        drop(limited);
        let reopened = StorePlatform::open(limited_config).unwrap();
        if backend == "memory" {
            reopened.import_store_snapshot(&after).unwrap();
        }
        let blocked_runtime = build(&reopened, false);
        drop(report);
        let status = blocked_runtime
            .procedural_reconciliation_status(crate::MemoryProceduralReconciliationStatusRequest {
                authority: blocked_runtime
                    .learning_attachment_status_authority()
                    .unwrap(),
            })
            .unwrap();
        assert_eq!(
            status.read_availability,
            bm_core::memory::ProceduralLearningReadAvailabilityV1::Blocked
        );
        let recovery = status
            .recovery
            .expect("fresh public discovery of durable capacity block");
        assert!(!blocked_runtime
            .due_procedural_feedback_jobs(256)
            .unwrap()
            .iter()
            .any(|job| job.job_id == recovery.job_id));
        let retained = reopened.export_store_snapshot().unwrap();
        drop(blocked_runtime);
        drop(reopened);
        let restored = StorePlatform::open(config).unwrap();
        if backend == "memory" {
            restored.import_store_snapshot(&retained).unwrap();
        }
        let governor = build(&restored, true);
        governor
            .resume_procedural_reconciliation(crate::MemoryProceduralReconciliationResumeRequest {
                operation_id: "capacity-expanded-explicit-resume".into(),
                job_id: recovery.job_id.clone(),
                expected_state_revision: recovery.expected_state_revision,
            })
            .unwrap();
        let runtime = Arc::new(build(&restored, false));
        let engine = crate::MemoryLearningEngine::attach(runtime.clone()).unwrap();
        let mut completed = false;
        for _ in 0..128 {
            // Exercise capacity recovery with a fresh resource observation at
            // each synthetic host scheduling boundary. Keep the real Store
            // budget and all transaction TTL/authority checks unchanged.
            runtime.refresh_runtime_resource_snapshot().unwrap();
            match engine
                .run_due_cycle(
                    crate::MemoryLearningCycleRequest {
                        lease_owner: "capacity-restored-worker".into(),
                        lease_duration_secs: 60,
                    },
                    &mut NoProvider,
                )
                .unwrap()
            {
                crate::MemoryLearningCycleOutcome::ProceduralReconciliation(page) => {
                    assert_eq!(page.job.job_id, recovery.job_id);
                    if page.completed {
                        completed = true;
                        break;
                    }
                }
                crate::MemoryLearningCycleOutcome::Blocked(state) => {
                    assert_eq!(state.reason, "governance_execution_binding_unavailable")
                }
                other => panic!("{backend}: resumed work must finish: {other:?}"),
            }
        }
        assert!(
            completed,
            "{backend}: real capacity recovery must publish its complete inventory"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pfi2_reconciliation_retry_exhaustion_is_a_valid_discoverable_block() {
    use crate::store_internal::procedural_feedback::{
        claim_procedural_feedback_job, retry_procedural_feedback_job,
    };
    use bm_core::memory::{
        ProceduralFeedbackErrorClassV1, ProceduralFeedbackJobStatusV1,
        ProceduralReconciliationBlockV1,
    };
    let root = std::env::temp_dir().join(format!(
        "bm-reconciliation-dead-letter-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for backend in ["memory", "file"]
        .into_iter()
        .chain(cfg!(feature = "sqlite-store").then_some("sqlite"))
    {
        for class in [
            ProceduralFeedbackErrorClassV1::StoreUnavailable,
            ProceduralFeedbackErrorClassV1::CasConflict,
        ] {
            let profile = ProfileId::native_dev_full().unwrap();
            let label = format!("{backend}-{class:?}");
            let config = match backend {
                "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
                "file" => StoreBackendConfig::file(root.join(&label), profile).unwrap(),
                _ => StoreBackendConfig::sqlite(root.join(format!("{label}.db")), profile).unwrap(),
            };
            let (mut platform, memory, input) = fixture(config.clone());
            complete(&platform, &memory, input).unwrap();
            memory
                .request_transcript_lifecycle(crate::MemoryTranscriptLifecycleRequest {
                    memory_space_id: memory.memory_space_id().into(),
                    channel_id: "sdk.direct".into(),
                    conversation_id: "atomic-promotion-chat".into(),
                    turn_id: Some("first-source".into()),
                    transition: bm_core::memory::TranscriptLifecycleTransition::DeleteRaw,
                    reason: "synthetic retry exhaustion".into(),
                })
                .unwrap();
            let mut job = memory
                .due_procedural_feedback_jobs(32)
                .unwrap()
                .into_iter()
                .find(|job| {
                    matches!(
                        job.work,
                        bm_core::memory::ProceduralLearningWorkV1::Reconcile { .. }
                    )
                })
                .unwrap();
            for _ in 0..job.max_attempts {
                let now = job.next_attempt_at.unwrap_or(job.created_at);
                let claimed = claim_procedural_feedback_job(
                    &platform,
                    memory.memory_write_transaction_scope(),
                    &memory.runtime_budget(),
                    &job.job_id,
                    "retry-exhaustion",
                    now + 60,
                    now,
                )
                .unwrap();
                job = retry_procedural_feedback_job(
                    &platform,
                    memory.memory_write_transaction_scope(),
                    &memory.runtime_budget(),
                    &job.job_id,
                    "retry-exhaustion",
                    claimed.lease_epoch,
                    class,
                    now,
                )
                .unwrap();
            }
            assert_eq!(job.status, ProceduralFeedbackJobStatusV1::DeadLetter);
            drop((job, memory));
            if backend != "memory" {
                drop(platform);
                platform = StorePlatform::open(config).unwrap();
            }
            let reopened = MemoryRuntime::builder()
                .identity(MemoryIdentity::new("atomic-agent", "atomic-owner").unwrap())
                .scope(MemoryScope::new("sdk.direct", "new-host-chat").unwrap())
                .store(crate::MemoryStoreHandle::from_platform(platform.clone()))
                .clock(Arc::new(Clock))
                .build()
                .unwrap();
            let status = reopened
                .procedural_reconciliation_status(
                    crate::MemoryProceduralReconciliationStatusRequest {
                        authority: reopened.learning_attachment_status_authority().unwrap(),
                    },
                )
                .unwrap();
            assert_eq!(
                status.read_availability,
                crate::ProceduralLearningReadAvailabilityV1::Blocked
            );
            assert_eq!(
                status.block_reason,
                Some(ProceduralReconciliationBlockV1::RepairRequired)
            );
            assert!(
                status.recovery.is_none(),
                "non-capacity dead letter is never implicitly resumable"
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
