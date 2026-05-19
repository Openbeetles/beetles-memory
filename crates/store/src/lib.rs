//! Store contracts and local store backends for Beetle Memory.

#[cfg(feature = "sqlite")]
mod sqlite;

use std::{
    fmt, fs,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use bm_core::{MemoryRecord, NewMemoryRecord};
use serde::{Deserialize, Serialize};

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStore;

pub const STORE_SCHEMA_VERSION: u32 = 2;

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreErrorKind {
    Io,
    Json,
    UnsupportedSchemaVersion,
    CorruptEventLog,
    SnapshotMismatch,
    BackendUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOperation {
    OpenBackend,
    LoadManifest,
    WriteManifest,
    LoadSnapshot,
    WriteSnapshot,
    ReadEventLog,
    AppendEvent,
    ReadRecords,
    InsertRecord,
    ReplaceRecord,
    DeleteRecord,
}

impl StoreOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenBackend => "open_backend",
            Self::LoadManifest => "load_manifest",
            Self::WriteManifest => "write_manifest",
            Self::LoadSnapshot => "load_snapshot",
            Self::WriteSnapshot => "write_snapshot",
            Self::ReadEventLog => "read_event_log",
            Self::AppendEvent => "append_event",
            Self::ReadRecords => "read_records",
            Self::InsertRecord => "insert_record",
            Self::ReplaceRecord => "replace_record",
            Self::DeleteRecord => "delete_record",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreError {
    pub kind: StoreErrorKind,
    pub operation: StoreOperation,
    pub path: Option<String>,
    pub message: String,
    pub recoverable: bool,
}

impl StoreError {
    pub fn new(
        kind: StoreErrorKind,
        operation: StoreOperation,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation,
            path: None,
            message: message.into(),
            recoverable: true,
        }
    }

    pub fn path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().display().to_string());
        self
    }

    pub fn recoverable(mut self, recoverable: bool) -> Self {
        self.recoverable = recoverable;
        self
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(
                f,
                "{:?} during {} at {}: {}",
                self.kind,
                self.operation.as_str(),
                path,
                self.message
            ),
            None => write!(
                f,
                "{:?} during {}: {}",
                self.kind,
                self.operation.as_str(),
                self.message
            ),
        }
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreSnapshotReport {
    pub schema_version: u32,
    pub snapshot_event_seq: u64,
    pub record_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreHealthReport {
    pub backend: &'static str,
    pub healthy: bool,
    pub record_count: usize,
    pub last_event_seq: u64,
    pub snapshot_event_seq: u64,
}

pub trait MemoryStore {
    fn insert(&mut self, record: NewMemoryRecord) -> StoreResult<MemoryRecord>;
    fn replace(&mut self, record: MemoryRecord) -> StoreResult<MemoryRecord>;
    fn delete(&mut self, record_id: &str) -> StoreResult<bool>;
    fn records(&self) -> StoreResult<Vec<MemoryRecord>>;
    fn snapshot(&mut self) -> StoreResult<StoreSnapshotReport>;
    fn health(&self) -> StoreHealthReport;
}

#[derive(Clone, Debug)]
pub struct InMemoryStore {
    records: Vec<MemoryRecord>,
    next_id: u64,
    last_event_seq: u64,
    snapshot_event_seq: u64,
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            next_id: 1,
            last_event_seq: 0,
            snapshot_event_seq: 0,
        }
    }
}

impl MemoryStore for InMemoryStore {
    fn insert(&mut self, record: NewMemoryRecord) -> StoreResult<MemoryRecord> {
        let stored = MemoryRecord {
            id: format!("mem-{}", self.next_id),
            identity: record.identity,
            scope: record.scope,
            content: record.content,
            source: record.source,
            domain: record.domain,
            plane: record.plane,
            meta: record.meta,
        };
        self.next_id += 1;
        self.last_event_seq += 1;
        self.records.push(stored.clone());
        Ok(stored)
    }

    fn replace(&mut self, record: MemoryRecord) -> StoreResult<MemoryRecord> {
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.id == record.id)
        {
            *existing = record.clone();
            self.last_event_seq += 1;
            Ok(record)
        } else {
            Ok(record)
        }
    }

    fn delete(&mut self, record_id: &str) -> StoreResult<bool> {
        let before = self.records.len();
        self.records.retain(|record| record.id != record_id);
        let deleted = self.records.len() != before;
        if deleted {
            self.last_event_seq += 1;
        }
        Ok(deleted)
    }

    fn records(&self) -> StoreResult<Vec<MemoryRecord>> {
        Ok(self.records.clone())
    }

    fn snapshot(&mut self) -> StoreResult<StoreSnapshotReport> {
        self.snapshot_event_seq = self.last_event_seq;
        Ok(StoreSnapshotReport {
            schema_version: STORE_SCHEMA_VERSION,
            snapshot_event_seq: self.snapshot_event_seq,
            record_count: self.records.len(),
        })
    }

    fn health(&self) -> StoreHealthReport {
        StoreHealthReport {
            backend: "memory",
            healthy: true,
            record_count: self.records.len(),
            last_event_seq: self.last_event_seq,
            snapshot_event_seq: self.snapshot_event_seq,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileStore {
    root: PathBuf,
    records: Vec<MemoryRecord>,
    next_id: u64,
    last_event_seq: u64,
    snapshot_event_seq: u64,
}

impl FileStore {
    pub fn open(root: impl AsRef<Path>) -> StoreResult<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|err| {
            StoreError::new(
                StoreErrorKind::Io,
                StoreOperation::OpenBackend,
                err.to_string(),
            )
            .path(&root)
        })?;

        let manifest = load_manifest(&root)?;
        validate_schema(
            manifest.schema_version,
            StoreOperation::LoadManifest,
            manifest_path(&root),
        )?;

        let snapshot = load_snapshot(&root)?;
        let mut records = snapshot
            .as_ref()
            .map(|snapshot| snapshot.records.clone())
            .unwrap_or_default();
        let snapshot_event_seq = snapshot
            .as_ref()
            .map(|snapshot| snapshot.snapshot_event_seq)
            .unwrap_or_default();

        if let Some(snapshot) = &snapshot {
            validate_schema(
                snapshot.schema_version,
                StoreOperation::LoadSnapshot,
                snapshot_path(&root),
            )?;
            if snapshot.snapshot_event_seq > manifest.last_event_seq {
                return Err(StoreError::new(
                    StoreErrorKind::SnapshotMismatch,
                    StoreOperation::LoadSnapshot,
                    "snapshot event seq is ahead of manifest",
                )
                .path(snapshot_path(&root))
                .recoverable(false));
            }
        }

        let replayed_last_event_seq = replay_events(&root, snapshot_event_seq, &mut records)?;
        let last_event_seq = manifest.last_event_seq.max(replayed_last_event_seq);
        let next_id = manifest.next_id.max(next_id_after(&records));

        let store = Self {
            root,
            records,
            next_id,
            last_event_seq,
            snapshot_event_seq,
        };
        store.write_manifest()?;
        Ok(store)
    }

    fn manifest(&self) -> StoreManifest {
        StoreManifest {
            schema_version: STORE_SCHEMA_VERSION,
            next_id: self.next_id,
            last_event_seq: self.last_event_seq,
            snapshot_event_seq: self.snapshot_event_seq,
        }
    }

    fn write_manifest(&self) -> StoreResult<()> {
        write_json_atomic(
            manifest_path(&self.root),
            &self.manifest(),
            StoreOperation::WriteManifest,
        )
    }
}

impl MemoryStore for FileStore {
    fn insert(&mut self, record: NewMemoryRecord) -> StoreResult<MemoryRecord> {
        let stored = MemoryRecord {
            id: format!("mem-{}", self.next_id),
            identity: record.identity,
            scope: record.scope,
            content: record.content,
            source: record.source,
            domain: record.domain,
            plane: record.plane,
            meta: record.meta,
        };
        let seq = self.last_event_seq + 1;
        append_event(&self.root, StoreEvent::record_inserted(seq, stored.clone()))?;

        self.records.push(stored.clone());
        self.next_id += 1;
        self.last_event_seq = seq;
        self.write_manifest()?;
        Ok(stored)
    }

    fn replace(&mut self, record: MemoryRecord) -> StoreResult<MemoryRecord> {
        let seq = self.last_event_seq + 1;
        append_event(&self.root, StoreEvent::record_replaced(seq, record.clone()))?;
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.id == record.id)
        {
            *existing = record.clone();
        } else {
            self.records.push(record.clone());
        }
        self.last_event_seq = seq;
        self.write_manifest()?;
        Ok(record)
    }

    fn delete(&mut self, record_id: &str) -> StoreResult<bool> {
        let before = self.records.len();
        self.records.retain(|record| record.id != record_id);
        let deleted = self.records.len() != before;
        if deleted {
            let seq = self.last_event_seq + 1;
            append_event(
                &self.root,
                StoreEvent::record_deleted(seq, record_id.to_owned()),
            )?;
            self.last_event_seq = seq;
            self.write_manifest()?;
        }
        Ok(deleted)
    }

    fn records(&self) -> StoreResult<Vec<MemoryRecord>> {
        Ok(self.records.clone())
    }

    fn snapshot(&mut self) -> StoreResult<StoreSnapshotReport> {
        self.snapshot_event_seq = self.last_event_seq;
        write_json_atomic(
            snapshot_path(&self.root),
            &StoreSnapshot {
                schema_version: STORE_SCHEMA_VERSION,
                snapshot_event_seq: self.snapshot_event_seq,
                records: self.records.clone(),
            },
            StoreOperation::WriteSnapshot,
        )?;
        self.write_manifest()?;

        Ok(StoreSnapshotReport {
            schema_version: STORE_SCHEMA_VERSION,
            snapshot_event_seq: self.snapshot_event_seq,
            record_count: self.records.len(),
        })
    }

    fn health(&self) -> StoreHealthReport {
        StoreHealthReport {
            backend: "file",
            healthy: true,
            record_count: self.records.len(),
            last_event_seq: self.last_event_seq,
            snapshot_event_seq: self.snapshot_event_seq,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoreManifest {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default = "default_next_id")]
    next_id: u64,
    #[serde(default)]
    last_event_seq: u64,
    #[serde(default)]
    snapshot_event_seq: u64,
}

impl Default for StoreManifest {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            next_id: 1,
            last_event_seq: 0,
            snapshot_event_seq: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoreSnapshot {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    snapshot_event_seq: u64,
    records: Vec<MemoryRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoreEvent {
    seq: u64,
    event_id: String,
    kind: String,
    record_id: String,
    #[serde(default)]
    record: Option<MemoryRecord>,
}

impl StoreEvent {
    fn record_inserted(seq: u64, record: MemoryRecord) -> Self {
        Self {
            seq,
            event_id: format!("evt-{seq}"),
            kind: "record_inserted".to_owned(),
            record_id: record.id.clone(),
            record: Some(record),
        }
    }

    fn record_replaced(seq: u64, record: MemoryRecord) -> Self {
        Self {
            seq,
            event_id: format!("evt-{seq}"),
            kind: "record_replaced".to_owned(),
            record_id: record.id.clone(),
            record: Some(record),
        }
    }

    fn record_deleted(seq: u64, record_id: String) -> Self {
        Self {
            seq,
            event_id: format!("evt-{seq}"),
            kind: "record_deleted".to_owned(),
            record_id,
            record: None,
        }
    }
}

fn load_manifest(root: &Path) -> StoreResult<StoreManifest> {
    let path = manifest_path(root);
    if !path.exists() {
        if store_dir_has_entries(root)? {
            return Err(StoreError::new(
                StoreErrorKind::UnsupportedSchemaVersion,
                StoreOperation::LoadManifest,
                "manifest.json is required for a non-empty store directory",
            )
            .path(path)
            .recoverable(false));
        }
        return Ok(StoreManifest::default());
    }
    read_json(path, StoreOperation::LoadManifest)
}

fn load_snapshot(root: &Path) -> StoreResult<Option<StoreSnapshot>> {
    let path = snapshot_path(root);
    if !path.exists() {
        return Ok(None);
    }
    read_json(path, StoreOperation::LoadSnapshot).map(Some)
}

fn replay_events(
    root: &Path,
    snapshot_event_seq: u64,
    records: &mut Vec<MemoryRecord>,
) -> StoreResult<u64> {
    let path = event_log_path(root);
    if !path.exists() {
        return Ok(snapshot_event_seq);
    }

    let file = File::open(&path).map_err(|err| {
        StoreError::new(
            StoreErrorKind::Io,
            StoreOperation::ReadEventLog,
            err.to_string(),
        )
        .path(&path)
    })?;
    let mut last_seq = snapshot_event_seq;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|err| {
            StoreError::new(
                StoreErrorKind::Io,
                StoreOperation::ReadEventLog,
                err.to_string(),
            )
            .path(&path)
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let event: StoreEvent = serde_json::from_str(&line).map_err(|err| {
            StoreError::new(
                StoreErrorKind::CorruptEventLog,
                StoreOperation::ReadEventLog,
                format!("{} line {}: {err}", file_name(&path), index + 1),
            )
            .path(&path)
            .recoverable(false)
        })?;
        last_seq = last_seq.max(event.seq);
        if event.seq <= snapshot_event_seq {
            continue;
        }
        match event.kind.as_str() {
            "record_inserted" => {
                let record = event_record(&event, &path)?;
                records.push(record);
            }
            "record_replaced" => {
                let record = event_record(&event, &path)?;
                if let Some(existing) = records
                    .iter_mut()
                    .find(|existing| existing.id == event.record_id)
                {
                    *existing = record;
                } else {
                    records.push(record);
                }
            }
            "record_deleted" => {
                records.retain(|record| record.id != event.record_id);
            }
            _ => {
                return Err(StoreError::new(
                    StoreErrorKind::CorruptEventLog,
                    StoreOperation::ReadEventLog,
                    format!("unknown event kind {}", event.kind),
                )
                .path(&path)
                .recoverable(false));
            }
        }
    }
    Ok(last_seq)
}

fn event_record(event: &StoreEvent, path: &Path) -> StoreResult<MemoryRecord> {
    event.record.clone().ok_or_else(|| {
        StoreError::new(
            StoreErrorKind::CorruptEventLog,
            StoreOperation::ReadEventLog,
            format!("event {} is missing record", event.event_id),
        )
        .path(path)
        .recoverable(false)
    })
}

fn append_event(root: &Path, event: StoreEvent) -> StoreResult<()> {
    let path = event_log_path(root);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| {
            StoreError::new(
                StoreErrorKind::Io,
                StoreOperation::AppendEvent,
                err.to_string(),
            )
            .path(&path)
        })?;
    let line = serde_json::to_string(&event).map_err(|err| {
        StoreError::new(
            StoreErrorKind::Json,
            StoreOperation::AppendEvent,
            err.to_string(),
        )
        .path(&path)
    })?;
    writeln!(file, "{line}").map_err(|err| {
        StoreError::new(
            StoreErrorKind::Io,
            StoreOperation::AppendEvent,
            err.to_string(),
        )
        .path(&path)
    })
}

fn read_json<T>(path: PathBuf, operation: StoreOperation) -> StoreResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(&path).map_err(|err| {
        StoreError::new(StoreErrorKind::Io, operation, err.to_string()).path(&path)
    })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        StoreError::new(
            StoreErrorKind::Json,
            operation,
            format!("{}: {err}", file_name(&path)),
        )
        .path(&path)
        .recoverable(false)
    })
}

fn write_json_atomic<T>(path: PathBuf, value: &T, operation: StoreOperation) -> StoreResult<()>
where
    T: Serialize,
{
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|err| {
        StoreError::new(StoreErrorKind::Json, operation, err.to_string()).path(&path)
    })?;
    fs::write(&tmp, bytes).map_err(|err| {
        StoreError::new(StoreErrorKind::Io, operation, err.to_string()).path(&tmp)
    })?;
    fs::rename(&tmp, &path)
        .map_err(|err| StoreError::new(StoreErrorKind::Io, operation, err.to_string()).path(&path))
}

fn validate_schema(version: u32, operation: StoreOperation, path: PathBuf) -> StoreResult<()> {
    if version == 1 || version == STORE_SCHEMA_VERSION {
        return Ok(());
    }
    Err(StoreError::new(
        StoreErrorKind::UnsupportedSchemaVersion,
        operation,
        format!("schema version {version} is not supported"),
    )
    .path(path)
    .recoverable(false))
}

fn next_id_after(records: &[MemoryRecord]) -> u64 {
    records
        .iter()
        .filter_map(|record| record.id.strip_prefix("mem-"))
        .filter_map(|suffix| suffix.parse::<u64>().ok())
        .max()
        .map(|id| id + 1)
        .unwrap_or(1)
}

fn manifest_path(root: &Path) -> PathBuf {
    root.join("manifest.json")
}

fn snapshot_path(root: &Path) -> PathBuf {
    root.join("snapshot.json")
}

fn event_log_path(root: &Path) -> PathBuf {
    root.join("events.jsonl")
}

fn default_schema_version() -> u32 {
    STORE_SCHEMA_VERSION
}

fn default_next_id() -> u64 {
    1
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("store file")
        .to_owned()
}

fn store_dir_has_entries(root: &Path) -> StoreResult<bool> {
    let mut entries = fs::read_dir(root).map_err(|err| {
        StoreError::new(
            StoreErrorKind::Io,
            StoreOperation::LoadManifest,
            err.to_string(),
        )
        .path(root)
    })?;
    Ok(entries.next().is_some())
}
