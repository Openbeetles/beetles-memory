use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(dead_code)]
#[path = "../src/p8_semantic.rs"]
mod p8_semantic;

use bm_sdk::{
    MemoryIdentity, MemoryRecallRequest, MemoryRecallTemporalOperation, MemoryRuntime, MemoryScope,
    MemoryStoreHandle, P8SemanticOffRunKey, P8SemanticOffRunRequest, ProfileId, StoreBackendConfig,
};
use p8_semantic::{
    produce_p8_semantic_summary, publish_p8_semantic_bundle_no_clobber, P8AblationEvaluationV1,
    P8AccuracyDecision, P8ArtifactAdmissionKind, P8ArtifactAdmissionLedger,
    P8ArtifactContractFailure, P8ArtifactId, P8ArtifactIdentityV1, P8ArtifactLimits,
    P8BenchmarkFamily, P8BenchmarkJoinReceiptV1, P8CapabilitySlice, P8DatasetStratum,
    P8GateCommandReceiptV1, P8InternalSuite, P8JudgeReceiptV1, P8MemoryUseDecision,
    P8QueryOperationKind, P8ReaderReceiptV1, P8ResourceMeasurement, P8SafetySlice,
    P8SemanticBenchmarkSummaryV1, P8SemanticOperatorReportV1, P8SemanticProducerIdentityV1,
    P8SemanticQuestionDetailInputV1, P8SemanticQuestionDetailV1, P8SemanticQuestionPlanV1,
    P8SemanticRunPlanV1, P8SemanticShardManifestV1, P8SemanticShardSubmissionV1, P8Sha256Digest,
    P8TaskKind, P8TemporalCorpusSlice, P8VerificationReceiptV1, P8VerifierIdentityV1,
    P8_SEMANTIC_BENCHMARK_SUMMARY_SCHEMA, P8_SEMANTIC_GATE_COMMAND_RECEIPT_SCHEMA,
    P8_SEMANTIC_OPERATOR_REPORT_SCHEMA, P8_SEMANTIC_PRODUCER_IDENTITY_SCHEMA,
    P8_SEMANTIC_QUESTION_DETAIL_SCHEMA, P8_SEMANTIC_RUN_PLAN_SCHEMA,
    P8_SEMANTIC_SHARD_MANIFEST_SCHEMA, P8_SEMANTIC_VERIFICATION_RECEIPT_SCHEMA,
    P8_SEMANTIC_VERIFIER_IDENTITY_SCHEMA,
};
use serde::{de::DeserializeOwned, Serialize};

fn assert_strict_artifact_value<T>(value: &T)
where
    T: std::fmt::Debug + PartialEq + Serialize + DeserializeOwned,
{
    let json = serde_json::to_value(value).expect("serialize strict P8 artifact");
    let decoded =
        serde_json::from_value::<T>(json.clone()).expect("deserialize strict P8 artifact");
    assert_eq!(&decoded, value);
    let mut unknown = json;
    unknown
        .as_object_mut()
        .expect("P8 artifact must serialize as an object")
        .insert("__unknown_p8_field".into(), serde_json::json!(true));
    assert!(
        serde_json::from_value::<T>(unknown).is_err(),
        "P8 artifact accepted an unknown top-level field"
    );
}

fn digest(byte: char) -> P8Sha256Digest {
    P8Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
}

fn p8_source_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("bm-replay workspace source root")
        .to_path_buf()
}

struct OperatorProcessResult {
    success: bool,
    stdout: String,
    stderr: String,
    report: Option<P8SemanticOperatorReportV1>,
}

fn run_gate_parent_process(receipt_path: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bm-p8-semantic-gate-parent"))
        .arg(env!("CARGO_BIN_EXE_bm-p8-semantic-operator"))
        .arg(p8_source_root())
        .arg(receipt_path)
        .output()
        .expect("spawn fixed P8 gate parent")
}

fn run_operator_process(
    root: &std::path::Path,
    gate_path: &std::path::Path,
    label: &str,
) -> OperatorProcessResult {
    let report_path = root.with_extension(format!("operator-{label}.json"));
    let output = Command::new(env!("CARGO_BIN_EXE_bm-p8-semantic-operator"))
        .arg(root)
        .arg(gate_path)
        .arg(&report_path)
        .output()
        .expect("spawn independent P8 operator");
    let report = report_path
        .try_exists()
        .expect("inspect report path")
        .then(|| {
            let report = serde_json::from_slice(
                &fs::read(&report_path).expect("read independent operator report"),
            )
            .expect("strict independent operator report");
            fs::remove_file(&report_path).expect("remove test-owned operator report");
            report
        });
    OperatorProcessResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        report,
    }
}

fn assert_operator_rejects(
    root: &std::path::Path,
    gate_path: &std::path::Path,
    label: &str,
    expected: P8ArtifactContractFailure,
) {
    let result = run_operator_process(root, gate_path, label);
    assert!(!result.success, "operator unexpectedly accepted {label}");
    let report_has_failure = result
        .report
        .as_ref()
        .is_some_and(|report| report.mismatches().contains(&expected));
    assert!(
        report_has_failure || result.stderr.contains(&format!("{expected:?}")),
        "missing {expected:?} for {label}; stdout={}; stderr={}; report={:?}",
        result.stdout,
        result.stderr,
        result.report
    );
}

fn artifact_id(value: &str) -> P8ArtifactId {
    P8ArtifactId::parse(value).expect("typed id")
}

fn one_question_artifacts() -> (
    P8SemanticProducerIdentityV1,
    P8SemanticRunPlanV1,
    P8SemanticQuestionDetailV1,
) {
    let profile = ProfileId::native_dev_full().expect("host-native dev-full");
    let platform =
        MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile).expect("store config"))
            .expect("store");
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("p8-agent", "p8-owner").expect("identity"))
        .scope(MemoryScope::new("p8", "one-question").expect("scope"))
        .store(platform)
        .build()
        .expect("runtime");
    let sdk_report = runtime
        .p8_semantic_off_run(P8SemanticOffRunRequest::new(MemoryRecallRequest {
            query: "current release premise".into(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        }))
        .expect("SDK report");

    let producer =
        P8SemanticProducerIdentityV1::build(digest('1'), digest('2'), digest('3'), digest('4'));
    let run_id = artifact_id("p8-contract-run");
    let question_id = artifact_id("q-1");
    let question_digest = digest('6');
    let run_plan = P8SemanticRunPlanV1::build(
        run_id.clone(),
        producer.identity_digest().clone(),
        digest('5'),
        1,
        vec![P8SemanticQuestionPlanV1::new(
            question_id.clone(),
            question_digest.clone(),
            0,
        )],
    )
    .expect("run plan");
    let evaluations = P8SemanticOffRunKey::ALL
        .into_iter()
        .map(|key| {
            (
                key,
                P8AblationEvaluationV1::new(
                    P8AccuracyDecision::Correct,
                    P8AccuracyDecision::Incorrect,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let detail = P8SemanticQuestionDetailV1::build(P8SemanticQuestionDetailInputV1 {
        producer_identity_digest: producer.identity_digest().clone(),
        run_plan_digest: run_plan.run_plan_digest().clone(),
        run_id,
        shard_index: 0,
        shard_total: 1,
        question_id: question_id.clone(),
        dataset_manifest_digest: digest('5'),
        question_manifest_digest: question_digest.clone(),
        benchmark_family: P8BenchmarkFamily::BeetleInternal,
        dataset_stratum: P8DatasetStratum::BeetleInternal {
            suite: P8InternalSuite::Safety,
        },
        capability_slices: vec![P8CapabilitySlice::DynamicState],
        task_kind: P8TaskKind::Reason,
        query_operation_kind: P8QueryOperationKind::CurrentQuery,
        temporal_corpus_slice: P8TemporalCorpusSlice::NotApplicable,
        safety_slices: vec![P8SafetySlice::Invalidation],
        profile,
        capability_identity: digest('7'),
        budget_identity: digest('8'),
        sdk_off_run_report: sdk_report,
        reader_receipt: P8ReaderReceiptV1::build(question_id.clone(), digest('9'), digest('a')),
        judge_receipt: P8JudgeReceiptV1::build(
            question_id.clone(),
            digest('b'),
            P8AccuracyDecision::Correct,
        ),
        benchmark_join_receipt: P8BenchmarkJoinReceiptV1::build(
            question_id,
            digest('5'),
            question_digest,
            digest('c'),
        ),
        output_digest: digest('d'),
        accuracy_decision: P8AccuracyDecision::Correct,
        memory_use_decision: P8MemoryUseDecision::NotApplicable,
        resource: P8ResourceMeasurement {
            elapsed_millis: 1,
            peak_rss_bytes: 1024,
        },
        ablation_evaluations: evaluations,
        failures: Vec::new(),
    })
    .expect("detail");
    (producer, run_plan, detail)
}

fn one_question_submission(
    run_plan: &P8SemanticRunPlanV1,
    detail: P8SemanticQuestionDetailV1,
) -> P8SemanticShardSubmissionV1 {
    let detail_bytes = serde_json::to_vec(&detail).expect("detail bytes").len() as u64 + 1;
    let detail_artifact = P8ArtifactIdentityV1::build(
        artifact_id("shard-00000.details.jsonl"),
        digest('d'),
        digest('e'),
        detail_bytes,
        1,
    );
    let manifest = P8SemanticShardManifestV1::build(
        run_plan,
        0,
        detail_artifact,
        std::slice::from_ref(&detail),
    )
    .expect("manifest");
    let manifest_bytes = serde_json::to_vec(&manifest).expect("manifest bytes").len() as u64;
    P8SemanticShardSubmissionV1::from_single_read(
        manifest,
        vec![detail],
        manifest_bytes,
        detail_bytes,
        digest('f'),
        digest('e'),
    )
}

#[test]
fn p8_v1_owner_set_is_typed_and_complete() {
    let schemas = [
        P8_SEMANTIC_PRODUCER_IDENTITY_SCHEMA,
        P8_SEMANTIC_RUN_PLAN_SCHEMA,
        P8_SEMANTIC_QUESTION_DETAIL_SCHEMA,
        P8_SEMANTIC_SHARD_MANIFEST_SCHEMA,
        P8_SEMANTIC_BENCHMARK_SUMMARY_SCHEMA,
        P8_SEMANTIC_VERIFIER_IDENTITY_SCHEMA,
        P8_SEMANTIC_GATE_COMMAND_RECEIPT_SCHEMA,
        P8_SEMANTIC_VERIFICATION_RECEIPT_SCHEMA,
        P8_SEMANTIC_OPERATOR_REPORT_SCHEMA,
    ];
    assert_eq!(schemas.into_iter().collect::<BTreeSet<_>>().len(), 9);
    assert!(schemas
        .into_iter()
        .all(|schema| schema.starts_with("beetle-memory.p8.") && schema.ends_with(".v1")));
}

#[test]
fn p8_reader_judge_and_join_receipts_reject_wrong_domain_json() {
    let question_id = artifact_id("question-receipt-domain");
    let reader = P8ReaderReceiptV1::build(question_id.clone(), digest('1'), digest('2'));
    let judge = P8JudgeReceiptV1::build(
        question_id.clone(),
        digest('3'),
        P8AccuracyDecision::Correct,
    );
    let join = P8BenchmarkJoinReceiptV1::build(question_id, digest('4'), digest('5'), digest('6'));
    let reader_json = serde_json::to_value(&reader).expect("reader receipt JSON");
    let judge_json = serde_json::to_value(&judge).expect("judge receipt JSON");
    let join_json = serde_json::to_value(&join).expect("join receipt JSON");
    let reader_receipt = reader_json["receipt_digest"].clone();
    let judge_receipt = judge_json["receipt_digest"].clone();
    let join_receipt = join_json["receipt_digest"].clone();

    let mut wrong_reader = reader_json;
    wrong_reader["receipt_digest"] = judge_receipt;
    assert!(serde_json::from_value::<P8ReaderReceiptV1>(wrong_reader).is_err());
    let mut wrong_judge = judge_json;
    wrong_judge["receipt_digest"] = join_receipt;
    assert!(serde_json::from_value::<P8JudgeReceiptV1>(wrong_judge).is_err());
    let mut wrong_join = join_json;
    wrong_join["receipt_digest"] = reader_receipt;
    assert!(serde_json::from_value::<P8BenchmarkJoinReceiptV1>(wrong_join).is_err());
}

#[test]
fn p8_v1_replaces_raw_reports_free_receipts_and_naked_merge_in_place() {
    let source = include_str!("../src/p8_semantic.rs");
    for forbidden in [
        "GovernedRecallEligibilityReport",
        "MemoryUpdateLineageReport",
        "PremiseEvaluationReport",
        "pub store_snapshot_receipt: String",
        "pub operation_authority_snapshot_receipt: String",
        "pub reader_identity: String",
        "pub judge_identity: String",
        "pub production_candidate_receipts: Vec<String>",
        "merge_p8_semantic_details",
        "verify_p8_semantic_summary(",
        "verification_receipt: String",
    ] {
        assert!(
            !source.contains(forbidden),
            "P8 v1 still exposes a forbidden bypass: {forbidden}"
        );
    }
    let lib = include_str!("../src/lib.rs");
    for forbidden_export in [
        "merge_p8_semantic_details",
        "verify_p8_semantic_summary",
        "P8VerifiedShardSet",
    ] {
        assert!(
            !lib.contains(forbidden_export),
            "P8 public surface exports a forbidden bypass: {forbidden_export}"
        );
    }
}

#[test]
fn p8_artifact_limits_accept_exact_and_reject_n_plus_one_before_read_or_parse() {
    let limits = P8ArtifactLimits::V1;
    assert_eq!(limits.detail_line_bytes(), 16 * 1024 * 1024);
    assert_eq!(limits.control_json_bytes(), 64 * 1024 * 1024);
    assert_eq!(limits.shard_detail_bytes(), 2 * 1024 * 1024 * 1024);
    assert_eq!(limits.total_detail_bytes(), 8 * 1024 * 1024 * 1024);
    assert_eq!(
        limits.total_operator_artifact_bytes(),
        10 * 1024 * 1024 * 1024
    );
    assert_eq!(limits.retained_handles(), 4096);
    assert_eq!(limits.operator_wall_millis(), 30 * 60 * 1000);

    for (kind, exact) in [
        (
            P8ArtifactAdmissionKind::DetailLine,
            limits.detail_line_bytes(),
        ),
        (
            P8ArtifactAdmissionKind::ControlJson,
            limits.control_json_bytes(),
        ),
        (
            P8ArtifactAdmissionKind::ShardDetails,
            limits.shard_detail_bytes(),
        ),
    ] {
        let mut exact_ledger = P8ArtifactAdmissionLedger::new(limits);
        assert!(exact_ledger.admit_declared(kind, exact).is_ok());
        assert_eq!(exact_ledger.read_pass_count(), 0);
        assert_eq!(exact_ledger.parsed_document_count(), 0);

        let mut over_ledger = P8ArtifactAdmissionLedger::new(limits);
        assert!(over_ledger
            .admit_declared(kind, exact.checked_add(1).expect("N+1"))
            .is_err());
        assert_eq!(over_ledger.read_pass_count(), 0);
        assert_eq!(over_ledger.parsed_document_count(), 0);
    }

    let mut detail_aggregate = P8ArtifactAdmissionLedger::new(limits);
    for _ in 0..4 {
        assert!(detail_aggregate
            .admit_declared(
                P8ArtifactAdmissionKind::ShardDetails,
                limits.shard_detail_bytes()
            )
            .is_ok());
    }
    assert!(detail_aggregate
        .admit_declared(P8ArtifactAdmissionKind::ShardDetails, 1)
        .is_err());
    assert_eq!(detail_aggregate.read_pass_count(), 0);
    assert_eq!(detail_aggregate.parsed_document_count(), 0);

    let mut operator_aggregate = P8ArtifactAdmissionLedger::new(limits);
    for _ in 0..4 {
        assert!(operator_aggregate
            .admit_declared(
                P8ArtifactAdmissionKind::ShardDetails,
                limits.shard_detail_bytes()
            )
            .is_ok());
    }
    for _ in 0..32 {
        assert!(operator_aggregate
            .admit_declared(
                P8ArtifactAdmissionKind::ControlJson,
                limits.control_json_bytes()
            )
            .is_ok());
    }
    assert!(operator_aggregate
        .admit_declared(P8ArtifactAdmissionKind::ControlJson, 1)
        .is_err());
    assert_eq!(operator_aggregate.read_pass_count(), 0);
    assert_eq!(operator_aggregate.parsed_document_count(), 0);

    let mut handle_ledger = P8ArtifactAdmissionLedger::new(limits);
    for _ in 0..limits.retained_handles() {
        assert!(handle_ledger.admit_retained_handle().is_ok());
    }
    assert_eq!(
        handle_ledger.retained_handle_count(),
        limits.retained_handles()
    );
    assert!(handle_ledger.admit_retained_handle().is_err());
}

#[test]
fn p8_operator_fold_is_independent_from_producer_fold() {
    let operator_source = include_str!("../src/p8_semantic_operator.rs");
    let operator_binary = include_str!("../src/bin/bm-p8-semantic-operator.rs");
    let process_authority = include_str!("../src/p8_process_authority.rs");
    let artifact_directory = include_str!("../src/p8_artifact_dir.rs");
    let retained_filesystem = include_str!("../src/retained_artifact_fs.rs");
    let gate_parent = include_str!("../src/p8_gate_parent.rs");
    let gate_binary = include_str!("../src/bin/bm-p8-semantic-gate-parent.rs");
    let semantic_source = include_str!("../src/p8_semantic.rs");
    let build_source = include_str!("../build.rs");
    let build_support_source = include_str!("../build_support.rs");
    let library_source = include_str!("../src/lib.rs");
    for forbidden in [
        "producer_fold",
        "merge_verified_shards",
        "P8VerifiedShardSet",
        "supplied_summary.clone()",
        "produce_p8_semantic_summary",
        "verify_shard_submissions",
    ] {
        assert!(
            !operator_source.contains(forbidden),
            "independent operator reused producer authority: {forbidden}"
        );
        assert!(
            !operator_binary.contains(forbidden),
            "operator binary reused producer authority: {forbidden}"
        );
    }
    assert!(!library_source.contains("verify_p8_semantic_bundle"));
    assert!(operator_source.contains("read_artifact_bytes_once"));
    assert!(operator_source.contains("recompute_summary_from_admitted_bytes"));
    assert!(operator_source.matches("ensure_deadline(").count() >= 8);
    assert!(operator_source.contains("run_p8_gate_contract"));
    assert!(operator_binary.contains("run_bounded_command"));
    assert!(operator_binary.contains("claim_internal_child_authority"));
    assert!(process_authority.contains("parent_executable"));
    assert!(process_authority.contains("install_authority_pipe"));
    assert!(artifact_directory.contains("install_file_no_replace_terminal"));
    assert!(retained_filesystem.contains("renameat2"));
    assert!(retained_filesystem.contains("renameatx_np"));
    assert!(retained_filesystem.contains("SetFileInformationByHandle"));
    assert!(retained_filesystem.contains("discard_same_file"));
    assert!(gate_parent.contains("install_file_no_replace_terminal"));
    assert!(!gate_binary.contains("println!(\"P8_SEMANTIC_GATE_PARENT_OK"));
    assert!(!gate_binary.contains("p7_secure_fs"));
    assert!(!operator_binary.contains("p7_secure_fs"));
    assert!(!semantic_source.contains("include_bytes!(\"p7_secure_fs.rs\")"));
    let p8_source_inputs = build_source
        .split("const P8_VERIFIER_SOURCE_INPUTS")
        .nth(1)
        .and_then(|source| source.split("];").next())
        .expect("P8 verifier source inputs");
    assert!(p8_source_inputs.contains("retained_artifact_fs.rs"));
    assert!(p8_source_inputs.contains("\"src/lib.rs\""));
    assert!(p8_source_inputs.contains("\"src/p8_quality\""));
    assert!(!p8_source_inputs.contains("p7_secure_fs.rs"));
    let verifier_identity_source = semantic_source
        .split("impl P8VerifierIdentityV1")
        .nth(1)
        .and_then(|source| source.split("pub fn identity_digest").next())
        .expect("P8 verifier identity source");
    assert!(verifier_identity_source.contains("P8_VERIFIER_SOURCE_FINGERPRINT"));
    assert!(!verifier_identity_source.contains("include_bytes!"));
    let sdk_workspace_inputs = build_support_source
        .split("const P7_SDK_BUILD_INPUTS")
        .nth(1)
        .and_then(|source| source.split("];").next())
        .expect("shared SDK workspace semantic inputs");
    for required in [
        "\"Cargo.lock\"",
        "\"crates/core/src\"",
        "\"crates/sdk/src\"",
    ] {
        assert!(sdk_workspace_inputs.contains(required));
    }
    assert!(!sdk_workspace_inputs.contains("crates/replay"));
    assert!(build_source.contains(
        "let workspace_semantic_inputs = p8_fingerprint_inputs(root, &P7_SDK_BUILD_INPUTS)"
    ));
    assert!(build_source.contains("&[&p8_source_fingerprint, &workspace_semantic_fingerprint]"));
    assert!(!operator_binary.contains("hard_link"));
    let public_supervisor = operator_binary
        .split("fn run_public_supervisor")
        .nth(1)
        .and_then(|source| source.split("fn run_authorized_internal_verifier").next())
        .expect("public supervisor source");
    let post_commit = public_supervisor
        .split(".install_file_no_replace_terminal")
        .nth(1)
        .expect("terminal commit call");
    for forbidden_after_commit in ["remove_file", "write_all", "sync_all", "PUBLIC_SUCCESS"] {
        assert!(
            !post_commit.contains(forbidden_after_commit),
            "fallible post-commit action remained: {forbidden_after_commit}"
        );
    }
    assert!(!library_source.contains("mod p8_semantic_operator"));
    assert!(!library_source.contains("run_p8_semantic_operator"));
    assert!(!operator_binary.contains("/usr/bin/printf"));
    assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/bin/bm-p8-semantic-operator-worker.rs")
        .exists());
}

#[test]
fn p8_internal_verifier_role_rejects_direct_execution_without_parent_authority() {
    let staging = std::env::temp_dir().join(format!(
        "bm-p8-direct-internal-staging-{}",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_bm-p8-semantic-operator"))
        .arg("--p8-internal-verify")
        .arg("missing-bundle")
        .arg("missing-gate")
        .output()
        .expect("spawn direct internal role");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("authority"));
    assert!(!staging.try_exists().expect("inspect direct staging path"));

    let authority_input = staging.with_extension("authority");
    fs::write(&authority_input, format!("{}\n", "0".repeat(64)))
        .expect("write forged authority input");
    let forged = Command::new(env!("CARGO_BIN_EXE_bm-p8-semantic-operator"))
        .arg("--p8-internal-verify")
        .arg("missing-bundle")
        .arg("missing-gate")
        .env("BM_P8_INTERNAL_PARENT_PID", std::process::id().to_string())
        .env("BM_P8_INTERNAL_AUTHORITY_TOKEN", "0".repeat(64))
        .env(
            "BM_P8_INTERNAL_PARENT_EXECUTABLE_IDENTITY",
            format!("p8_verifier_executable_identity:sha256:{}", "0".repeat(64)),
        )
        .stdin(Stdio::from(
            fs::File::open(&authority_input).expect("open forged authority input"),
        ))
        .output()
        .expect("spawn forged direct internal role");
    assert!(!forged.status.success());
    assert!(!staging.try_exists().expect("inspect forged staging path"));
    fs::remove_file(authority_input).expect("remove forged authority input");
}

#[test]
fn p8_producer_accepts_only_exact_verified_shards_and_safe_sdk_report() {
    let (_, run_plan, detail) = one_question_artifacts();
    let submission = one_question_submission(&run_plan, detail);
    let summary =
        produce_p8_semantic_summary(&run_plan, vec![submission]).expect("verified producer fold");
    assert_eq!(summary.overall().question_count, 1);
    assert_eq!(summary.overall().correct_count, 1);
    assert_eq!(summary.ablation_deltas().len(), 8);

    let serialized = serde_json::to_string(&summary).expect("summary JSON");
    for forbidden in [
        "GovernedRecallEligibilityReport",
        "MemoryUpdateLineageReport",
        "PremiseEvaluationReport",
        "private-owner",
        "raw-procedure",
        "credential",
        "\"path\"",
    ] {
        assert!(!serialized.contains(forbidden));
    }

    let (_, run_plan, detail) = one_question_artifacts();
    let submission = one_question_submission(&run_plan, detail);
    assert!(produce_p8_semantic_summary(&run_plan, Vec::new())
        .expect_err("missing shard must fail")
        .contains(&P8ArtifactContractFailure::ShardCoverageMismatch));
    assert!(
        produce_p8_semantic_summary(&run_plan, vec![submission.clone(), submission])
            .expect_err("duplicate shard must fail")
            .contains(&P8ArtifactContractFailure::ShardCoverageMismatch)
    );
}

#[test]
fn p8_no_clobber_bundle_and_independent_operator_recompute_from_file_bytes() {
    let (producer, run_plan, detail) = one_question_artifacts();
    let strict_detail = detail.clone();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_nanos();
    let root = fs::canonicalize(std::env::temp_dir())
        .expect("canonical test temp directory")
        .join(format!(
            "beetle-p8-semantic-operator-{}-{nonce}",
            std::process::id()
        ));
    let published =
        publish_p8_semantic_bundle_no_clobber(&root, &producer, &run_plan, vec![vec![detail]])
            .expect("no-clobber bundle");
    assert_eq!(published.artifact_count(), 5);
    assert!(published.total_bytes() > 0);
    assert!(
        publish_p8_semantic_bundle_no_clobber(&root, &producer, &run_plan, vec![Vec::new()])
            .expect_err("existing root must never be overwritten")
            .contains(&P8ArtifactContractFailure::DuplicateArtifact)
    );
    let strict_manifest: P8SemanticShardManifestV1 = serde_json::from_slice(
        &fs::read(root.join("shard-00000.manifest.json")).expect("strict manifest bytes"),
    )
    .expect("strict manifest");
    let strict_summary: P8SemanticBenchmarkSummaryV1 =
        serde_json::from_slice(&fs::read(root.join("summary.json")).expect("strict summary bytes"))
            .expect("strict summary");
    assert_strict_artifact_value(&producer);
    assert_strict_artifact_value(&run_plan);
    assert_strict_artifact_value(&strict_detail);
    assert_strict_artifact_value(&strict_manifest);
    assert_strict_artifact_value(&strict_summary);

    let gate_path = root.with_extension("gate-receipt.json");
    let fake_gate_path = root.with_extension("fake-gate-receipt.json");
    let fake_gate = Command::new(env!("CARGO_BIN_EXE_bm-p8-semantic-gate-parent"))
        .arg(env!("CARGO_BIN_EXE_bm-p8-semantic-gate-parent"))
        .arg(p8_source_root())
        .arg(&fake_gate_path)
        .output()
        .expect("spawn fake verifier gate");
    assert!(!fake_gate.status.success());
    assert!(!fake_gate_path.try_exists().expect("inspect fake gate path"));
    let gate_output = run_gate_parent_process(&gate_path);
    assert!(
        gate_output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&gate_output.stdout),
        String::from_utf8_lossy(&gate_output.stderr)
    );
    let duplicate_gate = run_gate_parent_process(&gate_path);
    assert!(!duplicate_gate.status.success());
    assert!(String::from_utf8_lossy(&duplicate_gate.stderr).contains("DuplicateArtifact"));
    let gate_stdout_path = std::path::PathBuf::from(format!("{}.stdout", gate_path.display()));
    let gate_stderr_path = std::path::PathBuf::from(format!("{}.stderr", gate_path.display()));
    let gate_bytes = fs::read(&gate_path).expect("gate receipt bytes");
    assert_eq!(
        fs::read(&gate_path).expect("gate receipt remains after duplicate attempt"),
        gate_bytes
    );
    let gate_json: serde_json::Value =
        serde_json::from_slice(&gate_bytes).expect("strict gate receipt JSON");
    let strict_gate: P8GateCommandReceiptV1 =
        serde_json::from_value(gate_json.clone()).expect("strict gate receipt");
    assert_strict_artifact_value(&strict_gate);
    for forged in [
        {
            let mut value = gate_json.clone();
            value
                .as_object_mut()
                .expect("gate object")
                .remove("receipt_digest");
            value
        },
        {
            let mut value = gate_json.clone();
            value["unexpected"] = serde_json::json!(true);
            value
        },
        {
            let mut value = gate_json.clone();
            value["stdout_digest"] =
                serde_json::json!(format!("p8_closed_stderr:sha256:{}", "0".repeat(64)));
            value
        },
        {
            let mut value = gate_json.clone();
            value["stdout_digest"] =
                serde_json::json!(format!("p8_closed_stdout:sha256:{}", "A".repeat(64)));
            value
        },
        {
            let mut value = gate_json.clone();
            value["stdout_digest"] =
                serde_json::json!(format!("p8_closed_stdout:sha256:{}", "0".repeat(63)));
            value
        },
    ] {
        assert!(
            serde_json::from_value::<P8GateCommandReceiptV1>(forged).is_err(),
            "gate receipt accepted missing, unknown, wrong-domain, uppercase, or wrong-length evidence"
        );
    }
    let gate_text = String::from_utf8(gate_bytes.clone()).expect("gate receipt UTF-8");
    let duplicate_schema = gate_text.replacen(
        "\"schema\":",
        "\"schema\":\"beetle-memory.p8.semantic-gate-command-receipt.v1\",\"schema\":",
        1,
    );
    assert!(serde_json::from_str::<P8GateCommandReceiptV1>(&duplicate_schema).is_err());

    let preexisting_report_path = root.with_extension("operator-preexisting.json");
    fs::write(&preexisting_report_path, b"preexisting-winner")
        .expect("write preexisting operator report");
    let preexisting = Command::new(env!("CARGO_BIN_EXE_bm-p8-semantic-operator"))
        .arg(&root)
        .arg(&gate_path)
        .arg(&preexisting_report_path)
        .output()
        .expect("spawn no-replace operator");
    assert!(!preexisting.status.success());
    assert_eq!(
        fs::read(&preexisting_report_path).expect("read preexisting winner"),
        b"preexisting-winner"
    );
    fs::remove_file(&preexisting_report_path).expect("remove preexisting winner");

    let operator = run_operator_process(&root, &gate_path, "baseline");
    assert!(
        operator.success,
        "stdout={}\nstderr={}",
        operator.stdout, operator.stderr
    );
    let report = operator.report.expect("operator report");
    assert_strict_artifact_value(&report);
    let report_json = serde_json::to_value(&report).expect("operator report JSON");
    let strict_verifier: P8VerifierIdentityV1 =
        serde_json::from_value(report_json["verifier_identity"].clone())
            .expect("strict verifier identity");
    assert_eq!(
        report_json["verifier_identity"]["execution_evidence"],
        serde_json::json!("observed_path_stable")
    );
    let strict_verification: P8VerificationReceiptV1 =
        serde_json::from_value(report_json["verification_receipt"].clone())
            .expect("strict verification receipt");
    assert_strict_artifact_value(&strict_verifier);
    assert_strict_artifact_value(&strict_verification);
    assert_eq!(
        report.mismatches(),
        &[P8ArtifactContractFailure::QualityThresholdsNotFrozen]
    );
    assert!(!report.release_eligible());
    assert_eq!(
        report.verification_receipt().admitted_artifact_bytes(),
        report.verification_receipt().bytes_read()
    );
    assert_eq!(report.verification_receipt().artifact_read_count(), 8);

    let manifest_path = root.join("shard-00000.manifest.json");
    let missing_manifest_path = root.join("shard-00000.manifest.missing");
    fs::rename(&manifest_path, &missing_manifest_path).expect("isolate missing manifest");
    assert_operator_rejects(
        &root,
        &gate_path,
        "missing-manifest",
        P8ArtifactContractFailure::ShardCoverageMismatch,
    );
    fs::rename(&missing_manifest_path, &manifest_path).expect("restore manifest");

    let extra_path = root.join("unexpected-artifact.json");
    fs::write(&extra_path, b"{}").expect("write unexpected artifact");
    assert_operator_rejects(
        &root,
        &gate_path,
        "extra-artifact",
        P8ArtifactContractFailure::ShardCoverageMismatch,
    );
    fs::remove_file(&extra_path).expect("remove unexpected artifact");

    let manifest_bytes = fs::read(&manifest_path).expect("manifest bytes");
    let mut forged_manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("manifest JSON");
    forged_manifest["detail_artifact"]["artifact_id"] =
        serde_json::json!("shard-99999.details.jsonl");
    fs::write(
        &manifest_path,
        serde_json::to_vec(&forged_manifest).expect("forged manifest JSON"),
    )
    .expect("write forged detail artifact id");
    assert_operator_rejects(
        &root,
        &gate_path,
        "detail-artifact-id",
        P8ArtifactContractFailure::IdentityInvalid,
    );
    fs::write(&manifest_path, &manifest_bytes).expect("restore manifest");

    let summary_path = root.join("summary.json");
    let summary_bytes = fs::read(&summary_path).expect("summary bytes");
    let mut raw_private: serde_json::Value =
        serde_json::from_slice(&summary_bytes).expect("summary JSON");
    raw_private["forbidden"] = serde_json::json!("private-owner-sentinel");
    fs::write(
        &summary_path,
        serde_json::to_vec(&raw_private).expect("raw-private tamper JSON"),
    )
    .expect("write raw-private tamper");
    assert_operator_rejects(
        &root,
        &gate_path,
        "raw-private",
        P8ArtifactContractFailure::SdkReportInvalid,
    );
    fs::write(&summary_path, &summary_bytes).expect("restore summary");

    let detail_path = root.join("shard-00000.details.jsonl");
    let detail_bytes = fs::read(&detail_path).expect("detail bytes");
    let forged_detail = String::from_utf8(detail_bytes.clone())
        .expect("detail UTF-8")
        .replacen("\"q-1\"", "\"q-2\"", 1);
    fs::write(&detail_path, forged_detail).expect("write digest/identity tamper");
    assert_operator_rejects(
        &root,
        &gate_path,
        "detail-digest",
        P8ArtifactContractFailure::PhysicalIdentityMismatch,
    );
    fs::write(&detail_path, &detail_bytes).expect("restore detail");

    let mut raw_procedure = detail_bytes.clone();
    raw_procedure.splice(
        raw_procedure.len().saturating_sub(1)..raw_procedure.len().saturating_sub(1),
        b"raw-procedure-sentinel".iter().copied(),
    );
    fs::write(&detail_path, raw_procedure).expect("write raw-procedure tamper");
    assert_operator_rejects(
        &root,
        &gate_path,
        "raw-procedure",
        P8ArtifactContractFailure::SdkReportInvalid,
    );
    fs::write(&detail_path, &detail_bytes).expect("restore detail after raw tamper");

    let mut forged_receipt = gate_json.clone();
    forged_receipt["receipt_digest"] =
        serde_json::json!(format!("p8_gate_command_receipt:sha256:{}", "0".repeat(64)));
    fs::write(
        &gate_path,
        serde_json::to_vec(&forged_receipt).expect("forged receipt JSON"),
    )
    .expect("isolated gate receipt tamper");
    assert_operator_rejects(
        &root,
        &gate_path,
        "forged-gate-receipt",
        P8ArtifactContractFailure::DigestInvalid,
    );
    fs::write(&gate_path, &gate_bytes).expect("restore gate receipt");

    let closed_stdout = fs::read(&gate_stdout_path).expect("closed stdout");
    let mut appended_stdout = closed_stdout.clone();
    appended_stdout.extend_from_slice(b"tamper");
    fs::write(&gate_stdout_path, appended_stdout).expect("tamper stdout sidecar");
    assert_operator_rejects(
        &root,
        &gate_path,
        "stdout-tamper",
        P8ArtifactContractFailure::ReceiptInvalid,
    );
    fs::write(&gate_stdout_path, &closed_stdout).expect("restore stdout sidecar");

    fs::remove_file(&gate_stderr_path).expect("remove test-owned stderr sidecar");
    fs::hard_link(&gate_stdout_path, &gate_stderr_path).expect("hardlink duplicate sidecar");
    assert_operator_rejects(
        &root,
        &gate_path,
        "hardlink-sidecar",
        P8ArtifactContractFailure::DuplicateArtifact,
    );
    fs::remove_file(&gate_stderr_path).expect("remove duplicate sidecar");
    fs::write(&gate_stderr_path, []).expect("restore stderr sidecar");

    let mut forged: serde_json::Value =
        serde_json::from_slice(&fs::read(&summary_path).expect("summary bytes"))
            .expect("summary JSON");
    forged["overall"]["correct_count"] = serde_json::json!(0);
    fs::write(
        &summary_path,
        serde_json::to_vec(&forged).expect("forged summary JSON"),
    )
    .expect("isolated tamper");
    assert_operator_rejects(
        &root,
        &gate_path,
        "summary-tamper",
        P8ArtifactContractFailure::SummaryMismatch,
    );

    fs::remove_dir_all(&root).expect("remove test-owned bundle");
    fs::remove_file(&gate_path).expect("remove test-owned sidecar");
    fs::remove_file(&gate_stdout_path).expect("remove test-owned stdout sidecar");
    fs::remove_file(&gate_stderr_path).expect("remove test-owned stderr sidecar");
}
