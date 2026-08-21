mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(feature = "sqlite-store")]
use std::thread;
#[cfg(feature = "sqlite-store")]
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_sdk::{
    GovernedRuntimeSkillWriteInput, MemoryMutationExecution, MemoryMutationReceipt,
    MemoryPrivacyClass, MemoryRuntime, MemoryWriteRequest, RuntimeSkillCreationRef,
    RuntimeSkillOwningScope, RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};
use sha2::{Digest, Sha256};

const OPERATION_ID: &str = "store-contract-durable-operation";
const MEMORY_SPACE_ID: &str = "space:test";
const RECEIPT_NAMESPACE: &str = "memory_mutation_receipts";
const AUDIT_NAMESPACE: &str = "memory_mutation_audits";

fn temp_root(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "beetle-memory-operation-recovery-{name}-{}-{suffix}",
        std::process::id()
    ))
}

fn file_config(root: &Path) -> StoreBackendConfig {
    StoreBackendConfig::file(root, support::native_persistent_profile()).expect("file config")
}

#[cfg(feature = "sqlite-store")]
fn sqlite_config(path: &Path) -> StoreBackendConfig {
    StoreBackendConfig::sqlite(path, support::native_persistent_profile()).expect("sqlite config")
}

fn operation_request(runtime: &MemoryRuntime) -> MemoryWriteRequest {
    let write = RuntimeSkillWrite {
        name: "runtime_skill__durable_operation_recovery".to_string(),
        topic: "durable operation recovery".to_string(),
        title: "Durable operation recovery".to_string(),
        summary: "One operation identity must commit one durable effect.".to_string(),
        content: "1. Persist the governed effect with its receipt and authoritative audit.\n2. Retry the same operation identity and verify that no second effect is committed."
            .to_string(),
        citations: vec!["store-contract:durable-operation".to_string()],
        source_chat_id: Some("chat-a".to_string()),
        observed_at: 100,
    };
    let owning_scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: runtime.subject_id().to_string(),
    };
    let candidate_ref = "store-contract:durable-operation-recovery".to_string();
    let verification_receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(format!("{candidate_ref}\n{}\n{}", write.title, write.content).as_bytes())
    );
    MemoryWriteRequest::Procedural {
        writes: vec![GovernedRuntimeSkillWriteInput {
            write,
            creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                candidate_ref,
                verification_receipt_digest,
            },
            privacy_class: MemoryPrivacyClass::SharedWithSubject,
        }],
        owning_scope,
        source: RuntimeSkillWriteSource::Manual,
    }
}

fn assert_authoritative_pair(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
    receipt: &MemoryMutationReceipt,
) {
    let key = receipt.identity.storage_key();
    let receipt_docs = platform
        .read_json_docs_by_keys(RECEIPT_NAMESPACE, std::slice::from_ref(&key))
        .expect("read authoritative receipt");
    let audit_docs = platform
        .read_json_docs_by_keys(AUDIT_NAMESPACE, std::slice::from_ref(&key))
        .expect("read authoritative audit");
    assert_eq!(receipt_docs.len(), 1, "exactly one receipt must persist");
    assert_eq!(audit_docs.len(), 1, "exactly one audit must persist");
    assert_eq!(
        serde_json::from_value::<MemoryMutationReceipt>(receipt_docs[0].value.clone())
            .expect("decode persisted receipt"),
        *receipt
    );
    assert_eq!(
        audit_docs[0].value["transaction_id"],
        receipt.transaction_id
    );
    assert_eq!(audit_docs[0].value["intent_digest"], receipt.intent_digest);
    assert_eq!(
        audit_docs[0].value["effect_plan_digest"],
        receipt.effect_plan_digest
    );
}

fn spawn_file_crash_worker(root: &Path, crash_point: &str) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("file_mutation_operation_crash_worker")
        .arg("--nocapture")
        .env("BM_OPERATION_FILE_WORKER", "1")
        .env("BM_OPERATION_STORE_ROOT", root)
        .env("BM_FILE_TRANSACTION_RECOVERY_WORKER", "1")
        .env("BM_FILE_TRANSACTION_CRASH_POINT", crash_point)
        .env("BM_FILE_TRANSACTION_CRASH_REQUIRES_OPERATION_PAIR", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn operation crash worker")
}

#[test]
fn file_operation_pair_recovers_prepared_and_committed_process_crashes() {
    for (crash_point, expected_first_outcome) in [
        ("after_apply_before_commit", "committed"),
        ("after_commit_before_cleanup", "replayed"),
    ] {
        let root = temp_root(crash_point);
        let status = spawn_file_crash_worker(&root, crash_point);
        assert!(!status.success(), "worker must terminate at {crash_point}");

        let platform = support::open_store(file_config(&root)).expect("recover file store");
        let runtime = support::runtime_for_scope(&platform, MEMORY_SPACE_ID, 100);
        let outcome = runtime
            .write_operation(OPERATION_ID, operation_request(&runtime))
            .expect("retry recovered operation");
        let receipt = match (expected_first_outcome, outcome) {
            ("committed", MemoryMutationExecution::Committed { receipt, .. }) => receipt,
            ("replayed", MemoryMutationExecution::Replayed { receipt }) => receipt,
            (_, actual) => panic!("unexpected recovered operation outcome: {actual:?}"),
        };
        let replay = runtime
            .write_operation(OPERATION_ID, operation_request(&runtime))
            .expect("repeat recovered operation");
        assert_eq!(
            replay,
            MemoryMutationExecution::Replayed {
                receipt: receipt.clone()
            }
        );
        assert_authoritative_pair(&platform, &receipt);
    }
}

#[test]
fn file_mutation_operation_crash_worker() {
    if std::env::var_os("BM_OPERATION_FILE_WORKER").is_none() {
        return;
    }
    let root = PathBuf::from(
        std::env::var_os("BM_OPERATION_STORE_ROOT").expect("operation worker store root"),
    );
    let platform = support::open_store(file_config(&root)).expect("open operation worker store");
    let runtime = support::runtime_for_scope(&platform, MEMORY_SPACE_ID, 100);
    runtime
        .write_operation(OPERATION_ID, operation_request(&runtime))
        .expect("operation must terminate at the configured crash point");
    panic!("operation did not terminate at the configured crash point");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_multiprocess_same_operation_identity_converges_to_one_receipt() {
    const WORKERS: usize = 4;
    let root = temp_root("sqlite-multiprocess");
    std::fs::create_dir_all(&root).expect("create sqlite operation root");
    let path = root.join("memory.sqlite3");
    let initializer =
        support::open_store(sqlite_config(&path)).expect("initialize sqlite operation store");
    let release = root.join("release");
    let mut children = Vec::new();
    for index in 0..WORKERS {
        let ready = root.join(format!("ready-{index}"));
        let outcome = root.join(format!("outcome-{index}.json"));
        let child = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("sqlite_mutation_operation_worker")
            .arg("--nocapture")
            .env("BM_OPERATION_SQLITE_WORKER", "1")
            .env("BM_OPERATION_SQLITE_PATH", &path)
            .env("BM_OPERATION_WORKER_READY", &ready)
            .env("BM_OPERATION_WORKER_RELEASE", &release)
            .env("BM_OPERATION_WORKER_OUTCOME", &outcome)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sqlite operation worker");
        children.push((child, ready, outcome));
        let deadline = Instant::now() + Duration::from_secs(15);
        while !children
            .last()
            .expect("just-pushed sqlite worker")
            .1
            .exists()
        {
            assert!(
                Instant::now() < deadline,
                "sqlite worker {index} did not become ready"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
    std::fs::write(&release, b"release").expect("release sqlite operation workers");

    let mut committed = 0;
    let mut replayed = 0;
    let mut receipts = Vec::new();
    for (mut child, _, outcome) in children {
        assert!(child.wait().expect("wait sqlite worker").success());
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(outcome).expect("read sqlite worker outcome"))
                .expect("decode sqlite worker outcome");
        match value["outcome"].as_str() {
            Some("committed") => committed += 1,
            Some("replayed") => replayed += 1,
            other => panic!("unexpected sqlite worker outcome: {other:?}"),
        }
        receipts.push(
            serde_json::from_value::<MemoryMutationReceipt>(value["receipt"].clone())
                .expect("decode sqlite worker receipt"),
        );
    }
    assert_eq!(committed, 1, "exactly one process may commit the effect");
    assert_eq!(replayed, WORKERS - 1);
    assert!(receipts.windows(2).all(|pair| pair[0] == pair[1]));

    drop(initializer);
    let platform =
        support::open_store(sqlite_config(&path)).expect("reopen sqlite operation store");
    let runtime = support::runtime_for_scope(&platform, MEMORY_SPACE_ID, 100);
    let MemoryMutationExecution::Replayed { receipt } = runtime
        .write_operation(OPERATION_ID, operation_request(&runtime))
        .expect("replay after sqlite multiprocess convergence")
    else {
        panic!("reopen must replay the committed operation")
    };
    assert_eq!(receipt, receipts[0]);
    assert_authoritative_pair(&platform, &receipt);
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_mutation_operation_worker() {
    if std::env::var_os("BM_OPERATION_SQLITE_WORKER").is_none() {
        return;
    }
    let path =
        PathBuf::from(std::env::var_os("BM_OPERATION_SQLITE_PATH").expect("sqlite operation path"));
    let ready = PathBuf::from(
        std::env::var_os("BM_OPERATION_WORKER_READY").expect("sqlite worker ready path"),
    );
    let release = PathBuf::from(
        std::env::var_os("BM_OPERATION_WORKER_RELEASE").expect("sqlite worker release path"),
    );
    let outcome_path = PathBuf::from(
        std::env::var_os("BM_OPERATION_WORKER_OUTCOME").expect("sqlite worker outcome path"),
    );
    let platform = support::open_store(sqlite_config(&path)).expect("open sqlite worker store");
    let runtime = support::runtime_for_scope(&platform, MEMORY_SPACE_ID, 100);
    std::fs::write(&ready, b"ready").expect("announce sqlite worker readiness");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !release.exists() {
        assert!(Instant::now() < deadline, "sqlite worker release timed out");
        thread::sleep(Duration::from_millis(2));
    }
    let (outcome, receipt) = match runtime
        .write_operation(OPERATION_ID, operation_request(&runtime))
        .expect("execute sqlite operation")
    {
        MemoryMutationExecution::Committed { receipt, .. } => ("committed", receipt),
        MemoryMutationExecution::Replayed { receipt } => ("replayed", receipt),
        MemoryMutationExecution::Rejected { report } => {
            panic!("durable operation unexpectedly rejected: {report:?}")
        }
    };
    std::fs::write(
        outcome_path,
        serde_json::to_vec(&serde_json::json!({
            "outcome": outcome,
            "receipt": receipt,
        }))
        .expect("encode sqlite worker outcome"),
    )
    .expect("persist sqlite worker outcome");
}
