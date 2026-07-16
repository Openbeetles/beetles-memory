use bm_sdk::{
    MemoryIdentity, MemoryScope, MemoryStoreHandle, ProfileId, RuntimeSkillWrite,
    StoreBackendConfig,
};

use crate::{
    run_replay_fixture, ReplayExpectedOutcome, ReplayFixture, ReplayOperation, ReplayRunReport,
    ReplayRunnerConfig,
};

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryHarnessReport {
    pub fixture_id: String,
    pub run: ReplayRunReport,
}

pub fn build_sdk_memory_harness_fixture(profile: ProfileId) -> bm_core::Result<ReplayFixture> {
    let platform = MemoryStoreHandle::open_in_memory(StoreBackendConfig::in_memory(profile)?)?;
    let snapshot = platform.export_replay_snapshot()?;
    Ok(ReplayFixture {
        fixture_id: "sdk-memory-harness".to_string(),
        profile,
        store_snapshot: snapshot,
        operations: vec![
            ReplayOperation::WriteProcedural {
                writes: vec![RuntimeSkillWrite {
                    name: "release_guard".to_string(),
                    topic: "release".to_string(),
                    title: "Release artifact guard".to_string(),
                    summary: "Verify release artifacts before publishing.".to_string(),
                    content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                    citations: vec!["replay harness".to_string()],
                    source_chat_id: Some("replay-chat".to_string()),
                    observed_at: 1_800_000_000,
                }],
            },
            ReplayOperation::Recall {
                query: "release artifact".to_string(),
                limit: 4,
            },
            ReplayOperation::Project {
                user_query: "How should I publish?".to_string(),
                system_max_len: 4096,
            },
            ReplayOperation::Inspect {
                query: "release".to_string(),
                system_max_len: 4096,
            },
        ],
        expected: ReplayExpectedOutcome {
            state_fingerprint: String::new(),
            event_fingerprint: String::new(),
            lifecycle_operations: vec![
                "maintain".to_string(),
                "inspect".to_string(),
                "project".to_string(),
            ],
            min_reports: 4,
            required_report_fragments: vec![
                "write accepted=true".to_string(),
                "runtime_skill__release_guard".to_string(),
                "inspect query=release".to_string(),
            ],
        },
    })
}

pub fn run_sdk_memory_harness(backend: StoreBackendConfig) -> bm_core::Result<MemoryHarnessReport> {
    let fixture = build_sdk_memory_harness_fixture(backend.profile())?;
    let mut config = ReplayRunnerConfig::for_backend(backend)?;
    config.identity = MemoryIdentity::new("harness-agent", "harness-owner")?;
    config.scope = MemoryScope::new("harness", "replay-chat")?;
    let run = run_replay_fixture(&fixture, config)?;
    Ok(MemoryHarnessReport {
        fixture_id: fixture.fixture_id,
        run,
    })
}
