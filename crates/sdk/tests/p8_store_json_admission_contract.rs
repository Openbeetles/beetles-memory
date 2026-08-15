#![cfg(feature = "nonproduction-replay-harness")]

mod support;

#[test]
fn direct_json_reads_reject_a_malformed_v7_typed_document() {
    let store = support::empty_store_platform(support::host_test_profile());
    let harness = store.replay_harness();
    let namespace = "runtime_skill_records";
    let key = "forged-runtime-skill-owner";

    harness
        .tamper_json_document_for_nonproduction_harness(
            namespace,
            key,
            serde_json::json!({"unexpected": true}),
        )
        .expect("seed malformed typed document through the nonproduction harness");

    let known_key_error = harness
        .read_json_docs_by_keys(namespace, &[key.to_string()])
        .expect_err("known-key direct read must reject malformed typed values");
    assert_eq!(known_key_error.stage(), "store_json_known_key_read");

    let namespace_error = harness
        .read_json_namespace(namespace)
        .expect_err("namespace direct read must reject malformed typed values");
    assert_eq!(namespace_error.stage(), "store_json_namespace_read");

    let export_error = harness
        .export_store_snapshot()
        .expect_err("snapshot export must reject malformed typed values");
    assert_eq!(export_error.stage(), "store_snapshot_export");
}

#[test]
fn governance_job_namespace_rejects_unknown_fields_and_wrong_physical_keys() {
    let store = support::empty_store_platform(support::host_test_profile());
    let harness = store.replay_harness();
    for (key, value) in [
        (
            "ptgj2:00000000000000000000000000000000000000000000000000000000",
            serde_json::json!({"unexpected": true}),
        ),
        (
            "wrong-job-key",
            serde_json::json!({
                "schemaVersion": 2,
                "jobId": "ptgj2:00000000000000000000000000000000000000000000000000000000",
                "unexpected": true
            }),
        ),
    ] {
        harness
            .tamper_json_document_for_nonproduction_harness("post_turn_governance_jobs", key, value)
            .expect("seed malformed governance job");
        let error = harness
            .read_json_docs_by_keys("post_turn_governance_jobs", &[key.to_string()])
            .expect_err("typed governance job must fail closed");
        assert_eq!(error.stage(), "store_json_known_key_read");
    }
}
