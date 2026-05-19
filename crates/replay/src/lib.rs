//! Replay fixtures for Beetle Memory.

use bm_core::{MemoryPlane, ProjectionSurface, RecallQuery, RuntimeProfile, WriteCandidate};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayReport {
    pub write_accepted: usize,
    pub recall_selected: usize,
    pub projection_blocks: usize,
    pub profile: String,
}

pub fn run_basic_replay() -> ReplayReport {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let write = runtime.write(
        WriteCandidate::new("agent:replay", "task:replay", "replay fact")
            .source("replay-fixture")
            .plane_hint(MemoryPlane::SharedFactual),
    );
    let recall = runtime.recall(
        RecallQuery::new("task:replay")
            .plane(MemoryPlane::SharedFactual)
            .limit(2),
    );
    let projection = runtime.project(&recall, ProjectionSurface::Prompt);

    ReplayReport {
        write_accepted: usize::from(write.record_id.is_some()),
        recall_selected: recall.selected.len(),
        projection_blocks: projection.blocks.len(),
        profile: RuntimeProfile::DevFull.as_str().to_owned(),
    }
}
