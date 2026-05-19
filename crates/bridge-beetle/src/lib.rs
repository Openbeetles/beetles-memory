//! Beetle migration contracts for Beetle Memory.

use bm_core::{MemoryDomain, MemoryPlane, WriteCandidate};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeetleMemorySource {
    pub system: String,
    pub record_id: String,
    pub content: String,
    pub origin_path: Option<String>,
}

impl BeetleMemorySource {
    pub fn new(
        system: impl Into<String>,
        record_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            system: system.into(),
            record_id: record_id.into(),
            content: content.into(),
            origin_path: None,
        }
    }

    pub fn origin_path(mut self, origin_path: impl Into<String>) -> Self {
        self.origin_path = Some(origin_path.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPlan {
    pub source: BeetleMemorySource,
    pub target_domain: MemoryDomain,
    pub target_plane: MemoryPlane,
    pub candidate: WriteCandidate,
}

#[derive(Clone, Debug, Default)]
pub struct MigrationPlanner;

impl MigrationPlanner {
    pub fn plan(&self, source: BeetleMemorySource) -> MigrationPlan {
        let target_plane = MemoryPlane::SharedFactual;
        let candidate = WriteCandidate::new(
            "beetle:migration",
            "beetle:migration",
            source.content.clone(),
        )
        .source(format!("beetle:{}", source.record_id))
        .plane_hint(target_plane);

        MigrationPlan {
            source,
            target_domain: target_plane.domain(),
            target_plane,
            candidate,
        }
    }
}
