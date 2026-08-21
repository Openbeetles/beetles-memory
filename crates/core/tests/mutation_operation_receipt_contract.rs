use bm_core::memory::{
    MemoryMutationAuditRecord, MemoryMutationEffect, MemoryMutationOperationIdentity,
    MemoryMutationOperationKind, MemoryMutationReceipt, MemoryMutationReplayDecision,
};
use bm_core::ErrorClass;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EFFECT_DIGEST: &str =
    "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const TX_DIGEST: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn identity() -> MemoryMutationOperationIdentity {
    MemoryMutationOperationIdentity::new(
        "host-operation-0001",
        "memory-space-main",
        "subject-agent-main",
        "subject-human-main",
        MemoryMutationOperationKind::Write,
    )
    .expect("canonical identity")
}

#[test]
fn operation_identity_is_scope_bound_and_rejects_noncanonical_components() {
    let identity = identity();
    assert_ne!(identity.operation_id_digest(), "host-operation-0001");
    assert!(identity.operation_id_digest().starts_with("sha256:"));
    assert_eq!(identity.memory_space_id(), "memory-space-main");
    assert_eq!(identity.mounted_subject_id(), "subject-agent-main");
    assert_eq!(identity.actor_subject_id(), "subject-human-main");
    assert!(identity.storage_key().starts_with("sha256:"));

    let other_actor = MemoryMutationOperationIdentity::new(
        "host-operation-0001",
        "memory-space-main",
        "subject-agent-main",
        "subject-other-human",
        MemoryMutationOperationKind::Write,
    )
    .expect("other actor identity");
    assert_ne!(
        identity.storage_key(),
        other_actor.storage_key(),
        "operation receipt keys must be actor-bound"
    );
    assert_ne!(
        identity.storage_key(),
        MemoryMutationOperationIdentity::new(
            "host-operation-0001",
            "memory-space-main",
            "subject-agent-main",
            "subject-human-main",
            MemoryMutationOperationKind::LongTermControl {
                operation: bm_core::memory::LongTermControlOperation::Correct,
            },
        )
        .expect("other operation kind identity")
        .storage_key(),
        "operation receipt keys must be operation-kind-bound"
    );

    let error = MemoryMutationOperationIdentity::new(
        " host-operation-0001",
        "memory-space-main",
        "subject-agent-main",
        "subject-human-main",
        MemoryMutationOperationKind::Write,
    )
    .expect_err("whitespace drift must fail closed");
    assert_eq!(error.class(), Some(ErrorClass::InvalidInput));
}

#[test]
fn committed_receipt_replays_only_the_same_canonical_intent() {
    let receipt = MemoryMutationReceipt::new(
        identity(),
        DIGEST_A,
        EFFECT_DIGEST,
        TX_DIGEST,
        MemoryMutationEffect::Changed,
        2,
        1_800_000_000,
    )
    .expect("receipt");

    assert_eq!(
        receipt
            .classify_replay(&identity(), DIGEST_A)
            .expect("same intent"),
        MemoryMutationReplayDecision::Replay
    );

    let collision = receipt
        .classify_replay(&identity(), DIGEST_B)
        .expect_err("one operation identity cannot alias another intent");
    assert_eq!(collision.class(), Some(ErrorClass::Conflict));
}

#[test]
fn receipt_rejects_untyped_or_uncommitted_proof_fields() {
    assert!(MemoryMutationReceipt::new(
        identity(),
        "not-a-digest",
        EFFECT_DIGEST,
        TX_DIGEST,
        MemoryMutationEffect::Changed,
        1,
        1_800_000_000,
    )
    .is_err());
    assert!(MemoryMutationReceipt::new(
        identity(),
        DIGEST_A,
        EFFECT_DIGEST,
        TX_DIGEST,
        MemoryMutationEffect::Changed,
        1,
        0,
    )
    .is_err());
}

#[test]
fn authoritative_audit_record_is_actor_and_receipt_bound() {
    let audit = MemoryMutationAuditRecord::new(
        identity(),
        DIGEST_A,
        EFFECT_DIGEST,
        TX_DIGEST,
        MemoryMutationEffect::Changed,
        2,
        "subject-human-main",
        1_800_000_000,
    )
    .expect("authoritative audit");
    assert_eq!(audit.actor_subject_id, "subject-human-main");
    assert_eq!(audit.audit_record_id, identity().storage_key());
}

#[test]
fn persisted_receipt_and_audit_never_contain_the_raw_caller_operation_id() {
    let receipt = MemoryMutationReceipt::new(
        identity(),
        DIGEST_A,
        EFFECT_DIGEST,
        TX_DIGEST,
        MemoryMutationEffect::Changed,
        1,
        1_800_000_000,
    )
    .expect("receipt");
    let audit = MemoryMutationAuditRecord::new(
        identity(),
        DIGEST_A,
        EFFECT_DIGEST,
        TX_DIGEST,
        MemoryMutationEffect::Changed,
        1,
        "subject-human-main",
        1_800_000_000,
    )
    .expect("audit");

    let persisted = format!(
        "{}\n{}",
        serde_json::to_string(&receipt).expect("receipt json"),
        serde_json::to_string(&audit).expect("audit json")
    );
    assert!(!persisted.contains("host-operation-0001"));
}
