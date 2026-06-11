use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::{Error, Result};
use serde_json::{json, Value};

use crate::{
    MemoryStoreEvent, StoreBackendConfig, StoreEngine, StoreEventLog, StorePathBudget,
    StoreRepairPolicy, StoreRepairReport, StoreSchemaManifest, StoreSnapshotBlob,
    StoreSnapshotJsonDoc, StoreSnapshotReplaceReport,
};

const FILE_ADDRESSING_VERSION: u64 = 2;
const FILE_ADDRESSING_DATA_DIR: &str = "_v2";
const FILE_ADDRESSING_INDEX_DIR: &str = "_keys";
const MIN_PHYSICAL_DIGEST_HEX_CHARS: usize = 16;
const MAX_PHYSICAL_DIGEST_HEX_CHARS: usize = 32;

pub struct FileStoreEngine {
    root: PathBuf,
    fsync: bool,
    path_budget: StorePathBudget,
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
        fs::create_dir_all(root.join("events"))
            .map_err(|error| Error::io("file_store_open", error))?;
        fs::create_dir_all(root.join("kv")).map_err(|error| Error::io("file_store_open", error))?;
        fs::create_dir_all(root.join("blob"))
            .map_err(|error| Error::io("file_store_open", error))?;
        fs::create_dir_all(root.join("snapshots"))
            .map_err(|error| Error::io("file_store_open", error))?;
        let engine = Self {
            root,
            fsync: config.fsync,
            path_budget: config.path_budget,
        };
        let repair = engine.repair_orphan_tmp_files(config.repair_policy)?;
        let manifest = engine.open_or_create_manifest(config)?;
        Ok((engine, repair, manifest))
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

    fn list_keys(&self, base: PathBuf, extension: &str) -> Result<Vec<String>> {
        let mut out = BTreeSet::new();
        for key in self.list_indexed_keys(&base, extension)? {
            out.insert(key);
        }
        for key in self.list_legacy_encoded_keys(base, extension)? {
            out.insert(key);
        }
        Ok(out.into_iter().collect())
    }

    fn list_indexed_keys(&self, base: &Path, extension: &str) -> Result<Vec<String>> {
        let index_base = base.join(FILE_ADDRESSING_INDEX_DIR);
        let data_base = base.join(FILE_ADDRESSING_DATA_DIR);
        let Ok(shards) = fs::read_dir(&index_base) else {
            return Ok(Vec::new());
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
                    continue;
                }
                if let Some(key) = self.read_key_index(&path, "file_store_list")? {
                    out.push(key);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn list_legacy_encoded_keys(&self, base: PathBuf, extension: &str) -> Result<Vec<String>> {
        let Ok(entries) = fs::read_dir(&base) else {
            return Ok(Vec::new());
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
}

impl StoreEventLog for FileStoreEngine {
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        if self
            .read_events()?
            .iter()
            .any(|existing| existing.event_id == event.event_id)
        {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        let path = self.events_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| Error::io("store_event_log", error))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| Error::io("store_event_log", error))?;
        serde_json::to_writer(&mut file, &event)
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        file.write_all(b"\n")
            .map_err(|error| Error::io("store_event_log", error))?;
        if self.fsync {
            file.sync_all()
                .map_err(|error| Error::io("store_event_log", error))?;
        }
        Ok(())
    }

    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        read_events_jsonl(&self.events_path())
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
    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        let paths = self.json_paths(namespace, key)?;
        match fs::read(&paths.data_path) {
            Ok(bytes) => {
                self.require_key_index_matches(&paths.index_path, key, "file_store_json_read")?;
                serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(|error| Error::config("file_store_json_read", error.to_string()))
            }
            Err(error) if is_not_found_or_invalid_filename(&error) => {
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

    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        let paths = self.json_paths(namespace, key)?;
        self.ensure_key_index_available(&paths, key, "file_store_json_write")?;
        self.write_key_index(&paths.index_path, key, "file_store_json_write")?;
        let bytes = serde_json::to_vec_pretty(&value)
            .map_err(|error| Error::config("file_store_json_write", error.to_string()))?;
        atomic_write(
            &paths.data_path,
            &bytes,
            self.fsync,
            "file_store_json_write",
        )
    }

    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        let paths = self.json_paths(namespace, key)?;
        let mut deleted = false;
        deleted |= remove_file_if_exists(&paths.data_path, "file_store_json_delete")?;
        deleted |= remove_file_if_exists(&paths.index_path, "file_store_json_delete")?;
        deleted |= remove_file_if_exists(&paths.legacy_path, "file_store_json_delete")?;
        Ok(deleted)
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        self.validate_directory_component(namespace, "file_store_list")?;
        self.list_keys(self.root.join("kv").join(namespace), "json")
    }

    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let paths = self.blob_paths(namespace, key)?;
        match fs::read(&paths.data_path) {
            Ok(bytes) => {
                self.require_key_index_matches(&paths.index_path, key, "file_store_blob_read")?;
                Ok(Some(bytes))
            }
            Err(error) if is_not_found_or_invalid_filename(&error) => {
                match fs::read(&paths.legacy_path) {
                    Ok(bytes) => Ok(Some(bytes)),
                    Err(error) if is_not_found_or_invalid_filename(&error) => Ok(None),
                    Err(error) => Err(Error::io("file_store_blob_read", error)),
                }
            }
            Err(error) => Err(Error::io("file_store_blob_read", error)),
        }
    }

    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        let paths = self.blob_paths(namespace, key)?;
        self.ensure_key_index_available(&paths, key, "file_store_blob_write")?;
        self.write_key_index(&paths.index_path, key, "file_store_blob_write")?;
        atomic_write(&paths.data_path, value, self.fsync, "file_store_blob_write")
    }

    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        let paths = self.blob_paths(namespace, key)?;
        let mut deleted = false;
        deleted |= remove_file_if_exists(&paths.data_path, "file_store_blob_delete")?;
        deleted |= remove_file_if_exists(&paths.index_path, "file_store_blob_delete")?;
        deleted |= remove_file_if_exists(&paths.legacy_path, "file_store_blob_delete")?;
        Ok(deleted)
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        self.validate_directory_component(namespace, "file_store_list")?;
        self.list_keys(self.root.join("blob").join(namespace), "bin")
    }

    fn replace_events(&self, events: &[MemoryStoreEvent]) -> Result<()> {
        let mut event_ids = BTreeSet::new();
        let mut bytes = Vec::new();
        for event in events {
            if !event_ids.insert(event.event_id.clone()) {
                return Err(Error::config(
                    "store_event_log",
                    format!("duplicate event id {}", event.event_id),
                ));
            }
            serde_json::to_writer(&mut bytes, event)
                .map_err(|error| Error::config("store_event_log", error.to_string()))?;
            bytes.push(b'\n');
        }
        atomic_write(&self.events_path(), &bytes, self.fsync, "store_event_log")
    }

    fn replace_snapshot(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport> {
        let import_id = format!("{}-{}", current_unix_secs(), std::process::id());
        let stage_root = self.root.join(format!(".snapshot-import-{import_id}"));
        let backup_root = self.root.join(format!(".snapshot-backup-{import_id}"));
        let _ = fs::remove_dir_all(&stage_root);
        let _ = fs::remove_dir_all(&backup_root);

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
            "file_store_snapshot_import",
        )?;
        for namespace in json_namespaces {
            move_path_if_exists(
                &self.json_dir(namespace),
                &backup_root.join("kv").join(namespace),
                "file_store_snapshot_import",
            )?;
        }
        for namespace in blob_namespaces {
            move_path_if_exists(
                &self.blob_dir(namespace),
                &backup_root.join("blob").join(namespace),
                "file_store_snapshot_import",
            )?;
        }

        move_path_if_exists(
            &stage_root.join("events").join("events.jsonl"),
            &self.events_path(),
            "file_store_snapshot_import",
        )?;
        for namespace in json_namespaces {
            move_path_if_exists(
                &stage_root.join("kv").join(namespace),
                &self.json_dir(namespace),
                "file_store_snapshot_import",
            )?;
        }
        for namespace in blob_namespaces {
            move_path_if_exists(
                &stage_root.join("blob").join(namespace),
                &self.blob_dir(namespace),
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
            "file_store_snapshot_rollback",
        )?;
        for namespace in json_namespaces {
            remove_path_if_exists(&self.json_dir(namespace), "file_store_snapshot_rollback")?;
            move_path_if_exists(
                &backup_root.join("kv").join(namespace),
                &self.json_dir(namespace),
                "file_store_snapshot_rollback",
            )?;
        }
        for namespace in blob_namespaces {
            remove_path_if_exists(&self.blob_dir(namespace), "file_store_snapshot_rollback")?;
            move_path_if_exists(
                &backup_root.join("blob").join(namespace),
                &self.blob_dir(namespace),
                "file_store_snapshot_rollback",
            )?;
        }
        Ok(())
    }
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
        for key in engine.list_json_keys(namespace)? {
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
        for key in engine.list_blob_keys(namespace)? {
            if !snapshot_keys.contains(&((*namespace).to_string(), key)) {
                deleted = deleted.saturating_add(1);
            }
        }
    }
    Ok(deleted)
}

fn move_path_if_exists(from: &Path, to: &Path, stage: &'static str) -> Result<bool> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::io(stage, error))?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(true),
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
    Ok(())
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
