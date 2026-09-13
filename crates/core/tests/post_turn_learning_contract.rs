use bm_core::memory::{
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef,
    PostTurnLearningEvidenceV2, ProceduralApplicabilityContextV1,
    ProceduralFeedbackApplicationLedgerV2, ProceduralFeedbackAuthorityV2,
    ProceduralFeedbackIdentityV1, ProceduralFeedbackJobRefV2, ProceduralFeedbackJobStatusV1,
    ProceduralFeedbackJobV2, ProceduralFeedbackReceiptV2, ProceduralFeedbackScopeIndexV2,
    ProceduralProjectionIdentityV1, ProceduralSelectionReceiptV1,
    POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION, PROCEDURAL_SELECTION_RECEIPT_SCHEMA_VERSION,
};
use bm_core::skills::AgentToolRegistryScope;

#[test]
fn pfi2_rejects_legacy_untyped_tool_evidence_wire() {
    // This is deliberately the released, ambiguous wire shape, not a serialization
    // of the new producer. A clean break must not reinterpret its privacy bool.
    let legacy = serde_json::json!({
        "registry_ref": {"registry_id": "tools-a", "fingerprint": "registry-a", "scope": "owner"},
        "tool_id": "archive-tool", "schema_fingerprint": "schema-a",
        "observations": [{
            "observation_id": "observation-a", "registry_id": "tools-a",
            "tool_id": "archive-tool", "schema_fingerprint": "schema-a",
            "call_id": "call-a", "task_signature": "archive-task",
            "summary": "private-derived method", "outcome": "succeeded",
            "error_code": null, "external_content": false,
            "private_content_used": false, "permission_tags": [], "risk_tags": [],
            "started_at": 90, "completed_at": 100
        }],
        "outcome": "succeeded", "user_visible_result_summary": null,
        "operator_note": null
    });
    assert!(
        serde_json::from_value::<bm_core::memory::AgentToolUsageFeedbackV3>(legacy).is_err(),
        "legacy bool and untyped summary must not acquire new evidence authority"
    );
}

#[test]
fn pfi2_canonical_tool_observation_requires_the_exact_call_identity() {
    use bm_core::memory::ToolObservationDigest;
    let mut wire = serde_json::json!({
        "observation_id": "observation-a", "call_id": "call-a", "tool_name": "extract",
        "summary": "Independent conversation observation", "external_content": false
    });
    assert!(serde_json::from_value::<ToolObservationDigest>(wire.clone()).is_ok());
    wire.as_object_mut().unwrap().remove("call_id");
    assert!(
        serde_json::from_value::<ToolObservationDigest>(wire).is_err(),
        "an evidence call id must bind canonical intake, not exist only in its own claim"
    );
}

#[test]
fn pfi2_canonical_source_tightens_method_admission_without_losing_execution() {
    use bm_core::memory::*;
    let input = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    let canonical = ToolObservationDigest {
        observation_id: "observation-a".into(),
        call_id: "call-a".into(),
        tool_name: "extract".into(),
        summary: "Independent result, not the method".into(),
        external_content: false,
    };
    let positive = input
        .admit_for_turn(std::slice::from_ref(&canonical), false, 100)
        .unwrap();
    assert_eq!(positive.execution_facts.len(), 1);
    assert_eq!(positive.method_evidence.len(), 1);
    for external in [false, true] {
        let mut source = canonical.clone();
        source.external_content = !external;
        let denied = input
            .admit_for_turn(std::slice::from_ref(&source), external, 100)
            .unwrap();
        assert_eq!(denied.execution_facts, positive.execution_facts);
        assert!(denied.method_evidence.is_empty());
        assert_eq!(
            denied.rejected_methods[0].rejection,
            ProceduralEvidenceRejection::ExternalMethodSource
        );
        let wire = serde_json::to_string(&denied).unwrap();
        assert!(!wire.contains(&input.method_evidence[0].body));
        assert!(!wire.contains(&input.method_evidence[0].method_id));
        assert!(!positive.validates_canonical_turn(std::slice::from_ref(&source), external, 100));
    }
    for mutation in 0..5 {
        let mut source = canonical.clone();
        let mut now = 100;
        match mutation {
            0 => source.call_id = "foreign-call".into(),
            1 => source.observation_id = "foreign-observation".into(),
            2 => source.tool_name = "other-tool".into(),
            3 => now = 99,
            _ => source.call_id = " call-a".into(),
        }
        assert_eq!(
            input.admit_for_turn(&[source], false, now),
            Err(ProceduralEvidenceRejection::ExecutionReferenceInvalid)
        );
    }
    assert!(input
        .admit_for_turn(&[canonical.clone(), canonical], false, 100)
        .is_err());
}

fn pfi2_input(
    sensitivity: bm_core::memory::ProceduralSourceSensitivity,
) -> bm_core::memory::AgentToolUsageFeedbackV3 {
    use bm_core::memory::*;
    AgentToolUsageFeedbackV3 {
        registry_ref: bm_core::skills::AgentToolRegistryRef::new("tools-a", digest('a')),
        tool_id: "extract".into(),
        schema_fingerprint: "schema-a".into(),
        execution_facts: vec![ToolExecutionFactV1 {
            observation_id: "observation-a".into(),
            call_id: "call-a".into(),
            outcome: ToolExecutionOutcome::Succeeded,
            source_sensitivity: sensitivity,
            started_at: Some(90),
            completed_at: Some(100),
        }],
        method_evidence: vec![ToolMethodEvidenceV1 {
            method_id: "method-a".into(),
            task_signature: "archive-task".into(),
            body: "1. Inspect the manifest.\n2. Extract into the workspace.".into(),
            execution_refs: vec!["observation-a".into()],
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
            external_content: false,
        }],
    }
}

#[test]
fn pfi2_nested_registry_metadata_is_strict_and_bounded() {
    use bm_core::memory::{AgentToolUsageFeedbackV3, ProceduralSourceSensitivity};
    let input = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    assert!(input.admit_content().is_ok());
    let mut nested_ref = serde_json::to_value(&input).unwrap();
    nested_ref["registry_ref"]["authority"] = serde_json::json!("forged");
    assert!(serde_json::from_value::<AgentToolUsageFeedbackV3>(nested_ref).is_err());
    let mut nested_scope = serde_json::to_value(&input).unwrap();
    nested_scope["registry_ref"]["scope"] = serde_json::json!({"conversation": {
        "conversation_id": "conversation-a", "raw": "synthetic-private-extra"
    }});
    assert!(serde_json::from_value::<AgentToolUsageFeedbackV3>(nested_scope).is_err());
}

#[test]
fn pfi2_registry_scope_cannot_bypass_the_identifier_byte_budget() {
    use bm_core::memory::ProceduralSourceSensitivity;
    use bm_core::skills::AgentToolRegistryScope;
    let mut input = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    for scope in [
        AgentToolRegistryScope::Project {
            project_id: "x".repeat(257),
        },
        AgentToolRegistryScope::Workspace {
            workspace_id: "x".repeat(257),
        },
        AgentToolRegistryScope::Conversation {
            conversation_id: "x".repeat(257),
        },
    ] {
        input.registry_ref.scope = scope;
        assert!(
            input.admit_content().is_err(),
            "nested scope must remain bounded"
        );
    }
}

fn pfi2_producer() -> bm_core::memory::ProceduralProducerBindingV1 {
    use bm_core::memory::*;
    let feedback = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    ProceduralProducerBindingV1::build(
        ProceduralProducerSpecV1 {
            binding_id: "execution-witness-a".into(),
            scope: ProceduralProducerScopeV1 {
                memory_space_id: "space-a".into(),
                mounted_subject_id: "agent-a".into(),
                channel_id: "sdk.direct".into(),
                chat_id: "chat-a".into(),
            },
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
                registry_ref: feedback.registry_ref,
                tool_id: feedback.tool_id,
                schema_fingerprint: feedback.schema_fingerprint,
            }],
            source_config_ref: "host-executor-registration-a".into(),
        },
        ProceduralProducerStateV1::Active,
        producer_operation("grant-1"),
        100,
        None,
    )
    .unwrap()
}

fn producer_operation(id: &str) -> bm_core::memory::MemoryMutationOperationIdentity {
    bm_core::memory::MemoryMutationOperationIdentity::new(
        id,
        "space-a",
        "agent-a",
        "governor-a",
        bm_core::memory::MemoryMutationOperationKind::ProceduralProducerControl,
    )
    .unwrap()
}

#[test]
fn pfi2_subject_validity_epoch_fences_old_work_and_all_chat_scopes() {
    use bm_core::memory::*;
    let producer = pfi2_producer();
    let scope = ProceduralSubjectScopeV1 {
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
    };
    let root =
        ProceduralSubjectValidityRootV1::initialize(scope.clone(), producer.spec.scope.clone(), 2)
            .unwrap();
    assert!(root.permits_learning_reads(2).unwrap());
    assert_eq!(
        root.bind_scope(producer.spec.scope.clone(), 2).unwrap(),
        root
    );
    let mut other_chat = producer.spec.scope.clone();
    other_chat.chat_id = "chat-b".into();
    let joined = root.bind_scope(other_chat, 2).unwrap();
    assert_eq!(joined.validity_epoch, root.validity_epoch);
    assert!(joined
        .producer_scopes
        .iter()
        .any(|scope| scope.chat_id == "chat-a"));
    assert!(joined
        .producer_scopes
        .iter()
        .any(|scope| scope.chat_id == "chat-b"));
    let mut foreign = producer.spec.scope.clone();
    foreign.mounted_subject_id = "agent-b".into();
    assert!(joined.bind_scope(foreign, 3).is_err());
    let mut excess = producer.spec.scope.clone();
    excess.chat_id = "chat-c".into();
    assert!(joined.bind_scope(excess, 2).is_err());
    let revoked = ProceduralProducerBindingV1::build(
        producer.spec.clone(),
        ProceduralProducerStateV1::Revoked,
        producer_operation("validity-revoke"),
        101,
        Some(&producer),
    )
    .unwrap();
    let first_work = ProceduralReconciliationWorkV1 {
        scope: scope.clone(),
        target_epoch: 2,
        trigger: ProceduralReconciliationTriggerV1::ProducerControl {
            operation: revoked.operation_identity.clone(),
            before: producer.revision_ref().unwrap(),
            after: revoked.revision_ref().unwrap(),
        },
    };
    let fenced = joined.begin(&first_work, 2).unwrap();
    assert!(!fenced.permits_learning_reads(2).unwrap());
    assert_eq!(fenced.producer_scopes, joined.producer_scopes);
    let second_work = ProceduralReconciliationWorkV1 {
        scope,
        target_epoch: 3,
        trigger: ProceduralReconciliationTriggerV1::SourceChange {
            transaction_id: "real-source-transaction-2".into(),
            operation: None,
            changes: vec![pfi2_source_deletion()],
        },
    };
    let refenced = fenced.begin(&second_work, 2).unwrap();
    let retained_wire = serde_json::to_value(&refenced).unwrap();
    assert_eq!(
        retained_wire.get("retained_work"),
        Some(&serde_json::json!([
            first_work.reference().unwrap(),
            second_work.reference().unwrap()
        ])),
        "advancing the epoch must preserve exact discoverable references to previous durable work"
    );
    assert_ne!(
        first_work.reference().unwrap().job_id,
        second_work.reference().unwrap().job_id
    );
    assert!(
        refenced.complete(&first_work, digest('c'), 2).is_err(),
        "a prior epoch cannot publish after a second withdrawal"
    );
    assert!(refenced
        .block(&first_work, ProceduralReconciliationBlockV1::Capacity, 2)
        .is_err());
    let blocked = refenced
        .block(&second_work, ProceduralReconciliationBlockV1::Capacity, 2)
        .unwrap();
    assert!(!blocked.permits_learning_reads(2).unwrap());
    assert!(
        blocked.complete(&second_work, digest('c'), 2).is_err(),
        "blocked work cannot pretend to have completed"
    );
    let resumed = blocked.resume(&second_work, 2).unwrap();
    assert!(!resumed.permits_learning_reads(2).unwrap());
    let ready = resumed.complete(&second_work, digest('c'), 2).unwrap();
    assert!(ready.permits_learning_reads(2).unwrap());
    assert_eq!(ready.validity_epoch, second_work.target_epoch);
    assert!(ready.complete(&second_work, digest('d'), 2).is_err());
    let reopened: ProceduralSubjectValidityRootV1 =
        serde_json::from_slice(&serde_json::to_vec(&ready).unwrap()).unwrap();
    assert_eq!(reopened, ready);
    reopened.validate(2).unwrap();
}

fn pfi2_source_deletion() -> bm_core::memory::ProceduralSourceTransitionV1 {
    use bm_core::memory::*;
    let owner_ref = GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "source-a");
    ProceduralSourceTransitionV1 {
        owner_ref: owner_ref.clone(),
        before: ProceduralSourcePostImageV1::Present {
            revision: GovernedOwnerRevisionRef::try_new(owner_ref, 1).unwrap(),
            content_digest: digest('a'),
        },
        after: ProceduralSourcePostImageV1::Deleted,
    }
}

#[test]
fn pfi2_source_root_retains_exact_cause_across_producer_registration_and_source_change() {
    use bm_core::memory::*;
    let change = pfi2_source_deletion();
    let original = ProceduralSourceDependentsRootV1::create(
        "space-a".into(),
        change.owner_ref.clone(),
        change.before.clone(),
        "real-source-create".into(),
        None,
    )
    .unwrap();
    let mut spec = pfi2_producer().spec;
    spec.claims.execution_facts = false;
    spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
        source_revision: change.before.material().unwrap().0.clone(),
    };
    let producer = ProceduralProducerBindingV1::build(
        spec.clone(),
        ProceduralProducerStateV1::Active,
        producer_operation("source-grant"),
        101,
        None,
    )
    .unwrap();
    let registered = original.bind(&producer).unwrap();
    assert_eq!(
        registered.last_change, original.last_change,
        "producer registration cannot impersonate a source mutation"
    );
    assert_eq!(registered.revision, 2);
    assert_eq!(registered.dependents.len(), 1);
    assert_eq!(
        registered.bind(&producer).unwrap(),
        registered,
        "retained revision registration is idempotent"
    );
    let operation = MemoryMutationOperationIdentity::new(
        "actual-source-update",
        "space-a",
        "agent-a",
        "agent-a",
        MemoryMutationOperationKind::Write,
    )
    .unwrap();
    let after = ProceduralSourcePostImageV1::Present {
        revision: GovernedOwnerRevisionRef::try_new(change.owner_ref, 2).unwrap(),
        content_digest: digest('b'),
    };
    let changed = registered
        .advance(
            after.clone(),
            "actual-source-tx".into(),
            Some(operation.clone()),
        )
        .unwrap();
    assert_eq!(changed.current, after);
    assert_eq!(changed.last_change.before, Some(original.current));
    assert_eq!(changed.last_change.operation, Some(operation));
    assert_eq!(changed.dependents, registered.dependents);
    assert_eq!(
        changed
            .advance(changed.current.clone(), "retention-only-tx".into(), None)
            .unwrap(),
        changed
    );
    spec.scope.memory_space_id = "other-space".into();
    let foreign_op = MemoryMutationOperationIdentity::new(
        "foreign",
        "other-space",
        "agent-a",
        "governor-a",
        MemoryMutationOperationKind::ProceduralProducerControl,
    )
    .unwrap();
    let foreign = ProceduralProducerBindingV1::build(
        spec,
        ProceduralProducerStateV1::Active,
        foreign_op,
        101,
        None,
    )
    .unwrap();
    assert!(registered.bind(&foreign).is_err());
}

#[test]
fn pfi2_source_terminal_and_deleted_images_are_distinct_and_never_resurrect() {
    use bm_core::memory::*;
    let change = pfi2_source_deletion();
    let root = ProceduralSourceDependentsRootV1::create(
        "space-a".into(),
        change.owner_ref.clone(),
        change.before.clone(),
        "create-source".into(),
        None,
    )
    .unwrap();
    let (revision, content_digest) = change.before.material().unwrap();
    for termination in [
        GovernedOwnerTermination::Invalidated,
        GovernedOwnerTermination::Superseded,
    ] {
        let control = LongTermMemoryVersionTransitionBinding::new(
            revision.clone(),
            "exact-control-revision",
            "a".repeat(64),
        )
        .unwrap();
        let terminal = ProceduralSourcePostImageV1::Terminated {
            revision: revision.clone(),
            content_digest: content_digest.into(),
            termination,
            control,
        };
        let terminated = root
            .advance(terminal.clone(), "terminate-source".into(), None)
            .unwrap();
        assert_eq!(
            terminated.current.material(),
            Some((revision, content_digest))
        );
        assert!(terminated
            .advance(root.current.clone(), "illegal-revive".into(), None)
            .is_err());
        let deleted = terminated
            .advance(
                ProceduralSourcePostImageV1::Deleted,
                "purge-source".into(),
                None,
            )
            .unwrap();
        assert!(deleted.current.material().is_none());
        assert_eq!(deleted.last_change.before, Some(terminal));
        assert!(deleted
            .advance(root.current.clone(), "illegal-recreate".into(), None)
            .is_err());
    }
    assert!(ProceduralSourceDependentsRootV1::create(
        "space-a".into(),
        change.owner_ref,
        ProceduralSourcePostImageV1::Deleted,
        "false-genesis".into(),
        None
    )
    .is_err());
}

#[test]
fn pfi2_compound_source_change_requires_nonempty_unique_canonical_exact_revisions() {
    use bm_core::memory::*;
    let a = pfi2_source_deletion();
    let mut b = a.clone();
    b.owner_ref.owner_id = "source-b".into();
    b.before = ProceduralSourcePostImageV1::Present {
        revision: GovernedOwnerRevisionRef::try_new(b.owner_ref.clone(), 1).unwrap(),
        content_digest: digest('b'),
    };
    let make = |changes| ProceduralReconciliationWorkV1 {
        scope: ProceduralSubjectScopeV1 {
            memory_space_id: "space-a".into(),
            mounted_subject_id: "agent-a".into(),
        },
        target_epoch: 2,
        trigger: ProceduralReconciliationTriggerV1::SourceChange {
            transaction_id: "compound-source-transaction".into(),
            operation: None,
            changes,
        },
    };
    assert!(make(vec![a.clone(), b.clone()]).reference().is_ok());
    assert!(make(vec![]).reference().is_err());
    assert!(make(vec![a.clone(), a.clone()]).reference().is_err());
    assert!(make(vec![b.clone(), a.clone()]).reference().is_err());
    b.before = a.before;
    assert!(
        make(vec![b]).reference().is_err(),
        "a different source owner cannot borrow the predecessor revision"
    );
}

#[test]
fn pfi2_reconciliation_identity_has_real_cause_without_transcript_or_fake_operation() {
    use bm_core::memory::*;
    let producer = pfi2_producer();
    let scope = ProceduralSubjectScopeV1 {
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
    };
    let root =
        ProceduralSubjectValidityRootV1::initialize(scope.clone(), producer.spec.scope, 1).unwrap();
    let before = GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::EvidenceDocument, "source-a"),
        1,
    )
    .unwrap();
    let work = ProceduralReconciliationWorkV1 {
        scope,
        target_epoch: 2,
        trigger: ProceduralReconciliationTriggerV1::SourceChange {
            transaction_id: "real-source-transaction-1".into(),
            operation: None,
            changes: vec![ProceduralSourceTransitionV1 {
                owner_ref: before.owner_ref.clone(),
                before: ProceduralSourcePostImageV1::Present {
                    revision: before.clone(),
                    content_digest: digest('a'),
                },
                after: ProceduralSourcePostImageV1::Present {
                    revision: GovernedOwnerRevisionRef::try_new(before.owner_ref.clone(), 2)
                        .unwrap(),
                    content_digest: digest('b'),
                },
            }],
        },
    };
    let first = work.reference().unwrap();
    assert_eq!(first, work.clone().reference().unwrap());
    let wire = serde_json::to_value(&work).unwrap();
    for forbidden in [
        "turn_id",
        "transcript_sequence",
        "transcript_digest",
        "operation",
        "raw_body",
    ] {
        assert!(!serde_json::to_string(&wire).unwrap().contains(forbidden));
        let mut smuggled = wire.clone();
        smuggled[forbidden] = serde_json::json!("synthetic-private-value");
        assert!(serde_json::from_value::<ProceduralReconciliationWorkV1>(smuggled).is_err());
    }
    let mut invalid = work.clone();
    invalid.target_epoch = 1;
    assert!(invalid.reference().is_err());
    invalid = work.clone();
    invalid.scope.mounted_subject_id = "agent-b".into();
    assert!(root.begin(&invalid, 1).is_err());
    assert_ne!(
        first.subject_root_key,
        invalid.reference().unwrap().subject_root_key
    );
    invalid = work.clone();
    let ProceduralReconciliationTriggerV1::SourceChange { changes, .. } = &mut invalid.trigger
    else {
        unreachable!()
    };
    changes[0].after = ProceduralSourcePostImageV1::Present {
        revision: before,
        content_digest: digest('b'),
    };
    assert!(
        invalid.reference().is_err(),
        "unchanged revision cannot masquerade as a source transition"
    );
    let fenced = root.begin(&work, 1).unwrap();
    let mut forged = serde_json::to_value(&fenced).unwrap();
    forged["state"] = serde_json::json!({"kind": "ready", "completion": null});
    assert!(
        serde_json::from_value::<ProceduralSubjectValidityRootV1>(forged)
            .unwrap()
            .validate(1)
            .is_err()
    );
    let mut missing = serde_json::to_value(&fenced).unwrap();
    missing.as_object_mut().unwrap().remove("state");
    assert!(serde_json::from_value::<ProceduralSubjectValidityRootV1>(missing).is_err());
}

#[test]
fn pfi2_reconciliation_transcript_lifecycle_has_exact_causal_identity() {
    use bm_core::memory::*;
    for (transition, after_state) in [("mask", "masked"), ("delete_raw", "raw_deleted")] {
        let operation = MemoryMutationOperationIdentity::new(
            "real-transcript-control",
            "space-a",
            "agent-a",
            "governor-a",
            MemoryMutationOperationKind::ProceduralLifecycle,
        )
        .unwrap();
        let wire = serde_json::json!({
            "scope": {"memory_space_id":"space-a", "mounted_subject_id":"agent-a"},
            "target_epoch":2,
            "trigger": {"kind":"transcript_lifecycle", "transaction_id":"real-transcript-tx", "operation":operation,
                "key":{"memory_space_id":"space-a", "channel_id":"desktop", "conversation_id":"conversation-a"},
                "turn_id":"turn-a", "sequence":9, "transition":transition,
                "before_state":"active", "after_state":after_state,
                "before_content_digest":digest('a'), "after_content_digest":digest('b')}
        });
        let work: ProceduralReconciliationWorkV1 = serde_json::from_value(wire.clone())
            .expect("real transcript withdrawal must have a typed trigger");
        assert!(work.reference().is_ok());
        for (field, value) in [
            ("sequence", serde_json::json!(0)),
            ("transition", serde_json::json!("archive")),
            ("after_state", serde_json::json!("active")),
            ("before_state", serde_json::json!("raw_deleted")),
            ("after_content_digest", serde_json::json!(digest('a'))),
        ] {
            let mut invalid = wire.clone();
            invalid["trigger"][field] = value;
            assert!(
                serde_json::from_value::<ProceduralReconciliationWorkV1>(invalid)
                    .unwrap()
                    .reference()
                    .is_err()
            );
        }
        let mut foreign = wire.clone();
        foreign["scope"]["mounted_subject_id"] = serde_json::json!("agent-b");
        assert!(
            serde_json::from_value::<ProceduralReconciliationWorkV1>(foreign)
                .unwrap()
                .reference()
                .is_err()
        );
    }
}

#[test]
fn pfi2_reconciliation_source_operation_is_real_and_plane_bound() {
    use bm_core::memory::*;
    let wire = serde_json::json!({
        "scope":{"memory_space_id":"space-a", "mounted_subject_id":"agent-b"}, "target_epoch":2,
        "trigger":{"kind":"source_change", "transaction_id":"real-source-tx", "changes":[pfi2_source_deletion()],
            "operation":MemoryMutationOperationIdentity::new("real-source-control", "space-a", "agent-a", "human-a",
                MemoryMutationOperationKind::LongTermControl {operation:LongTermControlOperation::Delete}).unwrap()}
    });
    let work: ProceduralReconciliationWorkV1 =
        serde_json::from_value(wire.clone()).expect("source MOR must be preserved");
    assert!(
        work.reference().is_ok(),
        "shared source control may affect another subject in the same space"
    );
    for (space, kind) in [
        ("space-b", MemoryMutationOperationKind::Write),
        ("space-a", MemoryMutationOperationKind::SoulDelete),
    ] {
        let mut invalid = wire.clone();
        invalid["trigger"]["operation"] = serde_json::to_value(
            MemoryMutationOperationIdentity::new(
                "wrong-operation",
                space,
                "agent-a",
                "human-a",
                kind,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            serde_json::from_value::<ProceduralReconciliationWorkV1>(invalid)
                .unwrap()
                .reference()
                .is_err()
        );
    }
}

#[test]
fn pfi2_producer_identity_scope_and_each_claim_are_independent() {
    use bm_core::memory::*;
    let binding = pfi2_producer();
    let feedback = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    assert!(binding.validate_contract());
    assert!(binding
        .authorize_tool_feedback(&binding.spec.principal, &binding.spec.scope, &feedback)
        .is_ok());
    let other = ProceduralProducerPrincipalV1::LocalCapability {
        capability_id: "executor-b".into(),
    };
    assert_eq!(
        binding.authorize_tool_feedback(&other, &binding.spec.scope, &feedback),
        Err(ProceduralProducerErrorV1::AuthorityMismatch)
    );
    let mut other_scope = binding.spec.scope.clone();
    other_scope.mounted_subject_id = "agent-b".into();
    assert_eq!(
        binding.authorize_tool_feedback(&binding.spec.principal, &other_scope, &feedback),
        Err(ProceduralProducerErrorV1::ScopeMismatch)
    );
    let mut narrowed = binding.spec.clone();
    narrowed.claims.method_declarations = false;
    let current = ProceduralProducerBindingV1::build(
        narrowed,
        ProceduralProducerStateV1::Active,
        producer_operation("grant-2"),
        101,
        Some(&binding),
    )
    .unwrap();
    assert_eq!(
        current.authorize_tool_feedback(&binding.spec.principal, &binding.spec.scope, &feedback),
        Err(ProceduralProducerErrorV1::ClaimForbidden)
    );
    let mut facts_only = feedback.clone();
    facts_only.method_evidence.clear();
    assert!(current
        .authorize_tool_feedback(&binding.spec.principal, &binding.spec.scope, &facts_only)
        .is_ok());
    let mut public_only = binding.spec.clone();
    public_only.claims.source_classifications = vec![ProceduralSourceSensitivity::NonPrivate];
    let public_only = ProceduralProducerBindingV1::build(
        public_only,
        ProceduralProducerStateV1::Active,
        producer_operation("grant-3"),
        101,
        Some(&binding),
    )
    .unwrap();
    assert_eq!(
        public_only.authorize_tool_feedback(
            &binding.spec.principal,
            &binding.spec.scope,
            &pfi2_input(ProceduralSourceSensitivity::Private)
        ),
        Err(ProceduralProducerErrorV1::SourceClassificationForbidden)
    );
}

#[test]
fn pfi2_producer_revisions_preserve_provenance_and_revocation_is_not_reprovisioning() {
    use bm_core::memory::*;
    let binding = pfi2_producer();
    let revoked = ProceduralProducerBindingV1::build(
        binding.spec.clone(),
        ProceduralProducerStateV1::Revoked,
        producer_operation("grant-4"),
        101,
        Some(&binding),
    )
    .unwrap();
    assert_eq!(revoked.revision, 2);
    assert_eq!(revoked.predecessor, Some(binding.revision_ref().unwrap()));
    assert_eq!(
        revoked.spec.binding_key().unwrap(),
        binding.spec.binding_key().unwrap()
    );
    assert!(revoked.validate_contract());
    assert_eq!(
        revoked.authorize_tool_feedback(
            &binding.spec.principal,
            &binding.spec.scope,
            &pfi2_input(ProceduralSourceSensitivity::NonPrivate)
        ),
        Err(ProceduralProducerErrorV1::Revoked)
    );
    assert_eq!(
        ProceduralProducerBindingV1::build(
            binding.spec.clone(),
            ProceduralProducerStateV1::Active,
            producer_operation("grant-5"),
            102,
            Some(&revoked)
        ),
        Err(ProceduralProducerErrorV1::Revoked)
    );
    let mut forged = binding.spec.clone();
    forged.source_authority = ProceduralProducerSourceAuthorityV1::HumanUser {
        subject_id: "human-a".into(),
    };
    assert_eq!(
        ProceduralProducerBindingV1::build(
            forged,
            ProceduralProducerStateV1::Active,
            producer_operation("grant-6"),
            101,
            Some(&binding)
        ),
        Err(ProceduralProducerErrorV1::RevisionConflict)
    );
    let mut model_witness = binding.spec.clone();
    model_witness.source_authority = ProceduralProducerSourceAuthorityV1::ModelInferred {
        subject_id: "agent-a".into(),
    };
    assert!(
        !model_witness.validate_contract(),
        "model inference cannot be an execution witness"
    );
    let mut broken = binding.clone();
    broken.revision += 1;
    assert!(!broken.validate_contract());
    let restored: ProceduralProducerBindingV1 =
        serde_json::from_slice(&serde_json::to_vec(&revoked).unwrap()).unwrap();
    assert_eq!(restored, revoked);
    assert!(restored.validate_contract());
}

#[test]
fn pfi2_usage_claims_require_execution_witness_authority() {
    use bm_core::memory::*;
    for source in [
        ProceduralProducerSourceAuthorityV1::GovernedSource {
            source_revision: GovernedOwnerRevisionRef::try_new(
                GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::EvidenceDocument, "source-a"),
                1,
            )
            .unwrap(),
        },
        ProceduralProducerSourceAuthorityV1::ModelInferred {
            subject_id: "agent-a".into(),
        },
        ProceduralProducerSourceAuthorityV1::RuntimeObservation,
        ProceduralProducerSourceAuthorityV1::HumanUser {
            subject_id: "human-a".into(),
        },
    ] {
        let mut spec = pfi2_producer().spec;
        spec.source_authority = source.clone();
        spec.claims.execution_facts = false;
        assert!(spec.validate_contract(), "method declarations remain valid");
        spec.claims.usage_feedback = true;
        assert_eq!(
            spec.validate_contract(),
            source.can_witness_execution(),
            "method provenance must not grant execution usage authority: {source:?}"
        );
    }
}

#[test]
fn pfi2_producer_head_requires_complete_immutable_material_closure() {
    use bm_core::memory::*;
    let first = pfi2_producer();
    let first_head = ProceduralProducerHeadV1::advance(&first, None).unwrap();
    assert!(first_head.validates_materials(std::slice::from_ref(&first)));
    let second = ProceduralProducerBindingV1::build(
        first.spec.clone(),
        ProceduralProducerStateV1::Revoked,
        producer_operation("grant-7"),
        101,
        Some(&first),
    )
    .unwrap();
    let head = ProceduralProducerHeadV1::advance(&second, Some(&first_head)).unwrap();
    assert!(head.validates_materials(&[first.clone(), second.clone()]));
    assert!(
        !head.validates_materials(std::slice::from_ref(&second)),
        "missing old material cannot be treated as empty history"
    );
    assert_eq!(
        ProceduralProducerHeadV1::advance(&second, None),
        Err(ProceduralProducerErrorV1::RepairRequired)
    );
    let mut forged = second.clone();
    forged.spec.principal = ProceduralProducerPrincipalV1::LocalCapability {
        capability_id: "other-executor".into(),
    };
    forged.content_digest = forged.canonical_digest().unwrap();
    let forged_head = ProceduralProducerHeadV1::advance(&forged, Some(&first_head)).unwrap();
    assert!(
        !forged_head.validates_materials(&[first, forged]),
        "a resealed chain cannot replace its authenticated principal"
    );
}

#[test]
fn pfi2_producer_revision_retains_its_exact_durable_operation_identity() {
    let producer = pfi2_producer();
    let wire = serde_json::to_value(&producer).unwrap();
    assert!(
        wire.get("operation_identity").is_some(),
        "a grant revision must locate its authoritative receipt/audit without scanning or guessing"
    );
}

#[test]
fn pfi2_retained_evidence_is_intersected_with_current_producer_authority() {
    use bm_core::memory::*;
    let producer = pfi2_producer();
    let evidence =
        tool_learning_evidence("1. Inspect the manifest.\n2. Extract into the workspace.");
    let check = |current: &ProceduralProducerBindingV1, evidence: &PostTurnLearningEvidenceV2| {
        producer.authorize_evidence_with_current(current, &producer.spec.scope, evidence)
    };
    assert_eq!(check(&producer, &evidence), Ok(()));
    let mut rotated_spec = producer.spec.clone();
    rotated_spec.source_config_ref = "rotated-host-config".into();
    let rotated = ProceduralProducerBindingV1::build(
        rotated_spec,
        ProceduralProducerStateV1::Active,
        producer_operation("rotate-current"),
        101,
        Some(&producer),
    )
    .unwrap();
    assert_eq!(
        check(&rotated, &evidence),
        Ok(()),
        "rotation must not erase valid historical evidence"
    );
    let mut narrowed_spec = rotated.spec.clone();
    narrowed_spec.claims.method_declarations = false;
    let narrowed = ProceduralProducerBindingV1::build(
        narrowed_spec,
        ProceduralProducerStateV1::Active,
        producer_operation("narrow-current"),
        102,
        Some(&rotated),
    )
    .unwrap();
    assert_eq!(
        check(&narrowed, &evidence),
        Err(ProceduralProducerErrorV1::ClaimForbidden)
    );
    let revoked = ProceduralProducerBindingV1::build(
        rotated.spec.clone(),
        ProceduralProducerStateV1::Revoked,
        producer_operation("revoke-current"),
        102,
        Some(&rotated),
    )
    .unwrap();
    assert_eq!(
        check(&revoked, &evidence),
        Err(ProceduralProducerErrorV1::Revoked)
    );
    let mut forged = evidence.clone();
    if let ProceduralFeedbackAuthorityV2::Producer {
        source_authority, ..
    } = &mut forged.authority
    {
        *source_authority = ProceduralProducerSourceAuthorityV1::HumanUser {
            subject_id: "human-a".into(),
        };
    }
    forged.learning_evidence_digest = forged.canonical_digest().unwrap();
    assert!(forged.validate_contract());
    assert_eq!(
        check(&producer, &forged),
        Err(ProceduralProducerErrorV1::AuthorityMismatch)
    );
}

#[test]
fn pfi2_execution_reduction_is_exact_deduplicated_and_scope_bound() {
    use bm_core::memory::*;
    let producer = pfi2_producer();
    let tool = producer.spec.tools[0].clone();
    let source = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "agent-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .unwrap();
    let first = ToolExecutionContributionV1::build(
        source.clone(),
        digest('f'),
        producer.revision_ref().unwrap(),
        tool.clone(),
        pfi2_input(ProceduralSourceSensitivity::Private)
            .execution_facts
            .remove(0),
    )
    .unwrap();
    assert!(first.reference().unwrap().validate_contract());
    let once = reduce_tool_execution_contributions(
        &producer.spec.scope,
        &tool,
        std::slice::from_ref(&first),
        16,
    )
    .unwrap();
    let retried = reduce_tool_execution_contributions(
        &producer.spec.scope,
        &tool,
        &[first.clone(), first.clone()],
        16,
    )
    .unwrap();
    assert_eq!(once, retried);
    let mut values = vec![first.clone()];
    for (index, outcome) in [
        ToolExecutionOutcome::Failed,
        ToolExecutionOutcome::Rejected,
        ToolExecutionOutcome::NotExecuted,
        ToolExecutionOutcome::Cancelled,
        ToolExecutionOutcome::Partial,
        ToolExecutionOutcome::Unknown,
    ]
    .into_iter()
    .enumerate()
    {
        let mut fact = first.fact.clone();
        fact.observation_id = format!("observation-{index}");
        fact.call_id = format!("actual-new-call-{index}");
        fact.outcome = outcome;
        values.push(
            ToolExecutionContributionV1::build(
                source.clone(),
                digest('f'),
                producer.revision_ref().unwrap(),
                tool.clone(),
                fact,
            )
            .unwrap(),
        );
    }
    let counts =
        reduce_tool_execution_contributions(&producer.spec.scope, &tool, &values, 16).unwrap();
    assert_eq!(counts.execution_success_sample(), Some((1, 2)));
    assert_eq!(
        (
            counts.failed,
            counts.rejected,
            counts.not_executed,
            counts.cancelled,
            counts.partial,
            counts.unknown
        ),
        (1, 1, 1, 1, 1, 1)
    );
    let mut changed_fact = first.fact.clone();
    changed_fact.outcome = ToolExecutionOutcome::Failed;
    let conflicting = ToolExecutionContributionV1::build(
        source,
        digest('f'),
        producer.revision_ref().unwrap(),
        tool.clone(),
        changed_fact,
    )
    .unwrap();
    assert_eq!(first.contribution_id, conflicting.contribution_id);
    assert_eq!(
        reduce_tool_execution_contributions(
            &producer.spec.scope,
            &tool,
            &[first.clone(), conflicting],
            16
        ),
        Err(ProceduralContributionErrorV1::IdentityConflict)
    );
    let mut other = producer.spec.scope.clone();
    other.mounted_subject_id = "agent-b".into();
    assert_eq!(
        reduce_tool_execution_contributions(&other, &tool, &values, 16),
        Err(ProceduralContributionErrorV1::ScopeMismatch)
    );
    assert_eq!(
        reduce_tool_execution_contributions(&producer.spec.scope, &tool, &values, 1),
        Err(ProceduralContributionErrorV1::CapacityBlocked)
    );
    let mut corrupt = first;
    corrupt.fact.outcome = ToolExecutionOutcome::Unknown;
    assert!(!corrupt.validate_contract());
}

#[test]
fn pfi2_experience_body_separates_counts_from_exact_method_sources() {
    use bm_core::memory::*;
    use bm_core::skills::{
        AgentToolExperienceBodyV1, AgentToolExperienceFocusV1, AgentToolMethodSourceRefV1,
    };
    let producer = pfi2_producer();
    let input = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    let contribution = ToolExecutionContributionV1::build(
        ProceduralFeedbackIdentityV1::new(
            "space-a",
            "agent-a",
            "sdk.direct",
            "chat-a",
            "conversation-a",
            "turn-a",
        )
        .unwrap(),
        digest('f'),
        producer.revision_ref().unwrap(),
        producer.spec.tools[0].clone(),
        input.execution_facts[0].clone(),
    )
    .unwrap();
    let counts = reduce_tool_execution_contributions(
        &producer.spec.scope,
        &producer.spec.tools[0],
        std::slice::from_ref(&contribution),
        16,
    )
    .unwrap();
    let reference = contribution.reference().unwrap();
    let execution = AgentToolExperienceBodyV1::Execution {
        counts,
        contributions: vec![reference.clone()],
    };
    assert!(execution.validate_contract());
    assert_eq!(execution.focus(), AgentToolExperienceFocusV1::Execution);
    assert_eq!(execution.method_content_digest().unwrap(), None);
    let mut wire = serde_json::to_value(&execution).unwrap();
    for field in [
        "task_signature",
        "procedure",
        "usage_guidance",
        "confidence",
    ] {
        assert!(wire.get(field).is_none());
    }
    wire["procedure"] = serde_json::json!("synthetic-smuggled-method");
    assert!(serde_json::from_value::<AgentToolExperienceBodyV1>(wire).is_err());
    let method = &input.method_evidence[0];
    let mut body = AgentToolExperienceBodyV1::Method {
        task_signature: method.task_signature.clone(),
        procedure: method.body.clone(),
        constraints: Vec::new(),
        sources: Vec::new(),
    };
    let method_digest = body.method_content_digest().unwrap().unwrap();
    if let AgentToolExperienceBodyV1::Method { sources, .. } = &mut body {
        sources.push(AgentToolMethodSourceRefV1 {
            source_job_id: reference.source_job_id.clone(),
            method_id: method.method_id.clone(),
            method_digest,
            execution_refs: vec![reference],
        });
    }
    assert!(body.validate_contract());
    let AgentToolExperienceFocusV1::Method { task_signature } = body.focus() else {
        panic!("method focus");
    };
    assert_eq!(task_signature, method.task_signature);
    if let AgentToolExperienceBodyV1::Method { procedure, .. } = &mut body {
        procedure.push_str("\n3. Verify a different extraction target.");
    }
    assert!(
        !body.validate_contract(),
        "another method cannot inherit the prior method's execution sources"
    );
}

#[test]
fn pfi2_private_and_unknown_sources_preserve_facts_without_method_payload() {
    use bm_core::memory::*;
    let positive = pfi2_input(ProceduralSourceSensitivity::NonPrivate)
        .admit_content()
        .unwrap();
    assert!(positive.validate_contract());
    assert!(!positive.method_evidence.is_empty());
    for (sensitivity, reason) in [
        (
            ProceduralSourceSensitivity::Private,
            ProceduralEvidenceRejection::PrivateMethod,
        ),
        (
            ProceduralSourceSensitivity::Unknown,
            ProceduralEvidenceRejection::UnknownMethodSource,
        ),
    ] {
        let mut input = pfi2_input(sensitivity);
        input.method_evidence[0].method_id = "synthetic-private-id-marker".into();
        input.method_evidence[0].body = "synthetic-private-body-marker".into();
        let admitted = input.admit_content().unwrap();
        assert!(admitted.validate_contract());
        assert_eq!(admitted.execution_facts, input.execution_facts);
        assert!(admitted.method_evidence.is_empty());
        assert_eq!(admitted.rejected_methods[0].rejection, reason);
        let encoded = serde_json::to_string(&admitted).unwrap();
        assert!(!encoded.contains("synthetic-private"));
        let restored: AdmittedAgentToolEvidenceV1 = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            restored.canonical_digest().unwrap(),
            admitted.canonical_digest().unwrap()
        );
        assert_eq!(restored, admitted);
    }
}

#[test]
fn pfi2_method_only_privacy_rejection_has_a_valid_empty_admitted_image() {
    use bm_core::memory::*;
    let mut input = pfi2_input(ProceduralSourceSensitivity::Private);
    input.execution_facts.clear();
    input.method_evidence[0].execution_refs.clear();
    input.method_evidence[0].source_sensitivity = ProceduralSourceSensitivity::Private;
    let admitted = input.admit_content().unwrap();
    assert!(admitted.execution_facts.is_empty());
    assert!(admitted.method_evidence.is_empty());
    assert!(!admitted.rejected_methods.is_empty());
    assert!(
        admitted.validate_contract(),
        "a legitimate rejected declaration is not a malformed request"
    );
}

#[test]
fn pfi2_conflicting_methods_have_no_input_order_winner_or_execution_loss() {
    use bm_core::memory::*;
    let first = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    let mut second = first.clone();
    second.execution_facts[0].observation_id = "observation-b".into();
    second.execution_facts[0].call_id = "call-b".into();
    second.method_evidence[0].method_id = "method-b".into();
    second.method_evidence[0].execution_refs = vec!["observation-b".into()];
    second.method_evidence[0].body = "1. List archive entries and validate paths.\n2. Extract into a temporary directory.\n3. Verify checksums before moving output.".into();
    let canonical = [&first, &second]
        .into_iter()
        .map(|input| ToolObservationDigest {
            observation_id: input.execution_facts[0].observation_id.clone(),
            call_id: input.execution_facts[0].call_id.clone(),
            tool_name: input.tool_id.clone(),
            summary: "Independent tool result".into(),
            external_content: false,
        })
        .collect::<Vec<_>>();
    for inputs in [
        vec![first.clone(), second.clone()],
        vec![second.clone(), first.clone()],
    ] {
        let admitted = admit_agent_tool_feedback_for_turn(&inputs, &canonical, false, 100).unwrap();
        for (index, group) in admitted.iter().enumerate() {
            assert_eq!(group.execution_facts, inputs[index].execution_facts);
            assert!(
                group.method_evidence.is_empty(),
                "conflicting methods must both be rejected, not select an array-order winner"
            );
            assert_eq!(
                group.rejected_methods,
                vec![ProceduralMethodDispositionV1 {
                    input_index: 0,
                    rejection: ProceduralEvidenceRejection::MethodOwnerContentConflict
                }]
            );
            assert!(group.validate_contract());
            assert!(!serde_json::to_string(group)
                .unwrap()
                .contains(&inputs[index].method_evidence[0].body));
        }
        let mut evidence = tool_learning_evidence(&inputs[0].method_evidence[0].body);
        evidence.tool_call_count = 2;
        evidence.agent_tool_feedback = admitted;
        evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
        assert!(
            evidence.validate_contract(),
            "privacy filtering must retain both input group orderings without payload sorting"
        );
    }
    second.method_evidence[0].task_signature = "different-task".into();
    let positive =
        admit_agent_tool_feedback_for_turn(&[first, second], &canonical, false, 100).unwrap();
    assert!(positive
        .iter()
        .all(|group| group.method_evidence.len() == 1 && group.rejected_methods.is_empty()));
}

#[test]
fn pfi2_method_quality_rejection_precedes_the_durable_image() {
    use bm_core::memory::*;
    let input = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    assert!(!input.admit_content().unwrap().method_evidence.is_empty());
    for body in [
        "synthetic-one-off-result-only",
        r#"{"raw_tool_payload":"synthetic-private-result"}"#,
    ] {
        let mut invalid_method = input.clone();
        invalid_method.method_evidence[0].body = body.into();
        let admitted = invalid_method.admit_content().unwrap();
        assert!(!admitted.execution_facts.is_empty());
        assert!(
            admitted.method_evidence.is_empty(),
            "weak or raw-payload-shaped text must not enter durable learning"
        );
        assert!(!admitted.rejected_methods.is_empty());
        assert!(admitted.validate_contract());
        assert!(!serde_json::to_string(&admitted)
            .unwrap()
            .contains("synthetic-"));
    }
}

#[test]
fn pfi2_invalid_references_duplicates_and_metadata_smuggling_fail_closed() {
    use bm_core::memory::*;
    let input = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    let mut invalid = input.clone();
    invalid.method_evidence[0].execution_refs = vec!["other-turn-observation".into()];
    assert_eq!(
        invalid.admit_content(),
        Err(ProceduralEvidenceRejection::ExecutionReferenceInvalid)
    );
    invalid = input.clone();
    invalid
        .execution_facts
        .push(invalid.execution_facts[0].clone());
    assert_eq!(
        invalid.admit_content(),
        Err(ProceduralEvidenceRejection::NonCanonical)
    );
    for forbidden in [
        "summary",
        "error_code",
        "private_content_used",
        "query_hash",
        "result_hash",
        "producer",
        "authority",
    ] {
        let mut value = serde_json::to_value(&input.execution_facts[0]).unwrap();
        value[forbidden] = serde_json::json!("synthetic-smuggled-value");
        assert!(
            serde_json::from_value::<ToolExecutionFactV1>(value).is_err(),
            "{forbidden}"
        );
    }
    let mut missing = serde_json::to_value(&input.execution_facts[0]).unwrap();
    missing
        .as_object_mut()
        .unwrap()
        .remove("source_sensitivity");
    assert!(serde_json::from_value::<ToolExecutionFactV1>(missing).is_err());
}

#[test]
fn pfi2_execution_sample_excludes_uncompleted_states_without_method_confidence() {
    use bm_core::memory::*;
    let mut counts = ToolExecutionCountsV1::default();
    for outcome in [
        ToolExecutionOutcome::Rejected,
        ToolExecutionOutcome::NotExecuted,
        ToolExecutionOutcome::Cancelled,
        ToolExecutionOutcome::Partial,
        ToolExecutionOutcome::Unknown,
    ] {
        counts.record(outcome).unwrap();
    }
    assert_eq!(counts.execution_success_sample(), None);
    counts.record(ToolExecutionOutcome::Failed).unwrap();
    assert_eq!(counts.execution_success_sample(), Some((0, 1)));
    counts.record(ToolExecutionOutcome::Succeeded).unwrap();
    assert_eq!(counts.execution_success_sample(), Some((1, 2)));
    let before = counts.clone();
    counts.succeeded = u64::MAX;
    assert!(counts.record(ToolExecutionOutcome::Succeeded).is_none());
    assert_eq!(counts.succeeded, u64::MAX);
    assert_eq!(counts.failed, before.failed);
}

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn tool_learning_evidence(summary: &str) -> PostTurnLearningEvidenceV2 {
    use bm_core::memory::*;
    let mut input = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    input.method_evidence[0].body = summary.into();
    // Deliberately form a post-image directly so malformed-image tests exercise
    // the durable validator, independently of input admission.
    let admitted = AdmittedAgentToolEvidenceV1 {
        registry_ref: input.registry_ref,
        tool_id: input.tool_id,
        schema_fingerprint: input.schema_fingerprint,
        execution_facts: input.execution_facts,
        submitted_method_count: 1,
        method_evidence: input.method_evidence,
        rejected_methods: Vec::new(),
    };
    let mut evidence = PostTurnLearningEvidenceV2 {
        schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
        conversation_id: "conversation-a".into(),
        turn_id: "turn-a".into(),
        canonical_turn_digest: digest('c'),
        tool_call_count: 1,
        selection_receipt: None,
        runtime_skill_feedback: vec![],
        agent_skill_feedback: vec![],
        task_learning_feedback: vec![],
        agent_tool_feedback: vec![admitted],
        authority: runtime_authority(),
        learning_evidence_digest: String::new(),
    };
    evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
    evidence
}

fn runtime_authority() -> ProceduralFeedbackAuthorityV2 {
    ProceduralFeedbackAuthorityV2::Producer {
        producer_revision: pfi2_producer().revision_ref().unwrap(),
        source_authority: bm_core::memory::ProceduralProducerSourceAuthorityV1::RuntimeObservation,
        confirmation: None,
    }
}

fn human_authority(operation: &str) -> ProceduralFeedbackAuthorityV2 {
    use bm_core::memory::*;
    let mut spec = pfi2_producer().spec;
    spec.binding_id = "human-witness-a".into();
    spec.source_authority = ProceduralProducerSourceAuthorityV1::HumanUser {
        subject_id: "human-a".into(),
    };
    let binding = ProceduralProducerBindingV1::build(
        spec,
        ProceduralProducerStateV1::Active,
        producer_operation("grant-8"),
        100,
        None,
    )
    .unwrap();
    ProceduralFeedbackAuthorityV2::Producer {
        producer_revision: binding.revision_ref().unwrap(),
        source_authority: binding.spec.source_authority,
        confirmation: Some(ProceduralHumanConfirmationV1 {
            actor_subject_id: "human-a".into(),
            operation_id: operation.into(),
            evidence_digest: digest('d'),
        }),
    }
}

#[test]
fn procedural_canonical_text_accepts_multiline_method_without_rewriting_digest() {
    let method = "1. Inspect the archive before extraction.\n2. Extract into an empty directory.\n3. Verify the extracted manifest.";
    let evidence = tool_learning_evidence(method);
    assert!(
        evidence.validate_contract(),
        "canonical method text must permit LF while preserving the actual observation"
    );
    assert_eq!(
        evidence.agent_tool_feedback[0].method_evidence[0].body,
        method
    );
    assert_eq!(
        evidence.learning_evidence_digest,
        evidence.canonical_digest().unwrap()
    );
    assert_ne!(
        evidence.learning_evidence_digest,
        tool_learning_evidence(&method.replace('\n', " ")).learning_evidence_digest,
        "canonical admission must not silently flatten the observed method"
    );
}

#[test]
fn procedural_canonical_text_preserves_identity_and_control_character_rejection() {
    assert!(
        tool_learning_evidence("1. Inspect the archive.\n2. Extract the archive.")
            .validate_contract()
    );
    for invalid in [
        "step one\r\nstep two",
        "step one\rstep two",
        "step one\tstep two",
        "step one\0step two",
        "step one\u{1b}step two",
        "step one\u{7f}step two",
    ] {
        assert!(
            !tool_learning_evidence(invalid).validate_contract(),
            "non-LF control characters must not enter canonical text: {invalid:?}"
        );
    }
    let mut evidence = tool_learning_evidence("1. Inspect the archive.\n2. Extract the archive.");
    evidence.agent_tool_feedback[0].method_evidence[0].task_signature = "archive\ntask".into();
    evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
    assert!(
        !evidence.validate_contract(),
        "LF is text-only and must never become canonical identity syntax"
    );
}

#[test]
fn applicability_is_canonical_and_enforces_exact_registry_scope() {
    let context = ProceduralApplicabilityContextV1::try_new(
        Some("project-a".to_string()),
        Some("workspace-a".to_string()),
        Some("conversation-a".to_string()),
    )
    .expect("context");
    assert!(context.validate_contract());
    assert!(
        context.permits_registry_scope(&AgentToolRegistryScope::Project {
            project_id: "project-a".to_string(),
        })
    );
    assert!(
        !context.permits_registry_scope(&AgentToolRegistryScope::Project {
            project_id: "project-b".to_string(),
        })
    );
    assert!(
        ProceduralApplicabilityContextV1::try_new(Some(" project-a".to_string()), None, None,)
            .is_err()
    );
}

#[test]
fn selection_receipt_binds_projection_scope_and_delivery() {
    let applicability =
        ProceduralApplicabilityContextV1::try_new(None, None, Some("conversation-a".to_string()))
            .expect("context");
    let mut receipt = ProceduralSelectionReceiptV1 {
        schema_version: PROCEDURAL_SELECTION_RECEIPT_SCHEMA_VERSION,
        receipt_ref: String::new(),
        signing_key_id: "selection-key-v1".to_string(),
        issued_at: 100,
        expires_at: 200,
        identity: ProceduralProjectionIdentityV1 {
            projection_id: "projection-a".to_string(),
            memory_space_id: "space-a".to_string(),
            mounted_subject_id: "subject-a".to_string(),
            channel: "sdk.direct".to_string(),
            chat_id: "chat-a".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: "turn-a".to_string(),
        },
        applicability,
        standard_agent_skills: Vec::new(),
        runtime_skills: Vec::new(),
        task_learnings: Vec::new(),
        agent_tool_experiences: Vec::new(),
        selection_digest: String::new(),
        delivery_digest: digest('a'),
        authority_tag: digest('b'),
    };
    receipt.selection_digest = receipt
        .canonical_selection_digest()
        .expect("selection digest");
    receipt.receipt_ref = receipt.canonical_receipt_ref().expect("receipt ref");
    assert!(receipt.validate_contract());

    receipt.identity.mounted_subject_id = "subject-b".to_string();
    assert!(!receipt.validate_contract());
}

#[test]
fn learning_evidence_has_typed_confirmation_and_digest_identity() {
    let mut evidence = tool_learning_evidence(
        &pfi2_input(bm_core::memory::ProceduralSourceSensitivity::NonPrivate).method_evidence[0]
            .body,
    );
    assert!(evidence.validate_contract());
    assert!(!evidence.authority.is_human_confirmation());
    evidence.authority = human_authority("confirm-turn-a");
    let payload_digest = evidence.canonical_confirmation_payload_digest().unwrap();
    if let ProceduralFeedbackAuthorityV2::Producer {
        confirmation: Some(confirmation),
        ..
    } = &mut evidence.authority
    {
        confirmation.evidence_digest = payload_digest;
    }
    evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
    assert!(evidence.validate_contract());
    assert!(evidence.authority.is_human_confirmation());
}

#[test]
fn confirmation_cannot_claim_an_unrelated_evidence_digest() {
    let mut evidence = tool_learning_evidence(
        &pfi2_input(bm_core::memory::ProceduralSourceSensitivity::NonPrivate).method_evidence[0]
            .body,
    );
    assert!(evidence.validate_contract());
    evidence.authority = human_authority("confirm-a");
    evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
    assert!(
        !evidence.validate_contract(),
        "confirmation must bind the exact producer and payload"
    );
}

#[test]
fn standard_skill_feedback_requires_the_actual_selection_receipt() {
    let mut evidence = PostTurnLearningEvidenceV2 {
        schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
        memory_space_id: "space-a".into(),
        mounted_subject_id: "subject-a".into(),
        conversation_id: "conversation-a".into(),
        turn_id: "turn-a".into(),
        canonical_turn_digest: digest('c'),
        tool_call_count: 0,
        selection_receipt: None,
        runtime_skill_feedback: vec![],
        agent_skill_feedback: vec![bm_core::memory::AgentSkillUsageFeedbackV1 {
            package_id: "package-a".into(),
            package_fingerprint: "fingerprint-a".into(),
            package_binding: digest('e'),
            outcome: bm_core::memory::ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "observation-a".into(),
        }],
        task_learning_feedback: vec![],
        agent_tool_feedback: vec![],
        authority: runtime_authority(),
        learning_evidence_digest: String::new(),
    };
    evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
    assert!(
        !evidence.validate_contract(),
        "unselected package feedback must fail closed"
    );
}

#[test]
fn procedural_job_identity_is_subject_and_evidence_exact() {
    let identity = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "subject-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .expect("identity");
    let job =
        ProceduralFeedbackJobV2::pending(identity.clone(), 1, digest('1'), digest('2'), 3, 5, 100)
            .expect("pending job");
    assert_eq!(job.status, ProceduralFeedbackJobStatusV1::Pending);
    assert!(job.validate().is_ok());

    let other_subject = ProceduralFeedbackJobV2::pending(
        ProceduralFeedbackIdentityV1::new(
            "space-a",
            "subject-b",
            "sdk.direct",
            "chat-a",
            "conversation-a",
            "turn-a",
        )
        .expect("other identity"),
        1,
        digest('1'),
        digest('2'),
        3,
        5,
        100,
    )
    .expect("other job");
    assert_ne!(job.job_id, other_subject.job_id);

    let other_evidence =
        ProceduralFeedbackJobV2::pending(identity, 1, digest('1'), digest('3'), 3, 5, 100)
            .expect("other evidence job");
    assert_ne!(job.job_id, other_evidence.job_id);
}

#[test]
fn pfi2_reconciliation_uses_the_durable_job_without_a_fabricated_turn() {
    use bm_core::memory::*;
    let work = ProceduralReconciliationWorkV1 {
        scope: ProceduralSubjectScopeV1 {
            memory_space_id: "space-a".into(),
            mounted_subject_id: "agent-a".into(),
        },
        target_epoch: 2,
        trigger: ProceduralReconciliationTriggerV1::SourceChange {
            transaction_id: "source-delete-transaction".into(),
            operation: None,
            changes: vec![pfi2_source_deletion()],
        },
    };
    let reference = work.reference().unwrap();
    let wire = serde_json::json!({
        "schema_version": 2, "job_id": reference.job_id, "idempotency_key": reference.job_id,
        "discovery_root_key": reference.subject_root_key,
        "work": {"kind": "reconcile", "source": work}, "checkpoint": null, "checkpoint_authority": null,
        "status": "pending", "state_revision": 1, "attempt_count": 0, "max_attempts": 5,
        "next_attempt_at": 100, "lease_owner": null, "lease_until": null, "lease_epoch": 0,
        "last_error_class": null, "receipt": null, "created_at": 100, "updated_at": 100, "terminal_at": null
    });
    let job: ProceduralFeedbackJobV2 = serde_json::from_value(wire.clone())
        .expect("the existing durable lane must accept causal reconciliation without Transcript placeholders");
    job.validate().unwrap();
    assert_eq!(serde_json::to_value(&job).unwrap(), wire);
    for forbidden in [
        "identity",
        "transcript_sequence",
        "transcript_digest",
        "submitted_count",
    ] {
        let mut forged = wire.clone();
        forged["work"][forbidden] = serde_json::json!("invented-turn");
        assert!(serde_json::from_value::<ProceduralFeedbackJobV2>(forged).is_err());
    }
}

#[test]
fn pfi2_durable_job_rejects_half_present_leases() {
    let identity = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "subject-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .unwrap();
    let job =
        ProceduralFeedbackJobV2::pending(identity, 1, digest('1'), digest('2'), 1, 5, 100).unwrap();
    for owner_only in [true, false] {
        let mut broken = job.clone();
        if owner_only {
            broken.lease_owner = Some("worker-a".into());
        } else {
            broken.lease_until = Some(200);
        }
        assert!(
            broken.validate().is_err(),
            "a partial durable lease must fail closed"
        );
    }
}

#[test]
fn pfi2_durable_lane_recognizes_successful_page_continuation() {
    let status: bm_core::memory::ProceduralFeedbackJobStatusV1 = serde_json::from_str(
        "\"ready_to_continue\"",
    )
    .expect("successful bounded work must remain discoverable without being a failed retry");
    assert!(status.is_active());
    assert!(!status.is_terminal());
    let identity = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "subject-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .unwrap();
    let mut feedback =
        ProceduralFeedbackJobV2::pending(identity, 1, digest('1'), digest('2'), 1, 5, 100).unwrap();
    feedback.status = status;
    assert!(
        feedback.validate().is_err(),
        "turn feedback cannot impersonate checkpointed reconciliation"
    );
}

#[test]
fn pfi2_checkpointed_pages_preserve_attempt_budget_and_reject_old_leases_and_epochs() {
    use bm_core::memory::*;
    let producer = pfi2_producer();
    let scope = ProceduralSubjectScopeV1 {
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
    };
    let root =
        ProceduralSubjectValidityRootV1::initialize(scope.clone(), producer.spec.scope.clone(), 2)
            .unwrap();
    let work = ProceduralReconciliationWorkV1 {
        scope,
        target_epoch: 2,
        trigger: ProceduralReconciliationTriggerV1::SourceChange {
            transaction_id: "actual-source-removal".into(),
            operation: None,
            changes: vec![pfi2_source_deletion()],
        },
    };
    let root = root.begin(&work, 2).unwrap();
    let initial = ProceduralReconciliationCheckpointV1::begin(
        &work,
        &root,
        ProceduralReconciliationManifestPinsV1 {
            agent_tool: ProceduralReconciliationManifestPinV1::Absent,
            runtime_skill: ProceduralReconciliationManifestPinV1::Absent,
        },
        101,
    )
    .unwrap();
    let pending = ProceduralFeedbackJobV2::pending_work(
        ProceduralLearningWorkV1::Reconcile {
            source: work.clone(),
        },
        1,
        101,
    )
    .unwrap();
    let claimed = pending.claim("worker-a", 200, 102).unwrap();
    let mut first = initial;
    first.page_number = 1;
    first.cursor = ProceduralReconciliationCursorV1::RuntimeSkill { after: None };
    first.updated_at = 103;
    first.content_digest = first.canonical_digest().unwrap();
    let continued = claimed
        .continue_reconciliation(
            first.clone(),
            bm_core::memory::ProceduralReconciliationPageAuthorityV1::for_claim(
                &claimed,
                "subject-a",
            )
            .unwrap(),
            "worker-a",
            claimed.lease_epoch,
            103,
        )
        .unwrap();
    let reopened: ProceduralFeedbackJobV2 =
        serde_json::from_slice(&serde_json::to_vec(&continued).unwrap()).unwrap();
    let next = reopened
        .claim("worker-b", 200, 104)
        .expect("successful pages must not exhaust a one-attempt budget");
    assert_eq!(next.attempt_count, claimed.attempt_count);
    assert!(next.lease_epoch > claimed.lease_epoch);
    let mut complete = first;
    complete.page_number = 2;
    complete.cursor = ProceduralReconciliationCursorV1::Complete;
    complete.updated_at = 105;
    complete.content_digest = complete.canonical_digest().unwrap();
    assert!(next
        .continue_reconciliation(
            complete.clone(),
            bm_core::memory::ProceduralReconciliationPageAuthorityV1::for_claim(&next, "subject-a")
                .unwrap(),
            "worker-a",
            claimed.lease_epoch,
            105
        )
        .is_err());
    let ready = next
        .continue_reconciliation(
            complete.clone(),
            bm_core::memory::ProceduralReconciliationPageAuthorityV1::for_claim(&next, "subject-a")
                .unwrap(),
            "worker-b",
            next.lease_epoch,
            105,
        )
        .unwrap();
    assert_eq!(ready.attempt_count, 1);
    assert!(ready.claim("finalizer", 200, 106).is_ok());
    let mut additional_scope = producer.spec.scope.clone();
    additional_scope.chat_id = "second-chat".into();
    let changed_root = root.bind_scope(additional_scope, 2).unwrap();
    let fresh = ProceduralReconciliationCheckpointV1::begin(
        &work,
        &changed_root,
        complete.current_manifests.clone(),
        107,
    )
    .unwrap();
    let restarting = ready.claim("restart-worker", 200, 106).unwrap();
    assert!(
        restarting
            .continue_reconciliation(
                fresh.clone(),
                bm_core::memory::ProceduralReconciliationPageAuthorityV1::for_claim(
                    &restarting,
                    "subject-a"
                )
                .unwrap(),
                "restart-worker",
                restarting.lease_epoch,
                107
            )
            .is_err(),
        "ordinary page continuation must not erase previous read proofs"
    );
    assert!(restarting
        .restart_reconciliation_snapshot(fresh.clone(), "old-worker", restarting.lease_epoch, 107)
        .is_err());
    assert!(restarting
        .restart_reconciliation_snapshot(
            fresh.clone(),
            "restart-worker",
            restarting.lease_epoch - 1,
            107
        )
        .is_err());
    let restarted = restarting
        .restart_reconciliation_snapshot(
            fresh.clone(),
            "restart-worker",
            restarting.lease_epoch,
            107,
        )
        .unwrap();
    assert_eq!(restarted.checkpoint, Some(fresh));
    assert_eq!(restarted.attempt_count, 1);
    assert_eq!(restarted.work, restarting.work);
    let reclaimed = restarted.claim("resumed-worker", 200, 108).unwrap();
    assert_eq!(reclaimed.attempt_count, 1);
    assert!(reclaimed.lease_epoch > restarting.lease_epoch);
    let mut capacity_block = reclaimed.clone();
    capacity_block.status = ProceduralFeedbackJobStatusV1::BlockedCapacity;
    capacity_block.lease_owner = None;
    capacity_block.lease_until = None;
    capacity_block.last_error_class = Some(ProceduralFeedbackErrorClassV1::BudgetExceeded);
    capacity_block.updated_at = 109;
    capacity_block.validate().unwrap();
    assert!(!capacity_block.status.is_active() && !capacity_block.status.is_terminal());
    assert!(capacity_block.claim("implicit-resume", 200, 110).is_err());
    let blocked_root = changed_root
        .block(&work, ProceduralReconciliationBlockV1::Capacity, 2)
        .unwrap();
    let resumed_root = blocked_root.resume(&work, 2).unwrap();
    let resume_snapshot = ProceduralReconciliationCheckpointV1::begin(
        &work,
        &resumed_root,
        complete.current_manifests.clone(),
        110,
    )
    .unwrap();
    let resumed = capacity_block
        .resume_reconciliation_capacity(resume_snapshot, 110)
        .unwrap();
    assert_eq!(resumed.attempt_count, 1);
    assert!(resumed.claim("explicit-resumed-worker", 200, 111).is_ok());
    let mut other_epoch = work;
    other_epoch.target_epoch += 1;
    assert!(complete.validate_for(&other_epoch).is_err());
    let mut tampered = complete;
    tampered.root_revision += 1;
    assert!(tampered
        .validate_for(match &pending.work {
            ProceduralLearningWorkV1::Reconcile { source } => source,
            _ => unreachable!(),
        })
        .is_err());
}

#[test]
fn pfi2_checkpoint_visibility_is_exact_material_not_owner_wide() {
    use bm_core::memory::*;
    let producer = pfi2_producer();
    let scope = ProceduralSubjectScopeV1 {
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
    };
    let root =
        ProceduralSubjectValidityRootV1::initialize(scope.clone(), producer.spec.scope.clone(), 2)
            .unwrap();
    let work = ProceduralReconciliationWorkV1 {
        scope,
        target_epoch: 2,
        trigger: ProceduralReconciliationTriggerV1::SourceChange {
            transaction_id: "actual-source-update".into(),
            operation: None,
            changes: vec![pfi2_source_deletion()],
        },
    };
    let root = root.begin(&work, 2).unwrap();
    let mut checkpoint = ProceduralReconciliationCheckpointV1::begin(
        &work,
        &root,
        ProceduralReconciliationManifestPinsV1 {
            agent_tool: ProceduralReconciliationManifestPinV1::Absent,
            runtime_skill: ProceduralReconciliationManifestPinV1::Absent,
        },
        101,
    )
    .unwrap();
    let owner = GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::AgentToolExperience,
        "agent_tool_experience:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let first = ProceduralAppliedOwnerBindingV1::AgentToolExperience {
        owner_revision: GovernedOwnerRevisionRef::try_new(owner.clone(), 1).unwrap(),
        content_digest: digest('1'),
    };
    let second = ProceduralAppliedOwnerBindingV1::AgentToolExperience {
        owner_revision: GovernedOwnerRevisionRef::try_new(owner.clone(), 2).unwrap(),
        content_digest: digest('2'),
    };
    checkpoint.page_number = 2;
    checkpoint.cursor = ProceduralReconciliationCursorV1::Complete;
    checkpoint.verified_owners = vec![
        ProceduralReconciliationOwnerProofV1 {
            binding: first.clone(),
            disposition: ProceduralReconciliationReadDispositionV1::Withdrawn,
        },
        ProceduralReconciliationOwnerProofV1 {
            binding: second.clone(),
            disposition: ProceduralReconciliationReadDispositionV1::Visible,
        },
    ];
    checkpoint.content_digest = checkpoint.canonical_digest().unwrap();
    assert!(checkpoint.permits_exact_read(&work, &second).unwrap());
    assert!(!checkpoint.permits_exact_read(&work, &first).unwrap());
    let unknown = ProceduralAppliedOwnerBindingV1::AgentToolExperience {
        owner_revision: GovernedOwnerRevisionRef::try_new(owner, 3).unwrap(),
        content_digest: digest('3'),
    };
    assert!(!checkpoint.permits_exact_read(&work, &unknown).unwrap());
    let original = producer.revision_ref().unwrap();
    assert!(
        serde_json::from_value::<ProceduralReconciliationDependencyPinV1>(serde_json::json!({
            "kind": "producer", "revision": original,
        }))
        .is_err(),
        "a current-only pin cannot stand in for original source authority"
    );
    let mut contradicting = checkpoint.clone();
    contradicting.dependencies = [false, true]
        .into_iter()
        .map(
            |permitted| ProceduralReconciliationDependencyPinV1::Producer {
                original_revision: original.clone(),
                current_revision: original.clone(),
                permitted,
            },
        )
        .collect();
    contradicting
        .dependencies
        .sort_by_cached_key(|pin| serde_json::to_vec(pin).unwrap());
    contradicting.content_digest = contradicting.canonical_digest().unwrap();
    assert!(
        contradicting.validate_for(&work).is_err(),
        "two permissions for the same original revision are not a canonical snapshot"
    );
    contradicting.dependencies.pop();
    contradicting.content_digest = contradicting.canonical_digest().unwrap();
    contradicting.validate_for(&work).unwrap();
    checkpoint
        .verified_owners
        .push(checkpoint.verified_owners[0].clone());
    checkpoint.content_digest = checkpoint.canonical_digest().unwrap();
    assert!(checkpoint.validate_for(&work).is_err());
}

#[test]
fn procedural_receipt_accounts_for_every_submitted_item() {
    let receipt = ProceduralFeedbackReceiptV2 {
        schema_version: bm_core::memory::PROCEDURAL_FEEDBACK_RECEIPT_SCHEMA_VERSION,
        job_id: "procedural-job-a".to_string(),
        operation_id: "procedural-operation-a".to_string(),
        transaction_id: "procedural-transaction-a".to_string(),
        mutation_receipt_key: digest('9'),
        transcript_digest: digest('4'),
        learning_evidence_digest: digest('5'),
        plan_digest: digest('6'),
        post_image_digest: digest('7'),
        submitted_count: 4,
        accepted_count: 2,
        partially_accepted_count: 0,
        method_dispositions: Vec::new(),
        deferred_count: 1,
        rejected_count: 1,
        changed_count: 2,
        reason_digest: digest('8'),
        completed_at: 100,
    };
    assert!(receipt.validate_contract());

    // A valid execution and a rejected method share one submitted group. The
    // durable receipt must preserve this distinction without rejected content.
    let mut partial_wire = serde_json::to_value(&receipt).unwrap();
    partial_wire["schema_version"] = serde_json::json!(2);
    partial_wire["accepted_count"] = serde_json::json!(1);
    partial_wire["partially_accepted_count"] = serde_json::json!(1);
    partial_wire["method_dispositions"] = serde_json::json!([{
        "feedback_group_ordinal": 0, "method_ordinal": 0,
        "phase": "application", "rejection": "method_owner_content_conflict"
    }]);
    let partial = serde_json::from_value::<ProceduralFeedbackReceiptV2>(partial_wire.clone())
        .expect("partial acceptance must be a durable public receipt, not a report-only counter");
    assert!(partial.validate_contract());
    let evidence = tool_learning_evidence(
        &pfi2_input(bm_core::memory::ProceduralSourceSensitivity::NonPrivate).method_evidence[0]
            .body,
    );
    let mut source_bound = partial.clone();
    source_bound.submitted_count = 1;
    source_bound.accepted_count = 0;
    source_bound.deferred_count = 0;
    source_bound.rejected_count = 0;
    source_bound.learning_evidence_digest = evidence.learning_evidence_digest.clone();
    assert!(source_bound.validates_evidence(&evidence));
    let mut nonexistent_method = source_bound.clone();
    nonexistent_method.method_dispositions[0].method_ordinal = 1;
    assert!(nonexistent_method.validate_contract());
    assert!(!nonexistent_method.validates_evidence(&evidence));
    let mut wrong_phase = source_bound;
    wrong_phase.method_dispositions[0].phase =
        bm_core::memory::ProceduralMethodDispositionPhaseV1::Intake;
    assert!(wrong_phase.validate_contract());
    assert!(!wrong_phase.validates_evidence(&evidence));
    for mutation in 0..6 {
        let mut invalid = partial_wire.clone();
        match mutation {
            0 => invalid["partially_accepted_count"] = serde_json::json!(2),
            1 => invalid["method_dispositions"][0]["feedback_group_ordinal"] = serde_json::json!(4),
            2 => {
                let repeated = invalid["method_dispositions"][0].clone();
                invalid["method_dispositions"]
                    .as_array_mut()
                    .unwrap()
                    .push(repeated);
            }
            3 => {
                invalid["method_dispositions"][0]["rejection"] = serde_json::json!("private_method")
            }
            4 => {
                invalid["method_dispositions"][0]["rejection"] = serde_json::json!("non_canonical")
            }
            _ => {
                invalid["accepted_count"] = serde_json::json!(4);
                invalid["partially_accepted_count"] = serde_json::json!(0);
                invalid["rejected_count"] = serde_json::json!(0);
                invalid["deferred_count"] = serde_json::json!(0);
            }
        }
        assert!(
            !serde_json::from_value::<ProceduralFeedbackReceiptV2>(invalid)
                .unwrap()
                .validate_contract()
        );
    }

    let mut incomplete = receipt;
    incomplete.rejected_count = 0;
    assert!(!incomplete.validate_contract());
}

#[test]
fn procedural_scope_index_separates_active_and_terminal_jobs() {
    let identity = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "subject-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .expect("identity");
    let job =
        ProceduralFeedbackJobV2::pending(identity.clone(), 1, digest('1'), digest('2'), 1, 5, 100)
            .expect("pending job");
    let mut index = ProceduralFeedbackScopeIndexV2::empty(&identity.producer_scope(), 100).unwrap();
    index
        .active_jobs
        .push(ProceduralFeedbackJobRefV2::from_job(&job));
    assert!(index.validate().is_ok());

    index.recent_terminal_jobs = index.active_jobs.clone();
    assert!(index.validate().is_err());
}

#[test]
fn application_ledger_binds_exact_job_evidence_and_owner_revision() {
    let identity = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "subject-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .expect("identity");
    let job = ProceduralFeedbackJobV2::pending(identity, 1, digest('1'), digest('2'), 1, 5, 100)
        .expect("pending job");
    let owner = GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::AgentToolExperience,
            "agent_tool_experience:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        1,
    )
    .expect("owner revision");
    let ledger = ProceduralFeedbackApplicationLedgerV2::build(
        &job,
        vec![
            bm_core::memory::ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                owner_revision: owner,
                content_digest: digest('e'),
            },
        ],
        vec![],
        vec![],
        1,
        101,
    )
    .expect("application ledger");
    assert_eq!(ledger.job_id, job.job_id);
    assert_eq!(
        ledger.learning_evidence_digest,
        job.feedback_source().unwrap().learning_evidence_digest
    );
    assert!(ledger.validate().is_ok());
}

#[test]
fn pfi2_scope_root_and_ledger_cannot_reseal_foreign_identity() {
    let identity = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "agent-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .unwrap();
    let mut scope = ProceduralFeedbackScopeIndexV2::empty(&identity.producer_scope(), 100).unwrap();
    assert!(scope.validate().is_ok());
    scope.mounted_subject_id = "agent-b".into();
    assert!(
        scope.validate().is_err(),
        "the root address must authenticate its exact declared scope"
    );
}

#[test]
fn pfi2_application_ledger_cannot_reseal_a_foreign_job() {
    let identity = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "agent-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .unwrap();
    let job =
        ProceduralFeedbackJobV2::pending(identity, 1, digest('1'), digest('2'), 1, 5, 100).unwrap();
    let mut ledger =
        ProceduralFeedbackApplicationLedgerV2::build(&job, vec![], vec![], vec![], 1, 100).unwrap();
    assert!(ledger.validate().is_ok());
    ledger.identity.turn_id = "foreign-turn".into();
    ledger.application_digest = ledger.canonical_digest().unwrap();
    assert!(
        ledger.validate().is_err(),
        "a resealed digest does not confer the other job identity"
    );
}

#[test]
fn pfi2_producer_root_precedes_transcript_and_is_exact_revision_bound() {
    use bm_core::memory::*;
    let producer = pfi2_producer();
    let source = &producer.spec.scope;
    let mut root = ProceduralFeedbackScopeIndexV2::empty(source, 100).unwrap();
    let first = ProceduralProducerHeadV1::advance(&producer, None).unwrap();
    root.bind_producer_head(&first, 100).unwrap();
    assert_eq!(root.producer_heads, vec![first.current.clone()]);
    assert!(root.active_jobs.is_empty() && root.recent_terminal_jobs.is_empty());
    let original = root.clone();
    root.bind_producer_head(&first, 100).unwrap();
    assert_eq!(root, original);
    let next = ProceduralProducerBindingV1::build(
        producer.spec.clone(),
        ProceduralProducerStateV1::Revoked,
        producer_operation("revoke-root-producer"),
        101,
        Some(&producer),
    )
    .unwrap();
    let head = ProceduralProducerHeadV1::advance(&next, Some(&first)).unwrap();
    root.bind_producer_head(&head, 101).unwrap();
    assert_eq!(root.producer_heads, vec![head.current]);
    let before_reject = root.clone();
    assert!(root.bind_producer_head(&first, 102).is_err());
    assert_eq!(root, before_reject);
    root.producer_heads.push(root.producer_heads[0].clone());
    assert!(root.validate().is_err());
}

#[test]
fn pfi2_ledger_retains_failed_execution_and_rejects_foreign_or_duplicate_contributions() {
    use bm_core::memory::*;
    let producer = pfi2_producer();
    let identity = ProceduralFeedbackIdentityV1::new(
        "space-a",
        "agent-a",
        "sdk.direct",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .unwrap();
    let job =
        ProceduralFeedbackJobV2::pending(identity.clone(), 1, digest('1'), digest('2'), 1, 5, 100)
            .unwrap();
    let mut fact = pfi2_input(ProceduralSourceSensitivity::NonPrivate)
        .execution_facts
        .remove(0);
    fact.outcome = ToolExecutionOutcome::Failed;
    let contribution = ToolExecutionContributionV1::build(
        identity,
        job.feedback_source()
            .unwrap()
            .learning_evidence_digest
            .clone(),
        producer.revision_ref().unwrap(),
        producer.spec.tools[0].clone(),
        fact,
    )
    .unwrap();
    let ledger = ProceduralFeedbackApplicationLedgerV2::build(
        &job,
        vec![],
        vec![contribution.clone()],
        vec![],
        1,
        100,
    )
    .unwrap();
    assert_eq!(
        ledger.execution_contributions[0].fact.outcome,
        ToolExecutionOutcome::Failed
    );
    let encoded = serde_json::to_string(&ledger).unwrap();
    assert_eq!(
        serde_json::from_str::<ProceduralFeedbackApplicationLedgerV2>(&encoded).unwrap(),
        ledger
    );
    assert!(ProceduralFeedbackApplicationLedgerV2::build(
        &job,
        vec![],
        vec![contribution.clone(), contribution.clone()],
        vec![],
        1,
        100
    )
    .is_err());
    let mut foreign = contribution;
    foreign.source.turn_id = "foreign-turn".into();
    foreign.contribution_id = foreign.canonical_identity().unwrap();
    foreign.content_digest = foreign.canonical_digest().unwrap();
    assert!(ProceduralFeedbackApplicationLedgerV2::build(
        &job,
        vec![],
        vec![foreign],
        vec![],
        1,
        100
    )
    .is_err());
}

#[test]
fn pfi2_subject_tool_reduction_preserves_real_chat_identity_and_stale_decisions() {
    use bm_core::memory::*;
    use bm_core::skills::{
        AgentToolExperienceBodyV1 as Body, AgentToolExperienceStatus as Status,
        AgentToolMethodSourceRefV1,
    };
    let producer = pfi2_producer();
    let tool = producer.spec.tools[0].clone();
    let scope = ProceduralSubjectScopeV1 {
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
    };
    let input = pfi2_input(ProceduralSourceSensitivity::NonPrivate);
    let mut facts = Vec::new();
    for chat in ["first-chat", "second-chat"] {
        let identity = ProceduralFeedbackIdentityV1::new(
            "space-a",
            "agent-a",
            "sdk.direct",
            chat,
            chat,
            "same-local-turn",
        )
        .unwrap();
        let mut fact = input.execution_facts[0].clone();
        fact.outcome = ToolExecutionOutcome::Succeeded;
        facts.push(
            ToolExecutionContributionV1::build(
                identity,
                digest('a'),
                producer.revision_ref().unwrap(),
                tool.clone(),
                fact,
            )
            .unwrap(),
        );
    }
    let mut body = Body::Method {
        task_signature: input.method_evidence[0].task_signature.clone(),
        procedure: input.method_evidence[0].body.clone(),
        constraints: Vec::new(),
        sources: Vec::new(),
    };
    let method_digest = body.method_content_digest().unwrap().unwrap();
    if let Body::Method { sources, .. } = &mut body {
        *sources = facts
            .iter()
            .map(|fact| AgentToolMethodSourceRefV1 {
                source_job_id: fact.reference().unwrap().source_job_id,
                method_id: "method-a".into(),
                method_digest: method_digest.clone(),
                execution_refs: vec![fact.reference().unwrap()],
            })
            .collect();
        sources.sort_by(|a, b| {
            (&a.source_job_id, &a.method_id).cmp(&(&b.source_job_id, &b.method_id))
        });
    }
    assert_eq!(
        reduce_agent_tool_experience_contributions(&scope, &tool, &body, &facts, None, 8)
            .unwrap()
            .1,
        Status::Active,
        "equal local turn/call IDs in distinct real chats remain independent evidence"
    );
    assert_eq!(
        reduce_agent_tool_experience_contributions(
            &scope,
            &tool,
            &body,
            &facts,
            Some(Status::Stale),
            8
        )
        .unwrap()
        .1,
        Status::Stale
    );
    let Body::Method { sources, .. } = &mut body else {
        unreachable!()
    };
    sources.retain(|source| source.source_job_id == facts[0].reference().unwrap().source_job_id);
    assert_eq!(
        reduce_agent_tool_experience_contributions(&scope, &tool, &body, &facts[..1], None, 8)
            .unwrap()
            .1,
        Status::Candidate
    );
    let foreign_scope = ProceduralSubjectScopeV1 {
        mounted_subject_id: "agent-b".into(),
        ..scope
    };
    assert!(reduce_agent_tool_experience_contributions(
        &foreign_scope,
        &tool,
        &body,
        &facts[..1],
        None,
        8
    )
    .is_err());
}
