#![cfg(feature = "sqlite")]

use bm_core::{
    MemoryPlane, MemoryRecordMeta, NewMemoryRecord, ProceduralSkillDraft, ProceduralSkillOrigin,
    ProceduralSkillState,
};
use bm_store::{MemoryStore, SqliteStore, StoreResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn sqlite_store_persists_procedural_metadata() -> StoreResult<()> {
    let root = TempStoreRoot::new("sqlite_store_persists_procedural_metadata");
    let db_path = root.path().join("memory.sqlite3");
    let mut store = SqliteStore::open(&db_path)?;
    store.insert(record())?;
    drop(store);

    let reopened = SqliteStore::open(&db_path)?;
    let records = reopened.records()?;
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].meta.procedural.as_ref().map(|meta| meta.origin),
        Some(ProceduralSkillOrigin::UserProvided)
    );
    Ok(())
}

fn record() -> NewMemoryRecord {
    let draft = ProceduralSkillDraft::new(
        "agent:s6",
        "project:s6",
        ProceduralSkillOrigin::UserProvided,
        "Release checklist",
        "release checklist",
        "When preparing a release, first verify status, then run tests, then commit.",
    );
    let mut meta = MemoryRecordMeta::default_for_plane(MemoryPlane::Procedural);
    meta.procedural = Some(bm_core::procedural_skill_meta_from_draft(
        &draft,
        ProceduralSkillState::Active,
        10,
    ));
    NewMemoryRecord {
        identity: "agent:s6".to_owned(),
        scope: "project:s6".to_owned(),
        content: draft.procedure,
        source: "unit-test".to_owned(),
        domain: MemoryPlane::Procedural.domain(),
        plane: MemoryPlane::Procedural,
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
        let path = std::env::temp_dir().join(format!("bm-s6-{name}-{nanos}"));
        fs::create_dir_all(&path).expect("create temp sqlite root");
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
