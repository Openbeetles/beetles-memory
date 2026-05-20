use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::{Error, Result};
use serde_json::Value;

use crate::{
    MemoryStoreEvent, StoreBackendConfig, StoreEngine, StoreEventLog, StoreRepairPolicy,
    StoreRepairReport, StoreSchemaManifest, StoreSnapshotBlob, StoreSnapshotJsonDoc,
    StoreSnapshotReplaceReport,
};

pub struct FileStoreEngine {
    root: PathBuf,
    fsync: bool,
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

    fn json_path(&self, namespace: &str, key: &str) -> PathBuf {
        self.root
            .join("kv")
            .join(namespace)
            .join(format!("{}.json", hex_encode(key.as_bytes())))
    }

    fn blob_path(&self, namespace: &str, key: &str) -> PathBuf {
        self.root
            .join("blob")
            .join(namespace)
            .join(format!("{}.bin", hex_encode(key.as_bytes())))
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

    fn list_encoded_keys(&self, base: PathBuf, extension: &str) -> Result<Vec<String>> {
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
        let path = self.events_path();
        let bytes = match fs::read(&path) {
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
}

impl StoreEngine for FileStoreEngine {
    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        let path = self.json_path(namespace, key);
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| Error::config("file_store_json_read", error.to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::io("file_store_json_read", error)),
        }
    }

    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&value)
            .map_err(|error| Error::config("file_store_json_write", error.to_string()))?;
        self.write_json_file(&self.json_path(namespace, key), &bytes)
    }

    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        let path = self.json_path(namespace, key);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::io("file_store_json_delete", error)),
        }
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        self.list_encoded_keys(self.root.join("kv").join(namespace), "json")
    }

    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.blob_path(namespace, key);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::io("file_store_blob_read", error)),
        }
    }

    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        atomic_write(
            &self.blob_path(namespace, key),
            value,
            self.fsync,
            "file_store_blob_write",
        )
    }

    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        let path = self.blob_path(namespace, key);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::io("file_store_blob_delete", error)),
        }
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        self.list_encoded_keys(self.root.join("blob").join(namespace), "bin")
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
            let path = stage_root
                .join("kv")
                .join(&doc.namespace)
                .join(format!("{}.json", hex_encode(doc.key.as_bytes())));
            let bytes = serde_json::to_vec_pretty(&doc.value)
                .map_err(|error| Error::config("file_store_snapshot_import", error.to_string()))?;
            atomic_write(&path, &bytes, self.fsync, "file_store_snapshot_import")?;
        }
        for blob in blobs {
            let path = stage_root
                .join("blob")
                .join(&blob.namespace)
                .join(format!("{}.bin", hex_encode(blob.key.as_bytes())));
            atomic_write(&path, &blob.value, self.fsync, "file_store_snapshot_import")?;
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
