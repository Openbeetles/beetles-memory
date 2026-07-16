mod support;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use bm_core::feature_gate::ProfileId;
use bm_sdk::nonproduction_replay_harness::{
    EmbeddedStoreEngine, MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig,
    StoreCapacityBudget, StoreConsistentReadRequest, StoreEngine, StoreEngineMutation,
    StoreEventLog, StoreEventScope, StoreJsonAddress, StoreJsonPrecondition,
    StoreTransactionReport, StoreTransactionRequest,
};
use serde_json::{json, Value};

const NAMESPACE: &str = "memory_graph_manifests";
const KEY: &str = "concurrency-generation";

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "beetle-memory-store-concurrency-{name}-{}-{}",
        std::process::id(),
        unique_suffix()
    ))
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_nanos()
}

fn event(id: &str) -> MemoryStoreEvent {
    MemoryStoreEvent::new(
        id,
        MemoryStoreEventKind::MemoryWrite,
        StoreEventScope::system("store_concurrency_contract"),
        1,
    )
    .with_plane(NAMESPACE)
    .with_record_key(KEY)
    .with_content_hash(id)
}

fn put_request(
    transaction_id: &str,
    expected: StoreJsonPrecondition,
    value: Value,
) -> StoreTransactionRequest {
    StoreTransactionRequest::new(
        transaction_id,
        vec![expected],
        vec![
            StoreEngineMutation::PutJson {
                namespace: NAMESPACE.to_string(),
                key: KEY.to_string(),
                value,
            },
            StoreEngineMutation::AppendEvent {
                event: Box::new(event(transaction_id)),
            },
        ],
        None,
    )
}

fn read_value(engine: &dyn StoreEngine, key: &str) -> Value {
    engine
        .read_consistent(&StoreConsistentReadRequest {
            json: vec![StoreJsonAddress::new(NAMESPACE, key)],
            blobs: Vec::new(),
            include_events: false,
        })
        .expect("consistent read")
        .json
        .into_iter()
        .next()
        .and_then(|doc| doc.value)
        .expect("stored value")
}

fn assert_one_cas_winner(
    first: bm_core::Result<StoreTransactionReport>,
    second: bm_core::Result<StoreTransactionReport>,
) {
    let outcomes = [first, second];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| result.as_ref().is_err_and(|error| {
                error.stage() == "memory_write_transaction_precondition_failed"
            }))
            .count(),
        1
    );
}

#[test]
fn independent_file_opens_exact_cas_has_one_winner() {
    let root = temp_root("file-independent-open");
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let (seed, _, _) = support::open_file_engine(&config).expect("seed engine");
    let v1 = json!({"generation": 1});
    seed.commit_transaction(&put_request(
        "file-seed",
        StoreJsonPrecondition::Absent {
            namespace: NAMESPACE.to_string(),
            key: KEY.to_string(),
        },
        v1.clone(),
    ))
    .expect("seed");

    let (first, _, _) = support::open_file_engine(&config).expect("first engine");
    let (second, _, _) = support::open_file_engine(&config).expect("second engine");
    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = barrier.clone();
    let first_v1 = v1.clone();
    let first = thread::spawn(move || {
        first_barrier.wait();
        first.commit_transaction(&put_request(
            "file-first",
            StoreJsonPrecondition::Exact {
                namespace: NAMESPACE.to_string(),
                key: KEY.to_string(),
                value: first_v1,
            },
            json!({"generation": 2, "writer": "first"}),
        ))
    });
    let second_barrier = barrier.clone();
    let second = thread::spawn(move || {
        second_barrier.wait();
        second.commit_transaction(&put_request(
            "file-second",
            StoreJsonPrecondition::Exact {
                namespace: NAMESPACE.to_string(),
                key: KEY.to_string(),
                value: v1,
            },
            json!({"generation": 2, "writer": "second"}),
        ))
    });

    assert_one_cas_winner(
        first.join().expect("first writer"),
        second.join().expect("second writer"),
    );
    let (reader, _, _) = support::open_file_engine(&config).expect("reader");
    assert_eq!(reader.read_events().expect("events").len(), 2);
    assert_eq!(read_value(&reader, KEY)["generation"], 2);
}

#[cfg(feature = "sqlite-store")]
#[test]
fn independent_sqlite_opens_exact_cas_has_one_winner() {
    let root = temp_root("sqlite-independent-open");
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(
        &path,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let (seed, _) = support::open_sqlite_engine(&config).expect("seed engine");
    let v1 = json!({"generation": 1});
    seed.commit_transaction(&put_request(
        "sqlite-seed",
        StoreJsonPrecondition::Absent {
            namespace: NAMESPACE.to_string(),
            key: KEY.to_string(),
        },
        v1.clone(),
    ))
    .expect("seed");

    let (first, _) = support::open_sqlite_engine(&config).expect("first engine");
    let (second, _) = support::open_sqlite_engine(&config).expect("second engine");
    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = barrier.clone();
    let first_v1 = v1.clone();
    let first = thread::spawn(move || {
        first_barrier.wait();
        first.commit_transaction(&put_request(
            "sqlite-first",
            StoreJsonPrecondition::Exact {
                namespace: NAMESPACE.to_string(),
                key: KEY.to_string(),
                value: first_v1,
            },
            json!({"generation": 2, "writer": "first"}),
        ))
    });
    let second_barrier = barrier.clone();
    let second = thread::spawn(move || {
        second_barrier.wait();
        second.commit_transaction(&put_request(
            "sqlite-second",
            StoreJsonPrecondition::Exact {
                namespace: NAMESPACE.to_string(),
                key: KEY.to_string(),
                value: v1,
            },
            json!({"generation": 2, "writer": "second"}),
        ))
    });

    assert_one_cas_winner(
        first.join().expect("first writer"),
        second.join().expect("second writer"),
    );
    let (reader, _) = support::open_sqlite_engine(&config).expect("reader");
    assert_eq!(reader.read_events().expect("events").len(), 2);
    assert_eq!(read_value(&reader, KEY)["generation"], 2);
}

#[test]
fn independent_file_open_consistent_read_never_observes_mixed_generation() {
    let root = temp_root("file-consistent-read");
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let (writer, _, _) = support::open_file_engine(&config).expect("writer");
    let (reader, _, _) = support::open_file_engine(&config).expect("reader");
    let keys = ["generation-a", "generation-b"];
    writer
        .commit_transaction(&generation_request(1, None, &keys))
        .expect("seed generation");

    let writer_thread = thread::spawn(move || {
        for generation in 2..=80 {
            writer
                .commit_transaction(&generation_request(generation, Some(generation - 1), &keys))
                .expect("replace generation");
        }
    });

    for _ in 0..160 {
        let read = reader
            .read_consistent(&StoreConsistentReadRequest {
                json: keys
                    .iter()
                    .map(|key| StoreJsonAddress::new(NAMESPACE, *key))
                    .collect(),
                blobs: Vec::new(),
                include_events: false,
            })
            .expect("consistent read");
        let generations = read
            .json
            .iter()
            .map(|doc| doc.value.as_ref().expect("value")["generation"].as_u64())
            .collect::<Vec<_>>();
        assert_eq!(generations.len(), 2);
        assert_eq!(generations[0], generations[1], "mixed generation: {read:?}");
    }
    writer_thread.join().expect("writer thread");
}

#[test]
fn file_incomplete_transaction_marker_fails_closed_for_open_and_consistent_read() {
    let root = temp_root("file-incomplete-transaction");
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let (engine, _, _) = support::open_file_engine(&config).expect("engine");
    fs::write(
        root.join(".beetle-memory.transaction"),
        br#"{"schema_version":1,"transaction_id":"interrupted","state":"prepared"}"#,
    )
    .expect("transaction marker");

    let read_error = engine
        .read_consistent(&StoreConsistentReadRequest::default())
        .expect_err("incomplete transaction must block reads");
    assert_eq!(read_error.stage(), "store_transaction_recovery_required");
    let open_error = support::open_file_engine(&config)
        .err()
        .expect("incomplete transaction must block reopen");
    assert_eq!(open_error.stage(), "store_transaction_recovery_required");
}

#[test]
fn embedded_transaction_rejects_append_only_audit_overflow() {
    let mut capacity = StoreCapacityBudget::full();
    capacity.event_log_max_items = 2;
    let engine = EmbeddedStoreEngine::new(capacity);
    for sequence in 1..=2 {
        engine
            .commit_transaction(&StoreTransactionRequest::new(
                format!("embedded-{sequence}"),
                Vec::new(),
                vec![StoreEngineMutation::AppendEvent {
                    event: Box::new(event(&format!("embedded-{sequence}"))),
                }],
                None,
            ))
            .expect("embedded append-only audit transaction");
    }
    let error = engine
        .commit_transaction(&StoreTransactionRequest::new(
            "embedded-3",
            Vec::new(),
            vec![StoreEngineMutation::AppendEvent {
                event: Box::new(event("embedded-3")),
            }],
            None,
        ))
        .expect_err("third event must not evict append-only audit history");
    assert_eq!(error.stage(), "store_budget_exceeded");

    let event_ids = engine
        .read_consistent(&StoreConsistentReadRequest {
            include_events: true,
            ..StoreConsistentReadRequest::default()
        })
        .expect("embedded consistent read")
        .events
        .into_iter()
        .map(|event| event.event_id)
        .collect::<Vec<_>>();
    assert_eq!(event_ids, vec!["embedded-1", "embedded-2"]);
}

fn generation_request(
    generation: u64,
    expected_generation: Option<u64>,
    keys: &[&str],
) -> StoreTransactionRequest {
    let preconditions = keys
        .iter()
        .map(|key| match expected_generation {
            Some(expected) => StoreJsonPrecondition::Exact {
                namespace: NAMESPACE.to_string(),
                key: (*key).to_string(),
                value: json!({"generation": expected}),
            },
            None => StoreJsonPrecondition::Absent {
                namespace: NAMESPACE.to_string(),
                key: (*key).to_string(),
            },
        })
        .collect();
    let mut mutations = keys
        .iter()
        .map(|key| StoreEngineMutation::PutJson {
            namespace: NAMESPACE.to_string(),
            key: (*key).to_string(),
            value: json!({"generation": generation}),
        })
        .collect::<Vec<_>>();
    mutations.push(StoreEngineMutation::AppendEvent {
        event: Box::new(event(&format!("generation-{generation}"))),
    });
    StoreTransactionRequest::new(
        format!("generation-{generation}"),
        preconditions,
        mutations,
        None,
    )
}

#[test]
fn multiprocess_file_exact_cas_has_one_winner() {
    let root = temp_root("file-multiprocess");
    fs::create_dir_all(&root).expect("root");
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let (seed, _, _) = support::open_file_engine(&config).expect("seed engine");
    seed.commit_transaction(&put_request(
        "process-seed",
        StoreJsonPrecondition::Absent {
            namespace: NAMESPACE.to_string(),
            key: KEY.to_string(),
        },
        json!({"generation": 1}),
    ))
    .expect("seed");

    let ready = root.join("ready");
    fs::create_dir_all(&ready).expect("ready dir");
    let go = root.join("go");
    let first_result = root.join("first.result");
    let second_result = root.join("second.result");
    let mut first = spawn_file_worker(&root, "first", &first_result);
    let mut second = spawn_file_worker(&root, "second", &second_result);
    wait_for_paths(&[ready.join("first"), ready.join("second")]);
    fs::write(&go, b"go").expect("release workers");
    assert!(first.wait().expect("first worker").success());
    assert!(second.wait().expect("second worker").success());

    let outcomes = [
        fs::read_to_string(&first_result).expect("first result"),
        fs::read_to_string(&second_result).expect("second result"),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|value| value.as_str() == "ok")
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|value| value.as_str() == "memory_write_transaction_precondition_failed")
            .count(),
        1
    );
    let (reader, _, _) = support::open_file_engine(&config).expect("reader");
    assert_eq!(reader.read_events().expect("events").len(), 2);
}

fn spawn_file_worker(root: &Path, writer: &str, result: &Path) -> std::process::Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("file_transaction_worker")
        .arg("--nocapture")
        .env("BM_STORE_FILE_TX_WORKER", "1")
        .env("BM_STORE_FILE_TX_ROOT", root)
        .env("BM_STORE_FILE_TX_WRITER", writer)
        .env("BM_STORE_FILE_TX_RESULT", result)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker")
}

fn wait_for_paths(paths: &[PathBuf]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !paths.iter().all(|path| path.exists()) {
        assert!(Instant::now() < deadline, "workers did not become ready");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn file_transaction_worker() {
    if std::env::var_os("BM_STORE_FILE_TX_WORKER").is_none() {
        return;
    }
    let root = PathBuf::from(std::env::var_os("BM_STORE_FILE_TX_ROOT").expect("worker root"));
    let writer = std::env::var("BM_STORE_FILE_TX_WRITER").expect("writer");
    let result =
        PathBuf::from(std::env::var_os("BM_STORE_FILE_TX_RESULT").expect("worker result path"));
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let (engine, _, _) = support::open_file_engine(&config).expect("worker engine");
    fs::write(root.join("ready").join(&writer), b"ready").expect("ready marker");
    wait_for_paths(&[root.join("go")]);
    let outcome = engine.commit_transaction(&put_request(
        &format!("process-{writer}"),
        StoreJsonPrecondition::Exact {
            namespace: NAMESPACE.to_string(),
            key: KEY.to_string(),
            value: json!({"generation": 1}),
        },
        json!({"generation": 2, "writer": writer}),
    ));
    let value = match outcome {
        Ok(_) => "ok".to_string(),
        Err(error) => error.stage().to_string(),
    };
    fs::write(result, value).expect("worker result");
}
