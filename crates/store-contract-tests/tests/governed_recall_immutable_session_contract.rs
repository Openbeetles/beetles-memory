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
