mod support;

use std::time::{Duration, Instant};

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    CanonicalTurnDelta, ConversationKey, ConversationScope, MemoryTurnDeliveryStatus,
    MemoryTurnProtocol, MemoryTurnSource, TranscriptInputMessage, TranscriptTurnRecord,
};
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{StoreBackendConfig, StoreCapacityBudget};
use serde_json::json;

fn percentile_micros(samples: &[Duration], percentile: usize) -> u128 {
    let mut micros = samples.iter().map(Duration::as_micros).collect::<Vec<_>>();
    micros.sort_unstable();
    let index = micros.len().saturating_mul(percentile).saturating_add(99) / 100;
    micros[index.saturating_sub(1).min(micros.len().saturating_sub(1))]
}

fn benchmark_turn(key: &ConversationKey, sequence: usize) -> TranscriptTurnRecord {
    let turn_id = format!("turn-{sequence:04}");
    let delta = CanonicalTurnDelta {
        turn_id,
        conversation: ConversationScope {
            channel: key.channel_id.clone(),
            chat_id: "benchmark-chat".to_string(),
            conversation_id: Some(key.conversation_id.clone()),
        },
        subject: "benchmark-subject".to_string(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: bm_core::memory::IngressKind::User,
            channel: key.channel_id.clone(),
            provider: Some("fixed-benchmark-provider".to_string()),
            protocol: MemoryTurnProtocol::Native,
            endpoint: None,
            model_alias: None,
            model_resolved: None,
            request_id: Some(format!("request-{sequence:04}")),
            client_conversation_hint: Some("benchmark-chat".to_string()),
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user(format!(
            "fixed benchmark input {sequence}"
        ))],
        assistant_message: Some(TranscriptInputMessage::assistant(
            "fixed benchmark response",
        )),
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    };
    TranscriptTurnRecord::from_delta(key, 0, &delta, Vec::new(), 1_800_000_000)
        .expect("benchmark turn")
}

fn logical_changed_json_bytes_upper_bound(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
) -> usize {
    let snapshot = platform
        .export_store_snapshot()
        .expect("benchmark snapshot");
    let head = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_recall_manifests")
        .expect("benchmark conversation head");
    let active_page_id = head.value["active_page_id"]
        .as_u64()
        .expect("active page id");
    let active_page_id = format!("{active_page_id:020}");
    let active_page = snapshot
        .json_docs
        .iter()
        .find(|doc| {
            doc.namespace == "conversation_transcript_pages"
                && doc.value["page_id"] == json!(active_page_id)
        })
        .expect("benchmark active page");
    let latest_owner = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "conversation_transcript")
        .max_by_key(|doc| doc.value["sequence"].as_u64().unwrap_or_default())
        .expect("benchmark latest owner");
    [&head.value, &active_page.value, &latest_owner.value]
        .into_iter()
        .map(|value| {
            serde_json::to_vec(value)
                .expect("encode benchmark JSON")
                .len()
        })
        .sum()
}

#[test]
#[ignore = "manual fixed-fixture release benchmark; run with --release --ignored --nocapture"]
fn file_transcript_append_and_tail_benchmark() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-transcript-benchmark-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("file benchmark config")
    .try_with_nonproduction_store_budget_limit(StoreCapacityBudget::full().into_runtime_budget())
    .expect("full benchmark budget");
    let platform = support::open_store(config).expect("open file benchmark store");
    let store = platform.conversation_transcript_store();
    let key =
        ConversationKey::new("benchmark-space", "llm.gateway", "benchmark-conversation").unwrap();
    let mut append_samples = Vec::with_capacity(1_000);

    for sequence in 1..=1_000 {
        let started = Instant::now();
        let report = store
            .append_turn(&benchmark_turn(&key, sequence))
            .expect("append benchmark turn");
        append_samples.push(started.elapsed());
        assert_eq!(report.sequence, sequence as u64);

        if matches!(sequence, 10 | 100 | 1_000) {
            let segment_start = match sequence {
                10 => 0,
                100 => 10,
                1_000 => 100,
                _ => unreachable!(),
            };
            let segment = &append_samples[segment_start..sequence];
            let mut tail_samples = Vec::with_capacity(25);
            for _ in 0..25 {
                let tail_started = Instant::now();
                let tail = store
                    .list_turns(&key, "benchmark-subject", 10)
                    .expect("read benchmark tail");
                tail_samples.push(tail_started.elapsed());
                assert_eq!(tail.len(), 10);
            }
            println!(
                "{}",
                json!({
                    "schema": "beetle-memory.transcript-performance.v1",
                    "backend": "file",
                    "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
                    "fsync_contract": "enabled",
                    "turn_count": sequence,
                    "append_segment_start": segment_start + 1,
                    "append_samples": segment.len(),
                    "append_micros": {
                        "p50": percentile_micros(segment, 50),
                        "p95": percentile_micros(segment, 95),
                        "max": percentile_micros(segment, 100)
                    },
                    "tail_limit": 10,
                    "tail_samples": tail_samples.len(),
                    "tail_micros": {
                        "p50": percentile_micros(&tail_samples, 50),
                        "p95": percentile_micros(&tail_samples, 95),
                        "max": percentile_micros(&tail_samples, 100)
                    },
                    "logical_documents_changed_per_append": 3,
                    "logical_index_documents_changed_per_append": 2,
                    "unrelated_owner_documents_changed_per_append": 0,
                    "logical_changed_json_bytes_upper_bound":
                        logical_changed_json_bytes_upper_bound(&platform)
                })
            );
        }
    }
    let _ = std::fs::remove_dir_all(root);
}
