use bm_core::memory::{
    GovernedWriteDecision, MemoryWriteAuthority, PostTurnGovernanceExecutionBindingV1,
    PostTurnGovernanceExecutionBlockReasonV1, PostTurnGovernanceIdentityV2,
    PostTurnGovernanceJobRefV2, PostTurnGovernanceJobStatusV2, PostTurnGovernanceJobV3,
    PostTurnGovernancePrivacyAuthorityV1, PostTurnGovernanceReconciliationCursorV1,
    PostTurnGovernanceScopeIndexV3, PostTurnPrivateGardenReport, PrivateGardenAdmissionDecision,
    MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS,
};

#[test]
fn private_garden_freeform_report_is_not_governed_write_decision() {
    let report = PostTurnPrivateGardenReport {
        attempted: true,
        executed: true,
        authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
        admission: PrivateGardenAdmissionDecision::Applied,
        writes: 1,
        moves: 0,
        deletes: 0,
        manifest: Vec::new(),
        skipped_reason: None,
    };

    assert_eq!(
        report.authority,
        MemoryWriteAuthority::LlmPrivateGardenFreeform
    );
    assert_eq!(report.admission, PrivateGardenAdmissionDecision::Applied);
    assert_ne!(
        format!("{:?}", report.admission),
        format!("{:?}", GovernedWriteDecision::Accepted)
    );
}

fn governance_job(conversation_id: &str) -> PostTurnGovernanceJobV3 {
    PostTurnGovernanceJobV3::pending(
        PostTurnGovernanceIdentityV2::new(
            "space:owner-default",
            "agent:governance",
            "llm.gateway",
            "chat-a",
            conversation_id,
            "turn-1",
        )
        .expect("identity"),
        1,
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "sha256:2222222222222222222222222222222222222222222222222222222222222222",
        PostTurnGovernanceExecutionBindingV1::Unbound,
        PostTurnGovernancePrivacyAuthorityV1 {
            policy_schema_version: 1,
            exact_policy_digest:
                "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                    .to_string(),
        },
        vec!["candidate-1".to_string()],
        0,
        5,
        1_800_000_000,
    )
    .expect("pending job")
}

#[test]
fn unbound_job_is_durably_blocked_without_inventing_a_binding_revision() {
    let job = governance_job("conversation-unbound");

    assert_eq!(
        job.execution_binding,
        PostTurnGovernanceExecutionBindingV1::Unbound
    );
    assert_eq!(
        job.status,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration
    );
    let block = job
        .execution_block_authority
        .as_ref()
        .expect("unbound job must carry durable block authority");
    assert_eq!(
        block.typed_block_reason,
        PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable
    );
    assert_eq!(block.binding_id, None);
    assert_eq!(block.binding_revision, None);
    assert_eq!(block.credential_ref_safe_id, None);
    assert_eq!(job.next_attempt_at, None);
    job.validate().expect("unbound blocked job is canonical");
}

#[test]
fn governance_job_identity_includes_conversation_and_contains_no_raw_turn() {
    let first = governance_job("conversation-a");
    let second = governance_job("conversation-b");
    assert_ne!(first.job_id, second.job_id);
    assert_eq!(
        first.scope_index_key, second.scope_index_key,
        "one mounted Entry runtime needs one deterministic due index across conversations"
    );
    assert!(first.job_id.starts_with("ptgj2:"));
    assert_eq!(first.job_id.len(), 62);

    let encoded = serde_json::to_string(&first).expect("job json");
    assert!(!encoded.contains("叫我青川"));
    assert!(!encoded.contains("inputMessages"));
    assert!(!encoded.contains("assistantMessage"));
    assert!(!encoded.contains("turn\":"));
}

#[test]
fn governance_job_validator_rejects_lease_and_terminal_state_drift() {
    let mut leased_without_authority = governance_job("conversation-a");
    leased_without_authority.status = PostTurnGovernanceJobStatusV2::Leased;
    leased_without_authority.lease_owner = Some("worker-a".to_string());
    leased_without_authority.lease_until = Some(1_800_000_030);
    assert!(leased_without_authority.validate().is_err());

    let mut success_without_receipt = governance_job("conversation-a");
    success_without_receipt.status = PostTurnGovernanceJobStatusV2::Succeeded;
    success_without_receipt.next_attempt_at = None;
    success_without_receipt.terminal_at = Some(1_800_000_010);
    assert!(success_without_receipt.validate().is_err());
}

#[test]
fn governance_scope_index_is_exact_bounded_and_active_only() {
    let job = governance_job("conversation-a");
    let mut index = PostTurnGovernanceScopeIndexV3::empty(&job.identity, 1_800_000_000);
    index
        .active_jobs
        .push(PostTurnGovernanceJobRefV2::from_job(&job));
    assert!(index.validate().is_ok());
    index
        .set_reconciliation_cursor(PostTurnGovernanceReconciliationCursorV1 {
            conversation_id: "conversation-a".to_string(),
            sequence: 1,
            turn_id: "turn-a".to_string(),
        })
        .expect("conversation a cursor");
    index
        .set_reconciliation_cursor(PostTurnGovernanceReconciliationCursorV1 {
            conversation_id: "conversation-b".to_string(),
            sequence: 1,
            turn_id: "turn-b".to_string(),
        })
        .expect("conversation b cursor");
    assert_eq!(index.reconciliation_cursors.len(), 2);

    index
        .active_jobs
        .push(PostTurnGovernanceJobRefV2::from_job(&job));
    assert!(index.validate().is_err());

    index.active_jobs.clear();
    for sequence in 0..=MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS {
        let mut reference = PostTurnGovernanceJobRefV2::from_job(&job);
        reference.job_id = format!("ptgj2:{sequence:056x}");
        reference.job_key.clone_from(&reference.job_id);
        index.active_jobs.push(reference);
    }
    assert!(index.validate().is_err());
}
