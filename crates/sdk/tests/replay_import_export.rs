mod support;

use bm_sdk::{
    ContinuitySnapshotImportMode, MemoryExportRequest, MemoryImportRequest, MemoryReplayRequest,
    ProfileId,
};

use support::{seeded_store_platform, test_runtime};

#[test]
fn runtime_replay_export_import_go_through_sdk_contract() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let replay = runtime
        .replay(MemoryReplayRequest {
            chat_id: "chat-1".to_string(),
            limit: 8,
        })
        .expect("replay");
    assert_eq!(replay.chat_id, "chat-1");

    let exported = runtime
        .export(MemoryExportRequest {
            chat_id: "chat-1".to_string(),
        })
        .expect("export");
    assert_eq!(exported.snapshot.chat_id, "chat-1");

    let imported = runtime
        .import(MemoryImportRequest {
            snapshot: exported.snapshot,
            target_chat_id: "chat-2".to_string(),
            mode: ContinuitySnapshotImportMode::FullRestore,
        })
        .expect("import");
    assert!(imported
        .outcome
        .decisions
        .iter()
        .any(|decision| decision.layer == "long_term_memory"));
}
