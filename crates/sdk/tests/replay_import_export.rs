mod support;

use std::sync::Arc;

use bm_sdk::{
    ContinuitySnapshotImportMode, MemoryExportRequest, MemoryImportRequest, MemoryReplayRequest,
    ProfileId,
};

use support::{test_runtime, HostMemoryPlatform};

#[test]
fn runtime_replay_export_import_go_through_sdk_contract() {
    let platform = Arc::new(HostMemoryPlatform::seeded());
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);

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
