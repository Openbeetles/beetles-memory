//! Store contracts and in-memory store for Beetle Memory.

use bm_core::{MemoryRecord, NewMemoryRecord};

pub trait MemoryStore {
    fn insert(&mut self, record: NewMemoryRecord) -> MemoryRecord;
    fn records(&self) -> Vec<MemoryRecord>;
}

#[derive(Clone, Debug, Default)]
pub struct InMemoryStore {
    records: Vec<MemoryRecord>,
    next_id: u64,
}

impl MemoryStore for InMemoryStore {
    fn insert(&mut self, record: NewMemoryRecord) -> MemoryRecord {
        self.next_id += 1;
        let stored = MemoryRecord {
            id: format!("mem-{}", self.next_id),
            identity: record.identity,
            scope: record.scope,
            content: record.content,
            source: record.source,
            domain: record.domain,
            plane: record.plane,
        };
        self.records.push(stored.clone());
        stored
    }

    fn records(&self) -> Vec<MemoryRecord> {
        self.records.clone()
    }
}
