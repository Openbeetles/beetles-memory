#![allow(dead_code)]

use bm_core::memory::{
    build_long_term_memory_facet_index_doc, memory_facet_manifest_key,
    plan_long_term_memory_upsert, scoped_long_term_memory_storage_key,
    scoped_memory_facet_owner_storage_key, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryEntryPlan, MemoryFacetIndexManifest, MemoryFacetOwnerVersion,
    MemoryFacetPostingDoc, MemoryFacetPostingRevision, MEMORY_FACET_INDEX_NAMESPACE,
    MEMORY_FACET_POSTING_NAMESPACE, MEMORY_FACET_SCHEMA_VERSION,
};
use bm_sdk::nonproduction_replay_harness::{
    MemoryStoreEventKind, StoreEventScope, StoreJsonPrecondition, StoreMutation,
    StoreMutationBatch, StorePlatform,
};

pub fn seed_scoped_long_term(
    platform: &StorePlatform,
    memory_space_id: &str,
    draft: &LongTermMemoryDraft,
    now_secs: u64,
) -> LongTermMemoryEntry {
    let entry = match plan_long_term_memory_upsert(None, draft, now_secs) {
        LongTermMemoryEntryPlan::Created(entry) => entry,
        other => panic!("new test owner must be created, got {other:?}"),
    };
    let key =
        scoped_long_term_memory_storage_key(memory_space_id, &entry.id).expect("scoped owner key");
    let subject_id = "subject:test";
    let facet = build_long_term_memory_facet_index_doc(
        &entry,
        memory_space_id,
        vec![subject_id.to_string()],
        1,
    );
    let facet_key = scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, &entry.id)
        .expect("scoped facet owner key");
    let owner_version = MemoryFacetOwnerVersion {
        owner_record_id: entry.id.clone(),
        owner_revision: entry.owner_revision,
        facet_index_revision: facet.facet_index_revision,
    };
    let posting_keys = facet
        .posting_keys_for_subject(subject_id)
        .expect("facet posting keys");
    let mut mutations = vec![
        StoreMutation::PutJson {
            namespace: "long_term".to_string(),
            key: key.clone(),
            value: serde_json::to_value(&entry).expect("serialize owner"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "long_term".to_string(),
            record_key: entry.id.clone(),
        },
        StoreMutation::PutJson {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: facet_key.clone(),
            value: serde_json::to_value(&facet).expect("serialize facet owner"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            record_key: format!("facet-owner:{}", facet.owner_record_id),
        },
    ];
    let mut preconditions = vec![
        StoreJsonPrecondition::Absent {
            namespace: "long_term".to_string(),
            key: key.clone(),
        },
        StoreJsonPrecondition::Absent {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: facet_key.clone(),
        },
    ];
    let manifest_key =
        memory_facet_manifest_key(memory_space_id, subject_id).expect("facet manifest key");
    if !posting_keys.is_empty() {
        for posting_key in &posting_keys {
            let posting = MemoryFacetPostingDoc {
                schema_version: MEMORY_FACET_SCHEMA_VERSION,
                memory_space_id: memory_space_id.to_string(),
                subject_id: subject_id.to_string(),
                posting_key: posting_key.clone(),
                revision: 1,
                owner_versions: vec![owner_version.clone()],
            };
            mutations.push(StoreMutation::PutJson {
                namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                key: posting_key.clone(),
                value: serde_json::to_value(posting).expect("serialize posting"),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                record_key: posting_key.clone(),
            });
            preconditions.push(StoreJsonPrecondition::Absent {
                namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                key: posting_key.clone(),
            });
        }
        let manifest = MemoryFacetIndexManifest {
            schema_version: MEMORY_FACET_SCHEMA_VERSION,
            memory_space_id: memory_space_id.to_string(),
            subject_id: subject_id.to_string(),
            owner_doc_count: 1,
            posting_doc_count: posting_keys.len(),
            revision: 1,
            owner_versions: vec![owner_version],
            posting_revisions: posting_keys
                .iter()
                .map(|posting_key| MemoryFacetPostingRevision {
                    posting_key: posting_key.clone(),
                    revision: 1,
                })
                .collect(),
        };
        mutations.push(StoreMutation::PutJson {
            namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            key: manifest_key.clone(),
            value: serde_json::to_value(manifest).expect("serialize facet manifest"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            record_key: manifest_key.clone(),
        });
        preconditions.push(StoreJsonPrecondition::Absent {
            namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            key: manifest_key,
        });
    }
    platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: format!("test-seed-{}", entry.id),
                operation: "test.seed_scoped_long_term".to_string(),
                scope: StoreEventScope::system("test.seed_scoped_long_term")
                    .with_memory_space(memory_space_id)
                    .with_subject(subject_id),
                mutations,
            },
            &preconditions,
        )
        .expect("seed scoped owner transaction");
    entry
}

pub fn delete_scoped_long_term(
    platform: &StorePlatform,
    memory_space_id: &str,
    entry: &LongTermMemoryEntry,
) {
    let key =
        scoped_long_term_memory_storage_key(memory_space_id, &entry.id).expect("scoped owner key");
    let value = serde_json::to_value(entry).expect("serialize owner");
    let subject_id = "subject:test";
    let facet = build_long_term_memory_facet_index_doc(
        entry,
        memory_space_id,
        vec![subject_id.to_string()],
        1,
    );
    let facet_key = scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, &entry.id)
        .expect("scoped facet owner key");
    let facet_value = serde_json::to_value(&facet).expect("serialize facet owner");
    let owner_version = MemoryFacetOwnerVersion {
        owner_record_id: entry.id.clone(),
        owner_revision: entry.owner_revision,
        facet_index_revision: facet.facet_index_revision,
    };
    let posting_keys = facet
        .posting_keys_for_subject(subject_id)
        .expect("facet posting keys");
    let mut mutations = vec![
        StoreMutation::DeleteJson {
            namespace: "long_term".to_string(),
            key: key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: "long_term".to_string(),
            record_key: entry.id.clone(),
        },
        StoreMutation::DeleteJson {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: facet_key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            record_key: format!("facet-owner:{}", facet.owner_record_id),
        },
    ];
    let mut preconditions = vec![
        StoreJsonPrecondition::Exact {
            namespace: "long_term".to_string(),
            key: key.clone(),
            value,
        },
        StoreJsonPrecondition::Exact {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: facet_key.clone(),
            value: facet_value,
        },
    ];
    let manifest_key =
        memory_facet_manifest_key(memory_space_id, subject_id).expect("facet manifest key");
    if !posting_keys.is_empty() {
        for posting_key in &posting_keys {
            let posting = MemoryFacetPostingDoc {
                schema_version: MEMORY_FACET_SCHEMA_VERSION,
                memory_space_id: memory_space_id.to_string(),
                subject_id: subject_id.to_string(),
                posting_key: posting_key.clone(),
                revision: 1,
                owner_versions: vec![owner_version.clone()],
            };
            mutations.push(StoreMutation::DeleteJson {
                namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                key: posting_key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                record_key: posting_key.clone(),
            });
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                key: posting_key.clone(),
                value: serde_json::to_value(posting).expect("serialize posting"),
            });
        }
        let manifest = MemoryFacetIndexManifest {
            schema_version: MEMORY_FACET_SCHEMA_VERSION,
            memory_space_id: memory_space_id.to_string(),
            subject_id: subject_id.to_string(),
            owner_doc_count: 1,
            posting_doc_count: posting_keys.len(),
            revision: 1,
            owner_versions: vec![owner_version],
            posting_revisions: posting_keys
                .iter()
                .map(|posting_key| MemoryFacetPostingRevision {
                    posting_key: posting_key.clone(),
                    revision: 1,
                })
                .collect(),
        };
        mutations.push(StoreMutation::DeleteJson {
            namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            key: manifest_key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            record_key: manifest_key.clone(),
        });
        preconditions.push(StoreJsonPrecondition::Exact {
            namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            key: manifest_key,
            value: serde_json::to_value(manifest).expect("serialize facet manifest"),
        });
    }
    platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: format!("test-delete-{}", entry.id),
                operation: "test.delete_scoped_long_term".to_string(),
                scope: StoreEventScope::system("test.delete_scoped_long_term")
                    .with_memory_space(memory_space_id)
                    .with_subject(subject_id),
                mutations,
            },
            &preconditions,
        )
        .expect("delete scoped owner transaction");
}
