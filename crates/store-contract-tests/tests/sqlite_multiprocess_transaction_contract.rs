#![cfg(feature = "sqlite-store")]

use std::collections::BTreeSet;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bm_core::feature_gate::ProfileId;
use bm_sdk::nonproduction_replay_harness::{
    MemoryStoreEvent, MemoryStoreEventKind, SqliteStoreEngine, StoreBackendConfig,
    StoreConsistentReadRequest, StoreEngine, StoreEngineMutation, StoreEventScope,
    StoreJsonAddress, StoreJsonPrecondition, StoreTransactionRequest,
};
use serde_json::{json, Value};

const NAMESPACE: &str = "memory_graph_manifests";
const REVISION_KEY: &str = "multiprocess-cas-revision";
const POST_IMAGE_KEY: &str = "multiprocess-cas-post-image";
const SQLITE_PRE_CAS_FAILPOINT: &str = "after_begin_immediate_before_load_transaction_state";
const FIRST_READY: u8 = b'1';
const SECOND_READY: u8 = b'2';
const BEGIN: u8 = b'B';
const PAUSED: u8 = b'P';
const RELEASE: u8 = b'C';
const SUCCESS: u8 = b'S';
const CAS_CONFLICT: u8 = b'X';

#[test]
fn sqlite_multiprocess_exact_cas_has_one_winner_and_complete_post_image() {
    let root = temp_root();
    std::fs::create_dir_all(&root).expect("create sqlite test root");
    let path = root.join("memory.sqlite3");
    let config = sqlite_config(&path);
    let (seed, _) = SqliteStoreEngine::open(&config).expect("open seed sqlite engine");
    seed.commit_transaction(&seed_request())
        .expect("seed expected revision");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind process barrier");
    let barrier = listener.local_addr().expect("barrier address");
    listener
        .set_nonblocking(true)
        .expect("make process barrier nonblocking");

    let mut first = spawn_worker(&path, barrier, "first");
    let mut second = spawn_worker(&path, barrier, "second");
    let (mut first_control, mut second_control) =
        accept_ready_workers(&listener, &mut first, &mut second);
    send_worker_signal(&mut first_control, BEGIN, "start first transaction");
    let mut first_pause =
        accept_worker_signal(&listener, &mut first, PAUSED, "first pre-CAS pause");

    send_worker_signal(&mut second_control, BEGIN, "start second transaction");
    assert_no_second_pre_cas_handshake(&listener, &mut second);

    send_worker_signal(&mut first_pause, RELEASE, "release first pre-CAS pause");
    assert_eq!(read_worker_outcome(&mut first_control), SUCCESS);
    assert!(first
        .wait_with_output()
        .expect("wait first worker")
        .status
        .success());

    let mut second_pause =
        accept_worker_signal(&listener, &mut second, PAUSED, "second pre-CAS pause");
    send_worker_signal(&mut second_pause, RELEASE, "release second pre-CAS pause");
    assert_eq!(read_worker_outcome(&mut second_control), CAS_CONFLICT);

    assert!(second
        .wait_with_output()
        .expect("wait second worker")
        .status
        .success());

    let (reader, _) = SqliteStoreEngine::open(&config).expect("open fresh sqlite reader");
    let post_image = reader
        .read_consistent(&StoreConsistentReadRequest {
            json: vec![
                StoreJsonAddress::new(NAMESPACE, REVISION_KEY),
                StoreJsonAddress::new(NAMESPACE, POST_IMAGE_KEY),
            ],
            blobs: Vec::new(),
            include_events: true,
        })
        .expect("read committed post image");

    let revision = post_image.json[0]
        .value
        .as_ref()
        .expect("committed revision");
    let body = post_image.json[1]
        .value
        .as_ref()
        .expect("committed post image");
    let writer = revision["writer"].as_str().expect("revision writer");
    assert!(matches!(writer, "first" | "second"));
    assert_eq!(revision["revision"], 2);
    assert_eq!(body["writer"], writer);
    assert_eq!(body["entries"], json!(["owner", "facet", "event"]));
    assert_eq!(post_image.events.len(), 2, "seed plus one winning commit");
    let event_ids = post_image
        .events
        .iter()
        .map(|event| event.event_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(event_ids.len(), 2, "committed events must be unique");
    assert!(event_ids.contains("sqlite-multiprocess-seed"));
    assert!(event_ids.contains(&format!("sqlite-multiprocess-{writer}").as_str()));
}

#[test]
fn sqlite_multiprocess_transaction_worker() {
    let Some(writer) = std::env::var_os("BM_SQLITE_MULTIPROCESS_WRITER") else {
        return;
    };
    let path =
        PathBuf::from(std::env::var_os("BM_SQLITE_MULTIPROCESS_PATH").expect("worker sqlite path"));
    let barrier =
        std::env::var("BM_SQLITE_MULTIPROCESS_BARRIER").expect("worker process barrier address");
    let writer = writer.to_string_lossy().into_owned();
    let (engine, _) = SqliteStoreEngine::open(&sqlite_config(&path)).expect("open worker engine");
    let mut stream = TcpStream::connect(barrier).expect("connect process barrier");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set worker barrier timeout");
    stream
        .write_all(&[if writer == "first" {
            FIRST_READY
        } else {
            SECOND_READY
        }])
        .expect("announce worker readiness");
    stream.flush().expect("flush worker readiness");
    let mut release = [0];
    stream
        .read_exact(&mut release)
        .expect("wait for barrier release");
    assert_eq!(release, [BEGIN]);

    let outcome = match engine.commit_transaction(&writer_request(&writer)) {
        Ok(_) => SUCCESS,
        Err(error) if error.stage() == "memory_write_transaction_precondition_failed" => {
            CAS_CONFLICT
        }
        Err(error) => panic!("worker must receive a typed CAS conflict, got {error}"),
    };
    stream.write_all(&[outcome]).expect("report worker outcome");
    stream.flush().expect("flush worker outcome");
}

fn accept_worker_signal(
    listener: &TcpListener,
    child: &mut std::process::Child,
    expected: u8,
    phase: &str,
) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (mut worker, _) = match listener.accept() {
            Ok(worker) => worker,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if let Some(status) = child.try_wait().expect("poll transaction worker") {
                    panic!("worker exited before {phase}: {status}");
                }
                assert!(Instant::now() < deadline, "worker did not reach {phase}");
                std::thread::yield_now();
                continue;
            }
            Err(error) => panic!("accept {phase}: {error}"),
        };
        worker
            .set_nonblocking(false)
            .expect("make parent worker stream blocking");
        worker
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("set parent barrier timeout");
        let mut signal = [0];
        worker
            .read_exact(&mut signal)
            .unwrap_or_else(|error| panic!("read {phase}: {error}"));
        assert_eq!(signal, [expected], "unexpected signal during {phase}");
        return worker;
    }
}

fn accept_ready_workers(
    listener: &TcpListener,
    first: &mut std::process::Child,
    second: &mut std::process::Child,
) -> (TcpStream, TcpStream) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut first_control = None;
    let mut second_control = None;
    while first_control.is_none() || second_control.is_none() {
        match listener.accept() {
            Ok((mut worker, _)) => {
                worker
                    .set_nonblocking(false)
                    .expect("make parent worker stream blocking");
                worker
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .expect("set parent barrier timeout");
                let mut signal = [0];
                worker
                    .read_exact(&mut signal)
                    .expect("read worker readiness");
                match signal {
                    [FIRST_READY] if first_control.is_none() => first_control = Some(worker),
                    [SECOND_READY] if second_control.is_none() => second_control = Some(worker),
                    _ => panic!("unexpected worker readiness signal: {signal:?}"),
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if let Some(status) = first.try_wait().expect("poll first transaction worker") {
                    panic!("first worker exited before readiness: {status}");
                }
                if let Some(status) = second.try_wait().expect("poll second transaction worker") {
                    panic!("second worker exited before readiness: {status}");
                }
                assert!(
                    Instant::now() < deadline,
                    "workers did not both become ready"
                );
                std::thread::yield_now();
            }
            Err(error) => panic!("accept worker readiness: {error}"),
        }
    }
    (
        first_control.expect("first worker readiness"),
        second_control.expect("second worker readiness"),
    )
}

fn assert_no_second_pre_cas_handshake(listener: &TcpListener, second: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match listener.accept() {
            Ok((mut worker, _)) => {
                worker
                    .set_nonblocking(false)
                    .expect("make unexpected handshake stream blocking");
                worker
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .expect("set unexpected handshake timeout");
                let mut signal = [0];
                worker
                    .read_exact(&mut signal)
                    .expect("read unexpected second handshake");
                panic!(
                    "second worker reached the pre-CAS failpoint while first held BEGIN IMMEDIATE: {:?}",
                    signal
                );
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if let Some(status) = second.try_wait().expect("poll second transaction worker") {
                    panic!("second worker exited before first commit released its write fence: {status}");
                }
                if Instant::now() >= deadline {
                    return;
                }
                std::thread::yield_now();
            }
            Err(error) => panic!("inspect second pre-CAS handshake: {error}"),
        }
    }
}

fn send_worker_signal(worker: &mut TcpStream, signal: u8, phase: &str) {
    worker
        .write_all(&[signal])
        .unwrap_or_else(|error| panic!("send {phase}: {error}"));
    worker
        .flush()
        .unwrap_or_else(|error| panic!("flush {phase}: {error}"));
}

fn read_worker_outcome(worker: &mut TcpStream) -> u8 {
    let mut outcome = [0];
    worker
        .read_exact(&mut outcome)
        .expect("read worker outcome");
    outcome[0]
}

fn spawn_worker(path: &Path, barrier: std::net::SocketAddr, writer: &str) -> std::process::Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("sqlite_multiprocess_transaction_worker")
        .arg("--nocapture")
        .env("BM_SQLITE_MULTIPROCESS_WRITER", writer)
        .env("BM_SQLITE_MULTIPROCESS_PATH", path)
        .env("BM_SQLITE_MULTIPROCESS_BARRIER", barrier.to_string())
        .env("BM_SQLITE_TRANSACTION_FAILPOINT", SQLITE_PRE_CAS_FAILPOINT)
        .env("BM_SQLITE_TRANSACTION_FAILPOINT_ADDR", barrier.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sqlite worker")
}

fn sqlite_config(path: &Path) -> StoreBackendConfig {
    StoreBackendConfig::sqlite(path, ProfileId::ServerLinuxDevFull).expect("sqlite config")
}

fn seed_request() -> StoreTransactionRequest {
    StoreTransactionRequest::new(
        "sqlite-multiprocess-seed",
        vec![StoreJsonPrecondition::Absent {
            namespace: NAMESPACE.to_string(),
            key: REVISION_KEY.to_string(),
        }],
        vec![
            StoreEngineMutation::PutJson {
                namespace: NAMESPACE.to_string(),
                key: REVISION_KEY.to_string(),
                value: json!({"revision": 1}),
            },
            StoreEngineMutation::AppendEvent {
                event: Box::new(event("sqlite-multiprocess-seed")),
            },
        ],
        None,
    )
}

fn writer_request(writer: &str) -> StoreTransactionRequest {
    StoreTransactionRequest::new(
        format!("sqlite-multiprocess-{writer}"),
        vec![StoreJsonPrecondition::Exact {
            namespace: NAMESPACE.to_string(),
            key: REVISION_KEY.to_string(),
            value: json!({"revision": 1}),
        }],
        vec![
            StoreEngineMutation::PutJson {
                namespace: NAMESPACE.to_string(),
                key: REVISION_KEY.to_string(),
                value: json!({"revision": 2, "writer": writer}),
            },
            StoreEngineMutation::PutJson {
                namespace: NAMESPACE.to_string(),
                key: POST_IMAGE_KEY.to_string(),
                value: complete_post_image(writer),
            },
            StoreEngineMutation::AppendEvent {
                event: Box::new(event(&format!("sqlite-multiprocess-{writer}"))),
            },
        ],
        None,
    )
}

fn complete_post_image(writer: &str) -> Value {
    json!({
        "writer": writer,
        "entries": ["owner", "facet", "event"],
        "revision": 2,
    })
}

fn event(event_id: &str) -> MemoryStoreEvent {
    MemoryStoreEvent::new(
        event_id,
        MemoryStoreEventKind::MemoryWrite,
        StoreEventScope::system("sqlite_multiprocess_transaction_contract"),
        1,
    )
    .with_plane(NAMESPACE)
    .with_record_key(REVISION_KEY)
    .with_content_hash(event_id)
}

fn temp_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "beetle-memory-sqlite-multiprocess-{}-{suffix}",
        std::process::id()
    ))
}
