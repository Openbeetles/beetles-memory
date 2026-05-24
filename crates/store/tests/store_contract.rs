use bm_core::feature_gate::ProfileId;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind};
use bm_core::platform::{MemorySystemKind, Platform};
use bm_store::{StoreBackendConfig, StorePlatform};

#[test]
fn in_memory_store_platform_covers_core_runtime_paths() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();

    assert_eq!(platform.memory_system_kind(), MemorySystemKind::Standalone);

    let state_fs = platform.state_fs();
    state_fs
        .write("runtime/state.json", br#"{"ready":true}"#)
        .unwrap();
    assert_eq!(
        state_fs.read("runtime/state.json").unwrap(),
        Some(br#"{"ready":true}"#.to_vec())
    );
    assert_eq!(state_fs.list_dir("runtime").unwrap(), vec!["state.json"]);
    state_fs.remove("runtime/state.json").unwrap();
    assert_eq!(state_fs.read("runtime/state.json").unwrap(), None);

    let skill_storage = platform.skill_storage();
    skill_storage.write("runtime-alpha", b"skill body").unwrap();
    assert_eq!(skill_storage.read("runtime-alpha").unwrap(), b"skill body");
    assert_eq!(skill_storage.list_names().unwrap(), vec!["runtime-alpha"]);
    skill_storage.remove("runtime-alpha").unwrap();
    assert!(skill_storage.list_names().unwrap().is_empty());

    let skill_meta = platform.skill_meta_store();
    skill_meta
        .write_meta(
            &["alpha".to_string(), "beta".to_string()],
            &["beta".to_string()],
        )
        .unwrap();
    assert_eq!(
        skill_meta.read_meta().unwrap(),
        (
            vec!["alpha".to_string(), "beta".to_string()],
            vec!["beta".to_string()]
        )
    );

    let sessions = platform.session_store();
    sessions
        .append("chat-a", "user", "remember the camera")
        .unwrap();
    sessions.append("chat-a", "assistant", "noted").unwrap();
    assert_eq!(sessions.message_count("chat-a").unwrap(), 2);
    assert_eq!(
        sessions.load_recent("chat-a", 1).unwrap()[0].content,
        "noted"
    );
    assert_eq!(sessions.list_chat_ids().unwrap(), vec!["chat-a"]);

    let summaries = platform.session_summary_store();
    summaries
        .set_with_count("chat-a", "camera context", 2)
        .unwrap();
    assert_eq!(
        summaries.get_with_count("chat-a").unwrap(),
        Some(("camera context".to_string(), 2))
    );

    let memory = platform.memory_store();
    memory.set_memory("stable memory").unwrap();
    memory
        .write_daily_note("2026-05-20.md", "daily note")
        .unwrap();
    assert_eq!(memory.get_memory().unwrap(), "stable memory");
    assert_eq!(
        memory.list_daily_note_names(1).unwrap(),
        vec!["2026-05-20.md"]
    );

    let long_term = platform.long_term_memory_store();
    let written = long_term
        .upsert_many(
            &[LongTermMemoryDraft {
                kind: LongTermMemoryKind::Fact,
                topic: "camera mode".to_string(),
                content: "Use manual exposure indoors".to_string(),
                keywords: vec!["camera".to_string(), "exposure".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                source_type: None,
                source_scope: None,
                confidence: None,
                freshness: None,
                stale_hint: None,
                supporting_citations: Vec::new(),
                evidence_count: None,
                observed_at: None,
                last_confirmed_at: None,
                source_revision: None,
            }],
            100,
        )
        .unwrap();
    assert_eq!(written, 1);
    assert_eq!(long_term.count().unwrap(), 1);
    assert_eq!(
        long_term.recall("camera", Some("chat-a"), 4).unwrap().len(),
        1
    );

    let _ = platform.active_work_store();
    let _ = platform.execution_state_store();
    let _ = platform.self_model_store();
    let _ = platform.self_authored_core_store();
    let _ = platform.core_revision_ledger_store();
    let _ = platform.relationship_constitution_store();
    let _ = platform.relationship_portfolio_store();
    let _ = platform.relationship_topology_store();
    let _ = platform.private_doc_store();
    let _ = platform.private_garden_store();
    let _ = platform.task_store();
    let _ = platform.task_run_store();
    let _ = platform.task_artifact_store();
    let _ = platform.task_learning_store();

    assert!(platform
        .read_events()
        .unwrap()
        .iter()
        .any(|event| event.kind_name == "memory.write"));
}
