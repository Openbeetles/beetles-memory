//! Real Store + official engine fixtures. No synthesized receipt or post-image.
use super::*;
use crate::*;
use bm_core::skills::AgentToolExperienceRevisionMaterialV4;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

const NOW: u64 = 1_800_000_000;
const STRATEGY: &str = "Calculate a checksum before extraction and reject a mismatch.";
struct Clock;
impl MemoryClock for Clock {
    fn now_secs(&self) -> u64 {
        NOW
    }
}
struct Http;
impl LlmHttpClient for Http {
    fn do_post(
        &mut self,
        _: &str,
        _: &[(&str, &str)],
        _: &[u8],
    ) -> Result<(u16, bm_core::platform::ResponseBody)> {
        panic!("synthetic contract must not access a provider")
    }
}
struct Model;
impl LlmClient for Model {
    fn chat(
        &self,
        _: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        _: Option<&[ToolSpec]>,
        _: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let content = if system.contains("method learning extraction") {
            assert!(messages
                .iter()
                .any(|message| message.content.contains(STRATEGY)));
            r#"[{"task_signature":"verify_file","procedure":"1. Calculate a checksum before extraction.\n2. Reject a mismatch before continuing.","constraints":[],"source_material_ids":["governed-source"]}]"#
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
struct Port;
impl GovernanceExecutionPort for Port {
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
            .run(&mut Http, &Model)
            .map_err(GovernanceExecutionPortFailure::Other)
    }
}

#[derive(Clone, Copy)]
enum SourceContract {
    AfterReconcile,
    LockedCommit,
    WireAllowed,
    WireWithdrawn,
    OpenCommitmentCorrupt,
    OpenWithdrawnCommitmentCorrupt,
    OpenWithdrawnBindingCommitmentCorrupt,
}

fn revoked_source_admission(config: StoreBackendConfig, reopen: bool, contract: SourceContract) {
    let store = MemoryStoreHandle::open_for_nonproduction_harness(config.clone()).unwrap();
    let registry = AgentToolRegistrySnapshot::compact(
        "source-tools",
        "executor",
        vec![AgentToolDescriptor::compact(
            "verify_file",
            "Verify",
            "schema-checksum",
        )],
        NOW,
    );
    let runtime = Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("source-agent", "source-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "source-chat").unwrap())
            .clock(Arc::new(Clock))
            .store(store.clone())
            .agent_tool_registry(registry.clone())
            .build()
            .unwrap(),
    );
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "checksum source".into(),
                    content: STRATEGY.into(),
                    keywords: vec!["checksum".into()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("source-chat".into()),
                    source_type: None,
                    source_scope: None,
                    subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
                    provenance: LongTermMemoryProvenance::new(
                        MemoryEvidenceAuthority::ProgramMemoryCanonical,
                    ),
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["synthetic-source".into()],
                    canonical_entities: vec![],
                    evidence_count: Some(1),
                    observed_at: Some(NOW),
                    source_revision: Some(1),
                }],
                deletes: vec![],
                skill_writes: vec![],
            },
        })
        .unwrap();
    let source: bm_core::memory::LongTermMemoryVersionMaterial = serde_json::from_value(
        store
            .platform()
            .read_json_namespace(super::super::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE)
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .value,
    )
    .unwrap();
    let mut scoped = runtime.scoped_runtime().clone();
    scoped.actor_subject_id = runtime
        .subject_registry()
        .system_governor()
        .unwrap()
        .subject_id
        .clone();
    let governor = MemoryRuntime::builder()
        .identity(runtime.config().identity.clone())
        .scope(runtime.scope().clone())
        .store(store.clone())
        .clock(Arc::new(Clock))
        .subject_registry(runtime.subject_registry().clone())
        .subject_relationship_graph(runtime.subject_relationship_graph().clone())
        .scoped_runtime(scoped)
        .agent_tool_registry(registry.clone())
        .build()
        .unwrap();
    let registered = governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "register-source-observer".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: ProceduralProducerSpecV1 {
                binding_id: "source-observer".into(),
                scope: ProceduralProducerScopeV1 {
                    memory_space_id: runtime.memory_space_id().into(),
                    mounted_subject_id: runtime.subject_id().into(),
                    channel_id: "sdk.direct".into(),
                    chat_id: "source-chat".into(),
                },
                principal: ProceduralProducerPrincipalV1::LocalCapability {
                    capability_id: "source-capability".into(),
                },
                source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                claims: ProceduralProducerClaimsV1 {
                    method_learning_materials: true,
                    method_adoptions: false,
                    method_task_evaluations: false,
                    execution_facts: false,
                    method_declarations: false,
                    usage_feedback: false,
                    source_classifications: vec![ProceduralSourceSensitivity::NonPrivate],
                },
                tools: vec![bm_core::memory::ProceduralProducerToolV1 {
                    registry_ref: registry.registry_ref(),
                    tool_id: "verify_file".into(),
                    schema_fingerprint: "schema-checksum".into(),
                }],
                source_config_ref: "synthetic-source-config".into(),
                method_evaluators: vec![],
            },
        })
        .unwrap();
    let capability = governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
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
            material_id: "governed-source".into(),
            purpose: MethodLearningMaterialPurposeV1::TaskConstraint,
            source: MethodLearningMaterialSourceV1::GovernedRevision {
                revision: source.owner_revision_ref(),
                content_digest: source.content_digest.clone(),
            },
            text: STRATEGY.into(),
            sensitivity: ProceduralSourceSensitivity::NonPrivate,
            external_content: false,
        }],
    });
    runtime
        .finalize_turn_with_procedural_evidence(
            &capability,
            MemoryTurnFinalizeRequest {
                turn: CanonicalTurnDelta {
                    turn_id: "source-turn".into(),
                    conversation: ConversationScope {
                        channel: "sdk.direct".into(),
                        chat_id: "source-chat".into(),
                        conversation_id: Some("source-chat".into()),
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
                        "Learn from the admitted checksum instructions.",
                    )],
                    assistant_message: Some(TranscriptInputMessage::assistant(
                        "I will use checksum validation.",
                    )),
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
    let engine = MemoryLearningEngine::attach(runtime.clone()).unwrap();
    let request = || MemoryLearningCycleRequest {
        lease_owner: "source-test".into(),
        lease_duration_secs: 60,
    };
    if matches!(
        contract,
        SourceContract::LockedCommit | SourceContract::WireAllowed | SourceContract::WireWithdrawn
    ) {
        let checked = Arc::new(AtomicBool::new(false));
        let bytes = Arc::new(AtomicUsize::new(0));
        let model = SourceBoundaryModel {
            runtime: runtime.clone(),
            store: store.clone(),
            source_id: source.owner_ref.owner_id.clone(),
            contract,
            checked: checked.clone(),
        };
        let outcome = engine
            .run_due_cycle(
                request(),
                &mut SourceBoundaryPort {
                    model: &model,
                    bytes: bytes.clone(),
                },
            )
            .unwrap();
        assert!(
            checked.load(Ordering::SeqCst),
            "nonempty admitted method must reach the boundary probe: {outcome:?}"
        );
        if matches!(contract, SourceContract::WireAllowed) {
            assert!(
                bytes.load(Ordering::SeqCst) > 0,
                "positive control must write real bytes to the synthetic transport"
            );
            assert!(
                matches!(outcome, MemoryLearningCycleOutcome::Completed(_)),
                "{outcome:?}"
            );
        } else {
            assert_eq!(
                bytes.load(Ordering::SeqCst),
                0,
                "withdrawal must stop before the inner transport sees any bytes"
            );
            assert!(
                !matches!(outcome, MemoryLearningCycleOutcome::Completed(_)),
                "revoked source unexpectedly completed"
            );
        }
        return;
    }
    let outcome = engine.run_due_cycle(request(), &mut Port).unwrap();
    let MemoryLearningCycleOutcome::Completed(report) = outcome else {
        panic!("official engine must produce an actual method: {outcome:?}")
    };
    let method: AgentToolExperienceRevisionMaterialV4 = serde_json::from_value(
        store
            .platform()
            .read_json_namespace(super::super::schema::AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
            .unwrap()
            .into_iter()
            .find(|doc| {
                serde_json::from_value::<AgentToolExperienceRevisionMaterialV4>(doc.value.clone())
                    .is_ok_and(|material| !material.body.method_sources().is_empty())
            })
            .unwrap()
            .value,
    )
    .unwrap();
    let mutation = StoreMutation::PutJson {
        namespace: super::super::schema::AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.into(),
        key: method.physical_key.clone(),
        value: serde_json::to_value(&method).unwrap(),
        event_kind: MemoryStoreEventKind::MemoryMaintenance,
        plane: "agent_tool_experience".into(),
        record_key: method.physical_key.clone(),
    };
    assert!(
        super::derive_method_application(
            store.platform(),
            &report.job,
            std::slice::from_ref(&mutation),
            &mut Vec::new()
        )
        .is_ok(),
        "nonempty source must have a positive Store admission control"
    );
    if matches!(
        contract,
        SourceContract::OpenCommitmentCorrupt
            | SourceContract::OpenWithdrawnCommitmentCorrupt
            | SourceContract::OpenWithdrawnBindingCommitmentCorrupt
    ) {
        if !matches!(contract, SourceContract::OpenCommitmentCorrupt) {
            runtime
                .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
                    memory_space_id: runtime.memory_space_id().into(),
                    channel_id: "sdk.direct".into(),
                    conversation_id: "source-chat".into(),
                    turn_id: Some("source-turn".into()),
                    transition: TranscriptLifecycleTransition::DeleteRaw,
                    reason: "withdraw synthetic transcript before structural audit".into(),
                })
                .unwrap();
            assert!(
                store
                    .platform()
                    .read_json_docs_by_keys(
                        super::super::schema::AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                        std::slice::from_ref(&method.physical_key),
                    )
                    .unwrap()
                    .is_empty(),
                "withdrawal really erased the immutable producing raw"
            );
        }
        drop(
            MemoryStoreHandle::open_for_nonproduction_harness(config.clone())
                .expect("positive Store-open control with lawful producing source"),
        );
        if matches!(
            contract,
            SourceContract::OpenWithdrawnBindingCommitmentCorrupt
        ) {
            let PostTurnGovernanceExecutionBindingV1::Bound {
                binding_id,
                binding_revision,
            } = &report.job.execution_binding
            else {
                panic!("real bound execution required");
            };
            let original =
                super::read_binding_snapshot(store.platform(), binding_id, *binding_revision)
                    .unwrap()
                    .unwrap();
            // Keep the exact binding id/revision and a syntactically valid,
            // reciprocal index. The immutable receipt must still reject this
            // different actual snapshot, even with every raw method erased.
            let forged = PostTurnGovernanceBindingSnapshotV1::new(
                original.source_owner_id.clone(),
                original.source_config_id.clone(),
                original.source_revision,
                original.protocol,
                original.endpoint.clone(),
                "synthetic-forged-model",
                original.credential_reference.clone(),
                original.request_timeout_ms,
                original.max_input_tokens,
                original.max_output_tokens,
                original.provider_permission_generation,
                original.created_at,
            )
            .unwrap();
            assert_eq!(forged.storage_key(), original.storage_key());
            assert_ne!(forged.canonical_digest, original.canonical_digest);
            let mut index = super::read_binding_revision_index(store.platform(), binding_id)
                .unwrap()
                .unwrap();
            index
                .revisions
                .iter_mut()
                .find(|revision| revision.binding_revision == *binding_revision)
                .unwrap()
                .canonical_digest
                .clone_from(&forged.canonical_digest);
            index.validate().unwrap();
            for (namespace, key, value) in [
                (
                    POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE,
                    forged.storage_key(),
                    serde_json::to_value(&forged).unwrap(),
                ),
                (
                    POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_NAMESPACE,
                    binding_id.clone(),
                    serde_json::to_value(index).unwrap(),
                ),
            ] {
                store
                    .platform()
                    .tamper_json_document_for_nonproduction_harness(namespace, &key, value.clone())
                    .unwrap();
                assert_eq!(
                    store
                        .platform()
                        .read_json_docs_by_keys(namespace, &[key])
                        .unwrap()[0]
                        .value,
                    value
                );
            }
        } else {
            // Syntactically valid job/index metadata is not proof that its
            // producing input still matches the durable application receipt.
            let mut forged_job = report.job.clone();
            forged_job.transcript_digest = format!("sha256:{}", "0".repeat(64));
            assert_ne!(forged_job.transcript_digest, report.job.transcript_digest);
            forged_job.validate().unwrap();
            let value = serde_json::to_value(&forged_job).unwrap();
            store
                .platform()
                .tamper_json_document_for_nonproduction_harness(
                    POST_TURN_GOVERNANCE_JOB_NAMESPACE,
                    &forged_job.job_id,
                    value.clone(),
                )
                .unwrap();
            assert_eq!(
                store
                    .platform()
                    .read_json_docs_by_keys(
                        POST_TURN_GOVERNANCE_JOB_NAMESPACE,
                        std::slice::from_ref(&forged_job.job_id)
                    )
                    .unwrap()[0]
                    .value,
                value
            );
        }
        drop(engine);
        drop(governor);
        drop(runtime);
        drop(store);
        let reopened = MemoryStoreHandle::open_for_nonproduction_harness(config);
        let Err(error) = reopened else {
            panic!("Store open must reject changed producing commitments even after raw provenance was erased")
        };
        assert!(
            error
                .to_string()
                .contains("semantic_method_source_repair_required"),
            "wrong Store-open rejection boundary: {error}"
        );
        return;
    }
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(source.owner_ref.owner_id.clone()),
            },
            reason: "withdraw synthetic method source".into(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .unwrap();
    let mut reconciled = false;
    for _ in 0..8 {
        match engine.run_due_cycle(request(), &mut Port).unwrap() {
            MemoryLearningCycleOutcome::ProceduralReconciliation(report) if report.completed => {
                reconciled = true
            }
            MemoryLearningCycleOutcome::Idle { .. } => break,
            _ => {}
        }
    }
    assert!(
        reconciled,
        "real reconciliation must finish before testing Ready source denial"
    );
    drop(engine);
    drop(governor);
    drop(runtime);
    let store = if reopen {
        drop(store);
        MemoryStoreHandle::open_for_nonproduction_harness(config).unwrap()
    } else {
        store
    };
    let before = store.platform().read_events().unwrap();
    assert!(
        super::derive_method_application(
            store.platform(),
            &report.job,
            &[mutation],
            &mut Vec::new()
        )
        .is_err(),
        "Store admission must reject revoked source even after the subject returned to Ready"
    );
    assert_eq!(store.platform().read_events().unwrap(), before);
}

#[test]
fn store_semantic_application_rejects_revoked_governed_source_after_reconciliation() {
    revoked_source_admission(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap(),
        false,
        SourceContract::AfterReconcile,
    );
}

#[test]
fn file_semantic_application_rejects_revoked_governed_source_after_reopen() {
    let path =
        std::env::temp_dir().join(format!("beetle-method-source-file-{}", std::process::id()));
    revoked_source_admission(
        StoreBackendConfig::file(&path, ProfileId::native_dev_full().unwrap()).unwrap(),
        true,
        SourceContract::AfterReconcile,
    );
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_semantic_application_rejects_revoked_governed_source_after_reopen() {
    let path = std::env::temp_dir().join(format!(
        "beetle-method-source-sqlite-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    revoked_source_admission(
        StoreBackendConfig::sqlite(
            path.join("memory.sqlite"),
            ProfileId::native_dev_full().unwrap(),
        )
        .unwrap(),
        true,
        SourceContract::AfterReconcile,
    );
    std::fs::remove_dir_all(path).unwrap();
}

struct RecordingHttp(Arc<AtomicUsize>);
impl LlmHttpClient for RecordingHttp {
    fn do_post(
        &mut self,
        _: &str,
        _: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, bm_core::platform::ResponseBody)> {
        self.0.fetch_add(body.len(), Ordering::SeqCst);
        Ok((200, bm_core::platform::ResponseBody::Heap(Vec::new())))
    }
}
struct SourceBoundaryPort<'a> {
    model: &'a SourceBoundaryModel,
    bytes: Arc<AtomicUsize>,
}
impl GovernanceExecutionPort for SourceBoundaryPort<'_> {
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
            .run(&mut RecordingHttp(self.bytes.clone()), self.model)
            .map_err(GovernanceExecutionPortFailure::Other)
    }
}
struct SourceBoundaryModel {
    runtime: Arc<MemoryRuntime>,
    store: MemoryStoreHandle,
    source_id: String,
    contract: SourceContract,
    checked: Arc<AtomicBool>,
}
impl LlmClient for SourceBoundaryModel {
    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        tools: Option<&[ToolSpec]>,
        choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let response = Model.chat(http, system, messages, tools, choice)?;
        if !system.contains("method learning extraction") {
            return Ok(response);
        }
        assert!(
            !self.checked.swap(true, Ordering::SeqCst),
            "one durable extraction attempt"
        );
        let prepared = if matches!(self.contract, SourceContract::LockedCommit) {
            let proposal: Vec<bm_core::memory::MethodLearningProposalV1> =
                serde_json::from_str(&response.content).unwrap();
            Some(prepare_real_semantic_commit(
                &self.runtime,
                self.store.platform(),
                &proposal[0],
            ))
        } else {
            None
        };
        if !matches!(self.contract, SourceContract::WireAllowed) {
            self.runtime
                .mutate_long_term_memory(MemoryLongTermMutationRequest {
                    operation: if matches!(self.contract, SourceContract::LockedCommit) {
                        // Keep a structurally valid Present source root so the
                        // primitive reaches its semantic read-authority gate.
                        MemoryLongTermMutation::ChangeScope {
                            target: MemoryLongTermTarget::RecordId(self.source_id.clone()),
                            source_scope: bm_core::memory::LongTermMemorySourceScope::User,
                            subject_visibility: MemorySubjectVisibilityPolicy::HiddenFromSubjects(
                                vec![self.runtime.subject_id().to_string()],
                            ),
                        }
                    } else {
                        MemoryLongTermMutation::Delete {
                            target: MemoryLongTermTarget::RecordId(self.source_id.clone()),
                        }
                    },
                    reason: "withdraw between admission and use".into(),
                    dry_run: false,
                    mode_input: RuntimeLifecycleModeInput::default(),
                })
                .unwrap();
        }
        if let Some(mut prepared) = prepared {
            // Exercise the Store's own authority, independently of SDK's extra
            // source CAS. All surviving preconditions and the batch/receipt are
            // generated by the real Store planner while the job is leased.
            prepared.preconditions.retain(|condition| {
                let namespace = match condition {
                    StoreJsonPrecondition::Exact { namespace, .. }
                    | StoreJsonPrecondition::Absent { namespace, .. } => namespace,
                };
                !matches!(
                    namespace.as_str(),
                    super::super::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE
                        | super::super::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE
                        | super::super::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE
                        | super::super::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE
                )
            });
            let platform = self.store.platform();
            let before_events = platform.read_events().unwrap();
            let before_job = super::read_job(platform, &prepared.after_job.job_id).unwrap();
            let before_receipts = platform
                .read_json_namespace(bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE)
                .unwrap();
            let before_audit = platform
                .read_json_namespace(bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE)
                .unwrap();
            let rejected = platform
                .commit_governed_memory_transaction_with_preconditions(
                    prepared.batch,
                    &prepared.preconditions,
                )
                .expect_err(
                    "locked Store must reject a leased semantic post-image after source withdrawal",
                );
            assert!(
                error_chain_has_stage(&rejected, "semantic_method_completion"),
                "wrong rejection boundary: {rejected:?}"
            );
            assert_eq!(platform.read_events().unwrap(), before_events);
            assert_eq!(
                platform
                    .read_json_namespace(bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE)
                    .unwrap(),
                before_audit
            );
            assert_eq!(
                super::read_job(platform, &prepared.after_job.job_id).unwrap(),
                before_job
            );
            assert_eq!(
                platform
                    .read_json_namespace(bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE)
                    .unwrap(),
                before_receipts
            );
            assert!(platform
                .read_json_namespace(super::super::schema::AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
                .unwrap()
                .is_empty());
        }
        // The actual Runtime HTTP wrapper must perform the last permission check
        // before forwarding this nonempty body to our recording transport.
        http.do_post(
            "http://127.0.0.1:11434/api/chat",
            &[],
            messages[0].content.as_bytes(),
        )?;
        Ok(response)
    }
}

fn prepare_real_semantic_commit(
    runtime: &MemoryRuntime,
    platform: &StorePlatform,
    proposal: &bm_core::memory::MethodLearningProposalV1,
) -> super::GovernanceCompletionPending {
    use super::super::agent_tool_experience::{
        combine_agent_tool_experience_plans, AgentToolExperienceStoreMutationPlanV1,
    };
    use bm_core::skills::*;
    let job: PostTurnGovernanceJobV4 = platform
        .read_json_namespace(POST_TURN_GOVERNANCE_JOB_NAMESPACE)
        .unwrap()
        .into_iter()
        .map(|doc| serde_json::from_value::<PostTurnGovernanceJobV4>(doc.value).unwrap())
        .find(|job| job.status == PostTurnGovernanceJobStatusV2::Leased)
        .expect("real leased job");
    let PostTurnGovernanceExecutionBindingV1::Bound {
        binding_id,
        binding_revision,
    } = &job.execution_binding
    else {
        panic!("bound job")
    };
    let binding = super::read_binding_snapshot(platform, binding_id, *binding_revision)
        .unwrap()
        .unwrap();
    let key = bm_core::memory::ConversationKey::new(
        &job.identity.memory_space_id,
        &job.identity.channel_id,
        &job.identity.conversation_id,
    )
    .unwrap();
    let transcript: bm_core::memory::TranscriptTurnRecord = serde_json::from_value(
        platform
            .read_json_docs_by_keys(
                "conversation_transcript",
                &[super::super::transcript_turn_storage_key(
                    &key,
                    &job.identity.mounted_subject_id,
                    &job.identity.turn_id,
                )],
            )
            .unwrap()
            .pop()
            .unwrap()
            .value,
    )
    .unwrap();
    let source = bm_core::memory::GovernanceMethodExtractionSourceV1::build(
        &job,
        &binding,
        &transcript,
        0,
        0,
        proposal,
    )
    .unwrap();
    let tool = source.tool.clone();
    let body = AgentToolExperienceBodyV1::Method {
        task_signature: proposal.task_signature.clone(),
        procedure: proposal.procedure.clone(),
        constraints: proposal.constraints.clone(),
        sources: vec![AgentToolMethodSourceRefV2::GovernanceExtraction {
            source: Box::new(source),
        }],
        usage_contributions: vec![],
    };
    let owning_scope = AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: runtime.subject_id().into(),
    };
    let material = AgentToolExperienceRevisionMaterialV4::build(
        runtime.memory_space_id(),
        owning_scope.clone(),
        &tool.registry_ref.registry_id,
        tool.registry_ref.scope,
        &tool.tool_id,
        &tool.schema_fingerprint,
        1,
        body,
        AgentToolExperienceStatus::Candidate,
        MemoryPrivacyClass::SharedWithSubject,
        NOW,
        NOW,
        None,
    )
    .unwrap();
    let head = AgentToolExperienceOwnerHeadV3::build(
        runtime.memory_space_id(),
        owning_scope.clone(),
        material.owner_ref.clone(),
        1,
        vec![AgentToolExperienceRetainedRevisionDigestV3::from_material(&material).unwrap()],
    )
    .unwrap();
    let manifest = AgentToolExperienceScopeManifestV1::build(
        1,
        runtime.memory_space_id(),
        owning_scope,
        vec![AgentToolExperienceHeadBindingV1::from_head_and_material(&head, &material).unwrap()],
        8,
    )
    .unwrap();
    let scope = StoreEventScope::new(
        &runtime.config().identity.agent_id,
        &runtime.config().identity.owner_id,
        "sdk.direct",
        "source-chat",
    )
    .with_memory_space(runtime.memory_space_id())
    .with_subject(runtime.subject_id())
    .with_conversation("source-chat");
    let plan = AgentToolExperienceStoreMutationPlanV1::try_new(
        format!("semantic-method:{}", job.job_id),
        runtime.scoped_runtime().actor_subject_id.clone(),
        scope.clone(),
        None,
        None,
        material,
        head,
        manifest,
        NOW,
    )
    .unwrap();
    let (mutations, conditions, _) = combine_agent_tool_experience_plans(&[plan], 8).unwrap();
    let summary = PostTurnGovernanceDecisionSummaryV1::from_semantic_report(
        &bm_core::memory::PostTurnSemanticGovernanceReport::skipped("method-only test"),
    )
    .unwrap();
    let super::GovernanceCompletionPlan::Pending(prepared) = super::plan_governance_job_completion(
        platform,
        scope,
        &job.job_id,
        job.lease_owner.as_deref().unwrap(),
        job.lease_epoch,
        summary,
        mutations,
        conditions,
        NOW,
    )
    .unwrap() else {
        panic!("real leased plan")
    };
    *prepared
}

#[test]
fn leased_semantic_commit_rechecks_source_after_sdk_source_cas_is_removed() {
    revoked_source_admission(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap(),
        false,
        SourceContract::LockedCommit,
    );
}
#[test]
fn method_source_http_first_byte_has_nonempty_positive_control() {
    revoked_source_admission(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap(),
        false,
        SourceContract::WireAllowed,
    );
}
#[test]
fn method_source_withdrawal_stops_http_before_first_byte() {
    revoked_source_admission(
        StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap(),
        false,
        SourceContract::WireWithdrawn,
    );
}

#[test]
fn file_semantic_source_commitment_mismatch_fails_at_store_open() {
    let path =
        std::env::temp_dir().join(format!("beetle-method-source-open-{}", std::process::id()));
    revoked_source_admission(
        StoreBackendConfig::file(&path, ProfileId::native_dev_full().unwrap()).unwrap(),
        true,
        SourceContract::OpenCommitmentCorrupt,
    );
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn file_withdrawn_transcript_still_rejects_broken_semantic_commitment_at_open() {
    let path = std::env::temp_dir().join(format!(
        "beetle-method-withdrawn-open-{}",
        std::process::id()
    ));
    revoked_source_admission(
        StoreBackendConfig::file(&path, ProfileId::native_dev_full().unwrap()).unwrap(),
        true,
        SourceContract::OpenWithdrawnCommitmentCorrupt,
    );
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn file_withdrawn_transcript_still_rejects_changed_binding_snapshot_at_open() {
    let path = std::env::temp_dir().join(format!(
        "beetle-method-withdrawn-binding-open-{}",
        std::process::id()
    ));
    revoked_source_admission(
        StoreBackendConfig::file(&path, ProfileId::native_dev_full().unwrap()).unwrap(),
        true,
        SourceContract::OpenWithdrawnBindingCommitmentCorrupt,
    );
    std::fs::remove_dir_all(path).unwrap();
}

fn error_chain_has_stage(error: &bm_core::Error, stage: &str) -> bool {
    let mut cause: &(dyn std::error::Error + 'static) = error;
    loop {
        if cause
            .downcast_ref::<bm_core::Error>()
            .is_some_and(|error| error.stage() == stage)
        {
            return true;
        }
        match cause.source() {
            Some(source) => cause = source,
            None => return false,
        }
    }
}
