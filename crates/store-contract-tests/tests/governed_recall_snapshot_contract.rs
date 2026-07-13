use bm_core::feature_gate::ProfileId;
use bm_core::memory::{MemoryStore, SessionSummaryStore};
use bm_sdk::nonproduction_replay_harness::{StoreBackendConfig, StorePlatform};

#[test]
fn governed_recall_snapshot_is_immutable_and_rejects_writes() {
    let live = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("live store");
    live.set_memory("memory-v1").expect("seed memory");
    live.write_daily_note("2026-07-11.md", "daily-v1")
        .expect("seed daily");
    live.set("chat-1", "summary-v1").expect("seed summary");

    let snapshot = live
        .load_governed_recall_snapshot()
        .expect("governed recall snapshot");

    live.set_memory("memory-v2").expect("update memory");
    live.write_daily_note("2026-07-11.md", "daily-v2")
        .expect("update daily");
    live.set("chat-1", "summary-v2").expect("update summary");

    assert_eq!(snapshot.platform().get_memory().unwrap(), "memory-v1");
    assert_eq!(
        snapshot.platform().get_daily_note("2026-07-11.md").unwrap(),
        "daily-v1"
    );
    assert_eq!(
        snapshot.platform().get("chat-1").unwrap().as_deref(),
        Some("summary-v1")
    );
    assert!(!snapshot.receipt().state_digest.is_empty());

    let error = snapshot
        .platform()
        .set_memory("must-not-write")
        .expect_err("recall snapshot must be read-only");
    assert_eq!(error.stage(), "governed_recall_snapshot_write_forbidden");
    assert_eq!(live.get_memory().unwrap(), "memory-v2");
}
