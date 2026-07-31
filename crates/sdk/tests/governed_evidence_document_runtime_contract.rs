#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::{Arc, Barrier};
use std::thread;

use bm_core::memory::canonical_recall_evidence_group;
use bm_sdk::nonproduction_replay_harness::{
    MemoryStoreEventKind, StoreEventScope, StoreJsonPrecondition, StoreMutation, StoreMutationBatch,
};
use bm_sdk::{
    default_memory_space_id, governed_evidence_document_content_digest,
    GovernedEvidenceDocumentChunk, GovernedEvidenceDocumentDraft,
    GovernedEvidenceDocumentSourceKind, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    LongTermMemoryDraft, LongTermMemoryKind, MemoryEvidenceAuthority,
    MemoryEvidenceDocumentMutation, MemoryEvidenceDocumentReadRequest, MemoryEvidenceRefVisibility,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest, MemoryWriteRequest,
    ParsedLongTermMemoryExtraction, PressureLevel, RuntimeLifecycleModeInput,
};
use serde_json::Value;

use support::{
    empty_store_platform, test_runtime_with_identity_scope, test_runtime_with_scope,
    test_runtime_with_scope_and_subject,
};

const MEMORY_SPACE_ID: &str = "space:owner-default";
const EVIDENCE_NAMESPACE: &str = "governed_evidence_documents";
const EVIDENCE_SOURCE_REF_NAMESPACE: &str = "governed_evidence_source_refs";

fn evidence_draft(
    document_id: &str,
    source_revision: u64,
    body: &str,
) -> Box<GovernedEvidenceDocumentDraft> {
    let source_locator = format!("opaque://sdk-contract/{document_id}");
    evidence_draft_with_source(
        document_id,
        MEMORY_SPACE_ID,
        "agent:agent-main",
        &source_locator,
        source_revision,
        body,
    )
}

fn evidence_draft_with_source(
    document_id: &str,
    memory_space_id: &str,
    mounted_subject_id: &str,
    source_locator: &str,
    source_revision: u64,
    body: &str,
) -> Box<GovernedEvidenceDocumentDraft> {
    let canonical_evidence_group =
        canonical_recall_evidence_group(&format!("sdk_contract:{document_id}"));
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:contract".to_string(),
        ordinal: 0,
        body: body.to_string(),
    }];
    Box::new(GovernedEvidenceDocumentDraft {
        memory_space_id: memory_space_id.to_string(),
        mounted_subject_id: mounted_subject_id.to_string(),
        document_id: document_id.to_string(),
        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
        source_locator: source_locator.to_string(),
        canonical_evidence_group: canonical_evidence_group.clone(),
        evidence_family_group: None,
        source_revision,
        body: body.to_string(),
        content_digest: governed_evidence_document_content_digest(
            source_locator,
            &canonical_evidence_group,
            None,
            body,
            &chunks,
        ),
        chunks,
        authority: MemoryEvidenceAuthority::WorldObservation,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        observed_at: 1_799_999_900 + source_revision,
    })
}

fn owner_ref(document_id: &str) -> GovernedMemoryOwnerRef {
    GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::EvidenceDocument, document_id)
}

fn json_contains_owner_ref(value: &Value, expected: &GovernedMemoryOwnerRef) -> bool {
    match value {
        Value::Object(fields) => {
            let is_owner_ref = fields.get("owner_plane").and_then(Value::as_str)
                == Some(expected.owner_plane.as_str())
                && fields.get("owner_id").and_then(Value::as_str)
                    == Some(expected.owner_id.as_str());
            is_owner_ref
                || fields
                    .values()
                    .any(|value| json_contains_owner_ref(value, expected))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| json_contains_owner_ref(value, expected)),
        _ => false,
    }
}

fn namespace_has_owner_ref(
    platform: &bm_sdk::MemoryStoreHandle,
    namespace: &str,
    expected: &GovernedMemoryOwnerRef,
) -> bool {
    platform
        .replay_harness()
        .read_json_namespace(namespace)
        .expect("read governed namespace")
        .iter()
        .any(|doc| json_contains_owner_ref(&doc.value, expected))
}

fn graph_has_owner_ref(
    platform: &bm_sdk::MemoryStoreHandle,
    expected: &GovernedMemoryOwnerRef,
) -> bool {
    platform
        .replay_harness()
        .export_store_snapshot()
        .expect("export graph snapshot")
        .json_docs
        .iter()
        .filter(|doc| doc.namespace.starts_with("memory_graph_"))
        .any(|doc| json_contains_owner_ref(&doc.value, expected))
}

fn assert_transaction_has_lifecycle_event(
    platform: &bm_sdk::MemoryStoreHandle,
    transaction_id: &str,
) {
    let events = platform
        .replay_harness()
        .read_events()
        .expect("read events");
    assert!(events.iter().any(|event| {
        event.kind_name == "runtime.lifecycle"
            && event.payload.get("transaction_id").map(String::as_str) == Some(transaction_id)
    }));
}

fn state_and_event_fingerprints(platform: &bm_sdk::MemoryStoreHandle) -> (String, String) {
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("export snapshot");
    (snapshot.state_fingerprint(), snapshot.event_fingerprint())
}

fn source_claim_values(platform: &bm_sdk::MemoryStoreHandle) -> Vec<Value> {
    platform
        .replay_harness()
        .read_json_namespace(EVIDENCE_SOURCE_REF_NAMESPACE)
        .expect("read governed evidence source claims")
        .into_iter()
        .map(|doc| doc.value)
        .collect()
}

#[test]
fn evidence_document_batch_atomically_commits_owner_facet_graph_and_lifecycle() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-contract",
    );
    let document_ids = ["evidence:batch:a", "evidence:batch:b"];

    let report = runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: document_ids
                .iter()
                .map(|document_id| MemoryEvidenceDocumentMutation::Upsert {
                    draft: evidence_draft(
                        document_id,
                        1,
                        "Governed evidence batches close owner, facet, graph, and lifecycle state.",
                    ),
                })
                .collect(),
        })
        .expect("write evidence document batch");

    assert!(report.accepted);
    assert_eq!(report.changed, 2);
    let summary = report.evidence_documents.expect("typed evidence summary");
    assert_eq!(summary.submitted, 2);
    assert_eq!(summary.created, 2);
    assert_eq!(summary.updated, 0);
    assert_eq!(summary.unchanged, 0);
    assert_eq!(summary.deleted, 0);
    assert_eq!(
        summary.owner_refs,
        document_ids
            .iter()
            .map(|id| owner_ref(id))
            .collect::<Vec<_>>()
    );
    let transaction = report.transaction.expect("evidence transaction");
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);
    assert_transaction_has_lifecycle_event(&platform, &transaction.transaction_id);

    let owners = platform
        .replay_harness()
        .read_json_namespace(EVIDENCE_NAMESPACE)
        .expect("read evidence owners");
    assert_eq!(owners.len(), 2);
    for document_id in document_ids {
        let expected = owner_ref(document_id);
        assert!(namespace_has_owner_ref(
            &platform,
            "memory_facet_indexes",
            &expected
        ));
        assert!(namespace_has_owner_ref(
            &platform,
            EVIDENCE_SOURCE_REF_NAMESPACE,
            &expected
        ));
        assert!(graph_has_owner_ref(&platform, &expected));
    }
}

#[test]
fn same_source_revision_and_digest_is_a_noop() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-noop",
    );
    let draft = evidence_draft(
        "evidence:noop",
        1,
        "The same source revision and digest must be replay-safe.",
    );
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: draft.clone(),
            }],
        })
        .expect("create evidence owner");
    let before = state_and_event_fingerprints(&platform);

    let report = runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert { draft }],
        })
        .expect("replay identical evidence owner");

    assert!(report.accepted);
    assert_eq!(report.changed, 0);
    let summary = report.evidence_documents.expect("typed evidence summary");
    assert_eq!(summary.submitted, 1);
    assert_eq!(summary.created, 0);
    assert_eq!(summary.updated, 0);
    assert_eq!(summary.unchanged, 1);
    assert_eq!(summary.deleted, 0);
    assert_eq!(state_and_event_fingerprints(&platform).0, before.0);
}

#[test]
fn typed_source_ref_drift_is_rejected_by_snapshot_import_with_zero_delta() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-source-ref-drift",
    );
    let draft = evidence_draft(
        "evidence:source-ref-drift",
        1,
        "A typed source ref must stay exactly bound to its governed owner.",
    );
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: draft.clone(),
            }],
        })
        .expect("create evidence owner");

    let mut corrupted = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("export evidence snapshot");
    let source_ref = corrupted
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == EVIDENCE_SOURCE_REF_NAMESPACE)
        .expect("typed evidence source ref");
    source_ref.value["owner_revision"] = serde_json::json!(99);
    let before = state_and_event_fingerprints(&platform);

    platform
        .replay_harness()
        .import_store_snapshot(&corrupted)
        .expect_err("snapshot import must reject a drifted typed source claim");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}

#[test]
fn same_source_revision_with_a_different_digest_fails_closed() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-revision-conflict",
    );
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft("evidence:revision-conflict", 1, "Original evidence body."),
            }],
        })
        .expect("create evidence owner");
    let before = state_and_event_fingerprints(&platform);

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft(
                    "evidence:revision-conflict",
                    1,
                    "Conflicting body at the same source revision.",
                ),
            }],
        })
        .expect_err("same source revision with a new digest must fail closed");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}

#[test]
fn duplicate_document_identity_in_one_batch_fails_closed() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-duplicate",
    );
    let before = state_and_event_fingerprints(&platform);

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: evidence_draft("evidence:duplicate", 1, "First duplicate payload."),
                },
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: evidence_draft("evidence:duplicate", 2, "Second duplicate payload."),
                },
            ],
        })
        .expect_err("duplicate evidence identity must reject the whole batch");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}

#[test]
fn batch_duplicate_source_identity_for_different_documents_fails_closed() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-duplicate-source-batch",
    );
    let source_locator = "opaque://sdk-contract/shared-source";
    let before = state_and_event_fingerprints(&platform);

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: evidence_draft_with_source(
                        "evidence:source-duplicate:a",
                        MEMORY_SPACE_ID,
                        "agent:agent-main",
                        source_locator,
                        1,
                        "First owner tries to claim the shared source identity.",
                    ),
                },
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: evidence_draft_with_source(
                        "evidence:source-duplicate:b",
                        MEMORY_SPACE_ID,
                        "agent:agent-main",
                        source_locator,
                        1,
                        "Second owner tries to claim the same source identity.",
                    ),
                },
            ],
        })
        .expect_err("different documents must not share one source locator/revision claim");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}

#[test]
fn cross_transaction_duplicate_source_identity_fails_closed_with_zero_delta() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-duplicate-source-cross-txn",
    );
    let source_locator = "opaque://sdk-contract/cross-transaction-source";
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    "evidence:source-cross:a",
                    MEMORY_SPACE_ID,
                    "agent:agent-main",
                    source_locator,
                    7,
                    "The first document owns this source identity claim.",
                ),
            }],
        })
        .expect("create first evidence source claim");
    let before = state_and_event_fingerprints(&platform);

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    "evidence:source-cross:b",
                    MEMORY_SPACE_ID,
                    "agent:agent-main",
                    source_locator,
                    7,
                    "A second document cannot steal the same source identity claim.",
                ),
            }],
        })
        .expect_err("second document must not acquire an occupied source claim");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}

#[test]
fn concurrent_writers_to_same_source_claim_allow_only_one_commit() {
    let platform = empty_store_platform(support::host_test_profile());
    let source_locator = "opaque://sdk-contract/concurrent-source";
    let barrier = Arc::new(Barrier::new(2));
    let handles = ["evidence:source-race:a", "evidence:source-race:b"]
        .into_iter()
        .map(|document_id| {
            let platform = platform.clone();
            let barrier = Arc::clone(&barrier);
            let source_locator = source_locator.to_string();
            thread::spawn(move || {
                let runtime = test_runtime_with_scope(
                    platform,
                    support::host_test_profile(),
                    "llm.gateway",
                    &format!("evidence-source-race-{document_id}"),
                );
                barrier.wait();
                runtime.write(MemoryWriteRequest::GovernedEvidenceDocuments {
                    mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                        draft: evidence_draft_with_source(
                            document_id,
                            MEMORY_SPACE_ID,
                            "agent:agent-main",
                            &source_locator,
                            1,
                            "Only one concurrent writer may acquire this source identity claim.",
                        ),
                    }],
                })
            })
        })
        .collect::<Vec<_>>();

    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().expect("writer thread"))
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
    assert_eq!(source_claim_values(&platform).len(), 1);
}

#[test]
fn source_revision_update_atomically_replaces_the_claim() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-source-revision-update",
    );
    let source_locator = "opaque://sdk-contract/revision-update-source";
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    "evidence:source-revision",
                    MEMORY_SPACE_ID,
                    "agent:agent-main",
                    source_locator,
                    1,
                    "Initial source claim revision.",
                ),
            }],
        })
        .expect("create initial source claim");
    let before_claims = source_claim_values(&platform);
    assert_eq!(before_claims.len(), 1);

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    "evidence:source-revision",
                    MEMORY_SPACE_ID,
                    "agent:agent-main",
                    source_locator,
                    2,
                    "Replacement source claim revision.",
                ),
            }],
        })
        .expect("replace source claim revision");

    let after_claims = source_claim_values(&platform);
    assert_eq!(after_claims.len(), 1);
    assert_ne!(after_claims, before_claims);
    assert_eq!(after_claims[0]["source_revision"].as_u64(), Some(2));
    assert_eq!(after_claims[0]["owner_revision"].as_u64(), Some(2));
    assert!(!after_claims[0].to_string().contains(source_locator));
}

#[test]
fn soul_private_evidence_document_write_fails_closed_with_zero_delta() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-soul-private",
    );
    let mut draft = evidence_draft(
        "evidence:soul-private",
        1,
        "Soul private evidence documents are not projection-visible governed evidence.",
    );
    draft.privacy = MemoryPrivacyClass::SoulPrivate;
    let before = state_and_event_fingerprints(&platform);

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert { draft }],
        })
        .expect_err("SoulPrivate governed evidence documents must fail closed");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}

#[test]
fn mixed_batch_with_soul_private_evidence_fails_closed_with_zero_delta() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-mixed-soul-private",
    );
    let valid = evidence_draft(
        "evidence:mixed-soul-private:valid",
        1,
        "A valid sibling must not commit when another draft violates privacy.",
    );
    let mut soul_private = evidence_draft(
        "evidence:mixed-soul-private:private",
        1,
        "Soul private evidence cannot enter the projection-visible evidence plane.",
    );
    soul_private.privacy = MemoryPrivacyClass::SoulPrivate;
    let before = state_and_event_fingerprints(&platform);

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![
                MemoryEvidenceDocumentMutation::Upsert { draft: valid },
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: soul_private,
                },
            ],
        })
        .expect_err("a mixed batch containing SoulPrivate evidence must fail closed");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}

#[test]
fn newer_revision_cannot_remount_an_existing_evidence_owner() {
    let platform = empty_store_platform(support::host_test_profile());
    let subject_a = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-remount-subject-a",
    );
    let subject_b = test_runtime_with_scope_and_subject(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-remount-subject-b",
        "agent:agent-secondary",
    );
    let document_id = "evidence:subject-remount";
    let source_locator = "opaque://sdk-contract/subject-remount";
    subject_a
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    document_id,
                    MEMORY_SPACE_ID,
                    "agent:agent-main",
                    source_locator,
                    1,
                    "The evidence owner is mounted to subject A.",
                ),
            }],
        })
        .expect("create subject A evidence owner");
    let before = state_and_event_fingerprints(&platform);

    subject_b
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    document_id,
                    MEMORY_SPACE_ID,
                    "agent:agent-secondary",
                    source_locator,
                    2,
                    "A newer revision cannot silently remount the existing owner.",
                ),
            }],
        })
        .expect_err("cross-subject evidence remount must fail closed");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}

#[test]
fn same_source_identity_is_scoped_by_subject_and_memory_space() {
    let platform = empty_store_platform(support::host_test_profile());
    let source_locator = "opaque://sdk-contract/scoped-source-boundary";
    let subject_a = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-source-subject-a",
    );
    let subject_b = test_runtime_with_scope_and_subject(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-source-subject-b",
        "agent:agent-secondary",
    );
    let other_space_id = default_memory_space_id("owner-other");
    let other_space = test_runtime_with_identity_scope(
        platform.clone(),
        support::host_test_profile(),
        "agent-main",
        "owner-other",
        "llm.gateway",
        "evidence-source-space-b",
    );

    subject_a
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    "evidence:source-boundary:subject-a",
                    MEMORY_SPACE_ID,
                    "agent:agent-main",
                    source_locator,
                    1,
                    "Subject A owns this scoped source identity.",
                ),
            }],
        })
        .expect("subject A claim");
    subject_b
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    "evidence:source-boundary:subject-b",
                    MEMORY_SPACE_ID,
                    "agent:agent-secondary",
                    source_locator,
                    1,
                    "Subject B may use the same locator and revision in its own scope.",
                ),
            }],
        })
        .expect("subject B claim");
    other_space
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    "evidence:source-boundary:space-b",
                    &other_space_id,
                    "agent:agent-main",
                    source_locator,
                    1,
                    "Another memory space may use the same locator and revision.",
                ),
            }],
        })
        .expect("other memory space claim");

    assert_eq!(source_claim_values(&platform).len(), 3);
    let subject_a_view = subject_a
        .read_governed_evidence_documents(MemoryEvidenceDocumentReadRequest {
            memory_space_id: MEMORY_SPACE_ID.to_string(),
            document_ids: vec!["evidence:source-boundary:subject-a".to_string()],
        })
        .expect("subject A view")
        .documents
        .remove(0)
        .source_locator_view;
    let subject_b_view = subject_b
        .read_governed_evidence_documents(MemoryEvidenceDocumentReadRequest {
            memory_space_id: MEMORY_SPACE_ID.to_string(),
            document_ids: vec!["evidence:source-boundary:subject-b".to_string()],
        })
        .expect("subject B view")
        .documents
        .remove(0)
        .source_locator_view;
    let other_space_view = other_space
        .read_governed_evidence_documents(MemoryEvidenceDocumentReadRequest {
            memory_space_id: other_space_id,
            document_ids: vec!["evidence:source-boundary:space-b".to_string()],
        })
        .expect("other space view")
        .documents
        .remove(0)
        .source_locator_view;
    let opaque_refs = [subject_a_view, subject_b_view, other_space_view]
        .into_iter()
        .map(|view| view.reference.expect("scoped opaque source reference"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        opaque_refs.len(),
        3,
        "opaque source identity must remain scoped"
    );
    assert!(opaque_refs.iter().all(|reference| {
        reference.starts_with("opaque:governed-source:") && !reference.contains(source_locator)
    }));
}

#[test]
fn snapshot_round_trip_preserves_same_document_id_in_distinct_memory_spaces() {
    let source = empty_store_platform(support::host_test_profile());
    let primary = test_runtime_with_scope(
        source.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-snapshot-primary-space",
    );
    let secondary_space_id = default_memory_space_id("owner-secondary");
    let secondary = test_runtime_with_identity_scope(
        source.clone(),
        support::host_test_profile(),
        "agent-main",
        "owner-secondary",
        "llm.gateway",
        "evidence-snapshot-secondary-space",
    );
    let document_id = "evidence:same-id-across-spaces";

    primary
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    document_id,
                    MEMORY_SPACE_ID,
                    "agent:agent-main",
                    "opaque://sdk-contract/snapshot-primary",
                    1,
                    "Primary memory-space evidence.",
                ),
            }],
        })
        .expect("write primary space evidence");
    secondary
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    document_id,
                    &secondary_space_id,
                    "agent:agent-main",
                    "opaque://sdk-contract/snapshot-secondary",
                    1,
                    "Secondary memory-space evidence.",
                ),
            }],
        })
        .expect("write secondary space evidence");
    let snapshot = source
        .replay_harness()
        .export_store_snapshot()
        .expect("export cross-space snapshot");
    let target = empty_store_platform(support::host_test_profile());

    target
        .replay_harness()
        .import_store_snapshot(&snapshot)
        .expect("import cross-space snapshot");

    let documents = target
        .replay_harness()
        .read_json_namespace(EVIDENCE_NAMESPACE)
        .expect("read imported evidence documents");
    assert_eq!(documents.len(), 2);
    assert_eq!(source_claim_values(&target).len(), 2);
}

#[test]
fn cross_space_exact_precondition_cannot_expand_evidence_effect_closure() {
    let platform = empty_store_platform(support::host_test_profile());
    let primary = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-effect-closure-primary",
    );
    let secondary_space_id = default_memory_space_id("owner-secondary");
    let secondary = test_runtime_with_identity_scope(
        platform.clone(),
        support::host_test_profile(),
        "agent-main",
        "owner-secondary",
        "llm.gateway",
        "evidence-effect-closure-secondary",
    );
    let document_id = "evidence:effect-closure-same-id";

    primary
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    document_id,
                    MEMORY_SPACE_ID,
                    "agent:agent-main",
                    "opaque://sdk-contract/effect-closure-primary",
                    1,
                    "Primary owner and claim must remain in the primary scope.",
                ),
            }],
        })
        .expect("create primary evidence owner");
    secondary
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft_with_source(
                    document_id,
                    &secondary_space_id,
                    "agent:agent-main",
                    "opaque://sdk-contract/effect-closure-secondary",
                    1,
                    "Secondary owner shares only the logical document id.",
                ),
            }],
        })
        .expect("create secondary evidence owner");

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("export cross-space evidence snapshot");
    let primary_owner = snapshot
        .json_docs
        .iter()
        .find(|doc| {
            doc.namespace == EVIDENCE_NAMESPACE
                && doc.value["memory_space_id"].as_str() == Some(MEMORY_SPACE_ID)
                && doc.value["document_id"].as_str() == Some(document_id)
        })
        .expect("primary evidence owner")
        .clone();
    let secondary_owner = snapshot
        .json_docs
        .iter()
        .find(|doc| {
            doc.namespace == EVIDENCE_NAMESPACE
                && doc.value["memory_space_id"].as_str() == Some(secondary_space_id.as_str())
                && doc.value["document_id"].as_str() == Some(document_id)
        })
        .expect("secondary evidence owner")
        .clone();
    let primary_claim = snapshot
        .json_docs
        .iter()
        .find(|doc| {
            doc.namespace == EVIDENCE_SOURCE_REF_NAMESPACE
                && doc.value["memory_space_id"].as_str() == Some(MEMORY_SPACE_ID)
                && doc.value["owner_ref"]["owner_id"].as_str() == Some(document_id)
        })
        .expect("primary evidence source claim")
        .clone();
    let secondary_claim = snapshot
        .json_docs
        .iter()
        .find(|doc| {
            doc.namespace == EVIDENCE_SOURCE_REF_NAMESPACE
                && doc.value["memory_space_id"].as_str() == Some(secondary_space_id.as_str())
                && doc.value["owner_ref"]["owner_id"].as_str() == Some(document_id)
        })
        .expect("secondary evidence source claim")
        .clone();
    let before = state_and_event_fingerprints(&platform);

    let error = platform
        .replay_harness()
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: "txn-cross-space-effect-closure-forgery".to_string(),
                operation: "test.cross_space_effect_closure".to_string(),
                scope: StoreEventScope::system("test.cross_space_effect_closure")
                    .with_memory_space(MEMORY_SPACE_ID)
                    .with_subject("agent:agent-main"),
                mutations: vec![
                    StoreMutation::PutJson {
                        namespace: EVIDENCE_NAMESPACE.to_string(),
                        key: primary_owner.key.clone(),
                        value: primary_owner.value.clone(),
                        event_kind: MemoryStoreEventKind::MemoryWrite,
                        plane: EVIDENCE_NAMESPACE.to_string(),
                        record_key: document_id.to_string(),
                    },
                    StoreMutation::PutJson {
                        namespace: EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                        key: primary_claim.key.clone(),
                        value: primary_claim.value.clone(),
                        event_kind: MemoryStoreEventKind::MemoryWrite,
                        plane: EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                        record_key: document_id.to_string(),
                    },
                    StoreMutation::DeleteJson {
                        namespace: EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                        key: secondary_claim.key.clone(),
                        event_kind: MemoryStoreEventKind::MemoryDelete,
                        plane: EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                        record_key: document_id.to_string(),
                    },
                ],
            },
            &[
                StoreJsonPrecondition::Exact {
                    namespace: EVIDENCE_NAMESPACE.to_string(),
                    key: secondary_owner.key.clone(),
                    value: secondary_owner.value.clone(),
                },
                StoreJsonPrecondition::Exact {
                    namespace: EVIDENCE_NAMESPACE.to_string(),
                    key: primary_owner.key.clone(),
                    value: primary_owner.value.clone(),
                },
                StoreJsonPrecondition::Exact {
                    namespace: EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                    key: primary_claim.key.clone(),
                    value: primary_claim.value.clone(),
                },
                StoreJsonPrecondition::Exact {
                    namespace: EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                    key: secondary_claim.key.clone(),
                    value: secondary_claim.value.clone(),
                },
            ],
        )
        .expect_err("cross-space source claim must be rejected by effect closure");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_evidence_source_ref_closure_invalid"
    );
    assert_eq!(state_and_event_fingerprints(&platform), before);
    let after = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("export rejected cross-space transaction snapshot");
    assert!(after.json_docs.iter().any(|doc| doc == &primary_owner));
    assert!(after.json_docs.iter().any(|doc| doc == &primary_claim));
}

#[test]
fn exact_scoped_detail_returns_only_the_safe_evidence_view() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform,
        support::host_test_profile(),
        "llm.gateway",
        "evidence-detail",
    );
    let draft = evidence_draft(
        "evidence:detail",
        7,
        "Exact detail reads expose governed evidence without physical addressing.",
    );
    let raw_source_locator = draft.source_locator.clone();
    let second = evidence_draft(
        "evidence:detail:second",
        1,
        "A batch read must observe the same immutable generation.",
    );
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: draft.clone(),
                },
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: second.clone(),
                },
            ],
        })
        .expect("create evidence owner");

    let report = runtime
        .read_governed_evidence_documents(MemoryEvidenceDocumentReadRequest {
            memory_space_id: MEMORY_SPACE_ID.to_string(),
            document_ids: vec![
                draft.document_id.clone(),
                second.document_id.clone(),
                "evidence:detail:missing".to_string(),
            ],
        })
        .expect("batch evidence detail");
    assert!(report.store_snapshot_consistent);
    assert_eq!(report.documents.len(), 2);
    assert_eq!(
        report.missing_document_ids,
        vec!["evidence:detail:missing".to_string()]
    );
    let view = report
        .documents
        .iter()
        .find(|view| view.owner_ref == owner_ref(&draft.document_id))
        .expect("evidence detail view");

    assert_eq!(view.owner_ref, owner_ref(&draft.document_id));
    assert_eq!(view.memory_space_id, draft.memory_space_id);
    assert_eq!(view.mounted_subject_id, draft.mounted_subject_id);
    assert_eq!(view.source_kind, draft.source_kind);
    assert_eq!(
        view.canonical_evidence_group,
        draft.canonical_evidence_group
    );
    assert_eq!(view.source_revision, draft.source_revision);
    assert_eq!(view.owner_revision, 1);
    assert_eq!(view.content_digest, draft.content_digest);
    assert_eq!(view.authority, draft.authority);
    assert_eq!(view.privacy, draft.privacy);
    assert_eq!(view.body, draft.body);
    assert_eq!(view.chunks, draft.chunks);
    assert_eq!(view.observed_at, draft.observed_at);
    assert!(view.created_at > 0);
    assert!(view.updated_at >= view.created_at);
    assert!(!view.shared_fact_surface_allowed);
    assert!(matches!(
        view.source_locator_view.visibility,
        MemoryEvidenceRefVisibility::GovernedOpaque | MemoryEvidenceRefVisibility::Redacted
    ));
    assert_ne!(
        view.source_locator_view.reference.as_deref(),
        Some(raw_source_locator.as_str())
    );
    let public_debug = format!("{report:?}");
    assert!(!public_debug.contains(&raw_source_locator));
    assert!(!public_debug.contains("physical_key"));
}

#[test]
fn same_owner_id_does_not_collide_across_long_term_and_evidence_document_planes() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-owner-plane",
    );
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "owner_plane_collision".to_string(),
                    content: "Typed owner planes prevent same-id collisions.".to_string(),
                    keywords: vec!["owner".to_string(), "plane".to_string()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("evidence-owner-plane".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["owner-plane-contract".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_799_999_900),
                    last_confirmed_at: Some(1_799_999_900),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("create long-term owner");
    let shared_owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store(MEMORY_SPACE_ID, "agent:agent-main")
        .expect("long-term store")
        .list(8)
        .expect("long-term owners")
        .into_iter()
        .find(|entry| entry.topic == "owner_plane_collision")
        .expect("long-term owner")
        .id;

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft(
                    &shared_owner_id,
                    1,
                    "Evidence owner with the same logical id remains a distinct plane.",
                ),
            }],
        })
        .expect("create same-id evidence owner");

    let long_term_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, &shared_owner_id);
    let evidence_ref = owner_ref(&shared_owner_id);
    assert_ne!(long_term_ref, evidence_ref);
    assert!(namespace_has_owner_ref(
        &platform,
        "memory_facet_indexes",
        &long_term_ref
    ));
    assert!(namespace_has_owner_ref(
        &platform,
        "memory_facet_indexes",
        &evidence_ref
    ));
    assert!(platform
        .replay_harness()
        .read_json_namespace(EVIDENCE_NAMESPACE)
        .expect("evidence owners")
        .iter()
        .any(|doc| doc.value["document_id"].as_str() == Some(shared_owner_id.as_str())));
}

#[test]
fn recall_decision_and_capsule_preserve_evidence_owner_and_block_shared_fact_surface() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform,
        support::host_test_profile(),
        "llm.gateway",
        "evidence-recall",
    );
    let document_id = "evidence:recall-owner";
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft(
                    document_id,
                    1,
                    "Zephyr quartz evidence capsules retain governed owner identity.",
                ),
            }],
        })
        .expect("create recall evidence owner");

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "zephyr quartz governed owner".to_string(),
            limit: 8,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("recall evidence owner");
    let expected = owner_ref(document_id);
    assert!(recall
        .delivery_report
        .selection_decisions
        .iter()
        .any(|decision| decision.owner_ref.as_ref() == Some(&expected) && decision.selected));
    assert!(recall
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.owner_ref == expected && !capsule.shared_fact_surface_allowed));
    assert!(recall.privacy_report.passed);
    assert_eq!(
        recall.privacy_report.checked_evidence_document_owner_count,
        1
    );
    assert_eq!(recall.privacy_report.checked_long_term_owner_count, 0);
}

#[test]
fn recall_snapshot_keeps_long_term_and_evidence_typed_owner_bindings_consistent() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "mixed-owner-recall",
    );
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "aurelia_basalt_owner_binding".to_string(),
                    content: "Aurelia basalt owner binding appears in long term memory."
                        .to_string(),
                    keywords: vec![
                        "aurelia".to_string(),
                        "basalt".to_string(),
                        "owner-binding".to_string(),
                    ],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("mixed-owner-recall".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["mixed-owner-long-term".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_799_999_900),
                    last_confirmed_at: Some(1_799_999_900),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("create long-term owner");
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft(
                    "evidence:mixed-owner-recall",
                    1,
                    "Aurelia basalt owner binding appears in governed evidence document.",
                ),
            }],
        })
        .expect("create evidence owner");

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "aurelia basalt owner binding".to_string(),
            limit: 8,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("mixed owner recall");

    assert!(recall.store_snapshot_consistent);
    assert!(recall.privacy_report.passed);
    assert!(recall.privacy_report.checked_long_term_owner_count >= 1);
    assert!(recall.privacy_report.checked_evidence_document_owner_count >= 1);
    assert!(recall
        .delivery_report
        .selection_decisions
        .iter()
        .any(|decision| {
            decision.selected
                && decision
                    .owner_ref
                    .as_ref()
                    .is_some_and(|owner| owner.owner_plane == GovernedMemoryOwnerPlane::LongTerm)
        }));
    assert!(recall
        .delivery_report
        .selection_decisions
        .iter()
        .any(|decision| {
            decision.selected
                && decision.owner_ref.as_ref().is_some_and(|owner| {
                    owner.owner_plane == GovernedMemoryOwnerPlane::EvidenceDocument
                })
        }));
    assert!(recall
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| { capsule.owner_ref.owner_plane == GovernedMemoryOwnerPlane::LongTerm }));
    assert!(recall
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| {
            capsule.owner_ref.owner_plane == GovernedMemoryOwnerPlane::EvidenceDocument
        }));
}

#[test]
fn concurrent_mixed_owner_updates_never_produce_a_torn_recall_snapshot() {
    let platform = empty_store_platform(support::host_test_profile());
    let reader = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "mixed-owner-snapshot-reader",
    );
    let seed_evidence = evidence_draft(
        "evidence:mixed-snapshot:seed",
        1,
        "Vermilion mica immutable snapshot evidence seed.",
    );
    reader
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: seed_evidence,
            }],
        })
        .expect("seed evidence owner");
    reader
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "mixed_snapshot_seed".to_string(),
                    content: "Vermilion mica immutable snapshot long-term seed.".to_string(),
                    keywords: vec!["vermilion".to_string(), "mica".to_string()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("mixed-owner-snapshot-writer".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["mixed-snapshot-seed".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_799_999_800),
                    last_confirmed_at: Some(1_799_999_800),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed long-term owner");

    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = Arc::clone(&barrier);
    let writer_platform = platform.clone();
    let writer = thread::spawn(move || {
        let runtime = test_runtime_with_scope(
            writer_platform,
            support::host_test_profile(),
            "llm.gateway",
            "mixed-owner-snapshot-writer",
        );
        writer_barrier.wait();
        for index in 0..16_u64 {
            runtime
                .write(MemoryWriteRequest::LongTermExtraction {
                    governed_skill_writes: Vec::new(),
                    runtime_skill_owning_scope: None,
                    extraction: ParsedLongTermMemoryExtraction {
                        upserts: vec![LongTermMemoryDraft {
                            kind: LongTermMemoryKind::Project,
                            topic: format!("mixed_snapshot_long_term_{index}"),
                            content: format!(
                                "Vermilion mica immutable snapshot long-term owner {index}."
                            ),
                            keywords: vec!["vermilion".to_string(), "mica".to_string()],
                            privacy: MemoryPrivacyClass::SharedWithSubject,
                            source_chat_id: Some("mixed-owner-snapshot-writer".to_string()),
                            source_type: None,
                            source_scope: None,
                            confidence: None,
                            freshness: None,
                            stale_hint: None,
                            supporting_citations: vec![format!("mixed-snapshot-{index}")],
                            canonical_entities: Vec::new(),
                            evidence_count: Some(1),
                            observed_at: Some(1_799_999_810 + index),
                            last_confirmed_at: Some(1_799_999_810 + index),
                            source_revision: Some(index + 2),
                        }],
                        deletes: Vec::new(),
                        skill_writes: Vec::new(),
                    },
                })
                .expect("append concurrent long-term owner");
            runtime
                .write(MemoryWriteRequest::GovernedEvidenceDocuments {
                    mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                        draft: evidence_draft(
                            &format!("evidence:mixed-snapshot:{index}"),
                            1,
                            &format!("Vermilion mica immutable snapshot evidence owner {index}."),
                        ),
                    }],
                })
                .expect("append concurrent evidence owner");
            thread::yield_now();
        }
    });

    barrier.wait();
    for _ in 0..64 {
        let recall = reader
            .recall(MemoryRecallRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                query: "vermilion mica immutable snapshot".to_string(),
                limit: 8,
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .expect("recall from one immutable mixed-owner snapshot");
        assert!(recall.store_snapshot_consistent);
        assert!(recall.privacy_report.passed);
        assert!(recall.delivery_report.integrity_failures.is_empty());
        assert_eq!(recall.graph_index_report.read_path_mutation_delta, 0);
    }
    writer.join().expect("mixed owner writer");
}

#[test]
fn projection_exposes_only_safe_delivery_counts_for_typed_evidence_owner() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform,
        support::host_test_profile(),
        "llm.gateway",
        "evidence-projection",
    );
    let document_id = "evidence:projection-owner";
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft(
                    document_id,
                    1,
                    "Orchid tungsten projection receipts bind typed evidence owners.",
                ),
            }],
        })
        .expect("create projection evidence owner");

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            user_query: "orchid tungsten typed evidence owner".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project evidence owner");
    assert!(projection
        .provider_payload()
        .system_memory_block()
        .contains("Orchid tungsten projection receipts bind typed evidence owners."));
    assert_eq!(projection.report().recall_delivery().selected_count, 1);
    assert_eq!(projection.report().recall_delivery().rendered_count, 1);
    assert!(projection
        .report()
        .recall_delivery()
        .integrity_failures
        .is_empty());
    assert!(!projection
        .report()
        .ui_api_projection()
        .contains(document_id));
}

#[test]
fn delete_atomically_removes_evidence_owner_facet_graph_membership_and_writes_lifecycle() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-delete",
    );
    let document_id = "evidence:delete";
    let expected = owner_ref(document_id);
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft(
                    document_id,
                    1,
                    "Deleting an evidence owner must cascade through governed indexes.",
                ),
            }],
        })
        .expect("create evidence owner");
    assert!(namespace_has_owner_ref(
        &platform,
        "memory_facet_indexes",
        &expected
    ));
    assert!(graph_has_owner_ref(&platform, &expected));

    let report = runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Delete {
                document_id: document_id.to_string(),
                expected_owner_revision: 1,
            }],
        })
        .expect("delete evidence owner");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let summary = report.evidence_documents.expect("typed evidence summary");
    assert_eq!(summary.submitted, 1);
    assert_eq!(summary.created, 0);
    assert_eq!(summary.updated, 0);
    assert_eq!(summary.unchanged, 0);
    assert_eq!(summary.deleted, 1);
    assert_eq!(summary.owner_refs, vec![expected.clone()]);
    let transaction = report.transaction.expect("delete transaction");
    assert_transaction_has_lifecycle_event(&platform, &transaction.transaction_id);
    assert!(platform
        .replay_harness()
        .read_json_namespace(EVIDENCE_NAMESPACE)
        .expect("evidence owners after delete")
        .is_empty());
    assert!(!namespace_has_owner_ref(
        &platform,
        "memory_facet_indexes",
        &expected
    ));
    assert!(!namespace_has_owner_ref(
        &platform,
        EVIDENCE_SOURCE_REF_NAMESPACE,
        &expected
    ));
    assert!(!graph_has_owner_ref(&platform, &expected));
}

#[test]
fn delete_with_the_wrong_expected_owner_revision_fails_closed() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "evidence-delete-conflict",
    );
    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: evidence_draft(
                    "evidence:delete-conflict",
                    1,
                    "Expected owner revision is mandatory delete CAS.",
                ),
            }],
        })
        .expect("create evidence owner");
    let before = state_and_event_fingerprints(&platform);

    runtime
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Delete {
                document_id: "evidence:delete-conflict".to_string(),
                expected_owner_revision: 2,
            }],
        })
        .expect_err("wrong expected owner revision must fail closed");

    assert_eq!(state_and_event_fingerprints(&platform), before);
}
