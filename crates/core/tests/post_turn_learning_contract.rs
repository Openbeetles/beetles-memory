use bm_core::memory::{
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef,
    PostTurnLearningEvidenceV1, ProceduralApplicabilityContextV1,
    ProceduralFeedbackApplicationLedgerV1, ProceduralFeedbackAuthorityV1,
    ProceduralFeedbackIdentityV1, ProceduralFeedbackJobRefV1, ProceduralFeedbackJobStatusV1,
    ProceduralFeedbackJobV1, ProceduralFeedbackReceiptV1, ProceduralFeedbackScopeIndexV1,
    ProceduralProjectionIdentityV1, ProceduralSelectionReceiptV1,
    POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION, PROCEDURAL_SELECTION_RECEIPT_SCHEMA_VERSION,
};
use bm_core::skills::AgentToolRegistryScope;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn tool_learning_evidence(summary: &str) -> PostTurnLearningEvidenceV1 {
    let mut evidence = PostTurnLearningEvidenceV1 {
        schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
        memory_space_id: "space-a".into(),
        mounted_subject_id: "subject-a".into(),
        conversation_id: "conversation-a".into(),
        turn_id: "turn-a".into(),
        canonical_turn_digest: digest('c'),
        tool_call_count: 1,
        selection_receipt: None,
        runtime_skill_feedback: vec![],
        agent_skill_feedback: vec![],
        task_learning_feedback: vec![],
        agent_tool_feedback: vec![bm_core::memory::AgentToolUsageFeedbackV2 {
            registry_ref: bm_core::skills::AgentToolRegistryRef::new("tools-a", digest('a')),
            tool_id: "archive-tool".into(),
            schema_fingerprint: "schema-a".into(),
            observations: vec![bm_core::skills::AgentToolObservationDigest {
                observation_id: "observation-a".into(),
                registry_id: "tools-a".into(),
                tool_id: "archive-tool".into(),
                schema_fingerprint: "schema-a".into(),
                call_id: Some("call-a".into()),
                task_signature: "archive-task".into(),
                summary: summary.into(),
                outcome: bm_core::skills::AgentToolOutcome::Succeeded,
                error_code: None,
                external_content: false,
                private_content_used: false,
                permission_tags: vec![],
                risk_tags: vec![],
                started_at: Some(90),
                completed_at: Some(100),
            }],
            outcome: bm_core::memory::ProceduralExecutionOutcomeV1::Succeeded,
            user_visible_result_summary: None,
            operator_note: None,
        }],
        authority: ProceduralFeedbackAuthorityV1::HostRuntimeObservation,
        learning_evidence_digest: String::new(),
    };
    evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
    evidence
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
        evidence.agent_tool_feedback[0].observations[0].summary,
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
    assert!(tool_learning_evidence("Archive extraction completed").validate_contract());
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
    let mut evidence = tool_learning_evidence("Archive extraction completed");
    evidence.agent_tool_feedback[0].observations[0].task_signature = "archive\ntask".into();
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
    let mut evidence = PostTurnLearningEvidenceV1 {
        schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
        memory_space_id: "space-a".to_string(),
        mounted_subject_id: "subject-a".to_string(),
        conversation_id: "conversation-a".to_string(),
        turn_id: "turn-a".to_string(),
        canonical_turn_digest: digest('c'),
        tool_call_count: 0,
        selection_receipt: None,
        runtime_skill_feedback: Vec::new(),
        agent_skill_feedback: Vec::new(),
        task_learning_feedback: Vec::new(),
        agent_tool_feedback: Vec::new(),
        authority: ProceduralFeedbackAuthorityV1::HostRuntimeObservation,
        learning_evidence_digest: String::new(),
    };
    evidence.learning_evidence_digest = evidence.canonical_digest().expect("evidence digest");
    assert!(evidence.validate_contract());
    assert!(!evidence.authority.is_human_confirmation());

    evidence.authority = ProceduralFeedbackAuthorityV1::HumanConfirmed {
        actor_subject_id: "human-user".to_string(),
        operation_id: "confirm-turn-a".to_string(),
        evidence_digest: evidence.canonical_confirmation_payload_digest().unwrap(),
    };
    evidence.learning_evidence_digest = evidence.canonical_digest().expect("confirmed digest");
    assert!(evidence.validate_contract());
    assert!(evidence.authority.is_human_confirmation());
}

#[test]
fn confirmation_cannot_claim_an_unrelated_evidence_digest() {
    let mut evidence = PostTurnLearningEvidenceV1 {
        schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
        memory_space_id: "space-a".into(),
        mounted_subject_id: "subject-a".into(),
        conversation_id: "conversation-a".into(),
        turn_id: "turn-a".into(),
        canonical_turn_digest: digest('c'),
        tool_call_count: 0,
        selection_receipt: None,
        runtime_skill_feedback: vec![],
        agent_skill_feedback: vec![],
        task_learning_feedback: vec![],
        agent_tool_feedback: vec![],
        authority: ProceduralFeedbackAuthorityV1::HumanConfirmed {
            actor_subject_id: "human-a".into(),
            operation_id: "confirm-a".into(),
            evidence_digest: digest('d'),
        },
        learning_evidence_digest: String::new(),
    };
    evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
    assert!(
        !evidence.validate_contract(),
        "a claimed confirmation must bind the actual evidence payload"
    );
}

#[test]
fn standard_skill_feedback_requires_the_actual_selection_receipt() {
    let mut evidence = PostTurnLearningEvidenceV1 {
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
        authority: ProceduralFeedbackAuthorityV1::HostRuntimeObservation,
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
        ProceduralFeedbackJobV1::pending(identity.clone(), 1, digest('1'), digest('2'), 3, 5, 100)
            .expect("pending job");
    assert_eq!(job.status, ProceduralFeedbackJobStatusV1::Pending);
    assert!(job.validate().is_ok());

    let other_subject = ProceduralFeedbackJobV1::pending(
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
        ProceduralFeedbackJobV1::pending(identity, 1, digest('1'), digest('3'), 3, 5, 100)
            .expect("other evidence job");
    assert_ne!(job.job_id, other_evidence.job_id);
}

#[test]
fn procedural_receipt_accounts_for_every_submitted_item() {
    let receipt = ProceduralFeedbackReceiptV1 {
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
        deferred_count: 1,
        rejected_count: 1,
        changed_count: 2,
        reason_digest: digest('8'),
        completed_at: 100,
    };
    assert!(receipt.validate_contract());

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
        ProceduralFeedbackJobV1::pending(identity.clone(), 1, digest('1'), digest('2'), 1, 5, 100)
            .expect("pending job");
    let mut index = ProceduralFeedbackScopeIndexV1::empty(&identity, 100);
    index
        .active_jobs
        .push(ProceduralFeedbackJobRefV1::from_job(&job));
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
    let job = ProceduralFeedbackJobV1::pending(identity, 1, digest('1'), digest('2'), 1, 5, 100)
        .expect("pending job");
    let owner = GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::AgentToolExperience,
            "agent_tool_experience:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        1,
    )
    .expect("owner revision");
    let ledger = ProceduralFeedbackApplicationLedgerV1::build(
        &job,
        vec![
            bm_core::memory::ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                owner_revision: owner,
                content_digest: digest('e'),
            },
        ],
        101,
    )
    .expect("application ledger");
    assert_eq!(ledger.job_id, job.job_id);
    assert_eq!(
        ledger.learning_evidence_digest,
        job.learning_evidence_digest
    );
    assert!(ledger.validate().is_ok());
}
