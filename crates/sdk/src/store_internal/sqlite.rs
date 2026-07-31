use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "nonproduction-replay-harness")]
use std::io::Write;
#[cfg(feature = "nonproduction-replay-harness")]
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;

use bm_core::{Error, Result};
use rusqlite::{params, Connection, ErrorCode, OpenFlags, OptionalExtension, TransactionBehavior};
use serde_json::Value;

#[cfg(feature = "nonproduction-replay-harness")]
use crate::enforce_event_key_budget;
use crate::store_internal::snapshot::StoreSnapshotBlob;
#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::transaction::{
    read_consistent_from_state, validate_restore_post_image_blob_bytes,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::StoreSnapshotReplaceReport;
use crate::{
    enforce_logical_key_budget, store_budget_error,
    store_internal::platform::StoreOpenPreflight,
    store_internal::transaction::{
        apply_transaction, read_bounded_known_keys_from_parts,
        scoped_projection_dependency_addresses, scoped_projection_root_addresses,
        validate_immutable_read_session_capacity, validate_scoped_projection_post_image,
        BackendTransactionState, StoreAdmissionAuthority, StoreBackendUsage,
        StoreBoundedKnownBlobRead, StoreBoundedKnownJsonRead, StoreBoundedKnownKeyReadResult,
        StoreImmutableReadSession, StoreReadReceipt, StoreReadSessionState,
        StoreTransactionAdmission, StoreTransactionContext,
    },
    MemoryStoreEvent, StoreBackendConfig, StoreCapacityBudget, StoreEngine, StoreEngineMutation,
    StoreEventLog, StoreMetricEventSourceRead, StoreSchemaManifest, StoreSnapshot,
    StoreSnapshotJsonDoc, StoreTransactionReport, StoreTransactionRequest, STORE_SCHEMA_ID,
    STORE_SCHEMA_VERSION,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreConsistentReadRequest, StoreConsistentReadResult};

pub struct SqliteStoreEngine {
    capacity: StoreCapacityBudget,
    admission_authority: StoreAdmissionAuthority,
    connection: Mutex<Connection>,
}

const SQLITE_SCHEMA_DDL: &str = r#"
    CREATE TABLE bm_schema (
        schema_id TEXT PRIMARY KEY,
        schema_version INTEGER NOT NULL,
        manifest_json TEXT NOT NULL
    );
    CREATE TABLE bm_event_log (
        sequence INTEGER PRIMARY KEY AUTOINCREMENT,
        event_id TEXT NOT NULL UNIQUE,
        event_json TEXT NOT NULL
    );
    CREATE TABLE bm_kv (
        namespace TEXT NOT NULL,
        key TEXT NOT NULL,
        value_json TEXT NOT NULL,
        PRIMARY KEY(namespace, key)
    );
    CREATE TABLE bm_blob (
        namespace TEXT NOT NULL,
        key TEXT NOT NULL,
        value_blob BLOB NOT NULL,
        PRIMARY KEY(namespace, key)
    );
    CREATE TABLE bm_snapshot_manifest (
        snapshot_id TEXT PRIMARY KEY,
        manifest_json TEXT NOT NULL
    );
"#;

#[derive(Debug, PartialEq, Eq)]
struct SqliteSchemaEntry {
    kind: String,
    name: String,
    table_name: String,
    sql: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SqliteFileIdentity {
    canonical_path: PathBuf,
    length: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    windows: WindowsSqliteFileIdentity,
    #[cfg(not(any(unix, windows)))]
    created_nanos: Option<u128>,
}

impl SqliteFileIdentity {
    fn same_file_object(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.device == other.device && self.inode == other.inode
        }
        #[cfg(windows)]
        {
            self.windows == other.windows
        }
        #[cfg(not(any(unix, windows)))]
        {
            self.canonical_path == other.canonical_path
                && self.created_nanos.is_some()
                && self.created_nanos == other.created_nanos
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct WindowsSqliteFileIdentity {
    volume_serial_number: u64,
    file_id: [u8; 16],
}

struct FreshSqlitePlaceholder {
    path: PathBuf,
    identity: SqliteFileIdentity,
    armed: bool,
}

impl FreshSqlitePlaceholder {
    fn create(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                Error::io(
                    "sqlite_store_open_preflight",
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        std::io::Error::new(
                            error.kind(),
                            "SQLite main database appeared during fresh-store admission",
                        )
                    } else {
                        error
                    },
                )
            })?;
        let identity = sqlite_file_identity_from_open_file(path, &file)?;
        let placeholder = Self {
            path: path.to_path_buf(),
            identity,
            armed: true,
        };
        placeholder.require_current()?;
        Ok(placeholder)
    }

    fn require_current(&self) -> Result<()> {
        require_sqlite_file_identity(&self.path, &self.identity)
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn cleanup_if_current(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let current = sqlite_file_identity(&self.path)?;
        if current
            .as_ref()
            .is_some_and(|current| current.same_file_object(&self.identity))
        {
            std::fs::remove_file(&self.path)
                .map_err(|error| Error::io("sqlite_store_open_preflight", error))?;
        }
        self.disarm();
        Ok(())
    }
}

impl Drop for FreshSqlitePlaceholder {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = self.cleanup_if_current();
    }
}

#[cfg(windows)]
fn sqlite_windows_file_identity(file: &std::fs::File) -> Result<WindowsSqliteFileIdentity> {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };

    let mut info = MaybeUninit::<FILE_ID_INFO>::uninit();
    // SAFETY: file owns a live handle and info has the exact layout required by
    // FileIdInfo.
    let status = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if status == 0 {
        return Err(Error::io(
            "sqlite_store_open_preflight",
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: a successful GetFileInformationByHandleEx initialized info.
    let info = unsafe { info.assume_init() };
    Ok(WindowsSqliteFileIdentity {
        volume_serial_number: info.VolumeSerialNumber,
        file_id: info.FileId.Identifier,
    })
}

fn sqlite_file_identity_from_open_file(
    path: &Path,
    file: &std::fs::File,
) -> Result<SqliteFileIdentity> {
    let metadata = file
        .metadata()
        .map_err(|error| Error::io("sqlite_store_open_preflight", error))?;
    if !metadata.is_file() {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite main database path must be a regular file",
        ));
    }
    #[cfg(windows)]
    let windows = sqlite_windows_file_identity(file)?;
    Ok(SqliteFileIdentity {
        canonical_path: std::fs::canonicalize(path)
            .map_err(|error| Error::io("sqlite_store_open_preflight", error))?,
        length: metadata.len(),
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
        #[cfg(windows)]
        windows,
        #[cfg(not(any(unix, windows)))]
        created_nanos: metadata
            .created()
            .ok()
            .and_then(|created| created.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos()),
    })
}

fn sqlite_file_identity(path: &Path) -> Result<Option<SqliteFileIdentity>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::io("sqlite_store_open_preflight", error)),
    };
    if metadata.file_type().is_symlink() {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite main database path must not be a symbolic link",
        ));
    }
    let file = std::fs::File::open(path)
        .map_err(|error| Error::io("sqlite_store_open_preflight", error))?;
    sqlite_file_identity_from_open_file(path, &file).map(Some)
}

fn require_sqlite_file_identity(path: &Path, expected: &SqliteFileIdentity) -> Result<()> {
    if sqlite_file_identity(path)?.as_ref() != Some(expected) {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite main database identity changed during open admission",
        ));
    }
    Ok(())
}

fn validate_existing_sqlite_file_fence(path: &Path, expected: &SqliteFileIdentity) -> Result<()> {
    // This is a conservative pathname fence, not an OS-level binding between a
    // rusqlite connection and its file handle. Repeat it around every SQLite
    // admission boundary so any observable replacement fails closed.
    require_sqlite_file_identity(path, expected)?;
    validate_sqlite_physical_open_preflight(path)?;
    require_sqlite_file_identity(path, expected)
}

struct ScopedJsonRead {
    documents: BTreeMap<(String, String), Value>,
    logical_bytes: usize,
}

fn decode_current_event(raw: &str, stage: &'static str) -> Result<MemoryStoreEvent> {
    let event: MemoryStoreEvent =
        serde_json::from_str(raw).map_err(|error| Error::config(stage, error.to_string()))?;
    event.validate_current_schema(stage)?;
    Ok(event)
}

fn validate_existing_sqlite_schema_read_only(
    path: &Path,
    config: &StoreBackendConfig,
) -> Result<Option<StoreSchemaManifest>> {
    validate_sqlite_physical_open_preflight(path)?;
    if !path
        .try_exists()
        .map_err(|error| Error::io("sqlite_store_schema", error))?
    {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| Error::storage("sqlite_store_schema", error))?;
    validate_existing_sqlite_schema_connection(&connection, config)
}

fn validate_existing_sqlite_schema_connection(
    connection: &Connection,
    config: &StoreBackendConfig,
) -> Result<Option<StoreSchemaManifest>> {
    let schema_table_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'bm_schema')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| Error::storage("sqlite_store_schema", error))?;
    if !schema_table_exists {
        let table_count: usize = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        if table_count == 0 {
            return Ok(None);
        }
        return Err(Error::config(
            "sqlite_store_schema",
            "schema is missing for a non-empty SQLite store",
        ));
    }
    let mut statement = connection
        .prepare(
            "SELECT schema_id, schema_version, manifest_json FROM bm_schema ORDER BY schema_id",
        )
        .map_err(|error| Error::storage("sqlite_store_schema", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| Error::storage("sqlite_store_schema", error))?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(row.map_err(|error| Error::storage("sqlite_store_schema", error))?);
    }
    if entries.len() != 1 {
        return Err(Error::config(
            "sqlite_store_schema",
            "SQLite store must contain exactly one schema authority row",
        ));
    }
    let (schema_id, schema_version, manifest_json) = entries.pop().expect("checked one row");
    let manifest: StoreSchemaManifest = serde_json::from_str(&manifest_json)
        .map_err(|error| Error::config("sqlite_store_schema", error.to_string()))?;
    if schema_id != manifest.schema_id || schema_version != manifest.schema_version {
        return Err(Error::config(
            "sqlite_store_schema",
            "SQLite schema columns do not match the manifest",
        ));
    }
    manifest.validate_against(
        config.backend,
        config.profile,
        config.memory_system_kind,
        "sqlite_store_schema",
    )?;
    validate_sqlite_schema_inventory(connection)?;
    Ok(Some(manifest))
}

fn validate_sqlite_sidecar_absence(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        match std::fs::symlink_metadata(&sidecar) {
            Ok(_) => {
                return Err(Error::config(
                    "sqlite_store_open_preflight",
                    format!(
                        "SQLite sidecar {} requires an explicit recovery owner",
                        sidecar.display()
                    ),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::io("sqlite_store_open_preflight", error)),
        }
    }
    Ok(())
}

fn validate_sqlite_physical_open_preflight(path: &Path) -> Result<()> {
    validate_sqlite_sidecar_absence(path)?;
    let Some(identity) = sqlite_file_identity(path)? else {
        return Ok(());
    };
    let mut file = std::fs::File::open(path)
        .map_err(|error| Error::io("sqlite_store_open_preflight", error))?;
    let length = file
        .metadata()
        .map_err(|error| Error::io("sqlite_store_open_preflight", error))?
        .len();
    if length != identity.length {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite main database length changed during physical admission",
        ));
    }
    if length == 0 {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "an existing SQLite database must contain a complete v6 header",
        ));
    }
    if length < 100 {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite database header is truncated",
        ));
    }
    let mut header = [0_u8; 100];
    file.read_exact(&mut header)
        .map_err(|error| Error::io("sqlite_store_open_preflight", error))?;
    if &header[..16] != b"SQLite format 3\0" {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite database header signature is invalid",
        ));
    }
    if header[18] != 1 || header[19] != 1 {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "WAL-mode SQLite headers are not admitted by the rollback-journal store contract",
        ));
    }
    Ok(())
}

fn validate_sqlite_integrity_read_only(connection: &Connection) -> Result<()> {
    let mut statement = connection
        .prepare("PRAGMA quick_check")
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    let results = rows
        .map(|row| row.map_err(|error| Error::storage("sqlite_store_open_preflight", error)))
        .collect::<Result<Vec<_>>>()?;
    if results != ["ok"] {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            format!("SQLite quick_check rejected the physical database: {results:?}"),
        ));
    }
    Ok(())
}

fn sqlite_schema_inventory(connection: &Connection) -> Result<Vec<SqliteSchemaEntry>> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql
             FROM sqlite_schema
             WHERE type IN ('table', 'index', 'trigger', 'view')
             ORDER BY type, name, tbl_name",
        )
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok(SqliteSchemaEntry {
                kind: row.get(0)?,
                name: row.get(1)?,
                table_name: row.get(2)?,
                sql: row.get(3)?,
            })
        })
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    rows.map(|row| row.map_err(|error| Error::storage("sqlite_store_open_preflight", error)))
        .collect()
}

fn validate_sqlite_schema_inventory(connection: &Connection) -> Result<()> {
    let canonical = Connection::open_in_memory()
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    canonical
        .execute_batch(SQLITE_SCHEMA_DDL)
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    if sqlite_schema_inventory(connection)? != sqlite_schema_inventory(&canonical)? {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite schema inventory does not match the exact v6 DDL",
        ));
    }
    Ok(())
}

fn sqlite_bounded_row_count(
    connection: &Connection,
    query: &'static str,
    capacity: usize,
    label: &'static str,
) -> Result<usize> {
    let probe_limit = i64::try_from(capacity.saturating_add(1)).unwrap_or(i64::MAX);
    let count = connection
        .query_row(query, params![probe_limit], |row| row.get::<_, usize>(0))
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    if count > capacity {
        return Err(store_budget_error(format!(
            "SQLite preflight {label} items {count} exceed {capacity}"
        )));
    }
    Ok(count)
}

fn sqlite_address_exceeds_budget(
    connection: &Connection,
    query: &'static str,
    first_max_bytes: usize,
    second_max_bytes: usize,
) -> Result<bool> {
    let first_max_bytes = i64::try_from(first_max_bytes).unwrap_or(i64::MAX);
    let second_max_bytes = i64::try_from(second_max_bytes).unwrap_or(i64::MAX);
    connection
        .query_row(query, params![first_max_bytes, second_max_bytes], |row| {
            row.get(0)
        })
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))
}

fn validate_sqlite_snapshot_envelope(
    connection: &Connection,
    capacity: StoreCapacityBudget,
) -> Result<(usize, usize, usize)> {
    let json_count = sqlite_bounded_row_count(
        connection,
        "SELECT COUNT(*) FROM (SELECT 1 FROM bm_kv LIMIT ?1)",
        capacity.kv_max_entries,
        "JSON",
    )?;
    let blob_capacity = capacity.kv_max_entries.saturating_sub(json_count);
    let blob_count = sqlite_bounded_row_count(
        connection,
        "SELECT COUNT(*) FROM (SELECT 1 FROM bm_blob LIMIT ?1)",
        blob_capacity,
        "blob",
    )?;
    let event_count = sqlite_bounded_row_count(
        connection,
        "SELECT COUNT(*) FROM (SELECT 1 FROM bm_event_log LIMIT ?1)",
        capacity.event_log_max_items,
        "event",
    )?;

    if sqlite_address_exceeds_budget(
        connection,
        "SELECT EXISTS(
             SELECT 1 FROM bm_kv
             WHERE length(CAST(namespace AS BLOB)) > ?1
                OR length(CAST(key AS BLOB)) > ?2
             LIMIT 1
         )",
        capacity.logical_namespace_max_bytes,
        capacity.logical_key_max_bytes,
    )? {
        return Err(store_budget_error(
            "SQLite preflight JSON address exceeds its logical byte budget",
        ));
    }
    if sqlite_address_exceeds_budget(
        connection,
        "SELECT EXISTS(
             SELECT 1 FROM bm_blob
             WHERE length(CAST(namespace AS BLOB)) > ?1
                OR length(CAST(key AS BLOB)) > ?2
             LIMIT 1
         )",
        capacity.logical_namespace_max_bytes,
        capacity.logical_key_max_bytes,
    )? {
        return Err(store_budget_error(
            "SQLite preflight blob address exceeds its logical byte budget",
        ));
    }
    let oversized_event_id: bool = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM bm_event_log
                 WHERE length(CAST(event_id AS BLOB)) > ?1
                 LIMIT 1
             )",
            params![i64::try_from(capacity.logical_key_max_bytes).unwrap_or(i64::MAX)],
            |row| row.get(0),
        )
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    if oversized_event_id {
        return Err(store_budget_error(
            "SQLite preflight event id exceeds the logical key byte budget",
        ));
    }

    let snapshot_manifest_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM bm_snapshot_manifest LIMIT 1)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    if snapshot_manifest_exists {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite snapshot manifest lane is not part of the active v6 footprint",
        ));
    }

    Ok((json_count, blob_count, event_count))
}

fn read_existing_sqlite_snapshot_read_only(
    connection: &Connection,
    manifest: StoreSchemaManifest,
    capacity: StoreCapacityBudget,
) -> Result<StoreSnapshot> {
    let (json_count, blob_count, event_count) =
        validate_sqlite_snapshot_envelope(connection, capacity)?;
    let mut json_docs = Vec::with_capacity(json_count);
    let mut json_bytes = 0_usize;
    let mut statement = connection
        .prepare(
            "SELECT namespace, key, length(CAST(value_json AS BLOB)), value_json
             FROM bm_kv ORDER BY namespace, key",
        )
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    while let Some(row) = rows
        .next()
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?
    {
        let namespace = row
            .get::<_, String>(0)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        let key = row
            .get::<_, String>(1)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        let raw_len = row
            .get::<_, usize>(2)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        if raw_len > capacity.snapshot_max_bytes.saturating_sub(json_bytes) {
            return Err(store_budget_error(
                "SQLite preflight JSON value exceeds the remaining snapshot budget",
            ));
        }
        let raw = row
            .get::<_, String>(3)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        if raw.len() != raw_len {
            return Err(Error::config(
                "sqlite_store_open_preflight",
                "SQLite JSON byte length changed during bounded row admission",
            ));
        }
        json_bytes = json_bytes
            .checked_add(raw.len())
            .ok_or_else(|| store_budget_error("SQLite preflight JSON byte count overflow"))?;
        if json_bytes > capacity.snapshot_max_bytes {
            return Err(store_budget_error(format!(
                "SQLite preflight JSON bytes {json_bytes} exceed {}",
                capacity.snapshot_max_bytes
            )));
        }
        let value = serde_json::from_str(&raw)
            .map_err(|error| Error::config("sqlite_store_open_preflight", error.to_string()))?;
        json_docs.push(StoreSnapshotJsonDoc {
            namespace,
            key,
            value,
        });
    }
    drop(rows);
    drop(statement);

    let mut blobs = Vec::with_capacity(blob_count);
    let mut blob_bytes = 0_usize;
    let mut statement = connection
        .prepare(
            "SELECT namespace, key, length(value_blob), value_blob
             FROM bm_blob ORDER BY namespace, key",
        )
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    while let Some(row) = rows
        .next()
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?
    {
        let namespace = row
            .get::<_, String>(0)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        let key = row
            .get::<_, String>(1)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        let value_len = row
            .get::<_, usize>(2)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        if value_len > capacity.blob_max_bytes.saturating_sub(blob_bytes) {
            return Err(store_budget_error(
                "SQLite preflight blob exceeds the remaining blob budget",
            ));
        }
        let value = row
            .get::<_, Vec<u8>>(3)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        if value.len() != value_len {
            return Err(Error::config(
                "sqlite_store_open_preflight",
                "SQLite blob byte length changed during bounded row admission",
            ));
        }
        blob_bytes = blob_bytes
            .checked_add(value.len())
            .ok_or_else(|| store_budget_error("SQLite preflight blob byte count overflow"))?;
        if blob_bytes > capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "SQLite preflight blob bytes {blob_bytes} exceed {}",
                capacity.blob_max_bytes
            )));
        }
        blobs.push(StoreSnapshotBlob {
            namespace,
            key,
            value,
        });
    }
    drop(rows);
    drop(statement);

    let mut events = Vec::with_capacity(event_count);
    let mut statement = connection
        .prepare(
            "SELECT event_id, length(CAST(event_json AS BLOB)), event_json
             FROM bm_event_log ORDER BY sequence",
        )
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
    while let Some(row) = rows
        .next()
        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?
    {
        let event_id = row
            .get::<_, String>(0)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        let raw_len = row
            .get::<_, usize>(1)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        if raw_len > capacity.snapshot_max_bytes.saturating_sub(json_bytes) {
            return Err(store_budget_error(
                "SQLite preflight event bytes exceed snapshot budget",
            ));
        }
        let raw = row
            .get::<_, String>(2)
            .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
        if raw.len() != raw_len {
            return Err(Error::config(
                "sqlite_store_open_preflight",
                "SQLite event byte length changed during bounded row admission",
            ));
        }
        json_bytes = json_bytes.saturating_add(raw_len);
        let event = decode_current_event(&raw, "sqlite_store_open_preflight")?;
        if event.event_id != event_id {
            return Err(Error::config(
                "sqlite_store_open_preflight",
                "SQLite event_id column does not match typed event",
            ));
        }
        events.push(event);
    }
    drop(rows);
    drop(statement);
    if events.len() != event_count {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite event count changed during bounded snapshot admission",
        ));
    }

    if json_docs.len() != json_count || blobs.len() != blob_count {
        return Err(Error::config(
            "sqlite_store_open_preflight",
            "SQLite entry count changed during bounded snapshot admission",
        ));
    }
    Ok(StoreSnapshot::new(manifest, json_docs, blobs, events))
}

fn read_scoped_json_exact(
    connection: &Connection,
    request: &crate::StoreScopedProjectionRequest,
    capacity: StoreCapacityBudget,
) -> Result<ScopedJsonRead> {
    let mut documents = BTreeMap::new();
    let mut logical_bytes = 0_usize;
    let mut observed = BTreeSet::new();
    let mut pending = scoped_projection_root_addresses(&request.json_namespaces, &request.scope)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    while let Some((namespace, key)) = pending.pop_first() {
        if !observed.insert((namespace.clone(), key.clone())) {
            continue;
        }
        if observed.len() > capacity.kv_max_entries {
            return Err(Error::config(
                "store_scoped_projection_budget_exceeded",
                "scoped projection exact-key reads exceed the pinned operation entry budget",
            ));
        }
        let raw = connection
            .query_row(
                "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![&namespace, &key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        let Some(raw) = raw else { continue };
        let value = serde_json::from_str::<Value>(&raw)
            .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?;
        let value_bytes = serde_json::to_vec(&value)
            .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?
            .len();
        logical_bytes = logical_bytes
            .checked_add(value_bytes)
            .ok_or_else(|| store_budget_error("scoped projection JSON byte overflow"))?;
        if logical_bytes > capacity.snapshot_max_bytes {
            return Err(Error::config(
                "store_scoped_projection_budget_exceeded",
                "scoped projection exceeds the pinned operation byte budget",
            ));
        }
        documents.insert((namespace, key), value);
        for dependency in scoped_projection_dependency_addresses(
            &documents,
            &request.json_namespaces,
            &request.scope,
        )? {
            if !observed.contains(&dependency) {
                pending.insert(dependency);
            }
        }
    }
    crate::store_internal::transaction::validate_scoped_recall_manifest_documents(
        &documents,
        &BTreeMap::new(),
        &request.scope,
    )?;
    crate::store_internal::transaction::validate_scoped_control_plane_documents(
        &documents,
        &request.scope,
        capacity.kv_max_entries,
    )?;
    Ok(ScopedJsonRead {
        documents,
        logical_bytes,
    })
}

struct SqliteImmutableReadSession<'a> {
    connection: MutexGuard<'a, Connection>,
    read: StoreReadSessionState,
}

impl Drop for SqliteImmutableReadSession<'_> {
    fn drop(&mut self) {
        let _ = self.connection.execute_batch("ROLLBACK");
    }
}

impl StoreImmutableReadSession for SqliteImmutableReadSession<'_> {
    fn read_json_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownJsonRead>> {
        let mut reads = Vec::with_capacity(addresses.len());
        for (namespace, key) in addresses {
            let raw: Option<String> = match self
                .connection
                .query_row(
                    "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_immutable_read_session", error))
            {
                Ok(raw) => raw,
                Err(error) => return self.read.fail(error),
            };
            let value = match raw
                .map(|raw| {
                    if raw.len() > self.read.remaining_json_bytes() {
                        return Err(Error::config(
                            "store_consistent_read_budget_exceeded",
                            "SQLite JSON value exceeds remaining immutable session ceiling",
                        ));
                    }
                    serde_json::from_str(&raw).map_err(|error| {
                        Error::config("store_immutable_read_session", error.to_string())
                    })
                })
                .transpose()
            {
                Ok(value) => value,
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
            let value: Option<Vec<u8>> = match self
                .connection
                .query_row(
                    "SELECT value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_immutable_read_session", error))
            {
                Ok(value) => value,
                Err(error) => return self.read.fail(error),
            };
            if value
                .as_ref()
                .is_some_and(|value| value.len() > self.read.remaining_blob_bytes())
            {
                return self.read.fail(Error::config(
                    "store_consistent_read_budget_exceeded",
                    "SQLite blob exceeds remaining immutable session ceiling",
                ));
            }
            reads.push(self.read.record_blob(namespace, key, value)?);
        }
        Ok(reads)
    }

    fn receipt(&self) -> Result<StoreReadReceipt> {
        self.read.receipt()
    }
}

impl SqliteStoreEngine {
    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn open_with_capacity(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
    ) -> Result<(Self, StoreSchemaManifest)> {
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
    ) -> Result<(Self, StoreSchemaManifest)> {
        Self::open_internal(config, capacity, admission_authority, Some(open_preflight))
    }

    fn open_internal(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
        admission_authority: StoreAdmissionAuthority,
        open_preflight: Option<&StoreOpenPreflight>,
    ) -> Result<(Self, StoreSchemaManifest)> {
        let path = config
            .data_path
            .clone()
            .ok_or_else(|| Error::config("sqlite_store_open", "sqlite path is required"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::io("sqlite_store_open", error))?;
        }
        validate_sqlite_physical_open_preflight(&path)?;
        let initial_identity = sqlite_file_identity(&path)?;
        let existing_store = initial_identity.is_some();
        let mut fresh_placeholder = if existing_store {
            None
        } else {
            Some(FreshSqlitePlaceholder::create(&path)?)
        };
        let expected_identity = initial_identity
            .clone()
            .or_else(|| {
                fresh_placeholder
                    .as_ref()
                    .map(|placeholder| placeholder.identity.clone())
            })
            .ok_or_else(|| {
                Error::config(
                    "sqlite_store_open_preflight",
                    "SQLite main database identity is missing after admission",
                )
            })?;

        let result = Self::open_claimed_path(
            config,
            capacity,
            admission_authority,
            open_preflight,
            path,
            &expected_identity,
            existing_store,
        );
        match result {
            Ok(result) => {
                if let Some(placeholder) = fresh_placeholder.as_mut() {
                    placeholder.disarm();
                }
                Ok(result)
            }
            Err(error) => {
                if let Some(placeholder) = fresh_placeholder.as_mut() {
                    placeholder.cleanup_if_current()?;
                }
                Err(error)
            }
        }
    }

    fn open_claimed_path(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
        admission_authority: StoreAdmissionAuthority,
        open_preflight: Option<&StoreOpenPreflight>,
        path: PathBuf,
        expected_identity: &SqliteFileIdentity,
        existing_store: bool,
    ) -> Result<(Self, StoreSchemaManifest)> {
        if existing_store {
            validate_existing_sqlite_file_fence(&path, expected_identity)?;
            validate_existing_sqlite_schema_read_only(&path, config)?.ok_or_else(|| {
                Error::config(
                    "sqlite_store_open_preflight",
                    "an existing SQLite file is not a complete v6 store",
                )
            })?;
            validate_existing_sqlite_file_fence(&path, expected_identity)?;
            if let Some(open_preflight) = open_preflight {
                validate_existing_sqlite_file_fence(&path, expected_identity)?;
                let connection =
                    Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .map_err(|error| Error::storage("sqlite_store_open_preflight", error))?;
                validate_existing_sqlite_file_fence(&path, expected_identity)?;
                let manifest = validate_existing_sqlite_schema_connection(&connection, config)?
                    .ok_or_else(|| {
                        Error::config(
                            "sqlite_store_open_preflight",
                            "SQLite schema disappeared during read-only admission",
                        )
                    })?;
                let snapshot =
                    read_existing_sqlite_snapshot_read_only(&connection, manifest, capacity)?;
                open_preflight.admit_snapshot(&snapshot, "sqlite_store_open_preflight")?;
                validate_sqlite_integrity_read_only(&connection)?;
                validate_existing_sqlite_file_fence(&path, expected_identity)?;
            }
        } else {
            validate_sqlite_sidecar_absence(&path)?;
            require_sqlite_file_identity(&path, expected_identity)?;
        }

        if existing_store {
            validate_existing_sqlite_file_fence(&path, expected_identity)?;
        } else {
            validate_sqlite_sidecar_absence(&path)?;
            require_sqlite_file_identity(&path, expected_identity)?;
        }
        let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|error| Error::storage("sqlite_store_open", error))?;
        if existing_store {
            validate_existing_sqlite_file_fence(&path, expected_identity)?;
        } else {
            validate_sqlite_sidecar_absence(&path)?;
            require_sqlite_file_identity(&path, expected_identity)?;
        }
        connection
            .busy_timeout(config.lock_timeout)
            .map_err(|error| Error::storage("sqlite_store_open", error))?;
        let engine = Self {
            capacity,
            admission_authority,
            connection: Mutex::new(connection),
        };
        let manifest = engine.init_schema(
            config,
            path,
            expected_identity,
            existing_store,
            open_preflight,
        )?;
        Ok((engine, manifest))
    }

    fn init_schema(
        &self,
        config: &StoreBackendConfig,
        path: PathBuf,
        expected_identity: &SqliteFileIdentity,
        existing_store: bool,
        open_preflight: Option<&StoreOpenPreflight>,
    ) -> Result<StoreSchemaManifest> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_open", error.to_string()))?;
        if existing_store {
            validate_existing_sqlite_file_fence(&path, expected_identity)?;
        } else {
            validate_sqlite_sidecar_absence(&path)?;
            require_sqlite_file_identity(&path, expected_identity)?;
        }
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        let now_secs = current_unix_secs();
        if existing_store {
            validate_existing_sqlite_file_fence(&path, expected_identity)?;
            let manifest = validate_existing_sqlite_schema_connection(&transaction, config)?
                .ok_or_else(|| {
                    Error::config(
                        "sqlite_store_open_preflight",
                        "SQLite schema disappeared before the write fence",
                    )
                })?;
            if let Some(open_preflight) = open_preflight {
                let snapshot = read_existing_sqlite_snapshot_read_only(
                    &transaction,
                    manifest.clone(),
                    self.capacity,
                )?;
                open_preflight.admit_snapshot(&snapshot, "sqlite_store_open_preflight")?;
            }
            validate_sqlite_integrity_read_only(&transaction)?;
            validate_existing_sqlite_file_fence(&path, expected_identity)?;
            transaction
                .rollback()
                .map_err(|error| Error::storage("sqlite_store_schema", error))?;
            validate_existing_sqlite_file_fence(&path, expected_identity)?;
            return Ok(manifest);
        }

        transaction
            .execute_batch(SQLITE_SCHEMA_DDL)
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        let manifest = StoreSchemaManifest::new(config.backend, config.profile, now_secs);
        let raw = serde_json::to_string(&manifest)
            .map_err(|error| Error::config("sqlite_store_schema", error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO bm_schema(schema_id, schema_version, manifest_json)
                     VALUES (?1, ?2, ?3)",
                params![STORE_SCHEMA_ID, STORE_SCHEMA_VERSION, raw],
            )
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        transaction
            .commit()
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        Ok(manifest)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn ensure_can_insert_event(
        capacity: StoreCapacityBudget,
        connection: &Connection,
        event: &MemoryStoreEvent,
    ) -> Result<()> {
        enforce_event_key_budget(capacity, event, "store_event_log")?;
        let duplicate: Option<String> = connection
            .query_row(
                "SELECT event_id FROM bm_event_log WHERE event_id = ?1",
                params![&event.event_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("store_event_log", error))?;
        if duplicate.is_some() {
            return Err(Error::config("store_event_log", "duplicate event id"));
        }
        let count: usize = connection
            .query_row("SELECT COUNT(*) FROM bm_event_log", [], |row| row.get(0))
            .map_err(|error| Error::storage("store_event_log", error))?;
        if count >= capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                count.saturating_add(1),
                capacity.event_log_max_items
            )));
        }
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn ensure_json_entry_budget(
        capacity: StoreCapacityBudget,
        connection: &Connection,
        namespace: &str,
        key: &str,
    ) -> Result<()> {
        enforce_logical_key_budget(capacity, namespace, key, "sqlite_store_json_write")?;
        let exists: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        if exists.is_some() {
            return Ok(());
        }
        let count: usize = connection
            .query_row("SELECT COUNT(*) FROM bm_kv", [], |row| row.get(0))
            .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        if count >= capacity.kv_max_entries {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                count.saturating_add(1),
                capacity.kv_max_entries
            )));
        }
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn ensure_blob_budget(
        capacity: StoreCapacityBudget,
        connection: &Connection,
        namespace: &str,
        key: &str,
        value_len: usize,
    ) -> Result<()> {
        enforce_logical_key_budget(capacity, namespace, key, "sqlite_store_blob_write")?;
        let previous: Option<usize> = connection
            .query_row(
                "SELECT length(value_blob) FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        let current: usize = connection
            .query_row(
                "SELECT COALESCE(SUM(length(value_blob)), 0) FROM bm_blob",
                [],
                |row| row.get(0),
            )
            .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        let next = current
            .saturating_sub(previous.unwrap_or(0))
            .saturating_add(value_len);
        if next > capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                next, capacity.blob_max_bytes
            )));
        }
        Ok(())
    }

    fn load_transaction_context(
        connection: &Connection,
        request: &StoreTransactionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreTransactionContext> {
        let mut touched = BackendTransactionState::default();
        let mut json_bytes = 0_usize;
        for (namespace, key) in &request.read_set().json {
            enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
            let row: Option<(usize, String)> = connection
                .query_row(
                    "SELECT length(value_json), value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            if let Some((bytes, raw)) = row {
                json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("transaction touched JSON byte count overflow")
                })?;
                if json_bytes > capacity.snapshot_max_bytes {
                    return Err(store_budget_error(format!(
                        "transaction touched JSON bytes {json_bytes} exceed {}",
                        capacity.snapshot_max_bytes
                    )));
                }
                let value = serde_json::from_str(&raw).map_err(|error| {
                    Error::config("memory_write_transaction", error.to_string())
                })?;
                touched.json.insert((namespace.clone(), key.clone()), value);
            }
        }
        for (namespace, prefix) in &request.read_set().json_prefixes {
            enforce_logical_key_budget(capacity, namespace, prefix, "memory_write_transaction")?;
            let mut statement = connection
                .prepare(
                    "SELECT key, length(value_json), value_json FROM bm_kv \
                     WHERE namespace = ?1 AND substr(key, 1, length(?2)) = ?2 ORDER BY key",
                )
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            let mut rows = statement
                .query(params![namespace, prefix])
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            while let Some(row) = rows
                .next()
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?
            {
                let key: String = row
                    .get(0)
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                if touched.json.contains_key(&(namespace.clone(), key.clone())) {
                    continue;
                }
                let bytes: usize = row
                    .get(1)
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                let raw: String = row
                    .get(2)
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("transaction touched JSON byte count overflow")
                })?;
                if json_bytes > capacity.snapshot_max_bytes {
                    return Err(store_budget_error(format!(
                        "transaction touched JSON bytes {json_bytes} exceed {}",
                        capacity.snapshot_max_bytes
                    )));
                }
                let value = serde_json::from_str(&raw).map_err(|error| {
                    Error::config("memory_write_transaction", error.to_string())
                })?;
                touched.json.insert((namespace.clone(), key), value);
            }
        }
        let mut touched_blob_bytes = 0_usize;
        for (namespace, key) in &request.read_set().blobs {
            enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
            let row: Option<(usize, Vec<u8>)> = connection
                .query_row(
                    "SELECT length(value_blob), value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            if let Some((bytes, value)) = row {
                touched_blob_bytes = touched_blob_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("transaction touched blob byte count overflow")
                })?;
                if touched_blob_bytes > capacity.blob_max_bytes {
                    return Err(store_budget_error(format!(
                        "transaction touched blob bytes {touched_blob_bytes} exceed {}",
                        capacity.blob_max_bytes
                    )));
                }
                touched
                    .blobs
                    .insert((namespace.clone(), key.clone()), value);
            }
        }
        let event_ids = request
            .mutations
            .iter()
            .filter_map(crate::store_internal::transaction::mutation_event_id)
            .collect::<BTreeSet<_>>();
        let mut existing_event_ids = BTreeSet::new();
        for event_id in event_ids {
            let exists: Option<String> = connection
                .query_row(
                    "SELECT event_id FROM bm_event_log WHERE event_id = ?1",
                    params![event_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            existing_event_ids.extend(exists);
        }
        let usage = StoreBackendUsage {
            kv_entries: connection
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM bm_kv) + (SELECT COUNT(*) FROM bm_blob)",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?,
            blob_bytes: connection
                .query_row(
                    "SELECT COALESCE(SUM(length(value_blob)), 0) FROM bm_blob",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?,
            event_count: connection
                .query_row("SELECT COUNT(*) FROM bm_event_log", [], |row| row.get(0))
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?,
        };
        Ok(StoreTransactionContext {
            touched,
            usage,
            existing_event_ids,
        })
    }
}

impl StoreEventLog for SqliteStoreEngine {
    #[cfg(feature = "nonproduction-replay-harness")]
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        let raw = serde_json::to_string(&event)
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        Self::ensure_can_insert_event(self.capacity, &connection, &event)?;
        connection
            .execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![event.event_id, raw],
            )
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    Error::config("store_event_log", "duplicate event id")
                } else {
                    Error::storage("store_event_log", error)
                }
            })?;
        Ok(())
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        let count: usize = connection
            .query_row("SELECT COUNT(*) FROM bm_event_log", [], |row| row.get(0))
            .map_err(|error| Error::storage("store_event_log", error))?;
        if count > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                count, self.capacity.event_log_max_items
            )));
        }
        let mut statement = connection
            .prepare("SELECT event_json FROM bm_event_log ORDER BY sequence ASC")
            .map_err(|error| Error::storage("store_event_log", error))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| Error::storage("store_event_log", error))?;
        let mut events = Vec::new();
        for row in rows {
            let raw = row.map_err(|error| Error::storage("store_event_log", error))?;
            events.push(decode_current_event(&raw, "store_event_log")?);
        }
        Ok(events)
    }
}

impl StoreEngine for SqliteStoreEngine {
    fn admission_authority(&self) -> &StoreAdmissionAuthority {
        &self.admission_authority
    }

    fn read_metric_events(
        &self,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreMetricEventSourceRead> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("runtime_metrics_event_store", error.to_string()))?;
        let (count, accounted_snapshot_bytes): (i64, i64) = connection
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_json AS BLOB))), 0) FROM bm_event_log",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| Error::storage("runtime_metrics_event_store", error))?;
        let count = usize::try_from(count).map_err(|_| {
            Error::config(
                "runtime_metrics_event_capacity",
                "runtime metric event count exceeds the platform address space",
            )
        })?;
        let accounted_snapshot_bytes = usize::try_from(accounted_snapshot_bytes).map_err(|_| {
            Error::config(
                "runtime_metrics_event_bytes",
                "runtime metric event bytes exceed the platform address space",
            )
        })?;
        if count > capacity.event_log_max_items {
            return Err(Error::config(
                "runtime_metrics_event_capacity",
                "runtime metric event source exceeds the active item budget",
            ));
        }
        if accounted_snapshot_bytes > capacity.snapshot_max_bytes {
            return Err(Error::config(
                "runtime_metrics_event_bytes",
                "runtime metric event source exceeds the active byte budget",
            ));
        }
        let mut statement = connection
            .prepare("SELECT event_json FROM bm_event_log ORDER BY sequence ASC")
            .map_err(|error| Error::storage("runtime_metrics_event_store", error))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| Error::storage("runtime_metrics_event_store", error))?;
        let mut events = Vec::with_capacity(count);
        let mut observed_bytes = 0_usize;
        for row in rows {
            let raw = row.map_err(|error| Error::storage("runtime_metrics_event_store", error))?;
            observed_bytes = observed_bytes.checked_add(raw.len()).ok_or_else(|| {
                Error::config(
                    "runtime_metrics_event_bytes",
                    "runtime metric event byte count overflow",
                )
            })?;
            events.push(decode_current_event(&raw, "runtime_metrics_event_store")?);
        }
        if events.len() != count || observed_bytes != accounted_snapshot_bytes {
            return Err(Error::config(
                "runtime_metrics_event_store",
                "runtime metric SQLite source changed during its bounded read",
            ));
        }
        Ok(StoreMetricEventSourceRead {
            events,
            accounted_snapshot_bytes,
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
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("memory_write_transaction", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
        #[cfg(feature = "nonproduction-replay-harness")]
        replay_harness_pause_after_begin_immediate()?;
        admission.validate_inside_engine_fence(self.capacity, &self.admission_authority)?;
        let context = Self::load_transaction_context(&tx, request, admission.operation_capacity())?;
        let plan = apply_transaction(admission, request, &context)?;

        for mutation in &plan.effective_request.mutations {
            match mutation {
                StoreEngineMutation::PutJson {
                    namespace,
                    key,
                    value,
                } => {
                    let raw = serde_json::to_string(value).map_err(|error| {
                        Error::config("memory_write_transaction", error.to_string())
                    })?;
                    tx.execute(
                        "INSERT OR REPLACE INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                        params![namespace, key, raw],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::DeleteJson { namespace, key } => {
                    tx.execute(
                        "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                        params![namespace, key],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::PutBlob {
                    namespace,
                    key,
                    value,
                } => {
                    tx.execute(
                        "INSERT OR REPLACE INTO bm_blob(namespace, key, value_blob) VALUES (?1, ?2, ?3)",
                        params![namespace, key, value],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::DeleteBlob { namespace, key } => {
                    tx.execute(
                        "DELETE FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                        params![namespace, key],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::AppendEvent { event } => {
                    let raw = serialize_event(event)?;
                    tx.execute(
                        "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                        params![&event.event_id, raw],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::DeleteJsonIfPresent { .. }
                | StoreEngineMutation::DeleteBlobIfPresent { .. } => {
                    return Err(Error::config(
                        "memory_write_transaction",
                        "conditional mutation reached SQLite primitive execution",
                    ));
                }
            }
        }
        tx.commit()
            .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
        Ok(plan.report)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn read_consistent(
        &self,
        request: &StoreConsistentReadRequest,
    ) -> Result<StoreConsistentReadResult> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        let mut state = BackendTransactionState::default();
        for address in &request.json {
            let raw: Option<String> = tx
                .query_row(
                    "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![&address.namespace, &address.key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if let Some(raw) = raw {
                let value = serde_json::from_str(&raw)
                    .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
                state
                    .json
                    .insert((address.namespace.clone(), address.key.clone()), value);
            }
        }
        for address in &request.blobs {
            let value: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                    params![&address.namespace, &address.key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if let Some(value) = value {
                state
                    .blobs
                    .insert((address.namespace.clone(), address.key.clone()), value);
            }
        }
        if request.include_events {
            let mut statement = tx
                .prepare("SELECT event_json FROM bm_event_log ORDER BY sequence ASC")
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            for row in rows {
                let raw =
                    row.map_err(|error| map_transaction_error("store_consistent_read", error))?;
                state
                    .events
                    .push(decode_current_event(&raw, "store_consistent_read")?);
            }
        }
        let result = read_consistent_from_state(request, &state);
        tx.commit()
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        Ok(result)
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

        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        let mut json = std::collections::BTreeMap::new();
        let mut json_bytes = 0_usize;
        for (namespace, key) in json_keys {
            let bytes: Option<usize> = tx
                .query_row(
                    "SELECT length(value_json) FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if let Some(bytes) = bytes {
                json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("consistent known-key JSON byte count overflow")
                })?;
                if json_bytes > capacity.snapshot_max_bytes {
                    return Err(Error::config(
                        "store_consistent_read_budget_exceeded",
                        format!(
                            "JSON bytes {json_bytes} exceed {}",
                            capacity.snapshot_max_bytes
                        ),
                    ));
                }
                let raw: String = tx
                    .query_row(
                        "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                        params![namespace, key],
                        |row| row.get(0),
                    )
                    .map_err(|error| map_transaction_error("store_consistent_read", error))?;
                let value = serde_json::from_str(&raw)
                    .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
                json.insert((namespace.clone(), key.clone()), value);
            }
        }
        let mut blobs = std::collections::BTreeMap::new();
        let mut blob_bytes = 0_usize;
        for (namespace, key) in blob_keys {
            let bytes: Option<usize> = tx
                .query_row(
                    "SELECT length(value_blob) FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if let Some(bytes) = bytes {
                blob_bytes = blob_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("consistent known-key blob byte count overflow")
                })?;
                if blob_bytes > capacity.blob_max_bytes {
                    return Err(Error::config(
                        "store_consistent_read_budget_exceeded",
                        format!("blob bytes {blob_bytes} exceed {}", capacity.blob_max_bytes),
                    ));
                }
                let value: Vec<u8> = tx
                    .query_row(
                        "SELECT value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                        params![namespace, key],
                        |row| row.get(0),
                    )
                    .map_err(|error| map_transaction_error("store_consistent_read", error))?;
                blobs.insert((namespace.clone(), key.clone()), value);
            }
        }
        let events = if include_events {
            let (event_count, event_bytes): (usize, usize) = tx
                .query_row(
                    "SELECT COUNT(*), COALESCE(SUM(length(event_json)), 0) FROM bm_event_log",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if event_count > capacity.event_log_max_items
                || json_bytes.saturating_add(event_bytes) > capacity.snapshot_max_bytes
            {
                return Err(Error::config(
                    "store_consistent_read_budget_exceeded",
                    "event log exceeds the consistent known-key read budget",
                ));
            }
            let mut statement = tx
                .prepare("SELECT event_json FROM bm_event_log ORDER BY sequence ASC")
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            let mut events = Vec::with_capacity(event_count);
            for row in rows {
                let raw =
                    row.map_err(|error| map_transaction_error("store_consistent_read", error))?;
                events.push(decode_current_event(&raw, "store_consistent_read")?);
            }
            events
        } else {
            Vec::new()
        };
        let result = read_bounded_known_keys_from_parts(
            json_keys,
            blob_keys,
            include_events,
            capacity,
            &json,
            &blobs,
            &events,
        )?;
        tx.commit()
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        Ok(result)
    }

    fn open_immutable_read_session<'a>(
        &'a self,
        capacity: StoreCapacityBudget,
    ) -> Result<Box<dyn StoreImmutableReadSession + 'a>> {
        validate_immutable_read_session_capacity(self.capacity, capacity)?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_immutable_read_session", error.to_string()))?;
        connection
            .execute_batch("BEGIN DEFERRED")
            .map_err(|error| map_transaction_error("store_immutable_read_session", error))?;
        let pinned_schema_version = connection
            .query_row(
                "SELECT schema_version FROM bm_schema WHERE schema_id = ?1",
                params![STORE_SCHEMA_ID],
                |row| row.get::<_, u32>(0),
            )
            .optional()
            .map_err(|error| map_transaction_error("store_immutable_read_session", error));
        match pinned_schema_version {
            Ok(Some(schema_version)) if schema_version == STORE_SCHEMA_VERSION => {}
            Ok(Some(schema_version)) => {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(Error::config(
                    "store_immutable_read_session",
                    format!(
                        "pinned schema version {schema_version} differs from {STORE_SCHEMA_VERSION}"
                    ),
                ));
            }
            Ok(None) => {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(Error::config(
                    "store_immutable_read_session",
                    "pinned schema manifest row is missing",
                ));
            }
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        Ok(Box::new(SqliteImmutableReadSession {
            connection,
            read: StoreReadSessionState::new(capacity),
        }))
    }

    fn read_scoped_projection(
        &self,
        request: &crate::StoreScopedProjectionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<crate::StoreScopedProjection> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        let scoped_json = read_scoped_json_exact(&tx, request, capacity)?;
        let events = if request.include_events {
            let (count, bytes): (usize, usize) = tx
                .query_row(
                    r#"SELECT COUNT(*), COALESCE(SUM(length(event_json)), 0)
                       FROM bm_event_log
                       WHERE json_extract(event_json, '$.scope.memory_space_id') = ?1
                         AND json_extract(event_json, '$.scope.physical_owning_scope.kind') = ?2
                         AND (
                           ?2 = 'shared_program'
                           OR (
                             json_extract(event_json, '$.scope.physical_owning_scope.mounted_subject_id') = ?3
                             AND json_extract(event_json, '$.scope.subject_id') = ?3
                           )
                         )"#,
                    params![
                        &request.scope.memory_space_id,
                        match &request.scope.physical_owning_scope {
                            crate::StorePhysicalOwningScope::Subject { .. } => "subject",
                            crate::StorePhysicalOwningScope::SharedProgram => "shared_program",
                        },
                        request.scope.mounted_subject_id(),
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
            if count > capacity.event_log_max_items
                || scoped_json.logical_bytes.saturating_add(bytes) > capacity.snapshot_max_bytes
            {
                return Err(Error::config(
                    "store_scoped_projection_budget_exceeded",
                    "scoped projection events exceed the pinned operation budget",
                ));
            }
            let mut statement = tx
                .prepare(
                    r#"SELECT event_json FROM bm_event_log
                       WHERE json_extract(event_json, '$.scope.memory_space_id') = ?1
                         AND json_extract(event_json, '$.scope.physical_owning_scope.kind') = ?2
                         AND (
                           ?2 = 'shared_program'
                           OR (
                             json_extract(event_json, '$.scope.physical_owning_scope.mounted_subject_id') = ?3
                             AND json_extract(event_json, '$.scope.subject_id') = ?3
                           )
                         )
                       ORDER BY sequence"#,
                )
                .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
            let rows = statement
                .query_map(
                    params![
                        &request.scope.memory_space_id,
                        match &request.scope.physical_owning_scope {
                            crate::StorePhysicalOwningScope::Subject { .. } => "subject",
                            crate::StorePhysicalOwningScope::SharedProgram => "shared_program",
                        },
                        request.scope.mounted_subject_id(),
                    ],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
            let mut events = Vec::with_capacity(count);
            for row in rows {
                let raw =
                    row.map_err(|error| map_transaction_error("store_scoped_projection", error))?;
                events.push(decode_current_event(&raw, "store_scoped_projection")?);
            }
            events
        } else {
            Vec::new()
        };
        let projection = crate::store_internal::transaction::read_scoped_projection_from_parts(
            request,
            capacity,
            &scoped_json.documents,
            &events,
        )?;
        tx.commit()
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        Ok(projection)
    }

    fn replace_scoped_projection(
        &self,
        request: &crate::StoreScopedProjectionReplaceRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<crate::StoreScopedProjectionReplaceReport> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        admission.validate_inside_engine_fence(self.capacity, &self.admission_authority)?;
        let projection_request = crate::StoreScopedProjectionRequest {
            scope: request.scope.clone(),
            json_namespaces: request.json_namespaces.clone(),
            include_events: false,
        };
        let scoped_json =
            read_scoped_json_exact(&tx, &projection_request, admission.operation_capacity())?;
        let deleted_addresses = scoped_json
            .documents
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        for doc in &request.json_docs {
            let address = (doc.namespace.clone(), doc.key.clone());
            let exists = tx
                .query_row(
                    "SELECT 1 FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![&doc.namespace, &doc.key],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_scoped_projection", error))?
                .is_some();
            if exists && !deleted_addresses.contains(&address) {
                return Err(Error::config(
                    "store_scoped_projection",
                    format!(
                        "replacement address {}/{} is owned by another projection scope",
                        doc.namespace, doc.key
                    ),
                ));
            }
        }
        for (namespace, key) in &deleted_addresses {
            tx.execute(
                "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        }
        let deleted_json = deleted_addresses.len();
        for doc in &request.json_docs {
            let raw = serde_json::to_string(&doc.value)
                .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?;
            tx.execute(
                "INSERT INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                params![&doc.namespace, &doc.key, raw],
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        }
        let deleted_events = tx
            .execute(
                r#"DELETE FROM bm_event_log
                   WHERE json_extract(event_json, '$.scope.memory_space_id') = ?1
                     AND json_extract(event_json, '$.scope.physical_owning_scope.kind') = ?2
                     AND (
                       ?2 = 'shared_program'
                       OR (
                         json_extract(event_json, '$.scope.physical_owning_scope.mounted_subject_id') = ?3
                         AND json_extract(event_json, '$.scope.subject_id') = ?3
                       )
                     )"#,
                params![
                    &request.scope.memory_space_id,
                    match &request.scope.physical_owning_scope {
                        crate::StorePhysicalOwningScope::Subject { .. } => "subject",
                        crate::StorePhysicalOwningScope::SharedProgram => "shared_program",
                    },
                    request.scope.mounted_subject_id(),
                ],
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        for event in &request.events {
            let raw = serialize_event(event)?;
            tx.execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![&event.event_id, raw],
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        }
        let (entries, blob_bytes, event_count): (usize, usize, usize) = tx
            .query_row(
                "SELECT (SELECT COUNT(*) FROM bm_kv) + (SELECT COUNT(*) FROM bm_blob), COALESCE((SELECT SUM(length(value_blob)) FROM bm_blob), 0), (SELECT COUNT(*) FROM bm_event_log)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        validate_scoped_projection_post_image(
            admission,
            request,
            entries,
            std::iter::once(blob_bytes),
            std::iter::empty(),
            event_count,
        )?;
        tx.commit()
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        Ok(crate::StoreScopedProjectionReplaceReport {
            admission_report_id: admission.report_id().to_string(),
            deleted_json,
            inserted_json: request.json_docs.len(),
            deleted_events,
            inserted_events: request.events.len(),
        })
    }

    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_json_read")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_read", error.to_string()))?;
        let raw: Option<String> = connection
            .query_row(
                "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_json_read", error))?;
        raw.map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| Error::config("sqlite_store_json_read", error.to_string()))
        })
        .transpose()
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        let raw = serde_json::to_string(&value)
            .map_err(|error| Error::config("sqlite_store_json_write", error.to_string()))?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_write", error.to_string()))?;
        Self::ensure_json_entry_budget(self.capacity, &connection, namespace, key)?;
        connection
            .execute(
                "INSERT OR REPLACE INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                params![namespace, key, raw],
            )
            .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_json_delete")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_delete", error.to_string()))?;
        let rows = connection
            .execute(
                "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| Error::storage("sqlite_store_json_delete", error))?;
        Ok(rows > 0)
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        enforce_logical_key_budget(self.capacity, namespace, "", "sqlite_store_json_list")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_list", error.to_string()))?;
        let mut statement = connection
            .prepare("SELECT key FROM bm_kv WHERE namespace = ?1 ORDER BY key ASC")
            .map_err(|error| Error::storage("sqlite_store_json_list", error))?;
        let rows = statement
            .query_map(params![namespace], |row| row.get::<_, String>(0))
            .map_err(|error| Error::storage("sqlite_store_json_list", error))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|error| Error::storage("sqlite_store_json_list", error))?);
        }
        Ok(out)
    }

    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_blob_read")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_read", error.to_string()))?;
        connection
            .query_row(
                "SELECT value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_blob_read", error))
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_write", error.to_string()))?;
        Self::ensure_blob_budget(self.capacity, &connection, namespace, key, value.len())?;
        connection
            .execute(
                "INSERT OR REPLACE INTO bm_blob(namespace, key, value_blob) VALUES (?1, ?2, ?3)",
                params![namespace, key, value],
            )
            .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_blob_delete")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_delete", error.to_string()))?;
        let rows = connection
            .execute(
                "DELETE FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| Error::storage("sqlite_store_blob_delete", error))?;
        Ok(rows > 0)
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        enforce_logical_key_budget(self.capacity, namespace, "", "sqlite_store_blob_list")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_list", error.to_string()))?;
        let mut statement = connection
            .prepare("SELECT key FROM bm_blob WHERE namespace = ?1 ORDER BY key ASC")
            .map_err(|error| Error::storage("sqlite_store_blob_list", error))?;
        let rows = statement
            .query_map(params![namespace], |row| row.get::<_, String>(0))
            .map_err(|error| Error::storage("sqlite_store_blob_list", error))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|error| Error::storage("sqlite_store_blob_list", error))?);
        }
        Ok(out)
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
        if events.len() > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                events.len(),
                self.capacity.event_log_max_items
            )));
        }
        let mut event_ids = std::collections::BTreeSet::new();
        let mut encoded_events = Vec::with_capacity(events.len());
        for event in events {
            enforce_event_key_budget(self.capacity, event, "sqlite_store_snapshot_import")?;
            if !event_ids.insert(event.event_id.clone()) {
                return Err(Error::config(
                    "store_event_log",
                    format!("duplicate event id {}", event.event_id),
                ));
            }
            encoded_events.push((event.event_id.clone(), serialize_event(event)?));
        }
        let encoded_docs = json_docs
            .iter()
            .map(|doc| {
                enforce_logical_key_budget(
                    self.capacity,
                    &doc.namespace,
                    &doc.key,
                    "sqlite_store_snapshot_import",
                )?;
                serde_json::to_string(&doc.value)
                    .map(|raw| (doc.namespace.clone(), doc.key.clone(), raw))
                    .map_err(|error| {
                        Error::config("sqlite_store_snapshot_import", error.to_string())
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        let json_snapshot_keys = json_docs
            .iter()
            .map(|doc| (doc.namespace.clone(), doc.key.clone()))
            .collect::<std::collections::BTreeSet<_>>();
        let blob_snapshot_keys = blobs
            .iter()
            .map(|blob| {
                enforce_logical_key_budget(
                    self.capacity,
                    &blob.namespace,
                    &blob.key,
                    "sqlite_store_snapshot_import",
                )?;
                Ok((blob.namespace.clone(), blob.key.clone()))
            })
            .collect::<Result<std::collections::BTreeSet<_>>>()?;

        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_snapshot_import", error.to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        let retained_json_entries = count_retained_rows(
            &tx,
            "bm_kv",
            "sqlite_store_snapshot_import",
            json_namespaces,
        )?;
        let final_json_entries = retained_json_entries.saturating_add(json_docs.len());
        if final_json_entries > self.capacity.kv_max_entries {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                final_json_entries, self.capacity.kv_max_entries
            )));
        }
        let retained_blob_lengths =
            retained_blob_lengths(&tx, "sqlite_store_snapshot_import", blob_namespaces)?;
        validate_restore_post_image_blob_bytes(
            self.capacity,
            retained_blob_lengths,
            blobs.iter().map(|blob| blob.value.len()),
        )?;

        let mut json_deleted = 0usize;
        for namespace in json_namespaces {
            let mut statement = tx
                .prepare("SELECT key FROM bm_kv WHERE namespace = ?1 ORDER BY key ASC")
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
            let rows = statement
                .query_map(params![namespace], |row| row.get::<_, String>(0))
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
            for row in rows {
                let key =
                    row.map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
                if !json_snapshot_keys.contains(&((*namespace).to_string(), key)) {
                    json_deleted = json_deleted.saturating_add(1);
                }
            }
            drop(statement);
            tx.execute("DELETE FROM bm_kv WHERE namespace = ?1", params![namespace])
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        }
        for (namespace, key, raw) in encoded_docs {
            tx.execute(
                "INSERT OR REPLACE INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                params![namespace, key, raw],
            )
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        }

        let mut blobs_deleted = 0usize;
        for namespace in blob_namespaces {
            let mut statement = tx
                .prepare("SELECT key FROM bm_blob WHERE namespace = ?1 ORDER BY key ASC")
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
            let rows = statement
                .query_map(params![namespace], |row| row.get::<_, String>(0))
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
            for row in rows {
                let key =
                    row.map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
                if !blob_snapshot_keys.contains(&((*namespace).to_string(), key)) {
                    blobs_deleted = blobs_deleted.saturating_add(1);
                }
            }
            drop(statement);
            tx.execute(
                "DELETE FROM bm_blob WHERE namespace = ?1",
                params![namespace],
            )
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        }
        for blob in blobs {
            tx.execute(
                "INSERT OR REPLACE INTO bm_blob(namespace, key, value_blob) VALUES (?1, ?2, ?3)",
                params![&blob.namespace, &blob.key, &blob.value],
            )
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        }

        tx.execute("DELETE FROM bm_event_log", [])
            .map_err(|error| Error::storage("store_event_log", error))?;
        for (event_id, raw) in encoded_events {
            tx.execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![event_id, raw],
            )
            .map_err(map_event_insert_error)?;
        }
        tx.commit()
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        Ok(StoreSnapshotReplaceReport {
            json_deleted,
            blobs_deleted,
            events_imported: events.len(),
        })
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn replay_harness_pause_after_begin_immediate() -> Result<()> {
    const FAILPOINT: &str = "after_begin_immediate_before_load_transaction_state";
    const HANDSHAKE: u8 = b'P';
    const RELEASE: u8 = b'C';

    if std::env::var_os("BM_SQLITE_TRANSACTION_FAILPOINT").as_deref()
        != Some(std::ffi::OsStr::new(FAILPOINT))
    {
        return Ok(());
    }

    let address = std::env::var("BM_SQLITE_TRANSACTION_FAILPOINT_ADDR").map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("sqlite replay failpoint address is required: {error}"),
        )
    })?;
    let mut stream = TcpStream::connect(&address).map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("connect sqlite replay failpoint {address}: {error}"),
        )
    })?;
    stream.write_all(&[HANDSHAKE]).map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("announce sqlite replay failpoint: {error}"),
        )
    })?;
    stream.flush().map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("flush sqlite replay failpoint: {error}"),
        )
    })?;
    let mut release = [0];
    stream.read_exact(&mut release).map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("wait for sqlite replay failpoint release: {error}"),
        )
    })?;
    if release != [RELEASE] {
        return Err(Error::config(
            "memory_write_transaction",
            "unexpected sqlite replay failpoint release",
        ));
    }
    Ok(())
}

fn map_transaction_error(stage: &'static str, error: rusqlite::Error) -> Error {
    match &error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            Error::config("store_transaction_busy", error.to_string())
        }
        _ => Error::storage(stage, error),
    }
}

fn serialize_event(event: &MemoryStoreEvent) -> Result<String> {
    serde_json::to_string(event)
        .map_err(|error| Error::config("store_event_log", error.to_string()))
}

#[cfg(feature = "nonproduction-replay-harness")]
fn count_retained_rows(
    connection: &Connection,
    table: &'static str,
    stage: &'static str,
    replaced_namespaces: &[&str],
) -> Result<usize> {
    let replaced = replaced_namespaces
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut statement = connection
        .prepare(&format!("SELECT namespace FROM {table}"))
        .map_err(|error| Error::storage(stage, error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| Error::storage(stage, error))?;
    let mut count = 0usize;
    for row in rows {
        let namespace = row.map_err(|error| Error::storage(stage, error))?;
        if !replaced.contains(namespace.as_str()) {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn retained_blob_lengths(
    connection: &Connection,
    stage: &'static str,
    replaced_namespaces: &[&str],
) -> Result<Vec<usize>> {
    let replaced = replaced_namespaces
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut statement = connection
        .prepare("SELECT namespace, length(value_blob) FROM bm_blob")
        .map_err(|error| Error::storage(stage, error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
        })
        .map_err(|error| Error::storage(stage, error))?;
    let mut lengths = Vec::new();
    for row in rows {
        let (namespace, len) = row.map_err(|error| Error::storage(stage, error))?;
        if !replaced.contains(namespace.as_str()) {
            lengths.push(len);
        }
    }
    Ok(lengths)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn map_event_insert_error(error: rusqlite::Error) -> Error {
    if error.to_string().contains("UNIQUE") {
        Error::config("store_event_log", "duplicate event id")
    } else {
        Error::storage("store_event_log", error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_schema_is_rejected_by_read_only_preflight_without_byte_changes() {
        let path = std::env::temp_dir().join(format!(
            "bm-sqlite-v5-zero-mutation-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let profile = crate::ProfileId::native_dev_full().expect("native test profile");
        let config = StoreBackendConfig::sqlite(&path, profile).expect("sqlite config");
        let mut legacy = StoreSchemaManifest::new(config.backend, config.profile, 7);
        legacy.schema_id = "beetle_memory_store_schema_v5".to_string();
        legacy.schema_version = 5;
        {
            let connection = Connection::open(&path).expect("create legacy sqlite");
            connection
                .execute_batch(
                    "CREATE TABLE bm_schema (
                        schema_id TEXT PRIMARY KEY,
                        schema_version INTEGER NOT NULL,
                        manifest_json TEXT NOT NULL
                    );",
                )
                .expect("create schema table");
            connection
                .execute(
                    "INSERT INTO bm_schema(schema_id, schema_version, manifest_json)
                     VALUES (?1, ?2, ?3)",
                    params![
                        &legacy.schema_id,
                        legacy.schema_version,
                        serde_json::to_string(&legacy).expect("legacy manifest")
                    ],
                )
                .expect("insert legacy schema");
        }
        let before = std::fs::read(&path).expect("read sqlite before");

        let error = validate_existing_sqlite_schema_read_only(&path, &config)
            .expect_err("v5 must fail closed");
        assert_eq!(error.stage(), "sqlite_store_schema");
        assert_eq!(std::fs::read(&path).expect("read sqlite after"), before);

        std::fs::remove_file(path).expect("remove sqlite fixture");
    }
}
