mod support;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use bm_core::feature_gate::ProfileId;
use bm_sdk::nonproduction_replay_harness::{
    MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig, StoreConsistentReadRequest,
    StoreEngine, StoreEngineMutation, StoreEventLog, StoreEventScope, StoreJsonAddress,
    StoreTransactionRequest,
};
use serde_json::json;

const TRANSACTION_NAMESPACE: &str = "file_transaction_concurrency";
const TRANSACTION_KEY: &str = "transaction";
const PRIMITIVE_NAMESPACE: &str = "file_primitive_concurrency";
const JSON_KEY: &str = "json";
const BLOB_KEY: &str = "blob";

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "beetle-memory-file-primitive-concurrency-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos(),
    ))
}

fn config(root: &Path) -> StoreBackendConfig {
    StoreBackendConfig::file(
        root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("file config")
}

fn event(id: &str, namespace: &str, key: &str) -> MemoryStoreEvent {
    MemoryStoreEvent::new(
        id,
        MemoryStoreEventKind::MemoryWrite,
        StoreEventScope::system("file_primitive_concurrency_contract"),
        1,
    )
    .with_plane(namespace)
    .with_record_key(key)
    .with_content_hash(id)
}

fn transaction_request() -> StoreTransactionRequest {
    StoreTransactionRequest::new(
        "whole-state-transaction",
        Vec::new(),
        vec![
            StoreEngineMutation::PutJson {
                namespace: TRANSACTION_NAMESPACE.to_string(),
                key: TRANSACTION_KEY.to_string(),
                value: json!({"committed": true}),
            },
            StoreEngineMutation::AppendEvent {
                event: Box::new(event(
                    "transaction-event",
                    TRANSACTION_NAMESPACE,
                    TRANSACTION_KEY,
                )),
            },
        ],
        None,
    )
}

fn spawn_paused_transaction(root: &Path, ready: &Path, release: &Path) -> Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("file_paused_transaction_worker")
        .arg("--nocapture")
        .env("BM_FILE_PRIMITIVE_CONCURRENCY_WORKER", "1")
        .env("BM_FILE_TRANSACTION_ROOT", root)
        .env(
            "BM_FILE_TRANSACTION_PAUSE_POINT",
            "after_prepare_before_apply",
        )
        .env("BM_FILE_TRANSACTION_PAUSE_READY", ready)
        .env("BM_FILE_TRANSACTION_PAUSE_RELEASE", release)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn paused transaction")
}

fn wait_for_pause(child: &mut Child, ready: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if ready.exists() {
            return;
        }
        if let Some(status) = child.try_wait().expect("poll transaction worker") {
            panic!("transaction worker exited before pause: {status}");
        }
        assert!(Instant::now() < deadline, "transaction did not reach pause");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn primitive_json_blob_and_event_share_transaction_lock_and_survive_whole_state_apply() {
    let root = temp_root("whole-state");
    let (primitive, _, _) = support::open_file_engine(&config(&root)).expect("primitive engine");
    let ready = root.join("transaction.pause.ready");
    let release = root.join("transaction.pause.release");
    let mut transaction = spawn_paused_transaction(&root, &ready, &release);
    wait_for_pause(&mut transaction, &ready);

    let (sender, receiver) = mpsc::channel();
    let primitive_writer = thread::spawn(move || {
        let result = (|| {
            primitive.put_json_value(PRIMITIVE_NAMESPACE, JSON_KEY, json!({"primitive": true}))?;
            primitive.put_blob(PRIMITIVE_NAMESPACE, BLOB_KEY, b"primitive-blob")?;
            primitive.append_event(event("primitive-event", PRIMITIVE_NAMESPACE, JSON_KEY))?;
            Ok::<(), bm_core::Error>(())
        })();
        sender.send(result).expect("send primitive result");
    });

    match receiver.recv_timeout(Duration::from_millis(200)) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            fs::write(&release, b"release").expect("release failed transaction");
            let _ = transaction.wait();
            panic!("primitive writer disconnected while transaction was paused");
        }
        Ok(result) => {
            fs::write(&release, b"release").expect("release bypassed transaction");
            let _ = transaction.wait();
            result.expect("bypassing primitive result");
            panic!(
                "primitive writes completed while the transaction held the canonical/advisory lock"
            );
        }
    }
    fs::write(&release, b"release").expect("release transaction");
    assert!(
        transaction.wait().expect("transaction worker").success(),
        "transaction worker must commit"
    );
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("primitive writer must resume")
        .expect("primitive writes");
    primitive_writer.join().expect("primitive writer");

    let (reader, _, _) = support::open_file_engine(&config(&root)).expect("reader");
    let read = reader
        .read_consistent(&StoreConsistentReadRequest {
            json: vec![
                StoreJsonAddress::new(TRANSACTION_NAMESPACE, TRANSACTION_KEY),
                StoreJsonAddress::new(PRIMITIVE_NAMESPACE, JSON_KEY),
            ],
            blobs: vec![bm_sdk::nonproduction_replay_harness::StoreBlobAddress::new(
                PRIMITIVE_NAMESPACE,
                BLOB_KEY,
            )],
            include_events: true,
        })
        .expect("consistent final read");
    assert_eq!(read.json[0].value, Some(json!({"committed": true})));
    assert_eq!(read.json[1].value, Some(json!({"primitive": true})));
    assert_eq!(
        read.blobs[0].value.as_deref(),
        Some(b"primitive-blob".as_slice())
    );
    assert!(read
        .events
        .iter()
        .any(|entry| entry.event_id == "transaction-event"));
    assert!(read
        .events
        .iter()
        .any(|entry| entry.event_id == "primitive-event"));
}

#[test]
fn file_paused_transaction_worker() {
    let Some(root) = std::env::var_os("BM_FILE_TRANSACTION_ROOT") else {
        return;
    };
    if std::env::var_os("BM_FILE_PRIMITIVE_CONCURRENCY_WORKER").is_none() {
        return;
    }
    support::open_file_engine(&config(Path::new(&root)))
        .expect("worker engine")
        .0
        .commit_transaction(&transaction_request())
        .expect("whole-state transaction");
}
