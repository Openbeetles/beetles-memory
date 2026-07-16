mod support;

use bm_sdk::nonproduction_replay_harness::{
    EmbeddedStoreEngine, FileStoreEngine, InMemoryStoreEngine, MemoryStoreEventKind,
    StoreCapacityBudget, StoreEngine, StoreEngineMutation, StoreEventScope,
    StoreTransactionRequest,
};
use serde_json::{json, Value};
use std::hash::{Hash, Hasher};

const NAMESPACE: &str = "session";
const JSON_KEY: &str = "conditional-delete-json";
const BLOB_KEY: &str = "conditional-delete-blob";

fn root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "bm-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn json_content_hash(value: &Value) -> String {
    content_hash(&serde_json::to_vec(value).expect("serialize canonical JSON before-image"))
}

fn seed_request(
    transaction_id: &str,
    json_value: Value,
    blob_value: &[u8],
) -> StoreTransactionRequest {
    StoreTransactionRequest::new(
        transaction_id,
        Vec::new(),
        vec![
            StoreEngineMutation::PutJson {
                namespace: NAMESPACE.to_string(),
                key: JSON_KEY.to_string(),
                value: json_value,
            },
            StoreEngineMutation::PutBlob {
                namespace: NAMESPACE.to_string(),
                key: BLOB_KEY.to_string(),
                value: blob_value.to_vec(),
            },
        ],
        None,
    )
}

fn conditional_delete_request(transaction_id: &str) -> StoreTransactionRequest {
    let json_event_template = StoreEngineMutation::conditional_delete_event_template(
        format!("event-json-{transaction_id}"),
        MemoryStoreEventKind::MemoryDelete,
        StoreEventScope::system("conditional-delete"),
        1_900_000_000,
    )
    .with_plane(NAMESPACE)
    .with_record_key(JSON_KEY);
    let blob_event_template = StoreEngineMutation::conditional_delete_event_template(
        format!("event-blob-{transaction_id}"),
        MemoryStoreEventKind::MemoryDelete,
        StoreEventScope::system("conditional-delete"),
        1_900_000_000,
    )
    .with_plane(NAMESPACE)
    .with_record_key(BLOB_KEY);
    StoreTransactionRequest::new(
        transaction_id,
        Vec::new(),
        vec![
            StoreEngineMutation::delete_json_if_present(NAMESPACE, JSON_KEY, json_event_template),
            StoreEngineMutation::delete_blob_if_present(NAMESPACE, BLOB_KEY, blob_event_template),
        ],
        None,
    )
}

fn assert_conditional_delete_hash_contract(
    seed: &dyn StoreEngine,
    deleter: &dyn StoreEngine,
    label: &str,
) {
    let first_json = json!({"z": 2, "nested": {"value": "first"}, "a": 1});
    let second_json = json!({"z": 2, "nested": {"value": "second"}, "a": 1});
    let first_blob = b"first-before-image";
    let second_blob = b"second-before-image";

    let first_transaction = format!("{label}-first");
    let first_delete = conditional_delete_request(&first_transaction);
    seed.commit_transaction(&seed_request(
        &format!("{label}-seed-first"),
        first_json.clone(),
        first_blob,
    ))
    .expect("seed first conditional delete before-image");
    let report = deleter
        .commit_transaction(&first_delete)
        .expect("delete first before-image");
    assert_eq!(report.changed_json, 1);
    assert_eq!(report.changed_blobs, 1);
    assert_eq!(report.appended_events, 2);

    let second_transaction = format!("{label}-second");
    let second_delete = conditional_delete_request(&second_transaction);
    seed.commit_transaction(&seed_request(
        &format!("{label}-seed-second"),
        second_json.clone(),
        second_blob,
    ))
    .expect("seed second conditional delete before-image at the same addresses");
    let report = deleter
        .commit_transaction(&second_delete)
        .expect("delete second before-image");
    assert_eq!(report.changed_json, 1);
    assert_eq!(report.changed_blobs, 1);
    assert_eq!(report.appended_events, 2);

    let events = deleter
        .read_events()
        .expect("read conditional delete events");
    let event_hash = |event_id: &str| {
        events
            .iter()
            .find(|event| event.event_id == event_id)
            .unwrap_or_else(|| panic!("missing event {event_id}"))
            .content_hash
            .clone()
    };
    let first_json_hash = event_hash(&format!("event-json-{first_transaction}"));
    let second_json_hash = event_hash(&format!("event-json-{second_transaction}"));
    let first_blob_hash = event_hash(&format!("event-blob-{first_transaction}"));
    let second_blob_hash = event_hash(&format!("event-blob-{second_transaction}"));

    assert_eq!(first_json_hash, json_content_hash(&first_json));
    assert_eq!(second_json_hash, json_content_hash(&second_json));
    assert_ne!(first_json_hash, second_json_hash);
    assert_eq!(first_blob_hash, content_hash(first_blob));
    assert_eq!(second_blob_hash, content_hash(second_blob));
    assert_ne!(first_blob_hash, second_blob_hash);
    assert_eq!(deleter.get_json_value(NAMESPACE, JSON_KEY).unwrap(), None);
    assert_eq!(deleter.get_blob(NAMESPACE, BLOB_KEY).unwrap(), None);

    let events_before_absent_delete = events.len();
    let absent = deleter
        .commit_transaction(&conditional_delete_request(&format!("{label}-absent")))
        .expect("delete absent value");
    assert_eq!(absent.changed_json, 0);
    assert_eq!(absent.changed_blobs, 0);
    assert_eq!(absent.appended_events, 0);
    assert_eq!(
        deleter.read_events().unwrap().len(),
        events_before_absent_delete,
        "missing before-images must not emit delete events"
    );
}

#[test]
fn shared_transaction_resolves_conditional_delete_for_in_memory_and_embedded() {
    let capacity = StoreCapacityBudget::full();
    let in_memory = InMemoryStoreEngine::new(capacity);
    assert_conditional_delete_hash_contract(&in_memory, &in_memory, "in-memory");
    let embedded = EmbeddedStoreEngine::new(capacity);
    assert_conditional_delete_hash_contract(&embedded, &embedded, "embedded");
}

#[test]
fn in_memory_and_embedded_known_key_reads_are_exact_bounded_and_absence_aware() {
    for engine in [
        Box::new(InMemoryStoreEngine::new(StoreCapacityBudget::full())) as Box<dyn StoreEngine>,
        Box::new(EmbeddedStoreEngine::new(StoreCapacityBudget::full())) as Box<dyn StoreEngine>,
    ] {
        engine
            .put_json_value(NAMESPACE, "target", json!({"value": "small"}))
            .expect("seed target");
        engine
            .put_json_value(NAMESPACE, "unrelated", json!({"value": "x".repeat(4096)}))
            .expect("seed unrelated large value");

        let mut operation_capacity = StoreCapacityBudget::full();
        operation_capacity.snapshot_max_bytes = 64;
        let result = engine
            .read_consistent_known_keys(
                &[
                    (NAMESPACE.to_string(), "target".to_string()),
                    (NAMESPACE.to_string(), "missing".to_string()),
                ],
                &[],
                false,
                operation_capacity,
            )
            .expect("read only requested keys under the operation budget");
        assert_eq!(result.json.len(), 2);
        assert_eq!(result.json[0].key, "target");
        assert!(result.json[0].value.is_some());
        assert_eq!(result.json[1].key, "missing");
        assert!(result.json[1].value.is_none());
        assert_eq!(result.receipt.json_doc_count, 1);

        let error = engine
            .read_consistent_known_keys(
                &[(NAMESPACE.to_string(), "unrelated".to_string())],
                &[],
                false,
                operation_capacity,
            )
            .expect_err("oversized requested value must fail before cloning it into the result");
        assert_eq!(error.stage(), "store_consistent_read_budget_exceeded");
    }
}

#[test]
fn file_backend_resolves_conditional_delete_event_from_locked_before_image() {
    let root = root("file-conditional-delete");
    let config = bm_sdk::nonproduction_replay_harness::StoreBackendConfig::file(
        &root,
        support::native_persistent_profile(),
    )
    .expect("file config");
    let (seed, _, _) = FileStoreEngine::open_with_capacity(&config, StoreCapacityBudget::full())
        .expect("open seed instance");
    let (deleter, _, _) = FileStoreEngine::open_with_capacity(&config, StoreCapacityBudget::full())
        .expect("open delete instance");
    assert_conditional_delete_hash_contract(&seed, &deleter, "file");
    std::fs::remove_dir_all(root).expect("remove file root");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_backend_resolves_conditional_delete_event_inside_begin_immediate() {
    use bm_sdk::nonproduction_replay_harness::SqliteStoreEngine;

    let root = root("sqlite-conditional-delete");
    std::fs::create_dir_all(&root).expect("create sqlite root");
    let config = bm_sdk::nonproduction_replay_harness::StoreBackendConfig::sqlite(
        root.join("memory.sqlite3"),
        support::native_persistent_profile(),
    )
    .expect("sqlite config");
    let (seed, _) = SqliteStoreEngine::open_with_capacity(&config, StoreCapacityBudget::full())
        .expect("open seed connection");
    let (deleter, _) = SqliteStoreEngine::open_with_capacity(&config, StoreCapacityBudget::full())
        .expect("open delete connection");
    assert_conditional_delete_hash_contract(&seed, &deleter, "sqlite");
    drop((seed, deleter));
    std::fs::remove_dir_all(root).expect("remove sqlite root");
}
