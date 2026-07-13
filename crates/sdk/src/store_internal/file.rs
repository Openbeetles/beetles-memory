use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bm_core::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::transaction::read_consistent_from_state;
use crate::{
    enforce_event_key_budget, enforce_logical_key_budget, store_budget_error,
    store_internal::transaction::{
        apply_transaction, read_consistent_namespaces_from_state, BackendTransactionState,
        EventOverflowPolicy,
    },
    MemoryStoreEvent, StoreBackendConfig, StoreCapacityBudget, StoreConsistentNamespaceReadRequest,
    StoreConsistentNamespaceReadResult, StoreEngine, StoreEngineMutation, StoreEventLog,
    StorePathBudget, StoreRepairPolicy, StoreRepairReport, StoreSchemaManifest, StoreSnapshotBlob,
    StoreSnapshotJsonDoc, StoreSnapshotReplaceReport, StoreTransactionReport,
    StoreTransactionRequest,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreConsistentReadRequest, StoreConsistentReadResult};

const FILE_ADDRESSING_VERSION: u64 = 2;
const FILE_ADDRESSING_DATA_DIR: &str = "_v2";
const FILE_ADDRESSING_INDEX_DIR: &str = "_keys";
const MIN_PHYSICAL_DIGEST_HEX_CHARS: usize = 16;
const MAX_PHYSICAL_DIGEST_HEX_CHARS: usize = 32;
const TRANSACTION_LOCK_FILE: &str = ".beetle-memory.lock";
const TRANSACTION_MARKER_FILE: &str = ".beetle-memory.transaction";
const TRANSACTION_REPAIR_REQUIRED_STAGE: &str = "memory_write_transaction_repair_required";
static SNAPSHOT_IMPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static DURABILITY_TRACE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static CANONICAL_ROOT_GATES: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

pub struct FileStoreEngine {
    root: PathBuf,
    local_root_gate: Arc<Mutex<()>>,
    fsync: bool,
    capacity: StoreCapacityBudget,
    path_budget: StorePathBudget,
    lock_timeout: std::time::Duration,
}

struct FileBackendLock<'a> {
    _advisory_lock: File,
    _local_root_gate: MutexGuard<'a, ()>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum FileTransactionJournalState {
    Prepared,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileTransactionImage {
    json_namespaces: Vec<String>,
    blob_namespaces: Vec<String>,
    json_docs: Vec<StoreSnapshotJsonDoc>,
    blobs: Vec<StoreSnapshotBlob>,
    events: Vec<MemoryStoreEvent>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileTransactionJournal {
    schema_version: u64,
    transaction_id: String,
    state: FileTransactionJournalState,
    before: FileTransactionImage,
    after: FileTransactionImage,
    checksum: String,
}

impl FileTransactionJournal {
    fn new(
        transaction_id: String,
        before: FileTransactionImage,
        after: FileTransactionImage,
    ) -> Result<Self> {
        let mut journal = Self {
            schema_version: 1,
            transaction_id,
            state: FileTransactionJournalState::Prepared,
            before,
            after,
            checksum: String::new(),
        };
        journal.refresh_checksum()?;
        Ok(journal)
    }

    fn refresh_checksum(&mut self) -> Result<()> {
        self.checksum = self.expected_checksum()?;
        Ok(())
    }

    fn verify_checksum(&self) -> Result<()> {
        if self.checksum != self.expected_checksum()? {
            return Err(Error::config(
                TRANSACTION_REPAIR_REQUIRED_STAGE,
                "file transaction journal checksum mismatch",
            ));
        }
        Ok(())
    }

    fn expected_checksum(&self) -> Result<String> {
        let bytes = serde_json::to_vec(&(
            self.schema_version,
            &self.transaction_id,
            self.state,
            &self.before,
            &self.after,
        ))
        .map_err(|error| Error::config("file_store_transaction", error.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, Debug)]
struct PhysicalKeyPaths {
    data_path: PathBuf,
    index_path: PathBuf,
    legacy_path: PathBuf,
}

impl FileStoreEngine {
    pub fn open(
        config: &StoreBackendConfig,
    ) -> Result<(Self, StoreRepairReport, StoreSchemaManifest)> {
        let root = config
            .data_path
            .clone()
            .ok_or_else(|| Error::config("file_store_open", "file store root is required"))?;
        fs::create_dir_all(&root).map_err(|error| Error::io("file_store_open", error))?;
        let root = fs::canonicalize(&root).map_err(|error| Error::io("file_store_open", error))?;
        let engine = Self {
            local_root_gate: canonical_root_gate(&root)?,
            root,
            fsync: config.fsync,
            capacity: config.capacity,
            path_budget: config.path_budget,
            lock_timeout: config.lock_timeout,
        };
        let (repair, manifest) = {
            let _lock = engine.acquire_backend_lock(true, "file_store_open")?;
            engine.recover_transaction_if_needed()?;
            fs::create_dir_all(engine.root.join("events"))
                .map_err(|error| Error::io("file_store_open", error))?;
            fs::create_dir_all(engine.root.join("kv"))
                .map_err(|error| Error::io("file_store_open", error))?;
            fs::create_dir_all(engine.root.join("blob"))
                .map_err(|error| Error::io("file_store_open", error))?;
            fs::create_dir_all(engine.root.join("snapshots"))
                .map_err(|error| Error::io("file_store_open", error))?;
            (
                engine.repair_orphan_tmp_files(config.repair_policy)?,
                engine.open_or_create_manifest(config)?,
            )
        };
        Ok((engine, repair, manifest))
    }

    fn acquire_backend_lock(
        &self,
        exclusive: bool,
        stage: &'static str,
    ) -> Result<FileBackendLock<'_>> {
        let local_root_gate = self.local_root_gate.lock().map_err(|_| {
            Error::config(
                "store_transaction_lock_failed",
                "canonical file-store root lock is poisoned",
            )
        })?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(TRANSACTION_LOCK_FILE))
            .map_err(|error| Error::io(stage, error))?;
        let started = Instant::now();
        loop {
            let result = if exclusive {
                lock.try_lock()
            } else {
                lock.try_lock_shared()
            };
            match result {
                Ok(()) => {
                    return Ok(FileBackendLock {
                        _advisory_lock: lock,
                        _local_root_gate: local_root_gate,
                    });
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    if started.elapsed() >= self.lock_timeout {
                        return Err(Error::config(
                            "store_transaction_busy",
                            format!("timed out acquiring file backend lock for {stage}"),
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(Error::io("store_transaction_lock_failed", error));
                }
            }
        }
    }

    fn transaction_marker_path(&self) -> PathBuf {
        self.root.join(TRANSACTION_MARKER_FILE)
    }

    fn recover_transaction_if_needed(&self) -> Result<()> {
        let path = self.transaction_marker_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(Error::io("file_store_transaction_recovery", error)),
        };
        let journal: FileTransactionJournal = serde_json::from_slice(&bytes).map_err(|error| {
            let stage = if has_complete_journal_shape(&bytes) {
                TRANSACTION_REPAIR_REQUIRED_STAGE
            } else {
                "store_transaction_recovery_required"
            };
            Error::config(
                stage,
                format!("file transaction journal is not recoverable: {error}"),
            )
        })?;
        if journal.schema_version != 1 {
            return Err(Error::config(
                TRANSACTION_REPAIR_REQUIRED_STAGE,
                format!(
                    "unsupported file transaction journal schema {}",
                    journal.schema_version
                ),
            ));
        }
        journal.verify_checksum()?;
        let image = match journal.state {
            FileTransactionJournalState::Prepared => &journal.before,
            FileTransactionJournalState::Committed => &journal.after,
        };
        self.restore_transaction_image(image)?;
        self.remove_transaction_journal("file_store_transaction_recovery")
    }

    fn transaction_image(
        state: &BackendTransactionState,
        json_namespaces: &BTreeSet<String>,
        blob_namespaces: &BTreeSet<String>,
    ) -> FileTransactionImage {
        FileTransactionImage {
            json_namespaces: json_namespaces.iter().cloned().collect(),
            blob_namespaces: blob_namespaces.iter().cloned().collect(),
            json_docs: state
                .json
                .iter()
                .map(|((namespace, key), value)| StoreSnapshotJsonDoc {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
            blobs: state
                .blobs
                .iter()
                .map(|((namespace, key), value)| StoreSnapshotBlob {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
            events: state.events.clone(),
        }
    }

    fn restore_transaction_image(&self, image: &FileTransactionImage) -> Result<()> {
        let json_namespaces = image
            .json_namespaces
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let blob_namespaces = image
            .blob_namespaces
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        self.replace_snapshot_unlocked(
            &json_namespaces,
            &blob_namespaces,
            &image.json_docs,
            &image.blobs,
            &image.events,
        )?;
        self.sync_root("file_store_transaction_recovery")
    }

    fn write_transaction_journal(&self, journal: &FileTransactionJournal) -> Result<()> {
        let bytes = serde_json::to_vec(journal)
            .map_err(|error| Error::config("file_store_transaction", error.to_string()))?;
        atomic_write(
            &self.transaction_marker_path(),
            &bytes,
            self.fsync,
            "file_store_transaction",
        )?;
        self.sync_root("file_store_transaction")
    }

    fn sync_root(&self, stage: &'static str) -> Result<()> {
        sync_directory(&self.root, self.fsync, stage)
    }

    fn remove_transaction_journal(&self, stage: &'static str) -> Result<()> {
        durability_trace("journal_remove");
        remove_file_if_exists(&self.transaction_marker_path(), stage)?;
        self.sync_root(stage)
    }

    fn maybe_crash_for_recovery_contract(&self, _point: &str) {
        #[cfg(feature = "nonproduction-replay-harness")]
        if std::env::var_os("BM_FILE_TRANSACTION_RECOVERY_WORKER").as_deref()
            == Some(std::ffi::OsStr::new("1"))
            && std::env::var_os("BM_FILE_TRANSACTION_CRASH_POINT").as_deref()
                == Some(std::ffi::OsStr::new(_point))
        {
            std::process::exit(86);
        }
    }

    fn maybe_pause_for_recovery_contract(&self, _point: &str) {
        #[cfg(feature = "nonproduction-replay-harness")]
        if std::env::var_os("BM_FILE_TRANSACTION_PAUSE_POINT").as_deref()
            == Some(std::ffi::OsStr::new(_point))
        {
            let ready = std::env::var_os("BM_FILE_TRANSACTION_PAUSE_READY")
                .map(PathBuf::from)
                .expect("transaction pause ready path");
            let release = std::env::var_os("BM_FILE_TRANSACTION_PAUSE_RELEASE")
                .map(PathBuf::from)
                .expect("transaction pause release path");
            fs::write(&ready, b"ready").expect("write transaction pause marker");
            while !release.exists() {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }

    fn load_transaction_state(&self) -> Result<BackendTransactionState> {
        let mut state = BackendTransactionState {
            events: read_events_jsonl(&self.events_path())?,
            ..BackendTransactionState::default()
        };
        for namespace in
            list_child_directory_names(&self.root.join("kv"), "file_store_transaction")?
        {
            for key in self.list_json_keys_unlocked(&namespace)? {
                if let Some(value) = self.get_json_value_unlocked(&namespace, &key)? {
                    state.json.insert((namespace.clone(), key), value);
                }
            }
        }
        for namespace in
            list_child_directory_names(&self.root.join("blob"), "file_store_transaction")?
        {
            for key in self.list_blob_keys_unlocked(&namespace)? {
                if let Some(value) = self.get_blob_unlocked(&namespace, &key)? {
                    state.blobs.insert((namespace.clone(), key), value);
                }
            }
        }
        Ok(state)
    }

    fn repair_orphan_tmp_files(&self, policy: StoreRepairPolicy) -> Result<StoreRepairReport> {
        let mut findings = Vec::new();
        collect_tmp_files(&self.root, &mut findings)?;
        if findings.is_empty() {
            return Ok(StoreRepairReport::clean());
        }
        match policy {
            StoreRepairPolicy::ReportOnly => Ok(StoreRepairReport::report_only(format!(
                "orphan tmp files: {}",
                findings.join(", ")
            ))),
            StoreRepairPolicy::RepairSafe => {
                for finding in &findings {
                    fs::remove_file(finding)
                        .map_err(|error| Error::io("file_store_repair", error))?;
                }
                Ok(StoreRepairReport {
                    checked: true,
                    repaired: true,
                    findings,
                })
            }
        }
    }

    fn open_or_create_manifest(&self, config: &StoreBackendConfig) -> Result<StoreSchemaManifest> {
        let path = self.root.join("manifest.json");
        let now_secs = current_unix_secs();
        if path.exists() {
            let bytes = fs::read(&path).map_err(|error| Error::io("file_store_manifest", error))?;
            let mut manifest: StoreSchemaManifest = serde_json::from_slice(&bytes)
                .map_err(|error| Error::config("file_store_manifest", error.to_string()))?;
            manifest.validate_against(
                config.backend,
                config.profile,
                config.memory_system_kind,
                "file_store_manifest",
            )?;
            manifest.touch_opened(now_secs);
            self.write_json_file(
                &path,
                &serde_json::to_vec_pretty(&manifest)
                    .map_err(|error| Error::config("file_store_manifest", error.to_string()))?,
            )?;
            Ok(manifest)
        } else {
            let manifest = StoreSchemaManifest::new(config.backend, config.profile, now_secs);
            self.write_json_file(
                &path,
                &serde_json::to_vec_pretty(&manifest)
                    .map_err(|error| Error::config("file_store_manifest", error.to_string()))?,
            )?;
            Ok(manifest)
        }
    }

    fn json_paths(&self, namespace: &str, key: &str) -> Result<PhysicalKeyPaths> {
        self.json_paths_at_root(&self.root, namespace, key)
    }

    fn json_paths_at_root(
        &self,
        root: &Path,
        namespace: &str,
        key: &str,
    ) -> Result<PhysicalKeyPaths> {
        self.physical_key_paths_at_root(root, "kv", namespace, key, "json")
    }

    fn blob_paths(&self, namespace: &str, key: &str) -> Result<PhysicalKeyPaths> {
        self.blob_paths_at_root(&self.root, namespace, key)
    }

    fn blob_paths_at_root(
        &self,
        root: &Path,
        namespace: &str,
        key: &str,
    ) -> Result<PhysicalKeyPaths> {
        self.physical_key_paths_at_root(root, "blob", namespace, key, "bin")
    }

    fn physical_key_paths_at_root(
        &self,
        root: &Path,
        lane: &str,
        namespace: &str,
        key: &str,
        extension: &str,
    ) -> Result<PhysicalKeyPaths> {
        enforce_logical_key_budget(self.capacity, namespace, key, "file_store_addressing")?;
        self.validate_addressing_budget(extension, "file_store_addressing")?;
        let digest = physical_key_digest(
            lane,
            namespace,
            key,
            self.path_budget.physical_key_digest_hex_chars,
        );
        let shard = &digest[..2];
        let data_file_name = format!("{digest}.{extension}");
        let index_file_name = format!("{digest}.json");

        self.validate_directory_component(lane, "file_store_addressing")?;
        self.validate_directory_component(namespace, "file_store_addressing")?;
        self.validate_directory_component(FILE_ADDRESSING_DATA_DIR, "file_store_addressing")?;
        self.validate_directory_component(FILE_ADDRESSING_INDEX_DIR, "file_store_addressing")?;
        self.validate_directory_component(shard, "file_store_addressing")?;
        self.validate_file_name(&data_file_name, "file_store_addressing")?;
        self.validate_file_name(&index_file_name, "file_store_addressing")?;
        self.validate_relative_path(
            &format!("{lane}/{namespace}/{FILE_ADDRESSING_DATA_DIR}/{shard}/{data_file_name}"),
            "file_store_addressing",
        )?;
        self.validate_relative_path(
            &format!("{lane}/{namespace}/{FILE_ADDRESSING_INDEX_DIR}/{shard}/{index_file_name}"),
            "file_store_addressing",
        )?;

        Ok(PhysicalKeyPaths {
            data_path: root
                .join(lane)
                .join(namespace)
                .join(FILE_ADDRESSING_DATA_DIR)
                .join(shard)
                .join(data_file_name),
            index_path: root
                .join(lane)
                .join(namespace)
                .join(FILE_ADDRESSING_INDEX_DIR)
                .join(shard)
                .join(index_file_name),
            legacy_path: root.join(lane).join(namespace).join(format!(
                "{}.{}",
                hex_encode(key.as_bytes()),
                extension
            )),
        })
    }

    fn events_path(&self) -> PathBuf {
        self.root.join("events").join("events.jsonl")
    }

    fn json_dir(&self, namespace: &str) -> PathBuf {
        self.root.join("kv").join(namespace)
    }

    fn blob_dir(&self, namespace: &str) -> PathBuf {
        self.root.join("blob").join(namespace)
    }

    fn write_json_file(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        atomic_write(path, bytes, self.fsync, "file_store_write")
    }

    fn validate_addressing_budget(&self, extension: &str, stage: &'static str) -> Result<()> {
        let digest_chars = self.path_budget.physical_key_digest_hex_chars;
        if !(MIN_PHYSICAL_DIGEST_HEX_CHARS..=MAX_PHYSICAL_DIGEST_HEX_CHARS).contains(&digest_chars)
        {
            return Err(Error::config(
                stage,
                format!(
                    "physical key digest hex chars {digest_chars} outside {MIN_PHYSICAL_DIGEST_HEX_CHARS}..={MAX_PHYSICAL_DIGEST_HEX_CHARS}"
                ),
            ));
        }
        let max_data_file_len = digest_chars + 1 + extension.len();
        let max_index_file_len = digest_chars + 1 + "json".len();
        if max_data_file_len > self.path_budget.max_file_name_bytes
            || max_index_file_len > self.path_budget.max_file_name_bytes
        {
            return Err(Error::config(
                stage,
                format!(
                    "physical key digest length exceeds file name budget {}",
                    self.path_budget.max_file_name_bytes
                ),
            ));
        }
        Ok(())
    }

    fn validate_directory_component(&self, value: &str, stage: &'static str) -> Result<()> {
        if value.is_empty() || value.contains('/') || value.contains('\\') {
            return Err(Error::config(
                stage,
                format!("invalid file store directory component {value:?}"),
            ));
        }
        if value.len() > self.path_budget.max_directory_name_bytes {
            return Err(Error::config(
                stage,
                format!(
                    "directory component {value:?} exceeds {} bytes",
                    self.path_budget.max_directory_name_bytes
                ),
            ));
        }
        Ok(())
    }

    fn validate_file_name(&self, value: &str, stage: &'static str) -> Result<()> {
        if value.is_empty() || value.contains('/') || value.contains('\\') {
            return Err(Error::config(
                stage,
                format!("invalid file store file name {value:?}"),
            ));
        }
        if value.len() > self.path_budget.max_file_name_bytes {
            return Err(Error::config(
                stage,
                format!(
                    "file name {value:?} exceeds {} bytes",
                    self.path_budget.max_file_name_bytes
                ),
            ));
        }
        Ok(())
    }

    fn validate_relative_path(&self, value: &str, stage: &'static str) -> Result<()> {
        if value.len() > self.path_budget.max_relative_path_bytes {
            return Err(Error::config(
                stage,
                format!(
                    "relative file store path exceeds {} bytes",
                    self.path_budget.max_relative_path_bytes
                ),
            ));
        }
        Ok(())
    }

    fn write_key_index(&self, path: &Path, key: &str, stage: &'static str) -> Result<()> {
        let value = json!({
            "addressing_version": FILE_ADDRESSING_VERSION,
            "key": key,
        });
        let bytes = serde_json::to_vec_pretty(&value)
            .map_err(|error| Error::config(stage, error.to_string()))?;
        atomic_write(path, &bytes, self.fsync, stage)
    }

    fn read_key_index(&self, path: &Path, stage: &'static str) -> Result<Option<String>> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if is_not_found_or_invalid_filename(&error) => return Ok(None),
            Err(error) => return Err(Error::io(stage, error)),
        };
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| Error::config(stage, error.to_string()))?;
        let version = value
            .get("addressing_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::config(stage, "file store key index missing version"))?;
        if version != FILE_ADDRESSING_VERSION {
            return Err(Error::config(
                stage,
                format!("unsupported file store key index version {version}"),
            ));
        }
        value
            .get("key")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| Error::config(stage, "file store key index missing key"))
            .map(Some)
    }

    fn require_key_index_matches(&self, path: &Path, key: &str, stage: &'static str) -> Result<()> {
        let Some(existing) = self.read_key_index(path, stage)? else {
            return Err(Error::config(
                stage,
                "file store physical data is missing key index",
            ));
        };
        if existing != key {
            return Err(Error::config(
                stage,
                "file store physical key collision detected",
            ));
        }
        Ok(())
    }

    fn ensure_key_index_available(
        &self,
        paths: &PhysicalKeyPaths,
        key: &str,
        stage: &'static str,
    ) -> Result<()> {
        if let Some(existing) = self.read_key_index(&paths.index_path, stage)? {
            if existing != key {
                return Err(Error::config(
                    stage,
                    "file store physical key collision detected",
                ));
            }
            return Ok(());
        }
        if paths.data_path.exists() {
            return Err(Error::config(
                stage,
                "file store physical data is missing key index",
            ));
        }
        Ok(())
    }

    fn validate_v2_pair_for_delete(
        &self,
        paths: &PhysicalKeyPaths,
        key: &str,
        stage: &'static str,
    ) -> Result<()> {
        let data_exists = paths.data_path.exists();
        let index = self.read_key_index(&paths.index_path, stage)?;
        match (data_exists, index) {
            (true, Some(existing)) if existing == key => Ok(()),
            (true, Some(_)) => Err(Error::config(
                stage,
                "file store physical key collision detected",
            )),
            (true, None) => Err(Error::config(
                stage,
                "file store physical data is missing key index",
            )),
            (false, Some(_)) => Err(Error::config(
                stage,
                "file store key index has missing physical data",
            )),
            (false, None) => Ok(()),
        }
    }

    fn write_json_value_at_root(
        &self,
        root: &Path,
        namespace: &str,
        key: &str,
        value: &Value,
        stage: &'static str,
    ) -> Result<()> {
        let paths = self.json_paths_at_root(root, namespace, key)?;
        self.ensure_key_index_available(&paths, key, stage)?;
        self.write_key_index(&paths.index_path, key, stage)?;
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|error| Error::config(stage, error.to_string()))?;
        atomic_write(&paths.data_path, &bytes, self.fsync, stage)
    }

    fn write_blob_at_root(
        &self,
        root: &Path,
        namespace: &str,
        key: &str,
        value: &[u8],
        stage: &'static str,
    ) -> Result<()> {
        let paths = self.blob_paths_at_root(root, namespace, key)?;
        self.ensure_key_index_available(&paths, key, stage)?;
        self.write_key_index(&paths.index_path, key, stage)?;
        atomic_write(&paths.data_path, value, self.fsync, stage)
    }

    fn list_keys(&self, lane: &str, namespace: &str, extension: &str) -> Result<Vec<String>> {
        let base = self.root.join(lane).join(namespace);
        let mut out = BTreeSet::new();
        for key in self.list_indexed_keys(lane, namespace, &base, extension)? {
            out.insert(key);
        }
        for key in self.list_legacy_encoded_keys(base, extension)? {
            out.insert(key);
        }
        Ok(out.into_iter().collect())
    }

    fn list_indexed_keys(
        &self,
        lane: &str,
        namespace: &str,
        base: &Path,
        extension: &str,
    ) -> Result<Vec<String>> {
        let index_base = base.join(FILE_ADDRESSING_INDEX_DIR);
        let data_base = base.join(FILE_ADDRESSING_DATA_DIR);
        let shards = match fs::read_dir(&index_base) {
            Ok(shards) => shards,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Error::io("file_store_list", error)),
        };
        let mut out = Vec::new();
        for shard in shards {
            let shard = shard.map_err(|error| Error::io("file_store_list", error))?;
            let shard_path = shard.path();
            if !shard_path.is_dir() {
                continue;
            }
            let Some(shard_name) = shard_path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let entries =
                fs::read_dir(&shard_path).map_err(|error| Error::io("file_store_list", error))?;
            for entry in entries {
                let entry = entry.map_err(|error| Error::io("file_store_list", error))?;
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let data_path = data_base
                    .join(shard_name)
                    .join(format!("{stem}.{extension}"));
                if !data_path.is_file() {
                    return Err(Error::config(
                        "file_store_list",
                        "file store key index has missing physical data",
                    ));
                }
                if let Some(key) = self.read_key_index(&path, "file_store_list")? {
                    let expected = self
                        .physical_key_paths_at_root(&self.root, lane, namespace, &key, extension)?;
                    if expected.index_path != path || expected.data_path != data_path {
                        return Err(Error::config(
                            "file_store_list",
                            "file store physical key index does not match logical key",
                        ));
                    }
                    out.push(key);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn list_legacy_encoded_keys(&self, base: PathBuf, extension: &str) -> Result<Vec<String>> {
        let entries = match fs::read_dir(&base) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Error::io("file_store_list", error)),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| Error::io("file_store_list", error))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some(extension) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if let Some(decoded) = hex_decode_to_string(stem) {
                out.push(decoded);
            }
        }
        out.sort();
        Ok(out)
    }

    fn ensure_can_append_event(&self, event: &MemoryStoreEvent) -> Result<()> {
        enforce_event_key_budget(self.capacity, event, "store_event_log")?;
        let events = self.read_events_unlocked()?;
        if events
            .iter()
            .any(|existing| existing.event_id == event.event_id)
        {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        if events.len() >= self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                events.len().saturating_add(1),
                self.capacity.event_log_max_items
            )));
        }
        Ok(())
    }

    fn json_entry_count(&self) -> Result<usize> {
        let mut count = 0usize;
        for namespace in list_child_directory_names(&self.root.join("kv"), "file_store_json_quota")?
        {
            count = count.saturating_add(self.list_json_keys_unlocked(&namespace)?.len());
        }
        Ok(count)
    }

    fn blob_total_bytes(&self) -> Result<usize> {
        let mut total = 0usize;
        for namespace in
            list_child_directory_names(&self.root.join("blob"), "file_store_blob_quota")?
        {
            for key in self.list_blob_keys_unlocked(&namespace)? {
                if let Some(value) = self.get_blob_unlocked(&namespace, &key)? {
                    total = total.saturating_add(value.len());
                }
            }
        }
        Ok(total)
    }

    fn ensure_json_entry_budget(&self, namespace: &str, key: &str) -> Result<()> {
        if self.get_json_value_unlocked(namespace, key)?.is_some() {
            return Ok(());
        }
        let count = self.json_entry_count()?;
        if count >= self.capacity.kv_max_entries {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                count.saturating_add(1),
                self.capacity.kv_max_entries
            )));
        }
        Ok(())
    }

    fn ensure_blob_total_budget(&self, namespace: &str, key: &str, value_len: usize) -> Result<()> {
        let previous = self
            .get_blob_unlocked(namespace, key)?
            .map(|value| value.len())
            .unwrap_or(0);
        let next = self
            .blob_total_bytes()?
            .saturating_sub(previous)
            .saturating_add(value_len);
        if next > self.capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                next, self.capacity.blob_max_bytes
            )));
        }
        Ok(())
    }

    fn validate_snapshot_capacity(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<()> {
        if events.len() > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                events.len(),
                self.capacity.event_log_max_items
            )));
        }
        for event in events {
            enforce_event_key_budget(self.capacity, event, "file_store_snapshot_import")?;
        }
        for doc in json_docs {
            enforce_logical_key_budget(
                self.capacity,
                &doc.namespace,
                &doc.key,
                "file_store_snapshot_import",
            )?;
        }
        for blob in blobs {
            enforce_logical_key_budget(
                self.capacity,
                &blob.namespace,
                &blob.key,
                "file_store_snapshot_import",
            )?;
        }
        let json_namespace_set = namespace_set(json_namespaces);
        let retained_json_entries =
            list_child_directory_names(&self.root.join("kv"), "file_store_json_quota")?
                .into_iter()
                .filter(|namespace| !json_namespace_set.contains(namespace.as_str()))
                .try_fold(0usize, |count, namespace| {
                    self.list_json_keys_unlocked(&namespace)
                        .map(|keys| count.saturating_add(keys.len()))
                })?;
        let final_json_entries = retained_json_entries.saturating_add(json_docs.len());
        if final_json_entries > self.capacity.kv_max_entries {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                final_json_entries, self.capacity.kv_max_entries
            )));
        }
        let blob_namespace_set = namespace_set(blob_namespaces);
        let retained_blob_bytes =
            list_child_directory_names(&self.root.join("blob"), "file_store_blob_quota")?
                .into_iter()
                .filter(|namespace| !blob_namespace_set.contains(namespace.as_str()))
                .try_fold(0usize, |count, namespace| {
                    let mut namespace_bytes = 0usize;
                    for key in self.list_blob_keys_unlocked(&namespace)? {
                        if let Some(value) = self.get_blob_unlocked(&namespace, &key)? {
                            namespace_bytes = namespace_bytes.saturating_add(value.len());
                        }
                    }
                    Ok::<usize, Error>(count.saturating_add(namespace_bytes))
                })?;
        let snapshot_blob_bytes = blobs.iter().map(|blob| blob.value.len()).sum::<usize>();
        let final_blob_bytes = retained_blob_bytes.saturating_add(snapshot_blob_bytes);
        if final_blob_bytes > self.capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                final_blob_bytes, self.capacity.blob_max_bytes
            )));
        }
        Ok(())
    }

    fn append_event_unchecked(&self, event: &MemoryStoreEvent) -> Result<()> {
        let path = self.events_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| Error::io("store_event_log", error))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| Error::io("store_event_log", error))?;
        serde_json::to_writer(&mut file, event)
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        file.write_all(b"\n")
            .map_err(|error| Error::io("store_event_log", error))?;
        if self.fsync {
            file.sync_all()
                .map_err(|error| Error::io("store_event_log", error))?;
        }
        Ok(())
    }

    fn with_exclusive_backend<T>(
        &self,
        stage: &'static str,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let _lock = self.acquire_backend_lock(true, stage)?;
        self.recover_transaction_if_needed()?;
        operation()
    }

    fn with_shared_backend<T>(
        &self,
        stage: &'static str,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        loop {
            let lock = self.acquire_backend_lock(false, stage)?;
            match fs::metadata(self.transaction_marker_path()) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return operation();
                }
                Err(error) => return Err(Error::io(stage, error)),
                Ok(_) => drop(lock),
            }

            let _lock = self.acquire_backend_lock(true, stage)?;
            self.recover_transaction_if_needed()?;
        }
    }

    fn read_events_unlocked(&self) -> Result<Vec<MemoryStoreEvent>> {
        let events = read_events_jsonl(&self.events_path())?;
        if events.len() > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                events.len(),
                self.capacity.event_log_max_items
            )));
        }
        Ok(events)
    }

    fn get_json_value_unlocked(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        let paths = self.json_paths(namespace, key)?;
        match fs::read(&paths.data_path) {
            Ok(bytes) => {
                self.require_key_index_matches(&paths.index_path, key, "file_store_json_read")?;
                serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(|error| Error::config("file_store_json_read", error.to_string()))
            }
            Err(error) if is_not_found_or_invalid_filename(&error) => {
                if self
                    .read_key_index(&paths.index_path, "file_store_json_read")?
                    .is_some()
                {
                    return Err(Error::config(
                        "file_store_json_read",
                        "file store key index has missing physical data",
                    ));
                }
                match fs::read(&paths.legacy_path) {
                    Ok(bytes) => serde_json::from_slice(&bytes)
                        .map(Some)
                        .map_err(|error| Error::config("file_store_json_read", error.to_string())),
                    Err(error) if is_not_found_or_invalid_filename(&error) => Ok(None),
                    Err(error) => Err(Error::io("file_store_json_read", error)),
                }
            }
            Err(error) => Err(Error::io("file_store_json_read", error)),
        }
    }

    fn list_json_keys_unlocked(&self, namespace: &str) -> Result<Vec<String>> {
        self.validate_directory_component(namespace, "file_store_list")?;
        self.list_keys("kv", namespace, "json")
    }

    fn get_blob_unlocked(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let paths = self.blob_paths(namespace, key)?;
        match fs::read(&paths.data_path) {
            Ok(bytes) => {
                self.require_key_index_matches(&paths.index_path, key, "file_store_blob_read")?;
                Ok(Some(bytes))
            }
            Err(error) if is_not_found_or_invalid_filename(&error) => {
                if self
                    .read_key_index(&paths.index_path, "file_store_blob_read")?
                    .is_some()
                {
                    return Err(Error::config(
                        "file_store_blob_read",
                        "file store key index has missing physical data",
                    ));
                }
                match fs::read(&paths.legacy_path) {
                    Ok(bytes) => Ok(Some(bytes)),
                    Err(error) if is_not_found_or_invalid_filename(&error) => Ok(None),
                    Err(error) => Err(Error::io("file_store_blob_read", error)),
                }
            }
            Err(error) => Err(Error::io("file_store_blob_read", error)),
        }
    }

    fn list_blob_keys_unlocked(&self, namespace: &str) -> Result<Vec<String>> {
        self.validate_directory_component(namespace, "file_store_list")?;
        self.list_keys("blob", namespace, "bin")
    }

    fn append_event_unlocked(&self, event: &MemoryStoreEvent) -> Result<()> {
        self.ensure_can_append_event(event)?;
        self.append_event_unchecked(event)
    }

    fn put_json_value_unlocked(&self, namespace: &str, key: &str, value: &Value) -> Result<()> {
        let paths = self.json_paths(namespace, key)?;
        self.ensure_json_entry_budget(namespace, key)?;
        self.ensure_key_index_available(&paths, key, "file_store_json_write")?;
        self.write_key_index(&paths.index_path, key, "file_store_json_write")?;
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|error| Error::config("file_store_json_write", error.to_string()))?;
        atomic_write(
            &paths.data_path,
            &bytes,
            self.fsync,
            "file_store_json_write",
        )
    }

    fn delete_json_value_unlocked(&self, namespace: &str, key: &str) -> Result<bool> {
        let paths = self.json_paths(namespace, key)?;
        self.validate_v2_pair_for_delete(&paths, key, "file_store_json_delete")?;
        let mut deleted = false;
        deleted |= remove_file_if_exists(&paths.data_path, "file_store_json_delete")?;
        deleted |= remove_file_if_exists(&paths.index_path, "file_store_json_delete")?;
        deleted |= remove_file_if_exists(&paths.legacy_path, "file_store_json_delete")?;
        Ok(deleted)
    }

    fn put_blob_unlocked(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        let paths = self.blob_paths(namespace, key)?;
        self.ensure_blob_total_budget(namespace, key, value.len())?;
        self.ensure_key_index_available(&paths, key, "file_store_blob_write")?;
        self.write_key_index(&paths.index_path, key, "file_store_blob_write")?;
        atomic_write(&paths.data_path, value, self.fsync, "file_store_blob_write")
    }

    fn delete_blob_unlocked(&self, namespace: &str, key: &str) -> Result<bool> {
        let paths = self.blob_paths(namespace, key)?;
        self.validate_v2_pair_for_delete(&paths, key, "file_store_blob_delete")?;
        let mut deleted = false;
        deleted |= remove_file_if_exists(&paths.data_path, "file_store_blob_delete")?;
        deleted |= remove_file_if_exists(&paths.index_path, "file_store_blob_delete")?;
        deleted |= remove_file_if_exists(&paths.legacy_path, "file_store_blob_delete")?;
        Ok(deleted)
    }

    fn replace_snapshot_unlocked(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport> {
        self.validate_snapshot_capacity(
            json_namespaces,
            blob_namespaces,
            json_docs,
            blobs,
            events,
        )?;
        let import_id = snapshot_import_id();
        let stage_root = self.root.join(format!(".snapshot-import-{import_id}"));
        let backup_root = self.root.join(format!(".snapshot-backup-{import_id}"));
        if stage_root.exists() || backup_root.exists() {
            return Err(Error::config(
                "file_store_snapshot_import",
                "snapshot staging directory already exists",
            ));
        }

        let prepare_result = self.prepare_snapshot_stage(&stage_root, json_docs, blobs, events);
        if let Err(error) = prepare_result {
            let _ = fs::remove_dir_all(&stage_root);
            return Err(error);
        }

        let json_deleted = count_deleted_json_keys(self, json_namespaces, json_docs)?;
        let blobs_deleted = count_deleted_blob_keys(self, blob_namespaces, blobs)?;

        let apply_result =
            self.apply_snapshot_stage(&stage_root, &backup_root, json_namespaces, blob_namespaces);
        if let Err(error) = apply_result {
            let _ = self.rollback_snapshot_stage(&backup_root, json_namespaces, blob_namespaces);
            let _ = fs::remove_dir_all(&stage_root);
            let _ = fs::remove_dir_all(&backup_root);
            return Err(error);
        }
        let _ = fs::remove_dir_all(&stage_root);
        let _ = fs::remove_dir_all(&backup_root);

        Ok(StoreSnapshotReplaceReport {
            json_deleted,
            blobs_deleted,
            events_imported: events.len(),
        })
    }
}

impl StoreEventLog for FileStoreEngine {
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        self.with_exclusive_backend("store_event_log", || self.append_event_unlocked(&event))
    }

    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        self.with_shared_backend("store_event_log", || self.read_events_unlocked())
    }
}

pub(crate) fn read_events_from_root(root: &Path) -> Result<Vec<MemoryStoreEvent>> {
    read_events_jsonl(&root.join("events").join("events.jsonl"))
}

fn read_events_jsonl(path: &Path) -> Result<Vec<MemoryStoreEvent>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io("store_event_log", error)),
    };
    let mut events = Vec::new();
    for raw in bytes.split(|byte| *byte == b'\n') {
        if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        events.push(
            serde_json::from_slice(raw)
                .map_err(|error| Error::config("store_event_log", error.to_string()))?,
        );
    }
    Ok(events)
}

impl StoreEngine for FileStoreEngine {
    fn commit_transaction(
        &self,
        request: &StoreTransactionRequest,
    ) -> Result<StoreTransactionReport> {
        let _lock = self.acquire_backend_lock(true, "memory_write_transaction")?;
        self.recover_transaction_if_needed()?;
        let current = self.load_transaction_state()?;
        let (next, report) = apply_transaction(
            self.capacity,
            request,
            &current,
            EventOverflowPolicy::Reject,
        )?;

        let mut json_namespaces = current
            .json
            .keys()
            .map(|(namespace, _)| namespace.clone())
            .chain(next.json.keys().map(|(namespace, _)| namespace.clone()))
            .collect::<BTreeSet<_>>();
        let mut blob_namespaces = current
            .blobs
            .keys()
            .map(|(namespace, _)| namespace.clone())
            .chain(next.blobs.keys().map(|(namespace, _)| namespace.clone()))
            .collect::<BTreeSet<_>>();
        for mutation in &request.mutations {
            match mutation {
                StoreEngineMutation::PutJson { namespace, .. }
                | StoreEngineMutation::DeleteJson { namespace, .. } => {
                    json_namespaces.insert(namespace.clone());
                }
                StoreEngineMutation::PutBlob { namespace, .. }
                | StoreEngineMutation::DeleteBlob { namespace, .. } => {
                    blob_namespaces.insert(namespace.clone());
                }
                StoreEngineMutation::AppendEvent { .. } => {}
            }
        }
        let before = Self::transaction_image(&current, &json_namespaces, &blob_namespaces);
        let after = Self::transaction_image(&next, &json_namespaces, &blob_namespaces);
        let mut journal =
            FileTransactionJournal::new(request.transaction_id.clone(), before, after)?;
        self.write_transaction_journal(&journal)?;
        self.maybe_crash_for_recovery_contract("after_prepare_before_apply");
        self.maybe_pause_for_recovery_contract("after_prepare_before_apply");

        self.restore_transaction_image(&journal.after)?;
        self.maybe_crash_for_recovery_contract("after_apply_before_commit");
        self.maybe_pause_for_recovery_contract("after_apply_before_commit");

        journal.state = FileTransactionJournalState::Committed;
        journal.refresh_checksum()?;
        self.write_transaction_journal(&journal)?;
        self.maybe_crash_for_recovery_contract("after_commit_before_cleanup");

        self.remove_transaction_journal("file_store_transaction")?;
        Ok(report)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn read_consistent(
        &self,
        request: &StoreConsistentReadRequest,
    ) -> Result<StoreConsistentReadResult> {
        self.with_shared_backend("store_consistent_read", || {
            let mut state = BackendTransactionState::default();
            for address in &request.json {
                if let Some(value) =
                    self.get_json_value_unlocked(&address.namespace, &address.key)?
                {
                    state
                        .json
                        .insert((address.namespace.clone(), address.key.clone()), value);
                }
            }
            for address in &request.blobs {
                if let Some(value) = self.get_blob_unlocked(&address.namespace, &address.key)? {
                    state
                        .blobs
                        .insert((address.namespace.clone(), address.key.clone()), value);
                }
            }
            if request.include_events {
                state.events = self.read_events_unlocked()?;
            }
            Ok(read_consistent_from_state(request, &state))
        })
    }

    fn read_consistent_namespaces(
        &self,
        request: &StoreConsistentNamespaceReadRequest,
    ) -> Result<StoreConsistentNamespaceReadResult> {
        self.with_shared_backend("store_consistent_read", || {
            let mut state = BackendTransactionState::default();
            for namespace in &request.json_namespaces {
                for key in self.list_json_keys_unlocked(namespace)? {
                    if let Some(value) = self.get_json_value_unlocked(namespace, &key)? {
                        state.json.insert((namespace.clone(), key), value);
                    }
                }
            }
            for namespace in &request.blob_namespaces {
                for key in self.list_blob_keys_unlocked(namespace)? {
                    if let Some(value) = self.get_blob_unlocked(namespace, &key)? {
                        state.blobs.insert((namespace.clone(), key), value);
                    }
                }
            }
            if request.include_events {
                state.events = self.read_events_unlocked()?;
            }
            read_consistent_namespaces_from_state(request, &state)
        })
    }

    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        self.with_shared_backend("file_store_json_read", || {
            self.get_json_value_unlocked(namespace, key)
        })
    }

    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        self.with_exclusive_backend("file_store_json_write", || {
            self.put_json_value_unlocked(namespace, key, &value)
        })
    }

    fn put_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: Value,
        event: MemoryStoreEvent,
    ) -> Result<()> {
        self.with_exclusive_backend("file_store_json_write", || {
            self.ensure_can_append_event(&event)?;
            self.put_json_value_unlocked(namespace, key, &value)?;
            self.append_event_unchecked(&event)
        })
    }

    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        self.with_exclusive_backend("file_store_json_delete", || {
            self.delete_json_value_unlocked(namespace, key)
        })
    }

    fn delete_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        event: MemoryStoreEvent,
    ) -> Result<bool> {
        self.with_exclusive_backend("file_store_json_delete", || {
            self.ensure_can_append_event(&event)?;
            let deleted = self.delete_json_value_unlocked(namespace, key)?;
            if deleted {
                self.append_event_unchecked(&event)?;
            }
            Ok(deleted)
        })
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        self.with_shared_backend("file_store_list", || {
            self.list_json_keys_unlocked(namespace)
        })
    }

    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        self.with_shared_backend("file_store_blob_read", || {
            self.get_blob_unlocked(namespace, key)
        })
    }

    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        self.with_exclusive_backend("file_store_blob_write", || {
            self.put_blob_unlocked(namespace, key, value)
        })
    }

    fn put_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: &[u8],
        event: MemoryStoreEvent,
    ) -> Result<()> {
        self.with_exclusive_backend("file_store_blob_write", || {
            self.ensure_can_append_event(&event)?;
            self.put_blob_unlocked(namespace, key, value)?;
            self.append_event_unchecked(&event)
        })
    }

    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        self.with_exclusive_backend("file_store_blob_delete", || {
            self.delete_blob_unlocked(namespace, key)
        })
    }

    fn delete_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        event: MemoryStoreEvent,
    ) -> Result<bool> {
        self.with_exclusive_backend("file_store_blob_delete", || {
            self.ensure_can_append_event(&event)?;
            let deleted = self.delete_blob_unlocked(namespace, key)?;
            if deleted {
                self.append_event_unchecked(&event)?;
            }
            Ok(deleted)
        })
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        self.with_shared_backend("file_store_list", || {
            self.list_blob_keys_unlocked(namespace)
        })
    }

    fn replace_snapshot(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport> {
        self.with_exclusive_backend("file_store_snapshot_import", || {
            self.replace_snapshot_unlocked(
                json_namespaces,
                blob_namespaces,
                json_docs,
                blobs,
                events,
            )
        })
    }
}

impl FileStoreEngine {
    fn prepare_snapshot_stage(
        &self,
        stage_root: &Path,
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<()> {
        fs::create_dir_all(stage_root.join("events"))
            .map_err(|error| Error::io("file_store_snapshot_import", error))?;
        let mut event_ids = BTreeSet::new();
        let mut event_bytes = Vec::new();
        for event in events {
            if !event_ids.insert(event.event_id.clone()) {
                return Err(Error::config(
                    "store_event_log",
                    format!("duplicate event id {}", event.event_id),
                ));
            }
            serde_json::to_writer(&mut event_bytes, event)
                .map_err(|error| Error::config("store_event_log", error.to_string()))?;
            event_bytes.push(b'\n');
        }
        atomic_write(
            &stage_root.join("events").join("events.jsonl"),
            &event_bytes,
            self.fsync,
            "file_store_snapshot_import",
        )?;
        for doc in json_docs {
            self.write_json_value_at_root(
                stage_root,
                &doc.namespace,
                &doc.key,
                &doc.value,
                "file_store_snapshot_import",
            )?;
        }
        for blob in blobs {
            self.write_blob_at_root(
                stage_root,
                &blob.namespace,
                &blob.key,
                &blob.value,
                "file_store_snapshot_import",
            )?;
        }
        Ok(())
    }

    fn apply_snapshot_stage(
        &self,
        stage_root: &Path,
        backup_root: &Path,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
    ) -> Result<()> {
        fs::create_dir_all(backup_root)
            .map_err(|error| Error::io("file_store_snapshot_import", error))?;
        fs::create_dir_all(self.root.join("events"))
            .map_err(|error| Error::io("file_store_snapshot_import", error))?;
        move_path_if_exists(
            &self.events_path(),
            &backup_root.join("events").join("events.jsonl"),
            self.fsync,
            "file_store_snapshot_import",
        )?;
        for namespace in json_namespaces {
            move_path_if_exists(
                &self.json_dir(namespace),
                &backup_root.join("kv").join(namespace),
                self.fsync,
                "file_store_snapshot_import",
            )?;
        }
        for namespace in blob_namespaces {
            move_path_if_exists(
                &self.blob_dir(namespace),
                &backup_root.join("blob").join(namespace),
                self.fsync,
                "file_store_snapshot_import",
            )?;
        }

        move_path_if_exists(
            &stage_root.join("events").join("events.jsonl"),
            &self.events_path(),
            self.fsync,
            "file_store_snapshot_import",
        )?;
        for namespace in json_namespaces {
            move_path_if_exists(
                &stage_root.join("kv").join(namespace),
                &self.json_dir(namespace),
                self.fsync,
                "file_store_snapshot_import",
            )?;
        }
        for namespace in blob_namespaces {
            move_path_if_exists(
                &stage_root.join("blob").join(namespace),
                &self.blob_dir(namespace),
                self.fsync,
                "file_store_snapshot_import",
            )?;
        }
        Ok(())
    }

    fn rollback_snapshot_stage(
        &self,
        backup_root: &Path,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
    ) -> Result<()> {
        remove_path_if_exists(&self.events_path(), "file_store_snapshot_rollback")?;
        move_path_if_exists(
            &backup_root.join("events").join("events.jsonl"),
            &self.events_path(),
            self.fsync,
            "file_store_snapshot_rollback",
        )?;
        for namespace in json_namespaces {
            remove_path_if_exists(&self.json_dir(namespace), "file_store_snapshot_rollback")?;
            move_path_if_exists(
                &backup_root.join("kv").join(namespace),
                &self.json_dir(namespace),
                self.fsync,
                "file_store_snapshot_rollback",
            )?;
        }
        for namespace in blob_namespaces {
            remove_path_if_exists(&self.blob_dir(namespace), "file_store_snapshot_rollback")?;
            move_path_if_exists(
                &backup_root.join("blob").join(namespace),
                &self.blob_dir(namespace),
                self.fsync,
                "file_store_snapshot_rollback",
            )?;
        }
        Ok(())
    }
}

fn canonical_root_gate(root: &Path) -> Result<Arc<Mutex<()>>> {
    let gates = CANONICAL_ROOT_GATES.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut gates = gates.lock().map_err(|_| {
        Error::config(
            "store_transaction_lock_failed",
            "canonical file-store root registry is poisoned",
        )
    })?;
    gates.retain(|_, gate| gate.strong_count() > 0);
    if let Some(gate) = gates.get(root).and_then(Weak::upgrade) {
        return Ok(gate);
    }
    let gate = Arc::new(Mutex::new(()));
    gates.insert(root.to_path_buf(), Arc::downgrade(&gate));
    Ok(gate)
}

fn has_complete_journal_shape(bytes: &[u8]) -> bool {
    let Ok(Value::Object(journal)) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    [
        "schema_version",
        "transaction_id",
        "state",
        "before",
        "after",
        "checksum",
    ]
    .into_iter()
    .all(|field| journal.contains_key(field))
}

fn collect_tmp_files(root: &Path, findings: &mut Vec<String>) -> Result<()> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(|error| Error::io("file_store_repair", error))?;
        let path = entry.path();
        if path.is_dir() {
            collect_tmp_files(&path, findings)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("tmp") {
            findings.push(path.to_string_lossy().to_string());
        }
    }
    findings.sort();
    Ok(())
}

fn list_child_directory_names(root: &Path, stage: &'static str) -> Result<Vec<String>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io(stage, error)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Error::io(stage, error))?;
        if !entry
            .file_type()
            .map_err(|error| Error::io(stage, error))?
            .is_dir()
        {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

fn namespace_set(namespaces: &[&str]) -> BTreeSet<String> {
    namespaces
        .iter()
        .map(|namespace| (*namespace).to_string())
        .collect()
}

fn count_deleted_json_keys(
    engine: &FileStoreEngine,
    namespaces: &[&str],
    docs: &[StoreSnapshotJsonDoc],
) -> Result<usize> {
    let snapshot_keys = docs
        .iter()
        .map(|doc| (doc.namespace.clone(), doc.key.clone()))
        .collect::<BTreeSet<_>>();
    let mut deleted = 0usize;
    for namespace in namespaces {
        for key in engine.list_json_keys_unlocked(namespace)? {
            if !snapshot_keys.contains(&((*namespace).to_string(), key)) {
                deleted = deleted.saturating_add(1);
            }
        }
    }
    Ok(deleted)
}

fn count_deleted_blob_keys(
    engine: &FileStoreEngine,
    namespaces: &[&str],
    blobs: &[StoreSnapshotBlob],
) -> Result<usize> {
    let snapshot_keys = blobs
        .iter()
        .map(|blob| (blob.namespace.clone(), blob.key.clone()))
        .collect::<BTreeSet<_>>();
    let mut deleted = 0usize;
    for namespace in namespaces {
        for key in engine.list_blob_keys_unlocked(namespace)? {
            if !snapshot_keys.contains(&((*namespace).to_string(), key)) {
                deleted = deleted.saturating_add(1);
            }
        }
    }
    Ok(deleted)
}

fn move_path_if_exists(from: &Path, to: &Path, fsync: bool, stage: &'static str) -> Result<bool> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::io(stage, error))?;
    }
    match fs::rename(from, to) {
        Ok(()) => {
            complete_rename_durability(from, to, fsync, stage)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::io(stage, error)),
    }
}

fn remove_path_if_exists(path: &Path, stage: &'static str) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(path).map_err(|error| Error::io(stage, error))
        }
        Ok(_) => fs::remove_file(path).map_err(|error| Error::io(stage, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(stage, error)),
    }
}

fn remove_file_if_exists(path: &Path, stage: &'static str) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if is_not_found_or_invalid_filename(&error) => Ok(false),
        Err(error) => Err(Error::io(stage, error)),
    }
}

fn is_not_found_or_invalid_filename(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidFilename
    )
}

fn atomic_write(path: &Path, bytes: &[u8], fsync: bool, stage: &'static str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::io(stage, error))?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp).map_err(|error| Error::io(stage, error))?;
        file.write_all(bytes)
            .map_err(|error| Error::io(stage, error))?;
        if fsync {
            file.sync_all().map_err(|error| Error::io(stage, error))?;
        }
    }
    fs::rename(&tmp, path).map_err(|error| Error::io(stage, error))?;
    complete_rename_durability(&tmp, path, fsync, stage)
}

fn complete_rename_durability(
    from: &Path,
    to: &Path,
    fsync: bool,
    stage: &'static str,
) -> Result<()> {
    let id = DURABILITY_TRACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let parents = [from.parent(), to.parent()]
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    let parent_count = if fsync { parents.len() } else { 0 };
    durability_trace(&format!("rename_begin|{id}|{parent_count}"));
    if fsync {
        for parent in parents {
            sync_directory(parent, true, stage)?;
            durability_trace(&format!("parent_sync|{id}|{}", parent.display()));
        }
    }
    durability_trace(&format!("rename_durable|{id}"));
    Ok(())
}

fn sync_directory(path: &Path, fsync: bool, stage: &'static str) -> Result<()> {
    if fsync {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| Error::io(stage, error))?;
    }
    Ok(())
}

fn durability_trace(_record: &str) {
    #[cfg(feature = "nonproduction-replay-harness")]
    if let Some(path) = std::env::var_os("BM_FILE_TRANSACTION_DURABILITY_TRACE") {
        if let Ok(mut trace) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(trace, "{_record}");
        }
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn snapshot_import_id() -> String {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = SNAPSHOT_IMPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let thread_id = format!("{:?}", std::thread::current().id())
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    format!(
        "{}-{}-{}-{}",
        now_nanos,
        std::process::id(),
        thread_id,
        sequence
    )
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn hex_decode_to_string(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for chunk in value.as_bytes().chunks_exact(2) {
        let hex = std::str::from_utf8(chunk).ok()?;
        bytes.push(u8::from_str_radix(hex, 16).ok()?);
    }
    String::from_utf8(bytes).ok()
}

fn physical_key_digest(lane: &str, namespace: &str, key: &str, hex_chars: usize) -> String {
    let first = digest_parts(
        0xcbf2_9ce4_8422_2325,
        &[
            b"bm-store-file-v2".as_slice(),
            lane.as_bytes(),
            namespace.as_bytes(),
            key.as_bytes(),
        ],
    );
    let second = digest_parts(
        0x8422_2325_cbf2_9ce4,
        &[
            b"bm-store-file-v2-key".as_slice(),
            key.as_bytes(),
            namespace.as_bytes(),
            lane.as_bytes(),
        ],
    );
    let full = format!("{first:016x}{second:016x}");
    full[..hex_chars].to_string()
}

fn digest_parts(seed: u64, parts: &[&[u8]]) -> u64 {
    let mut hash = seed;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
