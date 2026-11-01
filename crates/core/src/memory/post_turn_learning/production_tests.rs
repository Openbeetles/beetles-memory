use super::*;

fn material() -> MethodLearningMaterialV1 {
    MethodLearningMaterialV1 {
        material_id: "strategy-1".into(),
        purpose: MethodLearningMaterialPurposeV1::ExecutedStrategy,
        source: MethodLearningMaterialSourceV1::CanonicalInputMessage { input_index: 0 },
        text: "The executor compared the parsed checksum with the expected checksum before accepting the file.".into(),
        sensitivity: ProceduralSourceSensitivity::NonPrivate,
        external_content: false,
    }
}

#[test]
fn method_material_is_not_a_method_declaration_or_execution_witness() {
    let material = material();
    assert!(material.validate_contract());
    let value = serde_json::to_value(&material).unwrap();
    for forbidden in ["procedure", "authority", "used", "success", "revision"] {
        assert!(value.get(forbidden).is_none());
        let mut forged = value.clone();
        forged[forbidden] = serde_json::json!(true);
        assert!(serde_json::from_value::<MethodLearningMaterialV1>(forged).is_err());
    }
}

#[test]
fn extraction_cannot_launder_unknown_private_or_external_material() {
    let mut input = material();
    assert!(input.permits_method_extraction());
    for sensitivity in [
        ProceduralSourceSensitivity::Unknown,
        ProceduralSourceSensitivity::Private,
    ] {
        input.sensitivity = sensitivity;
        assert!(input.validate_contract());
        assert!(!input.permits_method_extraction());
    }
    input.sensitivity = ProceduralSourceSensitivity::NonPrivate;
    input.external_content = true;
    assert!(!input.permits_method_extraction());
}

#[test]
fn canonical_material_rejects_empty_text_oversize_and_raw_payload_purpose() {
    let mut input = material();
    input.text.clear();
    assert!(!input.validate_contract());
    input.text = "x".repeat(MAX_METHOD_LEARNING_MATERIAL_BYTES + 1);
    assert!(!input.validate_contract());
    let mut value = serde_json::to_value(material()).unwrap();
    value["purpose"] = serde_json::json!("raw_tool_output");
    assert!(serde_json::from_value::<MethodLearningMaterialV1>(value).is_err());
}

#[test]
fn model_candidate_cannot_assert_authority_usage_or_promotion() {
    let value = serde_json::json!({
        "task_signature": "verify_file",
        "procedure": "1. Check the expected checksum and calculate the file checksum.\n2. Verify the comparison before accepting the file; if values differ, reject the file and report failure.",
        "constraints": ["The expected checksum must come from an authenticated source."],
        "source_material_ids": ["strategy-1"]
    });
    let proposal: MethodLearningProposalV1 = serde_json::from_value(value.clone()).unwrap();
    assert!(proposal.validate_against(&[material()]).is_ok());
    let digest = proposal.content_digest().unwrap();
    let mut alternate_source = proposal.clone();
    alternate_source.source_material_ids = vec!["another-valid-source".into()];
    assert_eq!(alternate_source.content_digest().unwrap(), digest);
    alternate_source.constraints = vec!["Only compare authenticated SHA-256 values.".into()];
    assert_ne!(alternate_source.content_digest().unwrap(), digest);
    for forbidden in ["authority", "revision", "status", "success_count", "used"] {
        let mut forged = value.clone();
        forged[forbidden] = serde_json::json!("trusted");
        assert!(serde_json::from_value::<MethodLearningProposalV1>(forged).is_err());
    }
    let mut invalid = proposal;
    invalid.source_material_ids.push("strategy-1".into());
    assert!(invalid.validate_against(&[material()]).is_err());
    invalid.source_material_ids = vec!["unseen-source".into()];
    assert!(invalid.validate_against(&[material()]).is_err());
}

#[test]
fn task_outcome_is_not_inferred_from_tool_success_or_delivery() {
    let method = MethodContentRefV1 {
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
        registry_ref: AgentToolRegistryRef::new("tools", "fingerprint"),
        tool_id: "verify_file".into(),
        schema_fingerprint: "schema-a".into(),
        owner_revision: GovernedOwnerRevisionRef::try_new(
            crate::memory::GovernedMemoryOwnerRef {
                owner_plane: GovernedMemoryOwnerPlane::AgentToolExperience,
                owner_id: format!("agent_tool_experience:sha256:{}", "a".repeat(64)),
            },
            1,
        )
        .unwrap(),
        content_digest: format!("sha256:{}", "b".repeat(64)),
    };
    let mut adoption = MethodAdoptionEvidenceV1 {
        method: method.clone(),
        task_id: "task-1".into(),
        adoption: MethodAdoptionStatusV1::Adopted,
        observation_refs: vec!["call-1".into()],
    };
    let mut evaluation = MethodTaskEvaluationV1 {
        method,
        task_id: "task-1".into(),
        task_root_id: "task-root-1".into(),
        independence_group: "scenario-1".into(),
        evaluator_id: "file-verifier".into(),
        evaluator_revision: 1,
        outcome: MethodTaskOutcomeV1::Passed,
        observation_refs: vec!["verification-1".into()],
    };
    assert!(evaluation.supports_adoption(&adoption, MemoryTurnDeliveryStatus::Delivered));
    for outcome in [
        MethodTaskOutcomeV1::Unknown,
        MethodTaskOutcomeV1::Failed,
        MethodTaskOutcomeV1::Partial,
        MethodTaskOutcomeV1::Cancelled,
    ] {
        evaluation.outcome = outcome;
        assert!(!evaluation.supports_adoption(&adoption, MemoryTurnDeliveryStatus::Delivered));
    }
    evaluation.outcome = MethodTaskOutcomeV1::Passed;
    for delivery in [
        MemoryTurnDeliveryStatus::Cancelled,
        MemoryTurnDeliveryStatus::IncompleteStream,
        MemoryTurnDeliveryStatus::UserOnly,
        MemoryTurnDeliveryStatus::UpstreamFailed,
        MemoryTurnDeliveryStatus::RejectedByPolicy,
    ] {
        assert!(!evaluation.supports_adoption(&adoption, delivery));
    }
    for usage in [
        MethodAdoptionStatusV1::Unknown,
        MethodAdoptionStatusV1::NotAdopted,
    ] {
        adoption.adoption = usage;
        assert!(!evaluation.supports_adoption(&adoption, MemoryTurnDeliveryStatus::Delivered));
    }
    adoption.adoption = MethodAdoptionStatusV1::Adopted;
    evaluation.method.owner_revision.owner_revision = 2;
    assert!(!evaluation.supports_adoption(&adoption, MemoryTurnDeliveryStatus::Delivered));
    evaluation.method = adoption.method.clone();
    evaluation.task_id = "another-task".into();
    assert!(!evaluation.supports_adoption(&adoption, MemoryTurnDeliveryStatus::Delivered));
}

#[test]
fn method_content_identity_ignores_provenance_and_includes_constraints() {
    let mut proposal = MethodLearningProposalV1 {
        task_signature: "verify_file".into(),
        procedure: "1. Calculate the checksum.\n2. Verify the expected checksum before acceptance; reject on mismatch.".into(),
        constraints: vec!["Use an authenticated expected checksum.".into()],
        source_material_ids: vec!["source-one".into()],
    };
    let digest = proposal.content_digest().unwrap();
    proposal.source_material_ids = vec!["different-source".into()];
    assert_eq!(proposal.content_digest().unwrap(), digest);
    proposal.constraints = vec!["Use two authenticated expected checksums.".into()];
    assert_ne!(proposal.content_digest().unwrap(), digest);
}

#[test]
fn admitted_material_never_retains_rejected_private_text_or_source_identifier() {
    let mut private = material();
    private.material_id = "private-correlation-marker".into();
    private.source = MethodLearningMaterialSourceV1::ToolObservation {
        observation_id: "private-source-marker".into(),
    };
    private.text = "private-body-marker".into();
    private.sensitivity = ProceduralSourceSensitivity::Private;
    let group = MethodLearningInputV1 {
        registry_ref: AgentToolRegistryRef::new("tools", "fingerprint"),
        tool_id: "verify_file".into(),
        schema_fingerprint: "schema-a".into(),
        materials: vec![private],
    };
    let admitted = AdmittedMethodLearningInputV1::admit(&group).unwrap();
    assert!(admitted.validate_contract());
    assert!(admitted.materials.is_empty());
    assert_eq!(
        admitted.rejected_materials[0].rejection,
        MethodLearningMaterialRejectionV1::Private
    );
    let json = serde_json::to_string(&admitted).unwrap();
    for secret in [
        "private-correlation-marker",
        "private-source-marker",
        "private-body-marker",
    ] {
        assert!(!json.contains(secret));
    }
}

#[test]
fn production_material_only_is_authorized_content_but_not_procedural_work() {
    let mut input = super::super::PostTurnLearningInputV3::empty();
    input.method_learning_inputs.push(MethodLearningInputV1 {
        registry_ref: AgentToolRegistryRef::new("tools", "fingerprint"),
        tool_id: "verify_file".into(),
        schema_fingerprint: "schema-a".into(),
        materials: vec![material()],
    });
    assert!(input.validate_production_contract());
    assert!(input.has_authorized_content());
    assert!(!input.has_procedural_work());
    assert!(!input.has_signed_turn_context());
    let repeated = input.method_learning_inputs[0].clone();
    input.method_learning_inputs.push(repeated);
    assert!(!input.validate_production_contract());
}

fn production_binding() -> super::super::ProceduralProducerBindingV1 {
    use super::super::*;
    let spec = ProceduralProducerSpecV1 {
        binding_id: "bounded-learning".into(),
        scope: ProceduralProducerScopeV1 {
            memory_space_id: "space-a".into(),
            mounted_subject_id: "agent-a".into(),
            channel_id: "sdk".into(),
            chat_id: "chat".into(),
        },
        principal: ProceduralProducerPrincipalV1::LocalCapability {
            capability_id: "executor".into(),
        },
        source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
        claims: ProceduralProducerClaimsV1 {
            execution_facts: false,
            method_declarations: false,
            usage_feedback: false,
            method_learning_materials: true,
            method_adoptions: false,
            method_task_evaluations: false,
            source_classifications: vec![ProceduralSourceSensitivity::NonPrivate],
        },
        tools: vec![ProceduralProducerToolV1 {
            registry_ref: AgentToolRegistryRef::new("tools", "fingerprint"),
            tool_id: "verify_file".into(),
            schema_fingerprint: "schema-a".into(),
        }],
        method_evaluators: Vec::new(),
        source_config_ref: "executor-config".into(),
    };
    let identity = crate::memory::MemoryMutationOperationIdentity::new(
        "grant",
        "space-a",
        "agent-a",
        "governor",
        crate::memory::MemoryMutationOperationKind::ProceduralProducerControl,
    )
    .unwrap();
    ProceduralProducerBindingV1::build(spec, ProceduralProducerStateV1::Active, identity, 1, None)
        .unwrap()
}

#[test]
fn material_claim_is_independent_of_declaration_and_model_cannot_witness_usage() {
    let binding = production_binding();
    let mut input = super::super::PostTurnLearningInputV3::empty();
    input.method_learning_inputs.push(MethodLearningInputV1 {
        registry_ref: binding.spec.tools[0].registry_ref.clone(),
        tool_id: "verify_file".into(),
        schema_fingerprint: "schema-a".into(),
        materials: vec![material()],
    });
    assert!(binding.authorize_production_input(&input).is_ok());
    let mut forbidden = binding.clone();
    forbidden.spec.claims.method_learning_materials = false;
    forbidden.spec.claims.method_declarations = true;
    forbidden.content_digest = forbidden.canonical_digest().unwrap();
    assert!(forbidden.validate_contract());
    assert_eq!(
        forbidden.authorize_production_input(&input),
        Err(super::super::ProceduralProducerErrorV1::ClaimForbidden)
    );
    let mut spec = binding.spec;
    spec.source_authority = super::super::ProceduralProducerSourceAuthorityV1::ModelInferred {
        subject_id: "agent-a".into(),
    };
    spec.claims.method_adoptions = true;
    assert!(!spec.validate_contract());
    spec.claims.method_adoptions = false;
    spec.claims.method_task_evaluations = true;
    spec.method_evaluators.push(MethodTaskEvaluatorV1 {
        id: "verifier".into(),
        revision: 1,
    });
    assert!(!spec.validate_contract());
}

#[test]
fn evaluator_revision_and_independence_are_not_caller_invented_authority() {
    let mut binding = production_binding();
    binding.spec.claims.method_task_evaluations = true;
    binding.spec.method_evaluators = vec![MethodTaskEvaluatorV1 {
        id: "verifier".into(),
        revision: 3,
    }];
    binding.content_digest = binding.canonical_digest().unwrap();
    assert!(binding.validate_contract());
    let method = MethodContentRefV1 {
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
        registry_ref: AgentToolRegistryRef::new("tools", "fingerprint"),
        tool_id: "verify_file".into(),
        schema_fingerprint: "schema-a".into(),
        owner_revision: GovernedOwnerRevisionRef::try_new(
            crate::memory::GovernedMemoryOwnerRef {
                owner_plane: GovernedMemoryOwnerPlane::AgentToolExperience,
                owner_id: format!("agent_tool_experience:sha256:{}", "a".repeat(64)),
            },
            1,
        )
        .unwrap(),
        content_digest: format!("sha256:{}", "b".repeat(64)),
    };
    let evaluation = MethodTaskEvaluationV1 {
        method,
        task_id: "task-1".into(),
        task_root_id: "root-1".into(),
        independence_group: "scenario-1".into(),
        evaluator_id: "verifier".into(),
        evaluator_revision: 3,
        outcome: MethodTaskOutcomeV1::Passed,
        observation_refs: vec!["check-1".into()],
    };
    let mut input = super::super::PostTurnLearningInputV3::empty();
    input.method_task_evaluations.push(evaluation.clone());
    assert!(binding.authorize_production_input(&input).is_ok());
    input.method_task_evaluations[0].evaluator_revision = 4;
    assert_eq!(
        binding.authorize_production_input(&input),
        Err(super::super::ProceduralProducerErrorV1::AuthorityMismatch)
    );
    let mut other = evaluation.clone();
    other.task_id = "new-random-task".into();
    assert!(!other.independent_of(&evaluation));
    other.independence_group = "new-group".into();
    assert!(!other.independent_of(&evaluation));
    other.task_root_id = "root-2".into();
    assert!(other.independent_of(&evaluation));
    other.independence_group = evaluation.independence_group.clone();
    assert!(!other.independent_of(&evaluation));
}

#[test]
fn governance_extraction_source_never_claims_execution_or_human_authorship() {
    let source = synthetic_extraction_source();
    assert!(source.validate_contract());
    assert_eq!(
        source.source_authority(),
        crate::memory::MemoryEvidenceAuthority::ModelInferred
    );
    let reference = crate::skills::AgentToolMethodSourceRefV2::GovernanceExtraction {
        source: Box::new(source),
    };
    assert!(reference.validate_contract());
    assert!(reference.execution_refs().is_empty());
    assert!(reference.producer_declaration().is_none());
    let mut forged = serde_json::to_value(&reference).unwrap();
    forged["source"]["source_authority"] = serde_json::json!("user_asserted");
    assert!(serde_json::from_value::<crate::skills::AgentToolMethodSourceRefV2>(forged).is_err());
}

#[test]
fn extraction_source_requires_exact_binding_and_bounded_admitted_materials() {
    let mut source = synthetic_extraction_source();
    source.execution_binding = crate::memory::PostTurnGovernanceExecutionBindingV1::Unbound;
    assert!(!source.validate_contract());
    source = synthetic_extraction_source();
    source.source_job_id = "different-job".into();
    assert!(!source.validate_contract());
    source = synthetic_extraction_source();
    source.materials[0].content_digest = "unknown".into();
    assert!(!source.validate_contract());
}

fn synthetic_extraction_source() -> GovernanceMethodExtractionSourceV1 {
    let identity = crate::memory::PostTurnGovernanceIdentityV2::new(
        "space-a",
        "agent-a",
        "desktop",
        "chat-a",
        "conversation-a",
        "turn-a",
    )
    .unwrap();
    GovernanceMethodExtractionSourceV1 {
        source_job_id: identity.job_id(),
        identity,
        source_transcript_sequence: 1,
        source_transcript_digest: test_digest('a'),
        learning_evidence_digest: test_digest('b'),
        runtime_binding_digest: test_digest('c'),
        execution_binding: crate::memory::PostTurnGovernanceExecutionBindingV1::Bound {
            binding_id: "semantic-binding".into(),
            binding_revision: 1,
        },
        execution_binding_digest: test_digest('d'),
        governance_contract_version: 1,
        proposal_ordinal: 0,
        tool: super::super::ProceduralProducerToolV1 {
            registry_ref: AgentToolRegistryRef::new("tools", "fingerprint"),
            tool_id: "verify_file".into(),
            schema_fingerprint: "schema-a".into(),
        },
        materials: vec![MethodLearningMaterialRefV1::from_material(0, &material()).unwrap()],
        method_digest: test_method_body().method_content_digest().unwrap().unwrap(),
    }
}

fn test_digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

fn test_method_body() -> crate::skills::AgentToolExperienceBodyV1 {
    let task_signature = "verify_file".to_string();
    let procedure = "1. Calculate the checksum.\n2. Verify the expected checksum before acceptance; reject on mismatch.".to_string();
    let constraints = vec!["Use an authenticated expected checksum.".to_string()];
    let method_digest = crate::skills::canonical_agent_tool_method_content_digest(
        &task_signature,
        &procedure,
        &constraints,
    )
    .unwrap();
    crate::skills::AgentToolExperienceBodyV1::Method {
        task_signature,
        procedure,
        constraints,
        sources: vec![
            crate::skills::AgentToolMethodSourceRefV2::ProducerDeclaration {
                source_job_id: "original-declaration".into(),
                method_id: "checksum".into(),
                method_digest,
                execution_refs: vec![],
            },
        ],
        usage_contributions: vec![],
    }
}

fn test_method_material() -> crate::skills::AgentToolExperienceRevisionMaterialV4 {
    crate::skills::AgentToolExperienceRevisionMaterialV4::build(
        "space-a",
        crate::skills::AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: "agent-a".into(),
        },
        "tools",
        crate::skills::AgentToolRegistryScope::Global,
        "verify_file",
        "schema-a",
        1,
        test_method_body(),
        crate::skills::AgentToolExperienceStatus::Candidate,
        crate::memory::MemoryPrivacyClass::SharedWithSubject,
        100,
        100,
        None,
    )
    .unwrap()
}

// These fixtures exercise the pure owner reducer from exact immutable facts.
// Public finalize/Store closure tests separately prove producer authorization.
fn task_contribution(
    turn: &str,
    root: &str,
    group: &str,
    outcome: MethodTaskOutcomeV1,
) -> (
    MethodTaskContributionV1,
    super::super::ToolExecutionContributionV1,
) {
    let material = test_method_material();
    let source = super::super::ProceduralFeedbackIdentityV1::new(
        "space-a",
        "agent-a",
        "desktop",
        "chat-a",
        "conversation-a",
        turn,
    )
    .unwrap();
    let evidence_digest = test_digest('e');
    let producer = super::super::ProceduralProducerRevisionRefV1 {
        binding_key: test_digest('a'),
        revision: 1,
        content_digest: test_digest('b'),
    };
    let tool = super::super::ProceduralProducerToolV1 {
        registry_ref: AgentToolRegistryRef::new("tools", "fingerprint"),
        tool_id: "verify_file".into(),
        schema_fingerprint: "schema-a".into(),
    };
    let execution = super::super::ToolExecutionContributionV1::build(
        source.clone(),
        evidence_digest.clone(),
        producer.clone(),
        tool.clone(),
        super::super::ToolExecutionFactV1 {
            observation_id: "executed-call".into(),
            call_id: "call-1".into(),
            outcome: super::super::ToolExecutionOutcome::Succeeded,
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
            started_at: Some(101),
            completed_at: Some(102),
        },
    )
    .unwrap();
    let method = MethodContentRefV1 {
        memory_space_id: "space-a".into(),
        mounted_subject_id: "agent-a".into(),
        registry_ref: tool.registry_ref,
        tool_id: tool.tool_id,
        schema_fingerprint: tool.schema_fingerprint,
        owner_revision: material.owner_revision_ref(),
        content_digest: material.content_digest.clone(),
    };
    let mut contribution = MethodTaskContributionV1 {
        source_job_id: source.job_id(&evidence_digest).unwrap(),
        source,
        source_transcript_digest: test_digest('f'),
        learning_evidence_digest: evidence_digest,
        producer,
        adoption_ordinal: 0,
        evaluation_ordinal: 0,
        adoption: MethodAdoptionEvidenceV1 {
            method: method.clone(),
            task_id: format!("task-{turn}"),
            adoption: MethodAdoptionStatusV1::Adopted,
            observation_refs: vec!["executed-call".into()],
        },
        evaluation: MethodTaskEvaluationV1 {
            method,
            task_id: format!("task-{turn}"),
            task_root_id: root.into(),
            independence_group: group.into(),
            evaluator_id: "independent-checksum-verifier".into(),
            evaluator_revision: 1,
            outcome,
            observation_refs: vec!["evaluation-observation".into()],
        },
        method_content_digest: material.body.method_content_digest().unwrap().unwrap(),
        execution_refs: vec![execution.reference().unwrap()],
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        observed_at: 103,
        contribution_id: String::new(),
        content_digest: String::new(),
    };
    reseal_task_contribution(&mut contribution);
    assert!(contribution.validate_contract());
    (contribution, execution)
}

fn reseal_task_contribution(value: &mut MethodTaskContributionV1) {
    value.contribution_id = value.identity_digest().unwrap();
    value.content_digest = value.canonical_digest().unwrap();
}

#[test]
fn application_ledger_requires_explicit_task_evidence_even_when_empty() {
    use super::super::{ProceduralFeedbackApplicationLedgerV3, ProceduralFeedbackJobV2};
    let (task, _) = task_contribution("turn-ledger", "root", "group", MethodTaskOutcomeV1::Passed);
    let job = ProceduralFeedbackJobV2::pending(
        task.source,
        1,
        task.source_transcript_digest,
        task.learning_evidence_digest,
        1,
        5,
        100,
    )
    .unwrap();
    let ledger =
        ProceduralFeedbackApplicationLedgerV3::build(&job, vec![], vec![], vec![], vec![], 1, 104)
            .unwrap();
    let mut wire = serde_json::to_value(&ledger).unwrap();
    assert_eq!(
        wire.get("method_task_contributions"),
        Some(&serde_json::json!([]))
    );
    wire.as_object_mut()
        .unwrap()
        .remove("method_task_contributions");
    assert!(serde_json::from_value::<ProceduralFeedbackApplicationLedgerV3>(wire).is_err());
}

#[test]
fn application_ledger_retains_task_witness_and_rejects_detached_execution() {
    use super::super::{ProceduralFeedbackApplicationLedgerV3, ProceduralFeedbackJobV2};
    let (task, execution) =
        task_contribution("turn-ledger", "root", "group", MethodTaskOutcomeV1::Passed);
    let job = ProceduralFeedbackJobV2::pending(
        task.source.clone(),
        1,
        task.source_transcript_digest.clone(),
        task.learning_evidence_digest.clone(),
        1,
        5,
        100,
    )
    .unwrap();
    let ledger = ProceduralFeedbackApplicationLedgerV3::build(
        &job,
        vec![],
        vec![execution],
        vec![],
        vec![task.clone()],
        1,
        104,
    )
    .unwrap();
    let bytes = serde_json::to_vec(&ledger).unwrap();
    let restored: ProceduralFeedbackApplicationLedgerV3 = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored.method_task_contributions, vec![task]);
    assert!(restored.validate().is_ok());
    let mut detached = restored.clone();
    detached.execution_contributions.clear();
    detached.application_digest = detached.canonical_digest().unwrap();
    assert!(detached.validate().is_err());
    let mut forged = restored;
    forged.method_task_contributions[0].evaluation.task_root_id = "another-root".into();
    forged.application_digest = forged.canonical_digest().unwrap();
    assert!(forged.validate().is_err());
}

#[test]
fn semantic_completion_explicitly_binds_method_application() {
    use crate::memory::{
        PostTurnGovernanceDecisionSummaryV1, PostTurnGovernanceReceiptV4,
        PostTurnSemanticGovernanceReport,
    };
    let receipt = PostTurnGovernanceReceiptV4::new(
        "job-a",
        "transaction-a",
        test_digest('a'),
        test_digest('b'),
        104,
        PostTurnGovernanceDecisionSummaryV1::from_semantic_report(
            &PostTurnSemanticGovernanceReport::skipped("no_candidates"),
        )
        .unwrap(),
        crate::memory::PostTurnGovernanceMethodApplicationV1::None,
    )
    .unwrap();
    let mut wire = serde_json::to_value(&receipt).unwrap();
    assert!(wire.get("methodApplication").is_some());
    wire.as_object_mut().unwrap().remove("methodApplication");
    assert!(serde_json::from_value::<PostTurnGovernanceReceiptV4>(wire).is_err());
}

fn body_with_usage(
    contributions: &[MethodTaskContributionV1],
) -> crate::skills::AgentToolExperienceBodyV1 {
    let mut body = test_method_body();
    let crate::skills::AgentToolExperienceBodyV1::Method {
        usage_contributions,
        ..
    } = &mut body
    else {
        unreachable!()
    };
    *usage_contributions = contributions
        .iter()
        .map(|item| item.reference().unwrap())
        .collect();
    usage_contributions.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
    assert!(body.validate_contract());
    body
}

fn reduce_tasks(
    body: &crate::skills::AgentToolExperienceBodyV1,
    tasks: &[MethodTaskContributionV1],
    executions: &[super::super::ToolExecutionContributionV1],
) -> (
    crate::skills::AgentToolExperienceBodyV1,
    crate::skills::AgentToolExperienceStatus,
) {
    reduce_agent_tool_method_task_contributions(
        &super::super::ProceduralSubjectScopeV1 {
            memory_space_id: "space-a".into(),
            mounted_subject_id: "agent-a".into(),
        },
        &synthetic_extraction_source().tool,
        body,
        tasks,
        executions,
        None,
        32,
    )
    .unwrap()
}

#[test]
fn method_activation_requires_exact_adoption_and_independent_task_evaluation() {
    use crate::skills::AgentToolExperienceStatus as Status;
    let (first, fact1) =
        task_contribution("turn-1", "root-1", "group-1", MethodTaskOutcomeV1::Passed);
    let (second, fact2) =
        task_contribution("turn-2", "root-2", "group-2", MethodTaskOutcomeV1::Passed);
    let tasks = vec![first, second];
    let facts = vec![fact1, fact2];
    assert_eq!(
        reduce_tasks(&test_method_body(), &[], &facts).1,
        Status::Candidate
    );
    assert_eq!(
        reduce_tasks(&body_with_usage(&tasks), &tasks, &facts).1,
        Status::Active
    );
    let mut same_root = tasks.clone();
    same_root[1].evaluation.task_root_id = same_root[0].evaluation.task_root_id.clone();
    reseal_task_contribution(&mut same_root[1]);
    assert_eq!(
        reduce_tasks(&body_with_usage(&same_root), &same_root, &facts).1,
        Status::Candidate
    );
    let mut same_group = tasks.clone();
    same_group[1].evaluation.independence_group =
        same_group[0].evaluation.independence_group.clone();
    reseal_task_contribution(&mut same_group[1]);
    assert_eq!(
        reduce_tasks(&body_with_usage(&same_group), &same_group, &facts).1,
        Status::Candidate
    );
    let mut unknown_adoption = tasks.clone();
    unknown_adoption[1].adoption.adoption = MethodAdoptionStatusV1::Unknown;
    reseal_task_contribution(&mut unknown_adoption[1]);
    assert_eq!(
        reduce_tasks(
            &body_with_usage(&unknown_adoption),
            &unknown_adoption,
            &facts
        )
        .1,
        Status::Candidate
    );
}

#[test]
fn method_failure_and_withdrawn_evidence_cannot_leave_success_counts() {
    use crate::skills::AgentToolExperienceStatus as Status;
    let (first, fact1) =
        task_contribution("turn-1", "root-1", "group-1", MethodTaskOutcomeV1::Passed);
    let (second, fact2) =
        task_contribution("turn-2", "root-2", "group-2", MethodTaskOutcomeV1::Failed);
    let tasks = vec![first, second];
    let body = body_with_usage(&tasks);
    assert_eq!(
        reduce_tasks(&body, &tasks, &[fact1.clone(), fact2]).1,
        Status::Stale
    );
    let (reduced, status) = reduce_tasks(&body, &tasks[..1], &[fact1]);
    assert_eq!(status, Status::Candidate);
    assert_eq!(
        reduced.method_usage_contributions(),
        &[tasks[0].reference().unwrap()]
    );
    let (empty, status) = reduce_tasks(&body, &[], &[]);
    assert_eq!(status, Status::Candidate);
    assert!(empty.method_usage_contributions().is_empty());
}

#[test]
fn adopted_method_material_is_a_prior_revision_not_a_digest_cycle() {
    let (task, _) = task_contribution("turn-1", "root-1", "group-1", MethodTaskOutcomeV1::Passed);
    let prior = test_method_material();
    let body = body_with_usage(&[task]);
    let next = crate::skills::AgentToolExperienceRevisionMaterialV4::build(
        "space-a",
        prior.owning_scope.clone(),
        "tools",
        prior.registry_scope.clone(),
        "verify_file",
        "schema-a",
        2,
        body.clone(),
        crate::skills::AgentToolExperienceStatus::Candidate,
        prior.privacy_class,
        prior.created_at,
        104,
        Some(
            crate::skills::AgentToolExperienceRetainedRevisionDigestV3::from_material(&prior)
                .unwrap(),
        ),
    )
    .unwrap();
    assert!(next.validate_contract().accepted);
    let mut cycle = next.clone();
    let crate::skills::AgentToolExperienceBodyV1::Method {
        usage_contributions,
        ..
    } = &mut cycle.body
    else {
        unreachable!()
    };
    usage_contributions[0].method.owner_revision.owner_revision = 2;
    cycle.content_digest = cycle.canonical_content_digest().unwrap();
    assert!(!cycle.validate_contract().accepted);
    let mut wrong_scope = next;
    let crate::skills::AgentToolExperienceBodyV1::Method {
        usage_contributions,
        ..
    } = &mut wrong_scope.body
    else {
        unreachable!()
    };
    usage_contributions[0].method.mounted_subject_id = "agent-b".into();
    wrong_scope.content_digest = wrong_scope.canonical_content_digest().unwrap();
    assert!(!wrong_scope.validate_contract().accepted);
}

#[test]
fn unified_reducer_never_promotes_method_from_tool_successes() {
    let (_, first) = task_contribution("turn-1", "root-1", "group-1", MethodTaskOutcomeV1::Passed);
    let (_, second) = task_contribution("turn-2", "root-2", "group-2", MethodTaskOutcomeV1::Passed);
    let mut body = test_method_body();
    let crate::skills::AgentToolExperienceBodyV1::Method { sources, .. } = &mut body else {
        unreachable!()
    };
    let digest = sources[0].method_digest().to_string();
    *sources = [&first, &second]
        .iter()
        .enumerate()
        .map(
            |(index, fact)| crate::skills::AgentToolMethodSourceRefV2::ProducerDeclaration {
                source_job_id: fact.reference().unwrap().source_job_id,
                method_id: format!("method-{index}"),
                method_digest: digest.clone(),
                execution_refs: vec![fact.reference().unwrap()],
            },
        )
        .collect();
    crate::skills::AgentToolMethodSourceRefV2::canonicalize(sources).unwrap();
    let (_, status) = super::super::reduce_agent_tool_experience_contributions(
        &super::super::ProceduralSubjectScopeV1 {
            memory_space_id: "space-a".into(),
            mounted_subject_id: "agent-a".into(),
        },
        &first.tool.clone(),
        &body,
        &[first, second],
        &[],
        None,
        32,
    )
    .unwrap();
    assert_eq!(status, crate::skills::AgentToolExperienceStatus::Candidate);
}

#[test]
fn unknown_task_outcome_or_missing_exact_execution_does_not_activate_method() {
    let (first, fact1) =
        task_contribution("turn-1", "root-1", "group-1", MethodTaskOutcomeV1::Passed);
    let (second, fact2) =
        task_contribution("turn-2", "root-2", "group-2", MethodTaskOutcomeV1::Unknown);
    let tasks = vec![first, second];
    let body = body_with_usage(&tasks);
    assert_eq!(
        reduce_tasks(&body, &tasks, &[fact1.clone(), fact2]).1,
        crate::skills::AgentToolExperienceStatus::Candidate
    );
    let (without_second_execution, status) = reduce_tasks(&body, &tasks, &[fact1]);
    assert_eq!(status, crate::skills::AgentToolExperienceStatus::Candidate);
    assert_eq!(
        without_second_execution.method_usage_contributions(),
        &[tasks[0].reference().unwrap()]
    );
}

#[test]
fn governance_extraction_material_rejects_cross_subject_source() {
    let mut source = synthetic_extraction_source();
    let mut body = test_method_body();
    let crate::skills::AgentToolExperienceBodyV1::Method { sources, .. } = &mut body else {
        unreachable!()
    };
    source.identity.mounted_subject_id = "agent-b".into();
    source.source_job_id = source.identity.job_id();
    *sources = vec![
        crate::skills::AgentToolMethodSourceRefV2::GovernanceExtraction {
            source: Box::new(source),
        },
    ];
    assert!(body.validate_contract());
    let mut material = test_method_material();
    material.body = body;
    material.content_digest = material.canonical_content_digest().unwrap();
    assert!(!material.validate_contract().accepted);
}
