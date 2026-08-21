mod support;

use bm_sdk::nonproduction_replay_harness::{
    EmbeddedStoreEngine, FileStoreEngine, InMemoryStoreEngine, StoreBackendConfig,
    StoreCapacityBudget, StoreEngine, StoreEngineMutation, StoreTransactionRequest,
};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

const NAMESPACE: &str = "session";
#[cfg(feature = "sqlite-store")]
const KEY: &str = "p8-immutable-open-pin";
static TEMP_PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn unique_temp_suffix() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos(),
        TEMP_PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(feature = "sqlite-store")]
fn temp_sqlite_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "beetle-memory-p8-immutable-open-pin-{}.sqlite3",
        unique_temp_suffix()
    ))
}

fn temp_file_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "beetle-memory-p8-immutable-capacity-{}",
        unique_temp_suffix()
    ))
}

fn assert_open_pins_generation_before_writer<E>(engine: &E, key: &str)
where
    E: StoreEngine + Sync,
{
    engine
        .put_json_value(NAMESPACE, key, json!({"generation": 1}))
        .expect("seed generation one");
    let mut session = engine
        .open_immutable_read_session(StoreCapacityBudget::full())
        .expect("open immutable session");

    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let writer = scope.spawn(move || {
            ready_tx.send(()).expect("announce writer");
            go_rx.recv().expect("release writer");
            let result = engine.put_json_value(NAMESPACE, key, json!({"generation": 2}));
            done_tx.send(result).expect("report writer result");
        });
        ready_rx.recv().expect("writer ready");
        go_tx.send(()).expect("release writer");
        let early_writer_result = match done_rx.recv_timeout(std::time::Duration::from_millis(250))
        {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(error) => panic!("writer channel failed: {error}"),
        };

        let reads = session
            .read_json_known_keys(&[(NAMESPACE.to_string(), key.to_string())])
            .expect("first known-key read");
        assert_eq!(
            reads[0]
                .value
                .as_ref()
                .and_then(|value| value["generation"].as_u64()),
            Some(1),
            "session open must pin visibility before the first known-key read"
        );
        drop(session);
        let writer_result = early_writer_result.unwrap_or_else(|| {
            done_rx
                .recv_timeout(std::time::Duration::from_secs(6))
                .expect("writer completes after session drop")
        });
        writer_result.expect("commit generation two after session drop");
        writer.join().expect("writer thread");
    });

    let mut next_session = engine
        .open_immutable_read_session(StoreCapacityBudget::full())
        .expect("open next immutable session");
    let next_reads = next_session
        .read_json_known_keys(&[(NAMESPACE.to_string(), key.to_string())])
        .expect("next session known-key read");
    assert_eq!(
        next_reads[0]
            .value
            .as_ref()
            .and_then(|value| value["generation"].as_u64()),
        Some(2),
        "a new session must observe the committed successor generation"
    );
}

fn put_root_and_owner<E: StoreEngine>(
    engine: &E,
    transaction_id: &str,
    root_key: &str,
    owner_key: &str,
    generation: u64,
) {
    engine
        .commit_transaction(&StoreTransactionRequest::new(
            transaction_id,
            Vec::new(),
            vec![
                StoreEngineMutation::PutJson {
                    namespace: NAMESPACE.into(),
                    key: root_key.into(),
                    value: json!({"generation": generation, "owner_key": owner_key}),
                },
                StoreEngineMutation::PutJson {
                    namespace: NAMESPACE.into(),
                    key: owner_key.into(),
                    value: json!({"generation": generation}),
                },
            ],
            None,
        ))
        .expect("commit root and owner generation");
}

fn assert_root_and_owner_never_mix_generations<E>(reader: &E, writer: &E, suffix: &str)
where
    E: StoreEngine + Sync,
{
    let root_key = format!("p8-root-{suffix}");
    let owner_key = format!("p8-owner-{suffix}");
    put_root_and_owner(reader, &format!("seed-{suffix}"), &root_key, &owner_key, 1);
    let mut session = reader
        .open_immutable_read_session(reader.store_capacity())
        .expect("open immutable session");
    let root = session
        .read_json_known_keys(&[(NAMESPACE.to_string(), root_key.clone())])
        .expect("read root");
    assert_eq!(
        root[0]
            .value
            .as_ref()
            .and_then(|value| value["generation"].as_u64()),
        Some(1)
    );

    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let writer_root_key = root_key.clone();
        let writer_owner_key = owner_key.clone();
        let writer = scope.spawn(move || {
            ready_tx.send(()).expect("announce writer");
            go_rx.recv().expect("release writer");
            let result = writer.commit_transaction(&StoreTransactionRequest::new(
                format!("replace-{suffix}"),
                Vec::new(),
                vec![
                    StoreEngineMutation::PutJson {
                        namespace: NAMESPACE.into(),
                        key: writer_root_key,
                        value: json!({"generation": 2, "owner_key": writer_owner_key.clone()}),
                    },
                    StoreEngineMutation::PutJson {
                        namespace: NAMESPACE.into(),
                        key: writer_owner_key,
                        value: json!({"generation": 2}),
                    },
                ],
                None,
            ));
            done_tx.send(result).expect("report writer result");
        });
        ready_rx.recv().expect("writer ready");
        go_tx.send(()).expect("release writer");
        let early_writer_result = match done_rx.recv_timeout(std::time::Duration::from_millis(250))
        {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(error) => panic!("writer channel failed: {error}"),
        };

        let owner = session
            .read_json_known_keys(&[(NAMESPACE.to_string(), owner_key.clone())])
            .expect("read owner after writer starts");
        assert_eq!(
            owner[0]
                .value
                .as_ref()
                .and_then(|value| value["generation"].as_u64()),
            Some(1),
            "one immutable session cannot mix old root with new owner"
        );
        drop(session);
        let writer_result = early_writer_result.unwrap_or_else(|| {
            done_rx
                .recv_timeout(std::time::Duration::from_secs(6))
                .expect("writer completes after session drop")
        });
        writer_result.expect("replace root and owner after session drop");
        writer.join().expect("writer thread");
    });

    let mut next_session = reader
        .open_immutable_read_session(reader.store_capacity())
        .expect("open successor session");
    let next = next_session
        .read_json_known_keys(&[
            (NAMESPACE.to_string(), root_key),
            (NAMESPACE.to_string(), owner_key),
        ])
        .expect("read successor root and owner");
    assert!(next.iter().all(|read| {
        read.value
            .as_ref()
            .and_then(|value| value["generation"].as_u64())
            == Some(2)
    }));
}

fn assert_known_key_budget_and_receipt<E>(
    engine: &E,
    suffix: &str,
) -> (String, usize, usize, usize, usize, usize)
where
    E: StoreEngine,
{
    let json_key = format!("p8-json-{suffix}");
    let missing_json_key = format!("p8-json-missing-{suffix}");
    let blob_key = format!("p8-blob-{suffix}");
    let missing_blob_key = format!("p8-blob-missing-{suffix}");
    let json_value = json!({"value": "exact"});
    let blob_value = b"exact-blob".to_vec();
    engine
        .commit_transaction(&StoreTransactionRequest::new(
            format!("seed-known-key-{suffix}"),
            Vec::new(),
            vec![
                StoreEngineMutation::PutJson {
                    namespace: NAMESPACE.into(),
                    key: json_key.clone(),
                    value: json_value.clone(),
                },
                StoreEngineMutation::PutBlob {
                    namespace: NAMESPACE.into(),
                    key: blob_key.clone(),
                    value: blob_value.clone(),
                },
            ],
            None,
        ))
        .expect("seed known-key values");

    let mut exact_capacity = engine.store_capacity();
    exact_capacity.kv_max_entries = 4;
    exact_capacity.snapshot_max_bytes = serde_json::to_vec(&json_value).expect("JSON bytes").len();
    exact_capacity.blob_max_bytes = blob_value.len();
    let mut session = engine
        .open_immutable_read_session(exact_capacity)
        .expect("open exact known-key session");
    let json_reads = session
        .read_json_known_keys(&[
            (NAMESPACE.to_string(), json_key.clone()),
            (NAMESPACE.to_string(), missing_json_key.clone()),
        ])
        .expect("read existing and missing JSON");
    assert_eq!(json_reads[0].key, json_key);
    assert!(json_reads[0].value.is_some());
    assert_eq!(json_reads[1].key, missing_json_key);
    assert!(json_reads[1].value.is_none());
    let blob_reads = session
        .read_blob_known_keys(&[
            (NAMESPACE.to_string(), blob_key.clone()),
            (NAMESPACE.to_string(), missing_blob_key),
        ])
        .expect("read existing and missing blob");
    assert_eq!(blob_reads[0].key, blob_key);
    assert!(blob_reads[0].value.is_some());
    assert!(blob_reads[1].value.is_none());
    let receipt = session.receipt().expect("exact read receipt");
    let outcome = (
        receipt.state_digest,
        receipt.json_doc_count,
        receipt.blob_count,
        receipt.entry_count,
        receipt.json_bytes,
        receipt.blob_bytes,
    );
    drop(session);

    let mut duplicate_session = engine
        .open_immutable_read_session(engine.store_capacity())
        .expect("open duplicate session");
    duplicate_session
        .read_json_known_keys(&[(NAMESPACE.to_string(), json_key.clone())])
        .expect("first address");
    assert!(duplicate_session
        .read_json_known_keys(&[(NAMESPACE.to_string(), json_key.clone())])
        .is_err());
    assert!(duplicate_session.receipt().is_err());
    drop(duplicate_session);

    let mut entry_capacity = engine.store_capacity();
    entry_capacity.kv_max_entries = 1;
    let mut entry_session = engine
        .open_immutable_read_session(entry_capacity)
        .expect("open entry-bound session");
    entry_session
        .read_json_known_keys(&[(NAMESPACE.to_string(), json_key.clone())])
        .expect("exact one entry");
    assert!(entry_session
        .read_json_known_keys(&[(NAMESPACE.to_string(), missing_json_key)])
        .is_err());
    assert!(entry_session.receipt().is_err());
    drop(entry_session);

    let mut byte_capacity = engine.store_capacity();
    byte_capacity.snapshot_max_bytes = serde_json::to_vec(&json_value)
        .expect("JSON bytes")
        .len()
        .saturating_sub(1);
    let mut byte_session = engine
        .open_immutable_read_session(byte_capacity)
        .expect("open byte-bound session");
    assert!(byte_session
        .read_json_known_keys(&[(NAMESPACE.to_string(), json_key)])
        .is_err());
    assert!(byte_session.receipt().is_err());

    outcome
}

#[test]
fn immutable_session_open_pins_generation_before_first_known_key_read_in_memory() {
    assert_open_pins_generation_before_writer(
        &InMemoryStoreEngine::new(StoreCapacityBudget::full()),
        "p8-immutable-open-pin-in-memory",
    );
}

#[test]
fn immutable_session_open_pins_generation_before_first_known_key_read_embedded() {
    assert_open_pins_generation_before_writer(
        &EmbeddedStoreEngine::new(StoreCapacityBudget::full()),
        "p8-immutable-open-pin-embedded",
    );
}

#[test]
fn immutable_session_open_pins_generation_before_first_known_key_read_file() {
    let file_root = temp_file_root();
    let config = StoreBackendConfig::file(&file_root, support::native_persistent_profile())
        .expect("file config");
    let (engine, _, _) = FileStoreEngine::open_with_capacity(&config, StoreCapacityBudget::full())
        .expect("file engine");
    assert_open_pins_generation_before_writer(&engine, "p8-immutable-open-pin-file");
}

#[test]
fn immutable_session_root_and_owner_reads_never_mix_generations_on_guard_backends() {
    let in_memory = InMemoryStoreEngine::new(StoreCapacityBudget::full());
    assert_root_and_owner_never_mix_generations(&in_memory, &in_memory, "in-memory");

    let embedded = EmbeddedStoreEngine::new(StoreCapacityBudget::full());
    assert_root_and_owner_never_mix_generations(&embedded, &embedded, "embedded");

    let file_root = temp_file_root();
    let config = StoreBackendConfig::file(&file_root, support::native_persistent_profile())
        .expect("file config");
    let (file, _, _) = FileStoreEngine::open_with_capacity(&config, StoreCapacityBudget::full())
        .expect("file engine");
    assert_root_and_owner_never_mix_generations(&file, &file, "file");
}

#[test]
fn immutable_session_known_key_absence_order_budget_and_receipt_match_guard_backends() {
    let in_memory = InMemoryStoreEngine::new(StoreCapacityBudget::full());
    let expected = assert_known_key_budget_and_receipt(&in_memory, "shared");

    let embedded = EmbeddedStoreEngine::new(StoreCapacityBudget::full());
    assert_eq!(
        assert_known_key_budget_and_receipt(&embedded, "shared"),
        expected
    );

    let file_root = temp_file_root();
    let config = StoreBackendConfig::file(&file_root, support::native_persistent_profile())
        .expect("file config");
    let (file, _, _) = FileStoreEngine::open_with_capacity(&config, StoreCapacityBudget::full())
        .expect("file engine");
    assert_eq!(
        assert_known_key_budget_and_receipt(&file, "shared"),
        expected
    );
}

#[cfg(feature = "sqlite-store")]
#[test]
fn immutable_session_root_and_owner_reads_never_mix_generations_sqlite() {
    let path = temp_sqlite_path();
    let config =
        StoreBackendConfig::sqlite(&path, support::native_persistent_profile()).expect("config");
    let (reader, _) = support::open_sqlite_engine(&config).expect("reader");
    let (writer, _) = support::open_sqlite_engine(&config).expect("writer");
    assert_root_and_owner_never_mix_generations(&reader, &writer, "sqlite");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn immutable_session_known_key_absence_order_budget_and_receipt_match_sqlite() {
    let baseline = InMemoryStoreEngine::new(StoreCapacityBudget::full());
    let expected = assert_known_key_budget_and_receipt(&baseline, "shared");
    let path = temp_sqlite_path();
    let config =
        StoreBackendConfig::sqlite(&path, support::native_persistent_profile()).expect("config");
    let (sqlite, _) = support::open_sqlite_engine(&config).expect("sqlite");
    assert_eq!(
        assert_known_key_budget_and_receipt(&sqlite, "shared"),
        expected
    );
}

#[test]
fn immutable_session_request_capacity_cannot_exceed_any_engine_capacity() {
    let mut engine_capacity = StoreCapacityBudget::full();
    engine_capacity.kv_max_entries = 1;
    let mut oversized_request = engine_capacity;
    oversized_request.kv_max_entries = 2;

    let in_memory = InMemoryStoreEngine::new(engine_capacity);
    assert!(in_memory
        .open_immutable_read_session(oversized_request)
        .is_err());

    let embedded = EmbeddedStoreEngine::new(engine_capacity);
    assert!(embedded
        .open_immutable_read_session(oversized_request)
        .is_err());

    let file_root = temp_file_root();
    let file_config = StoreBackendConfig::file(&file_root, support::native_persistent_profile())
        .expect("file config");
    let (file, _, _) =
        FileStoreEngine::open_with_capacity(&file_config, engine_capacity).expect("file engine");
    assert!(file.open_immutable_read_session(oversized_request).is_err());

    #[cfg(feature = "sqlite-store")]
    {
        let sqlite_path = temp_sqlite_path();
        let sqlite_config =
            StoreBackendConfig::sqlite(&sqlite_path, support::native_persistent_profile())
                .expect("sqlite config");
        let (sqlite, _) =
            bm_sdk::nonproduction_replay_harness::SqliteStoreEngine::open_with_capacity(
                &sqlite_config,
                engine_capacity,
            )
            .expect("sqlite engine");
        assert!(sqlite
            .open_immutable_read_session(oversized_request)
            .is_err());
    }
}

#[cfg(feature = "sqlite-store")]
#[test]
fn immutable_session_open_pins_generation_before_first_known_key_read_sqlite() {
    let path = temp_sqlite_path();
    let config =
        StoreBackendConfig::sqlite(&path, support::native_persistent_profile()).expect("config");
    let (reader, _) = support::open_sqlite_engine(&config).expect("reader");
    let (writer, _) = support::open_sqlite_engine(&config).expect("writer");
    reader
        .put_json_value(NAMESPACE, KEY, json!({"generation": 1}))
        .expect("seed generation one");
    let session_capacity = reader.store_capacity();

    let mut session = reader
        .open_immutable_read_session(session_capacity)
        .expect("open immutable session");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (go_tx, go_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let writer_thread = std::thread::spawn(move || {
        ready_tx.send(()).expect("announce writer");
        go_rx.recv().expect("release writer");
        let result = writer.put_json_value(NAMESPACE, KEY, json!({"generation": 2}));
        done_tx.send(result).expect("report writer result");
    });
    ready_rx.recv().expect("writer ready");
    go_tx.send(()).expect("release writer");
    let early_writer_result = match done_rx.recv_timeout(std::time::Duration::from_millis(250)) {
        Ok(result) => Some(result),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
        Err(error) => panic!("writer channel failed: {error}"),
    };

    let reads = session
        .read_json_known_keys(&[(NAMESPACE.to_string(), KEY.to_string())])
        .expect("first known-key read");
    assert_eq!(reads.len(), 1);
    assert_eq!(
        reads[0]
            .value
            .as_ref()
            .and_then(|value| value["generation"].as_u64()),
        Some(1),
        "session open must pin visibility before the first known-key read"
    );
    drop(session);
    let writer_result = early_writer_result.unwrap_or_else(|| {
        done_rx
            .recv_timeout(std::time::Duration::from_secs(6))
            .expect("writer completes after session drop")
    });
    writer_result.expect("commit generation two after session drop");
    writer_thread.join().expect("writer thread");

    let mut next_session = reader
        .open_immutable_read_session(session_capacity)
        .expect("open next immutable session");
    let next_reads = next_session
        .read_json_known_keys(&[(NAMESPACE.to_string(), KEY.to_string())])
        .expect("next session known-key read");
    assert_eq!(
        next_reads[0]
            .value
            .as_ref()
            .and_then(|value| value["generation"].as_u64()),
        Some(2),
        "a new session must observe the committed successor generation"
    );
}
