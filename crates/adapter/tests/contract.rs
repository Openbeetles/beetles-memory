use std::sync::Arc;

use bm_adapter::{
    decode_json_adapter_command, dispatch_adapter_command, dispatch_adapter_command_with_services,
    governed_adapter_json_command_schema, AdapterAuthContext, AdapterCommand, AdapterEnvelope,
    AdapterErrorKey, AdapterJsonCommandOptions, AdapterMutationReliability, AdapterOperation,
    AdapterProtocolBinding, AdapterRequestIdentityError, AdapterRequestIdentityOwner,
    AdapterResponse, AdapterRuntimeServices, AdapterSdkReport, AdapterSource, TransportKind,
    TransportMode,
};
use bm_sdk::{
    CanonicalTurnDelta, ConversationKey, ConversationScope, HostRefVisibility, LlmClient,
    LlmHttpClient, LlmModelCompat, LlmResponse, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryCapabilityPolicy, MemoryClock, MemoryCloseRequest, MemoryEvidenceAuthority,
    MemoryIdentity, MemoryLongTermControlView, MemoryLongTermListRequest, MemoryPrivacyClass,
    MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemorySemanticJudgmentSource, MemoryStoreHandle, MemorySubjectVisibilityPolicy,
    MemoryTranscriptAttrWriteRequest, MemoryTranscriptCommitRequest, MemoryTranscriptReplayRequest,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    MemoryWriteCandidate, MemoryWriteRequest, Message, NoopMemoryAuditSink, PressureLevel,
    ProfileId, QueryFacetInput, ResponseBody, RuntimeLifecycleModeInput, StopReason,
    StoreBackendConfig, ToolChoicePolicy, ToolSpec, TranscriptAttrEnvelope,
    TranscriptAttrGovernance, TranscriptAttrLink, TranscriptAttrRedactionPolicy,
    TranscriptAttrScope, TranscriptAttrSource, TranscriptAttrSourceKind, TranscriptAttrTarget,
    TranscriptAttrValueKind, TranscriptInputMessage, TranscriptReplayView,
};
use serde_json::json;

struct FixedClock;

#[test]
fn public_adapter_labels_are_stable_snake_case() {
    let operations = [
        (AdapterOperation::Write, "write"),
        (AdapterOperation::FinalizeTurn, "finalize_turn"),
        (AdapterOperation::Recall, "recall"),
        (AdapterOperation::Project, "project"),
        (AdapterOperation::Maintain, "maintain"),
        (AdapterOperation::Inspect, "inspect"),
        (AdapterOperation::Recover, "recover"),
        (AdapterOperation::Replay, "replay"),
        (AdapterOperation::LongTermList, "long_term_list"),
        (AdapterOperation::LongTermDetail, "long_term_detail"),
        (AdapterOperation::LongTermMutate, "long_term_mutate"),
        (AdapterOperation::LongTermPolicy, "long_term_policy"),
        (
            AdapterOperation::TranscriptAttrWrite,
            "transcript_attr_write",
        ),
        (AdapterOperation::Capabilities, "capabilities"),
        (AdapterOperation::Subscribe, "subscribe"),
        (AdapterOperation::Close, "close"),
    ];
    for (operation, label) in operations {
        assert_eq!(operation.as_str(), label);
        assert_eq!(operation.to_string(), label);
    }

    let errors = [
        (AdapterErrorKey::InvalidJson, "invalid_json"),
        (AdapterErrorKey::Unauthorized, "unauthorized"),
        (AdapterErrorKey::Forbidden, "forbidden"),
        (AdapterErrorKey::Duplicated, "duplicated"),
        (
            AdapterErrorKey::MutationOperationIdRequired,
            "mutation_operation_id_required",
        ),
        (
            AdapterErrorKey::MutationOperationConflict,
            "mutation_operation_conflict",
        ),
        (AdapterErrorKey::PayloadTooLarge, "payload_too_large"),
        (AdapterErrorKey::OperationMismatch, "operation_mismatch"),
        (
            AdapterErrorKey::RuntimeBindingMismatch,
            "runtime_binding_mismatch",
        ),
        (
            AdapterErrorKey::UnsupportedOperation,
            "unsupported_operation",
        ),
        (AdapterErrorKey::RuntimeRejected, "runtime_rejected"),
    ];
    for (error, label) in errors {
        assert_eq!(error.as_str(), label);
        assert_eq!(error.to_string(), label);
    }
}

#[test]
fn legacy_continuity_transfer_operations_are_not_deserializable() {
    for operation in ["export", "import"] {
        let encoded = format!("\"{operation}\"");
        let error = serde_json::from_str::<AdapterOperation>(&encoded)
            .expect_err("legacy continuity transfer operation must not remain public");
        assert!(error.to_string().contains("unknown variant"), "{error}");
    }
}

#[test]
fn idempotency_material_is_canonical_and_payload_sensitive() {
    let first = AdapterCommand::Close(MemoryCloseRequest {
        reason: "operator shutdown".to_string(),
    });
    let retry = AdapterCommand::Close(MemoryCloseRequest {
        reason: "operator shutdown".to_string(),
    });
    let different = AdapterCommand::Close(MemoryCloseRequest {
        reason: "policy shutdown".to_string(),
    });

    let first_material = first
        .idempotency_fingerprint_material()
        .expect("canonical first material");
    assert_eq!(
        first_material,
        retry
            .idempotency_fingerprint_material()
            .expect("canonical retry material")
    );
    assert_ne!(
        first_material,
        different
            .idempotency_fingerprint_material()
            .expect("canonical different material")
    );
}

#[test]
fn mutation_reliability_inventory_is_exhaustive_and_distinguishes_durable_receipts() {
    let operations = [
        (
            AdapterOperation::Write,
            AdapterMutationReliability::DurableStoreReceipt,
            true,
        ),
        (
            AdapterOperation::FinalizeTurn,
            AdapterMutationReliability::ExplicitlyNonDurable,
            true,
        ),
        (
            AdapterOperation::Recall,
            AdapterMutationReliability::NotMutation,
            false,
        ),
        (
            AdapterOperation::Project,
            AdapterMutationReliability::NotMutation,
            false,
        ),
        (
            AdapterOperation::Maintain,
            AdapterMutationReliability::ExplicitlyNonDurable,
            true,
        ),
        (
            AdapterOperation::Inspect,
            AdapterMutationReliability::NotMutation,
            false,
        ),
        (
            AdapterOperation::Recover,
            AdapterMutationReliability::ExplicitlyNonDurable,
            true,
        ),
        (
            AdapterOperation::Replay,
            AdapterMutationReliability::NotMutation,
            false,
        ),
        (
            AdapterOperation::LongTermList,
            AdapterMutationReliability::NotMutation,
            false,
        ),
        (
            AdapterOperation::LongTermDetail,
            AdapterMutationReliability::NotMutation,
            false,
        ),
        (
            AdapterOperation::LongTermMutate,
            AdapterMutationReliability::DurableStoreReceipt,
            true,
        ),
        (
            AdapterOperation::LongTermPolicy,
            AdapterMutationReliability::ExplicitlyNonDurable,
            true,
        ),
        (
            AdapterOperation::TranscriptAttrWrite,
            AdapterMutationReliability::ExplicitlyNonDurable,
            true,
        ),
        (
            AdapterOperation::Capabilities,
            AdapterMutationReliability::NotMutation,
            false,
        ),
        (
            AdapterOperation::Subscribe,
            AdapterMutationReliability::NotMutation,
            false,
        ),
        (
            AdapterOperation::Close,
            AdapterMutationReliability::ExplicitlyNonDurable,
            true,
        ),
    ];
    assert_eq!(operations.len(), AdapterOperation::ALL.len());
    for (operation, reliability, reserved_in_flight) in operations {
        assert_eq!(operation.mutation_reliability(), reliability, "{operation}");
        assert_eq!(
            operation.requires_in_flight_reservation(),
            reserved_in_flight,
            "{operation}"
        );
    }
    let runtime = runtime();
    let lease = runtime
        .acquire_runtime_budget_lease()
        .expect("runtime budget lease");
    let binding = AdapterProtocolBinding::for_runtime(&runtime, &lease);
    assert_eq!(
        binding.capabilities.mutation_operation_inventory.len(),
        AdapterOperation::ALL.len()
    );
    assert_eq!(
        binding.capabilities.mutation_receipt_policy.retention,
        bm_sdk::MutationOperationReceiptRetentionPolicy::PinnedUntilCapacity
    );
    assert!(
        !binding
            .capabilities
            .mutation_receipt_policy
            .automatic_eviction
    );
    assert_eq!(
        binding
            .capabilities
            .mutation_receipt_policy
            .capacity_exhaustion,
        bm_sdk::MutationOperationReceiptCapacityExhaustion::FailClosed
    );
    assert_eq!(
        binding
            .capabilities
            .mutation_receipt_policy
            .durable_json_entries_per_operation,
        2
    );
    assert_eq!(
        binding
            .capabilities
            .mutation_receipt_policy
            .durable_events_per_operation,
        2
    );
}

#[test]
fn adapter_write_time_is_runtime_owned_and_payload_time_is_rejected() {
    let options = AdapterJsonCommandOptions::new("accepted-at-contract");
    let error = decode_json_adapter_command(
        AdapterOperation::Write,
        r#"{"name":"runtime_skill__clock","topic":"clock","title":"Clock","summary":"Clock","content":"Clock","owning_scope":{"kind":"subject","mounted_subject_id":"agent:agent-main"},"creation_ref":{"kind":"replay_promotion","candidate_ref":"test:adapter-clock","verification_receipt_digest":"sha256:1111111111111111111111111111111111111111111111111111111111111111"},"privacy_class":"shared_with_subject","observed_at":1800000000}"#,
        &options,
    )
    .expect_err("payload observed_at must not be part of the adapter contract");
    assert_eq!(error.stage(), "adapter_json_command");
    assert!(error.to_string().contains("unknown field `observed_at`"));

    let mut command = decode_json_adapter_command(
        AdapterOperation::Write,
        r#"{"name":"runtime_skill__clock","topic":"clock","title":"Clock","summary":"Clock","content":"Clock","owning_scope":{"kind":"subject","mounted_subject_id":"agent:agent-main"},"creation_ref":{"kind":"replay_promotion","candidate_ref":"test:adapter-clock","verification_receipt_digest":"sha256:1111111111111111111111111111111111111111111111111111111111111111"},"privacy_class":"shared_with_subject"}"#,
        &options,
    )
    .expect("write command");
    command.pin_accepted_at(1_912_345_678);
    let AdapterCommand::Write(MemoryWriteRequest::Procedural { writes, .. }) = command else {
        panic!("procedural write");
    };
    assert_eq!(writes[0].write.observed_at, 1_912_345_678);
}

#[test]
fn long_term_mutation_and_policy_fingerprints_are_stable_and_field_sensitive() {
    let options = AdapterJsonCommandOptions::new("idempotency-contract");
    for (operation, body, different_body) in [
        (
            AdapterOperation::LongTermMutate,
            r#"{"operation":{"delete":{"target":{"record_id":"record-a"}}},"reason":"operator delete","dry_run":true}"#,
            r#"{"operation":{"delete":{"target":{"record_id":"record-b"}}},"reason":"operator delete","dry_run":true}"#,
        ),
        (
            AdapterOperation::LongTermPolicy,
            r#"{"operation":{"resume":{"selector":{"topic_pattern":"release-*"}}},"reason":"resume release memory","dry_run":true}"#,
            r#"{"operation":{"resume":{"selector":{"topic_pattern":"security-*"}}},"reason":"resume release memory","dry_run":true}"#,
        ),
    ] {
        let first = decode_json_adapter_command(operation, body, &options).expect("first command");
        let retry = decode_json_adapter_command(operation, body, &options).expect("retry command");
        let different = decode_json_adapter_command(operation, different_body, &options)
            .expect("different command");
        assert_eq!(
            first
                .idempotency_fingerprint_material()
                .expect("first material"),
            retry
                .idempotency_fingerprint_material()
                .expect("retry material")
        );
        assert_ne!(
            first
                .idempotency_fingerprint_material()
                .expect("first material"),
            different
                .idempotency_fingerprint_material()
                .expect("different material")
        );
    }
}

#[test]
fn transport_request_identity_is_unique_but_missing_caller_key_has_no_durable_operation_id() {
    let owner = AdapterRequestIdentityOwner::new(TransportKind::Wss, "session-1", "principal-a");

    let first = owner.issue(None).expect("first identity");
    let second = owner.issue(None).expect("second identity");

    assert_ne!(first.request_id, second.request_id);
    assert_ne!(first.audit_id, second.audit_id);
    assert_eq!(first.mutation_operation_id, None);
    assert_eq!(second.mutation_operation_id, None);
}

#[test]
fn explicit_mutation_operation_identity_is_transport_neutral_and_principal_scoped() {
    let first_owner =
        AdapterRequestIdentityOwner::new(TransportKind::Mcp, "server-a", "principal-a");
    let retry_owner =
        AdapterRequestIdentityOwner::new(TransportKind::Wss, "session-b", "principal-a");
    let other_principal =
        AdapterRequestIdentityOwner::new(TransportKind::Mcp, "server-a", "principal-b");

    let first = first_owner
        .issue(Some("caller-key"))
        .expect("first identity");
    let retry = retry_owner
        .issue(Some("caller-key"))
        .expect("retry identity");
    let isolated = other_principal
        .issue(Some("caller-key"))
        .expect("isolated identity");

    assert_eq!(first.mutation_operation_id, retry.mutation_operation_id);
    assert_ne!(first.request_id, retry.request_id);
    assert_ne!(first.mutation_operation_id, isolated.mutation_operation_id);
    let operation_id = first
        .mutation_operation_id
        .as_deref()
        .expect("explicit mutation operation id");
    assert!(!operation_id.contains("principal-a"));
    assert!(!operation_id.contains("caller-key"));
    assert!(operation_id.starts_with("explicit:v1:sha256:"));
}

#[test]
fn transport_identity_rejects_missing_authority_material() {
    let missing_principal = AdapterRequestIdentityOwner::new(TransportKind::A2a, "peer", " ");
    assert_eq!(
        missing_principal.issue(None),
        Err(AdapterRequestIdentityError::EmptyPrincipal)
    );

    let owner = AdapterRequestIdentityOwner::new(TransportKind::A2a, "peer", "principal");
    assert_eq!(
        owner.issue(Some(" ")),
        Err(AdapterRequestIdentityError::EmptyExplicitIdempotencyKey)
    );
}

impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

fn host_test_profile() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosStandaloneMemory
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::DesktopLinuxEmbeddedSdk
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        ProfileId::EspEmbeddedSdk
    }
}

fn runtime() -> MemoryRuntime {
    let store = MemoryStoreHandle::open_in_memory(
        StoreBackendConfig::in_memory(host_test_profile()).expect("config"),
    )
    .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(store)
        .clock(Arc::new(FixedClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime")
}

fn envelope<T>(
    runtime: &MemoryRuntime,
    operation: AdapterOperation,
    payload: T,
) -> AdapterEnvelope<T> {
    let lease = runtime
        .acquire_runtime_budget_lease()
        .expect("runtime budget lease");
    AdapterEnvelope {
        protocol_version: bm_adapter::ExternalAiMemoryProtocolVersion::V2,
        runtime_binding: bm_adapter::AdapterProtocolBinding::for_runtime(runtime, &lease),
        request_id: "req-1".to_string(),
        transport: TransportKind::Http,
        mode: TransportMode::Server,
        operation,
        source: AdapterSource {
            source_id: "source-1".to_string(),
            source_kind: "http_client".to_string(),
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
        },
        auth: AdapterAuthContext {
            authenticated: true,
            auth_kind: "token".to_string(),
            principal: "operator".to_string(),
        },
        mutation_operation_id: Some("idem-1".to_string()),
        audit_id: "audit-1".to_string(),
        payload,
    }
}

fn durable_write_command(summary: &str) -> AdapterCommand {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Project,
        topic: "adapter-v2".to_string(),
    };
    AdapterCommand::Write(MemoryWriteRequest::Candidates {
        candidates: vec![MemoryWriteCandidate {
            candidate_id: "adapter-v2-candidate".to_string(),
            authority: MemoryEvidenceAuthority::ProgramMemoryCanonical,
            target: target.clone(),
            long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
            privacy: MemoryPrivacyClass::SharedWithSubject,
            content: MemoryCandidateContent::Text {
                topic: "adapter-v2".to_string(),
                body: summary.to_string(),
                keywords: vec!["adapter".to_string(), "receipt".to_string()],
            },
            evidence_refs: vec!["adapter-v2:contract".to_string()],
            canonical_entities: Vec::new(),
            semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                source: MemorySemanticJudgmentSource::RuntimeGate,
                decision: MemoryCandidateSemanticDecision::Accept,
                governed_target: Some(target),
                reason: "adapter V2 durable receipt fixture".to_string(),
            }),
        }],
        runtime_skill_owning_scope: None,
    })
}

#[test]
fn v1_durable_mutation_and_v2_missing_operation_identity_fail_closed() {
    let runtime = runtime();
    let mut legacy = envelope(
        &runtime,
        AdapterOperation::Write,
        durable_write_command("legacy mutation"),
    );
    legacy.protocol_version = bm_adapter::ExternalAiMemoryProtocolVersion::V1;
    let legacy_response = dispatch_adapter_command(&runtime, legacy).expect("legacy response");
    assert!(matches!(
        legacy_response,
        AdapterResponse::Rejected {
            error_key: AdapterErrorKey::RuntimeBindingMismatch,
            ..
        }
    ));

    for mutation_operation_id in [None, Some(String::new()), Some(" spaced ".to_string())] {
        let mut missing = envelope(
            &runtime,
            AdapterOperation::Write,
            durable_write_command("missing identity"),
        );
        missing.mutation_operation_id = mutation_operation_id;
        let response = dispatch_adapter_command(&runtime, missing).expect("missing id response");
        assert!(matches!(
            response,
            AdapterResponse::Rejected {
                error_key: AdapterErrorKey::MutationOperationIdRequired,
                ..
            }
        ));
    }
}

#[test]
fn v1_rejects_every_mutation_reliability_class_but_keeps_reads() {
    let runtime = runtime();
    let mut non_durable = envelope(
        &runtime,
        AdapterOperation::FinalizeTurn,
        AdapterCommand::FinalizeTurn(Box::new(MemoryTurnFinalizeRequest {
            turn: CanonicalTurnDelta {
                turn_id: "turn-v1-rejected".to_string(),
                conversation: ConversationScope {
                    channel: "local".to_string(),
                    chat_id: "chat-1".to_string(),
                    conversation_id: None,
                },
                subject: bm_sdk::default_agent_subject_id("agent-main"),
                delivery_status: MemoryTurnDeliveryStatus::Delivered,
                source: MemoryTurnSource {
                    ingress: bm_sdk::IngressKind::User,
                    channel: "local".to_string(),
                    provider: None,
                    protocol: MemoryTurnProtocol::Native,
                    endpoint: None,
                    model_alias: None,
                    model_resolved: None,
                    request_id: None,
                    client_conversation_hint: None,
                },
                actor: None,
                input_messages: vec![TranscriptInputMessage::user("v1 mutation")],
                assistant_message: Some(TranscriptInputMessage::assistant("rejected")),
                tool_observations: Vec::new(),
                external_content_used: false,
                candidate_ids: Vec::new(),
            },
            tool_calls: 0,
            runtime_skill_selected_ids: Vec::new(),
            task_learning_selected_ids: Vec::new(),
            reuse_outcome_note: String::new(),
            tool_usage_feedback: None,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })),
    );
    non_durable.protocol_version = bm_adapter::ExternalAiMemoryProtocolVersion::V1;
    assert!(matches!(
        dispatch_adapter_command(&runtime, non_durable).expect("V1 mutation rejection"),
        AdapterResponse::Rejected {
            error_key: AdapterErrorKey::RuntimeBindingMismatch,
            ..
        }
    ));

    let mut read = envelope(
        &runtime,
        AdapterOperation::Recall,
        AdapterCommand::Recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "v1 read".to_string(),
            limit: 1,
            tool_registry_refs: Vec::new(),
        }),
    );
    read.protocol_version = bm_adapter::ExternalAiMemoryProtocolVersion::V1;
    assert!(matches!(
        dispatch_adapter_command(&runtime, read).expect("V1 read"),
        AdapterResponse::Accepted { .. }
    ));
}

#[test]
fn v2_commit_and_replay_return_the_same_typed_safe_receipt_and_conflict_is_structured() {
    let runtime = runtime();
    let first = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::Write,
            durable_write_command("stable intent"),
        ),
    )
    .expect("first mutation");
    let AdapterResponse::Accepted {
        receipt: Some(committed_receipt),
        ..
    } = first
    else {
        panic!("first durable mutation must return its committed receipt");
    };

    let replay = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::Write,
            durable_write_command("stable intent"),
        ),
    )
    .expect("replayed mutation");
    let AdapterResponse::Replayed {
        mutation_operation_id,
        receipt,
        ..
    } = replay
    else {
        panic!("expected durable replay, got {replay:?}");
    };
    assert_eq!(mutation_operation_id, "idem-1");
    assert_eq!(receipt.mutation_operation_id, "idem-1");
    assert_eq!(receipt, committed_receipt);
    let encoded = serde_json::to_string(&receipt).expect("safe receipt JSON");
    for forbidden in [
        "memory_space_id",
        "mounted_subject_id",
        "intent_digest",
        "effect_plan_digest",
        "Durable operation receipt contract",
    ] {
        assert!(!encoded.contains(forbidden), "receipt leaked {forbidden}");
    }

    let conflict = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::Write,
            durable_write_command("different intent"),
        ),
    )
    .expect("conflicting mutation response");
    assert!(matches!(
        conflict,
        AdapterResponse::Rejected {
            error_key: AdapterErrorKey::MutationOperationConflict,
            ..
        }
    ));
}

#[test]
fn recall_command_dispatches_through_memory_runtime() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::Recall,
            AdapterCommand::Recall(MemoryRecallRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            request_id,
            audit_id,
            report: AdapterSdkReport::Recall(report),
            ..
        } => {
            assert_eq!(request_id, "req-1");
            assert_eq!(audit_id, "audit-1");
            assert_eq!(report.query, "release");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn project_command_returns_only_the_adapter_projection_contract() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::Project,
            AdapterCommand::Project(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: "release".to_string(),
                system_max_len: 1024,
                recent_messages_limit: 2,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
            }),
        ),
    )
    .expect("dispatch project");

    match response {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::Project(report),
            ..
        } => {
            assert_eq!(report.chars, report.projection_block.chars().count());
            assert!(!report.audit.projection_id.is_empty());
            assert!(report.audit.disclosure_integrity_passed);
            assert_eq!(report.audit.raw_private_violation_count, 0);
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn adapter_projection_path_has_no_provider_payload_capability() {
    let contract = include_str!("../src/contract.rs");
    let dispatch = include_str!("../src/dispatch.rs");

    for forbidden in [
        "MemoryProjectionOutput",
        "ProviderProjectionPayload",
        "provider_payload",
        "system_memory_block",
    ] {
        assert!(
            !contract.contains(forbidden),
            "adapter contract gained provider capability: {forbidden}"
        );
        assert!(
            !dispatch.contains(forbidden),
            "adapter dispatch gained provider capability: {forbidden}"
        );
    }
    assert!(dispatch.contains(".project_safe(request)"));
}

#[test]
fn operation_mismatch_is_rejected_before_runtime_call() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::Write,
            AdapterCommand::Recall(MemoryRecallRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Rejected { error_key, .. } => {
            assert_eq!(error_key, AdapterErrorKey::OperationMismatch);
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn json_adapter_preserves_structured_query_facets_for_recall_and_projection() {
    let options = AdapterJsonCommandOptions::new("adapter-test");
    let recall = decode_json_adapter_command(
        AdapterOperation::Recall,
        r#"{
            "temporal_operation":{"kind":"current"},
            "query":"typed entity",
            "structured_query_facets":[
                {"kind":"unresolved_entity","value":"Alice"}
            ]
        }"#,
        &options,
    )
    .expect("decode typed recall");
    let AdapterCommand::Recall(recall) = recall else {
        panic!("expected recall command");
    };
    assert_eq!(
        recall.structured_query_facets,
        vec![QueryFacetInput::UnresolvedEntity("Alice".to_string())]
    );

    let project = decode_json_adapter_command(
        AdapterOperation::Project,
        r#"{
            "temporal_operation":{"kind":"current"},
            "user_query":"typed temporal",
            "system_max_len":4096,
            "structured_query_facets":[
                {"kind":"unresolved_temporal","value":"last week"}
            ]
        }"#,
        &options,
    )
    .expect("decode typed projection");
    let AdapterCommand::Project(project) = project else {
        panic!("expected project command");
    };
    assert_eq!(
        project.structured_query_facets,
        vec![QueryFacetInput::UnresolvedTemporal("last week".to_string())]
    );
}

#[test]
fn recall_and_project_json_require_strict_typed_temporal_operation() {
    let options = AdapterJsonCommandOptions::new("adapter-temporal-contract");
    let recall = decode_json_adapter_command(
        AdapterOperation::Recall,
        r#"{
            "temporal_operation":{"kind":"historical_as_of","as_of_time":1700000000},
            "query":"typed historical recall"
        }"#,
        &options,
    )
    .expect("typed historical recall");
    let AdapterCommand::Recall(recall) = recall else {
        panic!("expected recall command");
    };
    assert_eq!(
        recall.temporal_operation,
        bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
            as_of_time: 1_700_000_000
        }
    );

    for body in [
        r#"{"query":"missing temporal"}"#,
        r#"{"temporal_operation":{"kind":"unknown"},"query":"unknown variant"}"#,
        r#"{"temporal_operation":{"kind":"current"},"query":"unknown field","legacy":true}"#,
    ] {
        assert!(
            decode_json_adapter_command(AdapterOperation::Recall, body, &options).is_err(),
            "strict recall payload unexpectedly accepted: {body}"
        );
    }

    let project = decode_json_adapter_command(
        AdapterOperation::Project,
        r#"{
            "temporal_operation":{"kind":"historical_as_of","as_of_time":1700000001},
            "user_query":"typed historical project",
            "system_max_len":4096
        }"#,
        &options,
    )
    .expect("typed historical project");
    let AdapterCommand::Project(project) = project else {
        panic!("expected project command");
    };
    assert_eq!(
        project.temporal_operation,
        bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
            as_of_time: 1_700_000_001
        }
    );
    assert!(decode_json_adapter_command(
        AdapterOperation::Project,
        r#"{
            "temporal_operation":{"kind":"current"},
            "query":"legacy alias",
            "system_max_len":4096
        }"#,
        &options,
    )
    .is_err());
}

#[test]
fn adapter_safe_dto_is_strict_and_all_protocol_sources_delegate_to_its_owner() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::Recall,
            AdapterCommand::Recall(MemoryRecallRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                query: "governed safe dto".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        ),
    )
    .expect("dispatch governed recall");
    let AdapterResponse::Accepted {
        report: AdapterSdkReport::Recall(report),
        ..
    } = response
    else {
        panic!("accepted governed recall report");
    };
    let dto = AdapterSdkReport::Recall(report)
        .governed_safe_report()
        .expect("governed safe DTO");
    let encoded = serde_json::to_string(&dto).expect("serialize governed safe DTO");
    for forbidden in [
        "owner_id",
        "owner_revision_ref",
        "procedure_content",
        "private_garden",
        "state_digest",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "adapter safe DTO leaked {forbidden}: {encoded}"
        );
    }
    let decoded: bm_adapter::AdapterGovernedSafeReportV1 =
        serde_json::from_str(&encoded).expect("strict governed safe DTO round-trip");
    assert_eq!(decoded, dto);
    let mut unknown: serde_json::Value =
        serde_json::from_str(&encoded).expect("safe DTO JSON value");
    unknown["unknown_business_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<bm_adapter::AdapterGovernedSafeReportV1>(unknown).is_err());

    for source in [
        include_str!("../../http/src/lib.rs"),
        include_str!("../../wss/src/lib.rs"),
        include_str!("../../mcp/src/lib.rs"),
        include_str!("../../a2a/src/lib.rs"),
        include_str!("../../cli/src/lib.rs"),
    ] {
        assert!(source.contains(".governed_safe_report()"));
        assert!(!source.contains("procedural_delivery_reports"));
    }
}

#[test]
fn governed_request_schema_is_owned_once_by_the_adapter_contract() {
    let recall =
        governed_adapter_json_command_schema(AdapterOperation::Recall).expect("recall schema");
    assert_eq!(
        recall.field_names,
        [
            "temporal_operation",
            "query",
            "limit",
            "structured_query_facets",
            "tool_registry_refs",
        ]
    );
    assert_eq!(
        recall.input_schema["required"],
        json!(["temporal_operation", "query"])
    );
    assert_eq!(recall.input_schema["additionalProperties"], false);
    assert!(recall.input_schema["properties"]["temporal_operation"]["oneOf"].is_array());
    assert_eq!(
        recall.input_schema["properties"]["temporal_operation"]["oneOf"][1]["properties"]
            ["as_of_time"]["minimum"],
        1
    );

    let project =
        governed_adapter_json_command_schema(AdapterOperation::Project).expect("project schema");
    assert_eq!(
        project.field_names,
        [
            "temporal_operation",
            "user_query",
            "system_max_len",
            "recent_messages_limit",
            "pressure",
            "mode_input",
            "structured_query_facets",
            "tool_registry_refs",
        ]
    );
    assert_eq!(
        project.input_schema["required"],
        json!(["temporal_operation", "user_query", "system_max_len"])
    );
    assert_eq!(project.input_schema["additionalProperties"], false);

    let finalize = governed_adapter_json_command_schema(AdapterOperation::FinalizeTurn)
        .expect("finalize turn schema");
    assert_eq!(finalize.input_schema["required"], json!(["turn"]));
    assert_eq!(finalize.input_schema["additionalProperties"], false);
    assert_eq!(
        finalize.input_schema["properties"]["turn"]["type"],
        "object"
    );
    assert!(governed_adapter_json_command_schema(AdapterOperation::Inspect).is_none());
}

#[test]
fn macos_standalone_profile_forwarding_is_exact_for_wss_and_mcp() {
    for manifest in [
        include_str!("../../wss/Cargo.toml"),
        include_str!("../../mcp/Cargo.toml"),
    ] {
        assert_eq!(
            manifest
                .lines()
                .filter(|line| line.starts_with("profile-desktop-macos-standalone-memory ="))
                .count(),
            1
        );
        assert!(manifest.contains(
            "profile-desktop-macos-standalone-memory = [\"bm-adapter/profile-desktop-macos-standalone-memory\"]"
        ));
    }
}

#[test]
fn long_term_list_command_dispatches_through_memory_runtime() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::LongTermList,
            AdapterCommand::LongTermList(MemoryLongTermListRequest {
                query: bm_sdk::LongTermMemoryQuery::default(),
                cursor: None,
                limit: 10,
                view: MemoryLongTermControlView::HostUi,
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            request_id,
            audit_id,
            report: AdapterSdkReport::LongTermList(report),
            ..
        } => {
            assert_eq!(request_id, "req-1");
            assert_eq!(audit_id, "audit-1");
            assert_eq!(report.total_visible, 0);
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

fn transcript_attr(
    key: ConversationKey,
    turn_id: impl Into<String>,
    message_id: impl Into<String>,
) -> TranscriptAttrEnvelope {
    TranscriptAttrEnvelope {
        attr_id: "adapter-usage-1".to_string(),
        target: TranscriptAttrTarget {
            key,
            scope: TranscriptAttrScope::Message,
            turn_id: turn_id.into(),
            message_id: Some(message_id.into()),
        },
        key: "host.adapter.model_usage".to_string(),
        value_kind: TranscriptAttrValueKind::JsonObject,
        schema_ref: Some("adapter.model-usage.v1".to_string()),
        value: json!({"input_tokens": 9, "output_tokens": 3, "usage_source": "provider_reported"}),
        visibility: HostRefVisibility::HostUi,
        source: TranscriptAttrSource {
            writer: "adapter-test".to_string(),
            source_kind: TranscriptAttrSourceKind::ProviderReported,
            written_at: 1_800_000_000,
            audit_reason: "adapter transcript attr contract".to_string(),
        },
        governance: TranscriptAttrGovernance {
            max_value_bytes: 4096,
            redaction_policy: TranscriptAttrRedactionPolicy::MetadataSurvivesMask,
            export_allowed: false,
        },
        links: vec![TranscriptAttrLink {
            relation: "model_invocation".to_string(),
            ref_kind: "model_invocation_id".to_string(),
            ref_id: "adapter-model-1".to_string(),
        }],
        created_at: 1_800_000_000,
        updated_at: 1_800_000_000,
    }
}

fn transcript_turn() -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: "turn-adapter-1".to_string(),
        conversation: ConversationScope {
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
            conversation_id: Some("conversation-a".to_string()),
        },
        subject: bm_sdk::default_agent_subject_id("agent-main"),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: bm_sdk::IngressKind::User,
            channel: "local".to_string(),
            provider: Some("adapter".to_string()),
            protocol: MemoryTurnProtocol::Native,
            endpoint: None,
            model_alias: None,
            model_resolved: None,
            request_id: Some("adapter-req-1".to_string()),
            client_conversation_hint: Some("conversation-a".to_string()),
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user("adapter user")],
        assistant_message: Some(TranscriptInputMessage::assistant("adapter assistant")),
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    }
}

#[test]
fn transcript_attr_write_command_dispatches_through_memory_runtime() {
    let runtime = runtime();
    let key = ConversationKey::new(runtime.memory_space_id(), "local", "conversation-a").unwrap();
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: transcript_turn(),
            host_refs: Vec::new(),
        })
        .expect("commit transcript");
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .expect("replay transcript");
    let turn = &replay.slice.turns[0];
    let message_id = turn
        .assistant_message
        .as_ref()
        .expect("assistant message")
        .message_id
        .clone();

    let attr = transcript_attr(key.clone(), turn.turn_id.clone(), message_id);
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            &runtime,
            AdapterOperation::TranscriptAttrWrite,
            AdapterCommand::TranscriptAttrWrite(MemoryTranscriptAttrWriteRequest {
                memory_space_id: key.memory_space_id,
                channel_id: key.channel_id,
                conversation_id: key.conversation_id,
                attrs: vec![attr],
                idempotency_key: Some("adapter-attr-write-1".to_string()),
                dry_run: false,
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::TranscriptAttrWrite(report),
            ..
        } => {
            assert_eq!(report.accepted_attrs.len(), 1);
            assert!(report.rejected_attrs.is_empty());
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn json_decoder_covers_adapter_memory_operations() {
    let options =
        AdapterJsonCommandOptions::new("test-adapter").with_default_source_chat_id("chat-1");
    let cases = [
        (
            AdapterOperation::Write,
            r#"{"name":"runtime_skill__adapter_write","topic":"adapter","title":"Adapter write","summary":"Adapter write summary","content":"1. Decode write payload.\n2. Dispatch common adapter command.","owning_scope":{"kind":"subject","mounted_subject_id":"agent:agent-main"},"creation_ref":{"kind":"replay_promotion","candidate_ref":"test:adapter-write","verification_receipt_digest":"sha256:2222222222222222222222222222222222222222222222222222222222222222"},"privacy_class":"shared_with_subject"}"#,
        ),
        (
            AdapterOperation::Recall,
            r#"{"temporal_operation":{"kind":"current"},"query":"release","limit":2}"#,
        ),
        (
            AdapterOperation::Project,
            r#"{"temporal_operation":{"kind":"current"},"user_query":"release","system_max_len":1024,"recent_messages_limit":2}"#,
        ),
        (
            AdapterOperation::Maintain,
            r#"{"user_content":"remember release guard","reply_content":"I will verify artifacts.","tool_calls":0}"#,
        ),
        (
            AdapterOperation::Inspect,
            r#"{"query":"release","system_max_len":1024}"#,
        ),
        (AdapterOperation::Recover, r#"{}"#),
        (
            AdapterOperation::Replay,
            r#"{"chat_id":"chat-1","limit":2}"#,
        ),
        (
            AdapterOperation::LongTermList,
            r#"{"query":{"kind":"project"},"limit":10}"#,
        ),
        (
            AdapterOperation::LongTermPolicy,
            r#"{"operation":{"suppress":{"selector":{"kind":"preference","topic_pattern":"temporary-*"},"duration":"until_manual_resume"}},"reason":"operator suppression"}"#,
        ),
        (
            AdapterOperation::TranscriptAttrWrite,
            r#"{
                "memory_space_id":"memory-space-owner-default",
                "channel_id":"local",
                "conversation_id":"conversation-a",
                "attrs":[],
                "idempotency_key":"attr-write-1",
                "dry_run":true
            }"#,
        ),
        (AdapterOperation::Close, r#"{"reason":"operator close"}"#),
    ];

    for (operation, body) in cases {
        let command =
            decode_json_adapter_command(operation, body, &options).expect("decode command");
        assert_eq!(command.operation(), operation);
    }

    let finalize_body = json!({"turn": transcript_turn()}).to_string();
    let finalize =
        decode_json_adapter_command(AdapterOperation::FinalizeTurn, &finalize_body, &options)
            .expect("decode finalize turn command");
    assert_eq!(finalize.operation(), AdapterOperation::FinalizeTurn);
}

#[test]
fn projection_and_inspection_require_runtime_owned_render_budget() {
    let options = AdapterJsonCommandOptions::new("test-adapter");
    for (operation, body) in [
        (
            AdapterOperation::Project,
            r#"{"temporal_operation":{"kind":"current"},"user_query":"release"}"#,
        ),
        (AdapterOperation::Inspect, r#"{"query":"release"}"#),
    ] {
        let error = decode_json_adapter_command(operation, body, &options)
            .expect_err("adapter must not invent a projection render budget");
        assert!(error.to_string().contains("system_max_len"), "{error}");
    }
}

#[test]
fn json_decoder_accepts_agent_tool_usage_feedback_as_write_payload() {
    let command = decode_json_adapter_command(
        AdapterOperation::Write,
        r#"{
            "tool_usage_feedback": {
                "registry_ref": {
                    "registry_id": "host-tools",
                    "fingerprint": "registry-fp",
                    "scope": "global"
                },
                "observations": [{
                    "observation_id": "obs-1",
                    "registry_id": "host-tools",
                    "tool_id": "pdf.extract",
                    "schema_fingerprint": "schema-pdf-v1",
                    "call_id": "call-1",
                    "task_signature": "extract_pdf_text",
                    "summary": "PDF extraction produced usable text.",
                    "outcome": "succeeded",
                    "error_code": null,
                    "external_content": true,
                    "private_content_used": false,
                    "permission_tags": ["filesystem.read"],
                    "risk_tags": ["external_content"],
                    "started_at": 1800000000,
                    "completed_at": 1800000001
                }],
                "user_visible_result_summary": "PDF extraction worked.",
                "reuse_outcome": "succeeded",
                "operator_note": null
            }
        }"#,
        &AdapterJsonCommandOptions::new("test-adapter"),
    )
    .expect("decode feedback");

    match command {
        AdapterCommand::Write(MemoryWriteRequest::AgentToolUsageFeedback { feedback }) => {
            assert_eq!(feedback.registry_ref.registry_id, "host-tools");
            assert_eq!(feedback.observations[0].tool_id, "pdf.extract");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn maintain_dispatch_uses_injected_runtime_services() {
    let runtime = runtime();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let command = decode_json_adapter_command(
        AdapterOperation::Maintain,
        r#"{"user_content":"remember the release process","reply_content":"I will verify artifacts first."}"#,
        &AdapterJsonCommandOptions::new("test-adapter"),
    )
    .expect("maintain command");

    let lease = runtime
        .acquire_runtime_budget_lease()
        .expect("runtime budget lease");
    let response = dispatch_adapter_command_with_services(
        &runtime,
        &lease,
        envelope(&runtime, AdapterOperation::Maintain, command),
        AdapterRuntimeServices {
            http: Some(&mut http),
            llm: Some(&llm),
        },
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::Maintain(report),
            ..
        } => {
            assert!(report.report.is_some());
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn finalize_dispatch_queues_governance_even_when_request_services_are_injected() {
    let runtime = runtime();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let command = decode_json_adapter_command(
        AdapterOperation::FinalizeTurn,
        &json!({"turn": transcript_turn()}).to_string(),
        &AdapterJsonCommandOptions::new("test-adapter"),
    )
    .expect("finalize command");
    let lease = runtime
        .acquire_runtime_budget_lease()
        .expect("runtime budget lease");
    let response = dispatch_adapter_command_with_services(
        &runtime,
        &lease,
        envelope(&runtime, AdapterOperation::FinalizeTurn, command),
        AdapterRuntimeServices {
            http: Some(&mut http),
            llm: Some(&llm),
        },
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::FinalizeTurn(report),
            ..
        } => {
            assert!(!report.maintenance_performed);
            assert_eq!(
                report.memory_consolidation.state,
                bm_sdk::MemoryConsolidationState::Queued
            );
            assert!(report.memory_consolidation.job_id.is_some());
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

struct StaticHttpClient;

impl LlmHttpClient for StaticHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> bm_sdk::Result<(u16, ResponseBody)> {
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

struct StaticLlmClient;

impl LlmClient for StaticLlmClient {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> bm_sdk::Result<LlmResponse> {
        Ok(LlmResponse {
            content: "Summary: release safety".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

#[test]
fn adapter_crate_manifest_has_no_direct_core_or_store_dependency() {
    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
        .expect("manifest");
    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or_default()
        .split('[')
        .next()
        .unwrap_or_default();

    assert!(!dependencies.contains("bm-core"));
    assert!(!dependencies.contains("bm-store"));
    assert!(dependencies.contains("bm-sdk"));
}
