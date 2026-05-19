//! SDK runtime entrypoint for Beetle Memory.

use bm_core::{
    MemoryPlane, NewMemoryRecord, ProjectionBlock, ProjectionReport, ProjectionSurface,
    RecallQuery, RecallSelection, RecallSelectionReport, RuntimeProfile, WriteCandidate,
    WriteReport,
};
use bm_store::MemoryStore;

pub struct MemoryRuntimeBuilder {
    profile: RuntimeProfile,
}

impl MemoryRuntimeBuilder {
    pub fn new(profile: RuntimeProfile) -> Self {
        Self { profile }
    }

    pub fn store<S>(self, store: S) -> MemoryRuntimeBuilderWithStore<S>
    where
        S: MemoryStore,
    {
        MemoryRuntimeBuilderWithStore {
            profile: self.profile,
            store,
        }
    }
}

pub struct MemoryRuntimeBuilderWithStore<S> {
    profile: RuntimeProfile,
    store: S,
}

impl<S> MemoryRuntimeBuilderWithStore<S>
where
    S: MemoryStore,
{
    pub fn build(self) -> MemoryRuntime<S> {
        MemoryRuntime {
            profile: self.profile,
            store: self.store,
        }
    }
}

pub struct MemoryRuntime<S> {
    profile: RuntimeProfile,
    store: S,
}

impl<S> MemoryRuntime<S>
where
    S: MemoryStore,
{
    pub fn write(&mut self, candidate: WriteCandidate) -> WriteReport {
        let content = candidate.content.trim();
        if content.is_empty() {
            return WriteReport::rejected("empty_content");
        }

        let Some(source) = candidate.source.as_deref().map(str::trim) else {
            return WriteReport::rejected("missing_source");
        };
        if source.is_empty() {
            return WriteReport::rejected("missing_source");
        }

        let plane = candidate.plane_hint.unwrap_or(MemoryPlane::SharedFactual);
        if !self.profile.allows_plane(plane) {
            return WriteReport::rejected("profile_rejected");
        }

        let record = self.store.insert(NewMemoryRecord {
            identity: candidate.identity,
            scope: candidate.scope,
            content: content.to_owned(),
            source: source.to_owned(),
            domain: plane.domain(),
            plane,
        });

        WriteReport::accepted(&record)
    }

    pub fn recall(&self, query: RecallQuery) -> RecallSelectionReport {
        let mut selected = Vec::new();
        let mut skipped = 0;

        for record in self.store.records() {
            if record.scope != query.scope {
                continue;
            }

            let allowed = query.domain.is_none_or(|domain| domain == record.domain)
                && query.plane.is_none_or(|plane| plane == record.plane);

            if allowed && selected.len() < query.limit {
                selected.push(RecallSelection::from(record));
            } else {
                skipped += 1;
            }
        }

        RecallSelectionReport {
            selected,
            skipped,
            profile: self.profile,
            query,
        }
    }

    pub fn project(
        &self,
        report: &RecallSelectionReport,
        surface: ProjectionSurface,
    ) -> ProjectionReport {
        let blocks = report
            .selected
            .iter()
            .map(|selection| ProjectionBlock {
                record_id: selection.record_id.clone(),
                domain: selection.domain,
                plane: selection.plane,
                content: selection.content.clone(),
            })
            .collect();

        ProjectionReport { surface, blocks }
    }
}
