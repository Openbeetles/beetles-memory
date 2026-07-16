mod support;

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    plan_long_term_memory_upsert, LongTermMemoryDraft, LongTermMemoryEntryPlan, LongTermMemoryKind,
    MemoryPrivacyClass,
};
use bm_core::platform::{MemorySystemKind, Platform};
use bm_sdk::nonproduction_replay_harness::StoreBackendConfig;
use support::{delete_scoped_long_term, seed_scoped_long_term};

#[test]
fn scoped_long_term_store_isolates_identical_logical_owners_by_memory_space() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    let space_a = platform
        .scoped_long_term_memory_read_store("space-a")
        .expect("space-a store");
    let space_b = platform
        .scoped_long_term_memory_read_store("space-b")
        .expect("space-b store");
    let draft = |content: &str| LongTermMemoryDraft {
        kind: LongTermMemoryKind::Project,
        topic: "same-logical-owner".to_string(),
        content: content.to_string(),
        keywords: Vec::new(),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: None,
        source_type: None,
        source_scope: None,
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: Vec::new(),
        canonical_entities: Vec::new(),
        evidence_count: None,
        observed_at: None,
        last_confirmed_at: None,
        source_revision: None,
    };

    let entry_a = seed_scoped_long_term(&platform, "space-a", &draft("space a"), 1);
    seed_scoped_long_term(&platform, "space-b", &draft("space b"), 1);
    assert_eq!(space_a.count().unwrap(), 1);
    assert_eq!(space_b.count().unwrap(), 1);
    assert_eq!(space_a.list(8).unwrap()[0].content, "space a");
    assert_eq!(space_b.list(8).unwrap()[0].content, "space b");

    delete_scoped_long_term(&platform, "space-a", &entry_a);
    assert_eq!(space_a.count().unwrap(), 0);
    assert_eq!(space_b.count().unwrap(), 1);
}

#[test]
fn in_memory_store_platform_covers_core_runtime_paths() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
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
    let skill_body = support::seed_runtime_skill(&platform, "runtime-alpha");
    assert_eq!(skill_storage.read("runtime-alpha").unwrap(), skill_body);
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

    let long_term_draft = LongTermMemoryDraft {
        kind: LongTermMemoryKind::Fact,
        privacy: MemoryPrivacyClass::SharedWithSubject,
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
        canonical_entities: Vec::new(),
        evidence_count: None,
        observed_at: None,
        last_confirmed_at: None,
        source_revision: None,
    };
    seed_scoped_long_term(&platform, "space:test", &long_term_draft, 100);
    let long_term = platform
        .scoped_long_term_memory_read_store("space:test")
        .expect("scoped long-term read store");
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

#[test]
fn core_revision_planner_rejects_stale_and_same_revision_conflicts() {
    let make_draft = |content: &str, source_revision: u64| LongTermMemoryDraft {
        kind: LongTermMemoryKind::Project,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        topic: "revision_planner".to_string(),
        content: content.to_string(),
        keywords: vec!["revision".to_string()],
        source_chat_id: Some("chat-a".to_string()),
        source_type: None,
        source_scope: None,
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec!["turn:revision-planner".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: Some(100),
        last_confirmed_at: Some(100),
        source_revision: Some(source_revision),
    };

    let owner = match plan_long_term_memory_upsert(None, &make_draft("v3", 3), 100) {
        LongTermMemoryEntryPlan::Created(owner) => owner,
        other => panic!("expected create, got {other:?}"),
    };
    assert_eq!(owner.owner_revision, 1);
    assert_eq!(
        plan_long_term_memory_upsert(Some(&owner), &make_draft("v3", 3), 200),
        LongTermMemoryEntryPlan::Noop
    );
    assert!(matches!(
        plan_long_term_memory_upsert(Some(&owner), &make_draft("v2", 2), 200),
        LongTermMemoryEntryPlan::Rejected(_)
    ));
    assert!(matches!(
        plan_long_term_memory_upsert(Some(&owner), &make_draft("same-revision-conflict", 3), 200),
        LongTermMemoryEntryPlan::Rejected(_)
    ));
}
