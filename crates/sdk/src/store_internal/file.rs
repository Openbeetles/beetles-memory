use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
#[cfg(feature = "nonproduction-replay-harness")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(feature = "nonproduction-replay-harness")]
use bm_core::memory::{MEMORY_MUTATION_AUDIT_NAMESPACE, MEMORY_MUTATION_RECEIPT_NAMESPACE};
use bm_core::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::enforce_event_key_budget;
use crate::store_internal::snapshot::StoreSnapshotBlob;
#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::transaction::{
    read_consistent_from_state, validate_restore_post_image_blob_bytes,
};
use crate::{
    enforce_logical_key_budget, store_budget_error,
    store_internal::platform::StoreOpenPreflight,
    store_internal::schema::{
        admit_store_json_address, admit_store_json_document, classify_store_blob_address,
        StoreAddressAdmission,
    },
    store_internal::transaction::{
        apply_transaction, read_bounded_known_keys_from_parts, read_scoped_projection_from_parts,
        scoped_projection_dependency_addresses, scoped_projection_root_addresses,
        validate_immutable_read_session_capacity, validate_scoped_projection_post_image,
        BackendTransactionState, StoreAdmissionAuthority, StoreBackendUsage,
        StoreBoundedKnownBlobRead, StoreBoundedKnownJsonRead, StoreBoundedKnownKeyReadResult,
        StoreImmutableReadSession, StoreReadReceipt, StoreReadSessionState,
        StoreTransactionAdmission, StoreTransactionContext,
    },
    MemoryStoreEvent, StoreBackendConfig, StoreCapacityBudget, StoreEngine, StoreEngineMutation,
    StoreEventLog, StoreMetricEventSourceRead, StorePathBudget, StoreRepairPolicy,
    StoreRepairReport, StoreSchemaManifest, StoreSnapshot, StoreSnapshotJsonDoc,
    StoreTransactionReport, StoreTransactionRequest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreConsistentReadRequest, StoreConsistentReadResult, StoreSnapshotReplaceReport};

const FILE_ADDRESSING_VERSION: u64 = 2;
const FILE_ADDRESSING_DATA_DIR: &str = "_v2";
const FILE_ADDRESSING_INDEX_DIR: &str = "_keys";
const MIN_PHYSICAL_DIGEST_HEX_CHARS: usize = 16;
const MAX_PHYSICAL_DIGEST_HEX_CHARS: usize = 32;
const TRANSACTION_LOCK_FILE: &str = ".beetle-memory.lock";
const TRANSACTION_MARKER_FILE: &str = ".beetle-memory.transaction";
const TRANSACTION_REPAIR_REQUIRED_STAGE: &str = "memory_write_transaction_repair_required";
#[cfg(feature = "nonproduction-replay-harness")]
static SNAPSHOT_IMPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static DURABILITY_TRACE_SEQUENCE: Mutex<u64> = Mutex::new(1);
static CANONICAL_ROOT_GATES: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

pub struct FileStoreEngine {
    root: PathBuf,
    local_root_gate: Arc<Mutex<()>>,
    fsync: bool,
    capacity: StoreCapacityBudget,
    path_budget: StorePathBudget,
    lock_timeout: std::time::Duration,
    admission_authority: StoreAdmissionAuthority,
}

struct FileBackendLock<'a> {
    _advisory_lock: File,
    _local_root_gate: MutexGuard<'a, ()>,
}

struct FileImmutableReadSession<'a> {
    engine: &'a FileStoreEngine,
    _lock: FileBackendLock<'a>,
    read: StoreReadSessionState,
}

struct ScopedJsonRead {
    documents: BTreeMap<(String, String), Value>,
    logical_bytes: usize,
}

impl StoreImmutableReadSession for FileImmutableReadSession<'_> {
    fn read_json_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownJsonRead>> {
        let mut reads = Vec::with_capacity(addresses.len());
        for (namespace, key) in addresses {
            let value = match self.engine.get_json_value_unlocked_bounded(
                namespace,
                key,
                self.read.remaining_json_bytes(),
            ) {
                Ok(value) => value.map(|(value, _)| value),
                Err(error) => return self.read.fail(error),
            };
            reads.push(self.read.record_json(namespace, key, value)?);
        }
        Ok(reads)
    }

    fn read_blob_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownBlobRead>> {
        let mut reads = Vec::with_capacity(addresses.len());
        for (namespace, key) in addresses {
            let value = match self.engine.get_blob_unlocked_bounded(
                namespace,
                key,
                self.read.remaining_blob_bytes(),
            ) {
                Ok(value) => value.map(|(value, _)| value),
                Err(error) => return self.read.fail(error),
            };
            reads.push(self.read.record_blob(namespace, key, value)?);
        }
        Ok(reads)
    }

    fn receipt(&self) -> Result<StoreReadReceipt> {
        self.read.receipt()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
enum FileTransactionJournalState {
    Prepared,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileTransactionImage {
    json: Vec<FileTransactionJsonValue>,
    blobs: Vec<FileTransactionBlobValue>,
    events: FileTransactionEventsImage,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum FileTransactionEventsImage {
    Append {
        prefix_len: u64,
        events: Vec<MemoryStoreEvent>,
    },
    Replace {
        events: Vec<MemoryStoreEvent>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileTransactionJsonValue {
    namespace: String,
    key: String,
    value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileTransactionBlobValue {
    namespace: String,
    key: String,
    value: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileTransactionJournal {
    schema_version: u64,
    transaction_id: String,
    state: FileTransactionJournalState,
    before: FileTransactionImage,
    after: FileTransactionImage,
    checksum: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileKeyIndex {
    addressing_version: u64,
    key: String,
}

impl FileTransactionJournal {
    fn new(
        transaction_id: String,
        before: FileTransactionImage,
        after: FileTransactionImage,
    ) -> Result<Self> {
        let mut journal = Self {
            schema_version: 2,
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

    fn validate_image_address_contract(&self) -> Result<()> {
        fn json_addresses(image: &FileTransactionImage) -> Result<BTreeSet<(&str, &str)>> {
            let addresses = image
                .json
                .iter()
                .map(|entry| (entry.namespace.as_str(), entry.key.as_str()))
                .collect::<BTreeSet<_>>();
            if addresses.len() != image.json.len() {
                return Err(Error::config(
                    TRANSACTION_REPAIR_REQUIRED_STAGE,
                    "file transaction journal contains duplicate JSON addresses",
                ));
            }
            Ok(addresses)
        }
        fn blob_addresses(image: &FileTransactionImage) -> Result<BTreeSet<(&str, &str)>> {
            let addresses = image
                .blobs
                .iter()
                .map(|entry| (entry.namespace.as_str(), entry.key.as_str()))
                .collect::<BTreeSet<_>>();
            if addresses.len() != image.blobs.len() {
                return Err(Error::config(
                    TRANSACTION_REPAIR_REQUIRED_STAGE,
                    "file transaction journal contains duplicate blob addresses",
                ));
            }
            Ok(addresses)
        }

        if self.transaction_id.trim().is_empty()
            || json_addresses(&self.before)? != json_addresses(&self.after)?
            || blob_addresses(&self.before)? != blob_addresses(&self.after)?
        {
            return Err(Error::config(
                TRANSACTION_REPAIR_REQUIRED_STAGE,
                "file transaction journal before/after address closure mismatch",
            ));
        }
        match (&self.before.events, &self.after.events) {
            (
                FileTransactionEventsImage::Append {
                    prefix_len: before, ..
                },
                FileTransactionEventsImage::Append {
                    prefix_len: after, ..
                },
            ) if before == after => Ok(()),
            (
                FileTransactionEventsImage::Replace { .. },
                FileTransactionEventsImage::Replace { .. },
            ) => Ok(()),
            _ => Err(Error::config(
                TRANSACTION_REPAIR_REQUIRED_STAGE,
                "file transaction journal event image contract mismatch",
            )),
        }
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
    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    pub fn open_with_capacity(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
    ) -> Result<(Self, StoreRepairReport, StoreSchemaManifest)> {
        let open_preflight = StoreOpenPreflight::for_nonproduction_harness(config, capacity)?;
        Self::open_internal(
            config,
            capacity,
            StoreAdmissionAuthority::new(),
            Some(&open_preflight),
        )
    }

    pub(crate) fn open_with_capacity_and_authority(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
        admission_authority: StoreAdmissionAuthority,
        open_preflight: &StoreOpenPreflight,
    ) -> Result<(Self, StoreRepairReport, StoreSchemaManifest)> {
        Self::open_internal(config, capacity, admission_authority, Some(open_preflight))
    }

    fn open_internal(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
        admission_authority: StoreAdmissionAuthority,
        open_preflight: Option<&StoreOpenPreflight>,
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
            capacity,
            path_budget: config.path_budget,
            lock_timeout: config.lock_timeout,
            admission_authority,
        };
        let existing_manifest = engine.validate_existing_manifest_read_only(config)?;
        if let Some(manifest) = existing_manifest.as_ref() {
            engine.run_open_preflight(manifest, open_preflight)?;
        }
        engine.ensure_missing_manifest_store_is_empty()?;
        let (repair, manifest) = {
            let _lock = if existing_manifest.is_some() {
                engine.acquire_existing_backend_lock(true, "file_store_open")?
            } else {
                engine.acquire_backend_lock(true, "file_store_open")?
            };
            engine.ensure_missing_manifest_store_is_empty()?;
            let recovery_plan =
                if let Some(manifest) = engine.validate_existing_manifest_read_only(config)? {
                    engine.run_open_preflight(&manifest, open_preflight)?
                } else {
                    None
                };
            engine.recover_validated_transaction(recovery_plan)?;
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
        self.acquire_backend_lock_internal(exclusive, stage, true)
    }

    fn acquire_existing_backend_lock(
        &self,
        exclusive: bool,
        stage: &'static str,
    ) -> Result<FileBackendLock<'_>> {
        self.acquire_backend_lock_internal(exclusive, stage, false)
    }

    fn acquire_backend_lock_internal(
        &self,
        exclusive: bool,
        stage: &'static str,
        create_if_missing: bool,
    ) -> Result<FileBackendLock<'_>> {
        let local_root_gate = self.local_root_gate.lock().map_err(|_| {
            Error::config(
                "store_transaction_lock_failed",
                "canonical file-store root lock is poisoned",
            )
        })?;
        let lock = OpenOptions::new()
            .create(create_if_missing)
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

    fn ensure_missing_manifest_store_is_empty(&self) -> Result<()> {
        let manifest_path = self.root.join("manifest.json");
        if manifest_path
            .try_exists()
            .map_err(|error| Error::io("file_store_manifest", error))?
        {
            return Ok(());
        }

        let transaction_exists = self
            .transaction_marker_path()
            .try_exists()
            .map_err(|error| Error::io("file_store_manifest", error))?;
        let persistent_state_exists = transaction_exists
            || ["kv", "blob", "events", "snapshots"].into_iter().try_fold(
                false,
                |found, lane| {
                    if found {
                        Ok(true)
                    } else {
                        contains_persistent_file(&self.root.join(lane))
                    }
                },
            )?;
        if persistent_state_exists {
            return Err(Error::config(
                "file_store_manifest",
                "manifest is missing for a non-empty file store",
            ));
        }
        Ok(())
    }

    fn validate_existing_manifest_read_only(
        &self,
        config: &StoreBackendConfig,
    ) -> Result<Option<StoreSchemaManifest>> {
        let path = self.root.join("manifest.json");
        let Some(bytes) = read_file_bounded(
            &path,
            self.capacity
                .snapshot_max_bytes
                .min(self.capacity.import_max_bytes),
            "file_store_manifest",
        )?
        else {
            return Ok(None);
        };
        let manifest: StoreSchemaManifest = serde_json::from_slice(&bytes)
            .map_err(|error| Error::config("file_store_manifest", error.to_string()))?;
        manifest.validate_against(
            config.backend,
            config.profile,
            config.memory_system_kind,
            "file_store_manifest",
        )?;
        Ok(Some(manifest))
    }

    fn run_open_preflight(
        &self,
        manifest: &StoreSchemaManifest,
        open_preflight: Option<&StoreOpenPreflight>,
    ) -> Result<Option<FileTransactionJournal>> {
        let Some(open_preflight) = open_preflight else {
            return self.read_transaction_journal_read_only();
        };
        let remap = |error: Error| Error::config("file_store_open_preflight", error.to_string());
        let journal = self.read_transaction_journal_read_only()?;
        let snapshot = self
            .read_store_snapshot_for_open_preflight(manifest, journal.as_ref())
            .map_err(remap)?;
        let Some(journal) = journal else {
            open_preflight.admit_snapshot(&snapshot, "file_store_open_preflight")?;
            return Ok(None);
        };
        let before = self
            .overlay_transaction_image_for_open_preflight(snapshot.clone(), &journal.before)
            .map_err(remap)?;
        open_preflight.admit_snapshot(&before, "file_store_open_preflight")?;
        let after = self
            .overlay_transaction_image_for_open_preflight(snapshot, &journal.after)
            .map_err(remap)?;
        open_preflight.admit_snapshot(&after, "file_store_open_preflight")?;
        Ok(Some(journal))
    }

    fn read_store_snapshot_for_open_preflight(
        &self,
        manifest: &StoreSchemaManifest,
        journal: Option<&FileTransactionJournal>,
    ) -> Result<StoreSnapshot> {
        self.validate_open_preflight_physical_footprint(journal)?;
        let physical_entry_limit = self
            .capacity
            .kv_max_entries
            .saturating_add(self.journal_address_count(journal, None, None))
            .saturating_add(1);
        let mut json_docs = Vec::new();
        let mut json_bytes = 0_usize;
        for namespace in list_child_directory_names_bounded(
            &self.root.join("kv"),
            physical_entry_limit,
            "file_store_open_preflight",
        )? {
            for key in self.open_preflight_address_keys("kv", &namespace, "json", journal)? {
                if json_docs.len().saturating_add(1) > physical_entry_limit {
                    return Err(store_budget_error(format!(
                        "file open preflight physical JSON entries exceed {physical_entry_limit}"
                    )));
                }
                let remaining = self.capacity.snapshot_max_bytes.saturating_sub(json_bytes);
                let paths = self.json_paths(&namespace, &key)?;
                let Some(bytes) =
                    read_file_bounded(&paths.data_path, remaining, "file_store_open_preflight")?
                else {
                    continue;
                };
                let value = serde_json::from_slice::<Value>(&bytes).map_err(|error| {
                    Error::config("file_store_open_preflight", error.to_string())
                })?;
                admit_store_json_document(&namespace, &key, &value, "file_store_open_preflight")?;
                json_bytes = json_bytes.checked_add(bytes.len()).ok_or_else(|| {
                    store_budget_error("file open preflight JSON byte count overflow")
                })?;
                json_docs.push(StoreSnapshotJsonDoc {
                    namespace: namespace.clone(),
                    key,
                    value,
                });
            }
        }
        let mut blobs = Vec::new();
        let mut blob_bytes = 0_usize;
        for namespace in list_child_directory_names_bounded(
            &self.root.join("blob"),
            physical_entry_limit,
            "file_store_open_preflight",
        )? {
            for key in self.open_preflight_address_keys("blob", &namespace, "bin", journal)? {
                if json_docs
                    .len()
                    .saturating_add(blobs.len())
                    .saturating_add(1)
                    > physical_entry_limit
                {
                    return Err(store_budget_error(format!(
                        "file open preflight physical entries exceed {physical_entry_limit}"
                    )));
                }
                let remaining = self.capacity.blob_max_bytes.saturating_sub(blob_bytes);
                let paths = self.blob_paths(&namespace, &key)?;
                let Some(value) =
                    read_file_bounded(&paths.data_path, remaining, "file_store_open_preflight")?
                else {
                    continue;
                };
                classify_store_blob_address(&namespace, &key, Some(&value))?;
                blob_bytes = blob_bytes.checked_add(value.len()).ok_or_else(|| {
                    store_budget_error("file open preflight blob byte count overflow")
                })?;
                blobs.push(StoreSnapshotBlob {
                    namespace: namespace.clone(),
                    key,
                    value,
                });
            }
        }
        let events = match journal.map(|journal| &journal.before.events) {
            Some(FileTransactionEventsImage::Append { prefix_len, .. }) => {
                read_events_jsonl_prefix_bounded(
                    &self.events_path(),
                    *prefix_len,
                    self.capacity,
                    self.capacity.snapshot_max_bytes.saturating_sub(json_bytes),
                    "file_store_open_preflight",
                )?
            }
            _ => self.read_events_unlocked_bounded(self.capacity, json_bytes)?,
        };
        Ok(StoreSnapshot::new(
            manifest.clone(),
            json_docs,
            blobs,
            events,
        ))
    }

    fn validate_open_preflight_physical_footprint(
        &self,
        journal: Option<&FileTransactionJournal>,
    ) -> Result<()> {
        const ROOT_FILES: &[&str] = &[
            "manifest.json",
            TRANSACTION_LOCK_FILE,
            TRANSACTION_MARKER_FILE,
        ];
        const ROOT_DIRECTORIES: &[&str] = &["events", "kv", "blob", "snapshots"];
        for entry in read_directory_entries_strict_bounded(
            &self.root,
            self.capacity.kv_max_entries.saturating_add(16),
            "file_store_open_preflight",
        )? {
            let name = entry_name_utf8(&entry, "file_store_open_preflight")?;
            let file_type = entry
                .file_type()
                .map_err(|error| Error::io("file_store_open_preflight", error))?;
            if file_type.is_symlink() {
                return Err(Error::config(
                    "file_store_open_preflight",
                    format!("file store root contains symlink {name}"),
                ));
            }
            if file_type.is_file()
                && (ROOT_FILES.contains(&name.as_str()) || is_orphan_tmp_path(&entry.path()))
            {
                continue;
            }
            if file_type.is_dir() && ROOT_DIRECTORIES.contains(&name.as_str()) {
                continue;
            }
            return Err(Error::config(
                "file_store_open_preflight",
                format!("unsupported file store root entry {name}"),
            ));
        }
        if !self
            .root
            .join(TRANSACTION_LOCK_FILE)
            .try_exists()
            .map_err(|error| Error::io("file_store_open_preflight", error))?
        {
            return Err(Error::config(
                "file_store_open_preflight",
                "existing file store is missing its fixed lock owner",
            ));
        }
        self.validate_single_file_lane("events", "events.jsonl")?;
        self.validate_single_file_lane("snapshots", "")?;
        self.validate_addressed_lane("kv", "json", true, journal)?;
        self.validate_addressed_lane("blob", "bin", false, journal)
    }

    fn validate_single_file_lane(&self, lane: &str, allowed_file: &str) -> Result<()> {
        let path = self.root.join(lane);
        for entry in read_directory_entries_strict_bounded(
            &path,
            self.capacity.kv_max_entries,
            "file_store_open_preflight",
        )? {
            let name = entry_name_utf8(&entry, "file_store_open_preflight")?;
            let file_type = entry
                .file_type()
                .map_err(|error| Error::io("file_store_open_preflight", error))?;
            if file_type.is_file() && (name == allowed_file || is_orphan_tmp_path(&entry.path())) {
                continue;
            }
            return Err(Error::config(
                "file_store_open_preflight",
                format!("unsupported {lane} lane entry {name}"),
            ));
        }
        Ok(())
    }

    fn validate_addressed_lane(
        &self,
        lane: &str,
        data_extension: &str,
        json_lane: bool,
        journal: Option<&FileTransactionJournal>,
    ) -> Result<()> {
        let lane_root = self.root.join(lane);
        for namespace_entry in read_directory_entries_strict_bounded(
            &lane_root,
            self.capacity.kv_max_entries,
            "file_store_open_preflight",
        )? {
            let namespace = entry_name_utf8(&namespace_entry, "file_store_open_preflight")?;
            let file_type = namespace_entry
                .file_type()
                .map_err(|error| Error::io("file_store_open_preflight", error))?;
            if !file_type.is_dir() || file_type.is_symlink() {
                return Err(Error::config(
                    "file_store_open_preflight",
                    format!("{lane} lane contains non-directory namespace {namespace}"),
                ));
            }
            if json_lane {
                admit_store_json_address(
                    &namespace,
                    "__file_open_preflight_namespace__",
                    "file_store_open_preflight",
                )?;
            } else {
                match classify_store_blob_address(
                    &namespace,
                    "__file_open_preflight_namespace__",
                    None,
                )? {
                    StoreAddressAdmission::Active(_) => {}
                    StoreAddressAdmission::ForbiddenLegacy(kind) => {
                        return Err(Error::config(
                            "file_store_open_preflight",
                            format!("forbidden legacy blob namespace {namespace} ({kind:?})"),
                        ));
                    }
                    StoreAddressAdmission::Unknown => {
                        return Err(Error::config(
                            "file_store_open_preflight",
                            format!("unsupported blob namespace {namespace}"),
                        ));
                    }
                }
            }
            self.validate_addressed_namespace_footprint(
                lane,
                &namespace,
                data_extension,
                &namespace_entry.path(),
                journal,
            )?;
        }
        Ok(())
    }

    fn validate_addressed_namespace_footprint(
        &self,
        lane: &str,
        namespace: &str,
        data_extension: &str,
        namespace_root: &Path,
        journal: Option<&FileTransactionJournal>,
    ) -> Result<()> {
        let touched_count = self.journal_address_count(journal, Some(lane), Some(namespace));
        let physical_key_limit = self.capacity.kv_max_entries.saturating_add(touched_count);
        let physical_entry_limit = physical_key_limit.saturating_add(1);
        for entry in read_directory_entries_strict_bounded(
            namespace_root,
            physical_entry_limit.saturating_add(2),
            "file_store_open_preflight",
        )? {
            let name = entry_name_utf8(&entry, "file_store_open_preflight")?;
            let file_type = entry
                .file_type()
                .map_err(|error| Error::io("file_store_open_preflight", error))?;
            if file_type.is_file() && is_orphan_tmp_path(&entry.path()) {
                continue;
            }
            if file_type.is_dir()
                && matches!(
                    name.as_str(),
                    FILE_ADDRESSING_INDEX_DIR | FILE_ADDRESSING_DATA_DIR
                )
            {
                continue;
            }
            return Err(Error::config(
                "file_store_open_preflight",
                format!("unsupported physical address entry {lane}/{namespace}/{name}"),
            ));
        }

        let indexes = self.collect_addressed_files(
            &namespace_root.join(FILE_ADDRESSING_INDEX_DIR),
            "json",
            physical_key_limit,
            physical_entry_limit,
        )?;
        let data = self.collect_addressed_files(
            &namespace_root.join(FILE_ADDRESSING_DATA_DIR),
            data_extension,
            physical_key_limit,
            physical_entry_limit,
        )?;
        let mut touched_digests = BTreeSet::new();
        if let Some(journal) = journal {
            let entries = if lane == "kv" {
                journal
                    .before
                    .json
                    .iter()
                    .chain(journal.after.json.iter())
                    .map(|entry| (entry.namespace.as_str(), entry.key.as_str()))
                    .collect::<Vec<_>>()
            } else {
                journal
                    .before
                    .blobs
                    .iter()
                    .chain(journal.after.blobs.iter())
                    .map(|entry| (entry.namespace.as_str(), entry.key.as_str()))
                    .collect::<Vec<_>>()
            };
            for (_, key) in entries
                .into_iter()
                .filter(|(entry_namespace, _)| *entry_namespace == namespace)
            {
                let paths = self.physical_key_paths_at_root(
                    &self.root,
                    lane,
                    namespace,
                    key,
                    data_extension,
                )?;
                let digest = paths
                    .data_path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| {
                        Error::config(
                            "file_store_open_preflight",
                            format!(
                                "physical address digest is invalid for {lane}/{namespace}/{key}"
                            ),
                        )
                    })?;
                touched_digests.insert(digest.to_string());
            }
        }
        let index_digests = indexes.keys().cloned().collect::<BTreeSet<_>>();
        let data_digests = data.keys().cloned().collect::<BTreeSet<_>>();
        if !index_digests
            .symmetric_difference(&data_digests)
            .all(|digest| touched_digests.contains(digest))
        {
            return Err(Error::config(
                "file_store_open_preflight",
                format!("unpaired physical address in {lane}/{namespace}"),
            ));
        }
        for (digest, index_path) in indexes {
            let key = self
                .read_key_index(&index_path, "file_store_open_preflight")?
                .ok_or_else(|| {
                    Error::config(
                        "file_store_open_preflight",
                        "physical key index disappeared during preflight",
                    )
                })?;
            let expected =
                self.physical_key_paths_at_root(&self.root, lane, namespace, &key, data_extension)?;
            let data_path = data.get(&digest);
            if expected.index_path != index_path
                || data_path.is_some_and(|data_path| expected.data_path != *data_path)
                || (data_path.is_none() && !touched_digests.contains(&digest))
            {
                return Err(Error::config(
                    "file_store_open_preflight",
                    format!("physical address does not match indexed key {lane}/{namespace}/{key}"),
                ));
            }
        }
        Ok(())
    }

    fn collect_addressed_files(
        &self,
        root: &Path,
        extension: &str,
        max_keys: usize,
        max_entries: usize,
    ) -> Result<BTreeMap<String, PathBuf>> {
        let mut files = BTreeMap::new();
        for shard_entry in read_directory_entries_strict_bounded(
            root,
            max_entries.min(256),
            "file_store_open_preflight",
        )? {
            let shard = entry_name_utf8(&shard_entry, "file_store_open_preflight")?;
            let file_type = shard_entry
                .file_type()
                .map_err(|error| Error::io("file_store_open_preflight", error))?;
            if !file_type.is_dir()
                || file_type.is_symlink()
                || shard.len() != 2
                || !shard
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err(Error::config(
                    "file_store_open_preflight",
                    format!("invalid physical address shard {shard}"),
                ));
            }
            let file_entries = read_directory_entries_strict_bounded(
                &shard_entry.path(),
                max_entries,
                "file_store_open_preflight",
            )?;
            if files.len().saturating_add(file_entries.len()) > max_entries {
                return Err(store_budget_error(format!(
                    "file_store_open_preflight physical address entries exceed {max_entries}"
                )));
            }
            for file_entry in file_entries {
                let path = file_entry.path();
                let name = entry_name_utf8(&file_entry, "file_store_open_preflight")?;
                let file_type = file_entry
                    .file_type()
                    .map_err(|error| Error::io("file_store_open_preflight", error))?;
                if file_type.is_file() && is_orphan_tmp_path(&path) {
                    continue;
                }
                let digest = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| {
                        Error::config(
                            "file_store_open_preflight",
                            format!("invalid physical address file {name}"),
                        )
                    })?;
                if !file_type.is_file()
                    || file_type.is_symlink()
                    || path.extension().and_then(|value| value.to_str()) != Some(extension)
                    || digest.len() != self.path_budget.physical_key_digest_hex_chars
                    || !digest
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                    || !digest.starts_with(&shard)
                    || files.insert(digest.to_string(), path).is_some()
                    || files.len() > max_keys
                {
                    return Err(Error::config(
                        "file_store_open_preflight",
                        format!("invalid or duplicate physical address file {name}"),
                    ));
                }
            }
        }
        Ok(files)
    }

    fn open_preflight_address_keys(
        &self,
        lane: &str,
        namespace: &str,
        data_extension: &str,
        journal: Option<&FileTransactionJournal>,
    ) -> Result<BTreeSet<String>> {
        let namespace_root = self.root.join(lane).join(namespace);
        let touched_count = self.journal_address_count(journal, Some(lane), Some(namespace));
        let physical_key_limit = self.capacity.kv_max_entries.saturating_add(touched_count);
        let indexes = self.collect_addressed_files(
            &namespace_root.join(FILE_ADDRESSING_INDEX_DIR),
            "json",
            physical_key_limit,
            physical_key_limit.saturating_add(1),
        )?;
        let mut keys = indexes
            .values()
            .map(|path| {
                self.read_key_index(path, "file_store_open_preflight")?
                    .ok_or_else(|| {
                        Error::config(
                            "file_store_open_preflight",
                            "physical key index disappeared during snapshot admission",
                        )
                    })
            })
            .collect::<Result<BTreeSet<_>>>()?;
        if let Some(journal) = journal {
            if lane == "kv" {
                keys.extend(
                    journal
                        .before
                        .json
                        .iter()
                        .chain(journal.after.json.iter())
                        .filter(|entry| entry.namespace == namespace)
                        .map(|entry| entry.key.clone()),
                );
            } else {
                keys.extend(
                    journal
                        .before
                        .blobs
                        .iter()
                        .chain(journal.after.blobs.iter())
                        .filter(|entry| entry.namespace == namespace)
                        .map(|entry| entry.key.clone()),
                );
            }
        }
        for key in &keys {
            self.physical_key_paths_at_root(&self.root, lane, namespace, key, data_extension)?;
        }
        Ok(keys)
    }

    fn journal_address_count(
        &self,
        journal: Option<&FileTransactionJournal>,
        lane: Option<&str>,
        namespace: Option<&str>,
    ) -> usize {
        let Some(journal) = journal else {
            return 0;
        };
        let mut addresses = BTreeSet::new();
        if lane.is_none() || lane == Some("kv") {
            addresses.extend(
                journal
                    .before
                    .json
                    .iter()
                    .chain(journal.after.json.iter())
                    .filter(|entry| namespace.is_none_or(|value| entry.namespace == value))
                    .map(|entry| ("kv", entry.namespace.as_str(), entry.key.as_str())),
            );
        }
        if lane.is_none() || lane == Some("blob") {
            addresses.extend(
                journal
                    .before
                    .blobs
                    .iter()
                    .chain(journal.after.blobs.iter())
                    .filter(|entry| namespace.is_none_or(|value| entry.namespace == value))
                    .map(|entry| ("blob", entry.namespace.as_str(), entry.key.as_str())),
            );
        }
        addresses.len()
    }

    fn overlay_transaction_image_for_open_preflight(
        &self,
        snapshot: StoreSnapshot,
        image: &FileTransactionImage,
    ) -> Result<StoreSnapshot> {
        let mut json = snapshot
            .json_docs
            .into_iter()
            .map(|doc| ((doc.namespace, doc.key), doc.value))
            .collect::<BTreeMap<_, _>>();
        for entry in &image.json {
            let address = (entry.namespace.clone(), entry.key.clone());
            match &entry.value {
                Some(value) => {
                    json.insert(address, value.clone());
                }
                None => {
                    json.remove(&address);
                }
            }
        }

        let mut blobs = snapshot
            .blobs
            .into_iter()
            .map(|blob| ((blob.namespace, blob.key), blob.value))
            .collect::<BTreeMap<_, _>>();
        for entry in &image.blobs {
            let address = (entry.namespace.clone(), entry.key.clone());
            match &entry.value {
                Some(value) => {
                    blobs.insert(address, value.clone());
                }
                None => {
                    blobs.remove(&address);
                }
            }
        }

        let events = match &image.events {
            FileTransactionEventsImage::Append {
                prefix_len: _,
                events,
            } => {
                let mut prefix = snapshot.events.clone();
                prefix.extend(events.iter().cloned());
                prefix
            }
            FileTransactionEventsImage::Replace { events } => events.clone(),
        };
        if events.len() > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "file open preflight event items {} exceed {}",
                events.len(),
                self.capacity.event_log_max_items
            )));
        }
        for event in &events {
            enforce_event_key_budget(self.capacity, event, "file_store_open_preflight")?;
        }

        Ok(StoreSnapshot::new(
            snapshot.schema_manifest,
            json.into_iter()
                .map(|((namespace, key), value)| StoreSnapshotJsonDoc {
                    namespace,
                    key,
                    value,
                })
                .collect(),
            blobs
                .into_iter()
                .map(|((namespace, key), value)| StoreSnapshotBlob {
                    namespace,
                    key,
                    value,
                })
                .collect(),
            events,
        ))
    }

    fn transaction_marker_path(&self) -> PathBuf {
        self.root.join(TRANSACTION_MARKER_FILE)
    }

    fn recover_transaction_if_needed(&self) -> Result<()> {
        let journal = self.read_transaction_journal_read_only()?;
        self.recover_validated_transaction(journal)
    }

    fn recover_validated_transaction(&self, journal: Option<FileTransactionJournal>) -> Result<()> {
        let Some(journal) = journal else {
            return Ok(());
        };
        let image = match journal.state {
            FileTransactionJournalState::Prepared => &journal.before,
            FileTransactionJournalState::Committed => &journal.after,
        };
        self.restore_transaction_image(image)?;
        self.remove_transaction_journal("file_store_transaction_recovery")
    }

    fn read_transaction_journal_read_only(&self) -> Result<Option<FileTransactionJournal>> {
        let path = self.transaction_marker_path();
        let Some(bytes) = read_file_bounded(
            &path,
            self.capacity
                .import_max_bytes
                .saturating_add(self.capacity.blob_max_bytes.saturating_mul(3)),
            "file_store_transaction_recovery",
        )?
        else {
            return Ok(None);
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
        if journal.schema_version != 2 {
            return Err(Error::config(
                TRANSACTION_REPAIR_REQUIRED_STAGE,
                format!(
                    "unsupported file transaction journal schema {}",
                    journal.schema_version
                ),
            ));
        }
        journal.verify_checksum()?;
        journal.validate_image_address_contract()?;
        Ok(Some(journal))
    }

    fn transaction_image(
        state: &BackendTransactionState,
        read_set: &crate::store_internal::transaction::StoreTransactionReadSet,
        events: FileTransactionEventsImage,
    ) -> FileTransactionImage {
        FileTransactionImage {
            json: read_set
                .json
                .iter()
                .map(|(namespace, key)| FileTransactionJsonValue {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: state.json.get(&(namespace.clone(), key.clone())).cloned(),
                })
                .collect(),
            blobs: read_set
                .blobs
                .iter()
                .map(|(namespace, key)| FileTransactionBlobValue {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: state.blobs.get(&(namespace.clone(), key.clone())).cloned(),
                })
                .collect(),
            events,
        }
    }

    fn restore_transaction_image(&self, image: &FileTransactionImage) -> Result<()> {
        for entry in &image.json {
            match &entry.value {
                Some(value) => {
                    let paths = self.json_paths(&entry.namespace, &entry.key)?;
                    self.write_key_index(
                        &paths.index_path,
                        &entry.key,
                        "file_store_transaction_recovery",
                    )?;
                    self.maybe_crash_for_recovery_contract("after_json_index_before_data", false);
                    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
                        Error::config("file_store_transaction_recovery", error.to_string())
                    })?;
                    atomic_write(
                        &paths.data_path,
                        &bytes,
                        self.fsync,
                        "file_store_transaction_recovery",
                    )?;
                }
                None => {
                    let paths = self.json_paths(&entry.namespace, &entry.key)?;
                    remove_file_if_exists(&paths.data_path, "file_store_transaction_recovery")?;
                    self.maybe_crash_for_recovery_contract(
                        "after_json_data_delete_before_index",
                        false,
                    );
                    remove_file_if_exists(&paths.index_path, "file_store_transaction_recovery")?;
                    remove_file_if_exists(&paths.legacy_path, "file_store_transaction_recovery")?;
                }
            }
        }
        for entry in &image.blobs {
            match &entry.value {
                Some(value) => {
                    let paths = self.blob_paths(&entry.namespace, &entry.key)?;
                    self.write_key_index(
                        &paths.index_path,
                        &entry.key,
                        "file_store_transaction_recovery",
                    )?;
                    atomic_write(
                        &paths.data_path,
                        value,
                        self.fsync,
                        "file_store_transaction_recovery",
                    )?;
                }
                None => {
                    let paths = self.blob_paths(&entry.namespace, &entry.key)?;
                    remove_file_if_exists(&paths.data_path, "file_store_transaction_recovery")?;
                    remove_file_if_exists(&paths.index_path, "file_store_transaction_recovery")?;
                    remove_file_if_exists(&paths.legacy_path, "file_store_transaction_recovery")?;
                }
            }
        }
        match &image.events {
            FileTransactionEventsImage::Append { prefix_len, events } => {
                let events_path = self.events_path();
                let mut events_file = OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(&events_path)
                    .map_err(|error| Error::io("file_store_transaction_recovery", error))?;
                events_file
                    .set_len(*prefix_len)
                    .map_err(|error| Error::io("file_store_transaction_recovery", error))?;
                events_file
                    .seek(SeekFrom::End(0))
                    .map_err(|error| Error::io("file_store_transaction_recovery", error))?;
                drop(events_file);
                for event in events {
                    self.append_event_unchecked(event)?;
                }
            }
            FileTransactionEventsImage::Replace { events } => {
                let bytes = events_jsonl_bytes(events)?;
                atomic_write(
                    &self.events_path(),
                    &bytes,
                    self.fsync,
                    "file_store_transaction_recovery",
                )?;
            }
        }
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

    fn maybe_crash_for_recovery_contract(&self, _point: &str, _contains_operation_pair: bool) {
        #[cfg(feature = "nonproduction-replay-harness")]
        if std::env::var_os("BM_FILE_TRANSACTION_RECOVERY_WORKER").as_deref()
            == Some(std::ffi::OsStr::new("1"))
            && std::env::var_os("BM_FILE_TRANSACTION_CRASH_POINT").as_deref()
                == Some(std::ffi::OsStr::new(_point))
            && (std::env::var_os("BM_FILE_TRANSACTION_CRASH_REQUIRES_OPERATION_PAIR").is_none()
                || _contains_operation_pair)
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
            self.validate_existing_manifest_read_only(config)?
                .ok_or_else(|| {
                    Error::config(
                        "file_store_manifest",
                        "existing manifest disappeared before open completed",
                    )
                })
        } else {
            self.ensure_missing_manifest_store_is_empty()?;
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

    #[cfg(feature = "nonproduction-replay-harness")]
    fn json_dir(&self, namespace: &str) -> PathBuf {
        self.root.join("kv").join(namespace)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
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
        let value = FileKeyIndex {
            addressing_version: FILE_ADDRESSING_VERSION,
            key: key.to_string(),
        };
        let bytes = serde_json::to_vec_pretty(&value)
            .map_err(|error| Error::config(stage, error.to_string()))?;
        atomic_write(path, &bytes, self.fsync, stage)
    }

    fn read_key_index(&self, path: &Path, stage: &'static str) -> Result<Option<String>> {
        let Some(bytes) = read_file_bounded(
            path,
            self.capacity.logical_key_max_bytes.saturating_add(1024),
            stage,
        )?
        else {
            return Ok(None);
        };
        let value: FileKeyIndex = serde_json::from_slice(&bytes)
            .map_err(|error| Error::config(stage, error.to_string()))?;
        if value.addressing_version != FILE_ADDRESSING_VERSION {
            return Err(Error::config(
                stage,
                format!(
                    "unsupported file store key index version {}",
                    value.addressing_version
                ),
            ));
        }
        Ok(Some(value.key))
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

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
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

    #[cfg(feature = "nonproduction-replay-harness")]
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

    #[cfg(feature = "nonproduction-replay-harness")]
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

    #[cfg(feature = "nonproduction-replay-harness")]
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

    #[cfg(feature = "nonproduction-replay-harness")]
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
                if let Some(bytes) = self.blob_value_len_unlocked(&namespace, &key)? {
                    total = total
                        .checked_add(bytes)
                        .ok_or_else(|| store_budget_error("file store blob byte count overflow"))?;
                }
            }
        }
        Ok(total)
    }

    fn blob_entry_count(&self) -> Result<usize> {
        let mut count = 0_usize;
        for namespace in
            list_child_directory_names(&self.root.join("blob"), "file_store_blob_quota")?
        {
            count = count.saturating_add(self.list_blob_keys_unlocked(&namespace)?.len());
        }
        Ok(count)
    }

    fn blob_value_len_unlocked(&self, namespace: &str, key: &str) -> Result<Option<usize>> {
        let paths = self.blob_paths(namespace, key)?;
        match fs::metadata(&paths.data_path) {
            Ok(metadata) => {
                self.require_key_index_matches(&paths.index_path, key, "file_store_blob_quota")?;
                usize::try_from(metadata.len())
                    .map(Some)
                    .map_err(|_| store_budget_error("blob length exceeds platform address space"))
            }
            Err(error) if is_not_found_or_invalid_filename(&error) => {
                if self
                    .read_key_index(&paths.index_path, "file_store_blob_quota")?
                    .is_some()
                {
                    return Err(Error::config(
                        "file_store_blob_quota",
                        "file store key index has missing physical data",
                    ));
                }
                match fs::metadata(&paths.legacy_path) {
                    Ok(metadata) => usize::try_from(metadata.len()).map(Some).map_err(|_| {
                        store_budget_error("blob length exceeds platform address space")
                    }),
                    Err(error) if is_not_found_or_invalid_filename(&error) => Ok(None),
                    Err(error) => Err(Error::io("file_store_blob_quota", error)),
                }
            }
            Err(error) => Err(Error::io("file_store_blob_quota", error)),
        }
    }

    fn transaction_event_usage(
        &self,
        request: &StoreTransactionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<(usize, BTreeSet<String>, u64)> {
        let event_ids = request
            .mutations
            .iter()
            .filter_map(crate::store_internal::transaction::mutation_event_id)
            .collect::<BTreeSet<_>>();
        let path = self.events_path();
        let prefix_len = match fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(Error::io("memory_write_transaction", error)),
        };
        let events = read_events_jsonl_bounded(
            &path,
            capacity,
            capacity.snapshot_max_bytes,
            "memory_write_transaction",
        )?;
        let mut existing = BTreeSet::new();
        if !event_ids.is_empty() {
            for event in &events {
                if event_ids.contains(event.event_id.as_str()) {
                    existing.insert(event.event_id.clone());
                }
            }
        }
        Ok((events.len(), existing, prefix_len))
    }

    fn load_transaction_context(
        &self,
        request: &StoreTransactionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<(StoreTransactionContext, u64)> {
        let mut touched = BackendTransactionState::default();
        let mut json_bytes = 0_usize;
        for (namespace, key) in &request.read_set().json {
            let remaining = capacity.snapshot_max_bytes.saturating_sub(json_bytes);
            if let Some((value, bytes)) =
                self.get_json_value_unlocked_bounded(namespace, key, remaining)?
            {
                json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("transaction touched JSON byte count overflow")
                })?;
                touched.json.insert((namespace.clone(), key.clone()), value);
            }
        }
        for (namespace, prefix) in &request.read_set().json_prefixes {
            for key in self.list_json_keys_unlocked(namespace)? {
                if !key.starts_with(prefix)
                    || touched.json.contains_key(&(namespace.clone(), key.clone()))
                {
                    continue;
                }
                let remaining = capacity.snapshot_max_bytes.saturating_sub(json_bytes);
                if let Some((value, bytes)) =
                    self.get_json_value_unlocked_bounded(namespace, &key, remaining)?
                {
                    json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                        store_budget_error("transaction touched JSON byte count overflow")
                    })?;
                    touched.json.insert((namespace.clone(), key), value);
                }
            }
        }
        let mut touched_blob_bytes = 0_usize;
        for (namespace, key) in &request.read_set().blobs {
            let remaining = capacity.blob_max_bytes.saturating_sub(touched_blob_bytes);
            if let Some((value, bytes)) =
                self.get_blob_unlocked_bounded(namespace, key, remaining)?
            {
                touched_blob_bytes = touched_blob_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("transaction touched blob byte count overflow")
                })?;
                touched
                    .blobs
                    .insert((namespace.clone(), key.clone()), value);
            }
        }
        let (event_count, existing_event_ids, event_prefix_len) =
            self.transaction_event_usage(request, capacity)?;
        let usage = StoreBackendUsage {
            kv_entries: self
                .json_entry_count()?
                .saturating_add(self.blob_entry_count()?),
            blob_bytes: self.blob_total_bytes()?,
            event_count,
        };
        Ok((
            StoreTransactionContext {
                touched,
                usage,
                existing_event_ids,
            },
            event_prefix_len,
        ))
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
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

    #[cfg(feature = "nonproduction-replay-harness")]
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

    #[cfg(feature = "nonproduction-replay-harness")]
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
        let mut retained_blob_lengths = Vec::new();
        for namespace in
            list_child_directory_names(&self.root.join("blob"), "file_store_blob_quota")?
                .into_iter()
                .filter(|namespace| !blob_namespace_set.contains(namespace.as_str()))
        {
            for key in self.list_blob_keys_unlocked(&namespace)? {
                if let Some(value) = self.get_blob_unlocked(&namespace, &key)? {
                    retained_blob_lengths.push(value.len());
                }
            }
        }
        validate_restore_post_image_blob_bytes(
            self.capacity,
            retained_blob_lengths,
            blobs.iter().map(|blob| blob.value.len()),
        )?;
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
        let event_bytes = serde_json::to_vec(event)
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        #[cfg(feature = "nonproduction-replay-harness")]
        if std::env::var_os("BM_FILE_TRANSACTION_RECOVERY_WORKER").as_deref()
            == Some(std::ffi::OsStr::new("1"))
            && std::env::var_os("BM_FILE_TRANSACTION_CRASH_POINT").as_deref()
                == Some(std::ffi::OsStr::new("mid_event_append"))
        {
            file.write_all(&event_bytes[..event_bytes.len().max(2) / 2])
                .expect("write partial recovery-contract event");
            file.sync_all()
                .expect("sync partial recovery-contract event");
            std::process::exit(86);
        }
        file.write_all(&event_bytes)
            .map_err(|error| Error::io("store_event_log", error))?;
        file.write_all(b"\n")
            .map_err(|error| Error::io("store_event_log", error))?;
        if self.fsync {
            file.sync_all()
                .map_err(|error| Error::io("store_event_log", error))?;
        }
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
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

    fn acquire_immutable_read_lock(&self) -> Result<FileBackendLock<'_>> {
        loop {
            let lock = self.acquire_backend_lock(false, "store_immutable_read_session")?;
            match fs::metadata(self.transaction_marker_path()) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(lock),
                Err(error) => return Err(Error::io("store_immutable_read_session", error)),
                Ok(_) => drop(lock),
            }
            let _lock = self.acquire_backend_lock(true, "store_immutable_read_session")?;
            self.recover_transaction_if_needed()?;
        }
    }

    fn read_events_unlocked(&self) -> Result<Vec<MemoryStoreEvent>> {
        read_events_jsonl_bounded(
            &self.events_path(),
            self.capacity,
            self.capacity.snapshot_max_bytes,
            "store_event_log",
        )
    }

    fn read_events_unlocked_bounded(
        &self,
        capacity: StoreCapacityBudget,
        already_used_json_bytes: usize,
    ) -> Result<Vec<MemoryStoreEvent>> {
        let path = self.events_path();
        let bytes = match fs::metadata(&path) {
            Ok(metadata) => usize::try_from(metadata.len()).map_err(|_| {
                store_budget_error("event log length does not fit the platform address space")
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(Error::io("store_consistent_read", error)),
        };
        if already_used_json_bytes.saturating_add(bytes) > capacity.snapshot_max_bytes {
            return Err(Error::config(
                "store_consistent_read_budget_exceeded",
                "event log exceeds the consistent known-key read budget",
            ));
        }
        read_events_jsonl_bounded(
            &path,
            capacity,
            capacity
                .snapshot_max_bytes
                .saturating_sub(already_used_json_bytes),
            "store_consistent_read_budget_exceeded",
        )
    }

    fn read_scoped_events_unlocked_bounded(
        &self,
        scope: &crate::StoreScopedProjectionScope,
        capacity: StoreCapacityBudget,
        already_used_json_bytes: usize,
    ) -> Result<Vec<MemoryStoreEvent>> {
        let remaining = capacity
            .snapshot_max_bytes
            .checked_sub(already_used_json_bytes)
            .ok_or_else(|| {
                Error::config(
                    "store_scoped_projection_budget_exceeded",
                    "scoped JSON already exceeds the pinned projection byte budget",
                )
            })?;
        let events = read_events_jsonl_bounded(
            &self.events_path(),
            capacity,
            remaining,
            "store_scoped_projection_budget_exceeded",
        )?;
        Ok(events
            .into_iter()
            .filter(|event| {
                crate::store_internal::transaction::event_matches_scoped_projection(event, scope)
            })
            .collect())
    }

    fn read_scoped_json_unlocked_exact(
        &self,
        request: &crate::StoreScopedProjectionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<ScopedJsonRead> {
        let mut json = BTreeMap::new();
        let mut json_bytes = 0_usize;
        let mut observed = BTreeSet::new();
        let mut pending =
            scoped_projection_root_addresses(&request.json_namespaces, &request.scope)?
                .into_iter()
                .collect::<BTreeSet<_>>();
        while let Some(address) = pending.pop_first() {
            if !observed.insert(address.clone()) {
                continue;
            }
            if observed.len() > capacity.kv_max_entries {
                return Err(Error::config(
                    "store_scoped_projection_budget_exceeded",
                    "scoped projection exact-key reads exceed the pinned operation entry budget",
                ));
            }
            let remaining = capacity.snapshot_max_bytes.saturating_sub(json_bytes);
            if let Some((value, bytes)) =
                self.get_json_value_unlocked_bounded(&address.0, &address.1, remaining)?
            {
                json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("scoped projection JSON byte count overflow")
                })?;
                json.insert(address, value);
                for dependency in scoped_projection_dependency_addresses(
                    &json,
                    &request.json_namespaces,
                    &request.scope,
                )? {
                    if !observed.contains(&dependency) {
                        pending.insert(dependency);
                    }
                }
            }
        }
        crate::store_internal::transaction::validate_scoped_recall_manifest_documents(
            &json,
            &BTreeMap::new(),
            &request.scope,
        )?;
        crate::store_internal::transaction::validate_scoped_control_plane_documents(
            &json,
            &request.scope,
            capacity.kv_max_entries,
        )?;
        Ok(ScopedJsonRead {
            documents: json,
            logical_bytes: json_bytes,
        })
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

    fn get_json_value_unlocked_bounded(
        &self,
        namespace: &str,
        key: &str,
        max_logical_bytes: usize,
    ) -> Result<Option<(Value, usize)>> {
        let paths = self.json_paths(namespace, key)?;
        let bytes = match read_file_bounded(
            &paths.data_path,
            self.capacity.snapshot_max_bytes,
            "store_consistent_read_budget_exceeded",
        )? {
            Some(bytes) => {
                self.require_key_index_matches(&paths.index_path, key, "store_consistent_read")?;
                bytes
            }
            None => {
                if self
                    .read_key_index(&paths.index_path, "store_consistent_read")?
                    .is_some()
                {
                    return Err(Error::config(
                        "store_consistent_read",
                        "file store key index has missing physical data",
                    ));
                }
                let Some(bytes) = read_file_bounded(
                    &paths.legacy_path,
                    self.capacity.snapshot_max_bytes,
                    "store_consistent_read_budget_exceeded",
                )?
                else {
                    return Ok(None);
                };
                bytes
            }
        };
        let value = serde_json::from_slice::<Value>(&bytes)
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        let logical_bytes = serde_json::to_vec(&value)
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?
            .len();
        if logical_bytes > max_logical_bytes {
            return Err(Error::config(
                "store_consistent_read_budget_exceeded",
                format!(
                    "logical JSON bytes {logical_bytes} exceed remaining budget {max_logical_bytes}"
                ),
            ));
        }
        Ok(Some((value, logical_bytes)))
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

    fn get_blob_unlocked_bounded(
        &self,
        namespace: &str,
        key: &str,
        max_bytes: usize,
    ) -> Result<Option<(Vec<u8>, usize)>> {
        let paths = self.blob_paths(namespace, key)?;
        let bytes = match read_file_bounded(
            &paths.data_path,
            max_bytes,
            "store_consistent_read_budget_exceeded",
        )? {
            Some(bytes) => {
                self.require_key_index_matches(&paths.index_path, key, "store_consistent_read")?;
                bytes
            }
            None => {
                if self
                    .read_key_index(&paths.index_path, "store_consistent_read")?
                    .is_some()
                {
                    return Err(Error::config(
                        "store_consistent_read",
                        "file store key index has missing physical data",
                    ));
                }
                let Some(bytes) = read_file_bounded(
                    &paths.legacy_path,
                    max_bytes,
                    "store_consistent_read_budget_exceeded",
                )?
                else {
                    return Ok(None);
                };
                bytes
            }
        };
        let byte_count = bytes.len();
        Ok(Some((bytes, byte_count)))
    }

    fn list_blob_keys_unlocked(&self, namespace: &str) -> Result<Vec<String>> {
        self.validate_directory_component(namespace, "file_store_list")?;
        self.list_keys("blob", namespace, "bin")
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn append_event_unlocked(&self, event: &MemoryStoreEvent) -> Result<()> {
        self.ensure_can_append_event(event)?;
        self.append_event_unchecked(event)
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    fn put_json_value_unlocked(&self, namespace: &str, key: &str, value: &Value) -> Result<()> {
        let paths = self.json_paths(namespace, key)?;
        self.ensure_json_entry_budget(namespace, key)?;
        self.ensure_key_index_available(&paths, key, "file_store_json_write")?;
        self.write_key_index(&paths.index_path, key, "file_store_json_write")?;
        self.maybe_crash_for_recovery_contract("after_json_index_before_data", false);
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|error| Error::config("file_store_json_write", error.to_string()))?;
        atomic_write(
            &paths.data_path,
            &bytes,
            self.fsync,
            "file_store_json_write",
        )
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_json_value_unlocked(&self, namespace: &str, key: &str) -> Result<bool> {
        let paths = self.json_paths(namespace, key)?;
        self.validate_v2_pair_for_delete(&paths, key, "file_store_json_delete")?;
        let mut deleted = false;
        deleted |= remove_file_if_exists(&paths.data_path, "file_store_json_delete")?;
        self.maybe_crash_for_recovery_contract("after_json_data_delete_before_index", false);
        deleted |= remove_file_if_exists(&paths.index_path, "file_store_json_delete")?;
        deleted |= remove_file_if_exists(&paths.legacy_path, "file_store_json_delete")?;
        Ok(deleted)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_blob_unlocked(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        let paths = self.blob_paths(namespace, key)?;
        self.ensure_blob_total_budget(namespace, key, value.len())?;
        self.ensure_key_index_available(&paths, key, "file_store_blob_write")?;
        self.write_key_index(&paths.index_path, key, "file_store_blob_write")?;
        atomic_write(&paths.data_path, value, self.fsync, "file_store_blob_write")
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_blob_unlocked(&self, namespace: &str, key: &str) -> Result<bool> {
        let paths = self.blob_paths(namespace, key)?;
        self.validate_v2_pair_for_delete(&paths, key, "file_store_blob_delete")?;
        let mut deleted = false;
        deleted |= remove_file_if_exists(&paths.data_path, "file_store_blob_delete")?;
        deleted |= remove_file_if_exists(&paths.index_path, "file_store_blob_delete")?;
        deleted |= remove_file_if_exists(&paths.legacy_path, "file_store_blob_delete")?;
        Ok(deleted)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
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
    #[cfg(feature = "nonproduction-replay-harness")]
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        self.with_exclusive_backend("store_event_log", || self.append_event_unlocked(&event))
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        self.with_shared_backend("store_event_log", || self.read_events_unlocked())
    }
}

pub(crate) fn read_metric_events_from_root(
    root: &Path,
    capacity: StoreCapacityBudget,
) -> Result<StoreMetricEventSourceRead> {
    let manifest_path = root.join("manifest.json");
    let event_path = root.join("events").join("events.jsonl");
    let manifest_metadata_bytes =
        required_metric_source_file_len(&manifest_path, "runtime_metrics_event_bytes")?;
    let event_metadata_bytes =
        optional_metric_source_file_len(&event_path, "runtime_metrics_event_bytes")?;
    let metadata_total = manifest_metadata_bytes
        .checked_add(event_metadata_bytes)
        .ok_or_else(|| {
            Error::config(
                "runtime_metrics_event_bytes",
                "runtime metric source metadata byte count overflow",
            )
        })?;
    if metadata_total > capacity.snapshot_max_bytes {
        return Err(Error::config(
            "runtime_metrics_event_bytes",
            "runtime metric source exceeds the active aggregate byte budget",
        ));
    }
    let manifest_bytes = read_file_bounded(
        &manifest_path,
        capacity.snapshot_max_bytes,
        "runtime_metrics_event_bytes",
    )?
    .ok_or_else(|| Error::config("runtime_metrics_event_store", "store manifest is required"))?;
    if manifest_bytes.len() != manifest_metadata_bytes {
        return Err(Error::config(
            "runtime_metrics_event_store",
            "runtime metric manifest changed during its bounded read",
        ));
    }
    let manifest: StoreSchemaManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| Error::config("runtime_metrics_event_store", error.to_string()))?;
    if manifest.schema_id != STORE_SCHEMA_ID || manifest.schema_version != STORE_SCHEMA_VERSION {
        return Err(Error::config(
            "runtime_metrics_event_store",
            format!(
                "unsupported store schema {} version {}",
                manifest.schema_id, manifest.schema_version
            ),
        ));
    }
    let remaining_bytes = capacity
        .snapshot_max_bytes
        .checked_sub(manifest_bytes.len())
        .ok_or_else(|| {
            Error::config(
                "runtime_metrics_event_bytes",
                "runtime metric manifest exceeds the active byte budget",
            )
        })?;
    let (events, event_bytes) = read_events_jsonl_bounded_with_receipt(
        &event_path,
        capacity,
        remaining_bytes,
        "runtime_metrics_event_capacity",
        "runtime_metrics_event_bytes",
    )?;
    if event_bytes != event_metadata_bytes {
        return Err(Error::config(
            "runtime_metrics_event_store",
            "runtime metric event file changed during its bounded read",
        ));
    }
    Ok(StoreMetricEventSourceRead {
        events,
        accounted_snapshot_bytes: manifest_bytes.len().checked_add(event_bytes).ok_or_else(
            || {
                Error::config(
                    "runtime_metrics_event_bytes",
                    "runtime metric source byte count overflow",
                )
            },
        )?,
    })
}

fn required_metric_source_file_len(path: &Path, stage: &'static str) -> Result<usize> {
    let metadata = std::fs::metadata(path).map_err(|error| Error::io(stage, error))?;
    usize::try_from(metadata.len()).map_err(|_| {
        Error::config(
            stage,
            "runtime metric source file exceeds the platform address space",
        )
    })
}

fn optional_metric_source_file_len(path: &Path, stage: &'static str) -> Result<usize> {
    match std::fs::metadata(path) {
        Ok(metadata) => usize::try_from(metadata.len()).map_err(|_| {
            Error::config(
                stage,
                "runtime metric source file exceeds the platform address space",
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(Error::io(stage, error)),
    }
}

fn read_file_bounded(
    path: &Path,
    max_bytes: usize,
    stage: &'static str,
) -> Result<Option<Vec<u8>>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if is_not_found_or_invalid_filename(&error) => return Ok(None),
        Err(error) => return Err(Error::io(stage, error)),
    };
    let metadata_len = file
        .metadata()
        .map_err(|error| Error::io(stage, error))?
        .len();
    if metadata_len > max_bytes as u64 {
        return Err(Error::config(
            stage,
            format!("file bytes {metadata_len} exceed remaining budget {max_bytes}"),
        ));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata_len).unwrap_or(max_bytes));
    Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::io(stage, error))?;
    if bytes.len() > max_bytes {
        return Err(Error::config(
            stage,
            format!(
                "streamed file bytes {} exceed remaining budget {max_bytes}",
                bytes.len()
            ),
        ));
    }
    Ok(Some(bytes))
}

fn read_events_jsonl_bounded(
    path: &Path,
    capacity: StoreCapacityBudget,
    max_bytes: usize,
    stage: &'static str,
) -> Result<Vec<MemoryStoreEvent>> {
    read_events_jsonl_bounded_with_receipt(path, capacity, max_bytes, stage, stage)
        .map(|(events, _)| events)
}

fn read_events_jsonl_bounded_with_receipt(
    path: &Path,
    capacity: StoreCapacityBudget,
    max_bytes: usize,
    item_stage: &'static str,
    byte_stage: &'static str,
) -> Result<(Vec<MemoryStoreEvent>, usize)> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), 0));
        }
        Err(error) => return Err(Error::io(byte_stage, error)),
    };
    let metadata_len = file
        .metadata()
        .map_err(|error| Error::io(byte_stage, error))?
        .len();
    if metadata_len > max_bytes as u64 {
        return Err(Error::config(
            byte_stage,
            "runtime metric event bytes exceed the active aggregate budget",
        ));
    }
    read_events_from_reader_bounded(
        BufReader::new(file),
        capacity,
        max_bytes,
        item_stage,
        byte_stage,
    )
}

fn read_events_jsonl_prefix_bounded(
    path: &Path,
    prefix_len: u64,
    capacity: StoreCapacityBudget,
    max_bytes: usize,
    stage: &'static str,
) -> Result<Vec<MemoryStoreEvent>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && prefix_len == 0 => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(Error::io(stage, error)),
    };
    let metadata_len = file
        .metadata()
        .map_err(|error| Error::io(stage, error))?
        .len();
    if prefix_len > metadata_len || prefix_len > max_bytes as u64 {
        return Err(Error::config(
            stage,
            format!(
                "event journal prefix {prefix_len} exceeds physical length {metadata_len} or budget {max_bytes}"
            ),
        ));
    }
    if prefix_len > 0 {
        file.seek(SeekFrom::Start(prefix_len - 1))
            .map_err(|error| Error::io(stage, error))?;
        let mut boundary = [0_u8; 1];
        file.read_exact(&mut boundary)
            .map_err(|error| Error::io(stage, error))?;
        if boundary != [b'\n'] {
            return Err(Error::config(
                stage,
                "event journal prefix must end on an exact JSONL record boundary",
            ));
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|error| Error::io(stage, error))?;
    }
    read_events_from_reader_bounded(
        BufReader::new(file.take(prefix_len)),
        capacity,
        usize::try_from(prefix_len)
            .map_err(|_| store_budget_error("event prefix exceeds platform address space"))?,
        stage,
        stage,
    )
    .map(|(events, _)| events)
}

fn read_events_from_reader_bounded(
    mut reader: impl BufRead,
    capacity: StoreCapacityBudget,
    max_bytes: usize,
    item_stage: &'static str,
    byte_stage: &'static str,
) -> Result<(Vec<MemoryStoreEvent>, usize)> {
    let mut events = Vec::new();
    let mut line = Vec::new();
    let mut consumed = 0_usize;
    loop {
        let buffer = reader
            .fill_buf()
            .map_err(|error| Error::io(byte_stage, error))?;
        if buffer.is_empty() {
            if !line.is_empty() {
                push_bounded_event_line(&mut events, &line, capacity, item_stage, byte_stage)?;
            }
            break;
        }
        let (take, terminated) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(index) => (index.saturating_add(1), true),
            None => (buffer.len(), false),
        };
        consumed = consumed
            .checked_add(take)
            .ok_or_else(|| store_budget_error("event log aggregate byte count overflow"))?;
        let content_len = take.saturating_sub(usize::from(terminated));
        if consumed > max_bytes || line.len().saturating_add(content_len) > max_bytes {
            return Err(Error::config(
                byte_stage,
                "event log line or aggregate bytes exceed operation budget",
            ));
        }
        line.extend_from_slice(&buffer[..content_len]);
        reader.consume(take);
        if terminated {
            push_bounded_event_line(&mut events, &line, capacity, item_stage, byte_stage)?;
            line.clear();
        }
    }
    Ok((events, consumed))
}

fn push_bounded_event_line(
    events: &mut Vec<MemoryStoreEvent>,
    raw: &[u8],
    capacity: StoreCapacityBudget,
    item_stage: &'static str,
    schema_stage: &'static str,
) -> Result<()> {
    if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(());
    }
    if events.len().saturating_add(1) > capacity.event_log_max_items {
        return Err(Error::config(
            item_stage,
            "event items exceed the active aggregate budget",
        ));
    }
    let event: MemoryStoreEvent = serde_json::from_slice(raw)
        .map_err(|error| Error::config(schema_stage, error.to_string()))?;
    event.validate_current_schema(schema_stage)?;
    events.push(event);
    Ok(())
}

fn events_jsonl_bytes(events: &[MemoryStoreEvent]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for event in events {
        serde_json::to_writer(&mut bytes, event)
            .map_err(|error| Error::config("file_store_transaction", error.to_string()))?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn transaction_contains_operation_pair(request: &StoreTransactionRequest) -> bool {
    let namespaces = request
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreEngineMutation::PutJson { namespace, .. } => Some(namespace.as_str()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    namespaces.contains(MEMORY_MUTATION_RECEIPT_NAMESPACE)
        && namespaces.contains(MEMORY_MUTATION_AUDIT_NAMESPACE)
}

#[cfg(not(feature = "nonproduction-replay-harness"))]
const fn transaction_contains_operation_pair(_request: &StoreTransactionRequest) -> bool {
    false
}

impl StoreEngine for FileStoreEngine {
    fn admission_authority(&self) -> &StoreAdmissionAuthority {
        &self.admission_authority
    }

    fn read_metric_events(
        &self,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreMetricEventSourceRead> {
        self.with_shared_backend("runtime_metrics_event_store", || {
            read_metric_events_from_root(&self.root, capacity)
        })
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn store_capacity(&self) -> StoreCapacityBudget {
        self.capacity
    }

    fn commit_transaction_admitted(
        &self,
        request: &StoreTransactionRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<StoreTransactionReport> {
        let _lock = self.acquire_backend_lock(true, "memory_write_transaction")?;
        self.recover_transaction_if_needed()?;
        admission.validate_inside_engine_fence(self.capacity, &self.admission_authority)?;
        let (context, event_prefix_len) =
            self.load_transaction_context(request, admission.operation_capacity())?;
        let plan = apply_transaction(admission, request, &context)?;
        let appended_events = plan
            .effective_request
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreEngineMutation::AppendEvent { event } => Some((**event).clone()),
                _ => None,
            })
            .collect();
        let before = Self::transaction_image(
            &context.touched,
            request.read_set(),
            FileTransactionEventsImage::Append {
                prefix_len: event_prefix_len,
                events: Vec::new(),
            },
        );
        let after = Self::transaction_image(
            &plan.next_touched,
            request.read_set(),
            FileTransactionEventsImage::Append {
                prefix_len: event_prefix_len,
                events: appended_events,
            },
        );
        let mut journal = FileTransactionJournal::new(
            plan.effective_request.transaction_id.clone(),
            before,
            after,
        )?;
        self.write_transaction_journal(&journal)?;
        let contains_operation_pair = transaction_contains_operation_pair(&plan.effective_request);
        self.maybe_crash_for_recovery_contract(
            "after_prepare_before_apply",
            contains_operation_pair,
        );
        self.maybe_pause_for_recovery_contract("after_prepare_before_apply");

        self.restore_transaction_image(&journal.after)?;
        self.maybe_crash_for_recovery_contract(
            "after_apply_before_commit",
            contains_operation_pair,
        );
        self.maybe_pause_for_recovery_contract("after_apply_before_commit");

        journal.state = FileTransactionJournalState::Committed;
        journal.refresh_checksum()?;
        self.write_transaction_journal(&journal)?;
        self.maybe_crash_for_recovery_contract(
            "after_commit_before_cleanup",
            contains_operation_pair,
        );

        self.remove_transaction_journal("file_store_transaction")?;
        Ok(plan.report)
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

    fn read_consistent_known_keys(
        &self,
        json_keys: &[(String, String)],
        blob_keys: &[(String, String)],
        include_events: bool,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreBoundedKnownKeyReadResult> {
        let requested_entries = json_keys.len().saturating_add(blob_keys.len());
        if requested_entries > capacity.kv_max_entries {
            return Err(Error::config(
                "store_consistent_read_budget_exceeded",
                format!(
                    "requested entries {requested_entries} exceed {}",
                    capacity.kv_max_entries
                ),
            ));
        }
        let mut addresses = BTreeSet::new();
        for (kind, keys) in [("json", json_keys), ("blob", blob_keys)] {
            for (namespace, key) in keys {
                enforce_logical_key_budget(capacity, namespace, key, "store_consistent_read")?;
                if !addresses.insert((kind, namespace.as_str(), key.as_str())) {
                    return Err(Error::config(
                        "store_consistent_read",
                        format!("duplicate {kind} known-key address {namespace}/{key}"),
                    ));
                }
            }
        }
        self.with_shared_backend("store_consistent_read", || {
            let mut json = BTreeMap::new();
            let mut json_bytes = 0_usize;
            for (namespace, key) in json_keys {
                let remaining = capacity.snapshot_max_bytes.saturating_sub(json_bytes);
                if let Some((value, bytes)) =
                    self.get_json_value_unlocked_bounded(namespace, key, remaining)?
                {
                    json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                        store_budget_error("consistent known-key JSON byte count overflow")
                    })?;
                    json.insert((namespace.clone(), key.clone()), value);
                }
            }
            let mut blobs = BTreeMap::new();
            let mut blob_bytes = 0_usize;
            for (namespace, key) in blob_keys {
                let remaining = capacity.blob_max_bytes.saturating_sub(blob_bytes);
                if let Some((value, bytes)) =
                    self.get_blob_unlocked_bounded(namespace, key, remaining)?
                {
                    blob_bytes = blob_bytes.checked_add(bytes).ok_or_else(|| {
                        store_budget_error("consistent known-key blob byte count overflow")
                    })?;
                    blobs.insert((namespace.clone(), key.clone()), value);
                }
            }
            let events = if include_events {
                self.read_events_unlocked_bounded(capacity, json_bytes)?
            } else {
                Vec::new()
            };
            read_bounded_known_keys_from_parts(
                json_keys,
                blob_keys,
                include_events,
                capacity,
                &json,
                &blobs,
                &events,
            )
        })
    }

    fn open_immutable_read_session<'a>(
        &'a self,
        capacity: StoreCapacityBudget,
    ) -> Result<Box<dyn StoreImmutableReadSession + 'a>> {
        validate_immutable_read_session_capacity(self.capacity, capacity)?;
        Ok(Box::new(FileImmutableReadSession {
            engine: self,
            _lock: self.acquire_immutable_read_lock()?,
            read: StoreReadSessionState::new(capacity),
        }))
    }

    fn read_scoped_projection(
        &self,
        request: &crate::StoreScopedProjectionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<crate::StoreScopedProjection> {
        self.with_shared_backend("store_scoped_projection", || {
            let scoped_json = self.read_scoped_json_unlocked_exact(request, capacity)?;
            let events = if request.include_events {
                self.read_scoped_events_unlocked_bounded(
                    &request.scope,
                    capacity,
                    scoped_json.logical_bytes,
                )?
            } else {
                Vec::new()
            };
            read_scoped_projection_from_parts(request, capacity, &scoped_json.documents, &events)
        })
    }

    fn replace_scoped_projection(
        &self,
        request: &crate::StoreScopedProjectionReplaceRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<crate::StoreScopedProjectionReplaceReport> {
        let _lock = self.acquire_backend_lock(true, "store_scoped_projection")?;
        self.recover_transaction_if_needed()?;
        admission.validate_inside_engine_fence(self.capacity, &self.admission_authority)?;

        let projection_request = crate::StoreScopedProjectionRequest {
            scope: request.scope.clone(),
            json_namespaces: request.json_namespaces.clone(),
            include_events: false,
        };
        let mut deleted = self
            .read_scoped_json_unlocked_exact(&projection_request, admission.operation_capacity())?;

        for doc in &request.json_docs {
            if let Some(existing) = self.get_json_value_unlocked(&doc.namespace, &doc.key)? {
                if !deleted
                    .documents
                    .contains_key(&(doc.namespace.clone(), doc.key.clone()))
                {
                    return Err(Error::config(
                        "store_scoped_projection",
                        format!(
                            "replacement address {}/{} is owned by another projection scope",
                            doc.namespace, doc.key
                        ),
                    ));
                }
                deleted
                    .documents
                    .entry((doc.namespace.clone(), doc.key.clone()))
                    .or_insert(existing);
            }
        }

        let existing_events = self.read_events_unlocked()?;
        let deleted_events = existing_events
            .iter()
            .filter(|event| {
                crate::store_internal::transaction::event_matches_scoped_projection(
                    event,
                    &request.scope,
                )
            })
            .count();
        let mut next_events = existing_events
            .iter()
            .filter(|event| {
                !crate::store_internal::transaction::event_matches_scoped_projection(
                    event,
                    &request.scope,
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        next_events.extend(request.events.iter().cloned());
        let mut event_ids = BTreeSet::new();
        if next_events
            .iter()
            .any(|event| !event_ids.insert(event.event_id.as_str()))
        {
            return Err(Error::config(
                "store_scoped_projection",
                "replacement would create a duplicate event id",
            ));
        }

        let next_entries = self
            .json_entry_count()?
            .saturating_add(self.blob_entry_count()?)
            .saturating_sub(deleted.documents.len())
            .saturating_add(request.json_docs.len());
        validate_scoped_projection_post_image(
            admission,
            request,
            next_entries,
            std::iter::once(self.blob_total_bytes()?),
            std::iter::empty(),
            next_events.len(),
        )?;

        let mut touched = deleted.documents.keys().cloned().collect::<BTreeSet<_>>();
        touched.extend(
            request
                .json_docs
                .iter()
                .map(|doc| (doc.namespace.clone(), doc.key.clone())),
        );
        let before = FileTransactionImage {
            json: touched
                .iter()
                .map(|(namespace, key)| FileTransactionJsonValue {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: deleted
                        .documents
                        .get(&(namespace.clone(), key.clone()))
                        .cloned(),
                })
                .collect(),
            blobs: Vec::new(),
            events: FileTransactionEventsImage::Replace {
                events: existing_events,
            },
        };
        let replacements = request
            .json_docs
            .iter()
            .map(|doc| ((doc.namespace.clone(), doc.key.clone()), doc.value.clone()))
            .collect::<BTreeMap<_, _>>();
        let after = FileTransactionImage {
            json: touched
                .iter()
                .map(|(namespace, key)| FileTransactionJsonValue {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: replacements.get(&(namespace.clone(), key.clone())).cloned(),
                })
                .collect(),
            blobs: Vec::new(),
            events: FileTransactionEventsImage::Replace {
                events: next_events,
            },
        };
        let physical_owner = match &request.scope.physical_owning_scope {
            crate::StorePhysicalOwningScope::Subject { mounted_subject_id } => {
                format!("subject_{mounted_subject_id}")
            }
            crate::StorePhysicalOwningScope::SharedProgram => "shared_program".to_string(),
        };
        let transaction_id = format!(
            "scoped_projection_{}_{}",
            request.scope.memory_space_id, physical_owner
        );
        let mut journal = FileTransactionJournal::new(transaction_id, before, after)?;
        self.write_transaction_journal(&journal)?;
        self.restore_transaction_image(&journal.after)?;
        journal.state = FileTransactionJournalState::Committed;
        journal.refresh_checksum()?;
        self.write_transaction_journal(&journal)?;
        self.remove_transaction_journal("store_scoped_projection")?;

        Ok(crate::StoreScopedProjectionReplaceReport {
            admission_report_id: admission.report_id().to_string(),
            deleted_json: deleted.documents.len(),
            inserted_json: request.json_docs.len(),
            deleted_events,
            inserted_events: request.events.len(),
        })
    }

    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        self.with_shared_backend("file_store_json_read", || {
            self.get_json_value_unlocked(namespace, key)
        })
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        self.with_exclusive_backend("file_store_json_write", || {
            self.put_json_value_unlocked(namespace, key, &value)
        })
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        self.with_exclusive_backend("file_store_json_delete", || {
            self.delete_json_value_unlocked(namespace, key)
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

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        self.with_exclusive_backend("file_store_blob_write", || {
            self.put_blob_unlocked(namespace, key, value)
        })
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        self.with_exclusive_backend("file_store_blob_delete", || {
            self.delete_blob_unlocked(namespace, key)
        })
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        self.with_shared_backend("file_store_list", || {
            self.list_blob_keys_unlocked(namespace)
        })
    }

    #[cfg(feature = "nonproduction-replay-harness")]
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
    #[cfg(feature = "nonproduction-replay-harness")]
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

    #[cfg(feature = "nonproduction-replay-harness")]
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

    #[cfg(feature = "nonproduction-replay-harness")]
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

fn contains_persistent_file(root: &Path) -> Result<bool> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(Error::io("file_store_manifest", error)),
    };
    for entry in entries {
        let entry = entry.map_err(|error| Error::io("file_store_manifest", error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| Error::io("file_store_manifest", error))?;
        let path = entry.path();
        if file_type.is_dir() {
            if contains_persistent_file(&path)? {
                return Ok(true);
            }
        } else if path.extension().and_then(|value| value.to_str()) != Some("tmp") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_directory_entries_strict_bounded(
    root: &Path,
    max_entries: usize,
    stage: &'static str,
) -> Result<Vec<fs::DirEntry>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io(stage, error)),
    };
    let mut bounded = Vec::with_capacity(max_entries.min(256));
    for entry in entries {
        if bounded.len() >= max_entries {
            return Err(store_budget_error(format!(
                "{stage} directory entries exceed {max_entries}"
            )));
        }
        bounded.push(entry.map_err(|error| Error::io(stage, error))?);
    }
    let mut entries = bounded;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn entry_name_utf8(entry: &fs::DirEntry, stage: &'static str) -> Result<String> {
    entry
        .file_name()
        .into_string()
        .map_err(|_| Error::config(stage, "file store path is not valid UTF-8"))
}

fn is_orphan_tmp_path(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("tmp")
}

fn list_child_directory_names(root: &Path, stage: &'static str) -> Result<Vec<String>> {
    list_child_directory_names_bounded(root, usize::MAX, stage)
}

fn list_child_directory_names_bounded(
    root: &Path,
    max_entries: usize,
    stage: &'static str,
) -> Result<Vec<String>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io(stage, error)),
    };
    let mut names = Vec::new();
    for entry in entries {
        if names.len() >= max_entries {
            return Err(store_budget_error(format!(
                "{stage} namespace entries exceed {max_entries}"
            )));
        }
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

#[cfg(feature = "nonproduction-replay-harness")]
fn namespace_set(namespaces: &[&str]) -> BTreeSet<String> {
    namespaces
        .iter()
        .map(|namespace| (*namespace).to_string())
        .collect()
}

#[cfg(feature = "nonproduction-replay-harness")]
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

#[cfg(feature = "nonproduction-replay-harness")]
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

#[cfg(feature = "nonproduction-replay-harness")]
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

#[cfg(feature = "nonproduction-replay-harness")]
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
    let mut sequence = DURABILITY_TRACE_SEQUENCE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = *sequence;
    *sequence = sequence.wrapping_add(1);
    drop(sequence);
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

#[cfg(feature = "nonproduction-replay-harness")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store_internal::recall_index::ArchiveRecallManifest;
    use serde_json::json;

    fn file_test_profile() -> crate::ProfileId {
        #[cfg(target_os = "macos")]
        return crate::ProfileId::DesktopMacosEmbeddedSdk;
        #[cfg(target_os = "windows")]
        return crate::ProfileId::DesktopWindowsEmbeddedSdk;
        #[cfg(target_os = "linux")]
        return crate::ProfileId::DesktopLinuxEmbeddedSdk;
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        compile_error!("file store tests require a supported production host target");
    }

    fn event_budget(snapshot_max_bytes: usize, event_log_max_items: usize) -> StoreCapacityBudget {
        StoreCapacityBudget {
            metric_source_max_items: 1,
            event_log_max_items,
            kv_max_entries: 8,
            blob_max_bytes: 1024,
            snapshot_max_bytes,
            logical_namespace_max_bytes: 128,
            logical_key_max_bytes: 128,
            event_record_key_max_bytes: 128,
            export_max_bytes: 1024,
            import_max_bytes: 1024,
        }
    }

    #[test]
    fn event_jsonl_streaming_budget_accepts_exact_and_rejects_plus_one() {
        let path = std::env::temp_dir().join(format!(
            "bm-event-jsonl-budget-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let event = MemoryStoreEvent::new(
            "event-1",
            crate::MemoryStoreEventKind::MemoryWrite,
            crate::StoreEventScope::new("agent", "owner", "channel", "chat")
                .with_memory_space("space")
                .with_subject("subject"),
            7,
        );
        let mut bytes = serde_json::to_vec(&event).expect("serialize event");
        bytes.push(b'\n');
        fs::write(&path, &bytes).expect("write event fixture");

        let exact = read_events_jsonl_bounded(
            &path,
            event_budget(bytes.len(), 1),
            bytes.len(),
            "event_budget_test",
        )
        .expect("exact byte and item budget");
        assert_eq!(exact, vec![event]);
        assert!(read_events_jsonl_bounded(
            &path,
            event_budget(bytes.len().saturating_sub(1), 1),
            bytes.len().saturating_sub(1),
            "event_budget_test",
        )
        .is_err());
        assert!(read_events_jsonl_bounded(
            &path,
            event_budget(bytes.len(), 0),
            bytes.len(),
            "event_budget_test",
        )
        .is_err());
        fs::remove_file(path).expect("remove event fixture");
    }

    #[test]
    fn legacy_manifest_is_rejected_before_file_store_mutation() {
        let root = std::env::temp_dir().join(format!(
            "bm-file-v5-zero-mutation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create fixture root");
        let profile = file_test_profile();
        let config = StoreBackendConfig::file(&root, profile).expect("file store config");
        let mut legacy = StoreSchemaManifest::new(config.backend, config.profile, 7);
        legacy.schema_id = "beetle_memory_store_schema_v5".to_string();
        legacy.schema_version = 5;
        let manifest_path = root.join("manifest.json");
        let before = serde_json::to_vec_pretty(&legacy).expect("legacy manifest");
        fs::write(&manifest_path, &before).expect("write legacy manifest");

        let error = match FileStoreEngine::open_with_capacity(&config, event_budget(4096, 8)) {
            Ok(_) => panic!("v5 must fail closed"),
            Err(error) => error,
        };
        assert_eq!(error.stage(), "file_store_manifest");
        assert_eq!(fs::read(&manifest_path).expect("manifest remains"), before);
        assert!(!root.join(TRANSACTION_LOCK_FILE).exists());
        assert!(!root.join("events").exists());
        assert!(!root.join("kv").exists());
        assert!(!root.join("blob").exists());
        assert!(!root.join("snapshots").exists());

        fs::remove_dir_all(root).expect("remove file fixture");
    }

    #[test]
    fn scoped_projection_excludes_soul_and_uses_exact_typed_subject_root_under_budgets() {
        let root = std::env::temp_dir().join(format!(
            "bm-file-scoped-exact-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let profile = file_test_profile();
        let config = StoreBackendConfig::file(&root, profile).expect("file store config");
        let store_capacity = StoreCapacityBudget {
            metric_source_max_items: 1,
            event_log_max_items: 128,
            kv_max_entries: 128,
            blob_max_bytes: 1024,
            snapshot_max_bytes: 256 * 1024,
            logical_namespace_max_bytes: 128,
            logical_key_max_bytes: 128,
            event_record_key_max_bytes: 128,
            export_max_bytes: 256 * 1024,
            import_max_bytes: 256 * 1024,
        };
        let (engine, _, _) =
            FileStoreEngine::open_with_capacity(&config, store_capacity).expect("open file store");
        let soul = json!({"identity_anchor": "target Soul"});
        engine
            .put_json_value_unlocked("self_authored_core", "subject-target", &soul)
            .expect("write target owner");
        let soul_request = crate::StoreScopedProjectionRequest {
            scope: crate::StoreScopedProjectionScope::subject("space-target", "subject-target")
                .expect("projection scope"),
            json_namespaces: vec!["self_authored_core".to_string()],
            include_events: false,
        };
        let excluded = engine
            .read_scoped_json_unlocked_exact(&soul_request, store_capacity)
            .expect("subject-global Soul namespace is excluded from memory-space projection");
        assert!(excluded.documents.is_empty());
        assert_eq!(excluded.logical_bytes, 0);

        let target_manifest =
            ArchiveRecallManifest::build(1, "space-target", "subject-target", std::iter::empty())
                .expect("target archive recall manifest");
        let target_key = target_manifest.physical_key.clone();
        let target = serde_json::to_value(target_manifest).expect("target manifest json");
        engine
            .put_json_value_unlocked("archive_recall_manifests", &target_key, &target)
            .expect("write target typed root");
        for index in 0..64 {
            let subject_id = format!("unrelated-subject-{index}");
            let manifest =
                ArchiveRecallManifest::build(1, "space-target", &subject_id, std::iter::empty())
                    .expect("unrelated archive recall manifest");
            let manifest_key = manifest.physical_key.clone();
            engine
                .put_json_value_unlocked(
                    "archive_recall_manifests",
                    &manifest_key,
                    &serde_json::to_value(manifest).expect("unrelated manifest json"),
                )
                .expect("write unrelated typed root");
        }
        let request = crate::StoreScopedProjectionRequest {
            scope: crate::StoreScopedProjectionScope::subject("space-target", "subject-target")
                .expect("projection scope"),
            json_namespaces: vec!["archive_recall_manifests".to_string()],
            include_events: false,
        };
        let exact_bytes = serde_json::to_vec(&target).expect("target bytes").len();
        let exact_capacity = StoreCapacityBudget {
            kv_max_entries: 1,
            snapshot_max_bytes: exact_bytes,
            ..store_capacity
        };
        let exact = engine
            .read_scoped_json_unlocked_exact(&request, exact_capacity)
            .expect("exact entry and byte budget");
        assert_eq!(exact.documents.len(), 1);
        assert_eq!(exact.logical_bytes, exact_bytes);
        assert_eq!(
            exact
                .documents
                .get(&("archive_recall_manifests".to_string(), target_key,)),
            Some(&target)
        );

        let entry_plus_one = StoreCapacityBudget {
            kv_max_entries: 0,
            ..exact_capacity
        };
        assert!(engine
            .read_scoped_json_unlocked_exact(&request, entry_plus_one)
            .is_err());
        let byte_plus_one = StoreCapacityBudget {
            snapshot_max_bytes: exact_bytes.saturating_sub(1),
            ..exact_capacity
        };
        assert!(engine
            .read_scoped_json_unlocked_exact(&request, byte_plus_one)
            .is_err());

        drop(engine);
        fs::remove_dir_all(root).expect("remove file store fixture");
    }
}
