mod support;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform as _;
use bm_sdk::nonproduction_replay_harness::{
    FileStoreEngine, MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig,
    StoreConsistentReadRequest, StoreEngine, StoreEngineMutation, StoreEventLog, StoreEventScope,
    StoreJsonAddress, StoreJsonPrecondition, StoreTransactionRequest,
};
use serde_json::{json, Value};

const NAMESPACE: &str = "session";
const KEY: &str = "generation";

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "beetle-memory-file-transaction-recovery-{name}-{}-{}",
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

fn event(transaction_id: &str) -> MemoryStoreEvent {
    MemoryStoreEvent::new(
        format!("event-{transaction_id}"),
        MemoryStoreEventKind::MemoryWrite,
        StoreEventScope::system("file_transaction_recovery_contract"),
        1,
    )
    .with_plane(NAMESPACE)
    .with_record_key(KEY)
    .with_content_hash(transaction_id)
}

fn request(
    transaction_id: &str,
    expected_generation: Option<u64>,
    generation: u64,
) -> StoreTransactionRequest {
    request_for_key(transaction_id, KEY, expected_generation, generation)
}

fn request_for_key(
    transaction_id: &str,
    key: &str,
    expected_generation: Option<u64>,
    generation: u64,
) -> StoreTransactionRequest {
    let preconditions = match expected_generation {
        Some(expected) => vec![StoreJsonPrecondition::Exact {
            namespace: NAMESPACE.to_string(),
            key: key.to_string(),
            value: json!({"generation": expected}),
        }],
        None => vec![StoreJsonPrecondition::Absent {
            namespace: NAMESPACE.to_string(),
            key: key.to_string(),
        }],
    };
    StoreTransactionRequest::new(
        transaction_id,
        preconditions,
        vec![
            StoreEngineMutation::PutJson {
                namespace: NAMESPACE.to_string(),
                key: key.to_string(),
                value: json!({"generation": generation}),
            },
            StoreEngineMutation::AppendEvent {
                event: Box::new(event(transaction_id)),
            },
        ],
        None,
    )
}

fn delete_request(transaction_id: &str) -> StoreTransactionRequest {
    StoreTransactionRequest::new(
        transaction_id,
        vec![StoreJsonPrecondition::Exact {
            namespace: NAMESPACE.to_string(),
            key: KEY.to_string(),
            value: json!({"generation": 1}),
        }],
        vec![
            StoreEngineMutation::DeleteJson {
                namespace: NAMESPACE.to_string(),
                key: KEY.to_string(),
            },
            StoreEngineMutation::AppendEvent {
                event: Box::new(event(transaction_id)),
            },
        ],
        None,
    )
}

fn open(root: &Path) -> FileStoreEngine {
    support::open_file_engine(&config(root))
        .expect("open file engine")
        .0
}

fn read_generation(engine: &FileStoreEngine) -> Value {
    engine
        .read_consistent(&StoreConsistentReadRequest {
            json: vec![StoreJsonAddress::new(NAMESPACE, KEY)],
            blobs: Vec::new(),
            include_events: true,
        })
        .expect("consistent read")
        .json
        .into_iter()
        .next()
        .and_then(|entry| entry.value)
        .expect("generation value")
}

fn seed(root: &Path) {
    open(root)
        .commit_transaction(&request("seed", None, 1))
        .expect("seed transaction");
}

fn crash_worker(root: &Path, crash_point: &str) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("file_transaction_recovery_crash_worker")
        .arg("--nocapture")
        .env("BM_FILE_TRANSACTION_RECOVERY_WORKER", "1")
        .env("BM_FILE_TRANSACTION_ROOT", root)
        .env("BM_FILE_TRANSACTION_CRASH_POINT", crash_point)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn crash worker")
}

fn primitive_crash_worker(root: &Path, crash_point: &str) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("file_primitive_recovery_crash_worker")
        .arg("--nocapture")
        .env("BM_FILE_PRIMITIVE_RECOVERY_WORKER", "1")
        .env("BM_FILE_TRANSACTION_ROOT", root)
        .env("BM_FILE_PRIMITIVE_CRASH_POINT", crash_point)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn primitive crash worker")
}

fn pause_worker(
    root: &Path,
    pause_point: &str,
    ready: &Path,
    release: &Path,
) -> std::process::Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("file_transaction_pause_worker")
        .arg("--nocapture")
        .env("BM_FILE_TRANSACTION_PAUSE_WORKER", "1")
        .env("BM_FILE_TRANSACTION_ROOT", root)
        .env("BM_FILE_TRANSACTION_PAUSE_POINT", pause_point)
        .env("BM_FILE_TRANSACTION_PAUSE_READY", ready)
        .env("BM_FILE_TRANSACTION_PAUSE_RELEASE", release)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pause worker")
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "path did not become ready: {path:?}"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn durability_worker(root: &Path, trace: &Path) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("file_transaction_durability_worker")
        .arg("--nocapture")
        .env("BM_FILE_TRANSACTION_DURABILITY_WORKER", "1")
        .env("BM_FILE_TRANSACTION_ROOT", root)
        .env("BM_FILE_TRANSACTION_DURABILITY_TRACE", trace)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn durability worker")
}

fn recovery_durability_worker(root: &Path, trace: &Path) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("file_transaction_recovery_durability_worker")
        .arg("--nocapture")
        .env("BM_FILE_TRANSACTION_RECOVERY_DURABILITY_WORKER", "1")
        .env("BM_FILE_TRANSACTION_ROOT", root)
        .env("BM_FILE_TRANSACTION_DURABILITY_TRACE", trace)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn recovery durability worker")
}

#[test]
fn prepared_journal_recovers_before_image_after_process_crash() {
    let root = temp_root("prepared");
    seed(&root);
    let reader = open(&root);

    let status = crash_worker(&root, "after_apply_before_commit");
    assert!(
        !status.success(),
        "worker must terminate at the prepared journal crash point"
    );
    assert_journal_state(&root, "prepared");

    assert_eq!(read_generation(&reader), json!({"generation": 1}));
    assert_eq!(
        reader.read_events().expect("events after rollback").len(),
        1
    );
    drop(reader);

    let reopened = open(&root);
    assert_eq!(read_generation(&reopened), json!({"generation": 1}));
    assert_eq!(
        reopened
            .read_events()
            .expect("events after repeated recovery")
            .len(),
        1
    );
    assert!(
        !root.join(".beetle-memory.transaction").exists(),
        "successful recovery must clean the durable journal"
    );
}

#[test]
fn prepared_journal_recovers_a_mid_put_unpaired_index() {
    let root = temp_root("mid-put");
    seed(&root);
    let status = crash_worker(&root, "after_json_index_before_data");
    assert!(!status.success());
    assert_journal_state(&root, "prepared");

    let reopened = open(&root);
    assert_eq!(read_generation(&reopened), json!({"generation": 1}));
    assert!(reopened
        .get_json_value(NAMESPACE, "new-generation")
        .expect("read recovered new key")
        .is_none());
    assert!(!root.join(".beetle-memory.transaction").exists());
}

#[test]
fn prepared_journal_recovers_a_mid_delete_unpaired_index() {
    let root = temp_root("mid-delete");
    seed(&root);
    let status = crash_worker(&root, "after_json_data_delete_before_index");
    assert!(!status.success());
    assert_journal_state(&root, "prepared");

    let reopened = open(&root);
    assert_eq!(read_generation(&reopened), json!({"generation": 1}));
    assert!(!root.join(".beetle-memory.transaction").exists());
}

#[test]
fn prepared_journal_recovers_a_truncated_event_suffix() {
    let root = temp_root("mid-event");
    seed(&root);
    let status = crash_worker(&root, "mid_event_append");
    assert!(!status.success());
    assert_journal_state(&root, "prepared");

    let reopened = open(&root);
    assert_eq!(read_generation(&reopened), json!({"generation": 1}));
    assert_eq!(reopened.read_events().expect("recovered events").len(), 1);
    assert!(!root.join(".beetle-memory.transaction").exists());
}

#[test]
fn primitive_blob_and_event_recover_as_one_journaled_transaction() {
    let root = temp_root("primitive-prepared");
    let platform = support::open_store(config(&root)).expect("initialize file store");
    assert!(platform
        .state_fs()
        .read("primitive/state.bin")
        .expect("read initial blob")
        .is_none());
    assert!(!platform
        .read_events()
        .expect("initial events")
        .iter()
        .any(|event| event.plane == "state_fs" && event.record_key == "primitive/state.bin"));
    drop(platform);

    let status = primitive_crash_worker(&root, "after_apply_before_commit");
    assert!(!status.success(), "primitive worker must crash after apply");
    assert_journal_state(&root, "prepared");

    let recovered = support::open_store(config(&root)).expect("recover file store");
    assert!(recovered
        .state_fs()
        .read("primitive/state.bin")
        .expect("read recovered blob")
        .is_none());
    assert!(
        !recovered
            .read_events()
            .expect("read recovered events")
            .iter()
            .any(|event| event.plane == "state_fs" && event.record_key == "primitive/state.bin"),
        "primitive data and its event must roll back together"
    );
    assert!(!root.join(".beetle-memory.transaction").exists());
}

#[test]
fn committed_journal_recovers_after_image_after_process_crash() {
    let root = temp_root("committed");
    seed(&root);

    let status = crash_worker(&root, "after_commit_before_cleanup");
    assert!(
        !status.success(),
        "worker must terminate at the committed journal crash point"
    );
    assert_journal_state(&root, "committed");

    let recovered = open(&root);
    assert_eq!(read_generation(&recovered), json!({"generation": 2}));
    assert_eq!(
        recovered
            .read_events()
            .expect("events after commit replay")
            .len(),
        2
    );
    drop(recovered);

    let reopened = open(&root);
    assert_eq!(read_generation(&reopened), json!({"generation": 2}));
    assert_eq!(
        reopened
            .read_events()
            .expect("events after repeated recovery")
            .len(),
        2
    );
    assert!(
        !root.join(".beetle-memory.transaction").exists(),
        "successful recovery must clean the durable journal"
    );
}

#[test]
fn ordinary_and_namespace_reads_recover_before_exposing_a_prepared_after_image() {
    let root = temp_root("prepared-after-image-read-fence");
    seed(&root);
    let ordinary_reader = open(&root);
    let namespace_reader = open(&root);

    let status = crash_worker(&root, "after_apply_before_commit");
    assert!(
        !status.success(),
        "worker must terminate after applying the prepared after-image"
    );
    assert_journal_state(&root, "prepared");

    assert_eq!(
        ordinary_reader
            .get_json_value(NAMESPACE, KEY)
            .expect("ordinary read after recovery"),
        Some(json!({"generation": 1}))
    );
    assert_eq!(
        ordinary_reader
            .read_events()
            .expect("ordinary event read after recovery")
            .len(),
        1
    );

    let exact_read = namespace_reader
        .read_consistent(&StoreConsistentReadRequest {
            json: vec![StoreJsonAddress::new(NAMESPACE, KEY)],
            blobs: Vec::new(),
            include_events: true,
        })
        .expect("exact read after recovery");
    assert_eq!(exact_read.json.len(), 1);
    assert_eq!(exact_read.json[0].value, Some(json!({"generation": 1})));
    assert_eq!(exact_read.events.len(), 1);
    assert!(
        !root.join(".beetle-memory.transaction").exists(),
        "the first ordinary read must complete prepared-journal recovery"
    );
}

#[test]
fn active_after_image_stays_hidden_until_commit_and_journal_cleanup_finish() {
    let root = temp_root("active-after-image-read-fence");
    seed(&root);
    let reader_config = config(&root).with_lock_timeout(Duration::from_millis(50));
    let ordinary_reader = support::open_file_engine(&reader_config)
        .expect("ordinary reader")
        .0;
    let namespace_reader = support::open_file_engine(&reader_config)
        .expect("namespace reader")
        .0;
    let ready = root.join("after-image.ready");
    let release = root.join("after-image.release");

    let mut writer = pause_worker(&root, "after_apply_before_commit", &ready, &release);
    wait_for_path(&ready);
    assert_journal_state(&root, "prepared");

    let ordinary_error = ordinary_reader
        .get_json_value(NAMESPACE, KEY)
        .expect_err("ordinary read must not expose the uncommitted after-image");
    assert_eq!(ordinary_error.stage(), "store_transaction_busy");
    let exact_read_error = namespace_reader
        .read_consistent(&StoreConsistentReadRequest {
            json: vec![StoreJsonAddress::new(NAMESPACE, KEY)],
            blobs: Vec::new(),
            include_events: true,
        })
        .expect_err("exact read must not expose the uncommitted after-image");
    assert_eq!(exact_read_error.stage(), "store_transaction_busy");

    fs::write(&release, b"release").expect("release writer");
    assert!(writer.wait().expect("pause worker").success());
    assert_eq!(
        ordinary_reader
            .get_json_value(NAMESPACE, KEY)
            .expect("ordinary committed read"),
        Some(json!({"generation": 2}))
    );
    let exact_read = namespace_reader
        .read_consistent(&StoreConsistentReadRequest {
            json: vec![StoreJsonAddress::new(NAMESPACE, KEY)],
            blobs: Vec::new(),
            include_events: true,
        })
        .expect("exact committed read");
    assert_eq!(exact_read.json[0].value, Some(json!({"generation": 2})));
    assert_eq!(exact_read.events.len(), 2);
    assert!(
        !root.join(".beetle-memory.transaction").exists(),
        "readers may proceed only after committed-journal cleanup"
    );
}

#[test]
fn independent_file_opens_still_admit_exactly_one_cas_writer() {
    let root = temp_root("independent-opens");
    seed(&root);
    let first = open(&root);
    let second = open(&root);
    let barrier = Arc::new(Barrier::new(2));

    let first_barrier = barrier.clone();
    let first_writer = thread::spawn(move || {
        first_barrier.wait();
        first.commit_transaction(&request("first", Some(1), 2))
    });
    let second_writer = thread::spawn(move || {
        barrier.wait();
        second.commit_transaction(&request("second", Some(1), 2))
    });

    let outcomes = [
        first_writer.join().expect("first writer"),
        second_writer.join().expect("second writer"),
    ];
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.as_ref().is_err_and(|error| {
                error.stage() == "memory_write_transaction_precondition_failed"
            }))
            .count(),
        1
    );

    let reader = open(&root);
    assert_eq!(read_generation(&reader), json!({"generation": 2}));
    assert_eq!(reader.read_events().expect("events after CAS").len(), 2);
}

#[test]
fn transaction_syncs_every_rename_parent_before_journal_removal() {
    let root = temp_root("durability-order");
    seed(&root);
    let trace = root.join("durability.trace");

    let status = durability_worker(&root, &trace);
    assert!(status.success(), "durability worker must commit");
    assert_durability_trace(&trace);
}

#[test]
fn recovery_syncs_every_rename_parent_before_journal_removal() {
    let root = temp_root("recovery-durability-order");
    seed(&root);
    let status = crash_worker(&root, "after_commit_before_cleanup");
    assert!(
        !status.success(),
        "crash worker must leave committed journal"
    );
    let trace = root.join("recovery-durability.trace");

    let status = recovery_durability_worker(&root, &trace);
    assert!(status.success(), "recovery durability worker must open");
    assert_durability_trace(&trace);
}

fn assert_durability_trace(trace: &Path) {
    let lines = fs::read_to_string(trace).expect("durability trace");
    let mut pending = BTreeMap::<String, usize>::new();
    let mut parent_syncs = BTreeMap::<String, BTreeSet<String>>::new();
    let mut saw_journal_remove = false;
    for line in lines.lines() {
        let fields = line.split('|').collect::<Vec<_>>();
        match fields.as_slice() {
            ["rename_begin", id, expected_parent_count, ..] => {
                pending.insert(
                    (*id).to_string(),
                    expected_parent_count.parse().expect("parent count"),
                );
            }
            ["parent_sync", id, parent] => {
                parent_syncs
                    .entry((*id).to_string())
                    .or_default()
                    .insert((*parent).to_string());
            }
            ["rename_durable", id] => {
                let expected = pending.remove(*id).expect("rename must be pending");
                let observed = parent_syncs.get(*id).map_or(0, BTreeSet::len);
                assert_eq!(observed, expected, "rename {id} trace:\n{lines}");
            }
            ["journal_remove"] => {
                assert!(
                    pending.is_empty(),
                    "unsynced rename before cleanup: {lines}"
                );
                saw_journal_remove = true;
            }
            _ => panic!("unknown durability trace record {line:?}"),
        }
    }
    assert!(
        saw_journal_remove,
        "journal removal must be observable: {lines}"
    );
    assert!(
        pending.is_empty(),
        "every rename must become durable: {lines}"
    );
}

#[test]
fn tampered_journal_checksum_phase_and_after_image_require_repair() {
    for tamper in ["checksum", "phase", "after"] {
        let root = temp_root(&format!("tampered-{tamper}"));
        seed(&root);
        let status = crash_worker(&root, "after_commit_before_cleanup");
        assert!(!status.success(), "crash worker must leave journal");

        let marker = root.join(".beetle-memory.transaction");
        let mut journal: Value = serde_json::from_slice(&fs::read(&marker).expect("journal bytes"))
            .expect("journal json");
        match tamper {
            "checksum" => journal["checksum"] = Value::String("0".repeat(64)),
            "phase" => journal["state"] = Value::String("prepared".to_string()),
            "after" => journal["after"]["json"][0]["value"] = json!({"generation": 99}),
            _ => unreachable!(),
        }
        fs::write(
            &marker,
            serde_json::to_vec(&journal).expect("tampered journal"),
        )
        .expect("write tampered journal");

        let error = support::open_file_engine(&config(&root))
            .err()
            .expect("tampered journal must fail closed");
        assert_eq!(
            error.stage(),
            "memory_write_transaction_repair_required",
            "tamper={tamper}: {error}"
        );
    }
}

#[test]
fn file_transaction_recovery_crash_worker() {
    let Some(root) = std::env::var_os("BM_FILE_TRANSACTION_ROOT") else {
        return;
    };
    let Some(crash_point) = std::env::var_os("BM_FILE_TRANSACTION_CRASH_POINT") else {
        return;
    };
    assert_eq!(
        std::env::var_os("BM_FILE_TRANSACTION_RECOVERY_WORKER"),
        Some("1".into()),
        "worker must be explicitly enabled"
    );

    let request = match crash_point.to_str() {
        Some("after_json_index_before_data") => {
            request_for_key("crash-mid-put", "new-generation", None, 2)
        }
        Some("after_json_data_delete_before_index") => delete_request("crash-mid-delete"),
        Some("mid_event_append") => request("crash-mid-event", Some(1), 2),
        _ => request("crash", Some(1), 2),
    };
    open(Path::new(&root))
        .commit_transaction(&request)
        .unwrap_or_else(|error| panic!("transaction returned before {crash_point:?}: {error}"));
    panic!("transaction did not terminate at {crash_point:?}");
}

#[test]
fn file_primitive_recovery_crash_worker() {
    let Some(root) = std::env::var_os("BM_FILE_TRANSACTION_ROOT") else {
        return;
    };
    let Some(crash_point) = std::env::var_os("BM_FILE_PRIMITIVE_CRASH_POINT") else {
        return;
    };
    assert_eq!(
        std::env::var_os("BM_FILE_PRIMITIVE_RECOVERY_WORKER"),
        Some("1".into()),
        "worker must be explicitly enabled"
    );

    let platform =
        support::open_store(config(Path::new(&root))).expect("open primitive file store");
    std::env::set_var("BM_FILE_TRANSACTION_RECOVERY_WORKER", "1");
    std::env::set_var("BM_FILE_TRANSACTION_CRASH_POINT", &crash_point);
    platform
        .state_fs()
        .write("primitive/state.bin", b"after")
        .unwrap_or_else(|error| panic!("primitive write returned before {crash_point:?}: {error}"));
    panic!("primitive write did not terminate at {crash_point:?}");
}

#[test]
fn file_transaction_pause_worker() {
    let Some(root) = std::env::var_os("BM_FILE_TRANSACTION_ROOT") else {
        return;
    };
    if std::env::var_os("BM_FILE_TRANSACTION_PAUSE_WORKER").is_none() {
        return;
    }
    open(Path::new(&root))
        .commit_transaction(&request("pause", Some(1), 2))
        .expect("paused transaction");
}

#[test]
fn file_transaction_durability_worker() {
    let Some(root) = std::env::var_os("BM_FILE_TRANSACTION_ROOT") else {
        return;
    };
    if std::env::var_os("BM_FILE_TRANSACTION_DURABILITY_WORKER").is_none() {
        return;
    }
    open(Path::new(&root))
        .commit_transaction(&request("durability", Some(1), 2))
        .expect("durability transaction");
}

#[test]
fn file_transaction_recovery_durability_worker() {
    let Some(root) = std::env::var_os("BM_FILE_TRANSACTION_ROOT") else {
        return;
    };
    if std::env::var_os("BM_FILE_TRANSACTION_RECOVERY_DURABILITY_WORKER").is_none() {
        return;
    }
    open(Path::new(&root));
}

fn assert_journal_state(root: &Path, expected: &str) {
    let bytes = std::fs::read(root.join(".beetle-memory.transaction"))
        .expect("crash must leave a durable transaction journal");
    let journal: Value = serde_json::from_slice(&bytes).expect("journal json");
    assert_eq!(journal["state"], expected, "journal: {journal}");
    assert!(journal["before"].is_object(), "journal: {journal}");
    assert!(journal["after"].is_object(), "journal: {journal}");
    assert!(
        journal["checksum"]
            .as_str()
            .is_some_and(|value| value.len() == 64),
        "journal: {journal}"
    );
}
