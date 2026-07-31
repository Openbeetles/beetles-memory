//! Independent P8 semantic artifact verifier.
//!
//! The operator reads each sealed artifact exactly once, validates raw bytes before JSON parsing,
//! and recomputes all aggregates without accepting producer-side in-memory authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Instant;

use bm_sdk::P8SemanticOffRunKey;
use serde::de::DeserializeOwned;

use crate::p8_artifact_dir::{
    observed_file_stability, P8ObservedFileStability, P8RetainedArtifactDirectory,
};
use crate::p8_semantic::{
    p8_semantic_detail_digest, physical_file_identity, P8AblationAggregate, P8AggregationSlice,
    P8ArtifactAdmissionKind, P8ArtifactAdmissionLedger, P8ArtifactContractFailure,
    P8ArtifactLimits, P8ClosedStderrRef, P8ClosedStdoutRef, P8GateCommandReceiptV1,
    P8MemoryUseDecision, P8SemanticBenchmarkSummaryV1, P8SemanticFailure, P8SemanticMetricCounts,
    P8SemanticMetricSlice, P8SemanticOperatorReportV1, P8SemanticProducerIdentityV1,
    P8SemanticQuestionDetailV1, P8SemanticRunPlanV1, P8SemanticShardManifestV1, P8Sha256Digest,
    P8VerificationReceiptV1, P8VerifierIdentityV1, P8_SEMANTIC_BENCHMARK_SUMMARY_SCHEMA,
};

struct OperatorReadLedger {
    admission: P8ArtifactAdmissionLedger,
    admitted_bytes: u64,
    bytes_read: u64,
    artifact_read_count: u64,
    physical_identities: BTreeSet<P8Sha256Digest>,
    retained_files: Vec<File>,
    retained_bindings: Vec<RetainedArtifactBinding>,
    retained_content_identities: Vec<P8ObservedFileStability>,
}

enum RetainedArtifactBinding {
    Bundle(String),
    Gate(String),
}

impl OperatorReadLedger {
    const fn new() -> Self {
        Self {
            admission: P8ArtifactAdmissionLedger::new(P8ArtifactLimits::V1),
            admitted_bytes: 0,
            bytes_read: 0,
            artifact_read_count: 0,
            physical_identities: BTreeSet::new(),
            retained_files: Vec::new(),
            retained_bindings: Vec::new(),
            retained_content_identities: Vec::new(),
        }
    }

    fn admit_before_read(
        &mut self,
        kind: P8ArtifactAdmissionKind,
        declared_bytes: u64,
        physical_identity: P8Sha256Digest,
    ) -> Result<(), P8ArtifactContractFailure> {
        self.admission.admit_declared(kind, declared_bytes)?;
        self.admission.admit_retained_handle()?;
        if !self.physical_identities.insert(physical_identity) {
            return Err(P8ArtifactContractFailure::DuplicateArtifact);
        }
        self.admitted_bytes = self
            .admitted_bytes
            .checked_add(declared_bytes)
            .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
        Ok(())
    }

    fn record_completed_read(
        &mut self,
        file: File,
        declared_bytes: u64,
        bytes_read: u64,
        binding: RetainedArtifactBinding,
        admitted_content_identity: P8ObservedFileStability,
    ) -> Result<(), P8ArtifactContractFailure> {
        if declared_bytes != bytes_read {
            return Err(P8ArtifactContractFailure::DetailBytesMismatch);
        }
        if observed_file_stability(&file)
            .map_err(|_| P8ArtifactContractFailure::ArtifactIoFailure)?
            != admitted_content_identity
        {
            return Err(P8ArtifactContractFailure::PhysicalIdentityMismatch);
        }
        self.admission.record_read_pass()?;
        self.bytes_read = self
            .bytes_read
            .checked_add(bytes_read)
            .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
        self.artifact_read_count = self
            .artifact_read_count
            .checked_add(1)
            .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
        self.retained_files.push(file);
        self.retained_bindings.push(binding);
        self.retained_content_identities
            .push(admitted_content_identity);
        Ok(())
    }

    fn verify_retained_bindings(
        &self,
        bundle: &P8RetainedArtifactDirectory,
        gate: &P8RetainedArtifactDirectory,
    ) -> Result<(), P8ArtifactContractFailure> {
        if self.retained_files.len() != self.retained_bindings.len()
            || self.retained_files.len() != self.retained_content_identities.len()
        {
            return Err(P8ArtifactContractFailure::ReadPassMismatch);
        }
        for ((file, binding), content_identity) in self
            .retained_files
            .iter()
            .zip(&self.retained_bindings)
            .zip(&self.retained_content_identities)
        {
            let result = match binding {
                RetainedArtifactBinding::Bundle(name) => bundle.verify_file_identity(name, file),
                RetainedArtifactBinding::Gate(name) => gate.verify_file_identity(name, file),
            };
            result.map_err(|_| P8ArtifactContractFailure::PhysicalIdentityMismatch)?;
            if observed_file_stability(file)
                .map_err(|_| P8ArtifactContractFailure::ArtifactIoFailure)?
                != *content_identity
            {
                return Err(P8ArtifactContractFailure::PhysicalIdentityMismatch);
            }
        }
        Ok(())
    }
}

pub fn run_p8_semantic_operator(
    root: &Path,
    gate_receipt_path: &Path,
    verifier_identity: P8VerifierIdentityV1,
) -> Result<P8SemanticOperatorReportV1, Vec<P8ArtifactContractFailure>> {
    let started = Instant::now();
    ensure_deadline(&started)?;
    let mut failures = verifier_identity.validate_contract();
    let mut ledger = OperatorReadLedger::new();
    let root = P8RetainedArtifactDirectory::open(root)
        .map_err(|_| vec![P8ArtifactContractFailure::PhysicalIdentityMismatch])?;
    let gate_parent_path = gate_receipt_path
        .parent()
        .ok_or_else(|| vec![P8ArtifactContractFailure::IdentityInvalid])?;
    let gate_parent = P8RetainedArtifactDirectory::open(gate_parent_path)
        .map_err(|_| vec![P8ArtifactContractFailure::PhysicalIdentityMismatch])?;
    let gate_receipt_name = artifact_component(gate_receipt_path)?;
    let gate_stdout_name = format!("{gate_receipt_name}.stdout");
    let gate_stderr_name = format!("{gate_receipt_name}.stderr");
    ledger
        .admission
        .admit_retained_handle()
        .map_err(|failure| vec![failure])?;
    ledger
        .admission
        .admit_retained_handle()
        .map_err(|failure| vec![failure])?;

    let gate_receipt_bytes = read_control_artifact_from_dir(
        &gate_parent,
        &gate_receipt_name,
        &mut ledger,
        &started,
        RetainedArtifactBinding::Gate(gate_receipt_name.clone()),
    )?;
    let gate_receipt: P8GateCommandReceiptV1 =
        parse_control_json(&gate_receipt_bytes, &mut ledger, &started)?;
    let trusted_source_root = crate::p8_semantic::p8_trusted_source_root()?;
    failures.extend(gate_receipt.validate_contract(&verifier_identity, &trusted_source_root));
    let closed_stdout = read_control_artifact_from_dir(
        &gate_parent,
        &gate_stdout_name,
        &mut ledger,
        &started,
        RetainedArtifactBinding::Gate(gate_stdout_name.clone()),
    )?;
    let closed_stderr = read_control_artifact_from_dir(
        &gate_parent,
        &gate_stderr_name,
        &mut ledger,
        &started,
        RetainedArtifactBinding::Gate(gate_stderr_name.clone()),
    )?;
    if u64::try_from(closed_stdout.len()).ok() != Some(gate_receipt.stdout_bytes)
        || P8ClosedStdoutRef::derive_bytes(&closed_stdout) != gate_receipt.stdout_digest
        || u64::try_from(closed_stderr.len()).ok() != Some(gate_receipt.stderr_bytes)
        || P8ClosedStderrRef::derive_bytes(&closed_stderr) != gate_receipt.stderr_digest
        || closed_stdout.as_slice() != crate::p8_semantic::P8_SEMANTIC_GATE_EXPECTED_STDOUT
    {
        failures.push(P8ArtifactContractFailure::ReceiptInvalid);
    }

    let producer_bytes = read_control_artifact_from_dir(
        &root,
        "producer-identity.json",
        &mut ledger,
        &started,
        RetainedArtifactBinding::Bundle("producer-identity.json".into()),
    )?;
    reject_forbidden_raw_material(&producer_bytes)?;
    let producer: P8SemanticProducerIdentityV1 =
        parse_control_json(&producer_bytes, &mut ledger, &started)?;
    failures.extend(producer.validate_contract());

    let run_plan_bytes = read_control_artifact_from_dir(
        &root,
        "run-plan.json",
        &mut ledger,
        &started,
        RetainedArtifactBinding::Bundle("run-plan.json".into()),
    )?;
    reject_forbidden_raw_material(&run_plan_bytes)?;
    let run_plan: P8SemanticRunPlanV1 = parse_control_json(&run_plan_bytes, &mut ledger, &started)?;
    failures.extend(run_plan.validate_contract());
    if producer.identity_digest() != &run_plan.producer_identity_digest {
        failures.push(P8ArtifactContractFailure::ProducerIdentityMismatch);
    }
    validate_exact_bundle_paths(&root, run_plan.shard_total)?;

    let supplied_summary_bytes = read_control_artifact_from_dir(
        &root,
        "summary.json",
        &mut ledger,
        &started,
        RetainedArtifactBinding::Bundle("summary.json".into()),
    )?;
    reject_forbidden_raw_material(&supplied_summary_bytes)?;
    let supplied_summary: P8SemanticBenchmarkSummaryV1 =
        parse_control_json(&supplied_summary_bytes, &mut ledger, &started)?;

    let mut manifests = Vec::new();
    let mut details_by_shard = Vec::new();
    for shard_index in 0..run_plan.shard_total {
        let manifest_name = format!("shard-{shard_index:05}.manifest.json");
        let manifest_bytes = read_control_artifact_from_dir(
            &root,
            &manifest_name,
            &mut ledger,
            &started,
            RetainedArtifactBinding::Bundle(manifest_name.clone()),
        )?;
        reject_forbidden_raw_material(&manifest_bytes)?;
        let manifest: P8SemanticShardManifestV1 =
            parse_control_json(&manifest_bytes, &mut ledger, &started)?;

        let detail_name = format!("shard-{shard_index:05}.details.jsonl");
        let (detail_bytes, detail_physical_identity) = read_detail_artifact_from_dir(
            &root,
            &detail_name,
            &mut ledger,
            &started,
            RetainedArtifactBinding::Bundle(detail_name.clone()),
        )?;
        reject_forbidden_raw_material(&detail_bytes)?;
        if P8Sha256Digest::derive_bytes("p8_shard_detail_artifact_v1", &detail_bytes)
            != manifest.detail_artifact.content_digest
            || detail_physical_identity != manifest.detail_artifact.physical_identity
        {
            failures.push(P8ArtifactContractFailure::PhysicalIdentityMismatch);
        }
        let details = parse_detail_lines(&detail_bytes, &mut ledger, &started)?;
        failures.extend(manifest.validate_with_details(&run_plan, &details));
        manifests.push(manifest);
        details_by_shard.push(details);
    }

    ensure_deadline(&started)?;
    let recomputed = match recompute_summary_from_admitted_bytes(
        &producer,
        &run_plan,
        &manifests,
        &details_by_shard,
    ) {
        Ok(summary) => summary,
        Err(mut recompute_failures) => {
            recompute_failures.extend(failures);
            recompute_failures.sort();
            recompute_failures.dedup();
            return Err(recompute_failures);
        }
    };
    ensure_deadline(&started)?;
    if supplied_summary != recomputed {
        failures.push(P8ArtifactContractFailure::SummaryMismatch);
    }
    if ledger.admitted_bytes != ledger.bytes_read {
        failures.push(P8ArtifactContractFailure::DetailBytesMismatch);
    }
    validate_exact_bundle_paths(&root, run_plan.shard_total)?;
    ledger
        .verify_retained_bindings(&root, &gate_parent)
        .map_err(|failure| vec![failure])?;
    root.verify_unchanged()
        .map_err(|_| vec![P8ArtifactContractFailure::PhysicalIdentityMismatch])?;
    gate_parent
        .verify_unchanged()
        .map_err(|_| vec![P8ArtifactContractFailure::PhysicalIdentityMismatch])?;
    failures.sort();
    failures.dedup();
    let verification_receipt = P8VerificationReceiptV1::build(
        &verifier_identity,
        &gate_receipt,
        ledger.admitted_bytes,
        ledger.bytes_read,
        ledger.artifact_read_count,
    );
    failures
        .extend(verification_receipt.validate_contract(&verifier_identity, Some(&gate_receipt)));
    let report = P8SemanticOperatorReportV1::from_independent_recomputation(
        run_plan.run_id,
        supplied_summary.summary_digest,
        recomputed.summary_digest,
        verifier_identity,
        verification_receipt,
        failures,
    );
    ensure_deadline(&started)?;
    Ok(report)
}

fn validate_exact_bundle_paths(
    root: &P8RetainedArtifactDirectory,
    shard_total: u32,
) -> Result<(), Vec<P8ArtifactContractFailure>> {
    let mut expected = BTreeSet::from([
        "producer-identity.json".to_string(),
        "run-plan.json".to_string(),
        "summary.json".to_string(),
    ]);
    for shard_index in 0..shard_total {
        expected.insert(format!("shard-{shard_index:05}.manifest.json"));
        expected.insert(format!("shard-{shard_index:05}.details.jsonl"));
    }
    let observed = root
        .exact_regular_file_names()
        .map_err(|_| vec![P8ArtifactContractFailure::PhysicalIdentityMismatch])?;
    if observed == expected {
        Ok(())
    } else {
        Err(vec![P8ArtifactContractFailure::ShardCoverageMismatch])
    }
}

fn artifact_component(path: &Path) -> Result<String, Vec<P8ArtifactContractFailure>> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| vec![P8ArtifactContractFailure::IdentityInvalid])
}

fn read_control_artifact_from_dir(
    root: &P8RetainedArtifactDirectory,
    file_name: &str,
    ledger: &mut OperatorReadLedger,
    started: &Instant,
    binding: RetainedArtifactBinding,
) -> Result<Vec<u8>, Vec<P8ArtifactContractFailure>> {
    ensure_deadline(started)?;
    let file = root
        .open_verified_file(file_name)
        .map_err(|_| vec![P8ArtifactContractFailure::PhysicalIdentityMismatch])?;
    read_control_artifact_file(file, ledger, started, binding)
}

fn read_control_artifact_file(
    mut file: File,
    ledger: &mut OperatorReadLedger,
    started: &Instant,
    binding: RetainedArtifactBinding,
) -> Result<Vec<u8>, Vec<P8ArtifactContractFailure>> {
    let metadata = file
        .metadata()
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    let content_identity = observed_file_stability(&file)
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    let physical_identity = physical_file_identity(&file, &metadata)?;
    ledger
        .admit_before_read(
            P8ArtifactAdmissionKind::ControlJson,
            metadata.len(),
            physical_identity,
        )
        .map_err(|failure| vec![failure])?;
    let bytes = read_artifact_bytes_once(
        &mut file,
        metadata.len(),
        P8ArtifactLimits::V1.control_json_bytes(),
    )
    .map_err(|failure| vec![failure])?;
    ledger
        .record_completed_read(
            file,
            metadata.len(),
            u64::try_from(bytes.len())
                .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?,
            binding,
            content_identity,
        )
        .map_err(|failure| vec![failure])?;
    ensure_deadline(started)?;
    Ok(bytes)
}

fn read_detail_artifact_from_dir(
    root: &P8RetainedArtifactDirectory,
    file_name: &str,
    ledger: &mut OperatorReadLedger,
    started: &Instant,
    binding: RetainedArtifactBinding,
) -> Result<(Vec<u8>, P8Sha256Digest), Vec<P8ArtifactContractFailure>> {
    ensure_deadline(started)?;
    let mut file = root
        .open_verified_file(file_name)
        .map_err(|_| vec![P8ArtifactContractFailure::PhysicalIdentityMismatch])?;
    let metadata = file
        .metadata()
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    let content_identity = observed_file_stability(&file)
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    let physical_identity = physical_file_identity(&file, &metadata)?;
    ledger
        .admit_before_read(
            P8ArtifactAdmissionKind::ShardDetails,
            metadata.len(),
            physical_identity.clone(),
        )
        .map_err(|failure| vec![failure])?;
    let bytes = read_detail_artifact_bytes_once(
        &mut file,
        metadata.len(),
        P8ArtifactLimits::V1.shard_detail_bytes(),
        P8ArtifactLimits::V1.detail_line_bytes(),
    )
    .map_err(|failure| vec![failure])?;
    ledger
        .record_completed_read(
            file,
            metadata.len(),
            u64::try_from(bytes.len())
                .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?,
            binding,
            content_identity,
        )
        .map_err(|failure| vec![failure])?;
    ensure_deadline(started)?;
    Ok((bytes, physical_identity))
}

fn read_artifact_bytes_once<R: Read>(
    mut reader: R,
    declared_bytes: u64,
    limit: u64,
) -> Result<Vec<u8>, P8ArtifactContractFailure> {
    if declared_bytes > limit {
        return Err(P8ArtifactContractFailure::ArtifactLimitExceeded);
    }
    let bounded_capacity = declared_bytes.min(1024 * 1024);
    let capacity = usize::try_from(bounded_capacity)
        .map_err(|_| P8ArtifactContractFailure::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    reader
        .by_ref()
        .take(
            declared_bytes
                .checked_add(1)
                .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?,
        )
        .read_to_end(&mut bytes)
        .map_err(|_| P8ArtifactContractFailure::ReadPassMismatch)?;
    if u64::try_from(bytes.len()).map_err(|_| P8ArtifactContractFailure::ArithmeticOverflow)?
        != declared_bytes
    {
        return Err(P8ArtifactContractFailure::DetailBytesMismatch);
    }
    Ok(bytes)
}

fn read_detail_artifact_bytes_once<R: Read>(
    mut reader: R,
    declared_bytes: u64,
    artifact_limit: u64,
    line_limit: u64,
) -> Result<Vec<u8>, P8ArtifactContractFailure> {
    if declared_bytes > artifact_limit || line_limit == 0 {
        return Err(P8ArtifactContractFailure::ArtifactLimitExceeded);
    }
    let capacity = usize::try_from(declared_bytes.min(1024 * 1024))
        .map_err(|_| P8ArtifactContractFailure::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut chunk = [0_u8; 64 * 1024];
    let mut observed = 0_u64;
    let mut line_bytes = 0_u64;
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|_| P8ArtifactContractFailure::ReadPassMismatch)?;
        if read == 0 {
            break;
        }
        observed = observed
            .checked_add(
                u64::try_from(read).map_err(|_| P8ArtifactContractFailure::ArithmeticOverflow)?,
            )
            .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
        if observed > declared_bytes {
            return Err(P8ArtifactContractFailure::DetailBytesMismatch);
        }
        for byte in &chunk[..read] {
            if *byte == b'\n' {
                if line_bytes
                    .checked_add(1)
                    .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?
                    > line_limit
                {
                    return Err(P8ArtifactContractFailure::ArtifactLimitExceeded);
                }
                line_bytes = 0;
            } else {
                line_bytes = line_bytes
                    .checked_add(1)
                    .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
                if line_bytes >= line_limit {
                    return Err(P8ArtifactContractFailure::ArtifactLimitExceeded);
                }
            }
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    if observed != declared_bytes {
        return Err(P8ArtifactContractFailure::DetailBytesMismatch);
    }
    Ok(bytes)
}

fn parse_control_json<T: DeserializeOwned>(
    bytes: &[u8],
    ledger: &mut OperatorReadLedger,
    started: &Instant,
) -> Result<T, Vec<P8ArtifactContractFailure>> {
    ensure_deadline(started)?;
    let value = serde_json::from_slice(bytes)
        .map_err(|_| vec![P8ArtifactContractFailure::SchemaMismatch])?;
    ledger
        .admission
        .record_parsed_document()
        .map_err(|failure| vec![failure])?;
    ensure_deadline(started)?;
    Ok(value)
}

fn parse_detail_lines(
    bytes: &[u8],
    ledger: &mut OperatorReadLedger,
    started: &Instant,
) -> Result<Vec<P8SemanticQuestionDetailV1>, Vec<P8ArtifactContractFailure>> {
    ensure_deadline(started)?;
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err(vec![P8ArtifactContractFailure::DetailRowsMismatch]);
    }
    let mut details = Vec::new();
    for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
        let line_bytes = u64::try_from(line.len())
            .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
        if line.is_empty()
            || line_bytes
                .checked_add(1)
                .is_none_or(|row_bytes| row_bytes > P8ArtifactLimits::V1.detail_line_bytes())
        {
            return Err(vec![P8ArtifactContractFailure::ArtifactLimitExceeded]);
        }
        details.push(
            serde_json::from_slice(line)
                .map_err(|_| vec![P8ArtifactContractFailure::SchemaMismatch])?,
        );
        ledger
            .admission
            .record_parsed_document()
            .map_err(|failure| vec![failure])?;
        ensure_deadline(started)?;
    }
    Ok(details)
}

fn ensure_deadline(started: &Instant) -> Result<(), Vec<P8ArtifactContractFailure>> {
    if operator_deadline_exceeded(started.elapsed().as_millis()) {
        Err(vec![P8ArtifactContractFailure::OperatorWallTimeExceeded])
    } else {
        Ok(())
    }
}

const fn operator_deadline_exceeded(elapsed_millis: u128) -> bool {
    elapsed_millis > P8ArtifactLimits::V1.operator_wall_millis() as u128
}

fn reject_forbidden_raw_material(bytes: &[u8]) -> Result<(), Vec<P8ArtifactContractFailure>> {
    const FORBIDDEN: [&[u8]; 7] = [
        b"private-owner-sentinel",
        b"private-space-sentinel",
        b"private-subject-sentinel",
        b"raw-procedure-sentinel",
        b"raw-soul-sentinel",
        b"credential-sentinel",
        b"path-sentinel",
    ];
    if FORBIDDEN
        .iter()
        .any(|needle| bytes.windows(needle.len()).any(|window| window == *needle))
    {
        Err(vec![P8ArtifactContractFailure::SdkReportInvalid])
    } else {
        Ok(())
    }
}

pub(crate) fn run_p8_gate_contract() -> Result<(), String> {
    use bm_core::memory::{board_subject_scope_id, SelfAuthoredCore};
    use bm_core::platform::Platform as _;
    use bm_sdk::{
        default_agent_subject_id, GovernedRuntimeSkillWriteInput, MemoryIdentity,
        MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallTemporalOperation, MemoryRuntime,
        MemoryScope, MemoryStoreHandle, MemoryWriteRequest, PressureLevel, ProfileId,
        RuntimeLifecycleModeInput, RuntimeSkillCreationRef, RuntimeSkillOwningScope,
        RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
    };

    fn build_runtime(
        platform: MemoryStoreHandle,
        owner_id: &str,
        actor_subject_id: Option<&str>,
    ) -> Result<MemoryRuntime, String> {
        let mut builder = MemoryRuntime::builder()
            .identity(
                MemoryIdentity::new("agent-main", owner_id).map_err(|error| error.to_string())?,
            )
            .scope(MemoryScope::new("p8.gate", "chat-a").map_err(|error| error.to_string())?)
            .store(platform);
        if let Some(subject_id) = actor_subject_id {
            builder = builder.subject_id(subject_id);
        }
        builder.build().map_err(|error| error.to_string())
    }

    fn skill_write(
        name: &str,
        content: &str,
        privacy_class: MemoryPrivacyClass,
    ) -> GovernedRuntimeSkillWriteInput {
        GovernedRuntimeSkillWriteInput {
            write: RuntimeSkillWrite {
                name: name.into(),
                topic: "P8 gate privacy".into(),
                title: "P8 gate privacy contract".into(),
                summary: "Verify that governed private procedures never cross safe surfaces."
                    .into(),
                content: content.into(),
                citations: vec!["p8 gate contract".into()],
                source_chat_id: Some("chat-a".into()),
                observed_at: 1_780_000_000,
            },
            creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                candidate_ref: format!("p8-gate:{name}"),
                verification_receipt_digest:
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
            },
            privacy_class,
        }
    }

    fn project(runtime: &MemoryRuntime) -> Result<bm_sdk::MemoryProjectionOutput, String> {
        runtime
            .project(MemoryProjectionRequest {
                temporal_operation: MemoryRecallTemporalOperation::Current,
                user_query: "P8 gate privacy procedure".into(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .map_err(|error| error.to_string())
    }

    fn assert_safe_surfaces(
        output: &bm_sdk::MemoryProjectionOutput,
        forbidden: &[&str],
    ) -> Result<(), String> {
        let report = output.report();
        let safe_surfaces = [
            report.ui_api_projection(),
            report.operator_projection(),
            report.gateway_audit().block.as_str(),
            report.shared_fact_projection(),
        ];
        let safe_json = serde_json::to_string(&(
            report.governed_public_report(),
            report.governed_operator_report(),
        ))
        .map_err(|_| "P8 gate safe report serialization failed".to_string())?;
        if forbidden.iter().any(|needle| {
            safe_surfaces.iter().any(|surface| surface.contains(needle))
                || safe_json.contains(needle)
                || output
                    .provider_payload()
                    .system_memory_block()
                    .contains(needle)
        }) {
            return Err("P8 gate observed private/cross-scope procedure leakage".into());
        }
        let operator = report.governed_operator_report().payload();
        if operator.session_open_count() != 1
            || operator.receipt_count() != 1
            || !operator.manifest_verified()
            || !operator.read_set_exact()
            || !report
                .governed_public_report()
                .validate_contract()
                .is_empty()
            || !report
                .governed_operator_report()
                .validate_contract()
                .is_empty()
        {
            return Err("P8 gate observed an invalid immutable safe-report receipt".into());
        }
        Ok(())
    }

    let profile = ProfileId::native_dev_full()
        .ok_or_else(|| "native dev-full profile is absent".to_string())?;
    let platform = MemoryStoreHandle::open_for_nonproduction_harness(
        StoreBackendConfig::in_memory(profile).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let runtime = build_runtime(platform.clone(), "owner-default", None)?;
    let private_procedure = "p8-private-runtime-skill-procedure-sentinel";
    let soul_procedure = "p8-soul-runtime-skill-procedure-sentinel";
    let cross_scope_procedure = "p8-cross-scope-runtime-skill-procedure-sentinel";
    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![
                skill_write(
                    "runtime_skill__p8_gate_private",
                    private_procedure,
                    MemoryPrivacyClass::PrivateGarden,
                ),
                skill_write(
                    "runtime_skill__p8_gate_soul",
                    soul_procedure,
                    MemoryPrivacyClass::SoulPrivate,
                ),
                skill_write(
                    "runtime_skill__p8_gate_shared",
                    cross_scope_procedure,
                    MemoryPrivacyClass::SharedWithSubject,
                ),
            ],
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: default_agent_subject_id("agent-main"),
            },
            source: RuntimeSkillWriteSource::Manual,
        })
        .map_err(|error| error.to_string())?;

    platform
        .replay_harness()
        .self_authored_core_store()
        .set(
            board_subject_scope_id(),
            &SelfAuthoredCore {
                identity_anchor: "p8 gate stable soul".into(),
                default_response_mode: "direct".into(),
                self_preservation_doctrine: "never expose private procedure".into(),
                ..SelfAuthoredCore::default()
            },
        )
        .map_err(|error| error.to_string())?;
    platform
        .replay_harness()
        .private_garden_store()
        .write(
            "chat-a",
            "p8-gate-private.md",
            "p8-gate-private-store-sentinel",
            1_780_000_000,
        )
        .map_err(|error| error.to_string())?;
    let before_core = platform
        .replay_harness()
        .self_authored_core_store()
        .get(board_subject_scope_id())
        .map_err(|error| error.to_string())?;
    let before_private = platform
        .replay_harness()
        .private_garden_store()
        .list("chat-a", 16)
        .map_err(|error| error.to_string())?;
    let before_ledger = platform
        .replay_harness()
        .core_revision_ledger_store()
        .get(board_subject_scope_id())
        .map_err(|error| error.to_string())?;

    assert_safe_surfaces(&project(&runtime)?, &[private_procedure, soul_procedure])?;
    let cross_subject = build_runtime(platform.clone(), "owner-default", Some("p8-other-subject"))?;
    assert_safe_surfaces(&project(&cross_subject)?, &[cross_scope_procedure])?;
    let cross_space = build_runtime(platform.clone(), "owner-other", None)?;
    assert_safe_surfaces(&project(&cross_space)?, &[cross_scope_procedure])?;

    let after_core = platform
        .replay_harness()
        .self_authored_core_store()
        .get(board_subject_scope_id())
        .map_err(|error| error.to_string())?;
    let after_private = platform
        .replay_harness()
        .private_garden_store()
        .list("chat-a", 16)
        .map_err(|error| error.to_string())?;
    let after_ledger = platform
        .replay_harness()
        .core_revision_ledger_store()
        .get(board_subject_scope_id())
        .map_err(|error| error.to_string())?;
    if before_core != after_core || before_private != after_private || before_ledger != after_ledger
    {
        return Err("P8 gate observed Soul/private projection mutation".into());
    }

    for sentinel in [
        b"private-owner-sentinel".as_slice(),
        b"raw-procedure-sentinel".as_slice(),
        b"raw-soul-sentinel".as_slice(),
    ] {
        if reject_forbidden_raw_material(sentinel).is_ok() {
            return Err("P8 gate raw material rejection contract did not execute".into());
        }
    }
    Ok(())
}

fn recompute_summary_from_admitted_bytes(
    producer: &P8SemanticProducerIdentityV1,
    run_plan: &P8SemanticRunPlanV1,
    manifests: &[P8SemanticShardManifestV1],
    details_by_shard: &[Vec<P8SemanticQuestionDetailV1>],
) -> Result<P8SemanticBenchmarkSummaryV1, Vec<P8ArtifactContractFailure>> {
    let mut failures = Vec::new();
    if producer.identity_digest() != &run_plan.producer_identity_digest
        || manifests.len() != usize::try_from(run_plan.shard_total).unwrap_or(usize::MAX)
        || details_by_shard.len() != manifests.len()
    {
        failures.push(P8ArtifactContractFailure::ShardCoverageMismatch);
    }
    let mut question_ids = BTreeSet::new();
    let mut ordered_details = Vec::new();
    for (index, (manifest, details)) in manifests.iter().zip(details_by_shard).enumerate() {
        if usize::try_from(manifest.shard_index).ok() != Some(index) {
            failures.push(P8ArtifactContractFailure::ShardIndexMismatch);
        }
        failures.extend(manifest.validate_with_details(run_plan, details));
        for detail in details {
            if !question_ids.insert(detail.question_id.clone()) {
                failures.push(P8ArtifactContractFailure::DuplicateQuestion);
            }
            ordered_details.push(detail);
        }
    }
    let planned = run_plan
        .ordered_questions
        .iter()
        .map(|question| &question.question_id)
        .collect::<BTreeSet<_>>();
    if question_ids.iter().collect::<BTreeSet<_>>() != planned {
        failures.push(P8ArtifactContractFailure::QuestionCoverageMismatch);
    }
    failures.sort();
    failures.dedup();
    if !failures.is_empty() {
        return Err(failures);
    }

    let mut overall = P8SemanticMetricCounts::default();
    let mut slices = BTreeMap::<P8AggregationSlice, P8SemanticMetricCounts>::new();
    let mut ablation_deltas = P8SemanticOffRunKey::ALL
        .into_iter()
        .map(|key| (key, P8AblationAggregate::default()))
        .collect::<BTreeMap<_, _>>();
    for detail in &ordered_details {
        observe(&mut overall, detail)?;
        let mut memberships = BTreeSet::from([
            P8AggregationSlice::BenchmarkFamily(detail.benchmark_family),
            P8AggregationSlice::DatasetStratum(detail.dataset_stratum.clone()),
            P8AggregationSlice::Task(detail.task_kind),
            P8AggregationSlice::QueryOperation(detail.query_operation_kind),
            P8AggregationSlice::TemporalCorpus(detail.temporal_corpus_slice),
            P8AggregationSlice::Profile(detail.profile),
        ]);
        memberships.extend(
            detail
                .capability_slices
                .iter()
                .copied()
                .map(P8AggregationSlice::Capability),
        );
        memberships.extend(
            detail
                .safety_slices
                .iter()
                .copied()
                .map(P8AggregationSlice::Safety),
        );
        for membership in memberships {
            observe(slices.entry(membership).or_default(), detail)?;
        }
        for observation in detail.sdk_off_run_report.observations() {
            let aggregate = ablation_deltas
                .get_mut(&observation.key())
                .expect("operator exact eight-key aggregate exists");
            checked_add(&mut aggregate.applicable_count, observation.applicable())?;
            checked_add(&mut aggregate.executed_count, observation.executed())?;
            let evaluation = detail
                .ablation_evaluations
                .get(&observation.key())
                .expect("validated detail has exact eight evaluations");
            checked_add(
                &mut aggregate.baseline_correct_count,
                evaluation.baseline_decision.is_correct() && observation.applicable(),
            )?;
            checked_add(
                &mut aggregate.off_run_correct_count,
                evaluation.off_run_decision.is_correct() && observation.executed(),
            )?;
        }
    }
    let mut summary = P8SemanticBenchmarkSummaryV1 {
        schema: P8_SEMANTIC_BENCHMARK_SUMMARY_SCHEMA.into(),
        producer_identity_digest: run_plan.producer_identity_digest.clone(),
        run_plan_digest: run_plan.run_plan_digest.clone(),
        run_id: run_plan.run_id.clone(),
        admitted_shard_manifest_digests: manifests
            .iter()
            .map(P8SemanticShardManifestV1::manifest_digest)
            .collect(),
        ordered_detail_digests: ordered_details
            .iter()
            .map(|detail| p8_semantic_detail_digest(detail))
            .collect(),
        overall,
        slices: slices
            .into_iter()
            .map(|(slice, metrics)| P8SemanticMetricSlice { slice, metrics })
            .collect(),
        ablation_deltas,
        summary_digest: P8Sha256Digest::derive_bytes(
            "p8_semantic_benchmark_summary_v1",
            b"uninitialized",
        ),
    };
    summary.summary_digest = summary.derived_digest();
    Ok(summary)
}

fn observe(
    metrics: &mut P8SemanticMetricCounts,
    detail: &P8SemanticQuestionDetailV1,
) -> Result<(), Vec<P8ArtifactContractFailure>> {
    checked_add_u64(&mut metrics.question_count, 1)?;
    checked_add_u64(
        &mut metrics.correct_count,
        u64::from(detail.accuracy_decision.is_correct()),
    )?;
    match detail.memory_use_decision {
        P8MemoryUseDecision::CurrentUsed => checked_add_u64(&mut metrics.current_use_count, 1)?,
        P8MemoryUseDecision::ObsoleteRejected => {
            checked_add_u64(&mut metrics.obsolete_rejected_count, 1)?
        }
        P8MemoryUseDecision::ObsoleteUsed => checked_add_u64(&mut metrics.obsolete_used_count, 1)?,
        P8MemoryUseDecision::InvalidatedRejected => {
            checked_add_u64(&mut metrics.invalidated_rejected_count, 1)?
        }
        P8MemoryUseDecision::InvalidatedUsed => {
            checked_add_u64(&mut metrics.invalidated_used_count, 1)?
        }
        P8MemoryUseDecision::NotApplicable => {}
    }
    let safety_count = u64::try_from(
        detail
            .failures
            .iter()
            .filter(|failure| **failure == P8SemanticFailure::SafetyViolation)
            .count(),
    )
    .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
    checked_add_u64(&mut metrics.safety_failure_count, safety_count)?;
    checked_add_u64(&mut metrics.elapsed_millis, detail.resource.elapsed_millis)?;
    metrics.peak_rss_bytes = metrics.peak_rss_bytes.max(detail.resource.peak_rss_bytes);
    Ok(())
}

fn checked_add(target: &mut u64, condition: bool) -> Result<(), Vec<P8ArtifactContractFailure>> {
    checked_add_u64(target, u64::from(condition))
}

fn checked_add_u64(target: &mut u64, amount: u64) -> Result<(), Vec<P8ArtifactContractFailure>> {
    *target = target
        .checked_add(amount)
        .ok_or_else(|| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{operator_deadline_exceeded, read_detail_artifact_bytes_once};
    use crate::p8_artifact_dir::observed_file_stability;
    use crate::p8_semantic::P8ArtifactLimits;
    use std::fs::{self, OpenOptions};
    use std::io::{Cursor, Write};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn p8_operator_deadline_accepts_exact_and_rejects_n_plus_one() {
        let exact = P8ArtifactLimits::V1.operator_wall_millis() as u128;
        assert!(!operator_deadline_exceeded(exact));
        assert!(operator_deadline_exceeded(exact + 1));
    }

    #[test]
    fn p8_detail_reader_rejects_line_n_plus_one_while_streaming() {
        assert_eq!(
            read_detail_artifact_bytes_once(Cursor::new(b"123\n"), 4, 8, 4).expect("exact line"),
            b"123\n"
        );
        assert_eq!(
            read_detail_artifact_bytes_once(Cursor::new(b"1234\n"), 5, 8, 4),
            Err(crate::p8_semantic::P8ArtifactContractFailure::ArtifactLimitExceeded)
        );
    }

    #[test]
    fn p8_retained_content_identity_rejects_same_inode_rewrite() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bm-p8-retained-content-{}-{nonce}",
            std::process::id()
        ));
        fs::write(&path, b"admitted").expect("write admitted bytes");
        let retained = fs::File::open(&path).expect("retain artifact");
        let admitted = observed_file_stability(&retained).expect("admitted content identity");
        let mut writer = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .expect("open same inode for rewrite");
        writer
            .write_all(b"rewritte")
            .and_then(|()| writer.sync_all())
            .expect("same-size rewrite");
        let rewritten = observed_file_stability(&retained).expect("rewritten content identity");
        assert_ne!(rewritten, admitted);
        fs::remove_file(path).expect("remove test artifact");
    }
}
