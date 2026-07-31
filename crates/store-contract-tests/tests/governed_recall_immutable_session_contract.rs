#[test]
fn production_recall_has_no_whole_store_snapshot_or_second_platform_path() {
    let runtime = include_str!("../../sdk/src/runtime.rs");
    let platform = include_str!("../../sdk/src/store_internal/platform.rs");
    let store_internal = include_str!("../../sdk/src/store_internal/mod.rs");
    let ops = include_str!("../../sdk/src/ops.rs");
    let sdk = include_str!("../../sdk/src/lib.rs");
    let forbidden_loader = ["load_governed_recall", "_snapshot"].concat();
    let forbidden_type = ["GovernedRecall", "Snapshot"].concat();
    let forbidden_engine = ["ReadOnlySnapshot", "StoreEngine"].concat();

    for source in [runtime, platform, store_internal] {
        assert!(!source.contains(&forbidden_loader));
        assert!(!source.contains(&forbidden_type));
        assert!(!source.contains(&forbidden_engine));
    }
    assert!(runtime.contains("with_recall_immutable_read_session"));
    assert!(runtime.contains("materialize_production_recall_read_view"));
    assert!(!ops.contains("pub store_read_receipt"));
    assert!(!sdk.contains("StorePlatform, StoreReadReceipt"));
}

#[test]
fn production_transcript_has_no_legacy_decoder_migration_or_recall_retry() {
    let runtime = include_str!("../../sdk/src/runtime.rs");
    let platform = include_str!("../../sdk/src/store_internal/platform.rs");
    let recall_index = include_str!("../../sdk/src/store_internal/recall_index.rs");

    for forbidden in [
        "LegacyConversationRecallManifest",
        "ensure_conversation_transcript_v2",
        "ConversationTranscriptMigrationRequired",
        "migration_attempted",
        "conversation.transcript.migrate_v1_to_v2",
    ] {
        assert!(
            !runtime.contains(forbidden)
                && !platform.contains(forbidden)
                && !recall_index.contains(forbidden),
            "production transcript source must not contain legacy path {forbidden}"
        );
    }
}

#[test]
fn production_p8_read_has_one_session_zero_scan_zero_live_fallback() {
    let runtime = include_str!("../../sdk/src/runtime.rs");
    let recall_read = include_str!("../../sdk/src/store_internal/recall_read.rs");
    let owner_start = runtime
        .find("fn materialize_recall_owner(")
        .expect("production owner materializer");
    let owner_end = runtime[owner_start..]
        .find("\n    fn recall_closure_with_feature_flags(")
        .map(|offset| owner_start + offset)
        .expect("owner materializer boundary");
    let owner_materializer = &runtime[owner_start..owner_end];
    let recall_start = runtime
        .find("fn recall_closure_with_feature_flags(")
        .expect("production recall");
    let recall_end = runtime[recall_start..]
        .find("\n    fn ")
        .map(|offset| recall_start + offset)
        .expect("production recall boundary");
    let production_recall = &runtime[recall_start..recall_end];

    assert!(owner_materializer.contains("materialize_long_term_owner_closure"));
    assert!(!owner_materializer.contains("read_runtime_skill_scope_snapshot"));
    assert!(!owner_materializer.contains("read_json_docs_by_keys"));
    assert!(!owner_materializer.contains("open_immutable_read_session"));
    assert!(recall_read.contains("materialize_runtime_skill_scope"));
    assert!(recall_read.contains("RuntimeSkillScopeManifest"));
    assert!(!recall_read.contains("read_runtime_skill_scope_snapshot"));
    assert!(production_recall.contains("let procedural_projection_items = if matches!("));
    assert!(production_recall.contains("build_runtime_skill_projection_items"));
    assert!(production_recall.contains("public_safe_runtime_skill_delivery_views"));
    assert!(production_recall.contains("runtime_skill_materializer_resolver.finish_dispatch"));
    assert!(production_recall.contains("ProductionRecallClosure"));
    assert!(!production_recall.contains("read_runtime_skill_scope_snapshot"));
}

#[test]
fn historical_recall_is_scope_root_known_key_only_and_has_no_current_index_fallback() {
    let runtime = include_str!("../../sdk/src/runtime.rs");
    let recall_read = include_str!("../../sdk/src/store_internal/recall_read.rs");
    let historical_start = recall_read
        .find("pub(crate) fn materialize_long_term_historical_scope(")
        .expect("historical scope materializer");
    let historical_end = recall_read[historical_start..]
        .find("\n    pub(crate) fn ")
        .map(|offset| historical_start + offset)
        .expect("historical materializer boundary");
    let historical_materializer = &recall_read[historical_start..historical_end];

    assert!(historical_materializer.contains("long_term_version_scope_manifest_key"));
    assert!(historical_materializer.contains("materialize_long_term_owner_closure"));
    assert!(historical_materializer.contains("max_as_of_candidates"));
    for forbidden in [
        "list_json_keys",
        "read_json_namespace",
        "read_json_docs_by_keys",
        "open_immutable_read_session",
        "memory_facet",
        "memory_graph",
    ] {
        assert!(
            !historical_materializer.contains(forbidden),
            "historical owner discovery must not use {forbidden}"
        );
    }

    assert!(runtime.contains("MemoryRecallTemporalOperation::HistoricalAsOf"));
    assert!(runtime.contains("historical_long_term_authorities"));
    assert!(runtime.contains("PersistentRecallGraphLoadReport::default()"));
    assert!(!runtime.contains("historical_facet"));
    assert!(!runtime.contains("historical_graph"));
}
