mod support;

use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind};
use bm_core::platform::Platform as _;
use bm_sdk::{
    MemoryRetentionCompactionRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput,
};

use support::{empty_store_platform, test_runtime};

#[test]
fn retention_quota_report_is_owned_by_sdk_runtime_not_host_limits() {
    let profile = ProfileId::EspEmbeddedSdk;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);

    let report = runtime.retention_quota_report();

    assert_eq!(report.owner, "sdk.runtime");
    assert!(report.session_transcript.max_recent_turns > 0);
    assert!(report.session_summary.refresh_after_turns > 0);
    assert_eq!(
        report.long_term_quota.max_records,
        runtime
            .runtime_budget()
            .memory_core_budget
            .profile_max_records
    );
    assert_eq!(
        report.compaction.store_snapshot_max_bytes,
        runtime.runtime_budget().store_budget.snapshot_max_bytes
    );
    assert!(report.host_direct_deletion_allowed.is_none());
    assert!(report.fail_closed_repair);
}

#[test]
fn retention_compaction_executor_compacts_metadata_without_deleting_accepted_memory() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    platform
        .long_term_memory_store()
        .upsert_many(
            &[LongTermMemoryDraft {
                kind: LongTermMemoryKind::Project,
                topic: "sdk_retention".to_string(),
                content: "Retention compaction must preserve accepted memory.".to_string(),
                keywords: vec!["retention".to_string(), "compaction".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: None,
                source_scope: None,
                confidence: None,
                freshness: None,
                stale_hint: None,
                supporting_citations: vec![
                    "turn-1".to_string(),
                    "turn-1".to_string(),
                    "turn-2".to_string(),
                    "turn-3".to_string(),
                    "turn-4".to_string(),
                    "turn-5".to_string(),
                    "turn-6".to_string(),
                    "turn-7".to_string(),
                ],
                evidence_count: Some(9),
                observed_at: Some(1_800_000_000),
                last_confirmed_at: Some(1_700_000_000),
                source_revision: Some(1),
            }],
            1_800_000_000,
        )
        .expect("seed long-term memory");
    let before_count = platform
        .long_term_memory_store()
        .count()
        .expect("before count");
    let runtime = test_runtime(platform.clone(), profile);

    let report = runtime
        .run_retention_compaction(MemoryRetentionCompactionRequest {
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("retention compaction");

    let after_count = platform
        .long_term_memory_store()
        .count()
        .expect("after count");
    assert_eq!(report.owner, "sdk.runtime");
    assert!(report.executed);
    assert_eq!(report.long_term_records_before, before_count);
    assert_eq!(report.long_term_records_after, after_count);
    assert!(after_count >= before_count);
    assert!(report.hygiene.factual_evidence_compacted >= 1);
    assert!(!report.destructive_deletes_performed);
    assert_eq!(report.host_direct_deletion_allowed, Some(false));
    assert!(report.fail_closed_repair);
}
