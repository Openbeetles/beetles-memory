use super::*;

fn feedback_error(key: crate::ProceduralLearningErrorKeyV1) -> Error {
    Error::Other {
        stage: "post_turn_learning_evidence",
        source: Box::new(crate::ProceduralLearningSdkError {
            operation: crate::ProceduralLearningSdkOperation::FinalizeTurn,
            key,
            disposition: crate::ProceduralLearningSdkErrorDisposition::ContractRejected,
        }),
    }
}

#[cfg(all(test, feature = "nonproduction-replay-harness"))]
mod promotion_atomic_tests {
    use super::*;
    use crate::{
        CanonicalTurnDelta, ConversationScope, MemoryTurnDeliveryStatus, MemoryTurnProtocol,
        MemoryTurnSource, PostTurnLearningInputV1, ProceduralFeedbackAuthorityInputV1,
        StoreBackendConfig, ToolObservationDigest, TranscriptInputMessage,
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
        let observations = ["first", "second"].into_iter().map(|call| bm_core::skills::AgentToolObservationDigest {
            observation_id: format!("{turn}-{call}"), registry_id: "promotion-test-tools".into(),
            tool_id: "archive.unpack".into(), schema_fingerprint: "schema-v1".into(),
            call_id: Some(format!("call-{turn}-{call}")), task_signature: "unpack archive".into(),
            summary: "1. Inspect the archive manifest\n2. Extract entries into the workspace\n3. Verify output checksums".into(),
            outcome: AgentToolOutcome::Succeeded, error_code: None, external_content: false,
            private_content_used: false, permission_tags: vec![], risk_tags: vec![],
            started_at: Some(1_800_000_000), completed_at: Some(1_800_000_000),
        }).collect::<Vec<_>>();
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
                        tool_name: value.tool_id.clone(),
                        summary: value.summary.clone(),
                        external_content: false,
                    })
                    .collect(),
                external_content_used: false,
                candidate_ids: vec![],
            },
            learning: PostTurnLearningInputV1 {
                tool_call_count: 2,
                agent_tool_feedback: vec![AgentToolUsageFeedbackV2 {
                    registry_ref: registry().registry_ref(),
                    tool_id: "archive.unpack".into(),
                    schema_fingerprint: "schema-v1".into(),
                    observations,
                    outcome: ProceduralExecutionOutcomeV1::Succeeded,
                    user_visible_result_summary: Some("Archive verified".into()),
                    operator_note: None,
                }],
                authority: ProceduralFeedbackAuthorityInputV1::HostRuntimeObservation,
                ..PostTurnLearningInputV1::empty()
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
        let platform = StorePlatform::open(config).unwrap();
        let memory = MemoryRuntime::builder()
            .identity(MemoryIdentity::new("atomic-agent", "atomic-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "atomic-promotion-chat").unwrap())
            .store(crate::MemoryStoreHandle::from_platform(platform.clone()))
            .clock(Arc::new(Clock))
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
            .agent_tool_registry(registry())
            .build()
            .unwrap();
        let first = memory
            .finalize_turn(request(&memory, "first-source"))
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
        let second = memory
            .finalize_turn(request(&memory, "second-source"))
            .unwrap();
        let job = memory
            .claim_due_procedural_feedback_job(
                &second.procedural_learning.job_id.unwrap(),
                "atomic-worker",
                1_800_000_060,
            )
            .unwrap();
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
                &job,
                &record,
                record.learning_evidence.as_ref().unwrap(),
                1_800_000_000,
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
        let input = ProceduralFeedbackCompletionInput {
            job_id: job.job_id.clone(),
            lease_owner: "atomic-worker".into(),
            lease_epoch: job.lease_epoch,
            operation_id: format!("procedural-feedback-{}", job.job_id),
            actor_subject_id: memory.config.scoped_runtime.actor_subject_id.clone(),
            experience_mutations: planned.mutations,
            experience_preconditions: planned.preconditions,
            applied_owner_bindings: planned.owner_bindings,
            accepted_count: planned.accepted_count,
            deferred_count: planned.deferred_count,
            rejected_count: planned.rejected_count,
            changed_count: planned.changed_count,
            reason_digest: sha256_field_digest(&[b"synthetic-promotion-completion"]),
            completed_at: 1_800_000_000,
        };
        (platform, memory, input)
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

    fn fill_with_durable_observation_jobs(
        platform: &StorePlatform,
        memory: &MemoryRuntime,
        minimum_ledgers: usize,
        minimum_entries: usize,
    ) {
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
            input.learning.authority = ProceduralFeedbackAuthorityInputV1::ModelInferred;
            let job_id = memory
                .finalize_turn(input)
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
        assert_eq!(platform.export_store_snapshot().unwrap(), before,
            "source drift rejects Runtime/material/head/manifest/ledger/receipt/audit/event together");
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
            compile_nonproduction_runtime_budget, NonproductionRuntimeBudgetLimits,
            RuntimeBudgetInput,
        };
        let (platform, memory, input) =
            fixture(StoreBackendConfig::in_memory(ProfileId::native_dev_full().unwrap()).unwrap());
        complete(&platform, &memory, input).unwrap();
        fill_with_durable_observation_jobs(
            &platform,
            &memory,
            minimum_valid_store_entries() + 1,
            0,
        );
        let report = memory.runtime_budget();
        let positive = platform
            .read_procedural_application_ledgers_with_runtime_budget(&report)
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
            let result = platform.read_procedural_application_ledgers_with_runtime_budget(&limited);
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
        let config =
            StoreBackendConfig::file(&root, ProfileId::native_dev_full().unwrap()).unwrap();
        let (platform, memory, input) = fixture(config.clone());
        fill_with_durable_observation_jobs(&platform, &memory, 0, minimum_valid_store_entries());
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
        let error = complete_procedural_feedback_job(
            &limited,
            scope.clone(),
            &limited_budget,
            input.clone(),
        )
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
}

impl MemoryRuntime {
    pub(super) fn build_post_turn_learning_evidence(
        &self,
        request: &MemoryTurnFinalizeRequest,
    ) -> Result<PostTurnLearningEvidenceV1> {
        let learning = &request.learning;
        let has_feedback = learning.selection_receipt.is_some()
            || !learning.runtime_skill_feedback.is_empty()
            || !learning.agent_skill_feedback.is_empty()
            || !learning.task_learning_feedback.is_empty()
            || !learning.agent_tool_feedback.is_empty();
        if (has_feedback
            && !self
                .capabilities
                .procedural_learning
                .finalize_feedback
                .visible)
            || (!learning.agent_tool_feedback.is_empty()
                && !self
                    .capabilities
                    .procedural_learning
                    .agent_tool_learning
                    .visible)
        {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable,
            ));
        }
        let conversation_id = request
            .turn
            .conversation
            .conversation_id
            .as_deref()
            .unwrap_or(request.turn.conversation.chat_id.as_str())
            .to_string();
        use crate::ProceduralLearningErrorKeyV1 as Key;
        let input = &request.learning;
        if request.turn.subject != self.config.scoped_runtime.mounted_subject_id
            || request.turn.conversation.channel != self.config.scope.channel
            || request.turn.conversation.chat_id != self.config.scope.chat_id
            || self
                .config
                .scope
                .conversation_id
                .as_ref()
                .is_some_and(|pinned| &conversation_id != pinned)
            || !self.procedural_subject_active()
        {
            return Err(feedback_error(Key::ScopeMismatch));
        }
        let authority = match &input.authority {
            bm_core::memory::ProceduralFeedbackAuthorityInputV1::HostRuntimeObservation => {
                ProceduralFeedbackAuthorityV1::HostRuntimeObservation
            }
            bm_core::memory::ProceduralFeedbackAuthorityInputV1::ModelInferred => {
                ProceduralFeedbackAuthorityV1::ModelInferred
            }
            bm_core::memory::ProceduralFeedbackAuthorityInputV1::HumanConfirmed {
                operation_id,
            } => {
                let actor_id = &self.config.scoped_runtime.actor_subject_id;
                let actor = self
                    .config
                    .subject_registry
                    .subject(actor_id)
                    .ok_or_else(|| feedback_error(Key::ConfirmationAuthorityInvalid))?;
                if actor.kind != bm_core::memory::SubjectKind::HumanUser
                    || actor.lifecycle_state != bm_core::memory::SubjectLifecycleState::Active
                    || operation_id.is_empty()
                    || operation_id.trim() != operation_id
                {
                    return Err(feedback_error(Key::ConfirmationAuthorityInvalid));
                }
                ProceduralFeedbackAuthorityV1::HumanConfirmed {
                    actor_subject_id: actor_id.clone(),
                    operation_id: operation_id.clone(),
                    evidence_digest: String::new(),
                }
            }
        };
        let mut evidence = PostTurnLearningEvidenceV1 {
            schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
            memory_space_id: self.config.memory_space_id.clone(),
            mounted_subject_id: self.config.scoped_runtime.mounted_subject_id.clone(),
            conversation_id,
            turn_id: request.turn.turn_id.clone(),
            canonical_turn_digest: canonical_turn_learning_digest(&request.turn)?,
            tool_call_count: input.tool_call_count,
            selection_receipt: input.selection_receipt.clone(),
            runtime_skill_feedback: input.runtime_skill_feedback.clone(),
            agent_skill_feedback: input.agent_skill_feedback.clone(),
            task_learning_feedback: input.task_learning_feedback.clone(),
            agent_tool_feedback: input.agent_tool_feedback.clone(),
            authority,
            learning_evidence_digest: String::new(),
        };
        let confirmation_digest = evidence.canonical_confirmation_payload_digest()?;
        if let ProceduralFeedbackAuthorityV1::HumanConfirmed {
            evidence_digest, ..
        } = &mut evidence.authority
        {
            *evidence_digest = confirmation_digest;
        }
        evidence.learning_evidence_digest = evidence.canonical_digest()?;
        if !evidence.validate_contract() {
            return Err(feedback_error(Key::ReceiptInvalid));
        }
        let store = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        if let Some(receipt) = &evidence.selection_receipt {
            store
                .verify_procedural_selection_receipt(receipt)
                .map_err(|_| feedback_error(Key::ReceiptInvalid))?;
            if receipt.identity.memory_space_id != evidence.memory_space_id
                || receipt.identity.mounted_subject_id != evidence.mounted_subject_id
                || receipt.identity.channel != request.turn.conversation.channel
                || receipt.identity.chat_id != request.turn.conversation.chat_id
                || receipt.identity.conversation_id != evidence.conversation_id
                || receipt.identity.turn_id != evidence.turn_id
                || receipt.applicability
                    != self.procedural_applicability_for_conversation(&evidence.conversation_id)?
            {
                return Err(feedback_error(Key::ScopeMismatch));
            }
        }
        let key = ConversationKey::from_delta(&evidence.memory_space_id, &request.turn)?;
        if let Some(existing) = self
            .config
            .platform
            .conversation_transcript_store()
            .get_turn(&key, &evidence.mounted_subject_id, &evidence.turn_id)?
        {
            if existing.learning_evidence.as_ref() != Some(&evidence) {
                return Err(feedback_error(Key::EvidenceConflict));
            }
            if !existing.permits_post_turn_learning() && evidence.selection_receipt.is_some() {
                return Err(feedback_error(Key::EvidenceConflict));
            }
            return Ok(evidence);
        }
        if let Some(receipt) = &evidence.selection_receipt {
            let now = self.config.clock.now_secs();
            if receipt.issued_at > now
                || now >= receipt.expires_at
                || receipt.expires_at.saturating_sub(receipt.issued_at) != 86_400
            {
                return Err(feedback_error(Key::ReceiptInvalid));
            }
        }
        self.validate_feedback_sources(&evidence)?;
        let mut observation_ids = BTreeSet::new();
        for feedback in &evidence.agent_tool_feedback {
            self.normalize_agent_tool_learning_feedback(feedback, &evidence.conversation_id)
                .map_err(|_| feedback_error(Key::EvidenceConflict))?;
            for observation in &feedback.observations {
                if !observation_ids.insert(observation.observation_id.clone())
                    || !request.turn.tool_observations.iter().any(|canonical| {
                        canonical.observation_id == observation.observation_id
                            && canonical.tool_name == observation.tool_id
                            && canonical.summary == observation.summary
                            && canonical.external_content == observation.external_content
                    })
                {
                    return Err(feedback_error(Key::EvidenceConflict));
                }
            }
        }
        if input.tool_call_count as usize != request.turn.tool_observations.len()
            || observation_ids.len() > input.tool_call_count as usize
        {
            return Err(feedback_error(Key::EvidenceConflict));
        }
        Ok(evidence)
    }

    fn normalize_agent_tool_learning_feedback(
        &self,
        feedback: &AgentToolUsageFeedbackV2,
        conversation_id: &str,
    ) -> Result<AgentToolUsageFeedbackV2> {
        let registry = self
            .agent_tool_registries()
            .into_iter()
            .find(|registry| registry.registry_ref() == feedback.registry_ref)
            .ok_or_else(|| {
                Error::config(
                    "post_turn_learning_evidence",
                    "agent tool feedback registry is unknown or stale",
                )
            })?;
        if !self
            .procedural_applicability_for_conversation(conversation_id)?
            .permits_registry_scope(&registry.scope)
        {
            return Err(Error::config(
                "post_turn_learning_evidence",
                "agent tool feedback registry scope is not applicable to this turn",
            ));
        }
        let first = feedback.observations.first().ok_or_else(|| {
            Error::config(
                "post_turn_learning_evidence",
                "agent tool feedback requires at least one runtime observation",
            )
        })?;
        if feedback.observations.iter().any(|observation| {
            observation.registry_id != registry.registry_id
                || observation.tool_id != first.tool_id
                || observation.schema_fingerprint != first.schema_fingerprint
        }) {
            return Err(Error::config(
                "post_turn_learning_evidence",
                "agent tool observations must bind one exact registry tool schema",
            ));
        }
        if !registry.tools.iter().any(|tool| {
            !tool.disabled
                && tool.tool_id == first.tool_id
                && tool.schema_fingerprint == first.schema_fingerprint
        }) {
            return Err(Error::config(
                "post_turn_learning_evidence",
                "agent tool feedback references an unknown, disabled, or stale tool schema",
            ));
        }
        let observations = feedback.observations.clone();
        let has_succeeded = observations
            .iter()
            .any(|value| value.outcome == AgentToolOutcome::Succeeded);
        let all_succeeded = observations
            .iter()
            .all(|value| value.outcome == AgentToolOutcome::Succeeded);
        let all_cancelled = observations
            .iter()
            .all(|value| value.outcome == AgentToolOutcome::Cancelled);
        let has_partial = observations
            .iter()
            .any(|value| value.outcome == AgentToolOutcome::Partial);
        let outcome = if all_succeeded {
            ProceduralExecutionOutcomeV1::Succeeded
        } else if all_cancelled {
            ProceduralExecutionOutcomeV1::Cancelled
        } else if has_succeeded || has_partial {
            ProceduralExecutionOutcomeV1::Partial
        } else {
            ProceduralExecutionOutcomeV1::Failed
        };
        let normalized = AgentToolUsageFeedbackV2 {
            registry_ref: feedback.registry_ref.clone(),
            tool_id: first.tool_id.clone(),
            schema_fingerprint: first.schema_fingerprint.clone(),
            observations,
            outcome,
            user_visible_result_summary: feedback.user_visible_result_summary.clone(),
            operator_note: feedback.operator_note.clone(),
        };
        if !normalized.validate_contract() || normalized != *feedback {
            return Err(Error::config(
                "post_turn_learning_evidence",
                "agent tool feedback is non-canonical",
            ));
        }
        Ok(normalized)
    }

    fn validate_feedback_sources(&self, evidence: &PostTurnLearningEvidenceV1) -> Result<()> {
        use crate::ProceduralLearningErrorKeyV1 as Key;
        for feedback in &evidence.agent_tool_feedback {
            self.normalize_agent_tool_learning_feedback(feedback, &evidence.conversation_id)
                .map_err(|_| feedback_error(Key::EvidenceConflict))?;
        }
        let applicability =
            self.procedural_applicability_for_conversation(&evidence.conversation_id)?;
        let expected_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: evidence.mounted_subject_id.clone(),
        };
        for feedback in &evidence.runtime_skill_feedback {
            if feedback.locator.owning_scope() != &expected_scope
                && feedback.locator.owning_scope() != &RuntimeSkillOwningScope::SharedProgram
            {
                return Err(feedback_error(Key::ScopeMismatch));
            }
            let snapshot = self.read_runtime_skill_locator_snapshot(
                &feedback.locator,
                "procedural_runtime_feedback",
            )?;
            let record = runtime_skill_record_for_locator(
                &snapshot,
                &feedback.locator,
                "procedural_runtime_feedback",
            )
            .map_err(|_| feedback_error(Key::EvidenceConflict))?;
            if record.content_digest != feedback.selected_content_digest
                || record.lifecycle.availability
                    != bm_core::skills::RuntimeSkillAvailability::Enabled
                || matches!(
                    record.lifecycle.state,
                    bm_core::skills::RuntimeSkillLifecycleState::Retired
                        | bm_core::skills::RuntimeSkillLifecycleState::Superseded
                )
            {
                return Err(feedback_error(Key::EvidenceConflict));
            }
        }
        for feedback in &evidence.agent_skill_feedback {
            let matches = self
                .config
                .agent_skill_registry
                .packages
                .iter()
                .filter(|package| {
                    package.status == bm_core::skills::AgentSkillPackageStatus::Active
                        && package.id == feedback.package_id
                        && package.fingerprint == feedback.package_fingerprint
                        && package.package_binding() == feedback.package_binding
                        && applicability.permits_agent_skill_scope(&package.scope)
                })
                .count();
            if matches != 1 {
                return Err(feedback_error(Key::EvidenceConflict));
            }
        }
        for feedback in &evidence.task_learning_feedback {
            let record = self
                .config
                .platform
                .task_learning_store()
                .get(&feedback.learning_id)?
                .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
            if !record
                .permits_usage_feedback(&self.config.scope.channel, &self.config.scope.chat_id)
                || record.canonical_content_digest()? != feedback.learning_digest
            {
                return Err(feedback_error(Key::EvidenceConflict));
            }
        }
        let store = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        if let Some(receipt) = &evidence.selection_receipt {
            let scope = AgentToolExperienceOwningScopeV1::Subject {
                mounted_subject_id: evidence.mounted_subject_id.clone(),
            };
            for selected in &receipt.agent_tool_experiences {
                if !evidence.agent_tool_feedback.iter().any(|feedback| {
                    feedback.registry_ref == selected.registry_ref
                        && feedback.tool_id == selected.tool_id
                        && feedback.schema_fingerprint == selected.schema_fingerprint
                }) {
                    continue;
                }
                let owner = GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::AgentToolExperience,
                    &selected.experience_owner_id,
                );
                let key =
                    agent_tool_experience_head_key(&evidence.memory_space_id, &scope, &owner)?;
                let head = store
                    .read_json_docs_by_keys(AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, &[key])?
                    .pop()
                    .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
                let head: AgentToolExperienceOwnerHeadV2 = serde_json::from_value(head.value)
                    .map_err(|_| feedback_error(Key::RepairRequired))?;
                if head.state == bm_core::skills::AgentToolExperienceHeadStateV2::Tombstoned
                    || head.current_revision != selected.experience_revision
                    || head.retained_revisions.last().is_none_or(|revision| {
                        revision.content_digest != selected.experience_content_digest
                    })
                {
                    return Err(feedback_error(Key::EvidenceConflict));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn run_claimed_procedural_feedback_job(
        &self,
        job: &ProceduralFeedbackJobV1,
        lease_owner: &str,
    ) -> Result<ProceduralFeedbackCompletionOutcome> {
        if !self.capabilities.procedural_learning.worker.visible {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable,
            ));
        }
        if !self.procedural_subject_active()
            || job.identity.memory_space_id != self.config.memory_space_id
            || job.identity.mounted_subject_id != self.config.scoped_runtime.mounted_subject_id
        {
            return Err(Error::conflict(
                "procedural_feedback_evidence",
                "learning authority does not match active subject",
            ));
        }
        let now_secs = self.config.clock.now_secs();
        let key = ConversationKey::new(
            &job.identity.memory_space_id,
            &job.identity.channel_id,
            &job.identity.conversation_id,
        )?;
        let record = self
            .config
            .platform
            .conversation_transcript_store()
            .get_turn(
                &key,
                &job.identity.mounted_subject_id,
                &job.identity.turn_id,
            )?
            .ok_or_else(|| {
                Error::config(
                    "procedural_feedback_evidence",
                    "canonical transcript root is unavailable",
                )
            })?;
        if !record.permits_post_turn_learning() {
            return Err(Error::conflict(
                "procedural_feedback_evidence",
                "canonical source no longer permits learning",
            ));
        }
        let evidence = record.learning_evidence.as_ref().ok_or_else(|| {
            Error::config(
                "procedural_feedback_evidence",
                "canonical transcript learning evidence is unavailable",
            )
        })?;
        if !evidence.agent_tool_feedback.is_empty()
            && !self
                .capabilities
                .procedural_learning
                .agent_tool_learning
                .visible
        {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable,
            ));
        }
        if record.sequence != job.transcript_sequence
            || post_turn_governance_transcript_digest(&record)? != job.transcript_digest
            || evidence.learning_evidence_digest != job.learning_evidence_digest
            || evidence.memory_space_id != job.identity.memory_space_id
            || evidence.mounted_subject_id != job.identity.mounted_subject_id
            || evidence.conversation_id != job.identity.conversation_id
            || evidence.turn_id != job.identity.turn_id
        {
            return Err(Error::config(
                "procedural_feedback_evidence",
                "procedural job and canonical transcript evidence diverged",
            ));
        }
        if !evidence.validate_contract() {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
            ));
        }
        if let Some(receipt) = &evidence.selection_receipt {
            self.config
                .store_platform
                .as_ref()
                .ok_or_else(|| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired))?
                .verify_procedural_selection_receipt(receipt)
                .map_err(|_| feedback_error(crate::ProceduralLearningErrorKeyV1::ReceiptInvalid))?;
        }
        let mut planned =
            match self.plan_agent_tool_feedback_application(job, &record, evidence, now_secs) {
                Ok(plan) => plan,
                Err(error) => {
                    let rejection = match &error {
                        Error::Other { source, .. } => source
                            .downcast_ref::<crate::ProceduralLearningSdkError>()
                            .filter(|typed| {
                                matches!(
                                    typed.key,
                                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict
                                        | crate::ProceduralLearningErrorKeyV1::ScopeMismatch
                                )
                            })
                            .map(|typed| typed.key),
                        _ => None,
                    };
                    let Some(key) = rejection else {
                        return Err(error);
                    };
                    ProceduralFeedbackApplicationPlan {
                        mutations: Vec::new(),
                        preconditions: Vec::new(),
                        owner_bindings: Vec::new(),
                        accepted_count: 0,
                        deferred_count: 0,
                        rejected_count: job.submitted_count,
                        changed_count: 0,
                        rejection_cause: Some(key),
                    }
                }
            };
        planned.preconditions.push(StoreJsonPrecondition::Exact {
            namespace: "conversation_transcript".to_string(),
            key: crate::store_internal::transcript_turn_storage_key(
                &key,
                &job.identity.mounted_subject_id,
                &job.identity.turn_id,
            ),
            value: serde_json::to_value(&record).map_err(|_| {
                Error::config(
                    "procedural_feedback_evidence",
                    "canonical transcript cannot be encoded",
                )
            })?,
        });
        let store = self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config(
                "procedural_feedback_complete",
                "durable procedural completion requires StorePlatform",
            )
        })?;
        complete_procedural_feedback_job(
            store,
            self.memory_write_transaction_scope(),
            &self.runtime_budget(),
            ProceduralFeedbackCompletionInput {
                job_id: job.job_id.clone(),
                lease_owner: lease_owner.to_string(),
                lease_epoch: job.lease_epoch,
                operation_id: format!("procedural-feedback-{}", job.job_id),
                actor_subject_id: self.config.scoped_runtime.actor_subject_id.clone(),
                experience_mutations: planned.mutations,
                experience_preconditions: planned.preconditions,
                applied_owner_bindings: planned.owner_bindings,
                accepted_count: planned.accepted_count,
                deferred_count: planned.deferred_count,
                rejected_count: planned.rejected_count,
                changed_count: planned.changed_count,
                reason_digest: sha256_field_digest(&[
                    b"procedural_feedback_governance_v1",
                    &planned.accepted_count.to_be_bytes(),
                    &planned.deferred_count.to_be_bytes(),
                    &planned.rejected_count.to_be_bytes(),
                    &serde_json::to_vec(&planned.rejection_cause).map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?,
                ]),
                completed_at: now_secs,
            },
        )
    }

    fn plan_agent_tool_feedback_application(
        &self,
        job: &ProceduralFeedbackJobV1,
        source_record: &bm_core::memory::TranscriptTurnRecord,
        evidence: &PostTurnLearningEvidenceV1,
        now_secs: u64,
    ) -> Result<ProceduralFeedbackApplicationPlan> {
        let runtime_budget = self.runtime_budget();
        let limits = &runtime_budget.governed_state_budget;
        let owning_scope = AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: job.identity.mounted_subject_id.clone(),
        };
        let store = self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config(
                "procedural_feedback_planning",
                "procedural feedback planning requires StorePlatform",
            )
        })?;
        let manifest_key =
            agent_tool_experience_scope_manifest_key(&job.identity.memory_space_id, &owning_scope)?;
        let mut manifest = store
            .read_json_docs_by_keys(
                AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
                &[manifest_key],
            )?
            .pop()
            .map(|doc| {
                serde_json::from_value::<AgentToolExperienceScopeManifestV1>(doc.value).map_err(
                    |error| Error::config("procedural_feedback_planning", error.to_string()),
                )
            })
            .transpose()?;
        let registries = self.agent_tool_registries();
        self.validate_feedback_sources(evidence)?;
        if let ProceduralFeedbackAuthorityV1::HumanConfirmed {
            actor_subject_id, ..
        } = &evidence.authority
        {
            let valid = self
                .config
                .subject_registry
                .subject(actor_subject_id)
                .is_some_and(|actor| {
                    actor.kind == bm_core::memory::SubjectKind::HumanUser
                        && actor.lifecycle_state == bm_core::memory::SubjectLifecycleState::Active
                });
            if !valid {
                return Ok(ProceduralFeedbackApplicationPlan {
                    mutations: Vec::new(),
                    preconditions: Vec::new(),
                    owner_bindings: Vec::new(),
                    accepted_count: 0,
                    deferred_count: 0,
                    rejected_count: job.submitted_count,
                    changed_count: 0,
                    rejection_cause: Some(
                        crate::ProceduralLearningErrorKeyV1::ConfirmationAuthorityInvalid,
                    ),
                });
            }
        }
        if evidence.authority == ProceduralFeedbackAuthorityV1::ModelInferred {
            return Ok(ProceduralFeedbackApplicationPlan {
                mutations: Vec::new(),
                preconditions: Vec::new(),
                owner_bindings: Vec::new(),
                accepted_count: job.submitted_count,
                deferred_count: 0,
                rejected_count: 0,
                changed_count: 0,
                rejection_cause: None,
            });
        }
        let mut accepted_count = u32::try_from(
            evidence
                .runtime_skill_feedback
                .len()
                .saturating_add(evidence.agent_skill_feedback.len())
                .saturating_add(evidence.task_learning_feedback.len()),
        )
        .map_err(|_| feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict))?;
        let mut deferred_count = 0_u32;
        let mut rejected_count = 0_u32;
        let mut accepted_observations = BTreeMap::<
            String,
            (
                AgentToolUsageFeedbackV2,
                String,
                Vec<bm_core::skills::AgentToolObservationDigest>,
            ),
        >::new();
        for feedback in &evidence.agent_tool_feedback {
            let registry = registries
                .iter()
                .find(|registry| registry.registry_ref() == feedback.registry_ref)
                .ok_or_else(|| {
                    Error::config(
                        "procedural_feedback_registry_unavailable",
                        "exact Agent Tool registry is not currently mounted",
                    )
                })?;
            if feedback
                .observations
                .iter()
                .any(|observation| observation.private_content_used)
            {
                rejected_count = rejected_count.saturating_add(1);
                continue;
            }
            if !self
                .procedural_applicability_for_conversation(&job.identity.conversation_id)?
                .permits_registry_scope(&registry.scope)
            {
                rejected_count = rejected_count.saturating_add(1);
                continue;
            }
            let succeeded = feedback
                .observations
                .iter()
                .filter(|observation| observation.outcome == AgentToolOutcome::Succeeded)
                .count();
            if succeeded == 0 {
                rejected_count = rejected_count.saturating_add(1);
                continue;
            }
            if succeeded < 2 && !evidence.authority.is_human_confirmation() {
                deferred_count = deferred_count.saturating_add(1);
                continue;
            }
            let mut by_task = BTreeMap::<String, Vec<_>>::new();
            for observation in &feedback.observations {
                by_task
                    .entry(observation.task_signature.clone())
                    .or_default()
                    .push(observation.clone());
            }
            if !evidence.authority.is_human_confirmation()
                && by_task.values().any(|observations| {
                    observations
                        .iter()
                        .filter(|observation| observation.outcome == AgentToolOutcome::Succeeded)
                        .count()
                        < 2
                })
            {
                deferred_count = deferred_count.saturating_add(1);
                continue;
            }
            let mut group = Vec::new();
            let mut terminated = false;
            for (task_signature, observations) in by_task {
                let owner_id = canonical_agent_tool_experience_owner_id(
                    &job.identity.memory_space_id,
                    &owning_scope,
                    &registry.registry_id,
                    &registry.scope,
                    &feedback.tool_id,
                    &feedback.schema_fingerprint,
                    &task_signature,
                )?;
                let owner_ref = GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::AgentToolExperience,
                    &owner_id,
                );
                let key = agent_tool_experience_head_key(
                    &job.identity.memory_space_id,
                    &owning_scope,
                    &owner_ref,
                )?;
                if let Some(doc) = store
                    .read_json_docs_by_keys(AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, &[key])?
                    .pop()
                {
                    let head: AgentToolExperienceOwnerHeadV2 = serde_json::from_value(doc.value)
                        .map_err(|_| {
                            Error::config("procedural_feedback_planning", "invalid experience head")
                        })?;
                    terminated |=
                        head.state == bm_core::skills::AgentToolExperienceHeadStateV2::Tombstoned;
                }
                group.push((owner_id, task_signature, observations));
            }
            if terminated {
                rejected_count = rejected_count.saturating_add(1);
                continue;
            }
            accepted_count = accepted_count.saturating_add(1);
            for (owner_id, task_signature, observations) in group {
                accepted_observations
                    .entry(owner_id)
                    .and_modify(|(_, _, existing)| existing.extend(observations.clone()))
                    .or_insert((feedback.clone(), task_signature, observations));
            }
        }
        if accepted_count
            .saturating_add(deferred_count)
            .saturating_add(rejected_count)
            != job.submitted_count
        {
            return Err(Error::config(
                "procedural_feedback_planning",
                "procedural governance did not account for every submitted feedback group",
            ));
        }
        let mut plans = Vec::new();
        for (owner_id, (feedback, task_signature, mut observations)) in accepted_observations {
            observations.sort_by(|left, right| left.observation_id.cmp(&right.observation_id));
            observations.dedup_by(|left, right| left.observation_id == right.observation_id);
            let registry = registries
                .iter()
                .find(|registry| registry.registry_ref() == feedback.registry_ref)
                .expect("accepted registry was validated above");
            let owner_ref = GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::AgentToolExperience,
                owner_id,
            );
            let head_key = agent_tool_experience_head_key(
                &job.identity.memory_space_id,
                &owning_scope,
                &owner_ref,
            )?;
            let previous_head = store
                .read_json_docs_by_keys(AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, &[head_key])?
                .pop()
                .map(|doc| {
                    serde_json::from_value::<AgentToolExperienceOwnerHeadV2>(doc.value).map_err(
                        |error| Error::config("procedural_feedback_planning", error.to_string()),
                    )
                })
                .transpose()?;
            if previous_head.as_ref().is_some_and(|head| {
                head.state == bm_core::skills::AgentToolExperienceHeadStateV2::Tombstoned
            }) {
                return Err(Error::config(
                    "agent_tool_experience_tombstoned",
                    "terminated experience cannot be recreated by ordinary learning",
                ));
            }
            let previous_material = previous_head
                .as_ref()
                .and_then(|head| head.retained_revisions.last())
                .map(|revision| {
                    store
                        .read_json_docs_by_keys(
                            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                            std::slice::from_ref(&revision.material_key),
                        )?
                        .pop()
                        .ok_or_else(|| {
                            Error::config(
                                "procedural_feedback_planning",
                                "Agent Tool experience head is missing its current material",
                            )
                        })
                        .and_then(|doc| {
                            serde_json::from_value::<AgentToolExperienceRevisionMaterialV2>(
                                doc.value,
                            )
                            .map_err(|error| {
                                Error::config("procedural_feedback_planning", error.to_string())
                            })
                        })
                })
                .transpose()?;
            let revision = previous_head
                .as_ref()
                .map_or(1, |head| head.current_revision.saturating_add(1));
            if usize::try_from(revision).unwrap_or(usize::MAX)
                > limits.max_agent_tool_experience_revisions_per_owner
            {
                return Err(Error::config(
                    "procedural_feedback_planning",
                    "Agent Tool experience revision capacity is exhausted",
                ));
            }
            let current_evidence_count = u32::try_from(observations.len()).map_err(|_| {
                Error::config(
                    "procedural_feedback_planning",
                    "Agent Tool observation count exceeds durable contract",
                )
            })?;
            let current_success_count = u32::try_from(
                observations
                    .iter()
                    .filter(|observation| observation.outcome == AgentToolOutcome::Succeeded)
                    .count(),
            )
            .map_err(|_| {
                Error::config(
                    "procedural_feedback_planning",
                    "Agent Tool success count exceeds durable contract",
                )
            })?;
            let mut evidence_refs = previous_material
                .as_ref()
                .map(|material| material.evidence_refs.clone())
                .unwrap_or_default();
            evidence_refs.extend(
                observations
                    .iter()
                    .map(|observation| observation.observation_id.clone()),
            );
            evidence_refs.sort();
            evidence_refs.dedup();
            if evidence_refs.len() > limits.max_agent_tool_experience_evidence_refs_per_owner {
                return Err(Error::config(
                    "procedural_feedback_planning",
                    "Agent Tool experience evidence capacity is exhausted",
                ));
            }
            let evidence_count = previous_material
                .as_ref()
                .map_or(0, |material| material.evidence_count)
                .saturating_add(current_evidence_count);
            let success_count = previous_material
                .as_ref()
                .map_or(0, |material| material.success_count)
                .saturating_add(current_success_count);
            let failure_count = previous_material
                .as_ref()
                .map_or(0, |material| material.failure_count)
                .saturating_add(current_evidence_count.saturating_sub(current_success_count));
            let first = observations.first().ok_or_else(|| {
                Error::config(
                    "procedural_feedback_planning",
                    "accepted Agent Tool group has no observations",
                )
            })?;
            let last_outcome = observations
                .last()
                .map(|observation| observation.outcome)
                .unwrap_or(AgentToolOutcome::Failed);
            let trigger_summary = feedback
                .user_visible_result_summary
                .as_deref()
                .unwrap_or(first.summary.as_str());
            let mut constraints = observations
                .iter()
                .flat_map(|observation| {
                    observation
                        .permission_tags
                        .iter()
                        .chain(observation.risk_tags.iter())
                        .cloned()
                })
                .collect::<Vec<_>>();
            constraints.sort();
            constraints.dedup();
            let predecessor = previous_material
                .as_ref()
                .map(AgentToolExperienceRetainedRevisionDigestV2::from_material)
                .transpose()?;
            let material = AgentToolExperienceRevisionMaterialV2::build(
                &job.identity.memory_space_id,
                owning_scope.clone(),
                &registry.registry_id,
                registry.scope.clone(),
                &feedback.tool_id,
                &feedback.schema_fingerprint,
                &task_signature,
                revision,
                &truncate_to_char_count(trigger_summary, 512),
                &truncate_to_char_count(&first.summary, 512),
                constraints,
                evidence_count,
                success_count,
                failure_count,
                last_outcome,
                if success_count >= 3 {
                    AgentToolExperienceConfidence::High
                } else {
                    AgentToolExperienceConfidence::Medium
                },
                AgentToolExperienceStatus::Active,
                evidence_refs,
                MemoryPrivacyClass::SharedWithSubject,
                previous_material
                    .as_ref()
                    .map_or(now_secs, |material| material.created_at),
                now_secs,
                predecessor,
            )?;
            let mut retained = previous_head
                .as_ref()
                .map(|head| head.retained_revisions.clone())
                .unwrap_or_default();
            retained.push(AgentToolExperienceRetainedRevisionDigestV2::from_material(
                &material,
            )?);
            let head = AgentToolExperienceOwnerHeadV2::build(
                &job.identity.memory_space_id,
                owning_scope.clone(),
                material.owner_ref.clone(),
                revision,
                retained,
            )?;
            let binding =
                AgentToolExperienceHeadBindingV1::from_head_and_material(&head, &material)?;
            let mut bindings = manifest
                .as_ref()
                .map(|manifest| manifest.bindings.clone())
                .unwrap_or_default();
            bindings.retain(|candidate| candidate.owner_ref != head.owner_ref);
            bindings.push(binding);
            let next_manifest = AgentToolExperienceScopeManifestV1::build(
                manifest
                    .as_ref()
                    .map_or(1, |manifest| manifest.revision.saturating_add(1)),
                &job.identity.memory_space_id,
                owning_scope.clone(),
                bindings,
                limits.max_agent_tool_experience_owners_per_subject,
            )?;
            plans.push(AgentToolExperienceStoreMutationPlanV1::try_new(
                format!("procedural-feedback-{}", job.job_id),
                self.config.scoped_runtime.actor_subject_id.clone(),
                self.memory_write_transaction_scope(),
                previous_head,
                manifest.clone(),
                material,
                head,
                next_manifest.clone(),
                now_secs,
            )?);
            manifest = Some(next_manifest);
        }
        let (mut mutations, mut preconditions, owner_revisions) =
            combine_agent_tool_experience_plans(
                &plans,
                limits.max_agent_tool_experience_owners_per_subject,
            )?;
        let mut owner_bindings = owner_revisions
            .into_iter()
            .map(|revision| {
                let content_digest = mutations
                    .iter()
                    .find_map(|mutation| match mutation {
                        StoreMutation::PutJson {
                            namespace, value, ..
                        } if namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
                            serde_json::from_value::<AgentToolExperienceRevisionMaterialV2>(
                                value.clone(),
                            )
                            .ok()
                            .filter(|material| {
                                material.owner_ref == revision.owner_ref
                                    && material.owner_revision == revision.owner_revision
                            })
                            .map(|material| material.content_digest)
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                Ok(
                    bm_core::memory::ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                        owner_revision: revision,
                        content_digest,
                    },
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut runtime_records = self.plan_runtime_promotions_from_accepted_experiences(
            job,
            source_record,
            &mutations,
            &mut preconditions,
            now_secs,
        )?;
        for record in &runtime_records {
            owner_bindings.push(
                bm_core::memory::ProceduralAppliedOwnerBindingV1::RuntimeSkill {
                    binding: bm_core::skills::RuntimeSkillOwnerBinding::from_record(record)?,
                },
            );
        }
        let mut runtime_feedback = BTreeMap::<String, Vec<_>>::new();
        for feedback in &evidence.runtime_skill_feedback {
            runtime_feedback
                .entry(feedback.locator.owner_id().to_owned())
                .or_default()
                .push(feedback);
        }
        for feedbacks in runtime_feedback.into_values() {
            let first = feedbacks[0];
            let snapshot = self.read_runtime_skill_locator_snapshot(
                &first.locator,
                "procedural_runtime_feedback",
            )?;
            let current = runtime_skill_record_for_locator(
                &snapshot,
                &first.locator,
                "procedural_runtime_feedback",
            )?;
            if feedbacks
                .iter()
                .any(|feedback| feedback.selected_content_digest != current.content_digest)
            {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                ));
            }
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_owned(),
                key: current.physical_key.clone(),
                value: serde_json::to_value(current).map_err(|_| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?,
            });
            if first.locator.owning_scope() == &RuntimeSkillOwningScope::SharedProgram {
                continue;
            }
            let outcomes = feedbacks
                .iter()
                .map(|feedback| feedback.outcome)
                .collect::<Vec<_>>();
            let next = current.apply_usage_feedback(&outcomes, now_secs)?;
            if next != *current {
                owner_bindings.push(
                    bm_core::memory::ProceduralAppliedOwnerBindingV1::RuntimeSkill {
                        binding: bm_core::skills::RuntimeSkillOwnerBinding::from_record(&next)?,
                    },
                );
                runtime_records.push(next);
            }
        }
        if !runtime_records.is_empty() {
            let scope = RuntimeSkillOwningScope::Subject {
                mounted_subject_id: evidence.mounted_subject_id.clone(),
            };
            let plan = self.plan_runtime_skill_owner_records(&scope, runtime_records)?;
            if !plan.blob_preconditions.is_empty() {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::RepairRequired,
                ));
            }
            mutations.extend(plan.mutations);
            preconditions.extend(plan.preconditions);
        }
        for feedback in &evidence.task_learning_feedback {
            let current = self
                .config
                .platform
                .task_learning_store()
                .get(&feedback.learning_id)?
                .ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict)
                })?;
            if !current
                .permits_usage_feedback(&self.config.scope.channel, &self.config.scope.chat_id)
                || current.canonical_content_digest()? != feedback.learning_digest
            {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                ));
            }
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: "task_learning".to_owned(),
                key: current.learning_id.clone(),
                value: serde_json::to_value(&current).map_err(|_| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?,
            });
        }
        let changed_count = u32::try_from(owner_bindings.len()).map_err(|_| {
            Error::config(
                "procedural_feedback_planning",
                "changed owner count overflow",
            )
        })?;
        // Selection freshness and the typed owner planner can depend on the
        // same source. Preserve one exact CAS per address; divergent snapshots
        // are evidence conflicts, never last-writer-wins conditions.
        let mut normalized_preconditions = Vec::new();
        merge_json_preconditions(&mut normalized_preconditions, preconditions)
            .map_err(|_| feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict))?;
        Ok(ProceduralFeedbackApplicationPlan {
            mutations,
            preconditions: normalized_preconditions,
            owner_bindings,
            accepted_count,
            deferred_count,
            rejected_count,
            changed_count,
            rejection_cause: None,
        })
    }

    fn plan_runtime_promotions_from_accepted_experiences(
        &self,
        job: &ProceduralFeedbackJobV1,
        source_record: &bm_core::memory::TranscriptTurnRecord,
        mutations: &[StoreMutation],
        preconditions: &mut Vec<StoreJsonPrecondition>,
        observed_at: u64,
    ) -> Result<Vec<RuntimeSkillOwnerRecord>> {
        use crate::store_internal::schema::{
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE, PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
        };
        use bm_core::memory::{
            ProceduralAppliedOwnerBindingV1, ProceduralFeedbackApplicationLedgerV1,
        };
        use bm_core::skills::{
            plan_runtime_skill_promotion_from_tool_evidence, RuntimeSkillToolPromotionDecision,
            RuntimeSkillToolPromotionInput, RuntimeSkillToolPromotionSource,
        };
        let materials = mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreMutation::PutJson {
                    namespace, value, ..
                } if namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => Some(value),
                _ => None,
            })
            .map(|value| {
                serde_json::from_value::<AgentToolExperienceRevisionMaterialV2>(value.clone())
                    .map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        if materials.is_empty() {
            return Ok(Vec::new());
        }
        let store =
            self.config.store_platform.as_ref().ok_or_else(|| {
                feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
            })?;
        let runtime_budget = self.runtime_budget();
        let limits = &runtime_budget.governed_state_budget;
        let scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: job.identity.mounted_subject_id.clone(),
        };
        let runtime_snapshot = read_runtime_skill_scope_snapshot(
            store,
            &job.identity.memory_space_id,
            &scope,
            limits.max_retained_runtime_skill_owners_per_scope,
        )?;
        preconditions.push(json_precondition(
            crate::store_internal::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            &runtime_snapshot.manifest_key,
            runtime_snapshot.manifest_value.clone(),
        ));
        // The durable application ledger is the historical source authority. The
        // bounded scheduling index deliberately drops old completed jobs.
        let mut ledgers = None;
        let registries = self.agent_tool_registries();
        let mut promoted = Vec::new();
        for material in materials {
            let creation_ref = RuntimeSkillCreationRef::AgentToolExperiencePromotion {
                experience_owner_ref: material.owner_ref.clone(),
            };
            let existing = runtime_snapshot
                .records
                .iter()
                .find(|record| record.creation_ref == creation_ref);
            if let Some(existing) = existing {
                preconditions.push(StoreJsonPrecondition::Exact {
                    namespace: crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.into(),
                    key: existing.physical_key.clone(),
                    value: serde_json::to_value(existing).map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?,
                });
            }
            let registry = registries
                .iter()
                .find(|registry| {
                    registry.registry_id == material.registry_id
                        && registry.scope == material.registry_scope
                })
                .ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict)
                })?;
            let mut sources = vec![(job.clone(), source_record.clone())];
            let mut source_preconditions = Vec::new();
            // An existing promotion, including a retired one, is never recreated.
            // Core owns that decision; historical raw evidence need not be reread.
            if existing.is_none() {
                if ledgers.is_none() {
                    ledgers = Some(
                        store.read_procedural_application_ledgers_with_runtime_budget(
                            &runtime_budget,
                        )?,
                    );
                }
                let head = mutations
                    .iter()
                    .find_map(|mutation| match mutation {
                        StoreMutation::PutJson {
                            namespace, value, ..
                        } if namespace == AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE => {
                            serde_json::from_value::<AgentToolExperienceOwnerHeadV2>(value.clone())
                                .ok()
                                .filter(|head| head.owner_ref == material.owner_ref)
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                let mut source_ids = BTreeSet::from([job.job_id.clone()]);
                for document in ledgers.as_ref().ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })? {
                    let ledger: ProceduralFeedbackApplicationLedgerV1 =
                        serde_json::from_value(document.value.clone()).map_err(|_| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    if ledger.identity.memory_space_id != job.identity.memory_space_id
                        || ledger.identity.mounted_subject_id != job.identity.mounted_subject_id
                    {
                        continue;
                    }
                    let applied =
                        ledger
                            .applied_owner_bindings
                            .iter()
                            .find_map(|binding| match binding {
                                ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                                    owner_revision,
                                    content_digest,
                                } if owner_revision.owner_ref == material.owner_ref => {
                                    Some((owner_revision, content_digest))
                                }
                                _ => None,
                            });
                    let Some((revision, digest)) = applied else {
                        continue;
                    };
                    ledger.validate()?;
                    let retained = head
                        .retained_revisions
                        .iter()
                        .find(|retained| {
                            retained.owner_revision == revision.owner_revision
                                && retained.owner_ref == revision.owner_ref
                                && retained.content_digest == *digest
                        })
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    if sources.len() >= limits.max_agent_tool_experience_evidence_refs_per_owner {
                        return Err(crate::store_internal::store_budget_error(
                            "promotion source evidence capacity is exhausted",
                        ));
                    }
                    let source_material_doc = store
                        .read_json_docs_by_keys(
                            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                            std::slice::from_ref(&retained.material_key),
                        )?
                        .pop()
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    let historical: AgentToolExperienceRevisionMaterialV2 =
                        serde_json::from_value(source_material_doc.value.clone()).map_err(
                            |_| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired),
                        )?;
                    if AgentToolExperienceRetainedRevisionDigestV2::from_material(&historical)?
                        != *retained
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    let source_job_doc = store
                        .read_json_docs_by_keys(
                            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                            std::slice::from_ref(&ledger.job_id),
                        )?
                        .pop()
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    let source_job: ProceduralFeedbackJobV1 =
                        serde_json::from_value(source_job_doc.value.clone()).map_err(|_| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    source_job.validate()?;
                    if source_job.status
                        != bm_core::memory::ProceduralFeedbackJobStatusV1::Succeeded
                        || source_job.identity != ledger.identity
                        || source_job.learning_evidence_digest != ledger.learning_evidence_digest
                        || source_job.job_id != ledger.job_id
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    let key = ConversationKey::new(
                        &ledger.identity.memory_space_id,
                        &ledger.identity.channel_id,
                        &ledger.identity.conversation_id,
                    )?;
                    let transcript = self
                        .config
                        .platform
                        .conversation_transcript_store()
                        .get_turn(
                            &key,
                            &ledger.identity.mounted_subject_id,
                            &ledger.identity.turn_id,
                        )?
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    if !transcript.permits_post_turn_learning() {
                        continue;
                    }
                    let source_evidence =
                        transcript.learning_evidence.as_ref().ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    if source_evidence.learning_evidence_digest != ledger.learning_evidence_digest
                        || !source_evidence.agent_tool_feedback.iter().any(|feedback| {
                            feedback.observations.iter().any(|observation| {
                                observation.task_signature == material.task_signature
                                    && observation.registry_id == material.registry_id
                                    && observation.tool_id == material.tool_id
                                    && observation.schema_fingerprint == material.schema_fingerprint
                                    && historical
                                        .evidence_refs
                                        .contains(&observation.observation_id)
                            })
                        })
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    if !source_ids.insert(source_job.job_id.clone()) {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    for (namespace, key, value) in [
                        (
                            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                            document.key.clone(),
                            document.value.clone(),
                        ),
                        (
                            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                            source_material_doc.key,
                            source_material_doc.value,
                        ),
                        (
                            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                            source_job_doc.key,
                            source_job_doc.value,
                        ),
                        (
                            "conversation_transcript",
                            crate::store_internal::transcript_turn_storage_key(
                                &key,
                                &ledger.identity.mounted_subject_id,
                                &ledger.identity.turn_id,
                            ),
                            serde_json::to_value(&transcript).map_err(|_| {
                                feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                            })?,
                        ),
                    ] {
                        source_preconditions.push(StoreJsonPrecondition::Exact {
                            namespace: namespace.into(),
                            key,
                            value,
                        });
                    }
                    sources.push((source_job, transcript));
                }
            }
            let sources = sources
                .iter()
                .map(|(job, transcript)| RuntimeSkillToolPromotionSource { job, transcript })
                .collect::<Vec<_>>();
            match plan_runtime_skill_promotion_from_tool_evidence(
                &RuntimeSkillToolPromotionInput {
                    experience: &material,
                    registry,
                    sources: &sources,
                    existing_owner: existing,
                    observed_at,
                },
            )? {
                RuntimeSkillToolPromotionDecision::Promote { record, .. } => {
                    preconditions.extend(source_preconditions);
                    promoted.push(*record);
                }
                RuntimeSkillToolPromotionDecision::Observe { .. }
                | RuntimeSkillToolPromotionDecision::Reject { .. }
                | RuntimeSkillToolPromotionDecision::AlreadyPromoted { .. } => {}
            }
        }
        Ok(promoted)
    }
}
