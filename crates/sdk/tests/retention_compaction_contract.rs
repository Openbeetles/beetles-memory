#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind};
use bm_sdk::{
    MemoryRetentionCompactionRequest, MemoryWriteRequest, ParsedLongTermMemoryExtraction,
    PressureLevel, ProfileId, RuntimeLifecycleModeInput,
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
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform.clone(), profile);
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    privacy: bm_sdk::MemoryPrivacyClass::SharedWithSubject,
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
                    canonical_entities: Vec::new(),
                    evidence_count: Some(9),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_700_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed long-term memory");
    let before_count = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .count()
        .expect("before count");
    let before = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(1)
        .expect("before owner")
        .pop()
        .expect("seeded owner");
    let report = runtime
        .run_retention_compaction(MemoryRetentionCompactionRequest {
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("retention compaction");

    let after_count = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .count()
        .expect("after count");
    assert_eq!(report.owner, "sdk.runtime");
    assert!(report.executed);
    assert_eq!(report.long_term_records_before, before_count);
    assert_eq!(report.long_term_records_after, after_count);
    assert!(after_count >= before_count);
    assert!(report.hygiene.factual_evidence_compacted >= 1);
    let after = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&before.id)
        .expect("after owner")
        .expect("retained owner");
    assert_eq!(after.source_revision, before.source_revision);
    assert_eq!(after.owner_revision, before.owner_revision + 1);
    assert_eq!(after.supporting_citations.len(), 6);
    assert_eq!(after.evidence_count, 6);
    let facet = platform
        .replay_harness()
        .read_json_namespace("memory_facet_indexes")
        .expect("facet namespace")
        .into_iter()
        .find(|doc| doc.value["owner_ref"]["owner_id"] == before.id)
        .expect("owner facet");
    assert_eq!(facet.value["owner_ref"]["owner_plane"], "long_term");
    assert!(facet.value.get("owner_record_id").is_none());
    assert!(facet.value.get("owner_plane").is_none());
    assert_eq!(
        facet
            .value
            .get("owner_revision")
            .and_then(|value| value.as_u64()),
        Some(after.owner_revision)
    );
    assert!(report.transaction.is_some());
    assert!(!report.destructive_deletes_performed);
    assert_eq!(report.host_direct_deletion_allowed, Some(false));
    assert!(report.fail_closed_repair);
}
