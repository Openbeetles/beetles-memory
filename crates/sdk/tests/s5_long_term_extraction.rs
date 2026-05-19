use bm_core::{MemoryPlane, RecallQuery, RuntimeProfile, SourceKind, SourceRef};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn extraction_parser_prepare_and_apply_routes_through_sdk_governance() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();
    let raw = r#"
    [
      {"plane":"factual","op":"upsert","kind":"project","topic":"S5 status","content":"S5 extracts long-term facts","keywords":["s5","memory"]},
      {"plane":"factual","op":"upsert","kind":"project","topic":"S5 status","content":"S5 extracts updated long-term facts","keywords":["s5","archive"]},
      {"plane":"skill","content":"下次抽长期记忆时先走 parser 再走 SDK write"},
      {"plane":"factual","op":"delete","kind":"project","topic":"S5 status"}
    ]
    "#;

    let prepared = runtime.prepare_long_term_extraction(
        raw,
        "agent:s5",
        "task:s5",
        SourceRef::new(SourceKind::LongTermExtraction, "long-term-extraction:test"),
    );

    assert_eq!(prepared.upserts.len(), 1);
    assert_eq!(prepared.deletes.len(), 0);
    assert_eq!(prepared.routed_to_procedural.len(), 1);
    assert_eq!(prepared.dropped_duplicates, 2);

    let applied = runtime.apply_long_term_extraction(prepared);
    assert_eq!(applied.reports.len(), 1);
    assert_eq!(applied.routed_to_procedural, 1);
    assert_eq!(applied.dropped_duplicates, 2);

    let recall = runtime.recall(RecallQuery::new("task:s5").plane(MemoryPlane::SharedFactual));
    assert_eq!(recall.selected.len(), 1);
    assert!(recall.selected[0]
        .content
        .contains("updated long-term facts"));
}
