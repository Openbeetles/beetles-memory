# Generic Rust Host Fixture

This fixture represents a normal SDK consumer:

- opens a `StorePlatform`;
- injects it as `Arc<dyn Platform>`;
- submits `MemoryWriteCandidate` records instead of writing memory planes;
- finalizes turns through canonical turn semantics;
- treats deferred governance jobs as SDK-owned recovery work.
