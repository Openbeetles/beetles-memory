use bm_core::{
    Confidence, Freshness, LongTermMemoryKind, MemoryPlane, MemoryRecordMeta, NewMemoryRecord,
};
use bm_store::{FileStore, InMemoryStore, MemoryStore, StoreResult, STORE_SCHEMA_VERSION};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn in_memory_store_replaces_and_deletes_metadata_records() -> StoreResult<()> {
    let mut store = InMemoryStore::default();
    let mut inserted = store.insert(record("initial"))?;
    inserted.content = "replaced".to_owned();
    inserted.meta.confidence = Confidence::High;

    let replaced = store.replace(inserted.clone())?;
    assert_eq!(replaced.content, "replaced");
    assert_eq!(store.records()?[0].meta.confidence, Confidence::High);

    assert!(store.delete(&inserted.id)?);
    assert!(store.records()?.is_empty());
    Ok(())
}

#[test]
fn file_store_replays_replace_delete_and_metadata() -> StoreResult<()> {
    let root = TempStoreRoot::new("file_store_replays_replace_delete_and_metadata");
    let mut store = FileStore::open(root.path())?;
    let mut inserted = store.insert(record("initial"))?;
    inserted.content = "replaced".to_owned();
    inserted.meta.freshness = Freshness::Current;
    store.replace(inserted.clone())?;
    store.snapshot()?;
    drop(store);

    let mut reopened = FileStore::open(root.path())?;
    assert_eq!(reopened.records()?[0].content, "replaced");
    assert_eq!(reopened.records()?[0].meta.freshness, Freshness::Current);
    assert!(reopened.delete(&inserted.id)?);
    drop(reopened);

    let reopened = FileStore::open(root.path())?;
    assert!(reopened.records()?.is_empty());
    Ok(())
}

fn record(content: &str) -> NewMemoryRecord {
    let mut meta = MemoryRecordMeta::default_for_plane(MemoryPlane::SharedFactual);
    meta.long_term_kind = Some(LongTermMemoryKind::Project);
    meta.topic = Some("s5".to_owned());
    meta.slot_id = Some("Project:agent:s5:task:s5:s5".to_owned());
    NewMemoryRecord {
        identity: "agent:s5".to_owned(),
        scope: "task:s5".to_owned(),
        content: content.to_owned(),
        source: "unit-test".to_owned(),
        domain: MemoryPlane::SharedFactual.domain(),
        plane: MemoryPlane::SharedFactual,
        meta,
    }
}

struct TempStoreRoot {
    path: PathBuf,
}

impl TempStoreRoot {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("bm-s5-{name}-{nanos}"));
        fs::create_dir_all(&path).expect("create temp store root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempStoreRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn schema_version_is_v2() {
    assert_eq!(STORE_SCHEMA_VERSION, 2);
}
