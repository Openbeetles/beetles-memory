use bm_core::memory::{
    run_persona_governance_replay_suite, run_recall_benchmark_suite, PersonaGovernanceReplayCase,
    RecallBenchmarkCase,
};
use bm_core::{Error, Result};
use bm_sdk::{
    ProfileId, MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION,
    MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata},
    io::{BufRead, BufReader, Read},
    path::{Component, Path, PathBuf},
    process::Command,
    rc::Rc,
    time::{Duration, SystemTime},
};

#[cfg(test)]
use crate::build_support::{P7_OPERATOR_BUILD_FINGERPRINT_CONTRACT, P7_OPERATOR_BUILD_INPUTS};
use crate::build_support::{P7_SDK_BUILD_FINGERPRINT_CONTRACT, P7_SDK_BUILD_INPUTS};
use crate::p7_process::{
    run_p7_bounded_command, run_p7_bounded_retained_executable, P7ProcessLimits,
    P7ProcessTermination,
};
use crate::p7_secure_fs::P7RetainedDirectoryOwner;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

const P7_CONTRACT_VERSION: &str = "p7_recall_delivery_v2";
const P7_PRODUCER_IDENTITY_SCHEMA_VERSION: &str = "p7_producer_identity_v2";
const P7_RECORDED_PRODUCER_IDENTITY_SCHEMA_VERSION: &str = "p7_recorded_producer_identity_v1";
const P7_VERIFIER_IDENTITY_SCHEMA_VERSION: &str = "p7_verifier_identity_v2";
pub const P7_VERIFIER_RELEASE_MANIFEST_SCHEMA_VERSION: &str = "p7_verifier_release_manifest_v1";
pub const P7_VERIFIER_RELEASE_MANIFEST_FILE_NAME: &str = "verifier-release-manifest.json";
pub const P7_VERIFIER_RELEASES_DIR: &str = "verifier/releases";
const P7_VERIFICATION_RECEIPT_SCHEMA_VERSION: &str = "p7_verification_receipt_v2";
const P7_VERIFICATION_POLICY_CONTRACT: &str = "p7_verification_policy_v2";
pub const P7_DETAIL_SCHEMA_VERSION: &str = "p7_question_detail_v1";
const P7_MAX_DETAIL_LINE_BYTES: usize = 16 * 1024 * 1024;
const P7_MAX_DATASET_OBJECT_BYTES: usize = 16 * 1024 * 1024;
const P7_MAX_DETAIL_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const P7_MAX_ALL_DETAIL_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const P7_QUESTION_EVALUATION_SCHEMA_VERSION: &str = "p7_question_evaluation_v1";
const P7_SOUL_REGRESSION_GATE_SCHEMA_VERSION: &str = "p7_soul_regression_gate_v1";
const P7_VERIFIER_PERFORMANCE_SCHEMA_VERSION: &str = "p7_verifier_performance_v2";
const P7_ARTIFACT_LIFECYCLE_RECEIPT_SCHEMA_VERSION: &str = "p7_artifact_read_lifecycle_receipt_v1";
const P7_VERIFIER_MAX_WALL_TIME: Duration = Duration::from_secs(30 * 60);
const P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const P7_MAX_CONTROL_JSON_BYTES: u64 = 64 * 1024 * 1024;
const P7_MAX_RSS_STDOUT_BYTES: u64 = 64 * 1024 * 1024;
const P7_MAX_RSS_STDERR_BYTES: u64 = 4 * 1024 * 1024;
const P7_MAX_RETAINED_ARTIFACT_HANDLES: usize = 4096;
const P7_PROCESS_STDOUT_CAP_BYTES: u64 = 64 * 1024 * 1024;
const P7_PROCESS_STDERR_CAP_BYTES: u64 = 64 * 1024 * 1024;
const P7_PROCESS_TOTAL_CAP_BYTES: u64 = 96 * 1024 * 1024;
const P7_MAX_RELEASE_SOURCE_FILES: usize = 4096;
const P7_RUNNER_PROJECTION_DIGEST_OBSERVATION_SCHEMA_VERSION: &str =
    "p7_runner_projection_digest_observation_v2";
const P7_PROJECTION_OWNER_IDENTITY_TOKEN_PREFIX: &str = "projection-owner-token:";
const P7_TRUSTED_SDK_BUILD_FINGERPRINT: &str = env!("BM_P7_TRUSTED_SDK_BUILD_FINGERPRINT");
const P7_EMBEDDED_OPERATOR_BUILD_FINGERPRINT: &str = env!("BM_P7_OPERATOR_BUILD_FINGERPRINT");
const P7_BUILD_SOURCE_ATTESTATION: &str = env!("BM_P7_BUILD_SOURCE_ATTESTATION");
const P7_WORKSPACE_BUILD_SOURCE_ATTESTATION: &str = "workspace_source";
const P7_FROZEN_ANCHOR_SHA256: &str = env!("BM_P7_FROZEN_ANCHOR_SHA256");
const P7_FROZEN_ANCHOR_GENERATOR_RECEIPT_SHA256: &str =
    env!("BM_P7_FROZEN_ANCHOR_GENERATOR_RECEIPT_SHA256");
const P7_OPERATOR_BUILD_PROFILE: &str = env!("BM_P7_OPERATOR_BUILD_PROFILE");
const P7_OPERATOR_BUILD_FEATURES: &str = env!("BM_P7_OPERATOR_BUILD_FEATURES");
const P7_RUNNER_RELEASES_DIR: &str = "releases";
const P7_RUNNER_RELEASE_FILE_NAME: &str = "beetle-memory-external-bench-runner";
pub const P7_RELEASE_GATE_ATTESTATION_FILE_NAME: &str = "release-gate-attestation.json";
pub const P7_RELEASE_GATE_SOURCE_MANIFEST_FILE_NAME: &str = "release-gate-source-manifest.json";
pub const P7_RELEASE_METADATA_FILE_NAME: &str = "release-metadata.json";
pub const P7_RELEASE_GATE_ATTESTATION_SCHEMA_VERSION: &str = "p7_release_gate_attestation_v2";
pub const P7_RELEASE_GATE_SOURCE_MANIFEST_SCHEMA_VERSION: &str =
    "p7_release_gate_source_manifest_v2";
pub const P7_RELEASE_GATE_ORCHESTRATOR_CONTRACT: &str =
    "p7_content_addressed_release_gate_orchestrator_v3";
pub const P7_RELEASE_GATE_PLAN_SCHEMA_VERSION: &str = "p7_release_gate_plan_v2";
pub const P7_RELEASE_METADATA_SCHEMA_VERSION: &str = "p7_content_addressed_release_metadata_v2";
pub const P7_RELEASE_GATE_SOURCE_FINGERPRINT_CONTRACT: &str = "p7_release_gate_source_sha256_v3";
pub const P7_PRODUCER_SEMANTIC_SOURCE_MANIFEST_SCHEMA_VERSION: &str =
    "p7_producer_semantic_source_manifest_v1";
pub const P7_PRODUCER_SEMANTIC_SOURCE_FINGERPRINT_CONTRACT: &str =
    "p7_producer_semantic_source_sha256_v1";
pub const P7_FROZEN_RUNNER_IDENTITY_RELATIVE_PATH: &str =
    "crates/replay/src/bin/bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs";
pub const P7_RUNNER_PREFLIGHT_SCHEMA_VERSION: &str = "p7_runner_preflight_v3";
pub const P7_COHORT_ADMISSION_SCHEMA_VERSION: &str = "p7_cohort_admission_v1";
pub const P7_COHORT_ADMISSION_FILE_NAME: &str = "cohort-admission.json";
pub const P7_SHARD_PRODUCER_PROVENANCE_SCHEMA_VERSION: &str = "p7_shard_producer_provenance_v2";
pub const P7_MERGED_PROVENANCE_SCHEMA_VERSION: &str = "p7_merged_provenance_v2";
pub const P7_REQUIRED_RELEASE_GATE_IDS: [&str; 14] = [
    "agent-memory-fmt",
    "agent-memory-check",
    "agent-memory-clippy",
    "agent-memory-test",
    "agent-memory-write-transaction-contract",
    "agent-memory-next-gen-plan-contract",
    "agent-memory-cross-target-compile-contract",
    "agent-memory-linux-execution-authority",
    "runner-fmt",
    "runner-test",
    "runner-clippy",
    "runner-shell-syntax",
    "runner-full-wall-fake",
    "runner-max-rss-fake",
];
const P7_PRODUCER_SEMANTIC_AGENT_MEMORY_INPUTS: [&str; 14] = [
    "Cargo.toml",
    "Cargo.lock",
    "crates/core/Cargo.toml",
    "crates/core/src",
    "crates/sdk/Cargo.toml",
    "crates/sdk/src",
    "crates/replay/Cargo.toml",
    "crates/replay/build.rs",
    "crates/replay/src/lib.rs",
    "crates/replay/src/bench.rs",
    "crates/replay/src/p7_process.rs",
    "crates/replay/src/p7_secure_fs.rs",
    "crates/replay/src/retained_artifact_fs.rs",
    "crates/replay/src/sealed_execution.rs",
];
const P7_PRODUCER_SEMANTIC_RUNNER_INPUTS: [&str; 5] = [
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    "src",
    "run_full_p7_wall.sh",
];
const P7_AGENT_MEMORY_RELEASE_GATE_SOURCE_INPUTS: [&str; 1] = ["."];
const P7_RUNNER_RELEASE_GATE_SOURCE_INPUTS: [&str; 1] = ["."];
const P7_AGENT_MEMORY_RELEASE_GATE_EXCLUDED_DIRECTORIES: [&str; 3] =
    ["memory", "results", "crates/sdk/memory"];
const P7_RUNNER_RELEASE_GATE_EXCLUDED_DIRECTORIES: [&str; 1] = ["releases"];
const P7_RELEASE_GATE_EXCLUDED_DIRECTORY_NAMES: [&str; 10] = [
    ".git",
    ".agents",
    ".codex",
    ".DS_Store",
    "target",
    "node_modules",
    "dist",
    "cache",
    ".cache",
    "logs",
];
const P7_RUNNER_BUILD_FINGERPRINT_CONTRACT: &str = "p7_runner_build_inputs_sha256_v2";
const P7_RUNNER_BUILD_INPUTS: [&str; 4] = ["Cargo.toml", "Cargo.lock", "build.rs", "src"];
const P7_FINGERPRINT_READ_BUFFER_BYTES: usize = 64 * 1024;
const P7_MAXIMUM_RSS_EVIDENCE_SCHEMA_VERSION: &str = "p7_maximum_rss_evidence_v3";
pub const P7_MAXIMUM_RSS_MEASUREMENT_SCHEMA_VERSION: &str = "p7_maximum_rss_measurement_v3";
pub const P7_MAXIMUM_RSS_MEASUREMENT_FILE_NAME: &str = "maximum-rss-measurement.json";
pub const P7_MAXIMUM_RSS_REPORT_FILE_NAME: &str = "maximum-rss-report.json";
const P7_MAXIMUM_RSS_SUITE: &str = "longmemeval_m_cleaned";
const P7_MAXIMUM_RSS_DATASET_INDEX: usize = 0;
const P7_MAXIMUM_RSS_QUESTION_INDEX: usize = 0;
const P7_MAXIMUM_RSS_LIMIT_BYTES: u64 = 6 * 1024 * 1024 * 1024;
const P7_MAXIMUM_RSS_ARTIFACT_STEM: &str =
    "longmemeval_m_cleaned.shard-0-of-1.limit-1.question-index-0";
const P7_ABLATION_METHOD: &str = "sdk_eval_recall_off_run_v1";
const P7_REQUIRED_ABLATION_SLICES: [&str; 7] = [
    "facet_off",
    "rank_fusion_off",
    "coverage_selection_off",
    "delivery_relevance_fusion_off",
    "evidence_family_rotation_off",
    "render_capsule_off",
    "capsule_dedupe_off",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P7FrozenRunnerIdentity {
    pub runner_build_fingerprint: &'static str,
    pub runner_lock_fingerprint: &'static str,
    pub executable_sha256: &'static str,
    pub gate_attestation_sha256: &'static str,
    pub release_metadata_sha256: &'static str,
    pub gate_source_fingerprint: &'static str,
    pub gate_source_manifest_sha256: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7RunnerDiskIdentity {
    runner_build_fingerprint: String,
    runner_lock_fingerprint: String,
    executable_sha256: String,
    executable_canonical_path: PathBuf,
    gate_attestation_sha256: String,
    release_metadata_sha256: String,
    gate_source_fingerprint: String,
    gate_source_manifest_sha256: String,
    gate_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7RunnerPreflightReport {
    pub schema_version: String,
    pub run_id: String,
    pub sdk_build_fingerprint: String,
    pub runner_build_fingerprint: String,
    pub runner_lock_fingerprint: String,
    pub executable_sha256: String,
    pub executable_canonical_path: String,
    pub gate_attestation_sha256: String,
    pub release_metadata_sha256: String,
    pub gate_source_fingerprint: String,
    pub gate_source_manifest_sha256: String,
    pub gate_ids: Vec<String>,
    pub build_profile: String,
}

impl P7RunnerPreflightReport {
    pub fn published_release_identity(&self) -> P7PublishedReleaseIdentity {
        P7PublishedReleaseIdentity {
            sdk_build_fingerprint: self.sdk_build_fingerprint.clone(),
            runner_build_fingerprint: self.runner_build_fingerprint.clone(),
            runner_lock_fingerprint: self.runner_lock_fingerprint.clone(),
            executable_sha256: self.executable_sha256.clone(),
            build_profile: self.build_profile.clone(),
            gate_attestation_sha256: self.gate_attestation_sha256.clone(),
            release_metadata_sha256: self.release_metadata_sha256.clone(),
            gate_source_fingerprint: self.gate_source_fingerprint.clone(),
            gate_source_manifest_sha256: self.gate_source_manifest_sha256.clone(),
            gate_ids: self.gate_ids.clone(),
        }
    }
}

pub fn p7_cohort_admission_creation_sequence() -> Vec<P7CohortAdmissionStep> {
    vec![
        P7CohortAdmissionStep {
            ordinal: 1,
            stage: P7CohortAdmissionStage::PreflightVerified,
        },
        P7CohortAdmissionStep {
            ordinal: 2,
            stage: P7CohortAdmissionStage::MaximumRssVerified,
        },
        P7CohortAdmissionStep {
            ordinal: 3,
            stage: P7CohortAdmissionStage::AdmissionPublished,
        },
    ]
}

pub fn validate_p7_cohort_admission_contract(
    admission: &P7CohortAdmission,
    run_id: &str,
    preflight_report_sha256: &str,
    maximum_rss_report_sha256: &str,
    release: &P7PublishedReleaseIdentity,
) -> Result<()> {
    let expected_plan = p7_release_gate_plan();
    let expected_producer_identity_sha256 = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(release).expect("P7 published release identity is serializable")
        )
    );
    if admission.schema_version != P7_COHORT_ADMISSION_SCHEMA_VERSION
        || admission.run_id != run_id
        || admission.creation_sequence != p7_cohort_admission_creation_sequence()
        || admission.preflight_report_sha256 != preflight_report_sha256
        || admission.maximum_rss_report_sha256 != maximum_rss_report_sha256
        || admission.orchestrator_plan_sha256 != expected_plan.plan_sha256
        || admission.producer_identity_sha256 != expected_producer_identity_sha256
        || admission.verifier_identity_sha256 != maximum_rss_report_sha256
        || &admission.release != release
        || !is_sha256(&admission.preflight_report_sha256)
        || !is_sha256(&admission.maximum_rss_report_sha256)
        || !is_sha256(&admission.orchestrator_plan_sha256)
        || !is_sha256(&admission.producer_identity_sha256)
        || !is_sha256(&admission.verifier_identity_sha256)
        || !p7_published_release_identity_is_valid(&admission.release)
    {
        return Err(p7_provenance_error(
            "P7 cohort admission does not bind the ordered preflight, RSS, and release identity",
        ));
    }
    Ok(())
}

fn p7_published_release_identity_is_valid(release: &P7PublishedReleaseIdentity) -> bool {
    release.build_profile == "release"
        && release.gate_ids == P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
        && [
            release.sdk_build_fingerprint.as_str(),
            release.runner_build_fingerprint.as_str(),
            release.runner_lock_fingerprint.as_str(),
            release.executable_sha256.as_str(),
            release.gate_attestation_sha256.as_str(),
            release.release_metadata_sha256.as_str(),
            release.gate_source_fingerprint.as_str(),
            release.gate_source_manifest_sha256.as_str(),
        ]
        .into_iter()
        .all(is_sha256)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7MaximumRssEvidence {
    pub schema_version: String,
    pub completed: bool,
    pub rss_gate_passed: bool,
    pub run_id: String,
    pub suite: String,
    pub dataset_file: String,
    pub dataset_sha256: String,
    pub input_bytes: u64,
    pub dataset_index: usize,
    pub question_index: usize,
    pub question_id: String,
    pub question_sha256: String,
    pub maximum_rss_bytes: u64,
    pub rss_limit_bytes: u64,
    pub measurement_report_sha256: String,
    pub measurement_child_exit_status: i32,
    pub measurement_elapsed_millis: u64,
    pub supervisor_receipt: crate::p7_process::P7ProcessReceipt,
    pub measured_executable_canonical_path: String,
    pub measured_executable_sha256: String,
    pub preflight_report_sha256: String,
    pub runner_stdout_sha256: String,
    pub runner_stderr_sha256: String,
    pub detail_sha256: String,
    pub summary_sha256: String,
    pub preflight_validated_after_measurement: bool,
    pub preflight: P7RunnerPreflightReport,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7MeasuredArtifactIdentity {
    pub device: u64,
    pub inode: u64,
    pub byte_len: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7MaximumRssMeasurementReport {
    pub schema_version: String,
    pub run_id: String,
    pub child_exit_status: i32,
    pub child_executable_canonical_path: String,
    pub child_executable_sha256: String,
    pub child_args: Vec<String>,
    pub maximum_rss_bytes: u64,
    pub supervisor_receipt: crate::p7_process::P7ProcessReceipt,
    pub runner_stdout: P7MeasuredArtifactIdentity,
    pub runner_stderr: P7MeasuredArtifactIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7RegularFileFreshness {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    modified_seconds: i64,
    #[cfg(unix)]
    modified_nanoseconds: i64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

impl P7RegularFileFreshness {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            mode: metadata.mode(),
            #[cfg(unix)]
            modified_seconds: metadata.mtime(),
            #[cfg(unix)]
            modified_nanoseconds: metadata.mtime_nsec(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum P7PlatformFileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows {
        volume_serial_number: u64,
        file_id: [u8; 16],
    },
}

#[cfg(unix)]
fn p7_platform_file_identity(file: &File) -> Result<P7PlatformFileIdentity> {
    let metadata = file.metadata().map_err(|source| Error::Io {
        source,
        stage: "p7_provenance_stat_file_identity",
    })?;
    Ok(P7PlatformFileIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn p7_platform_file_identity(file: &File) -> Result<P7PlatformFileIdentity> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };

    let mut info = MaybeUninit::<FILE_ID_INFO>::uninit();
    // SAFETY: file owns a live handle and info has the exact layout requested by FileIdInfo.
    let status = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if status == 0 {
        return Err(Error::Io {
            source: std::io::Error::last_os_error(),
            stage: "p7_provenance_read_file_identity",
        });
    }
    // SAFETY: a successful GetFileInformationByHandleEx initialized info.
    let info = unsafe { info.assume_init() };
    Ok(P7PlatformFileIdentity::Windows {
        volume_serial_number: info.VolumeSerialNumber,
        file_id: info.FileId.Identifier,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7OpenedArtifact {
    path: PathBuf,
    file_name: String,
    canonical_path: PathBuf,
    freshness: P7RegularFileFreshness,
    identity: P7PlatformFileIdentity,
    sha256: String,
}

impl P7OpenedArtifact {
    #[cfg(test)]
    fn capture(path: &Path, canonical_parent: &Path, canonical_root: &Path) -> Result<Self> {
        let mut session = P7ArtifactReadSession::default();
        let (_, _, _) = session.read_with(
            path,
            canonical_parent,
            canonical_root,
            None,
            P7ArtifactReadKind::Control,
            |reader, _| {
                std::io::copy(reader, &mut std::io::sink()).map_err(|source| Error::Io {
                    source,
                    stage: "p7_capture_snapshot",
                })?;
                Ok(())
            },
        )?;
        session.verify_retained()?;
        Ok(session.retained.remove(0).artifact)
    }

    fn open(path: &Path, owner: &P7RetainedDirectoryOwner) -> Result<(Self, File)> {
        if path.parent() != Some(owner.path()) {
            return Err(p7_provenance_error(
                "P7 streamed artifact escaped its canonical owner directory",
            ));
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| p7_provenance_error("P7 artifact name is not valid UTF-8"))?
            .to_string();
        owner.verify_unchanged().map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_verify_owner_before_open",
        })?;
        let file = owner
            .open_existing_file(&file_name)
            .map_err(|source| Error::Io {
                source,
                stage: "p7_provenance_openat_streamed_artifact",
            })?;
        Self::from_open_file(path, owner, file)
    }

    fn from_open_file(
        path: &Path,
        owner: &P7RetainedDirectoryOwner,
        file: File,
    ) -> Result<(Self, File)> {
        if path.parent() != Some(owner.path()) {
            return Err(p7_provenance_error(
                "P7 streamed artifact escaped its retained owner directory",
            ));
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| p7_provenance_error("P7 artifact name is not valid UTF-8"))?
            .to_string();
        let handle_metadata = file.metadata().map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_stat_open_streamed_artifact",
        })?;
        let freshness = P7RegularFileFreshness::from_metadata(&handle_metadata);
        let identity = p7_platform_file_identity(&file)?;
        owner
            .verify_file_identity(&file_name, &file)
            .map_err(|source| Error::Io {
                source,
                stage: "p7_provenance_verify_openat_artifact_identity",
            })?;
        Ok((
            Self {
                path: path.to_path_buf(),
                file_name,
                canonical_path: path.to_path_buf(),
                freshness,
                identity,
                sha256: String::new(),
            },
            file,
        ))
    }

    fn verify_retained_handle_unchanged(
        &self,
        file: &File,
        owner: &P7RetainedDirectoryOwner,
    ) -> Result<()> {
        owner.verify_unchanged().map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_verify_retained_owner",
        })?;
        owner
            .verify_file_identity(&self.file_name, file)
            .map_err(|source| Error::Io {
                source,
                stage: "p7_provenance_verify_retained_artifact_entry",
            })?;
        let handle_changed = file
            .metadata()
            .map(|metadata| P7RegularFileFreshness::from_metadata(&metadata) != self.freshness)
            .unwrap_or(true);
        let identity_changed = p7_platform_file_identity(file)
            .map(|identity| identity != self.identity)
            .unwrap_or(true);
        if handle_changed || identity_changed || owner.path().join(&self.file_name) != self.path {
            return Err(p7_provenance_error(
                "P7 evidence artifact identity or freshness changed during verification",
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    fn verify_unchanged(&self) -> Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| p7_provenance_error("P7 snapshot artifact has no owner"))?;
        let owner = P7RetainedDirectoryOwner::open_root(parent).map_err(|source| Error::Io {
            source,
            stage: "p7_snapshot_open_owner",
        })?;
        let file = owner
            .open_existing_file(&self.file_name)
            .map_err(|source| Error::Io {
                source,
                stage: "p7_snapshot_open_artifact",
            })?;
        self.verify_retained_handle_unchanged(&file, &owner)
    }
}

#[cfg(test)]
type P7RegularArtifactSnapshot = P7OpenedArtifact;

trait P7ArtifactAdmissionIdentity {
    fn canonical_path(&self) -> &Path;
    fn physical_identity(&self) -> Option<&P7PlatformFileIdentity>;
}

impl P7ArtifactAdmissionIdentity for P7OpenedArtifact {
    fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    fn physical_identity(&self) -> Option<&P7PlatformFileIdentity> {
        Some(&self.identity)
    }
}

impl P7ArtifactAdmissionIdentity for Path {
    fn canonical_path(&self) -> &Path {
        self
    }

    fn physical_identity(&self) -> Option<&P7PlatformFileIdentity> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum P7ArtifactReadKind {
    Control,
    Dataset,
    Detail,
    Operator,
    Release,
    Summary,
}

#[derive(Default)]
struct P7ArtifactReadLedger {
    canonical_paths: BTreeSet<PathBuf>,
    physical_identities: BTreeSet<P7PlatformFileIdentity>,
    admitted_artifact_bytes: u64,
    artifact_bytes_read: u64,
    detail_artifact_bytes_read: u64,
    admitted_detail_bytes: u64,
    full_read_pass_count: u64,
    duplicate_artifact_count: u64,
}

impl P7ArtifactReadLedger {
    fn admit<T: P7ArtifactAdmissionIdentity + ?Sized>(
        &mut self,
        artifact: &T,
        byte_len: u64,
        kind: P7ArtifactReadKind,
    ) -> Result<()> {
        let duplicate_path = self.canonical_paths.contains(artifact.canonical_path());
        let duplicate_identity = artifact
            .physical_identity()
            .is_some_and(|identity| self.physical_identities.contains(identity));
        if duplicate_path || duplicate_identity {
            self.duplicate_artifact_count = self
                .duplicate_artifact_count
                .checked_add(1)
                .ok_or_else(|| p7_provenance_error("P7 duplicate artifact count overflow"))?;
            return Err(p7_provenance_error(
                "P7 verifier attempted a duplicate path or physical artifact read",
            ));
        }
        self.canonical_paths
            .insert(artifact.canonical_path().to_path_buf());
        if let Some(identity) = artifact.physical_identity() {
            self.physical_identities.insert(identity.clone());
        }
        if kind == P7ArtifactReadKind::Detail && byte_len > P7_MAX_DETAIL_ARTIFACT_BYTES {
            return Err(p7_provenance_error(
                "P7 detail artifact exceeds its individual metadata admission",
            ));
        }
        let admitted_detail_bytes = if kind == P7ArtifactReadKind::Detail {
            self.admitted_detail_bytes
                .checked_add(byte_len)
                .ok_or_else(|| p7_provenance_error("P7 all-detail admission overflow"))?
        } else {
            self.admitted_detail_bytes
        };
        if admitted_detail_bytes > P7_MAX_ALL_DETAIL_ARTIFACT_BYTES {
            return Err(p7_provenance_error(
                "P7 detail artifacts exceed the all-detail metadata admission",
            ));
        }
        let admitted_artifact_bytes = self
            .admitted_artifact_bytes
            .checked_add(byte_len)
            .ok_or_else(|| p7_provenance_error("P7 global artifact admission overflow"))?;
        if admitted_artifact_bytes > P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES {
            return Err(p7_provenance_error(
                "P7 artifacts exceed the global metadata admission",
            ));
        }
        self.admitted_detail_bytes = admitted_detail_bytes;
        self.admitted_artifact_bytes = admitted_artifact_bytes;
        Ok(())
    }

    fn complete(
        &mut self,
        admitted_len: u64,
        bytes_read: u64,
        kind: P7ArtifactReadKind,
    ) -> Result<()> {
        if bytes_read != admitted_len {
            return Err(p7_provenance_error(
                "P7 artifact byte length changed during its full read pass",
            ));
        }
        self.full_read_pass_count = self
            .full_read_pass_count
            .checked_add(1)
            .ok_or_else(|| p7_provenance_error("P7 full read pass count overflow"))?;
        self.artifact_bytes_read = self
            .artifact_bytes_read
            .checked_add(bytes_read)
            .ok_or_else(|| p7_provenance_error("P7 artifact bytes-read overflow"))?;
        if kind == P7ArtifactReadKind::Detail {
            self.detail_artifact_bytes_read = self
                .detail_artifact_bytes_read
                .checked_add(bytes_read)
                .ok_or_else(|| p7_provenance_error("P7 detail bytes-read overflow"))?;
        }
        Ok(())
    }

    fn performance(&self, elapsed: Duration) -> P7VerifierPerformanceReport {
        P7VerifierPerformanceReport {
            schema_version: P7_VERIFIER_PERFORMANCE_SCHEMA_VERSION.to_string(),
            elapsed_millis: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
            max_elapsed_millis: P7_VERIFIER_MAX_WALL_TIME.as_millis() as u64,
            unique_artifact_count: self.canonical_paths.len() as u64,
            full_read_pass_count: self.full_read_pass_count,
            admitted_artifact_bytes: self.admitted_artifact_bytes,
            artifact_bytes_read: self.artifact_bytes_read,
            detail_artifact_bytes_read: self.detail_artifact_bytes_read,
            duplicate_artifact_count: self.duplicate_artifact_count,
            max_artifact_bytes_read: P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES,
            passed: elapsed <= P7_VERIFIER_MAX_WALL_TIME
                && self.admitted_artifact_bytes <= P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES
                && self.artifact_bytes_read <= P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES
                && self.canonical_paths.len() as u64 == self.full_read_pass_count
                && self.duplicate_artifact_count == 0,
        }
    }
}

struct P7RetainedArtifact {
    artifact: P7OpenedArtifact,
    file: File,
    owner: Rc<P7RetainedDirectoryOwner>,
}

impl P7RetainedArtifact {
    fn verify_unchanged(&self) -> Result<()> {
        self.artifact
            .verify_retained_handle_unchanged(&self.file, self.owner.as_ref())
    }
}

#[derive(Default)]
struct P7ArtifactReadSession {
    ledger: P7ArtifactReadLedger,
    retained: Vec<P7RetainedArtifact>,
    owners: BTreeMap<PathBuf, Rc<P7RetainedDirectoryOwner>>,
}

impl P7ArtifactReadSession {
    fn owner_for(
        &mut self,
        canonical_parent: &Path,
        canonical_root: &Path,
    ) -> Result<Rc<P7RetainedDirectoryOwner>> {
        if !canonical_parent.starts_with(canonical_root) {
            return Err(p7_provenance_error(
                "P7 artifact owner escaped the canonical benchmark root",
            ));
        }
        if let Some(owner) = self.owners.get(canonical_parent) {
            owner.verify_unchanged().map_err(|source| Error::Io {
                source,
                stage: "p7_provenance_verify_cached_owner",
            })?;
            return Ok(Rc::clone(owner));
        }

        let mut current_path = canonical_root.to_path_buf();
        let mut current = if let Some(owner) = self.owners.get(canonical_root) {
            Rc::clone(owner)
        } else {
            let owner = Rc::new(P7RetainedDirectoryOwner::open_root(canonical_root).map_err(
                |source| Error::Io {
                    source,
                    stage: "p7_provenance_open_retained_root",
                },
            )?);
            self.owners
                .insert(canonical_root.to_path_buf(), Rc::clone(&owner));
            owner
        };

        let relative = canonical_parent.strip_prefix(canonical_root).map_err(|_| {
            p7_provenance_error("P7 artifact owner is outside the canonical benchmark root")
        })?;
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(p7_provenance_error(
                    "P7 artifact owner contains a non-normal path component",
                ));
            };
            let component = component.to_str().ok_or_else(|| {
                p7_provenance_error("P7 artifact owner component is not valid UTF-8")
            })?;
            current_path.push(component);
            current = if let Some(owner) = self.owners.get(&current_path) {
                Rc::clone(owner)
            } else {
                let owner =
                    Rc::new(
                        current
                            .open_directory(component)
                            .map_err(|source| Error::Io {
                                source,
                                stage: "p7_provenance_openat_retained_directory",
                            })?,
                    );
                self.owners.insert(current_path.clone(), Rc::clone(&owner));
                owner
            };
        }
        current.verify_unchanged().map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_verify_retained_owner",
        })?;
        Ok(current)
    }

    #[allow(clippy::too_many_arguments)]
    fn read_with<T>(
        &mut self,
        path: &Path,
        canonical_parent: &Path,
        canonical_root: &Path,
        expected_sha256: Option<&str>,
        kind: P7ArtifactReadKind,
        consume: impl FnOnce(&mut dyn Read, u64) -> Result<T>,
    ) -> Result<(T, String, u64)> {
        let owner = self.owner_for(canonical_parent, canonical_root)?;
        let (artifact, file) = P7OpenedArtifact::open(path, owner.as_ref())?;
        self.consume_opened(artifact, file, owner, expected_sha256, kind, consume)
    }

    #[allow(clippy::too_many_arguments)]
    fn try_read_with<T>(
        &mut self,
        path: &Path,
        canonical_parent: &Path,
        canonical_root: &Path,
        expected_sha256: Option<&str>,
        kind: P7ArtifactReadKind,
        consume: impl FnOnce(&mut dyn Read, u64) -> Result<T>,
    ) -> Result<Option<(T, String, u64)>> {
        let owner = self.owner_for(canonical_parent, canonical_root)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| p7_provenance_error("P7 artifact name is not valid UTF-8"))?;
        let Some(file) = owner
            .try_open_existing_file(file_name)
            .map_err(|source| Error::Io {
                source,
                stage: "p7_provenance_try_openat_streamed_artifact",
            })?
        else {
            return Ok(None);
        };
        let (artifact, file) = P7OpenedArtifact::from_open_file(path, owner.as_ref(), file)?;
        self.consume_opened(artifact, file, owner, expected_sha256, kind, consume)
            .map(Some)
    }

    fn consume_opened<T>(
        &mut self,
        mut artifact: P7OpenedArtifact,
        mut file: File,
        owner: Rc<P7RetainedDirectoryOwner>,
        expected_sha256: Option<&str>,
        kind: P7ArtifactReadKind,
        consume: impl FnOnce(&mut dyn Read, u64) -> Result<T>,
    ) -> Result<(T, String, u64)> {
        if expected_sha256.is_some_and(|digest| !is_sha256(digest)) {
            return Err(p7_provenance_error(
                "P7 streamed artifact expected digest is invalid",
            ));
        }
        if self.retained.len() >= P7_MAX_RETAINED_ARTIFACT_HANDLES {
            return Err(p7_provenance_error(
                "P7 read session exceeded its retained handle limit",
            ));
        }
        let admitted_len = artifact.freshness.len;
        self.ledger.admit(&artifact, admitted_len, kind)?;
        let read_limit = admitted_len
            .checked_add(1)
            .ok_or_else(|| p7_provenance_error("P7 artifact read limit overflow"))?;
        let mut limited = (&mut file).take(read_limit);
        let mut reader = P7HashingReader::new(&mut limited);
        let material = consume(&mut reader, admitted_len)?;
        std::io::copy(&mut reader, &mut std::io::sink()).map_err(|source| Error::Io {
            source,
            stage: "p7_stream_artifact_growth_probe",
        })?;
        let (actual_sha256, bytes_read) = reader.finish();
        if bytes_read > admitted_len {
            return Err(p7_provenance_error(
                "P7 artifact grew beyond its admitted length during verification",
            ));
        }
        self.ledger.complete(admitted_len, bytes_read, kind)?;
        if expected_sha256.is_some_and(|expected| expected != actual_sha256) {
            return Err(p7_provenance_error("P7 artifact digest mismatch"));
        }
        artifact.sha256 = actual_sha256.clone();
        artifact.verify_retained_handle_unchanged(&file, owner.as_ref())?;
        self.retained.push(P7RetainedArtifact {
            artifact,
            file,
            owner,
        });
        Ok((material, actual_sha256, bytes_read))
    }

    fn read_json<T: DeserializeOwned>(
        &mut self,
        path: &Path,
        canonical_parent: &Path,
        canonical_root: &Path,
        expected_sha256: Option<&str>,
        kind: P7ArtifactReadKind,
        parse_stage: &'static str,
    ) -> Result<(T, String)> {
        let (parsed, digest, _) = self.read_with(
            path,
            canonical_parent,
            canonical_root,
            expected_sha256,
            kind,
            |reader, admitted_len| {
                if admitted_len > P7_MAX_CONTROL_JSON_BYTES {
                    return Err(p7_provenance_error(
                        "P7 JSON artifact exceeds its bounded parse limit",
                    ));
                }
                serde_json::from_reader(reader).map_err(|source| Error::Other {
                    source: Box::new(source),
                    stage: parse_stage,
                })
            },
        )?;
        Ok((parsed, digest))
    }

    fn try_read_json<T: DeserializeOwned>(
        &mut self,
        path: &Path,
        canonical_parent: &Path,
        canonical_root: &Path,
        expected_sha256: Option<&str>,
        kind: P7ArtifactReadKind,
        parse_stage: &'static str,
    ) -> Result<Option<(T, String)>> {
        self.try_read_with(
            path,
            canonical_parent,
            canonical_root,
            expected_sha256,
            kind,
            |reader, admitted_len| {
                if admitted_len > P7_MAX_CONTROL_JSON_BYTES {
                    return Err(p7_provenance_error(
                        "P7 JSON artifact exceeds its bounded parse limit",
                    ));
                }
                serde_json::from_reader(reader).map_err(|source| Error::Other {
                    source: Box::new(source),
                    stage: parse_stage,
                })
            },
        )
        .map(|material| material.map(|(parsed, digest, _)| (parsed, digest)))
    }

    fn read_raw(
        &mut self,
        path: &Path,
        canonical_parent: &Path,
        canonical_root: &Path,
        expected_sha256: Option<&str>,
        kind: P7ArtifactReadKind,
    ) -> Result<(String, u64)> {
        let (_, digest, bytes_read) = self.read_with(
            path,
            canonical_parent,
            canonical_root,
            expected_sha256,
            kind,
            |reader, _| {
                std::io::copy(reader, &mut std::io::sink()).map_err(|source| Error::Io {
                    source,
                    stage: "p7_stream_raw_artifact",
                })?;
                Ok(())
            },
        )?;
        Ok((digest, bytes_read))
    }

    fn verify_retained(&self) -> Result<()> {
        for owner in self.owners.values() {
            owner.verify_unchanged().map_err(|source| Error::Io {
                source,
                stage: "p7_provenance_verify_retained_directory_set",
            })?;
        }
        for artifact in &self.retained {
            artifact.verify_unchanged()?;
        }
        Ok(())
    }

    fn performance(&self, elapsed: Duration) -> P7VerifierPerformanceReport {
        self.ledger.performance(elapsed)
    }

    fn lifecycle_receipt(&self, lifecycle: &str) -> P7ArtifactLifecycleReceipt {
        P7ArtifactLifecycleReceipt {
            schema_version: P7_ARTIFACT_LIFECYCLE_RECEIPT_SCHEMA_VERSION.to_string(),
            lifecycle: lifecycle.to_string(),
            unique_artifact_count: self.ledger.canonical_paths.len() as u64,
            full_read_pass_count: self.ledger.full_read_pass_count,
            admitted_artifact_bytes: self.ledger.admitted_artifact_bytes,
            artifact_bytes_read: self.ledger.artifact_bytes_read,
            detail_artifact_bytes_read: self.ledger.detail_artifact_bytes_read,
            duplicate_artifact_count: self.ledger.duplicate_artifact_count,
            max_artifact_bytes_read: P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES,
            passed: self.ledger.admitted_artifact_bytes <= P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES
                && self.ledger.artifact_bytes_read <= P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES
                && self.ledger.canonical_paths.len() as u64 == self.ledger.full_read_pass_count
                && self.ledger.duplicate_artifact_count == 0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7RunnerBuildIdentity {
    pub sdk_build_fingerprint: String,
    pub runner_build_fingerprint: String,
    pub runner_lock_fingerprint: String,
    pub executable_sha256: String,
    pub build_profile: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7PublishedReleaseIdentity {
    pub sdk_build_fingerprint: String,
    pub runner_build_fingerprint: String,
    pub runner_lock_fingerprint: String,
    pub executable_sha256: String,
    pub build_profile: String,
    pub gate_attestation_sha256: String,
    pub release_metadata_sha256: String,
    pub gate_source_fingerprint: String,
    pub gate_source_manifest_sha256: String,
    pub gate_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P7VerifiedPublishedReleaseBundle {
    pub identity: P7PublishedReleaseIdentity,
    pub executable_canonical_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P7VerifiedPreflightArtifact {
    pub report: P7RunnerPreflightReport,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P7VerifiedWallCohortEvidence {
    pub preflight: P7RunnerPreflightReport,
    pub preflight_sha256: String,
    pub maximum_rss: P7MaximumRssEvidence,
    pub maximum_rss_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P7ShardBundleExpectation {
    pub run_id: String,
    pub suite: String,
    pub shard_index: usize,
    pub shard_total: usize,
    pub limit: Option<usize>,
    pub question_limit: Option<usize>,
    pub question_index: Option<usize>,
    pub build: P7RunnerBuildIdentity,
    pub release: P7PublishedReleaseIdentity,
    pub execution_kind: P7ProducerExecutionKind,
    pub cohort_admission_sha256: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct P7VerifiedShardBundle {
    pub summary: serde_json::Value,
    pub summary_sha256: String,
    pub detail_sha256: String,
    pub detail_rows: u64,
}

pub const P7_SHARD_BUNDLE_COMMIT_SCHEMA_VERSION: &str = "p7_shard_bundle_commit_v1";
pub const P7_MERGED_BUNDLE_COMMIT_SCHEMA_VERSION: &str = "p7_merged_bundle_commit_v1";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ShardBundleCommit {
    pub schema_version: String,
    pub run_id: String,
    pub suite: String,
    pub shard_index: usize,
    pub shard_total: usize,
    pub detail_file: String,
    pub detail_bytes: u64,
    pub detail_sha256: String,
    pub summary_file: String,
    pub summary_bytes: u64,
    pub summary_sha256: String,
    pub producer_identity_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7MergedBundleCommit {
    pub schema_version: String,
    pub run_id: String,
    pub suite: String,
    pub summary_file: String,
    pub summary_bytes: u64,
    pub summary_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P7UncommittedShardBundle {
    pub summary_present: bool,
    pub detail_present: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum P7ShardBundleState {
    Absent,
    Uncommitted(P7UncommittedShardBundle),
    Complete(P7VerifiedShardBundle),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum P7CohortAdmissionStage {
    PreflightVerified,
    MaximumRssVerified,
    AdmissionPublished,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7CohortAdmissionStep {
    pub ordinal: u8,
    pub stage: P7CohortAdmissionStage,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7CohortAdmission {
    pub schema_version: String,
    pub run_id: String,
    pub creation_sequence: Vec<P7CohortAdmissionStep>,
    pub preflight_report_sha256: String,
    pub maximum_rss_report_sha256: String,
    pub orchestrator_plan_sha256: String,
    pub producer_identity_sha256: String,
    pub verifier_identity_sha256: String,
    #[serde(flatten)]
    pub release: P7PublishedReleaseIdentity,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum P7ProducerExecutionKind {
    #[default]
    CohortShard,
    MaximumRssDiagnostic,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ShardProducerProvenance {
    pub schema_version: String,
    pub execution_kind: P7ProducerExecutionKind,
    pub run_id: String,
    pub contract_version: String,
    pub sdk_report_schema_version: u32,
    pub sdk_build_fingerprint: String,
    pub runner_build_fingerprint: String,
    pub runner_lock_fingerprint: String,
    pub executable_sha256: String,
    pub build_profile: String,
    pub gate_attestation_sha256: String,
    pub release_metadata_sha256: String,
    pub gate_source_fingerprint: String,
    pub gate_source_manifest_sha256: String,
    pub gate_ids: Vec<String>,
    pub cohort_admission_sha256: String,
    pub input_sha256: String,
    pub detail_schema_version: String,
    pub detail_sha256: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7RecordedProducerIdentity {
    pub schema_version: String,
    pub canonical_identity: String,
    pub canonical_identity_sha256: String,
}

impl P7RecordedProducerIdentity {
    pub fn record<T: Serialize>(identity: &T) -> std::result::Result<Self, serde_json::Error> {
        let canonical_identity = serde_json::to_string(identity)?;
        let canonical_identity_sha256 =
            format!("{:x}", Sha256::digest(canonical_identity.as_bytes()));
        Ok(Self {
            schema_version: P7_RECORDED_PRODUCER_IDENTITY_SCHEMA_VERSION.to_string(),
            canonical_identity,
            canonical_identity_sha256,
        })
    }

    pub fn parse<T: DeserializeOwned + Serialize>(&self) -> Result<T> {
        if self.schema_version != P7_RECORDED_PRODUCER_IDENTITY_SCHEMA_VERSION
            || !is_sha256(&self.canonical_identity_sha256)
            || format!("{:x}", Sha256::digest(self.canonical_identity.as_bytes()))
                != self.canonical_identity_sha256
        {
            return Err(p7_provenance_error(
                "P7 recorded producer identity envelope or digest is invalid",
            ));
        }
        let parsed =
            serde_json::from_str::<T>(&self.canonical_identity).map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_recorded_producer_identity_parse",
            })?;
        let canonical = serde_json::to_string(&parsed).map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "p7_recorded_producer_identity_reencode",
        })?;
        if canonical != self.canonical_identity {
            return Err(p7_provenance_error(
                "P7 producer identity original is not canonical JSON",
            ));
        }
        Ok(parsed)
    }
}

fn p7_parse_recorded_shard_producer(
    value: &serde_json::Value,
) -> Result<P7ShardProducerProvenance> {
    let recorded =
        serde_json::from_value::<P7RecordedProducerIdentity>(value.clone()).map_err(|source| {
            Error::Other {
                source: Box::new(source),
                stage: "p7_recorded_shard_producer_envelope_parse",
            }
        })?;
    recorded.parse()
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum P7QuestionType {
    NoGold,
    SingleGold,
    MultiGold,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum P7EvaluationApplicability {
    Applicable,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum P7SafeLocatorVisibility {
    GovernedOpaque,
    Redacted,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct P7SafeLocatorView {
    visibility: P7SafeLocatorVisibility,
    reference: P7RequiredNullableString,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct P7RequiredNullableString(Option<String>);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7QuestionEvaluationContract {
    pub schema_version: String,
    pub question_type: P7QuestionType,
    pub canonical_gold_count: usize,
    pub applicability: P7EvaluationApplicability,
}

impl P7QuestionEvaluationContract {
    pub fn from_canonical_gold_count(canonical_gold_count: usize) -> Self {
        let (question_type, applicability) = match canonical_gold_count {
            0 => (
                P7QuestionType::NoGold,
                P7EvaluationApplicability::NotApplicable,
            ),
            1 => (
                P7QuestionType::SingleGold,
                P7EvaluationApplicability::Applicable,
            ),
            _ => (
                P7QuestionType::MultiGold,
                P7EvaluationApplicability::Applicable,
            ),
        };
        Self {
            schema_version: P7_QUESTION_EVALUATION_SCHEMA_VERSION.to_string(),
            question_type,
            canonical_gold_count,
            applicability,
        }
    }

    pub fn is_evidence_question(&self) -> bool {
        self.applicability == P7EvaluationApplicability::Applicable
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ProducerIdentity {
    pub schema_version: String,
    pub contract_version: String,
    pub sdk_report_schema_version: u32,
    pub sdk_build_fingerprint: String,
    pub runner_build_fingerprint: String,
    pub runner_lock_fingerprint: String,
    pub executable_sha256: String,
    pub build_profile: String,
    pub input_sha256: String,
    pub detail_schema_version: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7VerifierIdentity {
    pub schema_version: String,
    pub operator_build_fingerprint: String,
    pub operator_executable_sha256: String,
    pub build_profile: String,
    pub build_features: Vec<String>,
    pub release_manifest_sha256: String,
    pub source_anchor_sha256: String,
    pub verification_policy_contract: String,
    pub verification_schema_version: String,
    pub verifier_digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7VerifierReleaseManifest {
    pub schema_version: String,
    pub executable_file_name: String,
    pub executable_sha256: String,
    pub build_profile: String,
    pub build_features: Vec<String>,
    pub verification_policy_contract: String,
    pub verification_schema_version: String,
    pub source_anchor_sha256: String,
    pub frozen_anchor_sha256: String,
    pub anchor_generator_receipt_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7VerifierReleasePublishReport {
    pub executable_canonical_path: PathBuf,
    pub executable_sha256: String,
    pub manifest_sha256: String,
    pub reused_identical: bool,
}

pub struct P7VerifierExecutionAuthority {
    identity: P7VerifierIdentity,
    process_authority: crate::p7_secure_fs::P7ProcessExecutionAuthority,
    release_session: P7ArtifactReadSession,
}

impl P7VerifierExecutionAuthority {
    pub fn identity(&self) -> &P7VerifierIdentity {
        &self.identity
    }

    pub fn release_executable_path(&self) -> &Path {
        self.process_authority.locator()
    }

    pub fn verify_retained(&mut self) -> Result<()> {
        self.process_authority
            .verify_retained()
            .map_err(|source| Error::Io {
                source,
                stage: "p7_verifier_revalidate_process_execution_authority",
            })?;
        self.release_session.verify_retained()
    }

    pub fn initialize_cohort<'authority>(
        &'authority mut self,
        root: &Path,
        run_id: &str,
    ) -> Result<crate::p7_secure_fs::P7AuthorityBoundArtifactTransaction<'authority>> {
        crate::p7_secure_fs::initialize_authority_bound_p7_cohort(self, root, run_id).map_err(
            |source| Error::Io {
                source,
                stage: "p7_verifier_initialize_authority_bound_cohort",
            },
        )
    }

    pub fn open_cohort<'authority>(
        &'authority mut self,
        root: &Path,
        run_id: &str,
    ) -> Result<crate::p7_secure_fs::P7AuthorityBoundArtifactTransaction<'authority>> {
        crate::p7_secure_fs::open_authority_bound_p7_cohort(self, root, run_id).map_err(|source| {
            Error::Io {
                source,
                stage: "p7_verifier_open_authority_bound_cohort",
            }
        })
    }
}

impl crate::p7_secure_fs::P7ExternalWriteAuthority for P7VerifierExecutionAuthority {
    fn verify_external_write_authority(&mut self) -> std::io::Result<()> {
        P7VerifierExecutionAuthority::verify_retained(self)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))
    }

    fn process_execution_authority(
        &mut self,
    ) -> &mut crate::p7_secure_fs::P7ProcessExecutionAuthority {
        &mut self.process_authority
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7VerificationReceipt {
    pub schema_version: String,
    pub cohort_digest: String,
    pub verifier_digest: String,
    pub receipt_digest: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7SoulRegressionGateReport {
    pub schema_version: String,
    pub inspect_contract_passed: bool,
    pub continuity_contract_passed: bool,
    pub recovery_contract_passed: bool,
    pub revision_contract_passed: bool,
    pub command_receipts: Vec<P7SoulRegressionCommandReceipt>,
    pub passed: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7SoulRegressionCommandReceipt {
    pub contract: String,
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7VerifierPerformanceReport {
    pub schema_version: String,
    pub elapsed_millis: u64,
    pub max_elapsed_millis: u64,
    pub unique_artifact_count: u64,
    pub full_read_pass_count: u64,
    pub admitted_artifact_bytes: u64,
    pub artifact_bytes_read: u64,
    pub detail_artifact_bytes_read: u64,
    pub duplicate_artifact_count: u64,
    pub max_artifact_bytes_read: u64,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ArtifactLifecycleReceipt {
    pub schema_version: String,
    pub lifecycle: String,
    pub unique_artifact_count: u64,
    pub full_read_pass_count: u64,
    pub admitted_artifact_bytes: u64,
    pub artifact_bytes_read: u64,
    pub detail_artifact_bytes_read: u64,
    pub duplicate_artifact_count: u64,
    pub max_artifact_bytes_read: u64,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseGateReceipt {
    pub gate_id: String,
    pub owner_root: String,
    pub argv: Vec<String>,
    pub tool_sha256: String,
    pub environment_sha256: String,
    pub exit_code: i32,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub source_fingerprint_after: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum P7ReleaseGateOwner {
    AgentMemory,
    ExternalRunner,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseGatePlanStep {
    pub ordinal: u8,
    pub gate_id: String,
    pub owner: P7ReleaseGateOwner,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseGatePlan {
    pub schema_version: String,
    pub orchestrator_contract: String,
    pub producer_identity_contract: String,
    pub verifier_identity_contract: String,
    pub steps: Vec<P7ReleaseGatePlanStep>,
    pub plan_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub enum P7ReleaseSourceManifestEntryKind {
    RegularFile,
    SymbolicLink,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseSourceManifestEntry {
    pub owner: String,
    pub relative_path: String,
    pub entry_kind: P7ReleaseSourceManifestEntryKind,
    pub byte_len: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseSourceManifest {
    pub schema_version: String,
    pub fingerprint_contract: String,
    pub source_fingerprint: String,
    pub entries: Vec<P7ReleaseSourceManifestEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseToolIdentity {
    pub logical_name: String,
    pub canonical_path: String,
    pub sha256: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseEnvironmentAttestation {
    pub variables: BTreeMap<String, String>,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseGateAttestation {
    pub schema_version: String,
    pub orchestrator_contract: String,
    pub plan: P7ReleaseGatePlan,
    pub identity: P7RunnerBuildIdentity,
    pub source_fingerprint: String,
    pub source_manifest_sha256: String,
    pub tools: Vec<P7ReleaseToolIdentity>,
    pub environment: P7ReleaseEnvironmentAttestation,
    pub gates: Vec<P7ReleaseGateReceipt>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ReleaseMetadata {
    pub schema_version: String,
    pub canonical_executable_path: String,
    pub identity: P7RunnerBuildIdentity,
    pub gate_attestation_sha256: String,
    pub gate_source_fingerprint: String,
    pub gate_source_manifest_sha256: String,
    pub gate_ids: Vec<String>,
}

#[derive(Clone, Copy)]
struct P7TrustedDataset {
    suite: &'static str,
    file_name: &'static str,
    input_sha256: &'static str,
}

const P7_TRUSTED_DATASETS: [P7TrustedDataset; 4] = [
    P7TrustedDataset {
        suite: "locomo",
        file_name: "locomo10.json",
        input_sha256: "79fa87e90f04081343b8c8debecb80a9a6842b76a7aa537dc9fdf651ea698ff4",
    },
    P7TrustedDataset {
        suite: "longmemeval_oracle",
        file_name: "longmemeval_oracle.json",
        input_sha256: "821a2034d219ab45846873dd14c14f12cfe7776e73527a483f9dac095d38620c",
    },
    P7TrustedDataset {
        suite: "longmemeval_s_cleaned",
        file_name: "longmemeval_s_cleaned.json",
        input_sha256: "d6f21ea9d60a0d56f34a05b609c79c88a451d2ae03597821ea3d5a9678c3a442",
    },
    P7TrustedDataset {
        suite: "longmemeval_m_cleaned",
        file_name: "longmemeval_m_cleaned.json",
        input_sha256: "9d79e5524794a2e6900a3aa9cb7d9152c5a3e8319c9a87c25494ba1eacee495f",
    },
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchmarkGateReport {
    pub suite: String,
    pub cases: usize,
    pub passed_cases: usize,
    pub failed_cases: Vec<String>,
    pub passed: bool,
}

pub fn run_recall_benchmark_gate(cases: &[RecallBenchmarkCase]) -> BenchmarkGateReport {
    let results = run_recall_benchmark_suite(cases);
    let failed_cases = results
        .iter()
        .filter(|result| !result.passed)
        .map(|result| result.case_name.to_string())
        .collect::<Vec<_>>();
    BenchmarkGateReport {
        suite: "recall".to_string(),
        cases: results.len(),
        passed_cases: results.len().saturating_sub(failed_cases.len()),
        passed: failed_cases.is_empty(),
        failed_cases,
    }
}

pub fn run_persona_governance_benchmark_gate(
    cases: &[PersonaGovernanceReplayCase],
) -> BenchmarkGateReport {
    let results = run_persona_governance_replay_suite(cases);
    let failed_cases = results
        .iter()
        .filter(|result| !result.passed)
        .map(|result| result.case_name.to_string())
        .collect::<Vec<_>>();
    BenchmarkGateReport {
        suite: "persona_governance".to_string(),
        cases: results.len(),
        passed_cases: results.len().saturating_sub(failed_cases.len()),
        passed: failed_cases.is_empty(),
        failed_cases,
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkClass {
    RecallMultisession,
    TemporalUpdate,
    SubjectProjection,
    SoulRegression,
    ProceduralReuse,
    PrivacyRefusal,
    AgentToolExperience,
}

impl MemoryBenchmarkClass {
    pub const ALL: [Self; 7] = [
        Self::RecallMultisession,
        Self::TemporalUpdate,
        Self::SubjectProjection,
        Self::SoulRegression,
        Self::ProceduralReuse,
        Self::PrivacyRefusal,
        Self::AgentToolExperience,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecallMultisession => "recall_multisession",
            Self::TemporalUpdate => "temporal_update",
            Self::SubjectProjection => "subject_projection",
            Self::SoulRegression => "soul_regression",
            Self::ProceduralReuse => "procedural_reuse",
            Self::PrivacyRefusal => "privacy_refusal",
            Self::AgentToolExperience => "agent_tool_experience",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkMode {
    Compact,
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkFixture {
    #[serde(default)]
    pub schema: String,
    pub fixture_id: String,
    pub class: MemoryBenchmarkClass,
    pub profile: ProfileId,
    pub mode: MemoryBenchmarkMode,
    #[serde(default)]
    pub evaluation_source: MemoryBenchmarkEvaluationSource,
    pub description: String,
    #[serde(default)]
    pub scenario: MemoryBenchmarkScenario,
    #[serde(default)]
    pub semantic_contract: MemoryBenchmarkSemanticContract,
    pub metrics: MemoryBenchmarkMetrics,
    pub thresholds: MemoryBenchmarkThresholds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_recall: Option<MemoryBenchmarkEvalRecall>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkEvaluationSource {
    #[default]
    ContractBaseline,
    RuntimeReplay,
    GoldenJudge,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkScenario {
    #[serde(default)]
    pub user_goal: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub expected_surfaces: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecall {
    #[serde(default)]
    pub suite: String,
    #[serde(default)]
    pub split: String,
    #[serde(default)]
    pub question_id: String,
    #[serde(default)]
    pub question_type: String,
    #[serde(default)]
    pub expected_evidence_refs: Vec<String>,
    #[serde(default)]
    pub source_candidates: Vec<String>,
    #[serde(default)]
    pub graph_anchor_candidates: Vec<String>,
    #[serde(default)]
    pub expanded_candidates: Vec<String>,
    #[serde(default)]
    pub eval_candidate_pool: Vec<String>,
    #[serde(default)]
    pub selected_candidates: Vec<String>,
    #[serde(default)]
    pub rendered_candidates: Vec<String>,
    #[serde(default)]
    pub rendered_block_preview: String,
    #[serde(default)]
    pub rendered_evidence_refs: Vec<String>,
    #[serde(default)]
    pub evidence_ref_index: Vec<MemoryBenchmarkEvalRecallEvidenceRefIndexEntry>,
    #[serde(default)]
    pub missing_evidence_refs: Vec<String>,
    #[serde(default)]
    pub diagnostics: MemoryBenchmarkEvalRecallDiagnostics,
    #[serde(default)]
    pub metrics: MemoryBenchmarkEvalRecallMetrics,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallEvidenceRefIndexEntry {
    #[serde(default)]
    pub candidate_id: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallStageEvidenceRefs {
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallGoldRank {
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub evidence_ref: String,
    #[serde(default)]
    pub rank: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallGraphDistanceToGold {
    #[serde(default)]
    pub candidate_id: String,
    #[serde(default)]
    pub evidence_ref: String,
    #[serde(default)]
    pub distance: Option<u8>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallDiagnostics {
    #[serde(default)]
    pub evidence_count: usize,
    #[serde(default)]
    pub first_any_hit_stage: String,
    #[serde(default)]
    pub first_all_hit_stage: String,
    #[serde(default)]
    pub matched_gold_by_stage: Vec<MemoryBenchmarkEvalRecallStageEvidenceRefs>,
    #[serde(default)]
    pub missing_gold_by_stage: Vec<MemoryBenchmarkEvalRecallStageEvidenceRefs>,
    #[serde(default)]
    pub gold_rank_by_stage: Vec<MemoryBenchmarkEvalRecallGoldRank>,
    #[serde(default)]
    pub miss_after_expanded: bool,
    #[serde(default)]
    pub source_anchor_ids: Vec<String>,
    #[serde(default)]
    pub graph_anchor_candidate_ids: Vec<String>,
    #[serde(default)]
    pub expanded_node_ids: Vec<String>,
    #[serde(default)]
    pub graph_neighbor_ids: Vec<String>,
    #[serde(default)]
    pub graph_distance_to_gold: Vec<MemoryBenchmarkEvalRecallGraphDistanceToGold>,
    #[serde(default)]
    pub truncated_count: usize,
    #[serde(default)]
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallMetrics {
    #[serde(default)]
    pub recall_at_k: Vec<MemoryBenchmarkEvalRecallAtK>,
    #[serde(default)]
    pub mrr_bps: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallAtK {
    pub k: usize,
    pub any_evidence_hit: bool,
    pub all_evidence_hit: bool,
    #[serde(default)]
    pub matched_evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkMetrics {
    pub accuracy_bps: u16,
    pub evidence_precision_bps: u16,
    pub projection_faithfulness_bps: u16,
    pub privacy_violation_count: u32,
    pub stale_memory_false_positive_count: u32,
    pub procedural_reuse_success_bps: u16,
    pub soul_regression_count: u32,
    pub latency_ms: u32,
    pub token_budget: u32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkThresholds {
    #[serde(default)]
    pub min_accuracy_bps: u16,
    #[serde(default)]
    pub min_evidence_precision_bps: u16,
    #[serde(default)]
    pub min_projection_faithfulness_bps: u16,
    #[serde(default)]
    pub max_privacy_violation_count: u32,
    #[serde(default)]
    pub max_stale_memory_false_positive_count: u32,
    #[serde(default)]
    pub min_procedural_reuse_success_bps: u16,
    #[serde(default)]
    pub max_soul_regression_count: u32,
    #[serde(default)]
    pub max_latency_ms: Option<u32>,
    #[serde(default)]
    pub max_token_budget: Option<u32>,
    #[serde(default)]
    pub max_memory_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkSemanticDimension {
    ProjectionShape,
    PrivacyRuntimeSemantics,
    SoulLifeSemantics,
    WorkIntegritySemantics,
    AgentToolExperienceSemantics,
    W4EvalRecallSemantics,
}

impl MemoryBenchmarkSemanticDimension {
    pub const ALL: [Self; 6] = [
        Self::ProjectionShape,
        Self::PrivacyRuntimeSemantics,
        Self::SoulLifeSemantics,
        Self::WorkIntegritySemantics,
        Self::AgentToolExperienceSemantics,
        Self::W4EvalRecallSemantics,
    ];
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkSemanticContract {
    #[serde(default)]
    pub dimensions: Vec<MemoryBenchmarkSemanticDimension>,
    #[serde(default)]
    pub provided_keys: Vec<String>,
    #[serde(default)]
    pub required_keys: Vec<String>,
    #[serde(default)]
    pub forbidden_keys: Vec<String>,
    #[serde(default)]
    pub observed_markers: Vec<String>,
    #[serde(default)]
    pub required_markers: Vec<String>,
    #[serde(default)]
    pub forbidden_markers: Vec<String>,
}

impl MemoryBenchmarkSemanticContract {
    fn is_empty(&self) -> bool {
        self.dimensions.is_empty()
            && self.provided_keys.is_empty()
            && self.required_keys.is_empty()
            && self.forbidden_keys.is_empty()
            && self.observed_markers.is_empty()
            && self.required_markers.is_empty()
            && self.forbidden_markers.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkBaseline {
    pub accuracy_bps: u16,
    pub evidence_precision_bps: u16,
    pub projection_faithfulness_bps: u16,
    pub privacy_violation_count: u32,
    pub stale_memory_false_positive_count: u32,
    pub procedural_reuse_success_bps: u16,
    pub soul_regression_count: u32,
    pub latency_ms: u32,
    pub token_budget: u32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkClassCoverage {
    pub class: MemoryBenchmarkClass,
    pub compact_fixtures: usize,
    pub full_fixtures: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkMissingClass {
    pub class: MemoryBenchmarkClass,
    pub mode: MemoryBenchmarkMode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkSemanticCoverage {
    pub dimension: MemoryBenchmarkSemanticDimension,
    pub fixture_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkFailure {
    pub fixture_id: String,
    pub class: MemoryBenchmarkClass,
    pub mode: MemoryBenchmarkMode,
    pub profile: ProfileId,
    pub stage: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkSemanticFailure {
    pub fixture_id: String,
    pub dimension: Option<MemoryBenchmarkSemanticDimension>,
    pub stage: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkReport {
    pub suite: String,
    pub total_fixtures: usize,
    pub passed_fixtures: usize,
    pub baseline: MemoryBenchmarkBaseline,
    pub class_coverage: Vec<MemoryBenchmarkClassCoverage>,
    pub missing_classes: Vec<MemoryBenchmarkMissingClass>,
    pub semantic_coverage: Vec<MemoryBenchmarkSemanticCoverage>,
    pub soul_kernel_judge: SoulKernelBenchmarkJudgeReport,
    pub subject_projection_judge: SubjectProjectionBenchmarkJudgeReport,
    pub agent_tool_experience_judge: AgentToolExperienceBenchmarkJudgeReport,
    pub w4_eval_recall_judge: W4EvalRecallBenchmarkJudgeReport,
    pub failures: Vec<MemoryBenchmarkFailure>,
    pub semantic_failures: Vec<MemoryBenchmarkSemanticFailure>,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulKernelBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub growth_proposal_contract_covered: bool,
    pub regression_suite_covered: bool,
    pub feedback_report_covered: bool,
    pub compact_digest_covered: bool,
    pub no_roleplay_gate_passed: bool,
    pub life_slot_gate_passed: bool,
    pub work_integrity_gate_passed: bool,
    pub privacy_zero_gate_passed: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectProjectionBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub projection_report_covered: bool,
    pub budget_compiler_covered: bool,
    pub faithfulness_gate_passed: bool,
    pub private_disclosure_integrity_gate_passed: bool,
    pub gateway_raw_audit_redaction_covered: bool,
    pub raw_audit_disabled_reason_covered: bool,
    pub cross_surface_consistency_passed: bool,
    pub benchmark_judge_attached: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentToolExperienceBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub no_experience_empty_hints_covered: bool,
    pub governed_experience_hint_covered: bool,
    pub schema_drift_rejection_covered: bool,
    pub private_observation_not_public_covered: bool,
    pub gateway_no_cold_route_covered: bool,
    pub compact_registry_forbidden_covered: bool,
    pub host_execution_boundary_covered: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4EvalRecallBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub fixture_count: usize,
    pub required_k_covered: bool,
    pub missing_evidence_reported: bool,
    pub source_expanded_selected_split_covered: bool,
    pub w4_1_diagnostic_schema_covered: bool,
    pub w4_1_candidate_pool_split_covered: bool,
    pub mrr_covered: bool,
    pub noisy_external_wall_required: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyBenchmarkSummary {
    #[serde(default)]
    pub suite: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub shards: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_source_sha256: Option<String>,
    #[serde(skip)]
    operator_content_hash_verified: bool,
    #[serde(skip)]
    producer_identity_digest: Option<String>,
    #[serde(default)]
    pub samples: usize,
    #[serde(default)]
    pub questions: usize,
    #[serde(default)]
    pub evidence_questions: usize,
    #[serde(default)]
    pub no_gold_questions: usize,
    #[serde(default)]
    pub any_evidence_hit: usize,
    #[serde(default)]
    pub all_evidence_hit: usize,
    #[serde(default)]
    pub write_errors: usize,
    #[serde(default)]
    pub recall_errors: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_hit_counts: Option<W4ExternalNoisyStageHitCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_diagnostics: Option<W4ExternalNoisyIndexDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w4_1_diagnostics: Option<W4ExternalNoisyW41Diagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facet_ablation: Option<W4ExternalNoisyFacetAblationDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p7_loss_ledger: Option<W4ExternalNoisyP7LossDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p7_production_delivery: Option<W4ExternalNoisyP7ProductionDeliveryDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p7_provenance: Option<P7MergedProvenance>,
}

impl W4ExternalNoisyBenchmarkSummary {
    pub fn operator_content_hash_verified(&self) -> bool {
        self.operator_content_hash_verified
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyStageHitCounts {
    #[serde(default)]
    pub source_any_evidence_hit: usize,
    #[serde(default)]
    pub source_all_evidence_hit: usize,
    #[serde(default)]
    pub expanded_any_evidence_hit: usize,
    #[serde(default)]
    pub expanded_all_evidence_hit: usize,
    #[serde(default)]
    pub reranked_any_evidence_hit: usize,
    #[serde(default)]
    pub reranked_all_evidence_hit: usize,
    #[serde(default)]
    pub selected_any_evidence_hit: usize,
    #[serde(default)]
    pub selected_all_evidence_hit: usize,
    #[serde(default)]
    pub projection_selected_any_evidence_hit: usize,
    #[serde(default)]
    pub projection_selected_all_evidence_hit: usize,
    #[serde(default)]
    pub rendered_any_evidence_hit: usize,
    #[serde(default)]
    pub rendered_all_evidence_hit: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyIndexDiagnostics {
    #[serde(default)]
    pub questions_with_index_report: usize,
    #[serde(default)]
    pub index_used_questions: usize,
    #[serde(default)]
    pub fallback_full_scan_questions: usize,
    #[serde(default)]
    pub source_candidate_count: usize,
    #[serde(default)]
    pub matched_source_anchor_count: usize,
    #[serde(default)]
    pub unmatched_source_anchor_count: usize,
    #[serde(default)]
    pub indexed_neighbor_count: usize,
    #[serde(default)]
    pub filtered_node_count: usize,
    #[serde(default)]
    pub filtered_edge_count: usize,
    #[serde(default)]
    pub filtered_backlink_count: usize,
    #[serde(default)]
    pub failure_count: usize,
    pub graph_manifest_contract_verified_questions: usize,
    pub graph_selected_dependency_chain_verified_questions: usize,
    pub graph_full_scope_closure_verified_questions: usize,
    pub graph_manifest_generation_present_questions: usize,
    pub graph_revision_present_questions: usize,
    pub graph_scope_digest_present_questions: usize,
    pub graph_maintenance_required_questions: usize,
    pub graph_incident_questions: usize,
    pub graph_read_path_mutation_delta: usize,
    #[serde(default)]
    pub facet_questions_with_index_report: usize,
    #[serde(default)]
    pub facet_index_used_questions: usize,
    #[serde(default)]
    pub facet_report_only_questions: usize,
    #[serde(default)]
    pub facet_fallback_full_scan_questions: usize,
    #[serde(default)]
    pub facet_source_candidate_count: usize,
    #[serde(default)]
    pub facet_matched_source_candidate_count: usize,
    #[serde(default)]
    pub facet_posting_key_lookup_count: usize,
    #[serde(default)]
    pub facet_manifest_matched_posting_count: usize,
    #[serde(default)]
    pub facet_posting_doc_read_count: usize,
    #[serde(default)]
    pub facet_owner_key_lookup_count: usize,
    #[serde(default)]
    pub facet_owner_doc_read_count: usize,
    #[serde(default)]
    pub facet_zero_posting_key_lookup_questions: usize,
    #[serde(default)]
    pub facet_clean_zero_hit_questions: usize,
    #[serde(default)]
    pub facet_manifest_integrity_verified_questions: usize,
    #[serde(default)]
    pub facet_manifest_integrity_failure_count: usize,
    #[serde(default)]
    pub facet_exact_match_count: usize,
    #[serde(default)]
    pub facet_expanded_match_count: usize,
    #[serde(default)]
    pub facet_failure_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyW41Diagnostics {
    #[serde(default)]
    pub questions_with_w4_1_diagnostics: usize,
    #[serde(default)]
    pub first_any_hit_stage_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub first_all_hit_stage_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub missing_gold_by_stage_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub miss_after_expanded_count: usize,
    #[serde(default)]
    pub gold_rank_found_count: usize,
    #[serde(default)]
    pub gold_rank_missing_count: usize,
    #[serde(default)]
    pub gold_rank_sum: usize,
    #[serde(default)]
    pub truncated_count: usize,
    #[serde(default)]
    pub blocked_reason_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub question_type_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub evidence_count_buckets: BTreeMap<String, usize>,
    #[serde(default)]
    pub source_signature_count: usize,
    #[serde(default)]
    pub repeated_source_signature_questions: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyFacetAblationDiagnostics {
    #[serde(default)]
    pub questions_with_ablation_report: usize,
    #[serde(default)]
    pub method_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub delivery_contribution_proven_questions: usize,
    #[serde(default)]
    pub render_growth: usize,
    #[serde(default)]
    pub required_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub report_available_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub delivery_contribution_proven_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub delivery_affected_candidate_occurrences: usize,
    #[serde(default)]
    pub selected_evidence_hit_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub rendered_evidence_hit_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub selected_all_hit_loss_count: BTreeMap<String, usize>,
    #[serde(default)]
    pub evidence_family_rotation_selected_all_hit_loss_count: BTreeMap<String, usize>,
    #[serde(default)]
    pub rendered_all_hit_loss_count: BTreeMap<String, usize>,
    #[serde(default)]
    pub expanded_candidate_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub selected_candidate_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub rendered_candidate_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub rendered_char_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub blocked_reason_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyP7LossDiagnostics {
    #[serde(default)]
    pub questions_with_loss_ledger: usize,
    #[serde(default)]
    pub expanded_hit_selected_miss_questions: usize,
    #[serde(default)]
    pub eval_selected_hit_rendered_miss_questions: usize,
    #[serde(default)]
    pub expanded_hit_selected_miss_evidence: usize,
    #[serde(default)]
    pub eval_selected_hit_rendered_miss_evidence: usize,
    #[serde(default)]
    pub eval_selected_hit_projection_selected_miss_questions: usize,
    #[serde(default)]
    pub eval_selected_hit_projection_selected_miss_evidence: usize,
    #[serde(default)]
    pub selected_hit_final_rendered_miss_questions: usize,
    #[serde(default)]
    pub selected_hit_final_rendered_miss_evidence: usize,
    #[serde(default)]
    pub eval_truncated_count: usize,
    #[serde(default)]
    pub eval_blocked_reason_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyP7ProductionDeliveryDiagnostics {
    #[serde(default)]
    pub questions_with_delivery_report: usize,
    #[serde(default)]
    pub eval_selected_matches_delivery_questions: usize,
    #[serde(default)]
    pub eval_rendered_matches_delivery_questions: usize,
    #[serde(default)]
    pub projection_selected_sources_proven_questions: usize,
    #[serde(default)]
    pub projection_delivery_proof_questions: usize,
    #[serde(default)]
    pub final_projection_integrity_questions: usize,
    #[serde(default)]
    pub final_projection_integrity_passed_questions: usize,
    #[serde(default)]
    pub final_projection_raw_private_violation_count: usize,
    #[serde(default)]
    pub final_projection_blocked_source_count: usize,
    #[serde(default)]
    pub final_projection_redacted_source_count: usize,
    #[serde(default)]
    pub schema_version_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub render_growth: usize,
    #[serde(default)]
    pub privacy_leak_count: usize,
    #[serde(default)]
    pub cross_subject_leak_count: usize,
    #[serde(default)]
    pub raw_soul_private_material_count: usize,
    #[serde(default)]
    pub blocked_reason_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub delivery_drop_reason_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyP7ShardDigest {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub shard: String,
    #[serde(default)]
    pub summary_sha256: String,
    #[serde(default)]
    pub detail_sha256: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7MergedProvenance {
    pub schema_version: String,
    pub run_id: String,
    pub contract_version: String,
    pub sdk_report_schema_version: u32,
    pub sdk_build_fingerprint: String,
    pub runner_build_fingerprint: String,
    pub runner_lock_fingerprint: String,
    pub executable_sha256: String,
    pub build_profile: String,
    pub gate_attestation_sha256: String,
    pub release_metadata_sha256: String,
    pub gate_source_fingerprint: String,
    pub gate_source_manifest_sha256: String,
    pub gate_ids: Vec<String>,
    pub cohort_admission_sha256: String,
    pub input_sha256: String,
    pub detail_schema_version: String,
    pub producer_identity: P7RecordedProducerIdentity,
    pub merged_detail_sha256: String,
    pub ordered_shard_digest_manifest: Vec<W4ExternalNoisyP7ShardDigest>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisySuiteReport {
    pub suite: String,
    pub run_id: String,
    pub completed: bool,
    pub samples: usize,
    pub questions: usize,
    pub evidence_questions: usize,
    pub no_gold_questions: usize,
    pub any_evidence_hit: usize,
    pub all_evidence_hit: usize,
    pub write_errors: usize,
    pub recall_errors: usize,
    pub shard_count: usize,
    pub expected_shard_count: Option<usize>,
    pub shards_valid: bool,
    pub expected_samples: Option<usize>,
    pub expected_questions: Option<usize>,
    pub expected_evidence_questions: Option<usize>,
    pub row_counts_valid: bool,
    pub summary_sha256: Option<String>,
    pub runner_source_sha256: Option<String>,
    pub any_evidence_hit_bps: u32,
    pub all_evidence_hit_bps: u32,
    pub noisy_split: bool,
    pub oracle_sanity_only: bool,
    pub baseline_any_evidence_hit: Option<usize>,
    pub baseline_all_evidence_hit: Option<usize>,
    pub regressed_against_baseline: bool,
    pub improved_against_baseline: bool,
    pub stage_hit_counts: Option<W4ExternalNoisyStageHitCounts>,
    pub index_diagnostics: Option<W4ExternalNoisyIndexDiagnostics>,
    pub w4_1_diagnostics: Option<W4ExternalNoisyW41Diagnostics>,
    pub facet_ablation: Option<W4ExternalNoisyFacetAblationDiagnostics>,
    pub p7_loss_ledger: Option<W4ExternalNoisyP7LossDiagnostics>,
    pub p7_production_delivery: Option<W4ExternalNoisyP7ProductionDeliveryDiagnostics>,
    pub p7_provenance: Option<P7MergedProvenance>,
    pub stage_attributed_improvement: bool,
    pub index_effect_proven: bool,
    pub facet_ablation_effect_proven: bool,
    pub facet_ablation_no_render_growth: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyWallReport {
    pub release_gate_passed: bool,
    pub benchmark_gate_passed: bool,
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p7_preflight: Option<P7RunnerPreflightReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p7_maximum_rss: Option<P7MaximumRssEvidence>,
    pub p7_maximum_rss_attached: bool,
    pub p7_maximum_rss_within_limit: bool,
    pub cohort_valid: bool,
    pub summary_attached: bool,
    pub required_suites_covered: bool,
    pub noisy_splits_covered: bool,
    pub completed: bool,
    pub no_runner_errors: bool,
    pub row_counts_covered: bool,
    pub shards_valid: bool,
    pub provenance_attached: bool,
    pub stage_diagnostics_attached: bool,
    pub index_diagnostics_attached: bool,
    pub index_no_full_scan: bool,
    pub w4_1_diagnostics_attached: bool,
    pub facet_ablation_attached: bool,
    pub oracle_sanity_only: bool,
    pub noisy_improvement_proven: bool,
    pub stage_attributed_improvement_proven: bool,
    pub index_effect_proven: bool,
    pub facet_ablation_effect_proven: bool,
    pub facet_ablation_no_render_growth: bool,
    pub p7_loss_ledger_attached: bool,
    pub p7_selection_loss_reduced: bool,
    pub p7_render_loss_reduced: bool,
    pub p7_ablation_effect_proven: bool,
    pub p7_no_render_growth: bool,
    pub p7_index_no_full_scan: bool,
    pub p7_no_privacy_regression: bool,
    pub p7_soul_regression_gate: P7SoulRegressionGateReport,
    pub p7_no_p6_regression: bool,
    pub p7_production_delivery_proven: bool,
    pub p7_provenance_valid: bool,
    pub producer_identities: Vec<P7ProducerIdentity>,
    pub verifier_identity: P7VerifierIdentity,
    pub verification_receipt: Option<P7VerificationReceipt>,
    pub verifier_performance: P7VerifierPerformanceReport,
    pub suite_reports: Vec<W4ExternalNoisySuiteReport>,
    pub blocked_reasons: Vec<String>,
}

pub fn evaluate_w4_external_noisy_wall(
    summaries: &[W4ExternalNoisyBenchmarkSummary],
) -> W4ExternalNoisyWallReport {
    let suite_reports = summaries
        .iter()
        .map(w4_external_noisy_suite_report)
        .collect::<Vec<_>>();
    let summary_attached = !summaries.is_empty();
    let required_suites = [
        "locomo",
        "longmemeval_oracle",
        "longmemeval_s_cleaned",
        "longmemeval_m_cleaned",
    ];
    let noisy_suites = ["locomo", "longmemeval_s_cleaned", "longmemeval_m_cleaned"];
    let required_suites_covered = summaries.len() == required_suites.len()
        && required_suites.iter().all(|suite| {
            summaries
                .iter()
                .filter(|summary| summary.suite == *suite)
                .count()
                == 1
        });
    let cohort_run_ids = summaries
        .iter()
        .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
        .filter_map(|summary| {
            summary.p7_provenance.as_ref().and_then(|provenance| {
                (p7_valid_run_id(&summary.run_id) && provenance.run_id == summary.run_id)
                    .then(|| summary.run_id.clone())
            })
        })
        .collect::<BTreeSet<_>>();
    let cohort_valid = required_suites_covered
        && cohort_run_ids.len() == 1
        && summaries.iter().all(|summary| {
            summary
                .p7_provenance
                .as_ref()
                .is_some_and(|provenance| provenance.run_id == summary.run_id)
        });
    let run_id = cohort_valid
        .then(|| cohort_run_ids.iter().next().cloned())
        .flatten();
    let noisy_splits_covered = noisy_suites
        .iter()
        .all(|suite| summaries.iter().any(|summary| summary.suite == *suite));
    let completed = summary_attached
        && required_suites_covered
        && required_suites.iter().all(|suite| {
            summaries
                .iter()
                .find(|summary| summary.suite == *suite)
                .is_some_and(|summary| summary.completed)
        });
    let no_runner_errors = summaries
        .iter()
        .all(|summary| summary.write_errors == 0 && summary.recall_errors == 0);
    let row_counts_covered = required_suites_covered
        && suite_reports
            .iter()
            .filter(|report| required_suites.iter().any(|suite| report.suite == *suite))
            .all(|report| report.row_counts_valid);
    let shards_valid = required_suites_covered
        && suite_reports
            .iter()
            .filter(|report| required_suites.iter().any(|suite| report.suite == *suite))
            .all(|report| report.shards_valid);
    let provenance_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| {
                summary
                    .summary_sha256
                    .as_deref()
                    .is_some_and(|hash| !hash.trim().is_empty())
                    && summary
                        .runner_source_sha256
                        .as_deref()
                        .is_some_and(|hash| !hash.trim().is_empty())
            });
    let stage_diagnostics_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| summary.stage_hit_counts.is_some());
    let index_diagnostics_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| summary.index_diagnostics.is_some());
    let index_no_full_scan = index_diagnostics_attached
        && required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(w4_external_index_diagnostics_no_full_scan);
    let w4_1_diagnostics_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(w4_external_w41_diagnostics_cover_summary);
    let facet_ablation_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(w4_external_facet_ablation_covers_summary);
    let facet_ablation_no_render_growth = facet_ablation_attached
        && required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| {
                summary
                    .facet_ablation
                    .as_ref()
                    .is_some_and(|diagnostics| diagnostics.render_growth == 0)
            });
    let oracle_sanity_only = true;
    let noisy_reports = suite_reports
        .iter()
        .filter(|report| report.noisy_split)
        .collect::<Vec<_>>();
    let noisy_improvement_proven = noisy_splits_covered
        && noisy_reports
            .iter()
            .all(|report| report.improved_against_baseline);
    let stage_attributed_improvement_proven = suite_reports
        .iter()
        .find(|report| report.suite == "longmemeval_m_cleaned")
        .is_some_and(|report| report.stage_attributed_improvement);
    let index_effect_proven = suite_reports
        .iter()
        .find(|report| report.suite == "longmemeval_m_cleaned")
        .is_some_and(|report| report.index_effect_proven);
    let facet_ablation_effect_proven = suite_reports
        .iter()
        .find(|report| report.suite == "longmemeval_m_cleaned")
        .is_some_and(|report| report.facet_ablation_effect_proven);
    let p7_loss_ledger_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(p7_loss_ledger_covers_summary);
    let p7_selection_loss_reduced = noisy_suites
        .iter()
        .all(|suite| p7_suite_quality_threshold_met(summaries, suite, true));
    let p7_render_loss_reduced = noisy_suites
        .iter()
        .all(|suite| p7_suite_quality_threshold_met(summaries, suite, false));
    let p7_ablation_effect_proven = noisy_suites
        .iter()
        .all(|suite| p7_ablation_proves_suite_effect(summaries, suite));
    let p7_no_render_growth = facet_ablation_no_render_growth
        && required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| {
                summary
                    .p7_production_delivery
                    .as_ref()
                    .is_some_and(|diagnostics| diagnostics.render_growth == 0)
            });
    let p7_index_no_full_scan = index_diagnostics_attached && index_no_full_scan;
    let p7_no_privacy_regression = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(p7_production_delivery_has_no_privacy_regression);
    let p7_no_p6_regression = shards_valid
        && noisy_improvement_proven
        && stage_attributed_improvement_proven
        && index_effect_proven
        && facet_ablation_effect_proven
        && facet_ablation_no_render_growth;
    let p7_production_delivery_proven = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(p7_production_delivery_covers_summary);
    let p7_provenance_valid = cohort_valid
        && required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(p7_provenance_valid_for_summary)
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .filter_map(|summary| summary.p7_provenance.as_ref())
            .map(|provenance| {
                (
                    provenance.run_id.clone(),
                    provenance.contract_version.clone(),
                    provenance.sdk_report_schema_version,
                    provenance.sdk_build_fingerprint.clone(),
                    provenance.runner_build_fingerprint.clone(),
                    provenance.runner_lock_fingerprint.clone(),
                    provenance.executable_sha256.clone(),
                    provenance.gate_attestation_sha256.clone(),
                    provenance.gate_source_fingerprint.clone(),
                    provenance.gate_ids.clone(),
                    provenance.build_profile.clone(),
                    provenance.detail_schema_version.clone(),
                )
            })
            .collect::<BTreeSet<_>>()
            .len()
            == 1;

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        summary_attached,
        "w4_external_noisy_wall_summary_missing",
    );
    push_missing(
        &mut blocked_reasons,
        required_suites_covered,
        "w4_external_noisy_wall_required_suites_missing",
    );
    push_missing(
        &mut blocked_reasons,
        noisy_splits_covered,
        "w4_external_noisy_wall_noisy_splits_missing",
    );
    push_missing(
        &mut blocked_reasons,
        completed,
        "w4_external_noisy_wall_incomplete",
    );
    push_missing(
        &mut blocked_reasons,
        no_runner_errors,
        "w4_external_noisy_wall_runner_errors",
    );
    push_missing(
        &mut blocked_reasons,
        row_counts_covered,
        "w4_external_noisy_wall_row_counts_invalid",
    );
    push_missing(
        &mut blocked_reasons,
        shards_valid,
        "w4_external_noisy_wall_shards_invalid",
    );
    push_missing(
        &mut blocked_reasons,
        provenance_attached,
        "w4_external_noisy_wall_provenance_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || stage_diagnostics_attached,
        "w4_external_noisy_wall_stage_diagnostics_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || index_diagnostics_attached,
        "w4_external_noisy_wall_index_diagnostics_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || index_no_full_scan,
        "w4_external_noisy_wall_index_full_scan_detected",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || w4_1_diagnostics_attached,
        "w4_external_noisy_wall_w4_1_diagnostics_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || facet_ablation_attached,
        "w4_external_noisy_wall_facet_ablation_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || facet_ablation_no_render_growth,
        "w4_external_noisy_wall_render_growth_detected",
    );
    push_missing(
        &mut blocked_reasons,
        noisy_improvement_proven,
        "w4_external_noisy_wall_improvement_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        stage_attributed_improvement_proven,
        "w4_external_noisy_wall_stage_attribution_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        index_effect_proven,
        "w4_external_noisy_wall_index_effect_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        facet_ablation_effect_proven,
        "w4_external_noisy_wall_facet_ablation_effect_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        p7_loss_ledger_attached,
        "p7_loss_ledger_missing",
    );
    push_missing(
        &mut blocked_reasons,
        p7_selection_loss_reduced,
        "p7_selection_loss_not_reduced",
    );
    push_missing(
        &mut blocked_reasons,
        p7_render_loss_reduced,
        "p7_render_loss_not_reduced",
    );
    push_missing(
        &mut blocked_reasons,
        p7_ablation_effect_proven,
        "p7_ablation_effect_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        p7_no_render_growth,
        "p7_render_growth_detected",
    );
    push_missing(
        &mut blocked_reasons,
        p7_index_no_full_scan,
        "p7_index_full_scan_detected",
    );
    push_missing(
        &mut blocked_reasons,
        p7_no_privacy_regression,
        "p7_privacy_regression",
    );
    push_missing(
        &mut blocked_reasons,
        p7_no_p6_regression,
        "p7_p6_regression",
    );
    push_missing(
        &mut blocked_reasons,
        p7_production_delivery_proven,
        "p7_production_delivery_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        p7_provenance_valid,
        "p7_provenance_invalid",
    );
    push_missing(&mut blocked_reasons, cohort_valid, "p7_run_cohort_invalid");
    blocked_reasons.push("p7_soul_regression_gate_missing".to_string());
    let benchmark_gate_passed = blocked_reasons.is_empty();
    blocked_reasons.push("p7_runner_preflight_missing".to_string());
    blocked_reasons.push("p7_maximum_rss_evidence_missing".to_string());
    blocked_reasons.sort();
    blocked_reasons.dedup();

    W4ExternalNoisyWallReport {
        release_gate_passed: false,
        benchmark_gate_passed,
        run_id,
        p7_preflight: None,
        p7_maximum_rss: None,
        p7_maximum_rss_attached: false,
        p7_maximum_rss_within_limit: false,
        cohort_valid,
        summary_attached,
        required_suites_covered,
        noisy_splits_covered,
        completed,
        no_runner_errors,
        row_counts_covered,
        shards_valid,
        provenance_attached,
        stage_diagnostics_attached,
        index_diagnostics_attached,
        index_no_full_scan,
        w4_1_diagnostics_attached,
        facet_ablation_attached,
        oracle_sanity_only,
        noisy_improvement_proven,
        stage_attributed_improvement_proven,
        index_effect_proven,
        facet_ablation_effect_proven,
        facet_ablation_no_render_growth,
        p7_loss_ledger_attached,
        p7_selection_loss_reduced,
        p7_render_loss_reduced,
        p7_ablation_effect_proven,
        p7_no_render_growth,
        p7_index_no_full_scan,
        p7_no_privacy_regression,
        p7_soul_regression_gate: P7SoulRegressionGateReport {
            schema_version: P7_SOUL_REGRESSION_GATE_SCHEMA_VERSION.to_string(),
            blocked_reasons: vec!["p7_soul_regression_gate_missing".to_string()],
            ..P7SoulRegressionGateReport::default()
        },
        p7_no_p6_regression,
        p7_production_delivery_proven,
        p7_provenance_valid,
        producer_identities: p7_producer_identities(summaries),
        verifier_identity: p7_verifier_identity(
            P7_EMBEDDED_OPERATOR_BUILD_FINGERPRINT,
            "",
            P7_OPERATOR_BUILD_PROFILE,
            p7_operator_build_features(),
            "",
            P7_EMBEDDED_OPERATOR_BUILD_FINGERPRINT,
        ),
        verification_receipt: None,
        verifier_performance: P7VerifierPerformanceReport {
            schema_version: P7_VERIFIER_PERFORMANCE_SCHEMA_VERSION.to_string(),
            elapsed_millis: 0,
            max_elapsed_millis: P7_VERIFIER_MAX_WALL_TIME.as_millis() as u64,
            unique_artifact_count: 0,
            full_read_pass_count: 0,
            admitted_artifact_bytes: 0,
            artifact_bytes_read: 0,
            detail_artifact_bytes_read: 0,
            duplicate_artifact_count: 0,
            max_artifact_bytes_read: P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES,
            passed: false,
        },
        suite_reports,
        blocked_reasons,
    }
}

pub fn bind_p7_verifier_identity(
    report: &mut W4ExternalNoisyWallReport,
    summaries: &[W4ExternalNoisyBenchmarkSummary],
    verifier: P7VerifierIdentity,
) {
    report.verification_receipt = p7_verification_receipt(summaries, &verifier);
    report.verifier_identity = verifier;
}

fn p7_producer_identity(provenance: &P7MergedProvenance) -> Option<P7ProducerIdentity> {
    provenance.producer_identity.parse().ok()
}

fn p7_json_digest(value: &impl Serialize) -> Option<String> {
    serde_json::to_vec(value)
        .ok()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
}

fn p7_detail_schema_supported(schema_version: &str) -> bool {
    matches!(schema_version, P7_DETAIL_SCHEMA_VERSION)
}

fn p7_producer_identities(
    summaries: &[W4ExternalNoisyBenchmarkSummary],
) -> Vec<P7ProducerIdentity> {
    let mut identities = summaries
        .iter()
        .filter_map(|summary| summary.p7_provenance.as_ref())
        .filter_map(p7_producer_identity)
        .collect::<Vec<_>>();
    identities.sort_by(|left, right| {
        (&left.input_sha256, &left.executable_sha256)
            .cmp(&(&right.input_sha256, &right.executable_sha256))
    });
    identities.dedup();
    identities
}

fn p7_verifier_identity(
    operator_build_fingerprint: &str,
    operator_executable_sha256: &str,
    build_profile: &str,
    mut build_features: Vec<String>,
    release_manifest_sha256: &str,
    source_anchor_sha256: &str,
) -> P7VerifierIdentity {
    build_features.sort();
    build_features.dedup();
    let mut identity = P7VerifierIdentity {
        schema_version: P7_VERIFIER_IDENTITY_SCHEMA_VERSION.to_string(),
        operator_build_fingerprint: operator_build_fingerprint.to_string(),
        operator_executable_sha256: operator_executable_sha256.to_string(),
        build_profile: build_profile.to_string(),
        build_features,
        release_manifest_sha256: release_manifest_sha256.to_string(),
        source_anchor_sha256: source_anchor_sha256.to_string(),
        verification_policy_contract: P7_VERIFICATION_POLICY_CONTRACT.to_string(),
        verification_schema_version: P7_VERIFICATION_RECEIPT_SCHEMA_VERSION.to_string(),
        verifier_digest: String::new(),
    };
    identity.verifier_digest = p7_json_digest(&identity).unwrap_or_default();
    identity
}

fn p7_operator_build_features() -> Vec<String> {
    P7_OPERATOR_BUILD_FEATURES
        .split(',')
        .filter(|feature| !feature.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn attest_p7_current_verifier_execution() -> Result<P7VerifierExecutionAuthority> {
    let mut process_authority =
        crate::p7_secure_fs::P7ProcessExecutionAuthority::claim().map_err(|source| Error::Io {
            source,
            stage: "p7_verifier_attest_inherited_execution_authority",
        })?;
    let current_executable = process_authority.locator().to_path_buf();
    p7_require_workspace_build_source_attestation()?;
    process_authority
        .verify_retained()
        .map_err(|source| Error::Io {
            source,
            stage: "p7_verifier_bind_execution_to_release_locator",
        })?;
    let execution_identity = process_authority.execution_identity().clone();
    let mut session = P7ArtifactReadSession::default();
    let identity = p7_verifier_identity_for_executable_with_session(
        &current_executable,
        P7VerifierExecutableEvidence::SealedExecution(execution_identity.clone()),
        &mut session,
    )?;
    if identity.operator_executable_sha256 != execution_identity.sha256 {
        return Err(p7_provenance_error(
            "P7 verifier executable differs from its launcher-issued SHA256",
        ));
    }
    session.verify_retained()?;
    Ok(P7VerifierExecutionAuthority {
        identity,
        process_authority,
        release_session: session,
    })
}

pub fn verify_p7_verifier_release_manifest_with_receipt(
    verifier_executable: &Path,
) -> Result<(P7VerifierIdentity, P7ArtifactLifecycleReceipt)> {
    p7_require_workspace_build_source_attestation()?;
    let retained = crate::p7_secure_fs::P7RetainedFile::open_executable(verifier_executable)
        .map_err(|source| Error::Io {
            source,
            stage: "p7_verifier_retain_release_executable",
        })?;
    let mut session = P7ArtifactReadSession::default();
    let identity = p7_verifier_identity_for_executable_with_session(
        verifier_executable,
        P7VerifierExecutableEvidence::RetainedRelease,
        &mut session,
    )?;
    retained.verify_unchanged().map_err(|source| Error::Io {
        source,
        stage: "p7_verifier_recheck_release_executable",
    })?;
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("verifier_release_manifest");
    Ok((identity, receipt))
}

pub fn publish_p7_verifier_release(
    benchmark_root: &Path,
) -> Result<P7VerifierReleasePublishReport> {
    let mut execution_authority = crate::p7_secure_fs::P7ProcessExecutionAuthority::claim()
        .map_err(|source| Error::Io {
            source,
            stage: "p7_verifier_publish_attest_execution_authority",
        })?;
    let verifier_executable = execution_authority.locator().to_path_buf();
    p7_require_workspace_build_source_attestation()?;
    p7_require_canonical_real_directory(benchmark_root)?;
    if P7_OPERATOR_BUILD_PROFILE != "release" {
        return Err(p7_provenance_error(
            "P7 verifier publisher requires a release-profile executable",
        ));
    }
    let executable_name = verifier_executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| p7_provenance_error("P7 verifier executable name is not UTF-8"))?
        .to_string();
    let mut transaction = execution_authority
        .begin_verifier_release(benchmark_root)
        .map_err(|source| Error::Io {
            source,
            stage: "p7_verifier_publish_begin_authority_bound_release",
        })?;
    let releases_path = transaction.releases_path().to_path_buf();
    if releases_path != benchmark_root.join(P7_VERIFIER_RELEASES_DIR) {
        return Err(p7_provenance_error(
            "P7 verifier release owner escaped the canonical benchmark root",
        ));
    }
    let publish = (|| -> Result<P7VerifierReleasePublishReport> {
        let executable_identity = transaction
            .copy_execution(&executable_name, 0o555)
            .map_err(|source| Error::Io {
                source,
                stage: "p7_verifier_publish_copy_authority_execution",
            })?;

        let mut build_features = p7_operator_build_features();
        build_features.sort();
        build_features.dedup();
        let manifest = P7VerifierReleaseManifest {
            schema_version: P7_VERIFIER_RELEASE_MANIFEST_SCHEMA_VERSION.to_string(),
            executable_file_name: executable_name.clone(),
            executable_sha256: executable_identity.sha256.clone(),
            build_profile: P7_OPERATOR_BUILD_PROFILE.to_string(),
            build_features,
            verification_policy_contract: P7_VERIFICATION_POLICY_CONTRACT.to_string(),
            verification_schema_version: P7_VERIFICATION_RECEIPT_SCHEMA_VERSION.to_string(),
            source_anchor_sha256: P7_EMBEDDED_OPERATOR_BUILD_FINGERPRINT.to_string(),
            frozen_anchor_sha256: P7_FROZEN_ANCHOR_SHA256.to_string(),
            anchor_generator_receipt_sha256: P7_FROZEN_ANCHOR_GENERATOR_RECEIPT_SHA256.to_string(),
        };
        let mut manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_verifier_publish_serialize_manifest",
            })?;
        manifest_bytes.push(b'\n');
        let manifest_sha256 = format!("{:x}", Sha256::digest(&manifest_bytes));
        transaction
            .publish_immutable_bytes(
                &manifest_bytes,
                ".verifier-release-manifest.tmp",
                P7_VERIFIER_RELEASE_MANIFEST_FILE_NAME,
            )
            .map_err(|source| Error::Io {
                source,
                stage: "p7_verifier_publish_manifest",
            })?;

        let release_path = releases_path.join(&executable_identity.sha256);
        let final_executable = release_path.join(&executable_name);
        let reused_identical = match transaction.install(&executable_identity.sha256) {
            Ok(()) => false,
            Err(source)
                if source.cleanup_permitted()
                    && source.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                transaction
                    .cleanup_uncommitted()
                    .map_err(|source| Error::Io {
                        source,
                        stage: "p7_verifier_publish_cleanup_reused_staging",
                    })?;
                true
            }
            Err(source) => {
                return Err(Error::Io {
                    source: source.into_inner(),
                    stage: "p7_verifier_publish_install_release",
                });
            }
        };
        let (published_identity, receipt) =
            verify_p7_verifier_release_manifest_with_receipt(&final_executable)?;
        if published_identity.operator_executable_sha256 != executable_identity.sha256
            || published_identity.release_manifest_sha256 != manifest_sha256
        {
            return Err(p7_provenance_error(
                "P7 existing verifier release differs from the staged content address",
            ));
        }
        if receipt.artifact_bytes_read == 0 {
            return Err(p7_provenance_error(
                "P7 verifier release verification read no artifact bytes",
            ));
        }
        Ok(P7VerifierReleasePublishReport {
            executable_canonical_path: final_executable,
            executable_sha256: executable_identity.sha256,
            manifest_sha256,
            reused_identical,
        })
    })();
    if publish.is_err() && !transaction.committed() {
        let _ = transaction.cleanup_uncommitted();
    }
    publish
}

fn p7_require_workspace_build_source_attestation() -> Result<()> {
    p7_validate_build_source_attestation(P7_BUILD_SOURCE_ATTESTATION)
}

fn p7_validate_build_source_attestation(attestation: &str) -> Result<()> {
    if attestation != P7_WORKSPACE_BUILD_SOURCE_ATTESTATION {
        return Err(p7_provenance_error(
            "P7 verifier release identity requires an attested workspace source build",
        ));
    }
    Ok(())
}

enum P7VerifierExecutableEvidence {
    RetainedRelease,
    SealedExecution(crate::p7_secure_fs::P7ContentIdentity),
}

fn p7_verifier_identity_for_executable_with_session(
    current_executable: &Path,
    executable_evidence: P7VerifierExecutableEvidence,
    session: &mut P7ArtifactReadSession,
) -> Result<P7VerifierIdentity> {
    if !current_executable.is_absolute() {
        return Err(p7_provenance_error(
            "P7 verifier executable locator must be absolute",
        ));
    }
    let parent = current_executable
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 operator executable has no owner directory"))?;
    let root = current_executable
        .ancestors()
        .last()
        .ok_or_else(|| p7_provenance_error("P7 operator executable has no filesystem root"))?;
    if fs::canonicalize(parent).map_err(|source| Error::Io {
        source,
        stage: "p7_operator_canonicalize_release_owner",
    })? != parent
    {
        return Err(p7_provenance_error(
            "P7 verifier release owner must be canonical",
        ));
    }
    let executable_sha256 = match executable_evidence {
        P7VerifierExecutableEvidence::RetainedRelease => {
            session
                .read_raw(
                    current_executable,
                    parent,
                    root,
                    None,
                    P7ArtifactReadKind::Operator,
                )?
                .0
        }
        P7VerifierExecutableEvidence::SealedExecution(identity) => {
            if identity.byte_len == 0 || !is_sha256(&identity.sha256) {
                return Err(p7_provenance_error(
                    "P7 sealed verifier execution identity is invalid",
                ));
            }
            identity.sha256
        }
    };
    let manifest_path = parent.join(P7_VERIFIER_RELEASE_MANIFEST_FILE_NAME);
    let (manifest, release_manifest_sha256) = session.read_json::<P7VerifierReleaseManifest>(
        &manifest_path,
        parent,
        root,
        None,
        P7ArtifactReadKind::Control,
        "p7_verifier_release_manifest_parse",
    )?;
    let executable_file_name = current_executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| p7_provenance_error("P7 verifier executable name is not UTF-8"))?;
    let content_address = parent
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| p7_provenance_error("P7 verifier release owner has no content address"))?;
    let mut expected_features = p7_operator_build_features();
    expected_features.sort();
    p7_validate_verifier_release_manifest(
        &manifest,
        executable_file_name,
        &executable_sha256,
        content_address,
        P7_OPERATOR_BUILD_PROFILE,
        &expected_features,
        P7_EMBEDDED_OPERATOR_BUILD_FINGERPRINT,
        &release_manifest_sha256,
    )?;
    Ok(p7_verifier_identity(
        P7_EMBEDDED_OPERATOR_BUILD_FINGERPRINT,
        &executable_sha256,
        &manifest.build_profile,
        manifest.build_features,
        &release_manifest_sha256,
        &manifest.source_anchor_sha256,
    ))
}

#[allow(clippy::too_many_arguments)]
fn p7_validate_verifier_release_manifest(
    manifest: &P7VerifierReleaseManifest,
    executable_file_name: &str,
    executable_sha256: &str,
    content_address: &str,
    embedded_build_profile: &str,
    expected_features: &[String],
    embedded_source_anchor: &str,
    release_manifest_sha256: &str,
) -> Result<()> {
    if manifest.schema_version != P7_VERIFIER_RELEASE_MANIFEST_SCHEMA_VERSION
        || manifest.executable_file_name != executable_file_name
        || manifest.executable_sha256 != executable_sha256
        || manifest.build_profile != "release"
        || embedded_build_profile != "release"
        || manifest.build_features != expected_features
        || manifest.verification_policy_contract != P7_VERIFICATION_POLICY_CONTRACT
        || manifest.verification_schema_version != P7_VERIFICATION_RECEIPT_SCHEMA_VERSION
        || manifest.source_anchor_sha256 != embedded_source_anchor
        || manifest.frozen_anchor_sha256 != P7_FROZEN_ANCHOR_SHA256
        || manifest.anchor_generator_receipt_sha256 != P7_FROZEN_ANCHOR_GENERATOR_RECEIPT_SHA256
        || !is_sha256(&manifest.frozen_anchor_sha256)
        || !is_sha256(&manifest.anchor_generator_receipt_sha256)
        || content_address != executable_sha256
        || !is_sha256(release_manifest_sha256)
    {
        return Err(p7_provenance_error(
            "P7 verifier release manifest, executable, profile, features, policy, or source anchor mismatch",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn p7_current_verifier_identity() -> P7VerifierIdentity {
    p7_verifier_identity(
        &"a".repeat(64),
        &"b".repeat(64),
        "release",
        Vec::new(),
        &"c".repeat(64),
        &"a".repeat(64),
    )
}

fn p7_verifier_identity_is_valid(identity: &P7VerifierIdentity) -> bool {
    identity.schema_version == P7_VERIFIER_IDENTITY_SCHEMA_VERSION
        && identity.verification_policy_contract == P7_VERIFICATION_POLICY_CONTRACT
        && identity.verification_schema_version == P7_VERIFICATION_RECEIPT_SCHEMA_VERSION
        && is_sha256(&identity.operator_build_fingerprint)
        && is_sha256(&identity.operator_executable_sha256)
        && identity.build_profile == "release"
        && is_sha256(&identity.release_manifest_sha256)
        && is_sha256(&identity.source_anchor_sha256)
        && identity.source_anchor_sha256 == identity.operator_build_fingerprint
        && identity
            .build_features
            .windows(2)
            .all(|features| features[0] < features[1])
        && is_sha256(&identity.verifier_digest)
        && p7_json_digest(&P7VerifierIdentity {
            verifier_digest: String::new(),
            ..identity.clone()
        })
        .as_deref()
            == Some(identity.verifier_digest.as_str())
}

fn p7_verification_receipt(
    summaries: &[W4ExternalNoisyBenchmarkSummary],
    verifier: &P7VerifierIdentity,
) -> Option<P7VerificationReceipt> {
    if summaries.is_empty()
        || summaries.iter().any(|summary| {
            !summary.operator_content_hash_verified
                || summary.producer_identity_digest.is_none()
                || summary.summary_sha256.is_none()
        })
    {
        return None;
    }
    let mut cohort_entries = summaries
        .iter()
        .map(|summary| {
            (
                summary.suite.clone(),
                summary.summary_sha256.clone().unwrap_or_default(),
                summary.producer_identity_digest.clone().unwrap_or_default(),
                summary
                    .p7_provenance
                    .as_ref()
                    .map(|provenance| provenance.merged_detail_sha256.clone())
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    cohort_entries.sort();
    p7_verification_receipt_for_cohort_entries(&cohort_entries, verifier)
}

fn p7_verification_receipt_for_cohort_entries(
    cohort_entries: &[(String, String, String, String)],
    verifier: &P7VerifierIdentity,
) -> Option<P7VerificationReceipt> {
    if !p7_verifier_identity_is_valid(verifier) {
        return None;
    }
    let cohort_digest = p7_json_digest(&cohort_entries)?;
    let verifier_digest = verifier.verifier_digest.clone();
    let receipt_digest = p7_json_digest(&(
        P7_VERIFICATION_RECEIPT_SCHEMA_VERSION,
        cohort_digest.as_str(),
        verifier_digest.as_str(),
    ))?;
    Some(P7VerificationReceipt {
        schema_version: P7_VERIFICATION_RECEIPT_SCHEMA_VERSION.to_string(),
        cohort_digest,
        verifier_digest,
        receipt_digest,
    })
}

pub fn attach_p7_soul_regression_gate(
    report: &mut W4ExternalNoisyWallReport,
    soul_gate: P7SoulRegressionGateReport,
) {
    report
        .blocked_reasons
        .retain(|reason| reason != "p7_soul_regression_gate_missing");
    if !soul_gate.passed {
        report
            .blocked_reasons
            .push("p7_soul_regression_gate_failed".to_string());
    }
    report.p7_soul_regression_gate = soul_gate;
    report.blocked_reasons.sort();
    report.blocked_reasons.dedup();
    report.benchmark_gate_passed = report.blocked_reasons.iter().all(|reason| {
        matches!(
            reason.as_str(),
            "p7_runner_preflight_missing" | "p7_maximum_rss_evidence_missing"
        )
    });
}

pub fn attach_p7_verifier_performance(
    report: &mut W4ExternalNoisyWallReport,
    performance: P7VerifierPerformanceReport,
) {
    report.verifier_performance = performance;
    report
        .blocked_reasons
        .retain(|reason| reason != "p7_verifier_performance_gate_failed");
    if !report.verifier_performance.passed {
        report
            .blocked_reasons
            .push("p7_verifier_performance_gate_failed".to_string());
    }
    report.blocked_reasons.sort();
    report.blocked_reasons.dedup();
}

pub fn run_p7_soul_regression_gate(sdk_root: &Path) -> Result<P7SoulRegressionGateReport> {
    let contracts: [(&str, &str, &[&str]); 4] = [
        (
            "inspect",
            "runtime::soul_kernel::tests::inspect_marks_bootstrap_empty_when_no_kernel_assets_exist",
            &[
                "test",
                "--release",
                "--locked",
                "--no-default-features",
                "-p",
                "bm-core",
                "runtime::soul_kernel::tests::inspect_marks_bootstrap_empty_when_no_kernel_assets_exist",
                "--",
                "--exact",
            ],
        ),
        (
            "continuity",
            "runtime::soul_kernel::tests::restore_runtime_bundle_repairs_missing_core_and_continuity",
            &[
                "test",
                "--release",
                "--locked",
                "--no-default-features",
                "-p",
                "bm-core",
                "runtime::soul_kernel::tests::restore_runtime_bundle_repairs_missing_core_and_continuity",
                "--",
                "--exact",
            ],
        ),
        (
            "recovery",
            "runtime_lifecycle_inspect_recover_and_close_are_sdk_level_operations",
            &[
                "test",
                "--release",
                "--locked",
                "--no-default-features",
                "--features",
                "nonproduction-replay-harness",
                "-p",
                "bm-sdk",
                "--test",
                "runtime_lifecycle_contract",
                "runtime_lifecycle_inspect_recover_and_close_are_sdk_level_operations",
                "--",
                "--exact",
            ],
        ),
        (
            "revision",
            "runtime_recover_commits_bundle_owner_facet_soul_and_lifecycle_atomically",
            &[
                "test",
                "--release",
                "--locked",
                "--no-default-features",
                "--features",
                "nonproduction-replay-harness",
                "-p",
                "bm-sdk",
                "--test",
                "runtime_lifecycle_contract",
                "runtime_recover_commits_bundle_owner_facet_soul_and_lifecycle_atomically",
                "--",
                "--exact",
            ],
        ),
    ];
    let mut receipts = Vec::with_capacity(contracts.len());
    let mut passed = BTreeMap::new();
    for (contract, exact_test_name, args) in contracts {
        let output = p7_run_supervised(
            Command::new("cargo").args(args).current_dir(sdk_root),
            Duration::from_secs(30 * 60),
            "p7_soul_regression_gate_execute",
        )?;
        let exit_code = output.status.code().unwrap_or(-1);
        passed.insert(
            contract,
            p7_exact_test_contract_passed(&output, exact_test_name),
        );
        receipts.push(P7SoulRegressionCommandReceipt {
            contract: contract.to_string(),
            argv: std::iter::once("cargo".to_string())
                .chain(args.iter().map(|arg| (*arg).to_string()))
                .collect(),
            exit_code,
            stdout_sha256: format!("{:x}", Sha256::digest(&output.stdout)),
            stderr_sha256: format!("{:x}", Sha256::digest(&output.stderr)),
        });
    }
    let inspect_contract_passed = passed.get("inspect").copied().unwrap_or(false);
    let continuity_contract_passed = passed.get("continuity").copied().unwrap_or(false);
    let recovery_contract_passed = passed.get("recovery").copied().unwrap_or(false);
    let revision_contract_passed = passed.get("revision").copied().unwrap_or(false);
    let mut blocked_reasons = Vec::new();
    for (name, value) in [
        ("inspect", inspect_contract_passed),
        ("continuity", continuity_contract_passed),
        ("recovery", recovery_contract_passed),
        ("revision", revision_contract_passed),
    ] {
        if !value {
            blocked_reasons.push(format!("soul_{name}_contract_failed"));
        }
    }
    Ok(P7SoulRegressionGateReport {
        schema_version: P7_SOUL_REGRESSION_GATE_SCHEMA_VERSION.to_string(),
        inspect_contract_passed,
        continuity_contract_passed,
        recovery_contract_passed,
        revision_contract_passed,
        command_receipts: receipts,
        passed: blocked_reasons.is_empty(),
        blocked_reasons,
    })
}

fn p7_exact_test_contract_passed(
    output: &crate::p7_process::P7ProcessOutput,
    exact_test_name: &str,
) -> bool {
    output.succeeded() && p7_exact_test_stdout_passed(&output.stdout, exact_test_name)
}

fn p7_exact_test_stdout_passed(stdout: &[u8], exact_test_name: &str) -> bool {
    let stdout = String::from_utf8_lossy(stdout);
    stdout
        .lines()
        .any(|line| line == format!("test {exact_test_name} ... ok"))
        && stdout
            .lines()
            .any(|line| line.starts_with("test result: ok. 1 passed; 0 failed;"))
}

fn p7_run_supervised(
    command: &mut Command,
    timeout: Duration,
    stage: &'static str,
) -> Result<crate::p7_process::P7ProcessOutput> {
    let output = run_p7_bounded_command(
        command,
        P7ProcessLimits {
            stdout_bytes: P7_PROCESS_STDOUT_CAP_BYTES,
            stderr_bytes: P7_PROCESS_STDERR_CAP_BYTES,
            total_bytes: P7_PROCESS_TOTAL_CAP_BYTES,
            timeout,
        },
    )
    .map_err(|source| Error::Io { source, stage })?;
    if output.termination != P7ProcessTermination::Exited {
        return Err(p7_provenance_error(
            "P7 supervised child exceeded its output or time budget",
        ));
    }
    Ok(output)
}

fn p7_run_retained_executable(
    executable: &Path,
    args: &[&str],
    stage: &'static str,
) -> Result<crate::p7_process::P7ProcessOutput> {
    let output = run_p7_bounded_retained_executable(
        executable,
        args,
        P7ProcessLimits {
            stdout_bytes: P7_MAX_CONTROL_JSON_BYTES,
            stderr_bytes: P7_MAX_CONTROL_JSON_BYTES,
            total_bytes: P7_MAX_CONTROL_JSON_BYTES,
            timeout: Duration::from_secs(5 * 60),
        },
    )
    .map_err(|source| Error::Io { source, stage })?;
    if output.termination != P7ProcessTermination::Exited {
        return Err(p7_preflight_error(
            "P7 retained executable exceeded its output or time budget",
        ));
    }
    Ok(output)
}

pub fn finalize_w4_external_noisy_release_report(
    mut report: W4ExternalNoisyWallReport,
    preflight: P7RunnerPreflightReport,
    maximum_rss: P7MaximumRssEvidence,
) -> Result<W4ExternalNoisyWallReport> {
    let run_id = report
        .run_id
        .as_deref()
        .ok_or_else(|| p7_provenance_error("P7 wall report has no run_id"))?;
    let trusted_dataset = p7_trusted_dataset(P7_MAXIMUM_RSS_SUITE)
        .ok_or_else(|| p7_provenance_error("trusted maximum RSS dataset is missing"))?;
    if preflight.schema_version != P7_RUNNER_PREFLIGHT_SCHEMA_VERSION
        || preflight.run_id != run_id
        || !is_sha256(&preflight.gate_attestation_sha256)
        || !is_sha256(&preflight.release_metadata_sha256)
        || !is_sha256(&preflight.gate_source_fingerprint)
        || preflight.gate_ids != P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
        || maximum_rss.run_id != run_id
        || maximum_rss.preflight != preflight
        || maximum_rss.schema_version != P7_MAXIMUM_RSS_EVIDENCE_SCHEMA_VERSION
        || !maximum_rss.completed
        || maximum_rss.suite != P7_MAXIMUM_RSS_SUITE
        || maximum_rss.dataset_file != trusted_dataset.file_name
        || maximum_rss.dataset_sha256 != trusted_dataset.input_sha256
        || maximum_rss.dataset_index != P7_MAXIMUM_RSS_DATASET_INDEX
        || maximum_rss.question_index != P7_MAXIMUM_RSS_QUESTION_INDEX
        || maximum_rss.rss_limit_bytes != P7_MAXIMUM_RSS_LIMIT_BYTES
        || maximum_rss.rss_gate_passed
            != (maximum_rss.maximum_rss_bytes <= maximum_rss.rss_limit_bytes)
        || maximum_rss.measurement_child_exit_status != 0
        || maximum_rss.measurement_elapsed_millis == 0
        || maximum_rss.supervisor_receipt.schema_version != "p7_sealed_process_receipt_v1"
        || maximum_rss.supervisor_receipt.maximum_rss_bytes != maximum_rss.maximum_rss_bytes
        || maximum_rss
            .supervisor_receipt
            .sealed_executable_sha256
            .as_deref()
            != Some(maximum_rss.measured_executable_sha256.as_str())
        || maximum_rss.measured_executable_canonical_path != preflight.executable_canonical_path
        || maximum_rss.measured_executable_sha256 != preflight.executable_sha256
        || ![
            maximum_rss.question_sha256.as_str(),
            maximum_rss.measurement_report_sha256.as_str(),
            maximum_rss.measured_executable_sha256.as_str(),
            maximum_rss.preflight_report_sha256.as_str(),
            maximum_rss.runner_stdout_sha256.as_str(),
            maximum_rss.runner_stderr_sha256.as_str(),
            maximum_rss.detail_sha256.as_str(),
            maximum_rss.summary_sha256.as_str(),
        ]
        .into_iter()
        .all(is_sha256)
        || !maximum_rss.preflight_validated_after_measurement
    {
        return Err(p7_provenance_error(
            "maximum RSS evidence is not bound to the P7 release cohort",
        ));
    }
    report.blocked_reasons.retain(|reason| {
        reason != "p7_runner_preflight_missing" && reason != "p7_maximum_rss_evidence_missing"
    });
    if !report.benchmark_gate_passed && report.blocked_reasons.is_empty() {
        report
            .blocked_reasons
            .push("p7_benchmark_gate_failed".to_string());
    }
    report.p7_preflight = Some(preflight);
    report.p7_maximum_rss_attached = true;
    report.p7_maximum_rss_within_limit = maximum_rss.rss_gate_passed;
    if !maximum_rss.rss_gate_passed {
        report
            .blocked_reasons
            .push("p7_maximum_rss_limit_exceeded".to_string());
    }
    report.p7_maximum_rss = Some(maximum_rss);
    if !report.p7_soul_regression_gate.passed {
        report
            .blocked_reasons
            .push("p7_soul_regression_gate_failed".to_string());
    }
    if report.verification_receipt.is_none() {
        report
            .blocked_reasons
            .push("p7_verification_receipt_missing".to_string());
    }
    if !report.verifier_performance.passed {
        report
            .blocked_reasons
            .push("p7_verifier_performance_gate_failed".to_string());
    }
    report.blocked_reasons.sort();
    report.blocked_reasons.dedup();
    report.release_gate_passed = report.benchmark_gate_passed && report.blocked_reasons.is_empty();
    Ok(report)
}

pub fn w4_external_noisy_summary_with_provenance(
    summary_json: &str,
) -> Result<W4ExternalNoisyBenchmarkSummary> {
    let mut summary = serde_json::from_str::<W4ExternalNoisyBenchmarkSummary>(summary_json)
        .map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "w4_external_noisy_summary_json",
        })?;
    summary.summary_sha256 = Some(format!("{:x}", Sha256::digest(summary_json.as_bytes())));
    summary.runner_source_sha256 = summary
        .p7_provenance
        .as_ref()
        .map(|provenance| provenance.runner_build_fingerprint.clone());
    summary.no_gold_questions = summary.questions.saturating_sub(summary.evidence_questions);
    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
fn verify_w4_external_noisy_summary(
    summary: &mut W4ExternalNoisyBenchmarkSummary,
    merged_summary_path: &Path,
    benchmark_root: &Path,
    parent: &Path,
    runner_preflight: &P7RunnerPreflightReport,
    runner_disk_identity: &P7RunnerDiskIdentity,
    admission: &P7CohortAdmission,
    admission_sha256: &str,
    session: &mut P7ArtifactReadSession,
) -> Result<()> {
    summary.operator_content_hash_verified = false;
    let Some(provenance) = summary.p7_provenance.as_ref() else {
        return Ok(());
    };
    if summary.run_id != provenance.run_id {
        return Err(p7_provenance_error(
            "merged summary and provenance run_id mismatch",
        ));
    }
    let expected_merged_name = format!("{}.merged.summary.json", summary.suite);
    if merged_summary_path
        .file_name()
        .and_then(|name| name.to_str())
        != Some(expected_merged_name.as_str())
    {
        return Err(p7_provenance_error("unexpected merged summary file name"));
    }
    let expected_parent = benchmark_root.join("results/runs").join(&summary.run_id);
    if parent != expected_parent || !parent.starts_with(benchmark_root) {
        return Err(p7_provenance_error(
            "merged summary escaped its canonical run cohort",
        ));
    }
    let trusted_dataset = p7_trusted_dataset(&summary.suite)
        .ok_or_else(|| p7_provenance_error("unknown release suite"))?;
    if provenance.schema_version != P7_MERGED_PROVENANCE_SCHEMA_VERSION
        || provenance.cohort_admission_sha256 != admission_sha256
        || admission.release != runner_preflight.published_release_identity()
    {
        return Err(p7_provenance_error(
            "merged provenance does not bind the verified cohort admission",
        ));
    }
    validate_p7_runner_disk_provenance(provenance, runner_disk_identity)?;
    validate_p7_release_identity_against_disk(provenance, trusted_dataset, runner_disk_identity)?;
    let expectation = w4_external_suite_expectation(&summary.suite)
        .ok_or_else(|| p7_provenance_error("unknown release suite"))?;
    if expectation.shard_count != summary.shards.len()
        || provenance.ordered_shard_digest_manifest.len() != summary.shards.len()
    {
        return Err(p7_provenance_error("release shard count mismatch"));
    }
    let data_dir = benchmark_root.join("data");
    p7_require_canonical_real_directory(&data_dir)?;
    let dataset_path = data_dir.join(trusted_dataset.file_name);
    let (expected_dataset, dataset_sha256, _) = session.read_with(
        &dataset_path,
        &data_dir,
        benchmark_root,
        Some(trusted_dataset.input_sha256),
        P7ArtifactReadKind::Dataset,
        |reader, _| load_p7_dataset_expectation(reader, trusted_dataset, expectation.shard_count),
    )?;
    if expected_dataset.input_sha256 != provenance.input_sha256
        || expected_dataset.input_sha256 != dataset_sha256
    {
        return Err(p7_provenance_error("input dataset digest mismatch"));
    }

    let mut merged_detail_hasher = Sha256::new();
    let mut additive_aggregate = serde_json::Map::new();
    let mut recomputed_aggregate = P7DetailAggregate::default();
    let mut seen_question_ids = BTreeSet::new();
    let mut seen_identities = BTreeSet::new();
    for (shard_index, (shard_name, digest)) in summary
        .shards
        .iter()
        .zip(&provenance.ordered_shard_digest_manifest)
        .enumerate()
    {
        let expected_shard_name = format!(
            "{}.shard-{shard_index}-of-{}.summary.json",
            summary.suite, expectation.shard_count
        );
        let shard_path_fragment = Path::new(&digest.shard);
        if shard_name != &expected_shard_name
            || shard_name != &digest.shard
            || digest.run_id != summary.run_id
            || shard_path_fragment
                .file_name()
                .and_then(|name| name.to_str())
                != Some(digest.shard.as_str())
        {
            return Err(p7_provenance_error("unsafe or mismatched shard path"));
        }
        let shard_path = parent.join(shard_path_fragment);
        let (shard_json, _) = session.read_json::<serde_json::Value>(
            &shard_path,
            parent,
            benchmark_root,
            Some(&digest.summary_sha256),
            P7ArtifactReadKind::Summary,
            "p7_provenance_parse_shard_summary",
        )?;
        if shard_json.get("suite").and_then(serde_json::Value::as_str)
            != Some(summary.suite.as_str())
            || shard_json
                .get("shard_index")
                .and_then(serde_json::Value::as_u64)
                != Some(shard_index as u64)
            || shard_json
                .get("shard_total")
                .and_then(serde_json::Value::as_u64)
                != Some(expectation.shard_count as u64)
            || shard_json
                .get("completed")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            || shard_json.get("run_id").and_then(serde_json::Value::as_str)
                != Some(summary.run_id.as_str())
        {
            return Err(p7_provenance_error("shard release coordinates mismatch"));
        }
        validate_p7_release_shard_full_run(&shard_json)?;
        if shard_json
            .get("input_sha256")
            .and_then(serde_json::Value::as_str)
            != Some(trusted_dataset.input_sha256)
        {
            return Err(p7_provenance_error("shard input dataset digest mismatch"));
        }
        accumulate_p7_shard_summary(&mut additive_aggregate, &shard_json)?;
        let producer_value = shard_json
            .get("producer")
            .ok_or_else(|| p7_provenance_error("shard summary is missing producer provenance"))?;
        let producer = p7_parse_recorded_shard_producer(producer_value)?;
        if producer.schema_version != P7_SHARD_PRODUCER_PROVENANCE_SCHEMA_VERSION
            || producer.execution_kind != P7ProducerExecutionKind::CohortShard
            || producer.run_id != summary.run_id
            || producer.contract_version != provenance.contract_version
            || producer.sdk_report_schema_version != provenance.sdk_report_schema_version
            || producer.sdk_build_fingerprint != provenance.sdk_build_fingerprint
            || producer.runner_build_fingerprint != provenance.runner_build_fingerprint
            || producer.runner_lock_fingerprint != provenance.runner_lock_fingerprint
            || producer.executable_sha256 != provenance.executable_sha256
            || producer.build_profile != provenance.build_profile
            || producer.gate_attestation_sha256 != provenance.gate_attestation_sha256
            || producer.release_metadata_sha256 != provenance.release_metadata_sha256
            || producer.gate_source_fingerprint != provenance.gate_source_fingerprint
            || producer.gate_source_manifest_sha256 != provenance.gate_source_manifest_sha256
            || producer.gate_ids != provenance.gate_ids
            || producer.cohort_admission_sha256 != provenance.cohort_admission_sha256
            || producer.input_sha256 != provenance.input_sha256
            || producer.detail_schema_version != provenance.detail_schema_version
            || producer.detail_sha256 != digest.detail_sha256
        {
            return Err(p7_provenance_error("shard producer provenance mismatch"));
        }

        let detail_name = digest
            .shard
            .strip_suffix(".summary.json")
            .map(|prefix| format!("{prefix}.jsonl"))
            .ok_or_else(|| p7_provenance_error("invalid shard summary file name"))?;
        let detail_path = parent.join(detail_name);
        let (shard_recomputed, detail_sha256, _) = session.read_with(
            &detail_path,
            parent,
            benchmark_root,
            Some(&digest.detail_sha256),
            P7ArtifactReadKind::Detail,
            |reader, _| {
                validate_p7_detail_file(
                    reader,
                    &digest.detail_sha256,
                    P7DetailValidationContext {
                        suite: &summary.suite,
                        run_id: &summary.run_id,
                        detail_schema_version: &producer.detail_schema_version,
                        expected_questions: &expected_dataset.questions_by_shard[shard_index],
                        expected_samples: expected_dataset.samples_by_shard[shard_index],
                    },
                    &mut seen_question_ids,
                    &mut seen_identities,
                )
            },
        )?;
        if detail_sha256 != digest.detail_sha256 {
            return Err(p7_provenance_error("P7 detail digest mismatch"));
        }
        validate_p7_shard_against_detail(&shard_json, &shard_recomputed)?;
        recomputed_aggregate.add_assign(&shard_recomputed)?;
        merged_detail_hasher.update(digest.detail_sha256.as_bytes());
        merged_detail_hasher.update([0]);
    }
    if format!("{:x}", merged_detail_hasher.finalize()) != provenance.merged_detail_sha256 {
        return Err(p7_provenance_error("merged detail digest mismatch"));
    }
    validate_p7_additive_merge(summary, &additive_aggregate)?;
    summary.no_gold_questions = recomputed_aggregate
        .questions
        .saturating_sub(recomputed_aggregate.evidence_questions);
    summary.facet_ablation = Some(recomputed_aggregate.facet_ablation.clone());
    validate_p7_summary_against_detail(summary, &recomputed_aggregate)?;
    summary.producer_identity_digest = summary.p7_provenance.as_ref().and_then(|provenance| {
        provenance
            .producer_identity
            .parse::<P7ProducerIdentity>()
            .ok()
            .map(|_| {
                provenance
                    .producer_identity
                    .canonical_identity_sha256
                    .clone()
            })
    });
    summary.operator_content_hash_verified = true;
    Ok(())
}

pub struct P7VerifiedWallInputContext {
    summaries: Vec<W4ExternalNoisyBenchmarkSummary>,
    preflight: P7RunnerPreflightReport,
    maximum_rss: P7MaximumRssEvidence,
    verifier_identity: P7VerifierIdentity,
    session: P7ArtifactReadSession,
    execution_authority: P7VerifierExecutionAuthority,
}

impl P7VerifiedWallInputContext {
    pub fn summaries(&self) -> &[W4ExternalNoisyBenchmarkSummary] {
        &self.summaries
    }

    pub fn verifier_identity(&self) -> &P7VerifierIdentity {
        &self.verifier_identity
    }

    pub fn release_inputs(
        &self,
        elapsed: Duration,
    ) -> (
        P7RunnerPreflightReport,
        P7MaximumRssEvidence,
        P7VerifierPerformanceReport,
    ) {
        (
            self.preflight.clone(),
            self.maximum_rss.clone(),
            self.session.performance(elapsed),
        )
    }

    pub fn verify_before_external_write(&mut self) -> Result<()> {
        self.execution_authority.verify_retained()?;
        self.session.verify_retained()
    }

    pub fn open_cohort<'authority>(
        &'authority mut self,
        root: &Path,
        run_id: &str,
    ) -> Result<crate::p7_secure_fs::P7AuthorityBoundArtifactTransaction<'authority>> {
        self.verify_before_external_write()?;
        self.execution_authority.open_cohort(root, run_id)
    }
}

pub fn verify_p7_wall_input_context(
    summary_paths: &[PathBuf],
    preflight_report_path: &Path,
) -> Result<P7VerifiedWallInputContext> {
    let authority = attest_p7_current_verifier_execution()?;
    verify_p7_wall_input_context_with_authority(summary_paths, preflight_report_path, authority)
}

pub fn verify_p7_wall_input_context_with_authority(
    summary_paths: &[PathBuf],
    preflight_report_path: &Path,
    mut execution_authority: P7VerifierExecutionAuthority,
) -> Result<P7VerifiedWallInputContext> {
    if summary_paths.is_empty() {
        return Err(p7_provenance_error(
            "P7 wall requires at least one merged summary",
        ));
    }
    let cohort_dir = preflight_report_path
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 preflight report has no cohort owner"))?;
    p7_require_canonical_real_directory(cohort_dir)?;
    if preflight_report_path != cohort_dir.join("preflight-report.json") {
        return Err(p7_provenance_error(
            "P7 preflight path differs from the fixed cohort contract",
        ));
    }
    let runs_dir = cohort_dir
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 cohort has no runs owner"))?;
    let results_dir = runs_dir
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 cohort has no results owner"))?;
    let benchmark_root = results_dir
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 results owner has no benchmark root"))?;
    if runs_dir.file_name().and_then(|name| name.to_str()) != Some("runs")
        || results_dir.file_name().and_then(|name| name.to_str()) != Some("results")
    {
        return Err(p7_provenance_error(
            "P7 cohort is not under results/runs/<run-id>",
        ));
    }
    p7_require_canonical_real_directory(benchmark_root)?;
    execution_authority.verify_retained()?;
    let verifier_identity = execution_authority.identity().clone();

    let mut session = P7ArtifactReadSession::default();
    let mut summaries = Vec::with_capacity(summary_paths.len());
    for path in summary_paths {
        if path.parent() != Some(cohort_dir) {
            return Err(p7_provenance_error(
                "all merged summaries must share the preflight cohort owner",
            ));
        }
        let (mut summary, summary_sha256) = session.read_json::<W4ExternalNoisyBenchmarkSummary>(
            path,
            cohort_dir,
            benchmark_root,
            None,
            P7ArtifactReadKind::Summary,
            "p7_wall_parse_merged_summary",
        )?;
        summary.summary_sha256 = Some(summary_sha256);
        summary.runner_source_sha256 = summary
            .p7_provenance
            .as_ref()
            .map(|provenance| provenance.runner_build_fingerprint.clone());
        summary.no_gold_questions = summary.questions.saturating_sub(summary.evidence_questions);
        summaries.push(summary);
    }
    let run_id = summaries
        .first()
        .map(|summary| summary.run_id.clone())
        .ok_or_else(|| p7_provenance_error("P7 wall has no run_id"))?;
    if !p7_valid_run_id(&run_id)
        || cohort_dir.file_name().and_then(|name| name.to_str()) != Some(run_id.as_str())
        || summaries.iter().any(|summary| summary.run_id != run_id)
    {
        return Err(p7_provenance_error("P7 wall cohort run_id mismatch"));
    }

    let maximum_rss_path = cohort_dir.join(P7_MAXIMUM_RSS_REPORT_FILE_NAME);
    let cohort_evidence = verify_p7_wall_cohort_evidence_in_session(
        benchmark_root,
        cohort_dir,
        &run_id,
        preflight_report_path,
        &maximum_rss_path,
        &mut session,
    )?;
    if !cohort_evidence.maximum_rss.rss_gate_passed {
        return Err(p7_provenance_error(
            "P7 wall maximum RSS evidence is not admitted for this cohort",
        ));
    }

    let admission_path = cohort_dir.join(P7_COHORT_ADMISSION_FILE_NAME);
    let (admission, admission_sha256) = session.read_json::<P7CohortAdmission>(
        &admission_path,
        cohort_dir,
        benchmark_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_wall_parse_cohort_admission",
    )?;
    validate_p7_cohort_admission_contract(
        &admission,
        &run_id,
        &cohort_evidence.preflight_sha256,
        &cohort_evidence.maximum_rss_sha256,
        &cohort_evidence.preflight.published_release_identity(),
    )?;

    for (path, summary) in summary_paths.iter().zip(&mut summaries) {
        verify_w4_external_noisy_summary(
            summary,
            path,
            benchmark_root,
            cohort_dir,
            &cohort_evidence.preflight,
            &cohort_evidence.runner_disk_identity,
            &admission,
            &admission_sha256,
            &mut session,
        )?;
    }

    Ok(P7VerifiedWallInputContext {
        summaries,
        preflight: cohort_evidence.preflight,
        maximum_rss: cohort_evidence.maximum_rss,
        verifier_identity,
        session,
        execution_authority,
    })
}

fn validate_p7_release_identity_against_disk(
    provenance: &P7MergedProvenance,
    dataset: P7TrustedDataset,
    runner: &P7RunnerDiskIdentity,
) -> Result<()> {
    let recorded = provenance.producer_identity.parse::<P7ProducerIdentity>()?;
    if provenance.schema_version != P7_MERGED_PROVENANCE_SCHEMA_VERSION
        || provenance.contract_version != P7_CONTRACT_VERSION
        || provenance.sdk_report_schema_version != MEMORY_RECALL_DELIVERY_SCHEMA_VERSION
        || provenance.sdk_build_fingerprint != P7_TRUSTED_SDK_BUILD_FINGERPRINT
        || provenance.runner_build_fingerprint != runner.runner_build_fingerprint
        || provenance.runner_lock_fingerprint != runner.runner_lock_fingerprint
        || provenance.executable_sha256 != runner.executable_sha256
        || provenance.gate_attestation_sha256 != runner.gate_attestation_sha256
        || provenance.release_metadata_sha256 != runner.release_metadata_sha256
        || provenance.gate_source_fingerprint != runner.gate_source_fingerprint
        || provenance.gate_source_manifest_sha256 != runner.gate_source_manifest_sha256
        || provenance.gate_ids != P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
        || !is_sha256(&runner.executable_sha256)
        || !is_sha256(&provenance.gate_attestation_sha256)
        || !is_sha256(&provenance.gate_source_fingerprint)
        || !is_sha256(&provenance.release_metadata_sha256)
        || !is_sha256(&provenance.gate_source_manifest_sha256)
        || !is_sha256(&provenance.cohort_admission_sha256)
        || provenance.build_profile != "release"
        || provenance.input_sha256 != dataset.input_sha256
        || !p7_detail_schema_supported(&provenance.detail_schema_version)
        || !is_sha256(&provenance.merged_detail_sha256)
        || recorded.schema_version != P7_PRODUCER_IDENTITY_SCHEMA_VERSION
        || recorded.contract_version != provenance.contract_version
        || recorded.sdk_report_schema_version != provenance.sdk_report_schema_version
        || recorded.sdk_build_fingerprint != provenance.sdk_build_fingerprint
        || recorded.runner_build_fingerprint != provenance.runner_build_fingerprint
        || recorded.runner_lock_fingerprint != provenance.runner_lock_fingerprint
        || recorded.executable_sha256 != provenance.executable_sha256
        || recorded.build_profile != provenance.build_profile
        || recorded.input_sha256 != provenance.input_sha256
        || recorded.detail_schema_version != provenance.detail_schema_version
    {
        return Err(p7_provenance_error("untrusted P7 release provenance"));
    }
    Ok(())
}

fn validate_p7_runner_disk_provenance(
    provenance: &P7MergedProvenance,
    disk: &P7RunnerDiskIdentity,
) -> Result<()> {
    if provenance.runner_build_fingerprint != disk.runner_build_fingerprint
        || provenance.runner_lock_fingerprint != disk.runner_lock_fingerprint
        || provenance.executable_sha256 != disk.executable_sha256
        || provenance.gate_attestation_sha256 != disk.gate_attestation_sha256
        || provenance.release_metadata_sha256 != disk.release_metadata_sha256
        || provenance.gate_source_fingerprint != disk.gate_source_fingerprint
        || provenance.gate_source_manifest_sha256 != disk.gate_source_manifest_sha256
        || provenance.gate_ids != disk.gate_ids
        || !is_sha256(&disk.runner_build_fingerprint)
        || !is_sha256(&disk.runner_lock_fingerprint)
        || !is_sha256(&disk.executable_sha256)
        || !is_sha256(&disk.gate_attestation_sha256)
        || !is_sha256(&disk.release_metadata_sha256)
        || !is_sha256(&disk.gate_source_fingerprint)
        || !is_sha256(&disk.gate_source_manifest_sha256)
        || disk.gate_ids != P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
    {
        return Err(p7_provenance_error(
            "runner source, executable, or governed release differs from producer provenance",
        ));
    }
    Ok(())
}

fn validate_p7_frozen_release_binding(
    frozen: P7FrozenRunnerIdentity,
    disk: &P7RunnerDiskIdentity,
    fresh_gate_source_fingerprint: &str,
) -> Result<()> {
    if !is_sha256(frozen.runner_build_fingerprint)
        || !is_sha256(frozen.runner_lock_fingerprint)
        || !is_sha256(frozen.executable_sha256)
        || !is_sha256(frozen.gate_attestation_sha256)
        || !is_sha256(frozen.release_metadata_sha256)
        || !is_sha256(frozen.gate_source_fingerprint)
        || !is_sha256(frozen.gate_source_manifest_sha256)
        || !is_sha256(fresh_gate_source_fingerprint)
        || disk.runner_build_fingerprint != frozen.runner_build_fingerprint
        || disk.runner_lock_fingerprint != frozen.runner_lock_fingerprint
        || disk.executable_sha256 != frozen.executable_sha256
        || disk.gate_attestation_sha256 != frozen.gate_attestation_sha256
        || disk.release_metadata_sha256 != frozen.release_metadata_sha256
        || disk.gate_source_fingerprint != frozen.gate_source_fingerprint
        || disk.gate_source_manifest_sha256 != frozen.gate_source_manifest_sha256
        || fresh_gate_source_fingerprint != frozen.gate_source_fingerprint
        || disk.gate_ids != P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
    {
        return Err(p7_preflight_error(
            "P7 frozen runner, gate attestation, or fresh gate source identity drifted",
        ));
    }
    Ok(())
}

fn validate_p7_release_shard_full_run(shard: &serde_json::Value) -> Result<()> {
    for field in ["limit", "question_limit", "question_index"] {
        if !shard.get(field).is_some_and(serde_json::Value::is_null) {
            return Err(p7_provenance_error(
                "release shard contains a diagnostic question filter",
            ));
        }
    }
    Ok(())
}

pub fn validate_p7_runner_preflight_report(
    benchmark_root: &Path,
    run_id: &str,
    report: &P7RunnerPreflightReport,
) -> Result<()> {
    if report.run_id != run_id {
        return Err(p7_preflight_error(
            "P7 preflight report run_id differs from cohort",
        ));
    }
    validate_p7_runner_preflight_report_against_disk(benchmark_root, run_id, report)
}

pub fn verify_p7_published_release_bundle_with_receipt(
    benchmark_root: &Path,
    sdk_root: &Path,
    runner_source_root: &Path,
    executable_sha256: &str,
) -> Result<(P7VerifiedPublishedReleaseBundle, P7ArtifactLifecycleReceipt)> {
    let canonical_root = fs::canonicalize(benchmark_root).map_err(|source| Error::Io {
        source,
        stage: "p7_release_bundle_canonicalize_benchmark_root",
    })?;
    let canonical_sdk_root = fs::canonicalize(sdk_root).map_err(|source| Error::Io {
        source,
        stage: "p7_release_bundle_canonicalize_sdk_root",
    })?;
    let canonical_runner_source_root =
        fs::canonicalize(runner_source_root).map_err(|source| Error::Io {
            source,
            stage: "p7_release_bundle_canonicalize_runner_source_root",
        })?;
    if canonical_root != benchmark_root
        || canonical_sdk_root != sdk_root
        || canonical_runner_source_root != runner_source_root
    {
        return Err(p7_preflight_error(
            "P7 release verifier roots must be canonical",
        ));
    }
    let mut session = P7ArtifactReadSession::default();
    let source = p7_release_gate_source_material_in_session(
        &canonical_sdk_root,
        &canonical_runner_source_root,
        &mut session,
        true,
    )?;
    let disk = p7_runner_disk_identity_for_release_sha_in_session(
        &canonical_root,
        executable_sha256,
        &canonical_sdk_root,
        &canonical_runner_source_root,
        &source,
        &mut session,
    )?;
    session.verify_retained()?;
    let verified = P7VerifiedPublishedReleaseBundle {
        identity: P7PublishedReleaseIdentity {
            sdk_build_fingerprint: source.sdk_build_fingerprint,
            runner_build_fingerprint: disk.runner_build_fingerprint,
            runner_lock_fingerprint: disk.runner_lock_fingerprint,
            executable_sha256: disk.executable_sha256,
            build_profile: "release".to_string(),
            gate_attestation_sha256: disk.gate_attestation_sha256,
            release_metadata_sha256: disk.release_metadata_sha256,
            gate_source_fingerprint: disk.gate_source_fingerprint,
            gate_source_manifest_sha256: disk.gate_source_manifest_sha256,
            gate_ids: disk.gate_ids,
        },
        executable_canonical_path: disk.executable_canonical_path,
    };
    let receipt = session.lifecycle_receipt("published_release_bundle");
    Ok((verified, receipt))
}

pub fn verify_p7_preflight_artifact_with_receipt(
    benchmark_root: &Path,
    run_id: &str,
) -> Result<(P7VerifiedPreflightArtifact, P7ArtifactLifecycleReceipt)> {
    if !p7_valid_run_id(run_id) {
        return Err(p7_preflight_error("P7 preflight run_id is invalid"));
    }
    let canonical_root = fs::canonicalize(benchmark_root).map_err(|source| Error::Io {
        source,
        stage: "p7_preflight_artifact_canonicalize_root",
    })?;
    if canonical_root != benchmark_root {
        return Err(p7_preflight_error(
            "P7 preflight benchmark root must be canonical",
        ));
    }
    let cohort_dir = canonical_root.join("results/runs").join(run_id);
    let preflight_path = cohort_dir.join("preflight-report.json");
    let mut session = P7ArtifactReadSession::default();
    let (report, sha256) = session.read_json::<P7RunnerPreflightReport>(
        &preflight_path,
        &cohort_dir,
        &canonical_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_preflight_artifact_parse",
    )?;
    validate_p7_producer_preflight_header(run_id, &report)?;
    let disk = p7_runner_producer_disk_identity_with_reads(&canonical_root, &report, &mut session)?;
    validate_p7_preflight_against_disk(&report, &disk)?;
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("preflight_artifact");
    Ok((P7VerifiedPreflightArtifact { report, sha256 }, receipt))
}

fn validate_p7_producer_preflight_header(
    run_id: &str,
    report: &P7RunnerPreflightReport,
) -> Result<()> {
    if !p7_valid_run_id(run_id)
        || report.schema_version != P7_RUNNER_PREFLIGHT_SCHEMA_VERSION
        || report.run_id != run_id
        || report.build_profile != "release"
        || report.gate_ids != P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
        || ![
            report.sdk_build_fingerprint.as_str(),
            report.runner_build_fingerprint.as_str(),
            report.runner_lock_fingerprint.as_str(),
            report.executable_sha256.as_str(),
            report.gate_attestation_sha256.as_str(),
            report.release_metadata_sha256.as_str(),
            report.gate_source_fingerprint.as_str(),
            report.gate_source_manifest_sha256.as_str(),
        ]
        .into_iter()
        .all(is_sha256)
    {
        return Err(p7_preflight_error(
            "P7 producer preflight report header is invalid",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[cfg(target_os = "linux")]
fn validate_p7_producer_preflight_report(
    benchmark_root: &Path,
    run_id: &str,
    report: &P7RunnerPreflightReport,
) -> Result<()> {
    validate_p7_producer_preflight_header(run_id, report)?;
    let mut session = P7ArtifactReadSession::default();
    let disk = p7_runner_producer_disk_identity_with_reads(benchmark_root, report, &mut session)?;
    validate_p7_preflight_against_disk(report, &disk)?;
    session.verify_retained()
}

fn validate_p7_preflight_against_disk(
    report: &P7RunnerPreflightReport,
    disk: &P7RunnerDiskIdentity,
) -> Result<()> {
    if report.runner_build_fingerprint != disk.runner_build_fingerprint
        || report.runner_lock_fingerprint != disk.runner_lock_fingerprint
        || report.executable_sha256 != disk.executable_sha256
        || report.executable_canonical_path != disk.executable_canonical_path.to_string_lossy()
        || report.gate_attestation_sha256 != disk.gate_attestation_sha256
        || report.release_metadata_sha256 != disk.release_metadata_sha256
        || report.gate_source_fingerprint != disk.gate_source_fingerprint
        || report.gate_source_manifest_sha256 != disk.gate_source_manifest_sha256
        || report.gate_ids != disk.gate_ids
    {
        return Err(p7_preflight_error(
            "P7 producer preflight differs from its immutable release bundle",
        ));
    }
    let output = p7_run_retained_executable(
        &disk.executable_canonical_path,
        &["--print-build-identity"],
        "p7_producer_execute_identity",
    )?;
    if !output.status.success() {
        return Err(p7_preflight_error(
            "P7 producer runner rejected --print-build-identity",
        ));
    }
    let embedded =
        serde_json::from_slice::<P7RunnerBuildIdentity>(&output.stdout).map_err(|source| {
            Error::Other {
                source: Box::new(source),
                stage: "p7_producer_parse_identity",
            }
        })?;
    if embedded.sdk_build_fingerprint != report.sdk_build_fingerprint
        || embedded.runner_build_fingerprint != report.runner_build_fingerprint
        || embedded.runner_lock_fingerprint != report.runner_lock_fingerprint
        || embedded.executable_sha256 != report.executable_sha256
        || embedded.build_profile != report.build_profile
    {
        return Err(p7_preflight_error(
            "P7 producer embedded identity differs from its preflight report",
        ));
    }
    Ok(())
}

pub fn validate_p7_runner_preflight_report_with_frozen(
    benchmark_root: &Path,
    run_id: &str,
    report: &P7RunnerPreflightReport,
    frozen: P7FrozenRunnerIdentity,
) -> Result<()> {
    let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| p7_preflight_error("bm-replay is not under the SDK workspace root"))?;
    let fresh = preflight_p7_runner_release_with_frozen(benchmark_root, sdk_root, frozen, run_id)?;
    if report != &fresh {
        return Err(p7_preflight_error(
            "P7 preflight report differs from current frozen producer release preflight",
        ));
    }
    Ok(())
}

fn validate_p7_runner_preflight_report_against_disk(
    benchmark_root: &Path,
    run_id: &str,
    report: &P7RunnerPreflightReport,
) -> Result<()> {
    if !p7_valid_run_id(run_id)
        || report.schema_version != P7_RUNNER_PREFLIGHT_SCHEMA_VERSION
        || report.run_id != run_id
        || report.build_profile != "release"
        || report.gate_ids != P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
        || ![
            report.sdk_build_fingerprint.as_str(),
            report.runner_build_fingerprint.as_str(),
            report.runner_lock_fingerprint.as_str(),
            report.executable_sha256.as_str(),
            report.gate_attestation_sha256.as_str(),
            report.release_metadata_sha256.as_str(),
            report.gate_source_fingerprint.as_str(),
        ]
        .into_iter()
        .all(is_sha256)
    {
        return Err(p7_preflight_error(
            "P7 preflight report header or governed identity is invalid",
        ));
    }
    p7_require_canonical_real_directory(benchmark_root)?;
    let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| p7_preflight_error("bm-replay is not under the SDK workspace root"))?;
    let canonical_sdk_root = fs::canonicalize(sdk_root).map_err(|source| Error::Io {
        source,
        stage: "p7_runner_preflight_canonicalize_sdk_root",
    })?;
    p7_require_canonical_real_directory(&canonical_sdk_root)?;
    let runner_root = benchmark_root.join("runner");
    p7_require_canonical_real_directory(&runner_root)?;

    let mut session = P7ArtifactReadSession::default();
    let source = p7_release_gate_source_material_in_session(
        &canonical_sdk_root,
        &runner_root,
        &mut session,
        true,
    )?;
    let disk = p7_runner_disk_identity_for_release_sha_in_session(
        benchmark_root,
        &report.executable_sha256,
        &canonical_sdk_root,
        &runner_root,
        &source,
        &mut session,
    )?;
    if report.sdk_build_fingerprint != source.sdk_build_fingerprint
        || report.sdk_build_fingerprint != P7_TRUSTED_SDK_BUILD_FINGERPRINT
        || report.runner_build_fingerprint != disk.runner_build_fingerprint
        || report.runner_lock_fingerprint != disk.runner_lock_fingerprint
        || report.executable_sha256 != disk.executable_sha256
        || report.executable_canonical_path != disk.executable_canonical_path.to_string_lossy()
        || report.gate_attestation_sha256 != disk.gate_attestation_sha256
        || report.release_metadata_sha256 != disk.release_metadata_sha256
        || report.gate_source_fingerprint != disk.gate_source_fingerprint
        || report.gate_source_fingerprint != source.manifest.source_fingerprint
        || report.gate_ids != disk.gate_ids
    {
        return Err(p7_preflight_error(
            "P7 preflight report differs from the current governed release on disk",
        ));
    }

    let output = p7_run_retained_executable(
        &disk.executable_canonical_path,
        &["--print-build-identity"],
        "p7_runner_preflight_execute_identity",
    )?;
    if !output.status.success() {
        return Err(p7_preflight_error(
            "governed runner rejected --print-build-identity",
        ));
    }
    let embedded =
        serde_json::from_slice::<P7RunnerBuildIdentity>(&output.stdout).map_err(|source| {
            Error::Other {
                source: Box::new(source),
                stage: "p7_runner_preflight_parse_identity",
            }
        })?;
    if embedded.sdk_build_fingerprint != report.sdk_build_fingerprint
        || embedded.runner_build_fingerprint != report.runner_build_fingerprint
        || embedded.runner_lock_fingerprint != report.runner_lock_fingerprint
        || embedded.executable_sha256 != report.executable_sha256
        || embedded.build_profile != report.build_profile
    {
        return Err(p7_preflight_error(
            "P7 preflight report differs from the governed runner embedded identity",
        ));
    }
    session.verify_retained()?;
    Ok(())
}

pub fn verify_p7_maximum_rss_evidence(
    benchmark_root: &Path,
    run_id: &str,
) -> Result<P7MaximumRssEvidence> {
    Ok(verify_p7_maximum_rss_evidence_with_receipt(benchmark_root, run_id)?.0)
}

pub fn verify_p7_maximum_rss_evidence_with_receipt(
    benchmark_root: &Path,
    run_id: &str,
) -> Result<(P7MaximumRssEvidence, P7ArtifactLifecycleReceipt)> {
    let mut session = P7ArtifactReadSession::default();
    let verified =
        verify_p7_maximum_rss_evidence_in_session(benchmark_root, run_id, &mut session, None)?;
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("maximum_rss");
    Ok((verified.evidence, receipt))
}

struct P7MaximumRssVerifiedMaterial {
    evidence: P7MaximumRssEvidence,
    runner_disk_identity: P7RunnerDiskIdentity,
}

fn verify_p7_maximum_rss_evidence_in_session(
    benchmark_root: &Path,
    run_id: &str,
    session: &mut P7ArtifactReadSession,
    preflight_material: Option<(P7RunnerPreflightReport, String)>,
) -> Result<P7MaximumRssVerifiedMaterial> {
    if !p7_valid_run_id(run_id) {
        return Err(p7_provenance_error("invalid or missing P7 RSS run_id"));
    }
    let canonical_root = fs::canonicalize(benchmark_root).map_err(|source| Error::Io {
        source,
        stage: "p7_maximum_rss_canonicalize_benchmark_root",
    })?;
    if canonical_root != benchmark_root {
        return Err(p7_provenance_error(
            "P7 RSS benchmark root must be canonical",
        ));
    }
    p7_require_canonical_real_directory(&canonical_root)?;
    let cohort_dir = canonical_root.join("results/runs").join(run_id);
    let canonical_cohort = fs::canonicalize(&cohort_dir).map_err(|source| Error::Io {
        source,
        stage: "p7_maximum_rss_canonicalize_cohort",
    })?;
    if canonical_cohort != cohort_dir
        || canonical_cohort != canonical_root.join("results/runs").join(run_id)
        || !canonical_cohort.starts_with(&canonical_root)
    {
        return Err(p7_provenance_error(
            "P7 RSS cohort must not traverse symlinks",
        ));
    }
    p7_require_canonical_real_directory(&canonical_cohort)?;

    let dataset = p7_trusted_dataset(P7_MAXIMUM_RSS_SUITE)
        .ok_or_else(|| p7_provenance_error("trusted maximum RSS dataset is missing"))?;
    let data_dir = canonical_root.join("data");
    p7_require_canonical_real_directory(&data_dir)?;
    let dataset_path = data_dir.join(dataset.file_name);
    let preflight_path = cohort_dir.join("preflight-report.json");
    let measurement_path = cohort_dir.join(P7_MAXIMUM_RSS_MEASUREMENT_FILE_NAME);
    let runner_stdout_path = cohort_dir.join("runner.stdout.log");
    let runner_stderr_path = cohort_dir.join("runner.stderr.log");
    let detail_path = cohort_dir.join(format!("{P7_MAXIMUM_RSS_ARTIFACT_STEM}.jsonl"));
    let summary_path = cohort_dir.join(format!("{P7_MAXIMUM_RSS_ARTIFACT_STEM}.summary.json"));
    let (preflight, preflight_sha256) = match preflight_material {
        Some(material) => material,
        None => session.read_json::<P7RunnerPreflightReport>(
            &preflight_path,
            &cohort_dir,
            &canonical_root,
            None,
            P7ArtifactReadKind::Control,
            "p7_maximum_rss_parse_preflight",
        )?,
    };
    if preflight.run_id != run_id {
        return Err(p7_provenance_error(
            "P7 RSS preflight run_id differs from cohort",
        ));
    }
    let (measurement, measurement_sha256) = session.read_json::<P7MaximumRssMeasurementReport>(
        &measurement_path,
        &cohort_dir,
        &canonical_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_maximum_rss_parse_measurement",
    )?;
    validate_p7_maximum_rss_measurement_contract(
        &measurement,
        &canonical_root,
        run_id,
        &preflight,
    )?;
    if measurement.runner_stdout.byte_len > P7_MAX_RSS_STDOUT_BYTES
        || measurement.runner_stderr.byte_len > P7_MAX_RSS_STDERR_BYTES
    {
        return Err(p7_provenance_error(
            "P7 RSS stdout or stderr exceeds its hard read budget",
        ));
    }

    let (expected_dataset, dataset_sha256, input_bytes) = session.read_with(
        &dataset_path,
        &data_dir,
        &canonical_root,
        Some(dataset.input_sha256),
        P7ArtifactReadKind::Dataset,
        |reader, _| load_p7_dataset_expectation(reader, dataset, 1),
    )?;
    if expected_dataset.input_sha256 != dataset_sha256 {
        return Err(p7_provenance_error(
            "trusted RSS dataset changed while being consumed",
        ));
    }
    let expected_question = expected_dataset
        .questions_by_shard
        .first()
        .and_then(|questions| questions.first())
        .cloned()
        .ok_or_else(|| p7_provenance_error("trusted RSS question is missing"))?;
    if expected_question.dataset_index != P7_MAXIMUM_RSS_DATASET_INDEX
        || expected_question.question_index != P7_MAXIMUM_RSS_QUESTION_INDEX
    {
        return Err(p7_provenance_error(
            "trusted RSS question coordinates drifted",
        ));
    }

    let (summary, summary_sha256) = session.read_json::<serde_json::Value>(
        &summary_path,
        &cohort_dir,
        &canonical_root,
        None,
        P7ArtifactReadKind::Summary,
        "p7_maximum_rss_parse_summary",
    )?;
    let producer = p7_parse_recorded_shard_producer(
        summary
            .get("producer")
            .ok_or_else(|| p7_provenance_error("P7 RSS summary producer is missing"))?,
    )?;

    let expected_detail_sha256 = producer.detail_sha256.clone();
    let (detail_aggregate, detail_sha256, _) = session.read_with(
        &detail_path,
        &cohort_dir,
        &canonical_root,
        Some(&expected_detail_sha256),
        P7ArtifactReadKind::Detail,
        |reader, _| {
            validate_p7_detail_file(
                reader,
                &expected_detail_sha256,
                P7DetailValidationContext {
                    suite: P7_MAXIMUM_RSS_SUITE,
                    run_id,
                    detail_schema_version: &producer.detail_schema_version,
                    expected_questions: std::slice::from_ref(&expected_question),
                    expected_samples: 1,
                },
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
            )
        },
    )?;

    validate_p7_maximum_rss_summary_contract(
        &summary,
        run_id,
        dataset,
        &dataset_path,
        input_bytes,
        &detail_path,
        &summary_path,
        &detail_sha256,
        &preflight,
    )?;
    validate_p7_shard_against_detail(&summary, &detail_aggregate)?;

    let (runner_stdout_sha256, runner_stdout_bytes) = session.read_raw(
        &runner_stdout_path,
        &cohort_dir,
        &canonical_root,
        Some(&measurement.runner_stdout.sha256),
        P7ArtifactReadKind::Control,
    )?;
    if runner_stdout_bytes == 0 {
        return Err(p7_provenance_error("P7 RSS runner stdout is empty"));
    }
    let runner_stdout_artifact = &session
        .retained
        .last()
        .ok_or_else(|| p7_provenance_error("P7 RSS stdout was not retained"))?
        .artifact;
    validate_p7_measured_artifact_identity(&measurement.runner_stdout, runner_stdout_artifact)?;
    let (runner_stderr_sha256, _) = session.read_raw(
        &runner_stderr_path,
        &cohort_dir,
        &canonical_root,
        Some(&measurement.runner_stderr.sha256),
        P7ArtifactReadKind::Control,
    )?;
    let runner_stderr_artifact = &session
        .retained
        .last()
        .ok_or_else(|| p7_provenance_error("P7 RSS stderr was not retained"))?
        .artifact;
    validate_p7_measured_artifact_identity(&measurement.runner_stderr, runner_stderr_artifact)?;
    let maximum_rss_bytes = measurement.maximum_rss_bytes;

    validate_p7_producer_preflight_header(run_id, &preflight)?;
    let runner_disk =
        p7_runner_producer_disk_identity_with_reads(&canonical_root, &preflight, session)?;
    validate_p7_preflight_against_disk(&preflight, &runner_disk)?;

    let evidence = P7MaximumRssEvidence {
        schema_version: P7_MAXIMUM_RSS_EVIDENCE_SCHEMA_VERSION.to_string(),
        completed: true,
        rss_gate_passed: maximum_rss_bytes <= P7_MAXIMUM_RSS_LIMIT_BYTES,
        run_id: run_id.to_string(),
        suite: P7_MAXIMUM_RSS_SUITE.to_string(),
        dataset_file: dataset.file_name.to_string(),
        dataset_sha256: expected_dataset.input_sha256,
        input_bytes,
        dataset_index: expected_question.dataset_index,
        question_index: expected_question.question_index,
        question_id: expected_question.question_id,
        question_sha256: format!(
            "{:x}",
            Sha256::digest(expected_question.question.as_bytes())
        ),
        maximum_rss_bytes,
        rss_limit_bytes: P7_MAXIMUM_RSS_LIMIT_BYTES,
        measurement_report_sha256: measurement_sha256,
        measurement_child_exit_status: measurement.child_exit_status,
        measurement_elapsed_millis: measurement.supervisor_receipt.elapsed_millis,
        supervisor_receipt: measurement.supervisor_receipt,
        measured_executable_canonical_path: measurement.child_executable_canonical_path,
        measured_executable_sha256: measurement.child_executable_sha256,
        preflight_report_sha256: preflight_sha256,
        runner_stdout_sha256,
        runner_stderr_sha256,
        detail_sha256,
        summary_sha256,
        preflight_validated_after_measurement: true,
        preflight,
    };
    Ok(P7MaximumRssVerifiedMaterial {
        evidence,
        runner_disk_identity: runner_disk,
    })
}

struct P7WallCohortEvidenceMaterial {
    preflight: P7RunnerPreflightReport,
    preflight_sha256: String,
    maximum_rss: P7MaximumRssEvidence,
    maximum_rss_sha256: String,
    runner_disk_identity: P7RunnerDiskIdentity,
}

fn verify_p7_wall_cohort_evidence_in_session(
    benchmark_root: &Path,
    cohort_dir: &Path,
    run_id: &str,
    preflight_report_path: &Path,
    maximum_rss_report_path: &Path,
    session: &mut P7ArtifactReadSession,
) -> Result<P7WallCohortEvidenceMaterial> {
    if !p7_valid_run_id(run_id) {
        return Err(p7_provenance_error("invalid or missing P7 wall run_id"));
    }
    p7_require_canonical_real_directory(benchmark_root)?;
    p7_require_canonical_real_directory(cohort_dir)?;
    let expected_cohort = benchmark_root.join("results/runs").join(run_id);
    if cohort_dir != expected_cohort || !cohort_dir.starts_with(benchmark_root) {
        return Err(p7_provenance_error(
            "P7 wall cohort escaped the canonical benchmark root",
        ));
    }
    let expected_preflight = cohort_dir.join("preflight-report.json");
    let expected_maximum_rss = cohort_dir.join(P7_MAXIMUM_RSS_REPORT_FILE_NAME);
    if preflight_report_path != expected_preflight
        || maximum_rss_report_path != expected_maximum_rss
    {
        return Err(p7_provenance_error(
            "P7 wall evidence paths differ from the fixed cohort contract",
        ));
    }

    let (preflight, preflight_sha256) = session.read_json::<P7RunnerPreflightReport>(
        preflight_report_path,
        cohort_dir,
        benchmark_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_wall_parse_preflight_report",
    )?;
    let (maximum_rss, maximum_rss_sha256) = session.read_json::<P7MaximumRssEvidence>(
        maximum_rss_report_path,
        cohort_dir,
        benchmark_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_wall_parse_maximum_rss_report",
    )?;
    let fresh_maximum_rss = verify_p7_maximum_rss_evidence_in_session(
        benchmark_root,
        run_id,
        session,
        Some((preflight.clone(), preflight_sha256.clone())),
    )?;
    if maximum_rss != fresh_maximum_rss.evidence {
        return Err(p7_provenance_error(
            "P7 wall maximum RSS report differs from current evidence",
        ));
    }
    Ok(P7WallCohortEvidenceMaterial {
        preflight,
        preflight_sha256,
        maximum_rss,
        maximum_rss_sha256,
        runner_disk_identity: fresh_maximum_rss.runner_disk_identity,
    })
}

pub fn verify_p7_wall_cohort_evidence_with_receipt(
    benchmark_root: &Path,
    run_id: &str,
) -> Result<(P7VerifiedWallCohortEvidence, P7ArtifactLifecycleReceipt)> {
    let canonical_root = fs::canonicalize(benchmark_root).map_err(|source| Error::Io {
        source,
        stage: "p7_wall_evidence_canonicalize_root",
    })?;
    if canonical_root != benchmark_root {
        return Err(p7_provenance_error(
            "P7 wall evidence benchmark root must be canonical",
        ));
    }
    let cohort_dir = canonical_root.join("results/runs").join(run_id);
    let preflight_path = cohort_dir.join("preflight-report.json");
    let maximum_rss_path = cohort_dir.join(P7_MAXIMUM_RSS_REPORT_FILE_NAME);
    let mut session = P7ArtifactReadSession::default();
    let material = verify_p7_wall_cohort_evidence_in_session(
        &canonical_root,
        &cohort_dir,
        run_id,
        &preflight_path,
        &maximum_rss_path,
        &mut session,
    )?;
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("wall_cohort_evidence");
    Ok((
        P7VerifiedWallCohortEvidence {
            preflight: material.preflight,
            preflight_sha256: material.preflight_sha256,
            maximum_rss: material.maximum_rss,
            maximum_rss_sha256: material.maximum_rss_sha256,
        },
        receipt,
    ))
}

pub fn verify_p7_cohort_admission(
    benchmark_root: &Path,
    run_id: &str,
) -> Result<(P7CohortAdmission, String)> {
    let (admission, digest, _) = verify_p7_cohort_admission_with_receipt(benchmark_root, run_id)?;
    Ok((admission, digest))
}

pub fn verify_p7_cohort_admission_with_receipt(
    benchmark_root: &Path,
    run_id: &str,
) -> Result<(P7CohortAdmission, String, P7ArtifactLifecycleReceipt)> {
    let mut session = P7ArtifactReadSession::default();
    let (admission, digest) =
        verify_p7_cohort_admission_in_session(benchmark_root, run_id, &mut session)?;
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("cohort_admission");
    Ok((admission, digest, receipt))
}

fn verify_p7_cohort_admission_in_session(
    benchmark_root: &Path,
    run_id: &str,
    session: &mut P7ArtifactReadSession,
) -> Result<(P7CohortAdmission, String)> {
    if !p7_valid_run_id(run_id) {
        return Err(p7_provenance_error("invalid P7 cohort admission run_id"));
    }
    let canonical_root = fs::canonicalize(benchmark_root).map_err(|source| Error::Io {
        source,
        stage: "p7_admission_canonicalize_benchmark_root",
    })?;
    if canonical_root != benchmark_root {
        return Err(p7_provenance_error(
            "P7 admission benchmark root must be canonical",
        ));
    }
    let cohort_dir = canonical_root.join("results/runs").join(run_id);
    p7_require_canonical_real_directory(&cohort_dir)?;
    let preflight_path = cohort_dir.join("preflight-report.json");
    let maximum_rss_path = cohort_dir.join(P7_MAXIMUM_RSS_REPORT_FILE_NAME);
    let cohort = verify_p7_wall_cohort_evidence_in_session(
        &canonical_root,
        &cohort_dir,
        run_id,
        &preflight_path,
        &maximum_rss_path,
        session,
    )?;
    if !cohort.maximum_rss.rss_gate_passed {
        return Err(p7_provenance_error(
            "P7 cohort admission requires a passing maximum RSS gate",
        ));
    }

    let admission_path = cohort_dir.join(P7_COHORT_ADMISSION_FILE_NAME);
    let (admission, admission_sha256) = session.read_json::<P7CohortAdmission>(
        &admission_path,
        &cohort_dir,
        &canonical_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_admission_parse",
    )?;
    validate_p7_cohort_admission_contract(
        &admission,
        run_id,
        &cohort.preflight_sha256,
        &cohort.maximum_rss_sha256,
        &cohort.preflight.published_release_identity(),
    )?;
    Ok((admission, admission_sha256))
}

pub fn verify_p7_shard_bundle_with_receipt(
    benchmark_root: &Path,
    expectation: &P7ShardBundleExpectation,
) -> Result<(P7ShardBundleState, P7ArtifactLifecycleReceipt)> {
    if !p7_valid_run_id(&expectation.run_id)
        || p7_trusted_dataset(&expectation.suite).is_none()
        || expectation.shard_total == 0
        || expectation.shard_index >= expectation.shard_total
    {
        return Err(p7_provenance_error(
            "P7 shard bundle expectation is invalid",
        ));
    }
    let canonical_root = fs::canonicalize(benchmark_root).map_err(|source| Error::Io {
        source,
        stage: "p7_shard_bundle_canonicalize_root",
    })?;
    if canonical_root != benchmark_root {
        return Err(p7_provenance_error(
            "P7 shard bundle benchmark root must be canonical",
        ));
    }
    let cohort_dir = canonical_root
        .join("results/runs")
        .join(&expectation.run_id);
    let stem = format!(
        "{}.shard-{}-of-{}",
        expectation.suite, expectation.shard_index, expectation.shard_total
    );
    let summary_name = format!("{stem}.summary.json");
    let detail_name = format!("{stem}.jsonl");
    let commit_name = format!("{stem}.commit.json");
    let summary_path = cohort_dir.join(&summary_name);
    let detail_path = cohort_dir.join(&detail_name);
    let commit_path = cohort_dir.join(&commit_name);
    let mut session = P7ArtifactReadSession::default();
    let commit = session.try_read_json::<P7ShardBundleCommit>(
        &commit_path,
        &cohort_dir,
        &canonical_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_shard_bundle_parse_commit",
    )?;
    if commit.is_none() {
        let summary_present = session
            .try_read_with(
                &summary_path,
                &cohort_dir,
                &canonical_root,
                None,
                P7ArtifactReadKind::Summary,
                |reader, _| {
                    std::io::copy(reader, &mut std::io::sink()).map_err(|source| Error::Io {
                        source,
                        stage: "p7_shard_bundle_probe_uncommitted_summary",
                    })
                },
            )?
            .is_some();
        let detail_present = session
            .try_read_with(
                &detail_path,
                &cohort_dir,
                &canonical_root,
                None,
                P7ArtifactReadKind::Detail,
                |reader, _| {
                    std::io::copy(reader, &mut std::io::sink()).map_err(|source| Error::Io {
                        source,
                        stage: "p7_shard_bundle_probe_uncommitted_detail",
                    })
                },
            )?
            .is_some();
        session.verify_retained()?;
        let receipt = session.lifecycle_receipt("shard_bundle_uncommitted_probe");
        let state = if summary_present || detail_present {
            P7ShardBundleState::Uncommitted(P7UncommittedShardBundle {
                summary_present,
                detail_present,
            })
        } else {
            P7ShardBundleState::Absent
        };
        return Ok((state, receipt));
    }
    let (commit, _) = commit.expect("checked committed shard bundle");
    if commit.schema_version != P7_SHARD_BUNDLE_COMMIT_SCHEMA_VERSION
        || commit.run_id != expectation.run_id
        || commit.suite != expectation.suite
        || commit.shard_index != expectation.shard_index
        || commit.shard_total != expectation.shard_total
        || commit.summary_file != summary_name
        || commit.detail_file != detail_name
        || !is_sha256(&commit.summary_sha256)
        || !is_sha256(&commit.detail_sha256)
        || !is_sha256(&commit.producer_identity_sha256)
    {
        return Err(p7_provenance_error(
            "P7 shard bundle commit manifest is invalid",
        ));
    }
    let summary = session.try_read_with(
        &summary_path,
        &cohort_dir,
        &canonical_root,
        Some(&commit.summary_sha256),
        P7ArtifactReadKind::Summary,
        |reader, admitted_len| {
            if admitted_len > P7_MAX_CONTROL_JSON_BYTES {
                return Err(p7_provenance_error(
                    "P7 shard summary exceeds its bounded parse limit",
                ));
            }
            serde_json::from_reader::<_, serde_json::Value>(reader).map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_shard_bundle_parse_summary",
            })
        },
    )?;
    let detail = session.try_read_with(
        &detail_path,
        &cohort_dir,
        &canonical_root,
        Some(&commit.detail_sha256),
        P7ArtifactReadKind::Detail,
        |reader, _| {
            let mut reader = BufReader::with_capacity(P7_FINGERPRINT_READ_BUFFER_BYTES, reader);
            let mut line = Vec::new();
            let mut rows = 0_u64;
            loop {
                if p7_read_bounded_line(&mut reader, &mut line, P7_MAX_DETAIL_LINE_BYTES)? == 0 {
                    break;
                }
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                if line.is_empty() {
                    return Err(p7_provenance_error(
                        "P7 shard detail contains an empty JSONL row",
                    ));
                }
                serde_json::from_slice::<serde_json::Value>(&line).map_err(|source| {
                    Error::Other {
                        source: Box::new(source),
                        stage: "p7_shard_bundle_parse_detail_row",
                    }
                })?;
                rows = rows
                    .checked_add(1)
                    .ok_or_else(|| p7_provenance_error("P7 shard detail row count overflow"))?;
            }
            Ok(rows)
        },
    )?;

    let state = match (summary, detail) {
        (None, None) | (Some(_), None) | (None, Some(_)) => {
            return Err(p7_provenance_error(
                "P7 committed shard bundle is incomplete",
            ));
        }
        (
            Some((summary, summary_sha256, summary_bytes)),
            Some((detail_rows, detail_sha256, detail_bytes)),
        ) => {
            let dataset = p7_trusted_dataset(&expectation.suite)
                .ok_or_else(|| p7_provenance_error("P7 shard bundle trusted dataset is missing"))?;
            let data_dir = canonical_root.join("data");
            let dataset_path = data_dir.join(dataset.file_name);
            let (input_sha256, input_bytes) = session.read_raw(
                &dataset_path,
                &data_dir,
                &canonical_root,
                Some(dataset.input_sha256),
                P7ArtifactReadKind::Dataset,
            )?;
            validate_p7_verified_shard_summary(
                &summary,
                &summary_name,
                &detail_name,
                detail_rows,
                &detail_sha256,
                dataset.file_name,
                input_bytes,
                &input_sha256,
                expectation,
            )?;
            let recorded = serde_json::from_value::<P7RecordedProducerIdentity>(
                summary
                    .get("producer")
                    .ok_or_else(|| p7_provenance_error("P7 shard summary producer is missing"))?
                    .clone(),
            )
            .map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_shard_bundle_commit_parse_producer_envelope",
            })?;
            if summary_sha256 != commit.summary_sha256
                || detail_sha256 != commit.detail_sha256
                || summary_bytes != commit.summary_bytes
                || detail_bytes != commit.detail_bytes
                || recorded.canonical_identity_sha256 != commit.producer_identity_sha256
            {
                return Err(p7_provenance_error(
                    "P7 shard bundle content differs from its commit manifest",
                ));
            }
            P7ShardBundleState::Complete(P7VerifiedShardBundle {
                summary,
                summary_sha256,
                detail_sha256,
                detail_rows,
            })
        }
    };
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("shard_bundle");
    Ok((state, receipt))
}

pub fn verify_p7_shard_set_with_receipt(
    benchmark_root: &Path,
    expectations: &[P7ShardBundleExpectation],
) -> Result<(Vec<P7VerifiedShardBundle>, P7ArtifactLifecycleReceipt)> {
    let first = expectations
        .first()
        .ok_or_else(|| p7_provenance_error("P7 shard set must not be empty"))?;
    if !p7_valid_run_id(&first.run_id)
        || first.shard_total != expectations.len()
        || expectations.iter().enumerate().any(|(index, expectation)| {
            expectation.run_id != first.run_id
                || expectation.suite != first.suite
                || expectation.shard_total != first.shard_total
                || expectation.shard_index != index
                || expectation.build != first.build
                || expectation.release != first.release
                || expectation.execution_kind != first.execution_kind
                || expectation.cohort_admission_sha256 != first.cohort_admission_sha256
        })
    {
        return Err(p7_provenance_error(
            "P7 shard set coordinates or producer identity are not globally exact",
        ));
    }
    let dataset = p7_trusted_dataset(&first.suite)
        .ok_or_else(|| p7_provenance_error("P7 shard set trusted dataset is missing"))?;
    let canonical_root = fs::canonicalize(benchmark_root).map_err(|source| Error::Io {
        source,
        stage: "p7_shard_set_canonicalize_root",
    })?;
    if canonical_root != benchmark_root {
        return Err(p7_provenance_error(
            "P7 shard set benchmark root must be canonical",
        ));
    }
    let cohort_dir = canonical_root.join("results/runs").join(&first.run_id);
    let data_dir = canonical_root.join("data");
    let dataset_path = data_dir.join(dataset.file_name);
    let mut session = P7ArtifactReadSession::default();
    let (input_sha256, input_bytes) = session.read_raw(
        &dataset_path,
        &data_dir,
        &canonical_root,
        Some(dataset.input_sha256),
        P7ArtifactReadKind::Dataset,
    )?;
    let mut bundles = Vec::with_capacity(expectations.len());
    for expectation in expectations {
        bundles.push(verify_p7_committed_shard_in_session(
            &canonical_root,
            &cohort_dir,
            dataset.file_name,
            input_bytes,
            &input_sha256,
            expectation,
            &mut session,
        )?);
    }
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("shard_set");
    Ok((bundles, receipt))
}

#[allow(clippy::too_many_arguments)]
fn verify_p7_committed_shard_in_session(
    canonical_root: &Path,
    cohort_dir: &Path,
    dataset_file_name: &str,
    input_bytes: u64,
    input_sha256: &str,
    expectation: &P7ShardBundleExpectation,
    session: &mut P7ArtifactReadSession,
) -> Result<P7VerifiedShardBundle> {
    let stem = format!(
        "{}.shard-{}-of-{}",
        expectation.suite, expectation.shard_index, expectation.shard_total
    );
    let summary_name = format!("{stem}.summary.json");
    let detail_name = format!("{stem}.jsonl");
    let commit_name = format!("{stem}.commit.json");
    let (commit, _) = session.read_json::<P7ShardBundleCommit>(
        &cohort_dir.join(commit_name),
        cohort_dir,
        canonical_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_shard_set_parse_commit",
    )?;
    if commit.schema_version != P7_SHARD_BUNDLE_COMMIT_SCHEMA_VERSION
        || commit.run_id != expectation.run_id
        || commit.suite != expectation.suite
        || commit.shard_index != expectation.shard_index
        || commit.shard_total != expectation.shard_total
        || commit.summary_file != summary_name
        || commit.detail_file != detail_name
        || !is_sha256(&commit.summary_sha256)
        || !is_sha256(&commit.detail_sha256)
        || !is_sha256(&commit.producer_identity_sha256)
    {
        return Err(p7_provenance_error(
            "P7 shard set commit manifest is invalid",
        ));
    }
    let (summary, summary_sha256, summary_bytes) = session.read_with(
        &cohort_dir.join(&summary_name),
        cohort_dir,
        canonical_root,
        Some(&commit.summary_sha256),
        P7ArtifactReadKind::Summary,
        |reader, admitted_len| {
            if admitted_len > P7_MAX_CONTROL_JSON_BYTES {
                return Err(p7_provenance_error(
                    "P7 shard set summary exceeds its parse cap",
                ));
            }
            serde_json::from_reader::<_, serde_json::Value>(reader).map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_shard_set_parse_summary",
            })
        },
    )?;
    let (detail_rows, detail_sha256, detail_bytes) = session.read_with(
        &cohort_dir.join(&detail_name),
        cohort_dir,
        canonical_root,
        Some(&commit.detail_sha256),
        P7ArtifactReadKind::Detail,
        |reader, _| {
            let mut reader = BufReader::with_capacity(P7_FINGERPRINT_READ_BUFFER_BYTES, reader);
            let mut line = Vec::new();
            let mut rows = 0_u64;
            while p7_read_bounded_line(&mut reader, &mut line, P7_MAX_DETAIL_LINE_BYTES)? != 0 {
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                if line.is_empty() {
                    return Err(p7_provenance_error(
                        "P7 shard set detail contains an empty JSONL row",
                    ));
                }
                serde_json::from_slice::<serde_json::Value>(&line).map_err(|source| {
                    Error::Other {
                        source: Box::new(source),
                        stage: "p7_shard_set_parse_detail_row",
                    }
                })?;
                rows = rows
                    .checked_add(1)
                    .ok_or_else(|| p7_provenance_error("P7 shard set row count overflow"))?;
            }
            Ok(rows)
        },
    )?;
    validate_p7_verified_shard_summary(
        &summary,
        &summary_name,
        &detail_name,
        detail_rows,
        &detail_sha256,
        dataset_file_name,
        input_bytes,
        input_sha256,
        expectation,
    )?;
    let recorded = serde_json::from_value::<P7RecordedProducerIdentity>(
        summary
            .get("producer")
            .ok_or_else(|| p7_provenance_error("P7 shard set producer is missing"))?
            .clone(),
    )
    .map_err(|source| Error::Other {
        source: Box::new(source),
        stage: "p7_shard_set_parse_producer_envelope",
    })?;
    if summary_sha256 != commit.summary_sha256
        || detail_sha256 != commit.detail_sha256
        || summary_bytes != commit.summary_bytes
        || detail_bytes != commit.detail_bytes
        || recorded.canonical_identity_sha256 != commit.producer_identity_sha256
    {
        return Err(p7_provenance_error(
            "P7 shard set bundle differs from its commit manifest",
        ));
    }
    Ok(P7VerifiedShardBundle {
        summary,
        summary_sha256,
        detail_sha256,
        detail_rows,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_p7_verified_shard_summary(
    summary: &serde_json::Value,
    summary_name: &str,
    detail_name: &str,
    detail_rows: u64,
    detail_sha256: &str,
    dataset_file_name: &str,
    input_bytes: u64,
    input_sha256: &str,
    expectation: &P7ShardBundleExpectation,
) -> Result<()> {
    let producer = p7_parse_recorded_shard_producer(
        summary
            .get("producer")
            .ok_or_else(|| p7_provenance_error("P7 shard summary is missing producer"))?,
    )?;
    let expected_limit =
        serde_json::to_value(expectation.limit).map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "p7_shard_bundle_encode_limit",
        })?;
    let expected_question_limit =
        serde_json::to_value(expectation.question_limit).map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "p7_shard_bundle_encode_question_limit",
        })?;
    let expected_question_index =
        serde_json::to_value(expectation.question_index).map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "p7_shard_bundle_encode_question_index",
        })?;
    let expected_input_file = format!("data/{dataset_file_name}");
    if summary
        .get("completed")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
        || summary.get("run_id").and_then(serde_json::Value::as_str)
            != Some(expectation.run_id.as_str())
        || summary.get("suite").and_then(serde_json::Value::as_str)
            != Some(expectation.suite.as_str())
        || summary
            .get("shard_index")
            .and_then(serde_json::Value::as_u64)
            != Some(expectation.shard_index as u64)
        || summary
            .get("shard_total")
            .and_then(serde_json::Value::as_u64)
            != Some(expectation.shard_total as u64)
        || summary.get("limit") != Some(&expected_limit)
        || summary.get("question_limit") != Some(&expected_question_limit)
        || summary.get("question_index") != Some(&expected_question_index)
        || summary
            .get("input_file")
            .and_then(serde_json::Value::as_str)
            != Some(expected_input_file.as_str())
        || summary
            .get("input_bytes")
            .and_then(serde_json::Value::as_u64)
            != Some(input_bytes)
        || summary
            .get("input_sha256")
            .and_then(serde_json::Value::as_str)
            != Some(input_sha256)
        || summary
            .get("detail_file")
            .and_then(serde_json::Value::as_str)
            != Some(detail_name)
        || summary
            .get("summary_file")
            .and_then(serde_json::Value::as_str)
            != Some(summary_name)
        || summary.get("questions").and_then(serde_json::Value::as_u64) != Some(detail_rows)
    {
        return Err(p7_provenance_error(
            "P7 completed shard bundle coordinates or row count mismatch",
        ));
    }
    let release = &expectation.release;
    let build = &expectation.build;
    if producer.schema_version != P7_SHARD_PRODUCER_PROVENANCE_SCHEMA_VERSION
        || producer.execution_kind != expectation.execution_kind
        || producer.run_id != expectation.run_id
        || producer.contract_version != P7_CONTRACT_VERSION
        || producer.sdk_report_schema_version != MEMORY_RECALL_DELIVERY_SCHEMA_VERSION
        || producer.sdk_build_fingerprint != build.sdk_build_fingerprint
        || producer.runner_build_fingerprint != build.runner_build_fingerprint
        || producer.runner_lock_fingerprint != build.runner_lock_fingerprint
        || producer.executable_sha256 != build.executable_sha256
        || producer.build_profile != build.build_profile
        || producer.gate_attestation_sha256 != release.gate_attestation_sha256
        || producer.release_metadata_sha256 != release.release_metadata_sha256
        || producer.gate_source_fingerprint != release.gate_source_fingerprint
        || producer.gate_source_manifest_sha256 != release.gate_source_manifest_sha256
        || producer.gate_ids != release.gate_ids
        || producer.cohort_admission_sha256 != expectation.cohort_admission_sha256
        || producer.input_sha256 != input_sha256
        || producer.detail_schema_version != P7_DETAIL_SCHEMA_VERSION
        || producer.detail_sha256 != detail_sha256
    {
        return Err(p7_provenance_error(
            "P7 completed shard producer or detail binding mismatch",
        ));
    }
    Ok(())
}

pub fn verify_p7_merged_resume_with_receipt(
    benchmark_root: &Path,
    run_id: &str,
    suite: &str,
    expected: &serde_json::Value,
) -> Result<(
    Option<(serde_json::Value, String)>,
    P7ArtifactLifecycleReceipt,
)> {
    if !p7_valid_run_id(run_id) || p7_trusted_dataset(suite).is_none() {
        return Err(p7_provenance_error(
            "P7 merged resume coordinates are invalid",
        ));
    }
    let canonical_root = fs::canonicalize(benchmark_root).map_err(|source| Error::Io {
        source,
        stage: "p7_merged_resume_canonicalize_root",
    })?;
    if canonical_root != benchmark_root {
        return Err(p7_provenance_error(
            "P7 merged resume benchmark root must be canonical",
        ));
    }
    let cohort_dir = canonical_root.join("results/runs").join(run_id);
    let summary_name = format!("{suite}.merged.summary.json");
    let merged_path = cohort_dir.join(&summary_name);
    let commit_path = cohort_dir.join(format!("{suite}.merged.commit.json"));
    let mut session = P7ArtifactReadSession::default();
    let Some((commit, _)) = session.try_read_json::<P7MergedBundleCommit>(
        &commit_path,
        &cohort_dir,
        &canonical_root,
        None,
        P7ArtifactReadKind::Control,
        "p7_merged_resume_parse_commit",
    )?
    else {
        session.verify_retained()?;
        let receipt = session.lifecycle_receipt("merged_resume_uncommitted");
        return Ok((None, receipt));
    };
    if commit.schema_version != P7_MERGED_BUNDLE_COMMIT_SCHEMA_VERSION
        || commit.run_id != run_id
        || commit.suite != suite
        || commit.summary_file != summary_name
        || !is_sha256(&commit.summary_sha256)
    {
        return Err(p7_provenance_error("P7 merged commit manifest is invalid"));
    }
    let material = session
        .try_read_with(
            &merged_path,
            &cohort_dir,
            &canonical_root,
            Some(&commit.summary_sha256),
            P7ArtifactReadKind::Summary,
            |reader, admitted_len| {
                if admitted_len > P7_MAX_CONTROL_JSON_BYTES {
                    return Err(p7_provenance_error(
                        "P7 merged summary exceeds its parse cap",
                    ));
                }
                serde_json::from_reader::<_, serde_json::Value>(reader).map_err(|source| {
                    Error::Other {
                        source: Box::new(source),
                        stage: "p7_merged_resume_parse",
                    }
                })
            },
        )?
        .ok_or_else(|| p7_provenance_error("P7 committed merged summary is missing"))?;
    let material_mismatch = {
        let (existing, digest, bytes) = &material;
        existing != expected || digest != &commit.summary_sha256 || *bytes != commit.summary_bytes
    };
    if material_mismatch {
        return Err(p7_provenance_error(
            "P7 immutable merged summary differs from validated shards",
        ));
    }
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("merged_resume");
    let (value, digest, _) = material;
    Ok((Some((value, digest)), receipt))
}

fn p7_expected_maximum_rss_child_args(benchmark_root: &Path, run_id: &str) -> Vec<String> {
    [
        "--root".to_string(),
        benchmark_root.to_string_lossy().into_owned(),
        "--run-id".to_string(),
        run_id.to_string(),
        "--suite".to_string(),
        P7_MAXIMUM_RSS_SUITE.to_string(),
        "--shard-index".to_string(),
        "0".to_string(),
        "--shard-total".to_string(),
        "1".to_string(),
        "--limit".to_string(),
        "1".to_string(),
        "--question-index".to_string(),
        P7_MAXIMUM_RSS_QUESTION_INDEX.to_string(),
    ]
    .into_iter()
    .collect()
}

fn validate_p7_maximum_rss_measurement_contract(
    measurement: &P7MaximumRssMeasurementReport,
    benchmark_root: &Path,
    run_id: &str,
    preflight: &P7RunnerPreflightReport,
) -> Result<()> {
    if measurement.schema_version != P7_MAXIMUM_RSS_MEASUREMENT_SCHEMA_VERSION
        || measurement.run_id != run_id
        || measurement.child_exit_status != 0
        || measurement.child_executable_canonical_path != preflight.executable_canonical_path
        || measurement.child_executable_sha256 != preflight.executable_sha256
        || !is_sha256(&measurement.child_executable_sha256)
        || measurement.child_args != p7_expected_maximum_rss_child_args(benchmark_root, run_id)
        || measurement.maximum_rss_bytes == 0
        || measurement.supervisor_receipt.schema_version != "p7_sealed_process_receipt_v1"
        || measurement.supervisor_receipt.maximum_rss_bytes != measurement.maximum_rss_bytes
        || measurement
            .supervisor_receipt
            .sealed_executable_sha256
            .as_deref()
            != Some(measurement.child_executable_sha256.as_str())
        || measurement.supervisor_receipt.pid == 0
        || measurement.supervisor_receipt.process_group <= 0
        || !is_sha256(&measurement.runner_stdout.sha256)
        || !is_sha256(&measurement.runner_stderr.sha256)
    {
        return Err(p7_provenance_error(
            "P7 maximum RSS measurement contract mismatch",
        ));
    }
    Ok(())
}

fn validate_p7_measured_artifact_identity(
    recorded: &P7MeasuredArtifactIdentity,
    actual: &P7OpenedArtifact,
) -> Result<()> {
    if recorded.byte_len != actual.freshness.len || recorded.sha256 != actual.sha256 {
        return Err(p7_provenance_error(
            "P7 RSS retained-FD artifact identity differs from the published path",
        ));
    }
    #[cfg(unix)]
    if recorded.device != actual.freshness.device || recorded.inode != actual.freshness.inode {
        return Err(p7_provenance_error(
            "P7 RSS retained-FD device or inode differs from the published path",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_p7_maximum_rss_summary_contract(
    summary: &serde_json::Value,
    run_id: &str,
    dataset: P7TrustedDataset,
    input_path: &Path,
    input_bytes: u64,
    detail_path: &Path,
    summary_path: &Path,
    detail_sha256: &str,
    preflight: &P7RunnerPreflightReport,
) -> Result<()> {
    let producer_value = summary
        .get("producer")
        .ok_or_else(|| p7_provenance_error("P7 RSS summary producer is missing"))?;
    let producer = p7_parse_recorded_shard_producer(producer_value)?;
    let elapsed_secs = summary
        .get("elapsed_secs")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| p7_provenance_error("P7 RSS elapsed time missing"))?;
    if p7_required_str(summary, "run_id", "P7 RSS summary run_id missing")? != run_id
        || p7_required_str(summary, "suite", "P7 RSS summary suite missing")?
            != P7_MAXIMUM_RSS_SUITE
        || p7_required_usize(summary, "shard_index", "P7 RSS shard index missing")? != 0
        || p7_required_usize(summary, "shard_total", "P7 RSS shard total missing")? != 1
        || p7_required_usize(summary, "samples", "P7 RSS sample count missing")? != 1
        || p7_required_usize(summary, "questions", "P7 RSS question count missing")? != 1
        || p7_required_usize(summary, "write_errors", "P7 RSS write error count missing")? != 0
        || p7_required_usize(
            summary,
            "recall_errors",
            "P7 RSS recall error count missing",
        )? != 0
        || p7_required_usize(summary, "limit", "P7 RSS limit missing")? != 1
        || !summary
            .get("question_limit")
            .is_some_and(serde_json::Value::is_null)
        || p7_required_usize(summary, "question_index", "P7 RSS question index missing")?
            != P7_MAXIMUM_RSS_QUESTION_INDEX
        || !p7_required_bool(summary, "completed", "P7 RSS completion flag missing")?
        || p7_required_str(summary, "input_file", "P7 RSS input path missing")?
            != input_path.to_str().unwrap_or("")
        || summary
            .get("input_bytes")
            .and_then(serde_json::Value::as_u64)
            != Some(input_bytes)
        || p7_required_str(summary, "input_sha256", "P7 RSS input digest missing")?
            != dataset.input_sha256
        || p7_required_str(summary, "detail_file", "P7 RSS detail path missing")?
            != detail_path.to_str().unwrap_or("")
        || p7_required_str(summary, "summary_file", "P7 RSS summary path missing")?
            != summary_path.to_str().unwrap_or("")
        || !elapsed_secs.is_finite()
        || elapsed_secs <= 0.0
        || producer.schema_version != P7_SHARD_PRODUCER_PROVENANCE_SCHEMA_VERSION
        || producer.execution_kind != P7ProducerExecutionKind::MaximumRssDiagnostic
        || producer.run_id != run_id
        || producer.contract_version != P7_CONTRACT_VERSION
        || producer.sdk_report_schema_version != MEMORY_RECALL_DELIVERY_SCHEMA_VERSION
        || producer.sdk_build_fingerprint != preflight.sdk_build_fingerprint
        || producer.runner_build_fingerprint != preflight.runner_build_fingerprint
        || producer.runner_lock_fingerprint != preflight.runner_lock_fingerprint
        || producer.executable_sha256 != preflight.executable_sha256
        || producer.gate_attestation_sha256 != preflight.gate_attestation_sha256
        || producer.release_metadata_sha256 != preflight.release_metadata_sha256
        || producer.gate_source_fingerprint != preflight.gate_source_fingerprint
        || producer.gate_source_manifest_sha256 != preflight.gate_source_manifest_sha256
        || producer.gate_ids != preflight.gate_ids
        || producer.build_profile != "release"
        || !producer.cohort_admission_sha256.is_empty()
        || producer.input_sha256 != dataset.input_sha256
        || !p7_detail_schema_supported(&producer.detail_schema_version)
        || producer.detail_sha256 != detail_sha256
    {
        return Err(p7_provenance_error(
            "P7 maximum RSS summary contract mismatch",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[cfg(unix)]
fn p7_require_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        source,
        stage: "p7_maximum_rss_stat_artifact",
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(p7_provenance_error(
            "P7 RSS evidence path must be a regular file",
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        source,
        stage: "p7_maximum_rss_canonicalize_artifact",
    })?;
    if canonical != path {
        return Err(p7_provenance_error(
            "P7 RSS evidence path must not traverse symlinks",
        ));
    }
    Ok(())
}

fn p7_require_canonical_real_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        source,
        stage: "p7_provenance_stat_owner_directory",
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(p7_provenance_error(
            "P7 evidence owner must be a real directory",
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        source,
        stage: "p7_provenance_canonicalize_owner_directory",
    })?;
    if canonical != path {
        return Err(p7_provenance_error(
            "P7 evidence owner directory must be canonical",
        ));
    }
    Ok(())
}

pub fn preflight_p7_runner_release_with_frozen(
    benchmark_root: &Path,
    sdk_root: &Path,
    frozen: P7FrozenRunnerIdentity,
    run_id: &str,
) -> Result<P7RunnerPreflightReport> {
    Ok(preflight_p7_runner_release_with_frozen_and_receipt(
        benchmark_root,
        sdk_root,
        frozen,
        run_id,
    )?
    .0)
}

pub fn preflight_p7_runner_release_with_frozen_and_receipt(
    benchmark_root: &Path,
    sdk_root: &Path,
    frozen: P7FrozenRunnerIdentity,
    run_id: &str,
) -> Result<(P7RunnerPreflightReport, P7ArtifactLifecycleReceipt)> {
    p7_require_canonical_real_directory(benchmark_root)?;
    let sdk_root = fs::canonicalize(sdk_root).map_err(|source| Error::Io {
        source,
        stage: "p7_runner_preflight_canonicalize_sdk_root",
    })?;
    p7_require_canonical_real_directory(&sdk_root)?;
    let runner_root = benchmark_root.join("runner");
    p7_require_canonical_real_directory(&runner_root)?;
    let mut session = P7ArtifactReadSession::default();
    let source =
        p7_release_gate_source_material_in_session(&sdk_root, &runner_root, &mut session, true)?;
    let sdk_build_fingerprint = source.sdk_build_fingerprint.clone();
    let disk = p7_runner_disk_identity_for_release_sha_in_session(
        benchmark_root,
        frozen.executable_sha256,
        &sdk_root,
        &runner_root,
        &source,
        &mut session,
    )?;
    validate_p7_frozen_release_binding(frozen, &disk, &source.manifest.source_fingerprint)?;
    if sdk_build_fingerprint != P7_TRUSTED_SDK_BUILD_FINGERPRINT
        || !is_sha256(&sdk_build_fingerprint)
    {
        return Err(p7_preflight_error(
            "P7 SDK or frozen runner disk identity drifted",
        ));
    }
    let output = p7_run_retained_executable(
        &disk.executable_canonical_path,
        &["--print-build-identity"],
        "p7_runner_preflight_execute_identity",
    )?;
    if !output.status.success() {
        return Err(p7_preflight_error(
            "frozen runner rejected --print-build-identity",
        ));
    }
    let embedded =
        serde_json::from_slice::<P7RunnerBuildIdentity>(&output.stdout).map_err(|source| {
            Error::Other {
                source: Box::new(source),
                stage: "p7_runner_preflight_parse_identity",
            }
        })?;
    if embedded.sdk_build_fingerprint != sdk_build_fingerprint
        || embedded.runner_build_fingerprint != disk.runner_build_fingerprint
        || embedded.runner_lock_fingerprint != disk.runner_lock_fingerprint
        || embedded.executable_sha256 != disk.executable_sha256
        || embedded.build_profile != "release"
        || !is_sha256(&embedded.sdk_build_fingerprint)
        || !is_sha256(&embedded.runner_build_fingerprint)
        || !is_sha256(&embedded.runner_lock_fingerprint)
        || !is_sha256(&embedded.executable_sha256)
    {
        return Err(p7_preflight_error(
            "P7 SDK, runner source, lock, profile, or executable identity drifted",
        ));
    }
    session.verify_retained()?;
    let executable_canonical_path = disk
        .executable_canonical_path
        .to_str()
        .ok_or_else(|| p7_preflight_error("P7 runner canonical path is not valid UTF-8"))?
        .to_string();
    let report = P7RunnerPreflightReport {
        schema_version: P7_RUNNER_PREFLIGHT_SCHEMA_VERSION.to_string(),
        run_id: run_id.to_string(),
        sdk_build_fingerprint,
        runner_build_fingerprint: disk.runner_build_fingerprint,
        runner_lock_fingerprint: disk.runner_lock_fingerprint,
        executable_sha256: disk.executable_sha256,
        executable_canonical_path,
        gate_attestation_sha256: disk.gate_attestation_sha256,
        release_metadata_sha256: disk.release_metadata_sha256,
        gate_source_fingerprint: disk.gate_source_fingerprint,
        gate_source_manifest_sha256: disk.gate_source_manifest_sha256,
        gate_ids: disk.gate_ids,
        build_profile: embedded.build_profile,
    };
    let receipt = session.lifecycle_receipt("release_preflight");
    Ok((report, receipt))
}

#[cfg(test)]
fn p7_benchmark_root_for_run(merged_summary_path: &Path, run_id: &str) -> Result<PathBuf> {
    if !p7_valid_run_id(run_id) {
        return Err(p7_provenance_error("invalid or missing P7 run_id"));
    }
    let run_dir = merged_summary_path
        .parent()
        .ok_or_else(|| p7_provenance_error("merged summary has no run directory"))?;
    let runs_dir = run_dir
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 run directory has no runs parent"))?;
    let results_dir = runs_dir
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 runs directory has no results parent"))?;
    if run_dir.file_name().and_then(|name| name.to_str()) != Some(run_id)
        || runs_dir.file_name().and_then(|name| name.to_str()) != Some("runs")
        || results_dir.file_name().and_then(|name| name.to_str()) != Some("results")
    {
        return Err(p7_provenance_error(
            "merged summary must be under results/runs/<run-id>",
        ));
    }
    results_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| p7_provenance_error("results directory has no benchmark root"))
}

fn p7_valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id != "."
        && run_id != ".."
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn p7_runner_release_executable_path(
    benchmark_root: &Path,
    executable_sha256: &str,
) -> Result<PathBuf> {
    if !is_sha256(executable_sha256) {
        return Err(p7_preflight_error(
            "frozen P7 runner executable digest is invalid",
        ));
    }
    Ok(benchmark_root
        .join("runner")
        .join(P7_RUNNER_RELEASES_DIR)
        .join(executable_sha256)
        .join(P7_RUNNER_RELEASE_FILE_NAME))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7ReleaseGateSpec {
    gate_id: &'static str,
    owner_root: String,
    argv: Vec<String>,
}

pub fn p7_release_gate_plan() -> P7ReleaseGatePlan {
    let commands: [(P7ReleaseGateOwner, &[&str]); 14] = [
        (
            P7ReleaseGateOwner::AgentMemory,
            &["cargo", "fmt", "--all", "--", "--check"],
        ),
        (
            P7ReleaseGateOwner::AgentMemory,
            &[
                "cargo",
                "check",
                "--locked",
                "--workspace",
                "--all-targets",
                "--no-default-features",
            ],
        ),
        (
            P7ReleaseGateOwner::AgentMemory,
            &[
                "cargo",
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--no-default-features",
                "--",
                "-D",
                "warnings",
            ],
        ),
        (
            P7ReleaseGateOwner::AgentMemory,
            &[
                "cargo",
                "test",
                "--locked",
                "--workspace",
                "--no-default-features",
            ],
        ),
        (
            P7ReleaseGateOwner::AgentMemory,
            &["bash", "scripts/check_memory_write_transaction_contract.sh"],
        ),
        (
            P7ReleaseGateOwner::AgentMemory,
            &["bash", "scripts/check_next_gen_memory_plan.sh"],
        ),
        (
            P7ReleaseGateOwner::AgentMemory,
            &[
                "bash",
                "scripts/check_cross_target_compile_gates.sh",
                "--strict",
            ],
        ),
        (
            P7ReleaseGateOwner::AgentMemory,
            &["bash", "scripts/check_p7_linux_execution_authority.sh"],
        ),
        (
            P7ReleaseGateOwner::ExternalRunner,
            &["cargo", "fmt", "--", "--check"],
        ),
        (
            P7ReleaseGateOwner::ExternalRunner,
            &["cargo", "test", "--locked", "--no-default-features"],
        ),
        (
            P7ReleaseGateOwner::ExternalRunner,
            &[
                "cargo",
                "clippy",
                "--locked",
                "--all-targets",
                "--no-default-features",
                "--",
                "-D",
                "warnings",
            ],
        ),
        (
            P7ReleaseGateOwner::ExternalRunner,
            &[
                "bash",
                "-n",
                "run_full_p7_wall.sh",
                "run_p7_max_rss.sh",
                "tests/run_full_p7_wall_fake_runner_test.sh",
                "tests/run_p7_max_rss_fake_runner_test.sh",
            ],
        ),
        (
            P7ReleaseGateOwner::ExternalRunner,
            &["bash", "tests/run_full_p7_wall_fake_runner_test.sh"],
        ),
        (
            P7ReleaseGateOwner::ExternalRunner,
            &["bash", "tests/run_p7_max_rss_fake_runner_test.sh"],
        ),
    ];
    let steps = P7_REQUIRED_RELEASE_GATE_IDS
        .into_iter()
        .zip(commands)
        .enumerate()
        .map(|(index, (gate_id, (owner, argv)))| P7ReleaseGatePlanStep {
            ordinal: u8::try_from(index + 1).expect("P7 gate count fits u8"),
            gate_id: gate_id.to_string(),
            owner,
            argv: argv.iter().map(|arg| (*arg).to_string()).collect(),
        })
        .collect::<Vec<_>>();
    let mut plan = P7ReleaseGatePlan {
        schema_version: P7_RELEASE_GATE_PLAN_SCHEMA_VERSION.to_string(),
        orchestrator_contract: P7_RELEASE_GATE_ORCHESTRATOR_CONTRACT.to_string(),
        producer_identity_contract: P7_PRODUCER_IDENTITY_SCHEMA_VERSION.to_string(),
        verifier_identity_contract: P7_VERIFIER_IDENTITY_SCHEMA_VERSION.to_string(),
        steps,
        plan_sha256: String::new(),
    };
    plan.plan_sha256 = p7_release_gate_plan_sha256(&plan);
    plan
}

fn p7_release_gate_plan_sha256(plan: &P7ReleaseGatePlan) -> String {
    let mut canonical = plan.clone();
    canonical.plan_sha256.clear();
    let body = serde_json::to_vec(&canonical).expect("P7 release gate plan is serializable");
    format!("{:x}", Sha256::digest(body))
}

pub fn verify_p7_release_gate_plan(plan: &P7ReleaseGatePlan) -> Result<()> {
    if plan != &p7_release_gate_plan() || plan.plan_sha256 != p7_release_gate_plan_sha256(plan) {
        return Err(p7_preflight_error(
            "P7 release gate plan differs from bm-replay authority",
        ));
    }
    Ok(())
}

fn p7_required_release_gate_specs(
    sdk_root: &Path,
    runner_root: &Path,
) -> Result<Vec<P7ReleaseGateSpec>> {
    p7_require_canonical_real_directory(sdk_root)?;
    p7_require_canonical_real_directory(runner_root)?;
    let sdk_owner = sdk_root
        .to_str()
        .ok_or_else(|| p7_preflight_error("P7 SDK gate owner path is not valid UTF-8"))?;
    let runner_owner = runner_root
        .to_str()
        .ok_or_else(|| p7_preflight_error("P7 runner gate owner path is not valid UTF-8"))?;
    Ok(p7_release_gate_plan()
        .steps
        .into_iter()
        .map(|step| P7ReleaseGateSpec {
            gate_id: P7_REQUIRED_RELEASE_GATE_IDS[usize::from(step.ordinal - 1)],
            owner_root: match step.owner {
                P7ReleaseGateOwner::AgentMemory => sdk_owner,
                P7ReleaseGateOwner::ExternalRunner => runner_owner,
            }
            .to_string(),
            argv: step.argv,
        })
        .collect())
}

fn validate_p7_release_gate_attestation(
    attestation: &P7ReleaseGateAttestation,
    expected_identity: &P7RunnerBuildIdentity,
    sdk_root: &Path,
    runner_root: &Path,
) -> Result<()> {
    if attestation.schema_version != P7_RELEASE_GATE_ATTESTATION_SCHEMA_VERSION
        || attestation.orchestrator_contract != P7_RELEASE_GATE_ORCHESTRATOR_CONTRACT
        || verify_p7_release_gate_plan(&attestation.plan).is_err()
        || &attestation.identity != expected_identity
        || !is_sha256(&attestation.source_fingerprint)
        || !is_sha256(&attestation.source_manifest_sha256)
        || p7_release_gate_environment_sha256(&attestation.environment.variables)?
            != attestation.environment.sha256
        || !is_sha256(&attestation.environment.sha256)
    {
        return Err(p7_preflight_error(
            "P7 release gate attestation header or identity mismatch",
        ));
    }
    let specs = p7_required_release_gate_specs(sdk_root, runner_root)?;
    let tools = attestation
        .tools
        .iter()
        .map(|tool| (tool.logical_name.as_str(), tool))
        .collect::<BTreeMap<_, _>>();
    if attestation.gates.len() != specs.len()
        || tools.len() != 2
        || !tools.contains_key("cargo")
        || !tools.contains_key("bash")
        || !p7_release_gate_environment_is_whitelisted(&attestation.environment.variables)
    {
        return Err(p7_preflight_error(
            "P7 release gate attestation does not cover the fixed gate set",
        ));
    }
    for tool in tools.values() {
        if !Path::new(&tool.canonical_path).is_absolute()
            || !is_sha256(&tool.sha256)
            || tool.version.trim().is_empty()
        {
            return Err(p7_preflight_error(
                "P7 release gate tool identity is invalid",
            ));
        }
    }
    for (receipt, spec) in attestation.gates.iter().zip(specs) {
        let logical_program = spec
            .argv
            .first()
            .ok_or_else(|| p7_preflight_error("P7 release gate argv is empty"))?;
        let tool = tools
            .get(logical_program.as_str())
            .ok_or_else(|| p7_preflight_error("P7 release gate tool is unattested"))?;
        let expected_argv = std::iter::once(tool.canonical_path.clone())
            .chain(spec.argv.into_iter().skip(1))
            .collect::<Vec<_>>();
        if receipt.gate_id != spec.gate_id
            || receipt.owner_root != spec.owner_root
            || receipt.argv != expected_argv
            || receipt.tool_sha256 != tool.sha256
            || receipt.environment_sha256 != attestation.environment.sha256
            || receipt.exit_code != 0
            || !is_sha256(&receipt.stdout_sha256)
            || !is_sha256(&receipt.stderr_sha256)
            || receipt.source_fingerprint_after != attestation.source_fingerprint
        {
            return Err(p7_preflight_error(
                "P7 release gate attestation receipt mismatch",
            ));
        }
    }
    Ok(())
}

fn p7_release_gate_environment_is_whitelisted(variables: &BTreeMap<String, String>) -> bool {
    const REQUIRED: [&str; 5] = [
        "CARGO_NET_OFFLINE",
        "LANG",
        "LC_ALL",
        "PATH",
        "RUST_BACKTRACE",
    ];
    const OPTIONAL: [&str; 6] = [
        "CARGO_HOME",
        "HOME",
        "MACOSX_DEPLOYMENT_TARGET",
        "RUSTUP_HOME",
        "SDKROOT",
        "TMPDIR",
    ];
    REQUIRED.into_iter().all(|key| variables.contains_key(key))
        && variables
            .keys()
            .all(|key| REQUIRED.contains(&key.as_str()) || OPTIONAL.contains(&key.as_str()))
        && variables.get("CARGO_NET_OFFLINE").map(String::as_str) == Some("true")
        && variables.get("LANG").map(String::as_str) == Some("C")
        && variables.get("LC_ALL").map(String::as_str) == Some("C")
        && variables.get("RUST_BACKTRACE").map(String::as_str) == Some("0")
}

fn p7_release_gate_environment_sha256(variables: &BTreeMap<String, String>) -> Result<String> {
    let mut hasher = Sha256::new();
    p7_hash_fingerprint_field(&mut hasher, b"beetle-memory:p7:release-gate-environment:v1")?;
    hasher.update(
        u64::try_from(variables.len())
            .map_err(|_| p7_preflight_error("P7 release gate environment is too large"))?
            .to_le_bytes(),
    );
    for (key, value) in variables {
        p7_hash_fingerprint_field(&mut hasher, key.as_bytes())?;
        p7_hash_fingerprint_field(&mut hasher, value.as_bytes())?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn p7_runner_producer_disk_identity_with_reads(
    benchmark_root: &Path,
    preflight: &P7RunnerPreflightReport,
    session: &mut P7ArtifactReadSession,
) -> Result<P7RunnerDiskIdentity> {
    p7_require_canonical_real_directory(benchmark_root)?;
    let expected_executable_path =
        p7_runner_release_executable_path(benchmark_root, &preflight.executable_sha256)?;
    let expected_parent = expected_executable_path
        .parent()
        .ok_or_else(|| p7_preflight_error("P7 producer release path has no parent"))?;
    p7_require_canonical_real_directory(expected_parent)?;
    let attestation_path = expected_parent.join(P7_RELEASE_GATE_ATTESTATION_FILE_NAME);
    let metadata_path = expected_parent.join(P7_RELEASE_METADATA_FILE_NAME);
    let source_manifest_path = expected_parent.join(P7_RELEASE_GATE_SOURCE_MANIFEST_FILE_NAME);

    let (executable_sha256, _) = session.read_raw(
        &expected_executable_path,
        expected_parent,
        benchmark_root,
        Some(&preflight.executable_sha256),
        P7ArtifactReadKind::Release,
    )?;
    let (attestation, attestation_sha256) = session.read_json::<P7ReleaseGateAttestation>(
        &attestation_path,
        expected_parent,
        benchmark_root,
        Some(&preflight.gate_attestation_sha256),
        P7ArtifactReadKind::Release,
        "p7_producer_parse_gate_attestation",
    )?;
    let (metadata, metadata_sha256) = session.read_json::<P7ReleaseMetadata>(
        &metadata_path,
        expected_parent,
        benchmark_root,
        Some(&preflight.release_metadata_sha256),
        P7ArtifactReadKind::Release,
        "p7_producer_parse_release_metadata",
    )?;
    let (source_manifest, source_manifest_sha256) = session.read_json::<P7ReleaseSourceManifest>(
        &source_manifest_path,
        expected_parent,
        benchmark_root,
        Some(&preflight.gate_source_manifest_sha256),
        P7ArtifactReadKind::Release,
        "p7_producer_parse_source_manifest",
    )?;
    let gate_ids = attestation
        .gates
        .iter()
        .map(|gate| gate.gate_id.clone())
        .collect::<Vec<_>>();
    if metadata.schema_version != P7_RELEASE_METADATA_SCHEMA_VERSION
        || attestation.schema_version != P7_RELEASE_GATE_ATTESTATION_SCHEMA_VERSION
        || source_manifest.schema_version != P7_RELEASE_GATE_SOURCE_MANIFEST_SCHEMA_VERSION
        || metadata.identity != attestation.identity
        || metadata.identity.executable_sha256 != executable_sha256
        || metadata.identity.build_profile != "release"
        || metadata.canonical_executable_path != expected_executable_path.to_string_lossy()
        || metadata.gate_attestation_sha256 != attestation_sha256
        || metadata.gate_source_manifest_sha256 != source_manifest_sha256
        || attestation.source_manifest_sha256 != source_manifest_sha256
        || metadata.gate_source_fingerprint != attestation.source_fingerprint
        || metadata.gate_source_fingerprint != source_manifest.source_fingerprint
        || metadata.gate_ids != gate_ids
        || gate_ids != P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
        || attestation.gates.iter().any(|gate| gate.exit_code != 0)
    {
        return Err(p7_preflight_error(
            "P7 immutable producer release governance is inconsistent",
        ));
    }
    Ok(P7RunnerDiskIdentity {
        runner_build_fingerprint: metadata.identity.runner_build_fingerprint,
        runner_lock_fingerprint: metadata.identity.runner_lock_fingerprint,
        executable_sha256: metadata.identity.executable_sha256,
        executable_canonical_path: expected_executable_path,
        gate_attestation_sha256: attestation_sha256,
        release_metadata_sha256: metadata_sha256,
        gate_source_fingerprint: metadata.gate_source_fingerprint,
        gate_source_manifest_sha256: source_manifest_sha256,
        gate_ids,
    })
}

#[cfg(test)]
fn p7_runner_disk_identity_for_release_sha(
    benchmark_root: &Path,
    executable_sha256: &str,
) -> Result<P7RunnerDiskIdentity> {
    let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| p7_preflight_error("bm-replay is not under the SDK workspace root"))?;
    let canonical_sdk_root = fs::canonicalize(sdk_root).map_err(|source| Error::Io {
        source,
        stage: "p7_runner_preflight_canonicalize_sdk_root",
    })?;
    let runner_root = benchmark_root.join("runner");
    let mut session = P7ArtifactReadSession::default();
    let source = p7_release_gate_source_material_in_session(
        &canonical_sdk_root,
        &runner_root,
        &mut session,
        true,
    )?;
    let disk = p7_runner_disk_identity_for_release_sha_in_session(
        benchmark_root,
        executable_sha256,
        &canonical_sdk_root,
        &runner_root,
        &source,
        &mut session,
    )?;
    session.verify_retained()?;
    Ok(disk)
}

fn p7_runner_disk_identity_for_release_sha_in_session(
    benchmark_root: &Path,
    executable_sha256: &str,
    canonical_sdk_root: &Path,
    canonical_runner_source_root: &Path,
    source: &P7ReleaseGateSourceMaterial,
    session: &mut P7ArtifactReadSession,
) -> Result<P7RunnerDiskIdentity> {
    p7_require_canonical_real_directory(benchmark_root)?;
    let runner_root = benchmark_root.join("runner");
    p7_require_canonical_real_directory(&runner_root)?;
    let expected_executable_path =
        p7_runner_release_executable_path(benchmark_root, executable_sha256)?;
    let expected_parent = expected_executable_path
        .parent()
        .ok_or_else(|| p7_preflight_error("frozen P7 runner path has no parent"))?;
    p7_require_canonical_real_directory(expected_parent)?;
    let canonical_runner_root = runner_root.clone();
    let canonical_expected_parent =
        fs::canonicalize(expected_parent).map_err(|source| Error::Io {
            source,
            stage: "p7_runner_preflight_canonicalize_frozen_executable_parent",
        })?;
    let expected_release_parent = canonical_runner_root
        .join(P7_RUNNER_RELEASES_DIR)
        .join(executable_sha256);
    if canonical_expected_parent != expected_release_parent {
        return Err(p7_preflight_error(
            "frozen P7 runner release directory is not the content-addressed owner path",
        ));
    }
    let attestation_path = expected_parent.join(P7_RELEASE_GATE_ATTESTATION_FILE_NAME);
    let metadata_path = expected_parent.join(P7_RELEASE_METADATA_FILE_NAME);
    let source_manifest_path = expected_parent.join(P7_RELEASE_GATE_SOURCE_MANIFEST_FILE_NAME);
    let (actual_executable_sha256, _) = session.read_raw(
        &expected_executable_path,
        expected_parent,
        benchmark_root,
        Some(executable_sha256),
        P7ArtifactReadKind::Release,
    )?;
    let (attestation, attestation_sha256) = session.read_json::<P7ReleaseGateAttestation>(
        &attestation_path,
        expected_parent,
        benchmark_root,
        None,
        P7ArtifactReadKind::Release,
        "p7_runner_preflight_parse_release_gate_attestation",
    )?;
    let (metadata, metadata_sha256) = session.read_json::<P7ReleaseMetadata>(
        &metadata_path,
        expected_parent,
        benchmark_root,
        None,
        P7ArtifactReadKind::Release,
        "p7_runner_preflight_parse_release_metadata",
    )?;
    let (source_manifest, source_manifest_sha256) = session.read_json::<P7ReleaseSourceManifest>(
        &source_manifest_path,
        expected_parent,
        benchmark_root,
        None,
        P7ArtifactReadKind::Release,
        "p7_runner_preflight_parse_release_source_manifest",
    )?;
    #[cfg(unix)]
    if fs::symlink_metadata(&expected_executable_path)
        .map_err(|source| Error::Io {
            source,
            stage: "p7_runner_preflight_stat_frozen_executable_mode",
        })?
        .mode()
        & 0o111
        == 0
    {
        return Err(p7_preflight_error(
            "frozen P7 runner release is not executable",
        ));
    }
    let executable_canonical_path = expected_executable_path.clone();
    if executable_canonical_path != canonical_expected_parent.join(P7_RUNNER_RELEASE_FILE_NAME) {
        return Err(p7_preflight_error(
            "frozen P7 runner executable escaped its content-addressed release path",
        ));
    }
    let runner_build_fingerprint = source.runner_build_fingerprint.clone();
    let runner_lock_fingerprint = source.runner_lock_fingerprint.clone();
    let expected_identity = P7RunnerBuildIdentity {
        sdk_build_fingerprint: P7_TRUSTED_SDK_BUILD_FINGERPRINT.to_string(),
        runner_build_fingerprint: runner_build_fingerprint.clone(),
        runner_lock_fingerprint: runner_lock_fingerprint.clone(),
        executable_sha256: actual_executable_sha256.clone(),
        build_profile: "release".to_string(),
    };
    validate_p7_release_gate_attestation(
        &attestation,
        &expected_identity,
        canonical_sdk_root,
        canonical_runner_source_root,
    )?;
    let attested_gate_ids = attestation
        .gates
        .iter()
        .map(|gate| gate.gate_id.clone())
        .collect::<Vec<_>>();
    let required_gate_ids = P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec();
    let executable_path = executable_canonical_path
        .to_str()
        .ok_or_else(|| p7_preflight_error("P7 runner canonical path is not valid UTF-8"))?;
    if metadata.schema_version != P7_RELEASE_METADATA_SCHEMA_VERSION
        || metadata.canonical_executable_path != executable_path
        || metadata.identity != expected_identity
        || metadata.identity != attestation.identity
        || metadata.gate_attestation_sha256 != attestation_sha256
        || !is_sha256(&metadata.gate_attestation_sha256)
        || metadata.gate_source_fingerprint != attestation.source_fingerprint
        || metadata.gate_source_fingerprint != source_manifest.source_fingerprint
        || !is_sha256(&metadata.gate_source_fingerprint)
        || source_manifest.schema_version != P7_RELEASE_GATE_SOURCE_MANIFEST_SCHEMA_VERSION
        || source_manifest.fingerprint_contract != P7_RELEASE_GATE_SOURCE_FINGERPRINT_CONTRACT
        || source_manifest != source.manifest
        || attestation.source_manifest_sha256 != source_manifest_sha256
        || metadata.gate_source_manifest_sha256 != source_manifest_sha256
        || metadata.gate_ids != required_gate_ids
        || metadata.gate_ids != attested_gate_ids
    {
        return Err(p7_preflight_error(
            "P7 release metadata does not bind the exact governed release",
        ));
    }
    Ok(P7RunnerDiskIdentity {
        runner_build_fingerprint,
        runner_lock_fingerprint,
        executable_sha256: actual_executable_sha256,
        executable_canonical_path,
        gate_attestation_sha256: attestation_sha256,
        release_metadata_sha256: metadata_sha256,
        gate_source_fingerprint: attestation.source_fingerprint,
        gate_source_manifest_sha256: source_manifest_sha256,
        gate_ids: attested_gate_ids,
    })
}

fn p7_fingerprint_inputs(root: &Path, relatives: &[&str]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for relative in relatives {
        p7_collect_regular_files(&root.join(relative), &mut files)?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

#[cfg(test)]
fn p7_operator_build_inputs(root: &Path) -> Result<Vec<PathBuf>> {
    p7_fingerprint_inputs(root, &P7_OPERATOR_BUILD_INPUTS)
}

pub fn p7_release_gate_source_fingerprint(sdk_root: &Path, runner_root: &Path) -> Result<String> {
    Ok(p7_release_gate_source_manifest(sdk_root, runner_root)?.source_fingerprint)
}

pub fn p7_release_gate_source_manifest(
    sdk_root: &Path,
    runner_root: &Path,
) -> Result<P7ReleaseSourceManifest> {
    Ok(p7_release_gate_source_manifest_with_receipt(sdk_root, runner_root)?.0)
}

pub fn p7_producer_semantic_source_manifest(
    sdk_root: &Path,
    runner_root: &Path,
) -> Result<P7ReleaseSourceManifest> {
    let broad = p7_release_gate_source_manifest(sdk_root, runner_root)?;
    let entries = broad
        .entries
        .into_iter()
        .filter(|entry| {
            let inputs = if entry.owner == "agent-memory" {
                &P7_PRODUCER_SEMANTIC_AGENT_MEMORY_INPUTS[..]
            } else if entry.owner == "external-runner" {
                &P7_PRODUCER_SEMANTIC_RUNNER_INPUTS[..]
            } else {
                return false;
            };
            inputs
                .iter()
                .any(|input| p7_manifest_input_contains(input, &entry.relative_path))
        })
        .collect::<Vec<_>>();
    p7_source_manifest_from_entries(
        entries,
        P7_PRODUCER_SEMANTIC_SOURCE_MANIFEST_SCHEMA_VERSION,
        P7_PRODUCER_SEMANTIC_SOURCE_FINGERPRINT_CONTRACT,
    )
}

fn p7_manifest_input_contains(input: &str, relative_path: &str) -> bool {
    input == "."
        || relative_path == input
        || relative_path
            .strip_prefix(input)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub fn p7_release_gate_source_manifest_with_receipt(
    sdk_root: &Path,
    runner_root: &Path,
) -> Result<(P7ReleaseSourceManifest, P7ArtifactLifecycleReceipt)> {
    let mut session = P7ArtifactReadSession::default();
    let material =
        p7_release_gate_source_material_in_session(sdk_root, runner_root, &mut session, false)?;
    session.verify_retained()?;
    let receipt = session.lifecycle_receipt("release_source_manifest");
    Ok((material.manifest, receipt))
}

struct P7ReleaseGateSourceMaterial {
    manifest: P7ReleaseSourceManifest,
    sdk_build_fingerprint: String,
    runner_build_fingerprint: String,
    runner_lock_fingerprint: String,
}

fn p7_release_gate_source_material_in_session(
    sdk_root: &Path,
    runner_root: &Path,
    session: &mut P7ArtifactReadSession,
    include_build_fingerprints: bool,
) -> Result<P7ReleaseGateSourceMaterial> {
    p7_require_canonical_real_directory(sdk_root)?;
    p7_require_canonical_real_directory(runner_root)?;
    let sdk_files = p7_release_gate_fingerprint_inputs(
        sdk_root,
        &P7_AGENT_MEMORY_RELEASE_GATE_SOURCE_INPUTS,
        &[P7_FROZEN_RUNNER_IDENTITY_RELATIVE_PATH],
        &P7_AGENT_MEMORY_RELEASE_GATE_EXCLUDED_DIRECTORIES,
    )?;
    let runner_files = p7_release_gate_fingerprint_inputs(
        runner_root,
        &P7_RUNNER_RELEASE_GATE_SOURCE_INPUTS,
        &[],
        &P7_RUNNER_RELEASE_GATE_EXCLUDED_DIRECTORIES,
    )?;
    let source_file_count = sdk_files
        .len()
        .checked_add(runner_files.len())
        .ok_or_else(|| p7_preflight_error("P7 release source file count overflow"))?;
    if source_file_count > P7_MAX_RELEASE_SOURCE_FILES {
        return Err(p7_preflight_error(
            "P7 release source exceeds its bounded file count",
        ));
    }
    let sdk_build_inputs = if include_build_fingerprints {
        p7_fingerprint_inputs(sdk_root, &P7_SDK_BUILD_INPUTS)?
    } else {
        Vec::new()
    };
    let runner_build_inputs = if include_build_fingerprints {
        p7_fingerprint_inputs(runner_root, &P7_RUNNER_BUILD_INPUTS)?
    } else {
        Vec::new()
    };
    let sdk_build_set = sdk_build_inputs.iter().cloned().collect::<BTreeSet<_>>();
    let runner_build_set = runner_build_inputs.iter().cloned().collect::<BTreeSet<_>>();
    let mut sdk_build_hasher = Sha256::new();
    let mut runner_build_hasher = Sha256::new();
    if include_build_fingerprints {
        p7_hash_fingerprint_field(
            &mut sdk_build_hasher,
            P7_SDK_BUILD_FINGERPRINT_CONTRACT.as_bytes(),
        )?;
        sdk_build_hasher.update(
            u64::try_from(sdk_build_inputs.len())
                .map_err(|_| p7_preflight_error("P7 SDK build input count overflow"))?
                .to_le_bytes(),
        );
        p7_hash_fingerprint_field(
            &mut runner_build_hasher,
            P7_RUNNER_BUILD_FINGERPRINT_CONTRACT.as_bytes(),
        )?;
        runner_build_hasher.update(
            u64::try_from(runner_build_inputs.len())
                .map_err(|_| p7_preflight_error("P7 runner build input count overflow"))?
                .to_le_bytes(),
        );
    }
    let mut entries = Vec::with_capacity(sdk_files.len() + runner_files.len());
    let mut runner_lock_fingerprint = String::new();
    let sdk_excluded_files = [P7_FROZEN_RUNNER_IDENTITY_RELATIVE_PATH];
    let runner_excluded_files: [&str; 0] = [];
    for (owner, root, files, excluded_files, excluded_directories) in [
        (
            "agent-memory",
            sdk_root,
            sdk_files,
            &sdk_excluded_files[..],
            &P7_AGENT_MEMORY_RELEASE_GATE_EXCLUDED_DIRECTORIES[..],
        ),
        (
            "external-runner",
            runner_root,
            runner_files,
            &runner_excluded_files[..],
            &P7_RUNNER_RELEASE_GATE_EXCLUDED_DIRECTORIES[..],
        ),
    ] {
        for file in files {
            let relative = file.strip_prefix(root).map_err(|_| {
                p7_preflight_error("release gate source input escaped its canonical root")
            })?;
            let metadata = fs::symlink_metadata(&file).map_err(|source| Error::Io {
                source,
                stage: "p7_preflight_stat_release_gate_manifest_entry",
            })?;
            let build_hasher = if owner == "agent-memory" && sdk_build_set.contains(&file) {
                Some(&mut sdk_build_hasher)
            } else if owner == "external-runner" && runner_build_set.contains(&file) {
                Some(&mut runner_build_hasher)
            } else {
                None
            };
            let (entry_kind, byte_len, sha256) = if metadata.file_type().is_symlink() {
                if build_hasher.is_some() {
                    return Err(p7_preflight_error(
                        "P7 build fingerprint inputs must not be symbolic links",
                    ));
                }
                let target = fs::read_link(&file).map_err(|source| Error::Io {
                    source,
                    stage: "p7_preflight_read_release_gate_symlink",
                })?;
                let canonical_target = fs::canonicalize(&file).map_err(|source| Error::Io {
                    source,
                    stage: "p7_preflight_canonicalize_release_gate_symlink",
                })?;
                if !canonical_target.starts_with(root)
                    || p7_release_gate_source_path_is_excluded(
                        root,
                        &canonical_target,
                        excluded_files,
                        excluded_directories,
                    )?
                    || fs::read_link(&file).map_err(|source| Error::Io {
                        source,
                        stage: "p7_preflight_reread_release_gate_symlink",
                    })? != target
                    || P7RegularFileFreshness::from_metadata(&fs::symlink_metadata(&file).map_err(
                        |source| Error::Io {
                            source,
                            stage: "p7_preflight_restat_release_gate_symlink",
                        },
                    )?) != P7RegularFileFreshness::from_metadata(&metadata)
                {
                    return Err(p7_preflight_error(
                        "P7 release gate symlink escaped its owner or changed during capture",
                    ));
                }
                let target_bytes = target.as_os_str().as_encoded_bytes();
                (
                    P7ReleaseSourceManifestEntryKind::SymbolicLink,
                    u64::try_from(target_bytes.len()).map_err(|_| {
                        p7_preflight_error("P7 release gate symlink target is too large")
                    })?,
                    format!("{:x}", Sha256::digest(target_bytes)),
                )
            } else {
                let parent = file.parent().ok_or_else(|| {
                    p7_preflight_error("release gate source file has no owner directory")
                })?;
                let (sha256, byte_len) = if let Some(hasher) = build_hasher {
                    p7_hash_fingerprint_field(hasher, relative.to_string_lossy().as_bytes())?;
                    let (_, sha256, byte_len) = session.read_with(
                        &file,
                        parent,
                        root,
                        None,
                        P7ArtifactReadKind::Release,
                        |reader, expected_len| {
                            p7_hash_fingerprint_reader(hasher, expected_len, reader)
                        },
                    )?;
                    (sha256, byte_len)
                } else {
                    session.read_raw(&file, parent, root, None, P7ArtifactReadKind::Release)?
                };
                if owner == "external-runner" && relative == Path::new("Cargo.lock") {
                    runner_lock_fingerprint = sha256.clone();
                }
                (
                    P7ReleaseSourceManifestEntryKind::RegularFile,
                    byte_len,
                    sha256,
                )
            };
            entries.push(P7ReleaseSourceManifestEntry {
                owner: owner.to_string(),
                relative_path: relative.to_string_lossy().into_owned(),
                entry_kind,
                byte_len,
                sha256,
            });
        }
    }
    let manifest = p7_source_manifest_from_entries(
        entries,
        P7_RELEASE_GATE_SOURCE_MANIFEST_SCHEMA_VERSION,
        P7_RELEASE_GATE_SOURCE_FINGERPRINT_CONTRACT,
    )?;
    if include_build_fingerprints && !is_sha256(&runner_lock_fingerprint) {
        return Err(p7_preflight_error(
            "P7 runner Cargo.lock is missing from release gate source inputs",
        ));
    }
    Ok(P7ReleaseGateSourceMaterial {
        manifest,
        sdk_build_fingerprint: if include_build_fingerprints {
            format!("{:x}", sdk_build_hasher.finalize())
        } else {
            String::new()
        },
        runner_build_fingerprint: if include_build_fingerprints {
            format!("{:x}", runner_build_hasher.finalize())
        } else {
            String::new()
        },
        runner_lock_fingerprint,
    })
}

fn p7_source_manifest_from_entries(
    mut entries: Vec<P7ReleaseSourceManifestEntry>,
    schema_version: &str,
    fingerprint_contract: &str,
) -> Result<P7ReleaseSourceManifest> {
    entries.sort_by(|left, right| {
        (&left.owner, &left.relative_path).cmp(&(&right.owner, &right.relative_path))
    });
    let mut hasher = Sha256::new();
    p7_hash_fingerprint_field(&mut hasher, fingerprint_contract.as_bytes())?;
    hasher.update(
        u64::try_from(entries.len())
            .map_err(|_| p7_preflight_error("release gate source input count overflow"))?
            .to_le_bytes(),
    );
    for entry in &entries {
        p7_hash_fingerprint_field(&mut hasher, entry.owner.as_bytes())?;
        p7_hash_fingerprint_field(&mut hasher, entry.relative_path.as_bytes())?;
        p7_hash_fingerprint_field(
            &mut hasher,
            match entry.entry_kind {
                P7ReleaseSourceManifestEntryKind::RegularFile => b"regular_file",
                P7ReleaseSourceManifestEntryKind::SymbolicLink => b"symbolic_link",
            },
        )?;
        hasher.update(entry.byte_len.to_le_bytes());
        p7_hash_fingerprint_field(&mut hasher, entry.sha256.as_bytes())?;
    }
    Ok(P7ReleaseSourceManifest {
        schema_version: schema_version.to_string(),
        fingerprint_contract: fingerprint_contract.to_string(),
        source_fingerprint: format!("{:x}", hasher.finalize()),
        entries,
    })
}

fn p7_release_gate_fingerprint_inputs(
    root: &Path,
    relatives: &[&str],
    excluded_relative_files: &[&str],
    excluded_relative_directories: &[&str],
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for relative in relatives {
        p7_collect_release_gate_source_files(
            root,
            &root.join(relative),
            excluded_relative_files,
            excluded_relative_directories,
            &mut files,
        )?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn p7_collect_release_gate_source_files(
    root: &Path,
    path: &Path,
    excluded_relative_files: &[&str],
    excluded_relative_directories: &[&str],
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    if p7_release_gate_source_path_is_excluded(
        root,
        path,
        excluded_relative_files,
        excluded_relative_directories,
    )? {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        source,
        stage: "p7_preflight_stat_release_gate_source",
    })?;
    if metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if !metadata.file_type().is_dir() {
        return Err(p7_preflight_error(
            "P7 release gate source input must be a regular file or directory",
        ));
    }
    let mut entries = fs::read_dir(path)
        .map_err(|source| Error::Io {
            source,
            stage: "p7_preflight_read_release_gate_source_directory",
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| Error::Io {
            source,
            stage: "p7_preflight_read_release_gate_source_directory",
        })?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        p7_collect_release_gate_source_files(
            root,
            &entry.path(),
            excluded_relative_files,
            excluded_relative_directories,
            files,
        )?;
    }
    Ok(())
}

fn p7_release_gate_source_path_is_excluded(
    root: &Path,
    path: &Path,
    excluded_relative_files: &[&str],
    excluded_relative_directories: &[&str],
) -> Result<bool> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| p7_preflight_error("release gate source input escaped its canonical root"))?;
    Ok(excluded_relative_files
        .iter()
        .any(|excluded| relative == Path::new(excluded))
        || excluded_relative_directories
            .iter()
            .any(|excluded| relative == Path::new(excluded) || relative.starts_with(excluded))
        || relative.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| P7_RELEASE_GATE_EXCLUDED_DIRECTORY_NAMES.contains(&name))
        }))
}

fn p7_collect_regular_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries = fs::read_dir(path)
        .map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_read_runner_build_input",
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_read_runner_build_input",
        })?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let child = entry.path();
        if child.is_dir() {
            p7_collect_regular_files(&child, files)?;
        } else if child.is_file() {
            files.push(child);
        }
    }
    Ok(())
}

#[cfg(test)]
fn p7_fingerprint_files_with_contract(
    root: &Path,
    files: &[PathBuf],
    contract: &str,
) -> Result<String> {
    let mut hasher = Sha256::new();
    p7_hash_fingerprint_field(&mut hasher, contract.as_bytes())?;
    let file_count = u64::try_from(files.len())
        .map_err(|_| p7_provenance_error("build input count overflow"))?;
    hasher.update(file_count.to_le_bytes());
    for file in files {
        let relative = file
            .strip_prefix(root)
            .map_err(|_| p7_provenance_error("build input is outside the fingerprint root"))?;
        p7_hash_fingerprint_field(&mut hasher, relative.to_string_lossy().as_bytes())?;
        p7_hash_fingerprint_file(&mut hasher, file)?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
fn p7_hash_fingerprint_file(hasher: &mut Sha256, path: &Path) -> Result<()> {
    let file = File::open(path).map_err(|source| Error::Io {
        source,
        stage: "p7_provenance_open_runner_build_input",
    })?;
    let expected_len = file
        .metadata()
        .map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_stat_runner_build_input",
        })?
        .len();
    let mut reader = BufReader::with_capacity(P7_FINGERPRINT_READ_BUFFER_BYTES, file);
    p7_hash_fingerprint_reader(hasher, expected_len, &mut reader)
}

fn p7_hash_fingerprint_reader<R: Read + ?Sized>(
    hasher: &mut Sha256,
    expected_len: u64,
    reader: &mut R,
) -> Result<()> {
    hasher.update(expected_len.to_le_bytes());
    let mut actual_len = 0_u64;
    let mut buffer = [0_u8; P7_FINGERPRINT_READ_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut buffer).map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_hash_runner_build_input",
        })?;
        if read == 0 {
            break;
        }
        actual_len = actual_len
            .checked_add(
                u64::try_from(read)
                    .map_err(|_| p7_provenance_error("runner build input length overflow"))?,
            )
            .ok_or_else(|| p7_provenance_error("runner build input length overflow"))?;
        hasher.update(&buffer[..read]);
    }
    if actual_len != expected_len {
        return Err(p7_provenance_error(
            "runner build input changed while fingerprinting",
        ));
    }
    Ok(())
}

fn p7_hash_fingerprint_field(hasher: &mut Sha256, value: &[u8]) -> Result<()> {
    let len = u64::try_from(value.len())
        .map_err(|_| p7_provenance_error("runner build fingerprint field overflow"))?;
    hasher.update(len.to_le_bytes());
    hasher.update(value);
    Ok(())
}

#[cfg(test)]
fn p7_sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|source| Error::Io {
        source,
        stage: "p7_provenance_read_runner_release_binary",
    })?;
    p7_sha256_reader(&mut file)
}

#[cfg(test)]
fn p7_sha256_reader(reader: &mut impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_hash_runner_release_binary",
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7ExpectedQuestionIdentity {
    case_id: String,
    dataset_index: usize,
    question_index: usize,
    question_id: String,
    question: String,
    gold_sources: Vec<String>,
}

struct P7ExpectedDataset {
    input_sha256: String,
    samples_by_shard: Vec<usize>,
    questions_by_shard: Vec<Vec<P7ExpectedQuestionIdentity>>,
}

fn load_p7_dataset_expectation(
    file: impl Read,
    dataset: P7TrustedDataset,
    shard_count: usize,
) -> Result<P7ExpectedDataset> {
    let mut stream = P7JsonArrayObjectStream::new(file);
    let mut samples_by_shard = vec![0_usize; shard_count];
    let mut questions_by_shard = vec![Vec::new(); shard_count];
    let mut seen_question_ids = BTreeSet::new();
    let mut dataset_index = 0_usize;
    while let Some(item) = stream.next_object()? {
        let shard_index = dataset_index % shard_count;
        samples_by_shard[shard_index] = samples_by_shard[shard_index].saturating_add(1);
        let questions = if dataset.suite == "locomo" {
            p7_locomo_expected_questions(&item, dataset_index)?
        } else {
            vec![p7_longmemeval_expected_question(&item, dataset_index)?]
        };
        for question in questions {
            if !seen_question_ids.insert(question.question_id.clone()) {
                return Err(p7_provenance_error("dataset question_id is not unique"));
            }
            questions_by_shard[shard_index].push(question);
        }
        dataset_index = dataset_index.saturating_add(1);
    }
    let (input_sha256, _) = stream.finish()?;
    if input_sha256 != dataset.input_sha256 {
        return Err(p7_provenance_error("trusted input dataset bytes changed"));
    }
    Ok(P7ExpectedDataset {
        input_sha256,
        samples_by_shard,
        questions_by_shard,
    })
}

fn p7_locomo_expected_questions(
    item: &serde_json::Value,
    dataset_index: usize,
) -> Result<Vec<P7ExpectedQuestionIdentity>> {
    let case_id = p7_required_str(item, "sample_id", "LoCoMo sample_id missing")?;
    let questions = item
        .get("qa")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| p7_provenance_error("LoCoMo qa array missing"))?;
    questions
        .iter()
        .enumerate()
        .map(|(question_index, question)| {
            Ok(P7ExpectedQuestionIdentity {
                case_id: case_id.to_string(),
                dataset_index,
                question_index,
                question_id: format!("{case_id}__q{question_index}"),
                question: p7_required_str(question, "question", "LoCoMo question missing")?
                    .to_string(),
                gold_sources: p7_string_array(
                    question
                        .get("evidence")
                        .ok_or_else(|| p7_provenance_error("LoCoMo evidence missing"))?,
                    "LoCoMo evidence must be a string array",
                )?,
            })
        })
        .collect()
}

fn p7_longmemeval_expected_question(
    item: &serde_json::Value,
    dataset_index: usize,
) -> Result<P7ExpectedQuestionIdentity> {
    let question_id = p7_required_str(item, "question_id", "LongMemEval question_id missing")?;
    Ok(P7ExpectedQuestionIdentity {
        case_id: question_id.to_string(),
        dataset_index,
        question_index: 0,
        question_id: question_id.to_string(),
        question: p7_required_str(item, "question", "LongMemEval question missing")?.to_string(),
        gold_sources: p7_string_array(
            item.get("answer_session_ids")
                .ok_or_else(|| p7_provenance_error("LongMemEval gold sources missing"))?,
            "LongMemEval gold sources must be a string array",
        )?,
    })
}

struct P7HashingReader<R> {
    inner: R,
    hasher: Sha256,
    bytes_read: u64,
}

impl<R> P7HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes_read: 0,
        }
    }

    fn finish(self) -> (String, u64) {
        (format!("{:x}", self.hasher.finalize()), self.bytes_read)
    }
}

impl<R: Read> Read for P7HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        self.bytes_read = self.bytes_read.checked_add(read as u64).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "P7 artifact bytes-read overflow",
            )
        })?;
        Ok(read)
    }
}

struct P7JsonArrayObjectStream<R> {
    reader: BufReader<P7HashingReader<R>>,
    started: bool,
    finished: bool,
    max_object_bytes: usize,
}

impl<R: Read> P7JsonArrayObjectStream<R> {
    fn new(file: R) -> Self {
        Self::with_object_limit(file, P7_MAX_DATASET_OBJECT_BYTES)
    }

    fn with_object_limit(file: R, max_object_bytes: usize) -> Self {
        Self {
            reader: BufReader::new(P7HashingReader::new(file)),
            started: false,
            finished: false,
            max_object_bytes,
        }
    }

    fn next_object(&mut self) -> Result<Option<serde_json::Value>> {
        if self.finished {
            return Ok(None);
        }
        let mut byte = [0_u8; 1];
        if !self.started {
            loop {
                if self.read_byte(&mut byte)? == 0 {
                    return Err(p7_provenance_error("input dataset is empty"));
                }
                if byte[0].is_ascii_whitespace() {
                    continue;
                }
                if byte[0] != b'[' {
                    return Err(p7_provenance_error("input dataset root is not an array"));
                }
                self.started = true;
                break;
            }
        }
        loop {
            if self.read_byte(&mut byte)? == 0 {
                return Err(p7_provenance_error("unexpected input dataset EOF"));
            }
            match byte[0] {
                b',' | b' ' | b'\n' | b'\r' | b'\t' => continue,
                b']' => {
                    self.finished = true;
                    return Ok(None);
                }
                b'{' => break,
                _ => return Err(p7_provenance_error("input dataset item is not an object")),
            }
        }
        let mut bytes = vec![b'{'];
        let mut depth = 1_i32;
        let mut in_string = false;
        let mut escaped = false;
        while depth > 0 {
            if self.read_byte(&mut byte)? == 0 {
                return Err(p7_provenance_error("unexpected EOF inside dataset object"));
            }
            if bytes.len() >= self.max_object_bytes {
                return Err(p7_provenance_error(
                    "input dataset object exceeds its allocation ceiling",
                ));
            }
            let current = byte[0];
            bytes.push(current);
            if in_string {
                if escaped {
                    escaped = false;
                } else if current == b'\\' {
                    escaped = true;
                } else if current == b'"' {
                    in_string = false;
                }
                continue;
            }
            match current {
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                _ => {}
            }
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_provenance_parse_input_dataset",
            })
    }

    fn finish(mut self) -> Result<(String, u64)> {
        if !self.finished {
            return Err(p7_provenance_error("input dataset was not fully consumed"));
        }
        let mut trailing = [0_u8; P7_FINGERPRINT_READ_BUFFER_BYTES];
        loop {
            let read = self
                .reader
                .read(&mut trailing)
                .map_err(|source| Error::Io {
                    source,
                    stage: "p7_provenance_hash_input_dataset",
                })?;
            if read == 0 {
                break;
            }
            if trailing[..read]
                .iter()
                .any(|byte| !byte.is_ascii_whitespace())
            {
                return Err(p7_provenance_error("input dataset has trailing content"));
            }
        }
        Ok(self.reader.into_inner().finish())
    }

    fn read_byte(&mut self, byte: &mut [u8; 1]) -> Result<usize> {
        self.reader.read(byte).map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_stream_input_dataset",
        })
    }
}

const P7_ADDITIVE_SUMMARY_FIELDS: &[&str] = &[
    "samples",
    "questions",
    "evidence_questions",
    "any_evidence_hit",
    "all_evidence_hit",
    "write_errors",
    "recall_errors",
    "stage_hit_counts",
    "index_diagnostics",
    "w4_1_diagnostics",
    "facet_ablation",
    "p7_loss_ledger",
    "p7_production_delivery",
];

fn accumulate_p7_shard_summary(
    aggregate: &mut serde_json::Map<String, serde_json::Value>,
    shard: &serde_json::Value,
) -> Result<()> {
    for field in P7_ADDITIVE_SUMMARY_FIELDS {
        let value = shard
            .get(*field)
            .ok_or_else(|| p7_provenance_error("shard additive field missing"))?;
        let target = aggregate
            .entry((*field).to_string())
            .or_insert(serde_json::Value::Null);
        add_p7_additive_json(target, value)?;
    }
    Ok(())
}

fn add_p7_additive_json(target: &mut serde_json::Value, value: &serde_json::Value) -> Result<()> {
    if target.is_null() {
        *target = value.clone();
        return Ok(());
    }
    match (target, value) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            let left_value = left
                .as_i64()
                .ok_or_else(|| p7_provenance_error("non-integer additive value"))?;
            let right_value = right
                .as_i64()
                .ok_or_else(|| p7_provenance_error("non-integer additive value"))?;
            *left = (left_value.saturating_add(right_value)).into();
            Ok(())
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            for (key, right_value) in right {
                let left_value = left.entry(key.clone()).or_insert(serde_json::Value::Null);
                add_p7_additive_json(left_value, right_value)?;
            }
            Ok(())
        }
        _ => Err(p7_provenance_error("non-additive shard summary field")),
    }
}

fn validate_p7_additive_merge(
    summary: &W4ExternalNoisyBenchmarkSummary,
    aggregate: &serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let merged = serde_json::to_value(summary).map_err(|source| Error::Other {
        source: Box::new(source),
        stage: "p7_provenance_serialize_merged_summary",
    })?;
    for field in P7_ADDITIVE_SUMMARY_FIELDS {
        if merged.get(*field) != aggregate.get(*field) {
            return Err(p7_provenance_error(
                "merged summary is not an exact shard merge",
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct P7DetailAggregate {
    samples: usize,
    questions: usize,
    evidence_questions: usize,
    any_evidence_hit: usize,
    all_evidence_hit: usize,
    write_errors: usize,
    recall_errors: usize,
    stage_hit_counts: W4ExternalNoisyStageHitCounts,
    index_diagnostics: W4ExternalNoisyIndexDiagnostics,
    w4_1_diagnostics: W4ExternalNoisyW41Diagnostics,
    facet_ablation: W4ExternalNoisyFacetAblationDiagnostics,
    p7_loss_ledger: W4ExternalNoisyP7LossDiagnostics,
    p7_production_delivery: W4ExternalNoisyP7ProductionDeliveryDiagnostics,
    source_signature_counts: BTreeMap<String, usize>,
    streamed_bytes_read: u64,
}

impl P7DetailAggregate {
    fn add_assign(&mut self, other: &Self) -> Result<()> {
        self.samples = self.samples.saturating_add(other.samples);
        self.questions = self.questions.saturating_add(other.questions);
        self.evidence_questions = self
            .evidence_questions
            .saturating_add(other.evidence_questions);
        self.any_evidence_hit = self.any_evidence_hit.saturating_add(other.any_evidence_hit);
        self.all_evidence_hit = self.all_evidence_hit.saturating_add(other.all_evidence_hit);
        self.write_errors = self.write_errors.saturating_add(other.write_errors);
        self.recall_errors = self.recall_errors.saturating_add(other.recall_errors);
        self.streamed_bytes_read = self
            .streamed_bytes_read
            .checked_add(other.streamed_bytes_read)
            .ok_or_else(|| p7_provenance_error("P7 detail bytes-read overflow"))?;
        add_stage_hit_counts(&mut self.stage_hit_counts, &other.stage_hit_counts);
        add_index_diagnostics(&mut self.index_diagnostics, &other.index_diagnostics);
        add_w41_diagnostics(&mut self.w4_1_diagnostics, &other.w4_1_diagnostics);
        add_facet_ablation(&mut self.facet_ablation, &other.facet_ablation);
        add_p7_loss(&mut self.p7_loss_ledger, &other.p7_loss_ledger);
        add_p7_production_delivery(
            &mut self.p7_production_delivery,
            &other.p7_production_delivery,
        );
        Ok(())
    }

    fn refresh_source_signature_diagnostics(&mut self) {
        self.w4_1_diagnostics.source_signature_count = self.source_signature_counts.len();
        self.w4_1_diagnostics.repeated_source_signature_questions = self
            .source_signature_counts
            .values()
            .map(|count| count.saturating_sub(1))
            .sum();
    }
}

struct P7DetailValidationContext<'a> {
    suite: &'a str,
    run_id: &'a str,
    detail_schema_version: &'a str,
    expected_questions: &'a [P7ExpectedQuestionIdentity],
    expected_samples: usize,
}

fn validate_p7_detail_file(
    file: impl Read,
    expected_sha256: &str,
    context: P7DetailValidationContext<'_>,
    seen_question_ids: &mut BTreeSet<String>,
    seen_identities: &mut BTreeSet<(String, usize, usize, String)>,
) -> Result<P7DetailAggregate> {
    if !p7_detail_schema_supported(context.detail_schema_version) {
        return Err(p7_provenance_error(
            "producer detail schema is not supported by this verifier",
        ));
    }
    let mut reader = BufReader::new(P7HashingReader::new(file));
    let mut aggregate = P7DetailAggregate {
        samples: context.expected_samples,
        ..P7DetailAggregate::default()
    };
    let mut line = Vec::new();
    let mut row_index = 0_usize;
    loop {
        let read = p7_read_bounded_line(&mut reader, &mut line, P7_MAX_DETAIL_LINE_BYTES)?;
        if read == 0 {
            break;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let expected = context
            .expected_questions
            .get(row_index)
            .ok_or_else(|| p7_provenance_error("detail contains an unexpected question"))?;
        let row =
            serde_json::from_slice::<serde_json::Value>(&line).map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_provenance_parse_detail_row",
            })?;
        if p7_required_str(&row, "schema_version", "detail schema version missing")?
            != context.detail_schema_version
        {
            return Err(p7_provenance_error(
                "detail row schema differs from producer provenance",
            ));
        }
        validate_p7_detail_identity(
            &row,
            context.suite,
            context.run_id,
            expected,
            seen_question_ids,
            seen_identities,
        )?;
        accumulate_p7_detail_row(&mut aggregate, &row, expected)?;
        row_index = row_index.saturating_add(1);
    }
    if row_index != context.expected_questions.len() {
        return Err(p7_provenance_error("detail row count mismatch"));
    }
    let (actual_sha256, streamed_bytes_read) = reader.into_inner().finish();
    if actual_sha256 != expected_sha256 {
        return Err(p7_provenance_error("shard detail digest mismatch"));
    }
    aggregate.streamed_bytes_read = streamed_bytes_read;
    Ok(aggregate)
}

fn p7_read_bounded_line<R: BufRead>(
    reader: &mut R,
    line: &mut Vec<u8>,
    max_line_bytes: usize,
) -> Result<usize> {
    line.clear();
    loop {
        let available = reader.fill_buf().map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_read_detail_row",
        })?;
        if available.is_empty() {
            return Ok(line.len());
        }
        let chunk_len = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        let next_line_len = line
            .len()
            .checked_add(chunk_len)
            .ok_or_else(|| p7_provenance_error("P7 detail row length overflow"))?;
        if next_line_len > max_line_bytes {
            return Err(p7_provenance_error(
                "P7 detail row exceeds the bounded line contract",
            ));
        }
        line.extend_from_slice(&available[..chunk_len]);
        reader.consume(chunk_len);
        if line.last() == Some(&b'\n') {
            return Ok(line.len());
        }
    }
}

fn validate_p7_detail_identity(
    row: &serde_json::Value,
    suite: &str,
    run_id: &str,
    expected: &P7ExpectedQuestionIdentity,
    seen_question_ids: &mut BTreeSet<String>,
    seen_identities: &mut BTreeSet<(String, usize, usize, String)>,
) -> Result<()> {
    let case_id = p7_required_str(row, "case_id", "detail case_id missing")?;
    let dataset_index = p7_required_usize(row, "dataset_index", "detail dataset_index missing")?;
    let question_index = p7_required_usize(row, "question_index", "detail question_index missing")?;
    let question_id = p7_required_str(row, "question_id", "detail question_id missing")?;
    let detail_gold = p7_string_array(
        row.get("gold_sources")
            .ok_or_else(|| p7_provenance_error("detail gold_sources missing"))?,
        "detail gold_sources must be a string array",
    )?;
    let detail_gold_groups = p7_canonical_groups(&detail_gold);
    if detail_gold.len() != detail_gold_groups.len()
        || detail_gold
            .iter()
            .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
    {
        return Err(p7_provenance_error(
            "detail gold_sources are not unique opaque canonical ids",
        ));
    }
    if p7_required_str(row, "suite", "detail suite missing")? != suite
        || p7_required_str(row, "run_id", "detail run_id missing")? != run_id
        || case_id != expected.case_id
        || dataset_index != expected.dataset_index
        || question_index != expected.question_index
        || question_id != expected.question_id
        || p7_required_str(row, "question", "detail question missing")? != expected.question
        || detail_gold_groups != p7_canonical_groups(&expected.gold_sources)
    {
        return Err(p7_provenance_error("detail question identity mismatch"));
    }
    if !seen_question_ids.insert(question_id.to_string())
        || !seen_identities.insert((
            case_id.to_string(),
            dataset_index,
            question_index,
            question_id.to_string(),
        ))
    {
        return Err(p7_provenance_error(
            "detail question identity is not unique",
        ));
    }
    Ok(())
}

fn accumulate_p7_detail_row(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    validate_p7_no_external_locators(row)?;
    let question_evaluation = P7QuestionEvaluationContract::from_canonical_gold_count(
        p7_canonical_groups(&expected.gold_sources).len(),
    );
    let claimed = row
        .get("question_evaluation")
        .ok_or_else(|| p7_provenance_error("detail question evaluation contract missing"))?;
    let claimed = serde_json::from_value::<P7QuestionEvaluationContract>(claimed.clone()).map_err(
        |source| Error::Other {
            source: Box::new(source),
            stage: "p7_provenance_parse_question_evaluation",
        },
    )?;
    if claimed != question_evaluation {
        return Err(p7_provenance_error(
            "detail question evaluation contract mismatch",
        ));
    }
    validate_p7_no_gold_ablation(row, question_evaluation.is_evidence_question())?;
    for field in [
        "index_diagnostics",
        "graph_index_report",
        "facet_index_report",
        "stage_diagnostics",
        "p7_loss_ledger",
        "eval_delivery_report",
        "final_projection_delivery_report",
        "sdk_projection_delivery_manifest",
        "runner_projection_digest_observation",
        "final_projection_integrity",
        "privacy_report",
    ] {
        if row.get(field).is_none_or(serde_json::Value::is_null) {
            return Err(p7_provenance_error("P7 detail proof field missing"));
        }
    }
    validate_p7_stage_candidate_reports(row)?;
    aggregate.questions = aggregate.questions.saturating_add(1);
    if question_evaluation.is_evidence_question() {
        aggregate.evidence_questions = aggregate.evidence_questions.saturating_add(1);
    }
    aggregate.write_errors = aggregate.write_errors.saturating_add(usize::from(
        !row.get("write_error")
            .is_none_or(serde_json::Value::is_null),
    ));
    aggregate.recall_errors = aggregate.recall_errors.saturating_add(usize::from(
        !row.get("recall_error")
            .is_none_or(serde_json::Value::is_null),
    ));

    let final_delivery = row
        .get("final_projection_delivery_report")
        .ok_or_else(|| p7_provenance_error("final projection delivery report missing"))?;
    let rendered_candidates = p7_rendered_candidates_from_delivery(final_delivery)?;
    let matched_gold = p7_match_gold_groups(&expected.gold_sources, &rendered_candidates);
    let gold_group_count = p7_canonical_groups(&expected.gold_sources).len();
    let any_hit = !matched_gold.is_empty();
    let all_hit = gold_group_count > 0 && matched_gold.len() == gold_group_count;
    if p7_required_bool(row, "any_evidence_hit", "detail any hit missing")? != any_hit
        || p7_required_bool(row, "all_evidence_hit", "detail all hit missing")? != all_hit
    {
        return Err(p7_provenance_error(
            "detail final rendered hit fact mismatch",
        ));
    }
    aggregate.any_evidence_hit = aggregate
        .any_evidence_hit
        .saturating_add(usize::from(any_hit));
    aggregate.all_evidence_hit = aggregate
        .all_evidence_hit
        .saturating_add(usize::from(all_hit));

    accumulate_p7_stage_hits(aggregate, row, expected)?;
    accumulate_p7_index_diagnostics(aggregate, row)?;
    accumulate_p7_w41_diagnostics(aggregate, row, expected)?;
    if question_evaluation.is_evidence_question() {
        accumulate_p7_ablation(aggregate, row, expected)?;
    }
    accumulate_p7_loss(aggregate, row, expected)?;
    accumulate_p7_production_delivery(aggregate, row)?;
    Ok(())
}

fn validate_p7_no_gold_ablation(row: &serde_json::Value, is_evidence_question: bool) -> Result<()> {
    if is_evidence_question {
        return Ok(());
    }
    let report = row
        .get("ablation_report")
        .ok_or_else(|| p7_provenance_error("SDK ablation applicability missing"))?;
    let valid_no_gold_contract =
        p7_required_str(report, "method", "no-gold SDK ablation method missing")?
            == P7_ABLATION_METHOD
            && p7_row_string_array(report, "required_slices")?.is_empty()
            && p7_required_array(report, "slices", "no-gold SDK ablation slices missing")?
                .is_empty()
            && !p7_required_bool(
                report,
                "delivery_contribution_proven",
                "no-gold SDK ablation contribution flag missing",
            )?
            && p7_required_usize(
                report,
                "render_growth",
                "no-gold SDK ablation render growth missing",
            )? == 0
            && p7_row_string_array(report, "blocked_reasons")?.is_empty();
    if !valid_no_gold_contract {
        return Err(p7_provenance_error(
            "no-gold SDK ablation report is not the exact not-applicable contract",
        ));
    }
    Ok(())
}

fn validate_p7_no_external_locators(value: &serde_json::Value) -> Result<()> {
    match value {
        serde_json::Value::String(value) if value.contains("external_eval:") => Err(
            p7_provenance_error("P7 detail exposes a raw external evaluation locator"),
        ),
        serde_json::Value::Array(values) => {
            for value in values {
                validate_p7_no_external_locators(value)?;
            }
            Ok(())
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                validate_p7_no_external_locators(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn accumulate_p7_stage_hits(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    let final_delivery = row
        .get("final_projection_delivery_report")
        .ok_or_else(|| p7_provenance_error("final projection delivery report missing"))?;
    let rendered_sources = p7_row_string_array(row, "rendered_sources")?;
    if rendered_sources != p7_rendered_sources_from_delivery(final_delivery)? {
        return Err(p7_provenance_error(
            "rendered sources are not owned by final projection delivery",
        ));
    }
    let projection_selected_sources = p7_row_string_array(row, "projection_selected_sources")?;
    if projection_selected_sources != p7_selected_sources_from_delivery(final_delivery)? {
        return Err(p7_provenance_error(
            "projection selected sources are not owned by final projection delivery",
        ));
    }
    let stages = [
        ("source", "source"),
        ("expanded", "expanded"),
        ("reranked", "reranked"),
        ("eval_selected", "selected"),
        ("projection_selected", "projection_selected"),
        ("final_rendered", "rendered"),
    ];
    for (field, stage) in stages {
        let candidates = p7_stage_candidates(row, field)?;
        let matched = p7_matched_gold_group_set(&expected.gold_sources, &candidates);
        let gold_count = p7_canonical_groups(&expected.gold_sources).len();
        let any = usize::from(!matched.is_empty());
        let all = usize::from(gold_count > 0 && matched.len() == gold_count);
        match stage {
            "source" => {
                aggregate.stage_hit_counts.source_any_evidence_hit += any;
                aggregate.stage_hit_counts.source_all_evidence_hit += all;
            }
            "expanded" => {
                aggregate.stage_hit_counts.expanded_any_evidence_hit += any;
                aggregate.stage_hit_counts.expanded_all_evidence_hit += all;
            }
            "reranked" => {
                aggregate.stage_hit_counts.reranked_any_evidence_hit += any;
                aggregate.stage_hit_counts.reranked_all_evidence_hit += all;
            }
            "selected" => {
                aggregate.stage_hit_counts.selected_any_evidence_hit += any;
                aggregate.stage_hit_counts.selected_all_evidence_hit += all;
            }
            "projection_selected" => {
                aggregate
                    .stage_hit_counts
                    .projection_selected_any_evidence_hit += any;
                aggregate
                    .stage_hit_counts
                    .projection_selected_all_evidence_hit += all;
            }
            "rendered" => {
                aggregate.stage_hit_counts.rendered_any_evidence_hit += any;
                aggregate.stage_hit_counts.rendered_all_evidence_hit += all;
            }
            _ => unreachable!(),
        }
    }
    let canonical_groups = p7_canonical_groups(&expected.gold_sources);
    let expected_question_type = match canonical_groups.len() {
        0 => "no_gold",
        1 => "single_gold",
        _ => "multi_gold",
    };
    let typed_question_contract_present = row.get("question_evaluation").is_some();
    if (!canonical_groups.is_empty() || typed_question_contract_present)
        && row
            .get("stage_diagnostics")
            .and_then(|diagnostics| diagnostics.get("question_type"))
            .and_then(serde_json::Value::as_str)
            != Some(expected_question_type)
    {
        return Err(p7_provenance_error("detail question type mismatch"));
    }
    Ok(())
}

fn accumulate_p7_index_diagnostics(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
) -> Result<()> {
    let claimed = row
        .get("index_diagnostics")
        .ok_or_else(|| p7_provenance_error("detail index diagnostics missing"))?;
    let derived = p7_index_diagnostics_from_raw_reports(row)?;
    let derived_value = serde_json::to_value(&derived).map_err(|source| Error::Other {
        source: Box::new(source),
        stage: "p7_provenance_serialize_recomputed_index_diagnostics",
    })?;
    if claimed != &derived_value {
        return Err(p7_provenance_error(
            "per-question index diagnostics do not match raw SDK reports",
        ));
    }
    add_index_diagnostics(&mut aggregate.index_diagnostics, &derived);
    Ok(())
}

fn p7_index_diagnostics_from_raw_reports(
    row: &serde_json::Value,
) -> Result<W4ExternalNoisyIndexDiagnostics> {
    let graph = row
        .get("graph_index_report")
        .ok_or_else(|| p7_provenance_error("raw graph index report missing"))?;
    let facet = row
        .get("facet_index_report")
        .ok_or_else(|| p7_provenance_error("raw facet index report missing"))?;
    validate_p7_safe_graph_index_report(graph)?;
    p7_required_usize(
        graph,
        "index_doc_count",
        "graph index document count missing",
    )?;
    let matched_source_anchor_count = p7_required_usize(
        graph,
        "matched_source_anchor_count",
        "graph matched source anchor count missing",
    )?;
    let indexed_neighbor_count = p7_required_usize(
        graph,
        "indexed_neighbor_count",
        "graph indexed neighbor count missing",
    )?;
    let manifest_contract_verified = p7_required_bool(
        graph,
        "manifest_contract_verified",
        "graph manifest contract verification missing",
    )?;
    let selected_dependency_chain_verified = p7_required_bool(
        graph,
        "selected_dependency_chain_verified",
        "graph selected dependency chain verification missing",
    )?;
    let full_scope_closure_verified = p7_required_bool(
        graph,
        "full_scope_closure_verified",
        "graph full-scope closure verification missing",
    )?;
    let manifest_generation_present = p7_required_bool(
        graph,
        "manifest_generation_present",
        "graph manifest generation presence missing",
    )?;
    let graph_revision_present = p7_required_bool(
        graph,
        "graph_revision_present",
        "graph revision presence missing",
    )?;
    let scope_digest_present = p7_required_bool(
        graph,
        "scope_digest_present",
        "graph scope digest presence missing",
    )?;
    let maintenance_required = p7_required_bool(
        graph,
        "maintenance_required",
        "graph maintenance requirement missing",
    )?;
    let incident_present =
        p7_required_bool(graph, "incident_present", "graph incident presence missing")?;
    let read_path_mutation_delta = p7_required_usize(
        graph,
        "read_path_mutation_delta",
        "graph read-path mutation delta missing",
    )?;
    let graph_used = p7_required_bool(graph, "used", "graph index used claim missing")?;

    let posting_key_lookup_count = p7_required_usize(
        facet,
        "posting_key_lookup_count",
        "facet posting key lookup count missing",
    )?;
    let manifest_matched_posting_count = p7_required_usize(
        facet,
        "manifest_matched_posting_count",
        "facet manifest-matched posting count missing",
    )?;
    let posting_doc_read_count = p7_required_usize(
        facet,
        "posting_doc_read_count",
        "facet posting read count missing",
    )?;
    let owner_key_lookup_count = p7_required_usize(
        facet,
        "owner_key_lookup_count",
        "facet owner key lookup count missing",
    )?;
    let owner_doc_read_count = p7_required_usize(
        facet,
        "owner_doc_read_count",
        "facet owner read count missing",
    )?;
    p7_required_usize(
        facet,
        "manifest_owner_doc_count",
        "facet manifest owner document count missing",
    )?;
    p7_required_usize(
        facet,
        "manifest_posting_doc_count",
        "facet manifest posting document count missing",
    )?;
    p7_required_usize(facet, "render_growth", "facet render growth claim missing")?;
    let manifest_integrity_verified = p7_required_bool(
        facet,
        "manifest_integrity_verified",
        "facet manifest integrity claim missing",
    )?;
    let facet_used = p7_required_bool(facet, "used", "facet index used claim missing")?;
    let facet_failure_count =
        p7_required_usize(facet, "failure_count", "facet failure count missing")?;
    let facet_integrity_failure_count = p7_required_usize(
        facet,
        "integrity_failure_count",
        "facet integrity failure count missing",
    )?;
    if facet_integrity_failure_count > facet_failure_count {
        return Err(p7_provenance_error(
            "facet integrity failure count exceeds raw failures",
        ));
    }
    if facet_used
        && (posting_key_lookup_count == 0
            || manifest_matched_posting_count > posting_key_lookup_count)
    {
        return Err(p7_provenance_error(
            "used facet report has an invalid posting lookup proof",
        ));
    }
    if facet_used
        && manifest_integrity_verified
        && (posting_doc_read_count != manifest_matched_posting_count
            || owner_doc_read_count != owner_key_lookup_count
            || (manifest_matched_posting_count == 0
                && (owner_key_lookup_count != 0 || owner_doc_read_count != 0))
            || (manifest_matched_posting_count > 0 && owner_key_lookup_count == 0))
    {
        return Err(p7_provenance_error(
            "verified facet report has an inconsistent bounded read proof",
        ));
    }
    Ok(W4ExternalNoisyIndexDiagnostics {
        questions_with_index_report: 1,
        index_used_questions: usize::from(graph_used),
        fallback_full_scan_questions: usize::from(p7_required_bool(
            graph,
            "fallback_full_scan",
            "graph fallback claim missing",
        )?),
        source_candidate_count: p7_required_usize(
            graph,
            "source_candidate_count",
            "graph source candidate count missing",
        )?,
        matched_source_anchor_count,
        unmatched_source_anchor_count: p7_required_usize(
            graph,
            "unmatched_source_anchor_count",
            "graph unmatched source anchor count missing",
        )?,
        indexed_neighbor_count,
        filtered_node_count: p7_required_usize(
            graph,
            "filtered_node_count",
            "graph filtered node count missing",
        )?,
        filtered_edge_count: p7_required_usize(
            graph,
            "filtered_edge_count",
            "graph filtered edge count missing",
        )?,
        filtered_backlink_count: p7_required_usize(
            graph,
            "filtered_backlink_count",
            "graph filtered backlink count missing",
        )?,
        failure_count: p7_required_usize(graph, "failure_count", "graph failure count missing")?,
        graph_manifest_contract_verified_questions: usize::from(
            graph_used && manifest_contract_verified,
        ),
        graph_selected_dependency_chain_verified_questions: usize::from(
            graph_used && selected_dependency_chain_verified,
        ),
        graph_full_scope_closure_verified_questions: usize::from(
            graph_used && full_scope_closure_verified,
        ),
        graph_manifest_generation_present_questions: usize::from(
            graph_used && manifest_generation_present,
        ),
        graph_revision_present_questions: usize::from(graph_used && graph_revision_present),
        graph_scope_digest_present_questions: usize::from(graph_used && scope_digest_present),
        graph_maintenance_required_questions: usize::from(maintenance_required),
        graph_incident_questions: usize::from(incident_present),
        graph_read_path_mutation_delta: read_path_mutation_delta,
        facet_questions_with_index_report: 1,
        facet_index_used_questions: usize::from(facet_used),
        facet_report_only_questions: usize::from(p7_required_bool(
            facet,
            "report_only",
            "facet report-only claim missing",
        )?),
        facet_fallback_full_scan_questions: usize::from(p7_required_bool(
            facet,
            "fallback_full_scan",
            "facet fallback claim missing",
        )?),
        facet_source_candidate_count: p7_required_usize(
            facet,
            "source_candidate_count",
            "facet source candidate count missing",
        )?,
        facet_matched_source_candidate_count: p7_required_usize(
            facet,
            "matched_source_candidate_count",
            "facet matched source candidate count missing",
        )?,
        facet_posting_key_lookup_count: posting_key_lookup_count,
        facet_manifest_matched_posting_count: manifest_matched_posting_count,
        facet_posting_doc_read_count: posting_doc_read_count,
        facet_owner_key_lookup_count: owner_key_lookup_count,
        facet_owner_doc_read_count: owner_doc_read_count,
        facet_zero_posting_key_lookup_questions: usize::from(
            facet_used && posting_key_lookup_count == 0,
        ),
        facet_clean_zero_hit_questions: usize::from(
            facet_used
                && manifest_integrity_verified
                && manifest_matched_posting_count == 0
                && posting_doc_read_count == 0
                && owner_key_lookup_count == 0
                && owner_doc_read_count == 0,
        ),
        facet_manifest_integrity_verified_questions: usize::from(
            facet_used && manifest_integrity_verified,
        ),
        facet_manifest_integrity_failure_count: usize::from(
            facet_used && !manifest_integrity_verified,
        ),
        facet_exact_match_count: p7_required_usize(
            facet,
            "exact_facet_match_count",
            "facet exact match count missing",
        )?,
        facet_expanded_match_count: p7_required_usize(
            facet,
            "expanded_facet_match_count",
            "facet expanded match count missing",
        )?,
        facet_failure_count: facet_integrity_failure_count,
    })
}

fn validate_p7_safe_graph_index_report(report: &serde_json::Value) -> Result<()> {
    let fields = report
        .as_object()
        .ok_or_else(|| p7_provenance_error("raw graph index report must be an object"))?;
    const FORBIDDEN_RAW_FIELDS: [&str; 8] = [
        "owner",
        "manifest_generation",
        "graph_revision",
        "scope_digest",
        "incident_token",
        "source_anchor_ids",
        "unmatched_source_anchor_ids",
        "expanded_node_ids",
    ];
    if fields.keys().any(|field| {
        FORBIDDEN_RAW_FIELDS.contains(&field.as_str())
            || field.ends_with("_id")
            || field.ends_with("_ids")
    }) {
        return Err(p7_provenance_error(
            "raw graph index report exposes a digest, token, or raw id",
        ));
    }
    if fields
        .values()
        .any(|value| !value.is_boolean() && value.as_u64().is_none())
    {
        return Err(p7_provenance_error(
            "raw graph index report must contain only safe booleans and counters",
        ));
    }
    Ok(())
}

fn accumulate_p7_w41_diagnostics(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    const STAGES: [&str; 5] = ["source", "expanded", "reranked", "selected", "rendered"];
    let diagnostics = row
        .get("stage_diagnostics")
        .ok_or_else(|| p7_provenance_error("stage diagnostics missing"))?;
    let gold_groups = p7_canonical_groups(&expected.gold_sources);
    let question_type = match gold_groups.len() {
        0 => "no_gold",
        1 => "single_gold",
        _ => "multi_gold",
    };
    let diagnostic_gold = p7_string_array(
        diagnostics
            .get("gold_evidence_refs")
            .ok_or_else(|| p7_provenance_error("W4.1 gold evidence refs missing"))?,
        "W4.1 gold evidence refs must be strings",
    )?;
    if p7_required_str(diagnostics, "suite", "W4.1 suite missing")?
        != p7_required_str(row, "suite", "detail suite missing")?
        || p7_required_str(diagnostics, "question_id", "W4.1 question_id missing")?
            != expected.question_id
        || p7_required_str(diagnostics, "question_type", "W4.1 question type missing")?
            != question_type
        || p7_required_usize(diagnostics, "evidence_count", "W4.1 evidence count missing")?
            != gold_groups.len()
        || p7_canonical_groups(&diagnostic_gold) != gold_groups
    {
        return Err(p7_provenance_error("W4.1 detail identity mismatch"));
    }

    let stage_candidates = [
        ("source", p7_stage_candidates(row, "source")?),
        ("expanded", p7_stage_candidates(row, "expanded")?),
        ("reranked", p7_stage_candidates(row, "reranked")?),
        ("selected", p7_stage_candidates(row, "eval_selected")?),
        ("rendered", p7_stage_candidates(row, "eval_rendered")?),
    ];
    let mut derived_matched = BTreeMap::new();
    let mut derived_missing = BTreeMap::new();
    let mut derived_ranks = BTreeMap::new();
    let mut first_any = None;
    let mut first_all = None;
    for (stage, candidates) in &stage_candidates {
        let matches = p7_match_gold_groups(&expected.gold_sources, candidates);
        let matched = matches
            .iter()
            .map(|(_, group)| group.clone())
            .collect::<BTreeSet<_>>();
        let missing = gold_groups
            .difference(&matched)
            .cloned()
            .collect::<BTreeSet<_>>();
        let ranks = matches
            .into_iter()
            .filter_map(|(candidate_id, group)| {
                candidates
                    .iter()
                    .position(|candidate| candidate.candidate_id == candidate_id)
                    .map(|rank| (group, rank + 1))
            })
            .collect::<BTreeMap<_, _>>();
        if first_any.is_none() && !matched.is_empty() {
            first_any = Some((*stage).to_string());
        }
        if first_all.is_none() && !gold_groups.is_empty() && missing.is_empty() {
            first_all = Some((*stage).to_string());
        }
        derived_matched.insert((*stage).to_string(), matched);
        derived_missing.insert((*stage).to_string(), missing);
        derived_ranks.insert((*stage).to_string(), ranks);
    }
    if p7_optional_str(
        diagnostics,
        "first_any_hit_stage",
        "W4.1 first any-hit stage missing",
    )? != first_any.as_deref()
        || p7_optional_str(
            diagnostics,
            "first_all_hit_stage",
            "W4.1 first all-hit stage missing",
        )? != first_all.as_deref()
        || p7_stage_evidence_groups(diagnostics, "matched_gold_by_stage")? != derived_matched
        || p7_stage_evidence_groups(diagnostics, "missing_gold_by_stage")? != derived_missing
    {
        return Err(p7_provenance_error(
            "W4.1 stage claims do not match detail stage sources",
        ));
    }
    let miss_after_expanded = !gold_groups.is_empty()
        && derived_missing
            .get("expanded")
            .is_some_and(|missing| !missing.is_empty());
    if p7_required_bool(
        diagnostics,
        "miss_after_expanded",
        "W4.1 expanded miss claim missing",
    )? != miss_after_expanded
    {
        return Err(p7_provenance_error(
            "W4.1 expanded miss claim does not match detail stages",
        ));
    }

    let ranks = p7_required_array(diagnostics, "gold_rank_by_stage", "W4.1 gold ranks missing")?;
    let mut seen_ranks = BTreeSet::new();
    let mut found_count = 0_usize;
    let mut missing_count = 0_usize;
    let mut rank_sum = 0_usize;
    for rank in ranks {
        let stage = p7_required_str(rank, "stage", "W4.1 gold rank stage missing")?;
        if !STAGES.contains(&stage) {
            return Err(p7_provenance_error("W4.1 gold rank stage is unknown"));
        }
        let evidence_ref =
            p7_required_str(rank, "evidence_ref", "W4.1 gold rank evidence ref missing")?;
        let evidence_groups = p7_canonical_groups(&[evidence_ref.to_string()]);
        if evidence_groups.len() != 1 {
            return Err(p7_provenance_error(
                "W4.1 gold rank evidence ref is not exact",
            ));
        }
        let evidence_group = evidence_groups
            .iter()
            .next()
            .expect("single canonical evidence group")
            .clone();
        if !gold_groups.contains(&evidence_group)
            || !seen_ranks.insert((stage.to_string(), evidence_group.clone()))
        {
            return Err(p7_provenance_error(
                "W4.1 gold rank identity is missing or duplicated",
            ));
        }
        let rank_value = match rank.get("rank") {
            Some(value) if value.is_null() => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| p7_provenance_error("W4.1 gold rank is invalid"))?,
            ),
            None => return Err(p7_provenance_error("W4.1 gold rank value missing")),
        };
        let expected_rank = derived_ranks
            .get(stage)
            .and_then(|ranks| ranks.get(&evidence_group))
            .copied();
        if expected_rank != rank_value {
            return Err(p7_provenance_error(
                "W4.1 gold rank does not match candidate-bound stage evidence",
            ));
        }
        if let Some(rank_value) = rank_value {
            found_count = found_count.saturating_add(1);
            rank_sum = rank_sum.saturating_add(rank_value);
        } else {
            missing_count = missing_count.saturating_add(1);
        }
    }
    if seen_ranks.len() != STAGES.len().saturating_mul(gold_groups.len()) {
        return Err(p7_provenance_error("W4.1 gold rank matrix is incomplete"));
    }

    let w41 = &mut aggregate.w4_1_diagnostics;
    w41.questions_with_w4_1_diagnostics = w41.questions_with_w4_1_diagnostics.saturating_add(1);
    if let Some(stage) = first_any {
        p7_increment(&mut w41.first_any_hit_stage_counts, &stage, 1);
    }
    if let Some(stage) = first_all {
        p7_increment(&mut w41.first_all_hit_stage_counts, &stage, 1);
    }
    for stage in STAGES {
        p7_increment(
            &mut w41.missing_gold_by_stage_counts,
            stage,
            derived_missing.get(stage).map_or(0, BTreeSet::len),
        );
    }
    w41.miss_after_expanded_count = w41
        .miss_after_expanded_count
        .saturating_add(usize::from(miss_after_expanded));
    w41.gold_rank_found_count = w41.gold_rank_found_count.saturating_add(found_count);
    w41.gold_rank_missing_count = w41.gold_rank_missing_count.saturating_add(missing_count);
    w41.gold_rank_sum = w41.gold_rank_sum.saturating_add(rank_sum);
    w41.truncated_count = w41.truncated_count.saturating_add(p7_required_usize(
        diagnostics,
        "truncated_count",
        "W4.1 truncated count missing",
    )?);
    for reason in p7_string_array(
        diagnostics
            .get("blocked_reasons")
            .ok_or_else(|| p7_provenance_error("W4.1 blocked reasons missing"))?,
        "W4.1 blocked reasons must be strings",
    )? {
        p7_increment(&mut w41.blocked_reason_counts, &reason, 1);
    }
    p7_increment(&mut w41.question_type_counts, question_type, 1);
    p7_increment(
        &mut w41.evidence_count_buckets,
        p7_evidence_count_bucket(gold_groups.len()),
        1,
    );
    let signature = p7_stage_candidates(row, "source")?
        .into_iter()
        .map(|candidate| {
            format!(
                "{}:{}",
                candidate.candidate_id,
                candidate.canonical_evidence_groups.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    p7_increment(&mut aggregate.source_signature_counts, &signature, 1);
    aggregate.refresh_source_signature_diagnostics();
    Ok(())
}

fn p7_stage_evidence_groups(
    diagnostics: &serde_json::Value,
    field: &str,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    const STAGES: [&str; 5] = ["source", "expanded", "reranked", "selected", "rendered"];
    let mut by_stage = BTreeMap::new();
    for entry in p7_required_array(diagnostics, field, "W4.1 stage evidence matrix missing")? {
        let stage = p7_required_str(entry, "stage", "W4.1 stage evidence name missing")?;
        if !STAGES.contains(&stage) {
            return Err(p7_provenance_error("W4.1 stage evidence name is unknown"));
        }
        let evidence_refs = p7_string_array(
            entry
                .get("evidence_refs")
                .ok_or_else(|| p7_provenance_error("W4.1 stage evidence refs missing"))?,
            "W4.1 stage evidence refs must be strings",
        )?;
        if by_stage
            .insert(stage.to_string(), p7_canonical_groups(&evidence_refs))
            .is_some()
        {
            return Err(p7_provenance_error("W4.1 stage evidence is duplicated"));
        }
    }
    if by_stage.len() != STAGES.len() {
        return Err(p7_provenance_error(
            "W4.1 stage evidence matrix is incomplete",
        ));
    }
    Ok(by_stage)
}

fn p7_evidence_count_bucket(count: usize) -> &'static str {
    match count {
        0 => "0",
        1 => "1",
        2 | 3 => "2_3",
        _ => "4_plus",
    }
}

fn accumulate_p7_ablation(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    let report = row
        .get("ablation_report")
        .ok_or_else(|| p7_provenance_error("detail ablation report missing"))?;
    if p7_required_str(report, "method", "ablation method missing")? != P7_ABLATION_METHOD {
        return Err(p7_provenance_error("unexpected ablation method"));
    }
    let required_slices = p7_row_string_array(report, "required_slices")?;
    let expected_slice_set = P7_REQUIRED_ABLATION_SLICES
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let required_slice_set = required_slices.iter().cloned().collect::<BTreeSet<_>>();
    if required_slices.len() != P7_REQUIRED_ABLATION_SLICES.len()
        || required_slice_set != expected_slice_set
    {
        return Err(p7_provenance_error(
            "ablation required slices are not the exact P7 set",
        ));
    }
    let slices = report
        .get("slices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| p7_provenance_error("detail ablation slices missing"))?;
    if slices.len() != P7_REQUIRED_ABLATION_SLICES.len() {
        return Err(p7_provenance_error(
            "ablation slice count does not match the P7 contract",
        ));
    }
    let mut actual_slice_set = BTreeSet::new();
    for slice in slices {
        let name = p7_required_str(slice, "name", "ablation slice name missing")?;
        if !actual_slice_set.insert(name.to_string()) || !expected_slice_set.contains(name) {
            return Err(p7_provenance_error(
                "ablation slices are duplicated or outside the P7 contract",
            ));
        }
        if p7_required_bool(slice, "feature_enabled", "ablation feature state missing")?
            || !p7_required_bool(
                slice,
                "report_available",
                "ablation report availability missing",
            )?
        {
            return Err(p7_provenance_error(
                "ablation slice is not a complete disabled off-run",
            ));
        }
        if !p7_required_bool(
            slice,
            "candidate_boundary_proven",
            "ablation candidate boundary proof missing",
        )? {
            return Err(p7_provenance_error(
                "ablation candidate boundary is not proven",
            ));
        }
    }
    if actual_slice_set != expected_slice_set {
        return Err(p7_provenance_error(
            "ablation slices are not the exact P7 set",
        ));
    }
    let diagnostics = &mut aggregate.facet_ablation;
    diagnostics.questions_with_ablation_report += 1;
    p7_increment(&mut diagnostics.method_counts, P7_ABLATION_METHOD, 1);
    let claimed_report_contribution = p7_required_bool(
        report,
        "delivery_contribution_proven",
        "ablation contribution flag missing",
    )?;
    let claimed_report_render_growth =
        p7_required_usize(report, "render_growth", "ablation render growth missing")?;
    for required in required_slices {
        p7_increment(&mut diagnostics.required_slice_counts, &required, 1);
    }
    for reason in p7_row_string_array(report, "blocked_reasons")? {
        p7_increment(&mut diagnostics.blocked_reason_counts, &reason, 1);
    }
    let gold_groups = p7_canonical_groups(&expected.gold_sources);
    let evidence_index = p7_authoritative_evidence_index(
        row.get("evidence_ref_index")
            .ok_or_else(|| p7_provenance_error("SDK safe evidence index missing"))?,
    )?;
    let evidence_by_candidate = p7_candidate_evidence_map(&evidence_index);
    let baseline_selected_owner =
        p7_candidate_evidence_map(&p7_stage_candidates(row, "eval_selected")?);
    let baseline_rendered_owner =
        p7_candidate_evidence_map(&p7_stage_candidates(row, "eval_rendered")?);
    let mut computed_report_contribution = false;
    let mut computed_report_render_growth = 0_usize;
    for slice in slices {
        let name = p7_required_str(slice, "name", "ablation slice name missing")?;
        let report_available = p7_required_bool(
            slice,
            "report_available",
            "ablation report availability missing",
        )?;
        if report_available {
            p7_increment(&mut diagnostics.report_available_slice_counts, name, 1);
        }
        let baseline_selected = p7_ablation_candidates(slice, "baseline_selected_candidates")?;
        let off_selected = p7_ablation_candidates(slice, "off_run_selected_candidates")?;
        let baseline_rendered = p7_ablation_candidates(slice, "baseline_rendered_candidates")?;
        let off_rendered = p7_ablation_candidates(slice, "off_run_rendered_candidates")?;
        if p7_candidate_evidence_map(&baseline_selected) != baseline_selected_owner
            || p7_candidate_evidence_map(&baseline_rendered) != baseline_rendered_owner
        {
            return Err(p7_provenance_error(
                "ablation baseline candidates differ from raw SDK stage candidates",
            ));
        }
        p7_require_full_candidate_bindings(&evidence_by_candidate, &off_selected)?;
        p7_require_rendered_candidate_bindings(
            &evidence_by_candidate,
            &p7_candidate_evidence_map(&baseline_selected),
            &baseline_rendered,
        )?;
        p7_require_rendered_candidate_bindings(
            &evidence_by_candidate,
            &p7_candidate_evidence_map(&off_selected),
            &off_rendered,
        )?;
        for (field, candidates) in [
            ("baseline_selected_candidate_ids", &baseline_selected),
            ("off_run_selected_candidate_ids", &off_selected),
            ("baseline_rendered_candidate_ids", &baseline_rendered),
            ("off_run_rendered_candidate_ids", &off_rendered),
        ] {
            let claimed_ids = p7_row_string_array(slice, field)?;
            let candidate_ids = candidates
                .iter()
                .map(|candidate| candidate.candidate_id.clone())
                .collect::<Vec<_>>();
            if claimed_ids != candidate_ids {
                return Err(p7_provenance_error(
                    "SDK ablation candidate identity claim differs from candidate-bound report",
                ));
            }
        }
        for (field, candidates) in [
            ("baseline_selected_evidence_refs", &baseline_selected),
            ("off_run_selected_evidence_refs", &off_selected),
            ("baseline_rendered_evidence_refs", &baseline_rendered),
            ("off_run_rendered_evidence_refs", &off_rendered),
        ] {
            let claims = p7_row_string_array(slice, field)?;
            if claims
                .iter()
                .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
                || p7_canonical_groups(&claims)
                    != p7_candidate_evidence_groups(candidates)
                        .into_iter()
                        .collect()
            {
                return Err(p7_provenance_error(
                    "ablation flat evidence claim differs from candidate-bound report",
                ));
            }
        }
        if baseline_selected.len()
            != p7_required_usize(
                slice,
                "baseline_selected_candidate_count",
                "ablation baseline selected candidate count missing",
            )?
            || off_selected.len()
                != p7_required_usize(
                    slice,
                    "off_run_selected_candidate_count",
                    "ablation off-run selected candidate count missing",
                )?
            || baseline_rendered.len()
                != p7_required_usize(
                    slice,
                    "baseline_rendered_candidate_count",
                    "ablation baseline rendered candidate count missing",
                )?
            || off_rendered.len()
                != p7_required_usize(
                    slice,
                    "off_run_rendered_candidate_count",
                    "ablation off-run rendered candidate count missing",
                )?
        {
            return Err(p7_provenance_error(
                "ablation candidate counts differ from candidate-bound reports",
            ));
        }
        let delivery_affected_candidate_ids = p7_affected_ablation_candidate_ids(
            &baseline_selected,
            &off_selected,
            &baseline_rendered,
            &off_rendered,
        );
        let claimed_affected_ids = p7_row_string_array(slice, "delivery_affected_candidate_ids")?
            .into_iter()
            .collect::<BTreeSet<_>>();
        if claimed_affected_ids != delivery_affected_candidate_ids
            || p7_required_usize(
                slice,
                "delivery_affected_candidate_count",
                "ablation affected count missing",
            )? != delivery_affected_candidate_ids.len()
        {
            return Err(p7_provenance_error(
                "ablation affected candidate claim differs from candidate-bound reports",
            ));
        }
        let sdk_delivery_affected_candidate_count_claim = p7_required_usize(
            slice,
            "sdk_delivery_affected_candidate_count_claim",
            "SDK ablation affected candidate count claim missing",
        )?;
        if sdk_delivery_affected_candidate_count_claim != delivery_affected_candidate_ids.len() {
            return Err(p7_provenance_error(
                "SDK ablation affected candidate count differs from candidate-bound facts",
            ));
        }
        diagnostics.delivery_affected_candidate_occurrences = diagnostics
            .delivery_affected_candidate_occurrences
            .saturating_add(delivery_affected_candidate_ids.len());
        let baseline_selected_matches =
            p7_match_gold_groups(&expected.gold_sources, &baseline_selected);
        let off_selected_matches = p7_match_gold_groups(&expected.gold_sources, &off_selected);
        let baseline_rendered_matches =
            p7_match_gold_groups(&expected.gold_sources, &baseline_rendered);
        let off_rendered_matches = p7_match_gold_groups(&expected.gold_sources, &off_rendered);
        let selected_delta =
            baseline_selected_matches.len() as i64 - off_selected_matches.len() as i64;
        let rendered_delta =
            baseline_rendered_matches.len() as i64 - off_rendered_matches.len() as i64;
        let selected_all_hit_lost = !gold_groups.is_empty()
            && baseline_selected_matches.len() == gold_groups.len()
            && off_selected_matches.len() != gold_groups.len();
        let rendered_all_hit_lost = !gold_groups.is_empty()
            && baseline_rendered_matches.len() == gold_groups.len()
            && off_rendered_matches.len() != gold_groups.len();
        if p7_required_i64(
            slice,
            "selected_evidence_hit_delta",
            "ablation selected hit delta missing",
        )? != selected_delta
            || p7_required_bool(
                slice,
                "selected_all_hit_lost",
                "ablation selected all-hit flag missing",
            )? != selected_all_hit_lost
            || p7_required_i64(
                slice,
                "rendered_evidence_hit_delta",
                "ablation rendered hit delta missing",
            )? != rendered_delta
            || p7_required_bool(
                slice,
                "rendered_all_hit_lost",
                "ablation rendered all-hit flag missing",
            )? != rendered_all_hit_lost
        {
            return Err(p7_provenance_error(
                "ablation evidence facts do not match canonical exact refs",
            ));
        }

        let expanded_candidate_delta = p7_signed_usize_delta(
            p7_required_usize(
                slice,
                "baseline_expanded_candidate_count",
                "ablation baseline expanded candidate count missing",
            )?,
            p7_required_usize(
                slice,
                "off_run_expanded_candidate_count",
                "ablation off-run expanded candidate count missing",
            )?,
        )?;
        let selected_candidate_delta = p7_signed_usize_delta(
            p7_required_usize(
                slice,
                "baseline_selected_candidate_count",
                "ablation baseline selected candidate count missing",
            )?,
            p7_required_usize(
                slice,
                "off_run_selected_candidate_count",
                "ablation off-run selected candidate count missing",
            )?,
        )?;
        let rendered_candidate_delta = p7_signed_usize_delta(
            p7_required_usize(
                slice,
                "baseline_rendered_candidate_count",
                "ablation baseline rendered candidate count missing",
            )?,
            p7_required_usize(
                slice,
                "off_run_rendered_candidate_count",
                "ablation off-run rendered candidate count missing",
            )?,
        )?;
        let rendered_char_delta = p7_signed_usize_delta(
            p7_required_usize(
                slice,
                "baseline_rendered_chars",
                "ablation baseline rendered chars missing",
            )?,
            p7_required_usize(
                slice,
                "off_run_rendered_chars",
                "ablation off-run rendered chars missing",
            )?,
        )?;
        let render_growth = p7_required_usize(
            slice,
            "baseline_render_growth",
            "ablation baseline render growth missing",
        )?
        .max(p7_required_usize(
            slice,
            "off_run_render_growth",
            "ablation off-run render growth missing",
        )?);
        for (field, expected_delta) in [
            ("expanded_candidate_delta", expanded_candidate_delta),
            ("selected_candidate_delta", selected_candidate_delta),
            ("rendered_candidate_delta", rendered_candidate_delta),
            ("rendered_char_delta", rendered_char_delta),
        ] {
            if p7_required_i64(slice, field, "ablation numeric delta missing")? != expected_delta {
                return Err(p7_provenance_error(
                    "ablation numeric delta does not match raw facts",
                ));
            }
        }
        if p7_required_usize(
            slice,
            "render_growth",
            "ablation slice render growth missing",
        )? != render_growth
        {
            return Err(p7_provenance_error(
                "ablation render growth does not match raw facts",
            ));
        }
        let slice_blocked_reasons = p7_row_string_array(slice, "blocked_reasons")?;
        let delivery_contribution_proven = report_available
            && slice_blocked_reasons.is_empty()
            && (selected_delta > 0
                || rendered_delta > 0
                || selected_all_hit_lost
                || rendered_all_hit_lost);
        if p7_required_bool(
            slice,
            "delivery_contribution_proven",
            "ablation contribution proof missing",
        )? != delivery_contribution_proven
        {
            return Err(p7_provenance_error(
                "ablation contribution proof does not match raw evidence facts",
            ));
        }
        if delivery_contribution_proven {
            computed_report_contribution = true;
            p7_increment(
                &mut diagnostics.delivery_contribution_proven_slice_counts,
                name,
                1,
            );
        }
        computed_report_render_growth = computed_report_render_growth
            .checked_add(render_growth)
            .ok_or_else(|| p7_provenance_error("ablation render growth overflow"))?;

        p7_increment_i64(
            &mut diagnostics.selected_evidence_hit_delta,
            name,
            selected_delta,
        );
        p7_increment_i64(
            &mut diagnostics.rendered_evidence_hit_delta,
            name,
            rendered_delta,
        );
        if selected_all_hit_lost {
            p7_increment(&mut diagnostics.selected_all_hit_loss_count, name, 1);
            if name == "evidence_family_rotation_off" {
                p7_increment(
                    &mut diagnostics.evidence_family_rotation_selected_all_hit_loss_count,
                    name,
                    1,
                );
            }
        }
        if rendered_all_hit_lost {
            p7_increment(&mut diagnostics.rendered_all_hit_loss_count, name, 1);
        }
        p7_increment_i64(
            &mut diagnostics.expanded_candidate_delta,
            name,
            expanded_candidate_delta,
        );
        p7_increment_i64(
            &mut diagnostics.selected_candidate_delta,
            name,
            selected_candidate_delta,
        );
        p7_increment_i64(
            &mut diagnostics.rendered_candidate_delta,
            name,
            rendered_candidate_delta,
        );
        p7_increment_i64(
            &mut diagnostics.rendered_char_delta,
            name,
            rendered_char_delta,
        );
        for reason in slice_blocked_reasons {
            p7_increment(&mut diagnostics.blocked_reason_counts, &reason, 1);
        }
    }
    if claimed_report_contribution != computed_report_contribution
        || claimed_report_render_growth != computed_report_render_growth
    {
        return Err(p7_provenance_error(
            "ablation report aggregate does not match recomputed slices",
        ));
    }
    diagnostics.delivery_contribution_proven_questions += usize::from(computed_report_contribution);
    diagnostics.render_growth = diagnostics
        .render_growth
        .checked_add(computed_report_render_growth)
        .ok_or_else(|| p7_provenance_error("ablation aggregate render growth overflow"))?;
    Ok(())
}

fn accumulate_p7_loss(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    let ledger = row
        .get("p7_loss_ledger")
        .ok_or_else(|| p7_provenance_error("detail P7 loss ledger missing"))?;
    let claimed_expanded = p7_required_array(
        ledger,
        "expanded_hit_selected_miss",
        "expanded-selected loss entries missing",
    )?;
    let claimed_eval_rendered = p7_required_array(
        ledger,
        "selected_hit_rendered_miss",
        "selected-rendered loss entries missing",
    )?;
    let gold_groups = p7_canonical_groups(&expected.gold_sources);
    let expanded_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "expanded")?,
    );
    let selected_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "eval_selected")?,
    );
    let eval_rendered_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "eval_rendered")?,
    );
    let projection_selected_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "projection_selected")?,
    );
    let final_rendered_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "final_rendered")?,
    );
    let expanded_selected_loss =
        p7_stage_loss_groups(&gold_groups, &expanded_groups, &selected_groups);
    let eval_selected_rendered_loss =
        p7_stage_loss_groups(&gold_groups, &selected_groups, &eval_rendered_groups);
    let eval_selected_projection_selected_loss =
        p7_stage_loss_groups(&gold_groups, &selected_groups, &projection_selected_groups);
    let final_selected_rendered_loss = p7_stage_loss_groups(
        &gold_groups,
        &projection_selected_groups,
        &final_rendered_groups,
    );
    if p7_loss_entry_groups(claimed_expanded)? != expanded_selected_loss
        || p7_loss_entry_groups(claimed_eval_rendered)? != eval_selected_rendered_loss
    {
        return Err(p7_provenance_error(
            "SDK loss ledger does not match independently recomputed eval stages",
        ));
    }
    let expanded_candidates = p7_stage_candidates(row, "expanded")?;
    let reranked_candidates = p7_stage_candidates(row, "reranked")?;
    let selected_candidates = p7_stage_candidates(row, "eval_selected")?;
    let rendered_candidates = p7_stage_candidates(row, "eval_rendered")?;
    let eval_delivery = row
        .get("eval_delivery_report")
        .ok_or_else(|| p7_provenance_error("eval delivery report missing"))?;
    validate_p7_loss_entries(
        claimed_expanded,
        &expanded_selected_loss,
        &expanded_candidates,
        &reranked_candidates,
        &selected_candidates,
        &rendered_candidates,
        eval_delivery,
    )?;
    validate_p7_loss_entries(
        claimed_eval_rendered,
        &eval_selected_rendered_loss,
        &expanded_candidates,
        &reranked_candidates,
        &selected_candidates,
        &rendered_candidates,
        eval_delivery,
    )?;
    let diagnostics = &mut aggregate.p7_loss_ledger;
    diagnostics.questions_with_loss_ledger += 1;
    diagnostics.expanded_hit_selected_miss_questions +=
        usize::from(!expanded_selected_loss.is_empty());
    diagnostics.eval_selected_hit_rendered_miss_questions +=
        usize::from(!eval_selected_rendered_loss.is_empty());
    diagnostics.eval_selected_hit_projection_selected_miss_questions +=
        usize::from(!eval_selected_projection_selected_loss.is_empty());
    diagnostics.selected_hit_final_rendered_miss_questions +=
        usize::from(!final_selected_rendered_loss.is_empty());
    diagnostics.expanded_hit_selected_miss_evidence = diagnostics
        .expanded_hit_selected_miss_evidence
        .saturating_add(expanded_selected_loss.len());
    diagnostics.eval_selected_hit_rendered_miss_evidence = diagnostics
        .eval_selected_hit_rendered_miss_evidence
        .saturating_add(eval_selected_rendered_loss.len());
    diagnostics.eval_selected_hit_projection_selected_miss_evidence = diagnostics
        .eval_selected_hit_projection_selected_miss_evidence
        .saturating_add(eval_selected_projection_selected_loss.len());
    diagnostics.selected_hit_final_rendered_miss_evidence = diagnostics
        .selected_hit_final_rendered_miss_evidence
        .saturating_add(final_selected_rendered_loss.len());
    diagnostics.eval_truncated_count =
        diagnostics
            .eval_truncated_count
            .saturating_add(p7_required_usize(
                ledger,
                "truncated_count",
                "P7 loss truncation count missing",
            )?);
    for reason in p7_row_string_array(ledger, "blocked_reasons")? {
        p7_increment(&mut diagnostics.eval_blocked_reason_counts, &reason, 1);
    }
    Ok(())
}

fn accumulate_p7_production_delivery(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
) -> Result<()> {
    let eval_delivery = row
        .get("eval_delivery_report")
        .ok_or_else(|| p7_provenance_error("eval delivery report missing"))?;
    let final_delivery = row
        .get("final_projection_delivery_report")
        .ok_or_else(|| p7_provenance_error("final projection delivery report missing"))?;
    let privacy = row
        .get("privacy_report")
        .ok_or_else(|| p7_provenance_error("detail privacy report missing"))?;
    let diagnostics = &mut aggregate.p7_production_delivery;
    diagnostics.questions_with_delivery_report += 1;

    let eval_selected = p7_stage_candidates(row, "eval_selected")?
        .into_iter()
        .map(|candidate| candidate.candidate_id)
        .collect::<BTreeSet<_>>();
    let delivery_selected = p7_row_string_array(eval_delivery, "selected_candidate_ids")?
        .into_iter()
        .collect::<BTreeSet<_>>();
    diagnostics.eval_selected_matches_delivery_questions +=
        usize::from(eval_selected == delivery_selected);
    diagnostics.projection_selected_sources_proven_questions += usize::from(
        p7_candidate_evidence_map(&p7_stage_candidates(row, "projection_selected")?)
            == p7_candidate_evidence_map(&p7_selected_candidates_from_delivery(final_delivery)?),
    );
    let eval_rendered = p7_stage_candidates(row, "eval_rendered")?
        .into_iter()
        .map(|candidate| candidate.candidate_id)
        .collect::<BTreeSet<_>>();
    let delivery_rendered = p7_delivery_candidate_ids(eval_delivery)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    diagnostics.eval_rendered_matches_delivery_questions +=
        usize::from(eval_rendered == delivery_rendered);

    let projection_proven = p7_required_bool(
        row,
        "projection_delivery_proven",
        "projection delivery proof flag missing",
    )?;
    let sdk_manifest = row
        .get("sdk_projection_delivery_manifest")
        .ok_or_else(|| p7_provenance_error("SDK projection delivery manifest missing"))?;
    validate_p7_sdk_projection_delivery_manifest(sdk_manifest, final_delivery)?;
    validate_p7_runner_projection_digest_observation(
        row.get("runner_projection_digest_observation")
            .ok_or_else(|| p7_provenance_error("runner projection digest observation missing"))?,
        sdk_manifest,
    )?;
    if !projection_proven {
        return Err(p7_provenance_error(
            "runner projection delivery flag contradicts the SDK manifest",
        ));
    }
    diagnostics.projection_delivery_proof_questions += 1;
    let integrity = row
        .get("final_projection_integrity")
        .ok_or_else(|| p7_provenance_error("final projection integrity missing"))?;
    let checked_surfaces = p7_row_string_array(integrity, "checked_surfaces")?;
    let checked_surface_set = checked_surfaces.iter().cloned().collect::<BTreeSet<_>>();
    let expected_surfaces = [
        "prompt",
        "ui_api",
        "operator_raw",
        "gateway_raw_audit",
        "shared_fact_surface",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    let surface_reports = p7_required_array(
        integrity,
        "surface_reports",
        "final projection surface reports missing",
    )?;
    let mut reported_surfaces = BTreeSet::new();
    let mut recomputed_violation_count = 0_usize;
    for report in surface_reports {
        let surface = p7_required_str(
            report,
            "surface",
            "final projection integrity surface name missing",
        )?;
        if !reported_surfaces.insert(surface.to_string()) {
            return Err(p7_provenance_error(
                "final projection integrity surface is duplicated",
            ));
        }
        let protected_exact_echo_count = p7_required_usize(
            report,
            "protected_exact_echo_count",
            "surface protected exact echo count missing",
        )?;
        let forbidden_marker_count = p7_required_usize(
            report,
            "forbidden_marker_count",
            "surface forbidden marker count missing",
        )?;
        let violation_count =
            p7_required_usize(report, "violation_count", "surface violation count missing")?;
        let recomputed_surface_violations = protected_exact_echo_count
            .checked_add(forbidden_marker_count)
            .ok_or_else(|| p7_provenance_error("surface violation count overflow"))?;
        if violation_count != recomputed_surface_violations
            || p7_required_bool(report, "passed", "surface integrity pass flag missing")?
                != (violation_count == 0)
        {
            return Err(p7_provenance_error(
                "surface integrity facts are internally inconsistent",
            ));
        }
        recomputed_violation_count = recomputed_violation_count
            .checked_add(violation_count)
            .ok_or_else(|| p7_provenance_error("top-level violation count overflow"))?;
    }
    if checked_surfaces.len() != checked_surface_set.len()
        || checked_surface_set != expected_surfaces
        || surface_reports.len() != expected_surfaces.len()
        || reported_surfaces != expected_surfaces
    {
        return Err(p7_provenance_error(
            "final projection integrity surfaces do not match the SDK contract",
        ));
    }
    let integrity_passed = p7_required_bool(
        integrity,
        "passed",
        "final projection integrity pass flag missing",
    )?;
    let raw_private_violation_count = p7_required_usize(
        integrity,
        "raw_private_violation_count",
        "final projection raw private violation count missing",
    )?;
    if raw_private_violation_count != recomputed_violation_count
        || integrity_passed != (recomputed_violation_count == 0)
    {
        return Err(p7_provenance_error(
            "final projection integrity aggregate contradicts surface reports",
        ));
    }
    diagnostics.final_projection_integrity_questions += 1;
    diagnostics.final_projection_integrity_passed_questions += usize::from(integrity_passed);
    diagnostics.final_projection_raw_private_violation_count = diagnostics
        .final_projection_raw_private_violation_count
        .saturating_add(raw_private_violation_count);
    diagnostics.final_projection_blocked_source_count = diagnostics
        .final_projection_blocked_source_count
        .saturating_add(p7_required_usize(
            integrity,
            "blocked_source_count",
            "final projection blocked source count missing",
        )?);
    diagnostics.final_projection_redacted_source_count = diagnostics
        .final_projection_redacted_source_count
        .saturating_add(p7_required_usize(
            integrity,
            "redacted_source_count",
            "final projection redacted source count missing",
        )?);
    if !integrity_passed {
        p7_increment(
            &mut diagnostics.blocked_reason_counts,
            "final_projection_private_disclosure_integrity_failed",
            1,
        );
    }
    p7_increment(
        &mut diagnostics.schema_version_counts,
        &p7_required_usize(
            final_delivery,
            "schema_version",
            "final delivery schema version missing",
        )?
        .to_string(),
        1,
    );
    diagnostics.render_growth = diagnostics.render_growth.saturating_add(p7_required_usize(
        final_delivery,
        "render_growth",
        "final delivery render growth missing",
    )?);

    let private_raw = p7_required_usize(
        privacy,
        "private_raw_candidate_count",
        "privacy private raw count missing",
    )?;
    let privacy_passed = p7_required_bool(privacy, "passed", "privacy pass flag missing")?;
    let privacy_failures = p7_row_string_array(privacy, "failures")?;
    if privacy_passed != (private_raw == 0 && privacy_failures.is_empty()) {
        return Err(p7_provenance_error(
            "privacy pass flag contradicts validator failures or private raw candidates",
        ));
    }
    diagnostics.raw_soul_private_material_count = diagnostics
        .raw_soul_private_material_count
        .saturating_add(private_raw);
    if !privacy_passed {
        diagnostics.privacy_leak_count += 1;
    }
    for failure in privacy_failures {
        if failure.contains("cross_subject") {
            diagnostics.cross_subject_leak_count += 1;
        }
        if failure.contains("raw_soul_private") {
            diagnostics.raw_soul_private_material_count += 1;
        }
    }
    for capsule in p7_required_array(
        final_delivery,
        "rendered_capsules",
        "final rendered capsules missing",
    )? {
        validate_p7_safe_source_locator_view(
            capsule
                .get("source_locator_view")
                .ok_or_else(|| p7_provenance_error("capsule source locator view missing"))?,
        )?;
        let redaction = p7_required_str(
            capsule,
            "redaction_state",
            "capsule redaction state missing",
        )?;
        let shared_fact_surface_allowed = p7_required_bool(
            capsule,
            "shared_fact_surface_allowed",
            "capsule shared fact surface eligibility missing",
        )?;
        let evidence_views = p7_required_array(
            capsule,
            "evidence_ref_views",
            "capsule evidence ref views missing",
        )?;
        let mut safe_references = Vec::new();
        for view in evidence_views {
            if let Some(reference) = validate_p7_safe_evidence_ref_view(view)? {
                safe_references.push(reference);
            }
        }
        let visible_references = p7_row_string_array(capsule, "visible_evidence_refs")?;
        let safe_groups = p7_canonical_groups(&safe_references);
        let visible_groups = p7_canonical_groups(&visible_references);
        if safe_references.len() != safe_groups.len()
            || visible_references.len() != visible_groups.len()
            || safe_groups != visible_groups
        {
            return Err(p7_provenance_error(
                "capsule visible evidence refs differ from safe locator views",
            ));
        }
        if matches!(
            redaction,
            "private_garden" | "soul_private" | "operator_diagnostic"
        ) || (shared_fact_surface_allowed && redaction != "public_runtime")
        {
            diagnostics.privacy_leak_count += 1;
        }
    }
    for failure in p7_row_string_array(final_delivery, "integrity_failures")? {
        p7_increment(&mut diagnostics.blocked_reason_counts, &failure, 1);
    }
    for reason in p7_row_string_array(final_delivery, "delivery_drop_reasons")? {
        p7_increment(&mut diagnostics.delivery_drop_reason_counts, &reason, 1);
    }
    Ok(())
}

fn parse_p7_safe_locator_view(view: &serde_json::Value) -> Result<P7SafeLocatorView> {
    serde_json::from_value::<P7SafeLocatorView>(view.clone()).map_err(|source| Error::Other {
        source: Box::new(source),
        stage: "p7_provenance_parse_safe_locator_view",
    })
}

fn validate_p7_safe_source_locator_view(view: &serde_json::Value) -> Result<Option<String>> {
    let typed = parse_p7_safe_locator_view(view)?;
    validate_p7_safe_locator_reason(&typed)?;
    match typed.visibility {
        P7SafeLocatorVisibility::GovernedOpaque => {
            let reference = typed.reference.0.ok_or_else(|| {
                p7_provenance_error("opaque capsule source locator reference missing")
            })?;
            let physical_key = reference
                .strip_prefix("opaque:governed-source:")
                .ok_or_else(|| p7_provenance_error("capsule source locator is not scoped"))?;
            if !valid_p7_scoped_governed_source_key(physical_key) {
                return Err(p7_provenance_error(
                    "capsule source locator governed key is invalid",
                ));
            }
            Ok(Some(reference))
        }
        P7SafeLocatorVisibility::Redacted => validate_p7_redacted_locator(typed),
    }
}

fn validate_p7_safe_evidence_ref_view(view: &serde_json::Value) -> Result<Option<String>> {
    let typed = parse_p7_safe_locator_view(view)?;
    validate_p7_safe_locator_reason(&typed)?;
    match typed.visibility {
        P7SafeLocatorVisibility::GovernedOpaque => {
            let reference = typed
                .reference
                .0
                .ok_or_else(|| p7_provenance_error("opaque capsule evidence reference missing"))?;
            let digest = reference
                .strip_prefix("opaque:evidence:")
                .ok_or_else(|| p7_provenance_error("capsule evidence reference is not opaque"))?;
            if !valid_p7_hex_digest(digest) {
                return Err(p7_provenance_error(
                    "capsule evidence reference opaque digest is invalid",
                ));
            }
            Ok(Some(reference))
        }
        P7SafeLocatorVisibility::Redacted => validate_p7_redacted_locator(typed),
    }
}

fn validate_p7_safe_locator_reason(typed: &P7SafeLocatorView) -> Result<()> {
    if typed.reason.trim().is_empty() {
        return Err(p7_provenance_error(
            "capsule source locator reason is empty",
        ));
    }
    Ok(())
}

fn validate_p7_redacted_locator(typed: P7SafeLocatorView) -> Result<Option<String>> {
    if typed.reference.0.is_some() {
        return Err(p7_provenance_error(
            "redacted capsule source locator exposed a reference",
        ));
    }
    Ok(None)
}

fn valid_p7_scoped_governed_source_key(physical_key: &str) -> bool {
    let Some(rest) = physical_key.strip_prefix("scope:") else {
        return false;
    };
    let Some((scope_digest, suffix)) = rest.split_once(':') else {
        return false;
    };
    if !valid_p7_hex_digest(scope_digest) {
        return false;
    }
    if suffix == "evidence_source_ref" {
        return true;
    }
    let Some(owner_digest) = suffix.strip_prefix("owner:") else {
        return false;
    };
    valid_p7_hex_digest(owner_digest)
}

fn valid_p7_hex_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_p7_sdk_projection_delivery_manifest(
    manifest: &serde_json::Value,
    final_delivery: &serde_json::Value,
) -> Result<()> {
    let object = manifest
        .as_object()
        .ok_or_else(|| p7_provenance_error("SDK projection delivery manifest is not an object"))?;
    let expected_fields = [
        "schema_version",
        "system_memory_block_sha256",
        "capsule_entries",
        "governed_block_entries",
        "prompt_visible_entries",
        "deterministic_envelope_sha256",
        "exact_render_match",
        "candidate_receipts",
        "integrity_failures",
    ];
    if object.len() != expected_fields.len()
        || expected_fields
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return Err(p7_provenance_error(
            "SDK projection delivery manifest contains an unexpected or missing field",
        ));
    }
    if p7_required_usize(
        manifest,
        "schema_version",
        "projection manifest schema version missing",
    )? != usize::try_from(MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION).unwrap_or(usize::MAX)
    {
        return Err(p7_provenance_error(
            "SDK projection delivery manifest contract mismatch",
        ));
    }
    let capsule_entries = p7_projection_manifest_entry_set(
        manifest,
        "capsule_entries",
        "SDK delivery capsule manifest entries are duplicated",
    )?;
    let governed_entries = p7_projection_manifest_entry_set(
        manifest,
        "governed_block_entries",
        "SDK governed projection block manifest entries are duplicated",
    )?;
    let prompt_entries = p7_projection_manifest_entry_set(
        manifest,
        "prompt_visible_entries",
        "SDK final prompt manifest entries are duplicated",
    )?;
    let final_capsules = p7_required_array(
        final_delivery,
        "rendered_capsules",
        "final rendered capsules missing",
    )?;
    let final_candidate_ids = final_capsules
        .iter()
        .map(|capsule| {
            p7_required_str(
                capsule,
                "candidate_id",
                "final rendered capsule candidate id missing",
            )
            .map(str::to_string)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let manifest_candidate_ids = capsule_entries
        .iter()
        .map(|(_, candidate_id, _)| candidate_id.clone())
        .collect::<BTreeSet<_>>();
    if final_candidate_ids.len() != final_capsules.len()
        || manifest_candidate_ids.len() != capsule_entries.len()
        || manifest_candidate_ids != final_candidate_ids
        || capsule_entries != governed_entries
        || capsule_entries != prompt_entries
    {
        return Err(p7_provenance_error(
            "SDK projection delivery digest sets are not bidirectionally exact",
        ));
    }
    if !p7_required_bool(
        manifest,
        "exact_render_match",
        "SDK deterministic projection render proof missing",
    )? || !p7_row_string_array(manifest, "integrity_failures")?.is_empty()
    {
        return Err(p7_provenance_error(
            "SDK deterministic projection render integrity failed",
        ));
    }
    let receipt_entries = p7_projection_manifest_receipt_set(manifest)?;
    let receipt_candidate_ids = receipt_entries
        .iter()
        .map(|(_, candidate_id, _)| candidate_id.clone())
        .collect::<BTreeSet<_>>();
    let capsule_owner_candidates = capsule_entries
        .iter()
        .map(|(owner_token, candidate_id, _)| (owner_token.clone(), candidate_id.clone()))
        .collect::<BTreeSet<_>>();
    let receipt_owner_candidates = receipt_entries
        .iter()
        .map(|(owner_token, candidate_id, _)| (owner_token.clone(), candidate_id.clone()))
        .collect::<BTreeSet<_>>();
    if receipt_candidate_ids.len() != receipt_entries.len()
        || receipt_candidate_ids != final_candidate_ids
        || receipt_owner_candidates != capsule_owner_candidates
    {
        return Err(p7_provenance_error(
            "SDK projection renderer receipts are not bidirectionally exact",
        ));
    }
    let system_digest = p7_required_str(
        manifest,
        "system_memory_block_sha256",
        "system memory block digest missing",
    )?;
    let deterministic_digest = p7_required_str(
        manifest,
        "deterministic_envelope_sha256",
        "deterministic projection envelope digest missing",
    )?;
    if !is_sha256(system_digest) || !is_sha256(deterministic_digest) {
        return Err(p7_provenance_error(
            "SDK projection envelope digest is invalid",
        ));
    }
    Ok(())
}

fn validate_p7_runner_projection_digest_observation(
    observation: &serde_json::Value,
    manifest: &serde_json::Value,
) -> Result<()> {
    let object = observation.as_object().ok_or_else(|| {
        p7_provenance_error("runner projection digest observation is not an object")
    })?;
    let expected_fields = [
        "schema_version",
        "system_memory_block_sha256",
        "runtime_envelope_sha256",
        "capsule_entries",
        "governed_block_entries",
        "prompt_visible_entries",
        "candidate_receipts",
    ];
    if object.len() != expected_fields.len()
        || expected_fields
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return Err(p7_provenance_error(
            "runner projection digest observation contains an unexpected or missing field",
        ));
    }
    if p7_required_str(
        observation,
        "schema_version",
        "runner projection digest observation schema missing",
    )? != P7_RUNNER_PROJECTION_DIGEST_OBSERVATION_SCHEMA_VERSION
    {
        return Err(p7_provenance_error(
            "runner projection digest observation schema mismatch",
        ));
    }

    let observed_capsules = p7_projection_observation_entry_set(observation, "capsule_entries")?;
    let observed_governed =
        p7_projection_observation_entry_set(observation, "governed_block_entries")?;
    let observed_prompt =
        p7_projection_observation_entry_set(observation, "prompt_visible_entries")?;
    let observed_receipts = p7_projection_observation_receipt_set(observation)?;
    let manifest_capsules = p7_projection_manifest_entry_set(
        manifest,
        "capsule_entries",
        "SDK delivery capsule manifest entries are duplicated",
    )?;
    let manifest_governed = p7_projection_manifest_entry_set(
        manifest,
        "governed_block_entries",
        "SDK governed projection block manifest entries are duplicated",
    )?;
    let manifest_prompt = p7_projection_manifest_entry_set(
        manifest,
        "prompt_visible_entries",
        "SDK final prompt manifest entries are duplicated",
    )?;
    let manifest_receipts = p7_projection_manifest_receipt_set(manifest)?;
    if observed_capsules != manifest_capsules
        || observed_governed != manifest_governed
        || observed_prompt != manifest_prompt
        || observed_receipts != manifest_receipts
    {
        return Err(p7_provenance_error(
            "runner content digest observation differs from the SDK projection manifest",
        ));
    }

    let observed_system = p7_required_str(
        observation,
        "system_memory_block_sha256",
        "runner system memory block digest missing",
    )?;
    let observed_envelope = p7_required_str(
        observation,
        "runtime_envelope_sha256",
        "runner runtime envelope digest missing",
    )?;
    let manifest_system = p7_required_str(
        manifest,
        "system_memory_block_sha256",
        "system memory block digest missing",
    )?;
    let manifest_envelope = p7_required_str(
        manifest,
        "deterministic_envelope_sha256",
        "deterministic projection envelope digest missing",
    )?;
    if !is_sha256(observed_system)
        || !is_sha256(observed_envelope)
        || observed_system != manifest_system
        || observed_envelope != manifest_envelope
    {
        return Err(p7_provenance_error(
            "runner system or envelope digest differs from the SDK projection manifest",
        ));
    }
    Ok(())
}

fn p7_projection_observation_entry_set(
    observation: &serde_json::Value,
    field: &str,
) -> Result<BTreeSet<(String, String, String)>> {
    let entries = p7_required_array(
        observation,
        field,
        "runner projection digest entries missing",
    )?;
    let mut entry_set = BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| {
            p7_provenance_error("runner projection digest entry is not an object")
        })?;
        if object.len() != 3
            || !object.contains_key("owner_identity_token")
            || !object.contains_key("candidate_id")
            || !object.contains_key("content_sha256")
        {
            return Err(p7_provenance_error(
                "runner projection digest entry contains raw or unexpected data",
            ));
        }
        let candidate_id = p7_required_str(
            entry,
            "candidate_id",
            "runner projection digest candidate id missing",
        )?;
        let owner_identity_token = p7_projection_owner_identity_token(
            entry,
            "runner projection owner identity token missing or invalid",
        )?;
        let content_sha256 = p7_required_str(
            entry,
            "content_sha256",
            "runner projection content digest missing",
        )?;
        if !is_sha256(content_sha256)
            || !entry_set.insert((
                owner_identity_token.to_string(),
                candidate_id.to_string(),
                content_sha256.to_string(),
            ))
        {
            return Err(p7_provenance_error(
                "runner projection digest entries are invalid or duplicated",
            ));
        }
    }
    Ok(entry_set)
}

fn p7_projection_observation_receipt_set(
    observation: &serde_json::Value,
) -> Result<BTreeSet<(String, String, String)>> {
    let entries = p7_required_array(
        observation,
        "candidate_receipts",
        "runner projection renderer receipts missing",
    )?;
    let mut entry_set = BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| {
            p7_provenance_error("runner projection renderer receipt is not an object")
        })?;
        if object.len() != 3
            || !object.contains_key("owner_identity_token")
            || !object.contains_key("candidate_id")
            || !object.contains_key("source_block_sha256")
        {
            return Err(p7_provenance_error(
                "runner projection renderer receipt contains raw or unexpected data",
            ));
        }
        let candidate_id = p7_required_str(
            entry,
            "candidate_id",
            "runner projection renderer receipt candidate id missing",
        )?;
        let owner_identity_token = p7_projection_owner_identity_token(
            entry,
            "runner projection renderer owner identity token missing or invalid",
        )?;
        let source_block_sha256 = p7_required_str(
            entry,
            "source_block_sha256",
            "runner projection renderer source block digest missing",
        )?;
        if !is_sha256(source_block_sha256)
            || !entry_set.insert((
                owner_identity_token.to_string(),
                candidate_id.to_string(),
                source_block_sha256.to_string(),
            ))
        {
            return Err(p7_provenance_error(
                "runner projection renderer receipts are invalid or duplicated",
            ));
        }
    }
    Ok(entry_set)
}

fn p7_projection_manifest_receipt_set(
    manifest: &serde_json::Value,
) -> Result<BTreeSet<(String, String, String)>> {
    let entries = p7_required_array(
        manifest,
        "candidate_receipts",
        "SDK projection renderer receipts missing",
    )?;
    let mut entry_set = BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| {
            p7_provenance_error("SDK projection renderer receipt is not an object")
        })?;
        if object.len() != 3
            || !object.contains_key("owner_identity_token")
            || !object.contains_key("candidate_id")
            || !object.contains_key("source_block_sha256")
        {
            return Err(p7_provenance_error(
                "SDK projection renderer receipt contains raw or unexpected data",
            ));
        }
        let candidate_id = p7_required_str(
            entry,
            "candidate_id",
            "SDK projection renderer receipt candidate id missing",
        )?;
        let owner_identity_token = p7_projection_owner_identity_token(
            entry,
            "SDK projection renderer owner identity token missing or invalid",
        )?;
        let source_block_sha256 = p7_required_str(
            entry,
            "source_block_sha256",
            "SDK projection renderer source block digest missing",
        )?;
        if !is_sha256(source_block_sha256)
            || !entry_set.insert((
                owner_identity_token.to_string(),
                candidate_id.to_string(),
                source_block_sha256.to_string(),
            ))
        {
            return Err(p7_provenance_error(
                "SDK projection renderer receipts are invalid or duplicated",
            ));
        }
    }
    if entry_set.len() != entries.len() {
        return Err(p7_provenance_error(
            "SDK projection renderer receipts are invalid or duplicated",
        ));
    }
    Ok(entry_set)
}

fn p7_projection_manifest_entry_set(
    manifest: &serde_json::Value,
    field: &str,
    duplicate_error: &'static str,
) -> Result<BTreeSet<(String, String, String)>> {
    let entries = p7_required_array(
        manifest,
        field,
        "SDK projection delivery manifest entries missing",
    )?;
    let mut entry_set = BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| {
            p7_provenance_error("SDK projection delivery manifest entry is not an object")
        })?;
        if object.len() != 3
            || !object.contains_key("owner_identity_token")
            || !object.contains_key("candidate_id")
            || !object.contains_key("content_sha256")
        {
            return Err(p7_provenance_error(
                "SDK projection delivery manifest entry contains raw or unexpected data",
            ));
        }
        let candidate_id = p7_required_str(
            entry,
            "candidate_id",
            "SDK projection delivery candidate id missing",
        )?;
        let owner_identity_token = p7_projection_owner_identity_token(
            entry,
            "SDK projection delivery owner identity token missing or invalid",
        )?;
        let content_sha256 = p7_required_str(
            entry,
            "content_sha256",
            "SDK projection delivery content digest missing",
        )?;
        if !is_sha256(content_sha256) {
            return Err(p7_provenance_error(
                "SDK projection delivery manifest contains an invalid content digest",
            ));
        }
        if !entry_set.insert((
            owner_identity_token.to_string(),
            candidate_id.to_string(),
            content_sha256.to_string(),
        )) {
            return Err(p7_provenance_error(duplicate_error));
        }
    }
    if entry_set.len() != entries.len() {
        return Err(p7_provenance_error(duplicate_error));
    }
    Ok(entry_set)
}

fn p7_projection_owner_identity_token<'a>(
    entry: &'a serde_json::Value,
    error: &'static str,
) -> Result<&'a str> {
    let token = p7_required_str(entry, "owner_identity_token", error)?;
    let digest = token
        .strip_prefix(P7_PROJECTION_OWNER_IDENTITY_TOKEN_PREFIX)
        .ok_or_else(|| p7_provenance_error(error))?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(p7_provenance_error(error));
    }
    Ok(token)
}

fn validate_p7_shard_against_detail(
    shard: &serde_json::Value,
    aggregate: &P7DetailAggregate,
) -> Result<()> {
    validate_p7_detail_metrics(shard, aggregate)
}

fn validate_p7_summary_against_detail(
    summary: &W4ExternalNoisyBenchmarkSummary,
    aggregate: &P7DetailAggregate,
) -> Result<()> {
    let value = serde_json::to_value(summary).map_err(|source| Error::Other {
        source: Box::new(source),
        stage: "p7_provenance_serialize_verified_summary",
    })?;
    validate_p7_detail_metrics(&value, aggregate)
}

fn validate_p7_detail_metrics(
    claimed: &serde_json::Value,
    aggregate: &P7DetailAggregate,
) -> Result<()> {
    let scalar_claims = [
        ("samples", aggregate.samples),
        ("questions", aggregate.questions),
        ("evidence_questions", aggregate.evidence_questions),
        ("any_evidence_hit", aggregate.any_evidence_hit),
        ("all_evidence_hit", aggregate.all_evidence_hit),
        ("write_errors", aggregate.write_errors),
        ("recall_errors", aggregate.recall_errors),
    ];
    for (field, expected) in scalar_claims {
        if claimed.get(field).and_then(serde_json::Value::as_u64) != Some(expected as u64) {
            return Err(p7_provenance_error(
                "summary scalar does not match detail recomputation",
            ));
        }
    }
    let recomputed = [
        (
            "stage_hit_counts",
            serde_json::to_value(&aggregate.stage_hit_counts),
        ),
        (
            "index_diagnostics",
            serde_json::to_value(&aggregate.index_diagnostics),
        ),
        (
            "w4_1_diagnostics",
            serde_json::to_value(&aggregate.w4_1_diagnostics),
        ),
        (
            "p7_loss_ledger",
            serde_json::to_value(&aggregate.p7_loss_ledger),
        ),
        (
            "p7_production_delivery",
            serde_json::to_value(&aggregate.p7_production_delivery),
        ),
    ];
    for (field, expected) in recomputed {
        let expected = expected.map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "p7_provenance_serialize_recomputed_detail",
        })?;
        if claimed.get(field) != Some(&expected) {
            return Err(p7_provenance_error(
                "summary diagnostics do not match detail recomputation",
            ));
        }
    }
    Ok(())
}

fn p7_rendered_sources_from_delivery(report: &serde_json::Value) -> Result<Vec<String>> {
    Ok(p7_candidate_evidence_groups(
        &p7_rendered_candidates_from_delivery(report)?,
    ))
}

fn p7_rendered_candidates_from_delivery(
    report: &serde_json::Value,
) -> Result<Vec<P7CandidateEvidence>> {
    let mut candidates = Vec::new();
    let mut seen_candidate_ids = BTreeSet::new();
    for capsule in p7_required_array(
        report,
        "rendered_capsules",
        "final rendered capsules missing",
    )? {
        let candidate_id = p7_required_str(
            capsule,
            "candidate_id",
            "final rendered capsule candidate id missing",
        )?;
        if !seen_candidate_ids.insert(candidate_id.to_string()) {
            return Err(p7_provenance_error(
                "final rendered capsule candidate ids are duplicated",
            ));
        }
        candidates.push(P7CandidateEvidence {
            candidate_id: candidate_id.to_string(),
            canonical_evidence_groups: p7_canonical_groups(&p7_row_string_array(
                capsule,
                "canonical_evidence_groups",
            )?)
            .into_iter()
            .collect(),
        });
    }
    Ok(candidates)
}

fn p7_candidate_evidence_groups(candidates: &[P7CandidateEvidence]) -> Vec<String> {
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    for candidate in candidates {
        for group in &candidate.canonical_evidence_groups {
            let group = bm_core::memory::canonical_recall_evidence_group(group);
            if !group.is_empty() && seen.insert(group.clone()) {
                sources.push(group);
            }
        }
    }
    sources
}

fn p7_selected_sources_from_delivery(report: &serde_json::Value) -> Result<Vec<String>> {
    Ok(p7_candidate_evidence_groups(
        &p7_selected_candidates_from_delivery(report)?,
    ))
}

fn p7_selected_candidates_from_delivery(
    report: &serde_json::Value,
) -> Result<Vec<P7CandidateEvidence>> {
    let selected_ids = p7_row_string_array(report, "selected_candidate_ids")?;
    let selected_id_set = selected_ids.iter().cloned().collect::<BTreeSet<_>>();
    if selected_id_set.len() != selected_ids.len() {
        return Err(p7_provenance_error(
            "final delivery selected candidate ids are not unique",
        ));
    }
    let mut decision_ids = BTreeSet::new();
    let mut candidates = Vec::new();
    for decision in p7_required_array(
        report,
        "selection_decisions",
        "final delivery selection decisions missing",
    )? {
        if !p7_required_bool(decision, "selected", "selection decision flag missing")? {
            continue;
        }
        let candidate_id = p7_required_str(
            decision,
            "candidate_id",
            "selection decision candidate id missing",
        )?;
        if !decision_ids.insert(candidate_id.to_string()) {
            return Err(p7_provenance_error(
                "final delivery selected decision ids are not unique",
            ));
        }
        candidates.push(P7CandidateEvidence {
            candidate_id: candidate_id.to_string(),
            canonical_evidence_groups: p7_canonical_groups(&p7_row_string_array(
                decision,
                "canonical_evidence_groups",
            )?)
            .into_iter()
            .collect(),
        });
    }
    if decision_ids != selected_id_set {
        return Err(p7_provenance_error(
            "final delivery selected ids do not match selected decisions",
        ));
    }
    Ok(candidates)
}

fn p7_delivery_candidate_ids(report: &serde_json::Value) -> Result<Vec<String>> {
    p7_required_array(
        report,
        "rendered_capsules",
        "delivery rendered capsules missing",
    )?
    .iter()
    .map(|capsule| {
        p7_required_str(capsule, "candidate_id", "delivery candidate id missing")
            .map(str::to_string)
    })
    .collect()
}

fn p7_required_str<'a>(
    value: &'a serde_json::Value,
    field: &str,
    message: &'static str,
) -> Result<&'a str> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_optional_str<'a>(
    value: &'a serde_json::Value,
    field: &str,
    message: &'static str,
) -> Result<Option<&'a str>> {
    match value.get(field) {
        Some(value) if value.is_null() => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| p7_provenance_error(message)),
        None => Err(p7_provenance_error(message)),
    }
}

fn p7_required_usize(
    value: &serde_json::Value,
    field: &str,
    message: &'static str,
) -> Result<usize> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_required_i64(value: &serde_json::Value, field: &str, message: &'static str) -> Result<i64> {
    value
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_signed_usize_delta(baseline: usize, off_run: usize) -> Result<i64> {
    let baseline = i128::try_from(baseline)
        .map_err(|_| p7_provenance_error("ablation baseline count exceeds signed range"))?;
    let off_run = i128::try_from(off_run)
        .map_err(|_| p7_provenance_error("ablation off-run count exceeds signed range"))?;
    i64::try_from(baseline - off_run)
        .map_err(|_| p7_provenance_error("ablation numeric delta exceeds i64"))
}

fn p7_required_bool(value: &serde_json::Value, field: &str, message: &'static str) -> Result<bool> {
    value
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_required_array<'a>(
    value: &'a serde_json::Value,
    field: &str,
    message: &'static str,
) -> Result<&'a Vec<serde_json::Value>> {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_string_array(value: &serde_json::Value, message: &'static str) -> Result<Vec<String>> {
    value
        .as_array()
        .ok_or_else(|| p7_provenance_error(message))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| p7_provenance_error(message))
        })
        .collect()
}

fn p7_row_string_array(value: &serde_json::Value, field: &str) -> Result<Vec<String>> {
    p7_string_array(
        value
            .get(field)
            .ok_or_else(|| p7_provenance_error("detail string array field missing"))?,
        "detail field must be a string array",
    )
}

#[cfg(test)]
fn p7_any_gold_hit(gold: &[String], actual: &[String]) -> bool {
    let gold_groups = p7_canonical_groups(gold);
    let actual_groups = p7_canonical_groups(actual);
    !gold_groups.is_empty() && !gold_groups.is_disjoint(&actual_groups)
}

#[cfg(test)]
fn p7_all_gold_hit(gold: &[String], actual: &[String]) -> bool {
    let gold_groups = p7_canonical_groups(gold);
    let actual_groups = p7_canonical_groups(actual);
    !gold_groups.is_empty() && gold_groups.is_subset(&actual_groups)
}

fn p7_canonical_groups(sources: &[String]) -> BTreeSet<String> {
    sources
        .iter()
        .map(|source| {
            let source = source.trim();
            let direct = bm_core::memory::canonical_recall_evidence_group(source);
            if source.starts_with("external_eval:") || direct == source.to_ascii_lowercase() {
                direct
            } else {
                bm_core::memory::canonical_recall_evidence_group(&format!("external_eval:{source}"))
            }
        })
        .filter(|group| !group.is_empty())
        .collect()
}

fn p7_stage_loss_groups(
    gold_groups: &BTreeSet<String>,
    upstream_groups: &BTreeSet<String>,
    downstream_groups: &BTreeSet<String>,
) -> BTreeSet<String> {
    gold_groups
        .intersection(upstream_groups)
        .filter(|group| !downstream_groups.contains(*group))
        .cloned()
        .collect()
}

fn p7_loss_entry_groups(entries: &[serde_json::Value]) -> Result<BTreeSet<String>> {
    let raw_groups = entries
        .iter()
        .map(|entry| {
            p7_required_str(
                entry,
                "canonical_evidence_group",
                "P7 loss entry canonical evidence group missing",
            )
            .map(str::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    let groups = raw_groups.iter().cloned().collect::<BTreeSet<_>>();
    if groups.len() != entries.len()
        || raw_groups
            .iter()
            .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
    {
        return Err(p7_provenance_error(
            "P7 loss ledger groups are not unique opaque canonical ids",
        ));
    }
    Ok(groups)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7LossCandidateMatch {
    candidate_id: String,
    rank: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7LossCandidateReason {
    candidate_id: String,
    drop_reason: String,
}

fn validate_p7_loss_entries(
    entries: &[serde_json::Value],
    expected_groups: &BTreeSet<String>,
    expanded: &[P7CandidateEvidence],
    reranked: &[P7CandidateEvidence],
    selected: &[P7CandidateEvidence],
    rendered: &[P7CandidateEvidence],
    delivery: &serde_json::Value,
) -> Result<()> {
    let mut entries_by_group = BTreeMap::new();
    for entry in entries {
        let group = p7_required_str(
            entry,
            "canonical_evidence_group",
            "P7 loss entry canonical evidence group missing",
        )?;
        if entries_by_group.insert(group.to_string(), entry).is_some() {
            return Err(p7_provenance_error(
                "P7 loss ledger contains duplicate canonical evidence groups",
            ));
        }
    }
    for group in expected_groups {
        let entry = entries_by_group
            .get(group)
            .ok_or_else(|| p7_provenance_error("P7 loss ledger entry missing"))?;
        let expanded_matches = p7_expected_loss_matches(group, expanded);
        let reranked_matches = p7_expected_loss_matches(group, reranked);
        let selected_matches = p7_expected_loss_matches(group, selected);
        let rendered_matches = p7_expected_loss_matches(group, rendered);
        if p7_claimed_loss_matches(entry, "expanded_matches")? != expanded_matches
            || p7_claimed_loss_matches(entry, "reranked_matches")? != reranked_matches
            || p7_claimed_loss_matches(entry, "selected_matches")? != selected_matches
            || p7_claimed_loss_matches(entry, "rendered_matches")? != rendered_matches
        {
            return Err(p7_provenance_error(
                "P7 loss ledger candidate matches or ranks disagree with SDK stages",
            ));
        }
        let expected_selection_losses = p7_expected_loss_reasons(
            delivery,
            "selection_decisions",
            "selected",
            &expanded_matches,
        )?;
        let expected_render_losses =
            p7_expected_loss_reasons(delivery, "render_decisions", "rendered", &selected_matches)?;
        if p7_claimed_loss_reasons(entry, "selection_losses")? != expected_selection_losses
            || p7_claimed_loss_reasons(entry, "render_losses")? != expected_render_losses
        {
            return Err(p7_provenance_error(
                "P7 loss ledger candidate-bound drop reasons disagree with delivery decisions",
            ));
        }
    }
    Ok(())
}

fn p7_expected_loss_matches(
    group: &str,
    candidates: &[P7CandidateEvidence],
) -> Vec<P7LossCandidateMatch> {
    candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            candidate
                .canonical_evidence_groups
                .iter()
                .any(|candidate_group| candidate_group == group)
        })
        .map(|(index, candidate)| P7LossCandidateMatch {
            candidate_id: candidate.candidate_id.clone(),
            rank: index + 1,
        })
        .collect()
}

fn p7_claimed_loss_matches(
    entry: &serde_json::Value,
    field: &str,
) -> Result<Vec<P7LossCandidateMatch>> {
    p7_required_array(entry, field, "P7 loss stage matches missing")?
        .iter()
        .map(|item| {
            Ok(P7LossCandidateMatch {
                candidate_id: p7_required_str(
                    item,
                    "candidate_id",
                    "P7 loss match candidate id missing",
                )?
                .to_string(),
                rank: p7_required_usize(item, "rank", "P7 loss match rank missing")?,
            })
        })
        .collect()
}

fn p7_expected_loss_reasons(
    delivery: &serde_json::Value,
    decisions_field: &str,
    accepted_field: &str,
    upstream_matches: &[P7LossCandidateMatch],
) -> Result<Vec<P7LossCandidateReason>> {
    let mut decisions = BTreeMap::new();
    for decision in p7_required_array(delivery, decisions_field, "P7 delivery decisions missing")? {
        let candidate_id = p7_required_str(
            decision,
            "candidate_id",
            "P7 delivery decision candidate id missing",
        )?;
        if decisions
            .insert(candidate_id.to_string(), decision)
            .is_some()
        {
            return Err(p7_provenance_error(
                "P7 delivery decisions contain duplicate candidate ids",
            ));
        }
    }
    let mut losses = Vec::new();
    for candidate in upstream_matches {
        let Some(decision) = decisions.get(&candidate.candidate_id) else {
            continue;
        };
        if p7_required_bool(
            decision,
            accepted_field,
            "P7 delivery decision acceptance flag missing",
        )? {
            continue;
        }
        if let Some(reason) = decision
            .get("drop_reason")
            .and_then(serde_json::Value::as_str)
        {
            losses.push(P7LossCandidateReason {
                candidate_id: candidate.candidate_id.clone(),
                drop_reason: reason.to_string(),
            });
        }
    }
    Ok(losses)
}

fn p7_claimed_loss_reasons(
    entry: &serde_json::Value,
    field: &str,
) -> Result<Vec<P7LossCandidateReason>> {
    p7_required_array(entry, field, "P7 candidate-bound losses missing")?
        .iter()
        .map(|item| {
            Ok(P7LossCandidateReason {
                candidate_id: p7_required_str(
                    item,
                    "candidate_id",
                    "P7 loss candidate id missing",
                )?
                .to_string(),
                drop_reason: p7_required_str(item, "drop_reason", "P7 loss drop reason missing")?
                    .to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
fn p7_gold_group_hit_count(gold_groups: &BTreeSet<String>, refs: &[String]) -> usize {
    let actual_groups = p7_canonical_groups(refs);
    gold_groups.intersection(&actual_groups).count()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7CandidateEvidence {
    candidate_id: String,
    canonical_evidence_groups: Vec<String>,
}

fn p7_stage_candidates(row: &serde_json::Value, field: &str) -> Result<Vec<P7CandidateEvidence>> {
    let reports = row
        .get("sdk_stage_candidates")
        .ok_or_else(|| p7_provenance_error("SDK stage candidate reports missing"))?;
    p7_candidate_evidence_array(
        reports
            .get(field)
            .ok_or_else(|| p7_provenance_error("SDK stage candidate report missing"))?,
    )
}

fn p7_ablation_candidates(
    slice: &serde_json::Value,
    field: &str,
) -> Result<Vec<P7CandidateEvidence>> {
    p7_candidate_evidence_array(
        slice
            .get(field)
            .ok_or_else(|| p7_provenance_error("ablation candidate-bound report missing"))?,
    )
}

fn p7_candidate_evidence_array(value: &serde_json::Value) -> Result<Vec<P7CandidateEvidence>> {
    let entries = value
        .as_array()
        .ok_or_else(|| p7_provenance_error("SDK candidate report must be an array"))?;
    let mut candidates = Vec::with_capacity(entries.len());
    let mut seen_candidate_ids = BTreeSet::new();
    for entry in entries {
        let candidate_id =
            p7_required_str(entry, "candidate_id", "SDK candidate report id missing")?;
        if candidate_id.trim().is_empty() {
            return Err(p7_provenance_error("SDK candidate report id is empty"));
        }
        if !seen_candidate_ids.insert(candidate_id.to_string()) {
            return Err(p7_provenance_error("SDK candidate report id is duplicated"));
        }
        let raw_groups = p7_row_string_array(entry, "canonical_evidence_groups")?;
        let groups = raw_groups
            .iter()
            .map(|group| bm_core::memory::canonical_recall_evidence_group(group))
            .collect::<BTreeSet<_>>();
        let ordered_groups = groups.iter().cloned().collect::<Vec<_>>();
        if groups.len() != raw_groups.len()
            || raw_groups != ordered_groups
            || raw_groups
                .iter()
                .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
        {
            return Err(p7_provenance_error(
                "SDK candidate report groups are not unique opaque canonical ids",
            ));
        }
        candidates.push(P7CandidateEvidence {
            candidate_id: candidate_id.to_string(),
            canonical_evidence_groups: groups.into_iter().collect(),
        });
    }
    Ok(candidates)
}

fn p7_authoritative_evidence_index(value: &serde_json::Value) -> Result<Vec<P7CandidateEvidence>> {
    let candidates = p7_candidate_evidence_array(value)?;
    if candidates
        .windows(2)
        .any(|window| window[0].candidate_id >= window[1].candidate_id)
    {
        return Err(p7_provenance_error(
            "SDK authoritative evidence index is not sorted by candidate id",
        ));
    }
    Ok(candidates)
}

fn p7_candidate_evidence_map(
    candidates: &[P7CandidateEvidence],
) -> BTreeMap<String, BTreeSet<String>> {
    candidates
        .iter()
        .map(|candidate| {
            (
                candidate.candidate_id.clone(),
                candidate
                    .canonical_evidence_groups
                    .iter()
                    .cloned()
                    .collect(),
            )
        })
        .collect()
}

fn p7_require_full_candidate_bindings(
    evidence_by_candidate: &BTreeMap<String, BTreeSet<String>>,
    candidates: &[P7CandidateEvidence],
) -> Result<()> {
    for candidate in candidates {
        let groups = candidate
            .canonical_evidence_groups
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if groups.is_empty() {
            return Err(p7_provenance_error(
                "selected candidate authoritative evidence binding is empty",
            ));
        }
        if evidence_by_candidate.get(&candidate.candidate_id) != Some(&groups) {
            return Err(p7_provenance_error(
                "selected candidate differs from the report-wide authoritative evidence index",
            ));
        }
    }
    Ok(())
}

fn p7_require_candidate_binding_subsets(
    evidence_by_candidate: &BTreeMap<String, BTreeSet<String>>,
    candidates: &[P7CandidateEvidence],
) -> Result<()> {
    for candidate in candidates {
        let groups = candidate
            .canonical_evidence_groups
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let Some(authoritative_groups) = evidence_by_candidate.get(&candidate.candidate_id) else {
            return Err(p7_provenance_error(
                "stage candidate is absent from the report-wide authoritative evidence index",
            ));
        };
        if !groups.is_subset(authoritative_groups) {
            return Err(p7_provenance_error(
                "stage candidate evidence exceeds its report-wide authoritative binding",
            ));
        }
    }
    Ok(())
}

fn p7_require_rendered_candidate_bindings(
    evidence_by_candidate: &BTreeMap<String, BTreeSet<String>>,
    selected_by_candidate: &BTreeMap<String, BTreeSet<String>>,
    candidates: &[P7CandidateEvidence],
) -> Result<()> {
    for candidate in candidates {
        let groups = candidate
            .canonical_evidence_groups
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if groups.is_empty() {
            return Err(p7_provenance_error(
                "rendered candidate evidence binding is empty",
            ));
        }
        let Some(authoritative_groups) = evidence_by_candidate.get(&candidate.candidate_id) else {
            return Err(p7_provenance_error(
                "rendered candidate is absent from the report-wide authoritative evidence index",
            ));
        };
        let Some(selected_groups) = selected_by_candidate.get(&candidate.candidate_id) else {
            return Err(p7_provenance_error(
                "rendered candidate is absent from the corresponding selected surface",
            ));
        };
        if !groups.is_subset(authoritative_groups) || !groups.is_subset(selected_groups) {
            return Err(p7_provenance_error(
                "rendered candidate evidence exceeds its authoritative selected binding",
            ));
        }
    }
    Ok(())
}

fn p7_affected_ablation_candidate_ids(
    baseline_selected: &[P7CandidateEvidence],
    off_selected: &[P7CandidateEvidence],
    baseline_rendered: &[P7CandidateEvidence],
    off_rendered: &[P7CandidateEvidence],
) -> BTreeSet<String> {
    let baseline_selected = p7_candidate_evidence_map(baseline_selected);
    let off_selected = p7_candidate_evidence_map(off_selected);
    let baseline_rendered = p7_candidate_evidence_map(baseline_rendered);
    let off_rendered = p7_candidate_evidence_map(off_rendered);
    baseline_selected
        .keys()
        .chain(off_selected.keys())
        .chain(baseline_rendered.keys())
        .chain(off_rendered.keys())
        .filter(|candidate_id| {
            baseline_selected.get(*candidate_id) != off_selected.get(*candidate_id)
                || baseline_rendered.get(*candidate_id) != off_rendered.get(*candidate_id)
        })
        .cloned()
        .collect()
}

fn p7_matched_gold_group_set(
    gold: &[String],
    candidates: &[P7CandidateEvidence],
) -> BTreeSet<String> {
    p7_match_gold_groups(gold, candidates)
        .into_iter()
        .map(|(_, group)| group)
        .collect()
}

fn validate_p7_stage_candidate_reports(row: &serde_json::Value) -> Result<()> {
    let evidence_index = p7_authoritative_evidence_index(
        row.get("evidence_ref_index")
            .ok_or_else(|| p7_provenance_error("SDK safe evidence index missing"))?,
    )?;
    let evidence_by_candidate = p7_candidate_evidence_map(&evidence_index);
    for field in ["source", "expanded", "reranked"] {
        p7_require_candidate_binding_subsets(
            &evidence_by_candidate,
            &p7_stage_candidates(row, field)?,
        )?;
    }
    p7_require_full_candidate_bindings(
        &evidence_by_candidate,
        &p7_stage_candidates(row, "eval_selected")?,
    )?;
    let eval_selected = p7_candidate_evidence_map(&p7_stage_candidates(row, "eval_selected")?);
    p7_require_rendered_candidate_bindings(
        &evidence_by_candidate,
        &eval_selected,
        &p7_stage_candidates(row, "eval_rendered")?,
    )?;

    let eval_delivery = row
        .get("eval_delivery_report")
        .ok_or_else(|| p7_provenance_error("eval delivery report missing"))?;
    let final_delivery = row
        .get("final_projection_delivery_report")
        .ok_or_else(|| p7_provenance_error("final projection delivery report missing"))?;
    for (field, expected) in [
        (
            "eval_selected",
            p7_selected_candidates_from_delivery(eval_delivery)?,
        ),
        (
            "eval_rendered",
            p7_rendered_candidates_from_delivery(eval_delivery)?,
        ),
        (
            "projection_selected",
            p7_selected_candidates_from_delivery(final_delivery)?,
        ),
        (
            "final_rendered",
            p7_rendered_candidates_from_delivery(final_delivery)?,
        ),
    ] {
        if p7_candidate_evidence_map(&p7_stage_candidates(row, field)?)
            != p7_candidate_evidence_map(&expected)
        {
            return Err(p7_provenance_error(
                "SDK stage candidate report differs from delivery owner",
            ));
        }
    }

    for (field, stage) in [
        ("source_sources", "source"),
        ("expanded_sources", "expanded"),
        ("reranked_sources", "reranked"),
        ("selected_sources", "eval_selected"),
        ("projection_selected_sources", "projection_selected"),
        ("rendered_sources", "final_rendered"),
    ] {
        let diagnostic_groups = p7_row_string_array(row, field)?;
        if diagnostic_groups
            .iter()
            .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
            || p7_canonical_groups(&diagnostic_groups)
                != p7_candidate_evidence_groups(&p7_stage_candidates(row, stage)?)
                    .into_iter()
                    .collect()
        {
            return Err(p7_provenance_error(
                "diagnostic stage groups differ from raw SDK candidate reports",
            ));
        }
    }
    Ok(())
}

fn p7_match_gold_groups(
    gold: &[String],
    candidates: &[P7CandidateEvidence],
) -> Vec<(String, String)> {
    let gold_groups = p7_canonical_groups(gold).into_iter().collect::<Vec<_>>();
    let mut groups_by_candidate = BTreeMap::<String, BTreeSet<String>>::new();
    for candidate in candidates {
        if candidate.candidate_id.trim().is_empty() {
            continue;
        }
        let groups = groups_by_candidate
            .entry(candidate.candidate_id.clone())
            .or_default();
        for group in &candidate.canonical_evidence_groups {
            let group = bm_core::memory::canonical_recall_evidence_group(group);
            if !group.is_empty() {
                groups.insert(group);
            }
        }
    }
    let candidates = groups_by_candidate.into_iter().collect::<Vec<_>>();
    let mut owner_by_gold = vec![None; gold_groups.len()];
    for candidate_index in 0..candidates.len() {
        let mut visited_gold = vec![false; gold_groups.len()];
        p7_augment_gold_match(
            candidate_index,
            &candidates,
            &gold_groups,
            &mut owner_by_gold,
            &mut visited_gold,
        );
    }
    owner_by_gold
        .into_iter()
        .enumerate()
        .filter_map(|(gold_index, candidate_index)| {
            candidate_index.map(|candidate_index| {
                (
                    candidates[candidate_index].0.clone(),
                    gold_groups[gold_index].clone(),
                )
            })
        })
        .collect()
}

fn p7_augment_gold_match(
    candidate_index: usize,
    candidates: &[(String, BTreeSet<String>)],
    gold_groups: &[String],
    owner_by_gold: &mut [Option<usize>],
    visited_gold: &mut [bool],
) -> bool {
    for (gold_index, gold_group) in gold_groups.iter().enumerate() {
        if visited_gold[gold_index] || !candidates[candidate_index].1.contains(gold_group) {
            continue;
        }
        visited_gold[gold_index] = true;
        if owner_by_gold[gold_index].is_none_or(|owner| {
            p7_augment_gold_match(owner, candidates, gold_groups, owner_by_gold, visited_gold)
        }) {
            owner_by_gold[gold_index] = Some(candidate_index);
            return true;
        }
    }
    false
}

fn p7_increment(map: &mut BTreeMap<String, usize>, key: &str, value: usize) {
    let entry = map.entry(key.to_string()).or_default();
    *entry = entry.saturating_add(value);
}

fn p7_increment_i64(map: &mut BTreeMap<String, i64>, key: &str, value: i64) {
    let entry = map.entry(key.to_string()).or_default();
    *entry = entry.saturating_add(value);
}

fn add_usize_map(target: &mut BTreeMap<String, usize>, source: &BTreeMap<String, usize>) {
    for (key, value) in source {
        p7_increment(target, key, *value);
    }
}

fn add_i64_map(target: &mut BTreeMap<String, i64>, source: &BTreeMap<String, i64>) {
    for (key, value) in source {
        p7_increment_i64(target, key, *value);
    }
}

fn add_stage_hit_counts(
    target: &mut W4ExternalNoisyStageHitCounts,
    source: &W4ExternalNoisyStageHitCounts,
) {
    target.source_any_evidence_hit += source.source_any_evidence_hit;
    target.source_all_evidence_hit += source.source_all_evidence_hit;
    target.expanded_any_evidence_hit += source.expanded_any_evidence_hit;
    target.expanded_all_evidence_hit += source.expanded_all_evidence_hit;
    target.reranked_any_evidence_hit += source.reranked_any_evidence_hit;
    target.reranked_all_evidence_hit += source.reranked_all_evidence_hit;
    target.selected_any_evidence_hit += source.selected_any_evidence_hit;
    target.selected_all_evidence_hit += source.selected_all_evidence_hit;
    target.projection_selected_any_evidence_hit += source.projection_selected_any_evidence_hit;
    target.projection_selected_all_evidence_hit += source.projection_selected_all_evidence_hit;
    target.rendered_any_evidence_hit += source.rendered_any_evidence_hit;
    target.rendered_all_evidence_hit += source.rendered_all_evidence_hit;
}

fn add_index_diagnostics(
    target: &mut W4ExternalNoisyIndexDiagnostics,
    source: &W4ExternalNoisyIndexDiagnostics,
) {
    macro_rules! add_fields {
        ($($field:ident),+ $(,)?) => {
            $(target.$field = target.$field.saturating_add(source.$field);)+
        };
    }
    add_fields!(
        questions_with_index_report,
        index_used_questions,
        fallback_full_scan_questions,
        source_candidate_count,
        matched_source_anchor_count,
        unmatched_source_anchor_count,
        indexed_neighbor_count,
        filtered_node_count,
        filtered_edge_count,
        filtered_backlink_count,
        failure_count,
        graph_manifest_contract_verified_questions,
        graph_selected_dependency_chain_verified_questions,
        graph_full_scope_closure_verified_questions,
        graph_manifest_generation_present_questions,
        graph_revision_present_questions,
        graph_scope_digest_present_questions,
        graph_maintenance_required_questions,
        graph_incident_questions,
        graph_read_path_mutation_delta,
        facet_questions_with_index_report,
        facet_index_used_questions,
        facet_report_only_questions,
        facet_fallback_full_scan_questions,
        facet_source_candidate_count,
        facet_matched_source_candidate_count,
        facet_posting_key_lookup_count,
        facet_manifest_matched_posting_count,
        facet_posting_doc_read_count,
        facet_owner_key_lookup_count,
        facet_owner_doc_read_count,
        facet_zero_posting_key_lookup_questions,
        facet_clean_zero_hit_questions,
        facet_manifest_integrity_verified_questions,
        facet_manifest_integrity_failure_count,
        facet_exact_match_count,
        facet_expanded_match_count,
        facet_failure_count,
    );
}

fn add_w41_diagnostics(
    target: &mut W4ExternalNoisyW41Diagnostics,
    source: &W4ExternalNoisyW41Diagnostics,
) {
    target.questions_with_w4_1_diagnostics = target
        .questions_with_w4_1_diagnostics
        .saturating_add(source.questions_with_w4_1_diagnostics);
    add_usize_map(
        &mut target.first_any_hit_stage_counts,
        &source.first_any_hit_stage_counts,
    );
    add_usize_map(
        &mut target.first_all_hit_stage_counts,
        &source.first_all_hit_stage_counts,
    );
    add_usize_map(
        &mut target.missing_gold_by_stage_counts,
        &source.missing_gold_by_stage_counts,
    );
    target.miss_after_expanded_count = target
        .miss_after_expanded_count
        .saturating_add(source.miss_after_expanded_count);
    target.gold_rank_found_count = target
        .gold_rank_found_count
        .saturating_add(source.gold_rank_found_count);
    target.gold_rank_missing_count = target
        .gold_rank_missing_count
        .saturating_add(source.gold_rank_missing_count);
    target.gold_rank_sum = target.gold_rank_sum.saturating_add(source.gold_rank_sum);
    target.truncated_count = target
        .truncated_count
        .saturating_add(source.truncated_count);
    add_usize_map(
        &mut target.blocked_reason_counts,
        &source.blocked_reason_counts,
    );
    add_usize_map(
        &mut target.question_type_counts,
        &source.question_type_counts,
    );
    add_usize_map(
        &mut target.evidence_count_buckets,
        &source.evidence_count_buckets,
    );
    target.source_signature_count = target
        .source_signature_count
        .saturating_add(source.source_signature_count);
    target.repeated_source_signature_questions = target
        .repeated_source_signature_questions
        .saturating_add(source.repeated_source_signature_questions);
}

fn add_facet_ablation(
    target: &mut W4ExternalNoisyFacetAblationDiagnostics,
    source: &W4ExternalNoisyFacetAblationDiagnostics,
) {
    target.questions_with_ablation_report += source.questions_with_ablation_report;
    add_usize_map(&mut target.method_counts, &source.method_counts);
    target.delivery_contribution_proven_questions += source.delivery_contribution_proven_questions;
    target.render_growth += source.render_growth;
    add_usize_map(
        &mut target.required_slice_counts,
        &source.required_slice_counts,
    );
    add_usize_map(
        &mut target.report_available_slice_counts,
        &source.report_available_slice_counts,
    );
    add_usize_map(
        &mut target.delivery_contribution_proven_slice_counts,
        &source.delivery_contribution_proven_slice_counts,
    );
    target.delivery_affected_candidate_occurrences +=
        source.delivery_affected_candidate_occurrences;
    add_i64_map(
        &mut target.selected_evidence_hit_delta,
        &source.selected_evidence_hit_delta,
    );
    add_i64_map(
        &mut target.rendered_evidence_hit_delta,
        &source.rendered_evidence_hit_delta,
    );
    add_usize_map(
        &mut target.selected_all_hit_loss_count,
        &source.selected_all_hit_loss_count,
    );
    add_usize_map(
        &mut target.evidence_family_rotation_selected_all_hit_loss_count,
        &source.evidence_family_rotation_selected_all_hit_loss_count,
    );
    add_usize_map(
        &mut target.rendered_all_hit_loss_count,
        &source.rendered_all_hit_loss_count,
    );
    add_i64_map(
        &mut target.expanded_candidate_delta,
        &source.expanded_candidate_delta,
    );
    add_i64_map(
        &mut target.selected_candidate_delta,
        &source.selected_candidate_delta,
    );
    add_i64_map(
        &mut target.rendered_candidate_delta,
        &source.rendered_candidate_delta,
    );
    add_i64_map(&mut target.rendered_char_delta, &source.rendered_char_delta);
    add_usize_map(
        &mut target.blocked_reason_counts,
        &source.blocked_reason_counts,
    );
}

fn add_p7_loss(
    target: &mut W4ExternalNoisyP7LossDiagnostics,
    source: &W4ExternalNoisyP7LossDiagnostics,
) {
    target.questions_with_loss_ledger += source.questions_with_loss_ledger;
    target.expanded_hit_selected_miss_questions += source.expanded_hit_selected_miss_questions;
    target.eval_selected_hit_rendered_miss_questions +=
        source.eval_selected_hit_rendered_miss_questions;
    target.expanded_hit_selected_miss_evidence += source.expanded_hit_selected_miss_evidence;
    target.eval_selected_hit_rendered_miss_evidence +=
        source.eval_selected_hit_rendered_miss_evidence;
    target.eval_selected_hit_projection_selected_miss_questions +=
        source.eval_selected_hit_projection_selected_miss_questions;
    target.eval_selected_hit_projection_selected_miss_evidence +=
        source.eval_selected_hit_projection_selected_miss_evidence;
    target.selected_hit_final_rendered_miss_questions +=
        source.selected_hit_final_rendered_miss_questions;
    target.selected_hit_final_rendered_miss_evidence +=
        source.selected_hit_final_rendered_miss_evidence;
    target.eval_truncated_count += source.eval_truncated_count;
    add_usize_map(
        &mut target.eval_blocked_reason_counts,
        &source.eval_blocked_reason_counts,
    );
}

fn add_p7_production_delivery(
    target: &mut W4ExternalNoisyP7ProductionDeliveryDiagnostics,
    source: &W4ExternalNoisyP7ProductionDeliveryDiagnostics,
) {
    target.questions_with_delivery_report += source.questions_with_delivery_report;
    target.eval_selected_matches_delivery_questions +=
        source.eval_selected_matches_delivery_questions;
    target.eval_rendered_matches_delivery_questions +=
        source.eval_rendered_matches_delivery_questions;
    target.projection_selected_sources_proven_questions +=
        source.projection_selected_sources_proven_questions;
    target.projection_delivery_proof_questions += source.projection_delivery_proof_questions;
    target.final_projection_integrity_questions += source.final_projection_integrity_questions;
    target.final_projection_integrity_passed_questions +=
        source.final_projection_integrity_passed_questions;
    target.final_projection_raw_private_violation_count +=
        source.final_projection_raw_private_violation_count;
    target.final_projection_blocked_source_count += source.final_projection_blocked_source_count;
    target.final_projection_redacted_source_count += source.final_projection_redacted_source_count;
    add_usize_map(
        &mut target.schema_version_counts,
        &source.schema_version_counts,
    );
    target.render_growth += source.render_growth;
    target.privacy_leak_count += source.privacy_leak_count;
    target.cross_subject_leak_count += source.cross_subject_leak_count;
    target.raw_soul_private_material_count += source.raw_soul_private_material_count;
    add_usize_map(
        &mut target.blocked_reason_counts,
        &source.blocked_reason_counts,
    );
    add_usize_map(
        &mut target.delivery_drop_reason_counts,
        &source.delivery_drop_reason_counts,
    );
}

#[cfg(test)]
mod p7_operator_unit_tests {
    use super::*;

    #[test]
    fn soul_exact_contract_rejects_zero_test_cargo_success() {
        let exact = "runtime_lifecycle_contract";
        assert!(!p7_exact_test_stdout_passed(
            b"running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored;\n",
            exact,
        ));
        assert!(p7_exact_test_stdout_passed(
            b"running 1 test\ntest runtime_lifecycle_contract ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n",
            exact,
        ));
    }

    fn expected_question(question_id: &str, question_index: usize) -> P7ExpectedQuestionIdentity {
        P7ExpectedQuestionIdentity {
            case_id: "case-1".to_string(),
            dataset_index: 0,
            question_index,
            question_id: question_id.to_string(),
            question: format!("question-{question_index}"),
            gold_sources: vec!["D1:1".to_string(), "D2:1".to_string()],
        }
    }

    fn detail_row(expected: &P7ExpectedQuestionIdentity) -> serde_json::Value {
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let stage_diagnostics = serde_json::json!({
            "suite": "test_suite",
            "question_id": expected.question_id,
            "question_type": "multi_gold",
            "evidence_count": 2,
            "gold_evidence_refs": [first_group.clone(), second_group.clone()],
            "first_any_hit_stage": "expanded",
            "first_all_hit_stage": "expanded",
            "matched_gold_by_stage": [
                {"stage": "source", "evidence_refs": []},
                {"stage": "expanded", "evidence_refs": [first_group.clone(), second_group.clone()]},
                {"stage": "reranked", "evidence_refs": [first_group.clone(), second_group.clone()]},
                {"stage": "selected", "evidence_refs": [first_group.clone(), second_group.clone()]},
                {"stage": "rendered", "evidence_refs": [first_group.clone(), second_group.clone()]}
            ],
            "missing_gold_by_stage": [
                {"stage": "source", "evidence_refs": [first_group.clone(), second_group.clone()]},
                {"stage": "expanded", "evidence_refs": []},
                {"stage": "reranked", "evidence_refs": []},
                {"stage": "selected", "evidence_refs": []},
                {"stage": "rendered", "evidence_refs": []}
            ],
            "gold_rank_by_stage": [
                {"stage": "source", "evidence_ref": first_group.clone(), "rank": null},
                {"stage": "source", "evidence_ref": second_group.clone(), "rank": null},
                {"stage": "expanded", "evidence_ref": first_group.clone(), "rank": 1},
                {"stage": "expanded", "evidence_ref": second_group.clone(), "rank": 2},
                {"stage": "reranked", "evidence_ref": first_group.clone(), "rank": 1},
                {"stage": "reranked", "evidence_ref": second_group.clone(), "rank": 2},
                {"stage": "selected", "evidence_ref": first_group.clone(), "rank": 1},
                {"stage": "selected", "evidence_ref": second_group.clone(), "rank": 2},
                {"stage": "rendered", "evidence_ref": first_group.clone(), "rank": 1},
                {"stage": "rendered", "evidence_ref": second_group.clone(), "rank": 2}
            ],
            "miss_after_expanded": false,
            "truncated_count": 0,
            "blocked_reasons": [],
            "selected_candidate_ids": ["candidate-1", "candidate-2"],
            "rendered_candidate_ids": ["candidate-1", "candidate-2"]
        });
        let required_slices = [
            "facet_off",
            "rank_fusion_off",
            "coverage_selection_off",
            "delivery_relevance_fusion_off",
            "evidence_family_rotation_off",
            "render_capsule_off",
            "capsule_dedupe_off",
        ];
        let ablation_slices = required_slices
            .iter()
            .map(|name| ablation_slice(name))
            .collect::<Vec<_>>();
        let ablation_report = serde_json::json!({
            "method": "sdk_eval_recall_off_run_v1",
            "required_slices": required_slices,
            "delivery_contribution_proven": true,
            "render_growth": 0,
            "blocked_reasons": [],
            "slices": ablation_slices
        });
        let loss_ledger = serde_json::json!({
            "expanded_hit_selected_miss": [],
            "selected_hit_rendered_miss": [],
            "truncated_count": 0,
            "blocked_reasons": []
        });
        let graph_index_report = serde_json::json!({
            "used": false,
            "fallback_full_scan": false,
            "manifest_contract_verified": false,
            "selected_dependency_chain_verified": false,
            "full_scope_closure_verified": false,
            "manifest_generation_present": false,
            "graph_revision_present": false,
            "scope_digest_present": false,
            "maintenance_required": false,
            "incident_present": false,
            "read_path_mutation_delta": 0,
            "source_candidate_count": 0,
            "matched_source_anchor_count": 0,
            "unmatched_source_anchor_count": 0,
            "indexed_neighbor_count": 0,
            "index_doc_count": 0,
            "filtered_node_count": 0,
            "filtered_edge_count": 0,
            "filtered_backlink_count": 0,
            "failure_count": 0
        });
        let facet_index_report = serde_json::json!({
            "used": true,
            "report_only": false,
            "fallback_full_scan": false,
            "source_candidate_count": 0,
            "matched_source_candidate_count": 0,
            "posting_key_lookup_count": 1,
            "manifest_matched_posting_count": 1,
            "posting_doc_read_count": 1,
            "owner_key_lookup_count": 1,
            "owner_doc_read_count": 1,
            "exact_facet_match_count": 0,
            "expanded_facet_match_count": 0,
            "manifest_owner_doc_count": 0,
            "manifest_posting_doc_count": 0,
            "manifest_integrity_verified": true,
            "render_growth": 0,
            "failure_count": 0,
            "integrity_failure_count": 0
        });
        let index_diagnostics = serde_json::json!({
            "questions_with_index_report": 1,
            "index_used_questions": 0,
            "fallback_full_scan_questions": 0,
            "source_candidate_count": 0,
            "matched_source_anchor_count": 0,
            "unmatched_source_anchor_count": 0,
            "indexed_neighbor_count": 0,
            "filtered_node_count": 0,
            "filtered_edge_count": 0,
            "filtered_backlink_count": 0,
            "failure_count": 0,
            "graph_manifest_contract_verified_questions": 0,
            "graph_selected_dependency_chain_verified_questions": 0,
            "graph_full_scope_closure_verified_questions": 0,
            "graph_manifest_generation_present_questions": 0,
            "graph_revision_present_questions": 0,
            "graph_scope_digest_present_questions": 0,
            "graph_maintenance_required_questions": 0,
            "graph_incident_questions": 0,
            "graph_read_path_mutation_delta": 0,
            "facet_questions_with_index_report": 1,
            "facet_index_used_questions": 1,
            "facet_report_only_questions": 0,
            "facet_fallback_full_scan_questions": 0,
            "facet_source_candidate_count": 0,
            "facet_matched_source_candidate_count": 0,
            "facet_posting_key_lookup_count": 1,
            "facet_manifest_matched_posting_count": 1,
            "facet_posting_doc_read_count": 1,
            "facet_owner_key_lookup_count": 1,
            "facet_owner_doc_read_count": 1,
            "facet_zero_posting_key_lookup_questions": 0,
            "facet_clean_zero_hit_questions": 0,
            "facet_manifest_integrity_verified_questions": 1,
            "facet_manifest_integrity_failure_count": 0,
            "facet_exact_match_count": 0,
            "facet_expanded_match_count": 0,
            "facet_failure_count": 0
        });
        serde_json::json!({
            "schema_version": P7_DETAIL_SCHEMA_VERSION,
            "suite": "test_suite",
            "run_id": "test-run",
            "case_id": expected.case_id,
            "dataset_index": expected.dataset_index,
            "question_index": expected.question_index,
            "question_id": expected.question_id,
            "question": expected.question,
            "question_evaluation": P7QuestionEvaluationContract::from_canonical_gold_count(2),
            "metrics": {},
            "gold_sources": [first_group.clone(), second_group.clone()],
            "selected_sources": [first_group.clone(), second_group.clone()],
            "projection_selected_sources": [first_group.clone(), second_group.clone()],
            "candidate_sources": [first_group.clone(), second_group.clone()],
            "source_sources": [],
            "expanded_sources": [first_group.clone(), second_group.clone()],
            "reranked_sources": [first_group.clone(), second_group.clone()],
            "rendered_sources": [first_group.clone(), second_group.clone()],
            "sdk_stage_candidates": {
                "source": [],
                "expanded": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "reranked": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "eval_selected": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "eval_rendered": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "projection_selected": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "final_rendered": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ]
            },
            "graph_index_report": graph_index_report,
            "facet_index_report": facet_index_report,
            "index_diagnostics": index_diagnostics,
            "stage_diagnostics": stage_diagnostics,
            "ablation_report": ablation_report,
            "p7_loss_ledger": loss_ledger,
            "eval_delivery_report": delivery_report(),
            "final_projection_delivery_report": delivery_report(),
            "sdk_projection_delivery_manifest": projection_delivery_manifest(),
            "runner_projection_digest_observation": projection_delivery_observation(),
            "final_projection_integrity": {
                "checked_surfaces": ["prompt", "ui_api", "operator_raw", "gateway_raw_audit", "shared_fact_surface"],
                "surface_reports": [
                    {"surface": "prompt", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true},
                    {"surface": "ui_api", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true},
                    {"surface": "operator_raw", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true},
                    {"surface": "gateway_raw_audit", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true},
                    {"surface": "shared_fact_surface", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true}
                ],
                "blocked_source_count": 0,
                "redacted_source_count": 0,
                "raw_private_violation_count": 0,
                "passed": true
            },
            "privacy_report": {
                "passed": true,
                "private_raw_candidate_count": 0,
                "failures": []
            },
            "projection_delivery_proven": true,
            "evidence_ref_index": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group]},
                {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group]}
            ],
            "candidate_score_breakdown": [],
            "any_evidence_hit": true,
            "all_evidence_hit": true,
            "write_error": null,
            "recall_error": null
        })
    }

    fn no_gold_detail_row(expected: &P7ExpectedQuestionIdentity) -> serde_json::Value {
        let mut row = detail_row(expected);
        let empty_stage_matrix = serde_json::json!([
            {"stage": "source", "evidence_refs": []},
            {"stage": "expanded", "evidence_refs": []},
            {"stage": "reranked", "evidence_refs": []},
            {"stage": "selected", "evidence_refs": []},
            {"stage": "rendered", "evidence_refs": []}
        ]);
        row["question_evaluation"] =
            serde_json::to_value(P7QuestionEvaluationContract::from_canonical_gold_count(0))
                .expect("serialize no-gold question contract");
        row["gold_sources"] = serde_json::json!([]);
        row["ablation_report"]["slices"] = serde_json::json!([]);
        row["ablation_report"]["required_slices"] = serde_json::json!([]);
        row["ablation_report"]["delivery_contribution_proven"] = serde_json::json!(false);
        row["any_evidence_hit"] = serde_json::json!(false);
        row["all_evidence_hit"] = serde_json::json!(false);
        row["stage_diagnostics"]["question_type"] = serde_json::json!("no_gold");
        row["stage_diagnostics"]["evidence_count"] = serde_json::json!(0);
        row["stage_diagnostics"]["gold_evidence_refs"] = serde_json::json!([]);
        row["stage_diagnostics"]["first_any_hit_stage"] = serde_json::Value::Null;
        row["stage_diagnostics"]["first_all_hit_stage"] = serde_json::Value::Null;
        row["stage_diagnostics"]["matched_gold_by_stage"] = empty_stage_matrix.clone();
        row["stage_diagnostics"]["missing_gold_by_stage"] = empty_stage_matrix;
        row["stage_diagnostics"]["gold_rank_by_stage"] = serde_json::json!([]);
        row
    }

    fn ablation_slice(name: &str) -> serde_json::Value {
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        serde_json::json!({
            "name": name,
            "feature_enabled": false,
            "report_available": true,
            "delivery_contribution_proven": true,
            "candidate_boundary_proven": true,
            "delivery_affected_candidate_ids": ["candidate-2"],
            "delivery_affected_candidate_count": 1,
            "sdk_delivery_affected_candidate_count_claim": 1,
            "baseline_selected_evidence_refs": [first_group.clone(), second_group.clone()],
            "off_run_selected_evidence_refs": [first_group.clone()],
            "baseline_rendered_evidence_refs": [first_group.clone(), second_group.clone()],
            "off_run_rendered_evidence_refs": [first_group.clone()],
            "baseline_selected_candidate_ids": ["candidate-1", "candidate-2"],
            "off_run_selected_candidate_ids": ["candidate-1"],
            "baseline_rendered_candidate_ids": ["candidate-1", "candidate-2"],
            "off_run_rendered_candidate_ids": ["candidate-1"],
            "baseline_selected_candidates": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
            ],
            "off_run_selected_candidates": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]}
            ],
            "baseline_rendered_candidates": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
            ],
            "off_run_rendered_candidates": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group]}
            ],
            "baseline_expanded_candidate_count": 2,
            "off_run_expanded_candidate_count": 2,
            "baseline_selected_candidate_count": 2,
            "off_run_selected_candidate_count": 1,
            "baseline_rendered_candidate_count": 2,
            "off_run_rendered_candidate_count": 1,
            "baseline_rendered_chars": 64,
            "off_run_rendered_chars": 32,
            "baseline_render_growth": 0,
            "off_run_render_growth": 0,
            "selected_evidence_hit_delta": 1,
            "rendered_evidence_hit_delta": 1,
            "selected_all_hit_lost": true,
            "rendered_all_hit_lost": true,
            "expanded_candidate_delta": 0,
            "selected_candidate_delta": 1,
            "rendered_candidate_delta": 1,
            "rendered_char_delta": 32,
            "render_growth": 0,
            "blocked_reasons": []
        })
    }

    fn delivery_report() -> serde_json::Value {
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let first_opaque_reference = format!("opaque:evidence:{}", "a".repeat(64));
        let second_opaque_reference = format!("opaque:evidence:{}", "b".repeat(64));
        let first_visible_group =
            p7_canonical_groups(std::slice::from_ref(&first_opaque_reference))
                .into_iter()
                .next()
                .expect("canonical first visible group");
        let second_visible_group =
            p7_canonical_groups(std::slice::from_ref(&second_opaque_reference))
                .into_iter()
                .next()
                .expect("canonical second visible group");
        serde_json::json!({
            "schema_version": MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            "owner": "sdk_recall_delivery",
            "selection_strategy": "profile_bounded_exact_evidence_coverage_with_relevance_fusion_v2",
            "render_strategy": "governed_evidence_capsule_v1",
            "selected_candidate_ids": ["candidate-1", "candidate-2"],
            "selection_decisions": [
                {
                    "candidate_id": "candidate-1",
                    "canonical_evidence_groups": [first_group.clone()],
                    "evidence_family_groups": [],
                    "selected": true,
                    "drop_reason": null
                },
                {
                    "candidate_id": "candidate-2",
                    "canonical_evidence_groups": [second_group.clone()],
                    "evidence_family_groups": [],
                    "selected": true,
                    "drop_reason": null
                }
            ],
            "rendered_capsules": [
                {
                    "candidate_id": "candidate-1",
                    "evidence_ref_views": [{"visibility": "governed_opaque", "reference": first_opaque_reference, "reason": "evidence_ref_governed_opaque"}],
                    "visible_evidence_refs": [first_visible_group],
                    "canonical_evidence_groups": [first_group],
                    "source_locator_view": {"visibility": "governed_opaque", "reference": format!("opaque:governed-source:scope:{}:evidence_source_ref", "a".repeat(64)), "reason": "source_locator_governed_opaque"},
                    "redaction_state": "public_runtime",
                    "shared_fact_surface_allowed": true,
                    "rendered_chars": 10
                },
                {
                    "candidate_id": "candidate-2",
                    "evidence_ref_views": [{"visibility": "governed_opaque", "reference": second_opaque_reference, "reason": "evidence_ref_governed_opaque"}],
                    "visible_evidence_refs": [second_visible_group],
                    "canonical_evidence_groups": [second_group],
                    "source_locator_view": {"visibility": "governed_opaque", "reference": format!("opaque:governed-source:scope:{}:evidence_source_ref", "b".repeat(64)), "reason": "source_locator_governed_opaque"},
                    "redaction_state": "public_runtime",
                    "shared_fact_surface_allowed": true,
                    "rendered_chars": 10
                }
            ],
            "covered_evidence_family_groups": [],
            "render_decisions": [],
            "render_budget_chars": 100,
            "rendered_chars": 20,
            "render_growth": 0,
            "integrity_failures": [],
            "delivery_drop_reasons": []
        })
    }

    fn projection_delivery_manifest() -> serde_json::Value {
        serde_json::json!({
            "schema_version": MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION,
            "system_memory_block_sha256": "3".repeat(64),
            "deterministic_envelope_sha256": "4".repeat(64),
            "exact_render_match": true,
            "capsule_entries": [
                {"owner_identity_token": projection_owner_identity_token('7'), "candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"owner_identity_token": projection_owner_identity_token('8'), "candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "governed_block_entries": [
                {"owner_identity_token": projection_owner_identity_token('7'), "candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"owner_identity_token": projection_owner_identity_token('8'), "candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "prompt_visible_entries": [
                {"owner_identity_token": projection_owner_identity_token('7'), "candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"owner_identity_token": projection_owner_identity_token('8'), "candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "candidate_receipts": [
                {"owner_identity_token": projection_owner_identity_token('7'), "candidate_id": "candidate-1", "source_block_sha256": "5".repeat(64)},
                {"owner_identity_token": projection_owner_identity_token('8'), "candidate_id": "candidate-2", "source_block_sha256": "6".repeat(64)}
            ],
            "integrity_failures": []
        })
    }

    fn projection_delivery_observation() -> serde_json::Value {
        serde_json::json!({
            "schema_version": P7_RUNNER_PROJECTION_DIGEST_OBSERVATION_SCHEMA_VERSION,
            "system_memory_block_sha256": "3".repeat(64),
            "runtime_envelope_sha256": "4".repeat(64),
            "capsule_entries": [
                {"owner_identity_token": projection_owner_identity_token('7'), "candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"owner_identity_token": projection_owner_identity_token('8'), "candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "governed_block_entries": [
                {"owner_identity_token": projection_owner_identity_token('7'), "candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"owner_identity_token": projection_owner_identity_token('8'), "candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "prompt_visible_entries": [
                {"owner_identity_token": projection_owner_identity_token('7'), "candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"owner_identity_token": projection_owner_identity_token('8'), "candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "candidate_receipts": [
                {"owner_identity_token": projection_owner_identity_token('7'), "candidate_id": "candidate-1", "source_block_sha256": "5".repeat(64)},
                {"owner_identity_token": projection_owner_identity_token('8'), "candidate_id": "candidate-2", "source_block_sha256": "6".repeat(64)}
            ]
        })
    }

    fn projection_owner_identity_token(digest_digit: char) -> String {
        format!(
            "{P7_PROJECTION_OWNER_IDENTITY_TOKEN_PREFIX}{}",
            digest_digit.to_string().repeat(64)
        )
    }

    fn remove_projection_manifest_entry(row: &mut serde_json::Value, index: usize) {
        for field in [
            "capsule_entries",
            "governed_block_entries",
            "prompt_visible_entries",
            "candidate_receipts",
        ] {
            row["sdk_projection_delivery_manifest"][field]
                .as_array_mut()
                .expect("projection delivery manifest entries")
                .remove(index);
        }
    }

    fn remove_projection_observation_entry(row: &mut serde_json::Value, index: usize) {
        for field in [
            "capsule_entries",
            "governed_block_entries",
            "prompt_visible_entries",
            "candidate_receipts",
        ] {
            row["runner_projection_digest_observation"][field]
                .as_array_mut()
                .expect("runner projection digest observation entries")
                .remove(index);
        }
    }

    fn write_detail(rows: &[serde_json::Value], name: &str) -> (PathBuf, String) {
        let path =
            std::env::temp_dir().join(format!("bm-p7-detail-{}-{name}.jsonl", std::process::id()));
        let mut bytes = Vec::new();
        for row in rows {
            bytes.extend(serde_json::to_vec(row).expect("serialize detail row"));
            bytes.push(b'\n');
        }
        fs::write(&path, &bytes).expect("write detail fixture");
        let digest = format!("{:x}", Sha256::digest(&bytes));
        (path, digest)
    }

    fn verify_rows(
        rows: &[serde_json::Value],
        expected: &[P7ExpectedQuestionIdentity],
        name: &str,
    ) -> Result<P7DetailAggregate> {
        let (path, digest) = write_detail(rows, name);
        let mut file = File::open(&path).expect("open detail fixture");
        let result = validate_p7_detail_file(
            &mut file,
            &digest,
            P7DetailValidationContext {
                suite: "test_suite",
                run_id: "test-run",
                detail_schema_version: P7_DETAIL_SCHEMA_VERSION,
                expected_questions: expected,
                expected_samples: 1,
            },
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        );
        let _ = fs::remove_file(path);
        result
    }

    #[test]
    fn detail_line_reader_accepts_max_and_rejects_max_plus_one_during_read() {
        let mut accepted = BufReader::new(std::io::Cursor::new(b"1234567\n".to_vec()));
        let mut line = Vec::new();
        assert_eq!(
            p7_read_bounded_line(&mut accepted, &mut line, 8).expect("exact line bound"),
            8
        );
        assert_eq!(line, b"1234567\n");

        let mut rejected = BufReader::new(std::io::Cursor::new(b"12345678\n".to_vec()));
        assert!(p7_read_bounded_line(&mut rejected, &mut line, 8).is_err());
        assert!(
            line.len() <= 8,
            "reader must reject before growing past the bound"
        );
    }

    #[test]
    fn detail_metadata_admission_accepts_individual_exact_and_rejects_plus_one() {
        let mut exact = P7ArtifactReadLedger::default();
        exact
            .admit(
                Path::new("/detail-exact"),
                P7_MAX_DETAIL_ARTIFACT_BYTES,
                P7ArtifactReadKind::Detail,
            )
            .expect("individual detail exact boundary");

        let mut plus_one = P7ArtifactReadLedger::default();
        assert!(plus_one
            .admit(
                Path::new("/detail-plus-one"),
                P7_MAX_DETAIL_ARTIFACT_BYTES + 1,
                P7ArtifactReadKind::Detail,
            )
            .is_err());
    }

    #[test]
    fn detail_metadata_admission_accepts_all_detail_exact_and_rejects_plus_one() {
        let mut exact = P7ArtifactReadLedger::default();
        for index in 0..4 {
            exact
                .admit(
                    Path::new(match index {
                        0 => "/detail-0",
                        1 => "/detail-1",
                        2 => "/detail-2",
                        _ => "/detail-3",
                    }),
                    P7_MAX_DETAIL_ARTIFACT_BYTES,
                    P7ArtifactReadKind::Detail,
                )
                .expect("all-detail exact boundary");
        }
        assert_eq!(
            exact.admitted_detail_bytes,
            P7_MAX_ALL_DETAIL_ARTIFACT_BYTES
        );
        assert!(exact
            .admit(Path::new("/detail-plus-one"), 1, P7ArtifactReadKind::Detail,)
            .is_err());
    }

    #[test]
    fn global_metadata_admission_is_one_ten_gib_cohort_budget() {
        let mut exact = P7ArtifactReadLedger::default();
        exact
            .admit(
                Path::new("/cohort-exact"),
                P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES,
                P7ArtifactReadKind::Control,
            )
            .expect("global exact boundary");

        let mut plus_one = P7ArtifactReadLedger::default();
        assert!(plus_one
            .admit(
                Path::new("/cohort-plus-one"),
                P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES + 1,
                P7ArtifactReadKind::Control,
            )
            .is_err());
    }

    #[test]
    fn shared_artifact_evidence_allows_only_one_full_read_pass() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-shared-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("shared read root");
        let path = root.join("preflight.json");
        fs::write(&path, b"{\"run_id\":\"run-1\"}\n").expect("shared evidence fixture");
        let mut session = P7ArtifactReadSession::default();
        session
            .read_json::<serde_json::Value>(
                &path,
                &root,
                &root,
                None,
                P7ArtifactReadKind::Control,
                "p7_test_parse_shared_evidence",
            )
            .expect("first shared evidence full read");
        assert!(session
            .read_json::<serde_json::Value>(
                &path,
                &root,
                &root,
                None,
                P7ArtifactReadKind::Control,
                "p7_test_reparse_shared_evidence",
            )
            .is_err());
        let performance = session.ledger.performance(Duration::ZERO);
        assert_eq!(performance.unique_artifact_count, 1);
        assert_eq!(performance.full_read_pass_count, 1);
        assert_eq!(
            performance.admitted_artifact_bytes,
            performance.artifact_bytes_read
        );
        assert_eq!(performance.detail_artifact_bytes_read, 0);
        assert_eq!(performance.duplicate_artifact_count, 1);
        assert!(!performance.passed);
        drop(session);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn eight_shard_read_session_streams_dataset_once_and_each_bundle_artifact_once() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-eight-shard-session-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let data_dir = root.join("data");
        let cohort_dir = root.join("results/runs/run-8");
        fs::create_dir_all(&data_dir).expect("eight-shard data dir");
        fs::create_dir_all(&cohort_dir).expect("eight-shard cohort dir");
        let dataset = data_dir.join("dataset.json");
        fs::write(&dataset, b"{\"dataset\":\"shared\"}\n").expect("shared dataset");

        let mut session = P7ArtifactReadSession::default();
        session
            .read_raw(
                &dataset,
                &data_dir,
                &root,
                None,
                P7ArtifactReadKind::Dataset,
            )
            .expect("single shared dataset stream");
        for shard_index in 0..8 {
            for (suffix, kind) in [
                ("commit.json", P7ArtifactReadKind::Control),
                ("summary.json", P7ArtifactReadKind::Summary),
                ("jsonl", P7ArtifactReadKind::Detail),
            ] {
                let path = cohort_dir.join(format!("locomo.shard-{shard_index}-of-8.{suffix}"));
                fs::write(&path, format!("shard={shard_index};kind={suffix}\n"))
                    .expect("shard artifact");
                session
                    .read_raw(&path, &cohort_dir, &root, None, kind)
                    .expect("single shard artifact stream");
            }
        }

        session.verify_retained().expect("retained artifact set");
        let receipt = session.lifecycle_receipt("eight_shard_fixture");
        assert_eq!(receipt.unique_artifact_count, 25);
        assert_eq!(receipt.full_read_pass_count, 25);
        assert_eq!(receipt.duplicate_artifact_count, 0);
        assert!(receipt.detail_artifact_bytes_read > 0);
        assert!(receipt.passed);
        drop(session);
        fs::remove_dir_all(root).expect("remove eight-shard fixture");
    }

    #[cfg(unix)]
    #[test]
    fn read_session_rejects_hard_link_alias_before_second_full_read() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-hard-link-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("hard-link read root");
        let first = root.join("first.json");
        let alias = root.join("alias.json");
        fs::write(
            &first,
            br#"{"run_id":"run-1"}
"#,
        )
        .expect("hard-link fixture");
        fs::hard_link(&first, &alias).expect("hard-link alias");

        let mut session = P7ArtifactReadSession::default();
        session
            .read_json::<serde_json::Value>(
                &first,
                &root,
                &root,
                None,
                P7ArtifactReadKind::Control,
                "p7_test_parse_hard_link_source",
            )
            .expect("first physical artifact read");
        assert!(session
            .read_json::<serde_json::Value>(
                &alias,
                &root,
                &root,
                None,
                P7ArtifactReadKind::Control,
                "p7_test_parse_hard_link_alias",
            )
            .is_err());
        let performance = session.performance(Duration::ZERO);
        assert_eq!(performance.unique_artifact_count, 1);
        assert_eq!(performance.full_read_pass_count, 1);
        assert_eq!(performance.duplicate_artifact_count, 1);
        assert!(!performance.passed);

        drop(session);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn read_session_rejects_same_path_and_noncanonical_alias_before_second_full_read() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-path-alias-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("path alias read root");
        let path = root.join("artifact.json");
        fs::write(&path, b"{}\n").expect("path alias fixture");

        let mut session = P7ArtifactReadSession::default();
        session
            .read_raw(&path, &root, &root, None, P7ArtifactReadKind::Control)
            .expect("first path read");
        assert!(session
            .read_raw(&path, &root, &root, None, P7ArtifactReadKind::Control)
            .is_err());
        let noncanonical = root.join(".").join("artifact.json");
        assert!(session
            .read_raw(
                &noncanonical,
                &root,
                &root,
                None,
                P7ArtifactReadKind::Control,
            )
            .is_err());
        let performance = session.performance(Duration::ZERO);
        assert_eq!(performance.unique_artifact_count, 1);
        assert_eq!(performance.full_read_pass_count, 1);
        assert_eq!(performance.duplicate_artifact_count, 2);

        drop(session);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn read_session_rejects_artifact_modified_during_streaming() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-stream-mutation-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("stream mutation root");
        let path = root.join("artifact.bin");
        fs::write(&path, b"stable bytes\n").expect("stream mutation fixture");

        let mut session = P7ArtifactReadSession::default();
        let result = session.read_with(
            &path,
            &root,
            &root,
            None,
            P7ArtifactReadKind::Control,
            |reader, _| {
                let mut first = [0_u8; 1];
                reader.read_exact(&mut first).map_err(|source| Error::Io {
                    source,
                    stage: "p7_test_read_before_stream_mutation",
                })?;
                fs::write(&path, b"changed bytes with a different length\n").map_err(|source| {
                    Error::Io {
                        source,
                        stage: "p7_test_mutate_streamed_artifact",
                    }
                })?;
                std::io::copy(reader, &mut std::io::sink()).map_err(|source| Error::Io {
                    source,
                    stage: "p7_test_finish_mutated_stream",
                })?;
                Ok(())
            },
        );
        assert!(result.is_err());

        drop(session);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn current_operator_rejects_non_retained_test_process_launch() {
        assert!(attest_p7_current_verifier_execution().is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires the trusted Linux release-profile execution gate"]
    fn p7_sealed_execution_identity_binds_memfd_bytes_to_release_manifest() {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(P7_OPERATOR_BUILD_PROFILE, "release");
        let source = fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let source_identity = crate::p7_secure_fs::P7RetainedFile::open_executable(&source)
            .expect("retain release test executable")
            .hash_once()
            .expect("hash release test executable");
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!(
                "bm-p7-sealed-identity-{}-{nonce}",
                std::process::id()
            ));
        let release = root.join(&source_identity.sha256);
        fs::create_dir_all(&release).expect("create content-addressed release");
        let executable_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .expect("test executable name");
        let executable = release.join(executable_name);
        fs::copy(&source, &executable).expect("copy release test executable");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o555))
            .expect("set release executable mode");
        let mut build_features = p7_operator_build_features();
        build_features.sort();
        build_features.dedup();
        let manifest = P7VerifierReleaseManifest {
            schema_version: P7_VERIFIER_RELEASE_MANIFEST_SCHEMA_VERSION.to_string(),
            executable_file_name: executable_name.to_string(),
            executable_sha256: source_identity.sha256.clone(),
            build_profile: P7_OPERATOR_BUILD_PROFILE.to_string(),
            build_features,
            verification_policy_contract: P7_VERIFICATION_POLICY_CONTRACT.to_string(),
            verification_schema_version: P7_VERIFICATION_RECEIPT_SCHEMA_VERSION.to_string(),
            source_anchor_sha256: P7_EMBEDDED_OPERATOR_BUILD_FINGERPRINT.to_string(),
            frozen_anchor_sha256: P7_FROZEN_ANCHOR_SHA256.to_string(),
            anchor_generator_receipt_sha256: P7_FROZEN_ANCHOR_GENERATOR_RECEIPT_SHA256.to_string(),
        };
        let mut manifest_bytes = serde_json::to_vec_pretty(&manifest).expect("serialize manifest");
        manifest_bytes.push(b'\n');
        fs::write(
            release.join(P7_VERIFIER_RELEASE_MANIFEST_FILE_NAME),
            manifest_bytes,
        )
        .expect("write release manifest");

        let mut retained = crate::p7_secure_fs::P7RetainedFile::open_executable(&executable)
            .expect("retain published verifier");
        let args = vec![
            "bench::p7_operator_unit_tests::p7_sealed_execution_identity_child".to_string(),
            "--exact".to_string(),
            "--ignored".to_string(),
            "--nocapture".to_string(),
        ];
        let (mut command, guard, _) = retained
            .executable_command(&args)
            .expect("build sealed verifier command");
        let output = command.output().expect("run sealed verifier child");
        drop(guard);
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "subprocess child"]
    fn p7_sealed_execution_identity_child() {
        let expected_sha256 =
            std::env::var("BM_P7_RETAINED_EXECUTABLE_SHA256").expect("launcher executable SHA256");
        let mut authority = attest_p7_current_verifier_execution()
            .expect("sealed execution must bind to release manifest");
        let identity = authority.identity();
        assert_eq!(identity.operator_executable_sha256, expected_sha256);
        authority
            .verify_retained()
            .expect("retained release manifest and locator");
    }

    #[test]
    fn retained_session_rejects_same_byte_current_path_file_id_replacement() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!(
                "bm-p7-retained-path-replacement-{}",
                std::process::id()
            ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("replacement test root");
        let path = root.join("artifact.bin");
        let replacement = root.join("replacement.bin");
        fs::write(&path, b"stable\n").expect("initial artifact");
        fs::write(&replacement, b"stable\n").expect("same-byte replacement");

        let mut session = P7ArtifactReadSession::default();
        session
            .read_raw(&path, &root, &root, None, P7ArtifactReadKind::Control)
            .expect("initial retained read");
        let retained_identity = session.retained[0].artifact.identity.clone();

        fs::remove_file(&path).expect("unlink retained path");
        fs::rename(&replacement, &path).expect("install same-byte replacement");
        let current_owner = P7RetainedDirectoryOwner::open_root(&root)
            .expect("open replacement owner without following reparse points");
        let current = current_owner
            .open_existing_file("artifact.bin")
            .expect("open replacement without following reparse points");
        assert_ne!(
            p7_platform_file_identity(&current).expect("replacement file identity"),
            retained_identity
        );
        assert!(session.verify_retained().is_err());

        drop(session);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn retained_session_rejects_parent_directory_replacement() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-retained-parent-swap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("results/runs/run-1")).expect("retained cohort path");
        let cohort = root.join("results/runs/run-1");
        let artifact = cohort.join("artifact.json");
        fs::write(&artifact, b"{}\n").expect("retained parent artifact");

        let mut session = P7ArtifactReadSession::default();
        session
            .read_json::<serde_json::Value>(
                &artifact,
                &cohort,
                &root,
                None,
                P7ArtifactReadKind::Control,
                "p7_test_parent_swap_parse",
            )
            .expect("initial retained parent read");

        let displaced = root.join("results/runs/run-1.displaced");
        fs::rename(&cohort, &displaced).expect("displace retained cohort directory");
        fs::create_dir(&cohort).expect("install replacement cohort directory");
        fs::write(cohort.join("artifact.json"), b"{}\n").expect("replacement artifact");
        assert!(session.verify_retained().is_err());

        drop(session);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn retained_session_accepts_exact_length_and_rejects_one_byte_growth() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-retained-growth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("growth test root");
        let exact = root.join("exact.bin");
        fs::write(&exact, b"exact").expect("exact fixture");
        let mut exact_session = P7ArtifactReadSession::default();
        exact_session
            .read_raw(&exact, &root, &root, None, P7ArtifactReadKind::Control)
            .expect("exact admitted length");
        exact_session
            .verify_retained()
            .expect("exact retained file");

        let growing = root.join("growing.bin");
        fs::write(&growing, b"exact").expect("growth fixture");
        let mut growth_session = P7ArtifactReadSession::default();
        let result = growth_session.read_with(
            &growing,
            &root,
            &root,
            None,
            P7ArtifactReadKind::Control,
            |reader, admitted_len| {
                let mut admitted = vec![0_u8; admitted_len as usize];
                reader
                    .read_exact(&mut admitted)
                    .map_err(|source| Error::Io {
                        source,
                        stage: "p7_test_read_admitted_growth_bytes",
                    })?;
                let mut writer =
                    fs::OpenOptions::new()
                        .append(true)
                        .open(&growing)
                        .map_err(|source| Error::Io {
                            source,
                            stage: "p7_test_open_growth_writer",
                        })?;
                use std::io::Write as _;
                writer.write_all(b"+").map_err(|source| Error::Io {
                    source,
                    stage: "p7_test_append_growth_byte",
                })?;
                Ok(())
            },
        );
        assert!(result.is_err());

        drop(exact_session);
        drop(growth_session);
        let _ = fs::remove_dir_all(root);
    }

    fn test_shard_expectation(run_id: &str) -> P7ShardBundleExpectation {
        P7ShardBundleExpectation {
            run_id: run_id.to_string(),
            suite: "locomo".to_string(),
            shard_index: 0,
            shard_total: 1,
            limit: None,
            question_limit: None,
            question_index: None,
            build: P7RunnerBuildIdentity {
                sdk_build_fingerprint: "1".repeat(64),
                runner_build_fingerprint: "2".repeat(64),
                runner_lock_fingerprint: "3".repeat(64),
                executable_sha256: "4".repeat(64),
                build_profile: "release".to_string(),
            },
            release: P7PublishedReleaseIdentity {
                gate_attestation_sha256: "5".repeat(64),
                release_metadata_sha256: "6".repeat(64),
                gate_source_fingerprint: "7".repeat(64),
                gate_source_manifest_sha256: "8".repeat(64),
                gate_ids: P7_REQUIRED_RELEASE_GATE_IDS
                    .iter()
                    .map(|gate| (*gate).to_string())
                    .collect(),
                ..P7PublishedReleaseIdentity::default()
            },
            execution_kind: P7ProducerExecutionKind::CohortShard,
            cohort_admission_sha256: "9".repeat(64),
        }
    }

    #[test]
    fn shard_bundle_owner_classifies_absent_and_uncommitted_partial_pair() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-shard-partial-pair-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cohort = root.join("results/runs/run-1");
        fs::create_dir_all(&cohort).expect("partial pair cohort");
        let expectation = test_shard_expectation("run-1");
        assert!(matches!(
            verify_p7_shard_bundle_with_receipt(&root, &expectation)
                .expect("absent shard bundle")
                .0,
            P7ShardBundleState::Absent
        ));

        fs::write(cohort.join("locomo.shard-0-of-1.summary.json"), b"{}\n")
            .expect("partial summary");
        assert!(matches!(
            verify_p7_shard_bundle_with_receipt(&root, &expectation)
                .expect("uncommitted summary")
                .0,
            P7ShardBundleState::Uncommitted(P7UncommittedShardBundle {
                summary_present: true,
                detail_present: false,
            })
        ));
        fs::remove_file(cohort.join("locomo.shard-0-of-1.summary.json"))
            .expect("remove partial summary fixture");
        fs::write(cohort.join("locomo.shard-0-of-1.jsonl"), b"{}\n").expect("partial detail");
        assert!(matches!(
            verify_p7_shard_bundle_with_receipt(&root, &expectation)
                .expect("uncommitted detail")
                .0,
            P7ShardBundleState::Uncommitted(P7UncommittedShardBundle {
                summary_present: false,
                detail_present: true,
            })
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn merged_resume_rejects_giant_json_before_parse_allocation() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-merged-giant-json-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cohort = root.join("results/runs/run-1");
        fs::create_dir_all(&cohort).expect("giant merged cohort");
        let merged = cohort.join("locomo.merged.summary.json");
        let file = File::create(&merged).expect("giant merged fixture");
        file.set_len(P7_MAX_CONTROL_JSON_BYTES + 1)
            .expect("sparse giant merged fixture");
        assert!(verify_p7_merged_resume_with_receipt(
            &root,
            "run-1",
            "locomo",
            &serde_json::json!({}),
        )
        .expect("uncommitted giant summary is ignored")
        .0
        .is_none());

        let commit = P7MergedBundleCommit {
            schema_version: P7_MERGED_BUNDLE_COMMIT_SCHEMA_VERSION.to_string(),
            run_id: "run-1".to_string(),
            suite: "locomo".to_string(),
            summary_file: "locomo.merged.summary.json".to_string(),
            summary_bytes: P7_MAX_CONTROL_JSON_BYTES + 1,
            summary_sha256: "a".repeat(64),
        };
        fs::write(
            cohort.join("locomo.merged.commit.json"),
            serde_json::to_vec(&commit).expect("serialize merged commit"),
        )
        .expect("publish merged commit fixture");
        assert!(verify_p7_merged_resume_with_receipt(
            &root,
            "run-1",
            "locomo",
            &serde_json::json!({}),
        )
        .is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn retained_session_rejects_current_path_symlink_replacement() {
        use std::os::unix::fs::symlink;

        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!("bm-p7-retained-symlink-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("symlink replacement root");
        let path = root.join("artifact.bin");
        let replacement = root.join("replacement.bin");
        fs::write(&path, b"stable\n").expect("initial artifact");
        fs::write(&replacement, b"stable\n").expect("replacement target");

        let mut session = P7ArtifactReadSession::default();
        session
            .read_raw(&path, &root, &root, None, P7ArtifactReadKind::Control)
            .expect("initial retained read");
        fs::remove_file(&path).expect("unlink retained path");
        symlink(&replacement, &path).expect("install symlink replacement");
        assert!(session.verify_retained().is_err());

        drop(session);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn final_wall_fresh_rss_revalidation_rejects_post_admission_evidence_replacement() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!(
                "bm-p7-post-admission-rss-replacement-{}",
                std::process::id()
            ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("RSS replacement root");
        let admission = root.join(P7_COHORT_ADMISSION_FILE_NAME);
        fs::write(&admission, b"admitted\n").expect("admission fixture");

        for artifact_name in [
            P7_MAXIMUM_RSS_MEASUREMENT_FILE_NAME,
            "trusted-dataset.json",
            "maximum-rss-detail.jsonl",
            "runner.stdout.log",
            "runner.time.log",
        ] {
            let artifact = root.join(artifact_name);
            let original = format!("original-{artifact_name}\n");
            fs::write(&artifact, original.as_bytes()).expect("original RSS evidence");
            let expected_sha256 = format!("{:x}", Sha256::digest(original.as_bytes()));

            let mut admission_session = P7ArtifactReadSession::default();
            admission_session
                .read_raw(&admission, &root, &root, None, P7ArtifactReadKind::Control)
                .expect("cohort admission read");
            admission_session
                .verify_retained()
                .expect("admission remained stable");

            fs::write(&artifact, format!("replaced-{artifact_name}\n"))
                .expect("replace admitted RSS evidence");
            let mut final_wall_session = P7ArtifactReadSession::default();
            assert!(
                final_wall_session
                    .read_raw(
                        &artifact,
                        &root,
                        &root,
                        Some(&expected_sha256),
                        P7ArtifactReadKind::Control,
                    )
                    .is_err(),
                "final wall accepted replaced {artifact_name} after admission"
            );
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detail_recomputation_accepts_exact_final_projection_facts() {
        let expected = expected_question("q-1", 0);
        let aggregate = verify_rows(&[detail_row(&expected)], &[expected], "valid")
            .expect("exact detail should verify");

        assert_eq!(aggregate.questions, 1);
        assert_eq!(aggregate.stage_hit_counts.rendered_all_evidence_hit, 1);
        assert_eq!(
            aggregate
                .facet_ablation
                .evidence_family_rotation_selected_all_hit_loss_count
                .get("evidence_family_rotation_off"),
            Some(&1)
        );
        assert_eq!(
            aggregate
                .p7_production_delivery
                .projection_delivery_proof_questions,
            1
        );
    }

    #[test]
    fn no_gold_question_is_typed_not_applicable_and_excluded_from_evidence_metrics() {
        let mut expected = expected_question("q-no-gold", 0);
        expected.gold_sources.clear();
        let row = no_gold_detail_row(&expected);

        let aggregate = verify_rows(&[row], &[expected], "no-gold")
            .expect("typed no-gold detail should verify without ablation");

        assert_eq!(aggregate.questions, 1);
        assert_eq!(aggregate.evidence_questions, 0);
        assert_eq!(aggregate.any_evidence_hit, 0);
        assert_eq!(aggregate.all_evidence_hit, 0);
        assert_eq!(aggregate.facet_ablation.questions_with_ablation_report, 0);
        assert_eq!(
            aggregate
                .w4_1_diagnostics
                .question_type_counts
                .get("no_gold"),
            Some(&1)
        );
    }

    #[test]
    fn no_gold_question_rejects_ablation_and_missing_typed_contract() {
        let mut expected = expected_question("q-no-gold-invalid", 0);
        expected.gold_sources.clear();

        let mut with_ablation = no_gold_detail_row(&expected);
        with_ablation["ablation_report"] = serde_json::json!({});
        assert!(verify_rows(
            &[with_ablation],
            std::slice::from_ref(&expected),
            "no-gold-with-ablation"
        )
        .is_err());

        let mut missing_contract = no_gold_detail_row(&expected);
        missing_contract
            .as_object_mut()
            .expect("detail object")
            .remove("question_evaluation");
        assert!(verify_rows(&[missing_contract], &[expected], "no-gold-missing-contract").is_err());
    }

    #[test]
    fn no_gold_question_rejects_each_contradictory_ablation_claim() {
        let mut expected = expected_question("q-no-gold-contradiction", 0);
        expected.gold_sources.clear();

        for (field, value) in [
            ("required_slices", serde_json::json!(["facet_off"])),
            ("delivery_contribution_proven", serde_json::json!(true)),
            ("render_growth", serde_json::json!(1)),
            ("blocked_reasons", serde_json::json!(["unexpected"])),
        ] {
            let mut row = no_gold_detail_row(&expected);
            row["ablation_report"][field] = value;
            assert!(
                verify_rows(
                    std::slice::from_ref(&row),
                    std::slice::from_ref(&expected),
                    field
                )
                .is_err(),
                "no-gold contract accepted contradictory {field}"
            );
        }
    }

    #[test]
    fn detail_recomputation_rejects_raw_capsule_source_locator_view() {
        let expected = expected_question("q-raw-locator", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]["source_locator_view"] = serde_json::json!({
            "visibility": "governed_opaque",
            "reference": "external_eval:D1:1",
            "reason": "source_locator_governed_opaque"
        });

        assert!(verify_rows(&[row], &[expected], "raw-capsule-locator").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_url_evidence_locator_view() {
        let expected = expected_question("q-url-evidence-locator", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]["evidence_ref_views"][0]
            ["reference"] = serde_json::json!("https://example.test/raw");

        assert!(verify_rows(&[row], &[expected], "url-evidence-locator").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_absolute_path_evidence_locator_view() {
        let expected = expected_question("q-path-evidence-locator", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]["evidence_ref_views"][0]
            ["reference"] = serde_json::json!("/private/raw/evidence");

        assert!(verify_rows(&[row], &[expected], "path-evidence-locator").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_forged_evidence_visibility() {
        let expected = expected_question("q-forged-evidence-visibility", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]["evidence_ref_views"][0]
            ["visibility"] = serde_json::json!("public_citation");

        assert!(verify_rows(&[row], &[expected], "forged-evidence-visibility").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_missing_locator_reference_field() {
        let expected = expected_question("q-missing-locator-reference", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]["evidence_ref_views"][0]
            .as_object_mut()
            .expect("typed evidence locator view")
            .remove("reference");

        assert!(verify_rows(&[row], &[expected], "missing-locator-reference").is_err());
    }

    #[test]
    fn detail_recomputation_requires_visible_refs_to_match_safe_views_exactly() {
        let expected = expected_question("q-visible-ref-mismatch", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]["visible_evidence_refs"] =
            serde_json::json!([]);

        assert!(verify_rows(&[row], &[expected], "visible-ref-mismatch").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_cross_subject_shared_fact_eligibility() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]["redaction_state"] =
            serde_json::json!("shared_with_subject");

        let aggregate = verify_rows(&[row], &[expected], "forged-shared-fact-eligibility")
            .expect("privacy violation must remain measurable for the release gate");

        assert_eq!(aggregate.p7_production_delivery.privacy_leak_count, 1);
    }

    #[test]
    fn detail_recomputation_rejects_privacy_pass_with_validator_failures() {
        let expected = expected_question("q-privacy-validator-failure", 0);
        let mut row = detail_row(&expected);
        row["privacy_report"]["passed"] = serde_json::json!(true);
        row["privacy_report"]["private_raw_candidate_count"] = serde_json::json!(0);
        row["privacy_report"]["failures"] = serde_json::json!(["redaction_failed"]);

        assert!(verify_rows(&[row], &[expected], "privacy-validator-failure").is_err());
    }

    #[test]
    fn detail_recomputation_requires_typed_shared_fact_eligibility() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]
            .as_object_mut()
            .expect("rendered capsule")
            .remove("shared_fact_surface_allowed");

        assert!(verify_rows(&[row], &[expected], "missing-shared-fact-eligibility").is_err());
    }

    #[test]
    fn detail_recomputation_attributes_projection_only_loss_to_final_rendered() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["rendered_sources"] =
            serde_json::json!([bm_core::memory::canonical_recall_evidence_group(
                "external_eval:D1:1"
            )]);
        row["sdk_stage_candidates"]["final_rendered"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [
                bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1")
            ]
        }]);
        row["final_projection_delivery_report"]["rendered_capsules"]
            .as_array_mut()
            .expect("final rendered capsules")
            .pop();
        remove_projection_manifest_entry(&mut row, 1);
        remove_projection_observation_entry(&mut row, 1);
        row["all_evidence_hit"] = serde_json::json!(false);

        let aggregate = verify_rows(&[row], &[expected], "projection-only-loss")
            .expect("projection-only loss should be independently attributed");

        assert_eq!(
            aggregate
                .p7_loss_ledger
                .eval_selected_hit_rendered_miss_evidence,
            0
        );
        assert_eq!(
            aggregate
                .p7_loss_ledger
                .selected_hit_final_rendered_miss_questions,
            1
        );
        assert_eq!(
            aggregate
                .p7_loss_ledger
                .selected_hit_final_rendered_miss_evidence,
            1
        );
    }

    #[test]
    fn detail_recomputation_separates_eval_to_projection_selection_loss() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        row["projection_selected_sources"] = serde_json::json!([first_group.clone()]);
        row["rendered_sources"] = serde_json::json!([first_group.clone()]);
        let one_projection_candidate = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group]
        }]);
        row["sdk_stage_candidates"]["projection_selected"] = one_projection_candidate.clone();
        row["sdk_stage_candidates"]["final_rendered"] = one_projection_candidate;
        row["final_projection_delivery_report"]["selected_candidate_ids"] =
            serde_json::json!(["candidate-1"]);
        row["final_projection_delivery_report"]["selection_decisions"][1]["selected"] =
            serde_json::json!(false);
        row["final_projection_delivery_report"]["selection_decisions"][1]["drop_reason"] =
            serde_json::json!("profile_budget_exhausted");
        row["final_projection_delivery_report"]["rendered_capsules"]
            .as_array_mut()
            .expect("final rendered capsules")
            .pop();
        remove_projection_manifest_entry(&mut row, 1);
        remove_projection_observation_entry(&mut row, 1);
        row["all_evidence_hit"] = serde_json::json!(false);

        let aggregate = verify_rows(&[row], &[expected], "projection-selection-loss")
            .expect("projection selection loss should be independently attributed");

        assert_eq!(
            aggregate
                .p7_loss_ledger
                .eval_selected_hit_projection_selected_miss_evidence,
            1
        );
        assert_eq!(
            aggregate
                .p7_loss_ledger
                .selected_hit_final_rendered_miss_evidence,
            0
        );
    }

    #[test]
    fn detail_recomputation_rejects_duplicate_and_missing_identity() {
        let first = expected_question("q-1", 0);
        let second = expected_question("q-2", 1);
        let row = detail_row(&first);

        assert!(verify_rows(
            &[row.clone(), row.clone()],
            &[first.clone(), second.clone()],
            "duplicate"
        )
        .is_err());
        assert!(verify_rows(&[row], &[first, second], "missing").is_err());
    }

    #[test]
    fn detail_recomputation_counts_canonical_gold_groups_not_raw_duplicates() {
        let mut expected = expected_question("q-1", 0);
        expected.gold_sources = vec!["D1:1".to_string(), "D1:1".to_string(), "D2:1".to_string()];
        let row = detail_row(&expected);

        let aggregate = verify_rows(&[row], &[expected], "duplicate-gold-locator")
            .expect("duplicate raw locators must collapse to canonical gold groups");

        assert_eq!(
            aggregate.w4_1_diagnostics.evidence_count_buckets.get("2_3"),
            Some(&1)
        );
        assert_eq!(
            aggregate
                .w4_1_diagnostics
                .question_type_counts
                .get("multi_gold"),
            Some(&1)
        );
    }

    #[test]
    fn detail_recomputation_rejects_forged_multi_gold_refs() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["ablation_report"]["slices"][0]["off_run_selected_evidence_refs"] =
            serde_json::json!(["external_eval:D1:1", "external_eval:D2:1"]);

        assert!(verify_rows(&[row], &[expected], "multi-gold-forgery").is_err());
    }

    #[test]
    fn ablation_accepts_governed_off_run_candidate_in_report_wide_evidence_index() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let mut full_groups = vec![first_group.clone(), second_group.clone()];
        full_groups.sort();
        let slice = &mut row["ablation_report"]["slices"][0];
        let off_run_selected = serde_json::json!([{
            "candidate_id": "candidate-off-run",
            "canonical_evidence_groups": full_groups.clone()
        }]);
        let off_run_rendered = serde_json::json!([{
            "candidate_id": "candidate-off-run",
            "canonical_evidence_groups": [first_group.clone()]
        }]);
        slice["off_run_selected_candidate_ids"] = serde_json::json!(["candidate-off-run"]);
        slice["off_run_rendered_candidate_ids"] = serde_json::json!(["candidate-off-run"]);
        slice["off_run_selected_candidates"] = off_run_selected;
        slice["off_run_rendered_candidates"] = off_run_rendered;
        slice["off_run_selected_evidence_refs"] =
            serde_json::json!([first_group.clone(), second_group.clone()]);
        slice["off_run_rendered_evidence_refs"] = serde_json::json!([first_group.clone()]);
        slice["delivery_affected_candidate_ids"] =
            serde_json::json!(["candidate-1", "candidate-2", "candidate-off-run"]);
        slice["delivery_affected_candidate_count"] = serde_json::json!(3);
        slice["sdk_delivery_affected_candidate_count_claim"] = serde_json::json!(3);
        row["evidence_ref_index"]
            .as_array_mut()
            .expect("evidence index")
            .push(serde_json::json!({
                "candidate_id": "candidate-off-run",
                "canonical_evidence_groups": full_groups
            }));

        verify_rows(&[row], &[expected], "off-run-report-wide-index")
            .expect("report-wide evidence index must include governed off-run candidates");
    }

    #[test]
    fn ablation_rejects_off_run_candidate_missing_from_report_wide_evidence_index() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let slice = &mut row["ablation_report"]["slices"][0];
        let off_run_candidate = serde_json::json!([{
            "candidate_id": "candidate-off-run",
            "canonical_evidence_groups": [first_group]
        }]);
        slice["off_run_selected_candidate_ids"] = serde_json::json!(["candidate-off-run"]);
        slice["off_run_rendered_candidate_ids"] = serde_json::json!(["candidate-off-run"]);
        slice["off_run_selected_candidates"] = off_run_candidate.clone();
        slice["off_run_rendered_candidates"] = off_run_candidate;
        slice["delivery_affected_candidate_ids"] =
            serde_json::json!(["candidate-1", "candidate-2", "candidate-off-run"]);
        slice["delivery_affected_candidate_count"] = serde_json::json!(3);
        slice["sdk_delivery_affected_candidate_count_claim"] = serde_json::json!(3);

        assert!(verify_rows(&[row], &[expected], "off-run-index-missing").is_err());
    }

    #[test]
    fn authoritative_evidence_index_rejects_reordered_or_empty_candidate_ids() {
        let expected = expected_question("q-1", 0);
        let mut reordered = detail_row(&expected);
        reordered["evidence_ref_index"]
            .as_array_mut()
            .expect("evidence index")
            .reverse();
        assert!(verify_rows(
            &[reordered],
            std::slice::from_ref(&expected),
            "reordered-evidence-index"
        )
        .is_err());

        let mut empty_id = detail_row(&expected);
        empty_id["evidence_ref_index"][0]["candidate_id"] = serde_json::json!("");
        assert!(verify_rows(&[empty_id], &[expected], "empty-evidence-index-id").is_err());
    }

    #[test]
    fn ablation_rejects_empty_selected_evidence_binding() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let slice = &mut row["ablation_report"]["slices"][0];
        slice["off_run_selected_candidates"][0]["canonical_evidence_groups"] =
            serde_json::json!([]);
        slice["off_run_selected_evidence_refs"] = serde_json::json!([]);

        assert!(verify_rows(&[row], &[expected], "empty-selected-evidence").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_one_ablation_candidate_claiming_two_golds() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let forged_candidate = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()]
        }]);
        row["evidence_ref_index"][0]["canonical_evidence_groups"] =
            serde_json::json!([first_group.clone(), second_group.clone()]);
        let slice = &mut row["ablation_report"]["slices"][0];
        slice["off_run_selected_candidates"] = forged_candidate.clone();
        slice["off_run_rendered_candidates"] = forged_candidate;
        slice["off_run_selected_evidence_refs"] =
            serde_json::json!([first_group.clone(), second_group.clone()]);
        slice["off_run_rendered_evidence_refs"] = serde_json::json!([first_group, second_group]);
        slice["selected_evidence_hit_delta"] = serde_json::json!(0);
        slice["rendered_evidence_hit_delta"] = serde_json::json!(0);
        slice["selected_all_hit_lost"] = serde_json::json!(false);
        slice["rendered_all_hit_lost"] = serde_json::json!(false);
        slice["delivery_contribution_proven"] = serde_json::json!(false);
        slice["delivery_affected_candidate_ids"] =
            serde_json::json!(["candidate-1", "candidate-2"]);
        slice["delivery_affected_candidate_count"] = serde_json::json!(2);

        assert!(
            accumulate_p7_ablation(&mut P7DetailAggregate::default(), &row, &expected,).is_err()
        );
    }

    #[test]
    fn loss_recomputation_rejects_one_selected_candidate_claiming_two_golds() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        row["sdk_stage_candidates"]["eval_selected"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group, second_group]
        }]);

        assert!(accumulate_p7_loss(&mut P7DetailAggregate::default(), &row, &expected).is_err());
    }

    #[test]
    fn loss_recomputation_rejects_forged_candidate_rank_and_drop_reason() {
        let group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let expanded = vec![P7CandidateEvidence {
            candidate_id: "candidate-1".to_string(),
            canonical_evidence_groups: vec![group.clone()],
        }];
        let expected_groups = BTreeSet::from([group.clone()]);
        let delivery = serde_json::json!({
            "selection_decisions": [{
                "candidate_id": "candidate-1",
                "selected": false,
                "drop_reason": "ProfileBudgetExhausted"
            }],
            "render_decisions": []
        });
        let valid_entry = serde_json::json!({
            "canonical_evidence_group": group,
            "expanded_matches": [{"candidate_id": "candidate-1", "rank": 1}],
            "reranked_matches": [{"candidate_id": "candidate-1", "rank": 1}],
            "selected_matches": [],
            "rendered_matches": [],
            "selection_losses": [{
                "candidate_id": "candidate-1",
                "drop_reason": "ProfileBudgetExhausted"
            }],
            "render_losses": []
        });
        assert!(validate_p7_loss_entries(
            std::slice::from_ref(&valid_entry),
            &expected_groups,
            &expanded,
            &expanded,
            &[],
            &[],
            &delivery,
        )
        .is_ok());

        let mut forged_rank = valid_entry.clone();
        forged_rank["expanded_matches"][0]["rank"] = serde_json::json!(2);
        assert!(validate_p7_loss_entries(
            &[forged_rank],
            &expected_groups,
            &expanded,
            &expanded,
            &[],
            &[],
            &delivery,
        )
        .is_err());

        let mut forged_reason = valid_entry;
        forged_reason["selection_losses"][0]["drop_reason"] =
            serde_json::json!("DuplicateEvidenceGroup");
        assert!(validate_p7_loss_entries(
            &[forged_reason],
            &expected_groups,
            &expanded,
            &expanded,
            &[],
            &[],
            &delivery,
        )
        .is_err());
    }

    #[test]
    fn detail_recomputation_rejects_one_candidate_claiming_two_gold_groups() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        row["final_projection_delivery_report"]["selected_candidate_ids"] =
            serde_json::json!(["candidate-1"]);
        row["final_projection_delivery_report"]["selection_decisions"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()],
            "evidence_family_groups": [],
            "selected": true,
            "drop_reason": null
        }]);
        row["final_projection_delivery_report"]["rendered_capsules"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "evidence_ref_views": [],
            "visible_evidence_refs": [],
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()],
            "source_locator_view": {"visibility": "governed_opaque", "reference": format!("opaque:governed-source:scope:{}:evidence_source_ref", "c".repeat(64)), "reason": "source_locator_governed_opaque"},
            "redaction_state": "public_runtime",
            "shared_fact_surface_allowed": true,
            "rendered_chars": 20
        }]);
        row["projection_selected_sources"] =
            serde_json::json!([first_group.clone(), second_group.clone()]);
        row["rendered_sources"] = serde_json::json!([first_group, second_group]);
        remove_projection_manifest_entry(&mut row, 1);

        assert!(verify_rows(&[row], &[expected], "one-candidate-two-golds").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_forged_stage_arrays_when_raw_sdk_candidates_disagree() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["sdk_stage_candidates"]["eval_selected"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [
                bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1")
            ]
        }]);

        assert!(verify_rows(&[row], &[expected], "forged-stage-array").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_multi_gold_stage_claim_from_one_candidate() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let one_candidate = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()]
        }]);
        row["evidence_ref_index"] = one_candidate.clone();
        for stage in ["expanded", "reranked", "eval_selected", "eval_rendered"] {
            row["sdk_stage_candidates"][stage] = one_candidate.clone();
        }
        for field in ["expanded_sources", "reranked_sources", "selected_sources"] {
            row[field] = serde_json::json!([first_group.clone(), second_group.clone()]);
        }
        row["eval_delivery_report"]["selected_candidate_ids"] = serde_json::json!(["candidate-1"]);
        row["eval_delivery_report"]["selection_decisions"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()],
            "evidence_family_groups": [],
            "selected": true,
            "drop_reason": null
        }]);
        row["eval_delivery_report"]["rendered_capsules"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "evidence_ref_views": [],
            "visible_evidence_refs": [],
            "canonical_evidence_groups": [first_group, second_group],
            "source_locator_view": {"visibility": "governed_opaque", "reference": format!("opaque:governed-source:scope:{}:evidence_source_ref", "d".repeat(64)), "reason": "source_locator_governed_opaque"},
            "redaction_state": "public_runtime",
            "shared_fact_surface_allowed": true,
            "rendered_chars": 20
        }]);
        row["stage_diagnostics"]["selected_candidate_ids"] = serde_json::json!(["candidate-1"]);
        row["stage_diagnostics"]["rendered_candidate_ids"] = serde_json::json!(["candidate-1"]);

        assert!(verify_rows(&[row], &[expected], "one-candidate-stage-all-hit").is_err());
    }

    #[test]
    fn detail_recomputation_requires_exact_disabled_ablation_slice_set() {
        let expected = expected_question("q-1", 0);

        let mut duplicate = detail_row(&expected);
        duplicate["ablation_report"]["slices"][6]["name"] = serde_json::json!("facet_off");
        assert!(verify_rows(
            &[duplicate],
            std::slice::from_ref(&expected),
            "duplicate-ablation-slice"
        )
        .is_err());

        let mut enabled = detail_row(&expected);
        enabled["ablation_report"]["slices"][0]["feature_enabled"] = serde_json::json!(true);
        assert!(verify_rows(
            &[enabled],
            std::slice::from_ref(&expected),
            "enabled-ablation-slice"
        )
        .is_err());

        let mut missing_raw_chars = detail_row(&expected);
        missing_raw_chars["ablation_report"]["slices"][0]
            .as_object_mut()
            .expect("ablation slice")
            .remove("off_run_rendered_chars");
        assert!(verify_rows(
            &[missing_raw_chars],
            &[expected],
            "missing-ablation-raw-chars"
        )
        .is_err());
    }

    #[test]
    fn canonical_exact_group_does_not_parse_composite_external_locator() {
        let gold = vec!["D1:1".to_string(), "D2:1".to_string()];
        let composite = vec!["external_eval:D1:1|D2:1".to_string()];

        assert!(!p7_any_gold_hit(&gold, &composite));
        assert!(!p7_all_gold_hit(&gold, &composite));
        assert_eq!(
            p7_gold_group_hit_count(&p7_canonical_groups(&gold), &composite),
            0
        );
    }

    #[test]
    fn final_delivery_join_keeps_opaque_canonical_groups_without_recovering_locators() {
        let opaque_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let report = serde_json::json!({
            "rendered_capsules": [{
                "candidate_id": "candidate-1",
                "canonical_evidence_groups": [opaque_group.clone()]
            }]
        });

        assert_eq!(
            p7_rendered_sources_from_delivery(&report).expect("opaque final delivery"),
            vec![opaque_group]
        );
    }

    #[test]
    fn deterministic_gold_matching_consumes_each_candidate_and_gold_at_most_once() {
        let gold = vec!["D1:1".to_string(), "D2:1".to_string()];
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let multi_group_candidate = P7CandidateEvidence {
            candidate_id: "candidate-b".to_string(),
            canonical_evidence_groups: vec![first_group.clone(), second_group.clone()],
        };

        let one_candidate =
            p7_match_gold_groups(&gold, std::slice::from_ref(&multi_group_candidate));
        assert_eq!(one_candidate.len(), 1);

        let candidates = vec![
            multi_group_candidate,
            P7CandidateEvidence {
                candidate_id: "candidate-a".to_string(),
                canonical_evidence_groups: vec![first_group],
            },
        ];
        let mut reversed = candidates.clone();
        reversed.reverse();
        let expected = p7_match_gold_groups(&gold, &candidates);
        assert_eq!(expected.len(), 2);
        assert_eq!(p7_match_gold_groups(&gold, &reversed), expected);
    }

    #[test]
    fn detail_recomputation_rejects_forged_rendered_ablation_and_raw_numeric_facts() {
        let expected = expected_question("q-1", 0);
        let mut forged_rendered = detail_row(&expected);
        forged_rendered["ablation_report"]["slices"][0]["rendered_evidence_hit_delta"] =
            serde_json::json!(0);
        assert!(verify_rows(
            &[forged_rendered],
            std::slice::from_ref(&expected),
            "rendered-forgery"
        )
        .is_err());

        let mut forged_count = detail_row(&expected);
        forged_count["ablation_report"]["slices"][0]["off_run_rendered_candidate_count"] =
            serde_json::json!(2);
        assert!(verify_rows(
            &[forged_count],
            std::slice::from_ref(&expected),
            "count-forgery"
        )
        .is_err());

        let mut forged_growth = detail_row(&expected);
        forged_growth["ablation_report"]["slices"][0]["off_run_render_growth"] =
            serde_json::json!(1);
        assert!(verify_rows(&[forged_growth], &[expected], "growth-forgery").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_tampered_sdk_ablation_candidate_identity() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["ablation_report"]["slices"][0]["off_run_selected_candidate_ids"] =
            serde_json::json!(["candidate-2"]);

        assert!(verify_rows(
            &[row],
            std::slice::from_ref(&expected),
            "candidate-id-forgery"
        )
        .is_err());

        let mut count_claim = detail_row(&expected);
        count_claim["ablation_report"]["slices"][0]
            ["sdk_delivery_affected_candidate_count_claim"] = serde_json::json!(2);
        assert!(verify_rows(&[count_claim], &[expected], "candidate-count-forgery").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_tampered_raw_index_report() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row)
            .expect("untampered raw reports");
        row["graph_index_report"]["source_candidate_count"] = serde_json::json!(999);

        assert!(accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row).is_err());
    }

    #[test]
    fn detail_recomputation_rejects_tampered_graph_v2_integrity_facts() {
        let expected = expected_question("q-1", 0);
        let mut selected_chain = detail_row(&expected);
        selected_chain["graph_index_report"]["used"] = serde_json::json!(true);
        selected_chain["index_diagnostics"]["index_used_questions"] = serde_json::json!(1);
        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &selected_chain)
            .expect("untampered graph v2 diagnostics");
        selected_chain["graph_index_report"]["selected_dependency_chain_verified"] =
            serde_json::json!(true);
        assert!(accumulate_p7_index_diagnostics(
            &mut P7DetailAggregate::default(),
            &selected_chain
        )
        .is_err());

        let mut read_mutation = detail_row(&expected);
        read_mutation["graph_index_report"]["read_path_mutation_delta"] = serde_json::json!(1);
        assert!(
            accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &read_mutation)
                .is_err()
        );
    }

    #[test]
    fn unused_graph_metadata_is_not_counted_as_used_graph_proof() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["graph_index_report"]["manifest_contract_verified"] = serde_json::json!(true);
        row["graph_index_report"]["selected_dependency_chain_verified"] = serde_json::json!(true);
        row["graph_index_report"]["full_scope_closure_verified"] = serde_json::json!(true);
        row["graph_index_report"]["manifest_generation_present"] = serde_json::json!(true);
        row["graph_index_report"]["graph_revision_present"] = serde_json::json!(true);
        row["graph_index_report"]["scope_digest_present"] = serde_json::json!(true);

        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row)
            .expect("unused graph metadata is informational, not a used-graph proof");
    }

    #[test]
    fn unused_facet_metadata_is_not_counted_as_used_facet_proof() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["facet_index_report"]["used"] = serde_json::json!(false);
        row["facet_index_report"]["posting_doc_read_count"] = serde_json::json!(0);
        row["facet_index_report"]["owner_doc_read_count"] = serde_json::json!(0);
        row["index_diagnostics"]["facet_index_used_questions"] = serde_json::json!(0);
        row["index_diagnostics"]["facet_posting_doc_read_count"] = serde_json::json!(0);
        row["index_diagnostics"]["facet_owner_doc_read_count"] = serde_json::json!(0);
        row["index_diagnostics"]["facet_manifest_integrity_verified_questions"] =
            serde_json::json!(0);

        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row)
            .expect("unused facet metadata is informational, not a used-facet proof");
    }

    #[test]
    fn facet_zero_hit_requires_lookup_and_exact_manifest_read_proof() {
        let expected = expected_question("q-1", 0);
        let mut clean_zero_hit = detail_row(&expected);
        clean_zero_hit["facet_index_report"]["manifest_matched_posting_count"] =
            serde_json::json!(0);
        clean_zero_hit["facet_index_report"]["posting_doc_read_count"] = serde_json::json!(0);
        clean_zero_hit["facet_index_report"]["owner_key_lookup_count"] = serde_json::json!(0);
        clean_zero_hit["facet_index_report"]["owner_doc_read_count"] = serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_manifest_matched_posting_count"] =
            serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_posting_doc_read_count"] = serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_owner_key_lookup_count"] = serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_owner_doc_read_count"] = serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_clean_zero_hit_questions"] =
            serde_json::json!(1);
        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &clean_zero_hit)
            .expect("manifest-verified zero hit remains a used bounded lookup");

        let mut missing_manifest_posting = clean_zero_hit.clone();
        missing_manifest_posting["facet_index_report"]["manifest_matched_posting_count"] =
            serde_json::json!(1);
        assert!(accumulate_p7_index_diagnostics(
            &mut P7DetailAggregate::default(),
            &missing_manifest_posting,
        )
        .is_err());

        let mut no_lookup = clean_zero_hit;
        no_lookup["facet_index_report"]["posting_key_lookup_count"] = serde_json::json!(0);
        assert!(
            accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &no_lookup).is_err()
        );
    }

    #[test]
    fn graph_v2_raw_index_report_requires_every_safe_integrity_fact() {
        let expected = expected_question("q-1", 0);
        for field in [
            "manifest_contract_verified",
            "selected_dependency_chain_verified",
            "full_scope_closure_verified",
            "manifest_generation_present",
            "graph_revision_present",
            "scope_digest_present",
            "maintenance_required",
            "incident_present",
            "read_path_mutation_delta",
        ] {
            let mut row = detail_row(&expected);
            row["graph_index_report"]
                .as_object_mut()
                .expect("graph report")
                .remove(field);
            assert!(
                accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row).is_err(),
                "missing {field} must fail closed"
            );
        }
    }

    #[test]
    fn graph_v2_raw_index_report_rejects_sensitive_values() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["graph_index_report"]["scope_digest"] = serde_json::json!("not-safe-to-expose");

        assert!(accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row).is_err());
    }

    #[test]
    fn graph_v2_index_diagnostics_are_additive() {
        let source = W4ExternalNoisyIndexDiagnostics {
            graph_manifest_contract_verified_questions: 1,
            graph_selected_dependency_chain_verified_questions: 2,
            graph_full_scope_closure_verified_questions: 3,
            graph_manifest_generation_present_questions: 4,
            graph_revision_present_questions: 5,
            graph_scope_digest_present_questions: 6,
            graph_maintenance_required_questions: 7,
            graph_incident_questions: 8,
            graph_read_path_mutation_delta: 9,
            ..W4ExternalNoisyIndexDiagnostics::default()
        };
        let mut aggregate = source.clone();

        add_index_diagnostics(&mut aggregate, &source);

        assert_eq!(aggregate.graph_manifest_contract_verified_questions, 2);
        assert_eq!(
            aggregate.graph_selected_dependency_chain_verified_questions,
            4
        );
        assert_eq!(aggregate.graph_full_scope_closure_verified_questions, 6);
        assert_eq!(aggregate.graph_manifest_generation_present_questions, 8);
        assert_eq!(aggregate.graph_revision_present_questions, 10);
        assert_eq!(aggregate.graph_scope_digest_present_questions, 12);
        assert_eq!(aggregate.graph_maintenance_required_questions, 14);
        assert_eq!(aggregate.graph_incident_questions, 16);
        assert_eq!(aggregate.graph_read_path_mutation_delta, 18);
    }

    #[test]
    fn graph_v2_index_diagnostics_reject_legacy_shape() {
        assert!(
            serde_json::from_value::<W4ExternalNoisyIndexDiagnostics>(serde_json::json!({
                "questions_with_index_report": 1,
                "index_used_questions": 1
            }))
            .is_err()
        );
    }

    #[test]
    fn summary_recomputation_rejects_tampered_graph_v2_aggregate() {
        let expected = expected_question("q-1", 0);
        let aggregate = verify_rows(&[detail_row(&expected)], &[expected], "graph-v2-summary")
            .expect("exact graph v2 detail");
        let mut claimed = serde_json::json!({
            "samples": aggregate.samples,
            "questions": aggregate.questions,
            "evidence_questions": aggregate.evidence_questions,
            "any_evidence_hit": aggregate.any_evidence_hit,
            "all_evidence_hit": aggregate.all_evidence_hit,
            "write_errors": aggregate.write_errors,
            "recall_errors": aggregate.recall_errors,
            "stage_hit_counts": aggregate.stage_hit_counts,
            "index_diagnostics": aggregate.index_diagnostics,
            "w4_1_diagnostics": aggregate.w4_1_diagnostics,
            "facet_ablation": aggregate.facet_ablation,
            "p7_loss_ledger": aggregate.p7_loss_ledger,
            "p7_production_delivery": aggregate.p7_production_delivery,
        });
        validate_p7_detail_metrics(&claimed, &aggregate).expect("exact graph v2 aggregate");
        claimed["index_diagnostics"]["graph_selected_dependency_chain_verified_questions"] =
            serde_json::json!(1);

        assert!(validate_p7_detail_metrics(&claimed, &aggregate).is_err());
    }

    #[test]
    fn graph_v2_release_conditions_require_selected_chain_but_not_full_scope() {
        let diagnostics_value = serde_json::json!({
            "questions_with_index_report": 2,
            "index_used_questions": 2,
            "fallback_full_scan_questions": 0,
            "source_candidate_count": 2,
            "matched_source_anchor_count": 2,
            "unmatched_source_anchor_count": 0,
            "indexed_neighbor_count": 2,
            "filtered_node_count": 0,
            "filtered_edge_count": 0,
            "filtered_backlink_count": 0,
            "failure_count": 0,
            "graph_manifest_contract_verified_questions": 2,
            "graph_selected_dependency_chain_verified_questions": 2,
            "graph_full_scope_closure_verified_questions": 0,
            "graph_manifest_generation_present_questions": 2,
            "graph_revision_present_questions": 2,
            "graph_scope_digest_present_questions": 2,
            "graph_maintenance_required_questions": 0,
            "graph_incident_questions": 0,
            "graph_read_path_mutation_delta": 0,
            "facet_questions_with_index_report": 2,
            "facet_index_used_questions": 2,
            "facet_report_only_questions": 0,
            "facet_fallback_full_scan_questions": 0,
            "facet_source_candidate_count": 2,
            "facet_matched_source_candidate_count": 2,
            "facet_posting_key_lookup_count": 2,
            "facet_manifest_matched_posting_count": 2,
            "facet_posting_doc_read_count": 2,
            "facet_owner_key_lookup_count": 2,
            "facet_owner_doc_read_count": 2,
            "facet_zero_posting_key_lookup_questions": 0,
            "facet_clean_zero_hit_questions": 0,
            "facet_manifest_integrity_verified_questions": 2,
            "facet_manifest_integrity_failure_count": 0,
            "facet_exact_match_count": 2,
            "facet_expanded_match_count": 0,
            "facet_failure_count": 0
        });
        let diagnostics =
            serde_json::from_value::<W4ExternalNoisyIndexDiagnostics>(diagnostics_value.clone())
                .expect("graph v2 diagnostics");
        let mut summary = W4ExternalNoisyBenchmarkSummary {
            questions: 2,
            index_diagnostics: Some(diagnostics.clone()),
            ..W4ExternalNoisyBenchmarkSummary::default()
        };

        assert!(index_diagnostics_show_index_effect(&diagnostics));
        assert!(w4_external_index_diagnostics_no_full_scan(&summary));

        for field in [
            "graph_manifest_contract_verified_questions",
            "graph_selected_dependency_chain_verified_questions",
            "graph_manifest_generation_present_questions",
            "graph_revision_present_questions",
            "graph_scope_digest_present_questions",
        ] {
            let mut invalid_value = diagnostics_value.clone();
            invalid_value[field] = serde_json::json!(1);
            let invalid: W4ExternalNoisyIndexDiagnostics =
                serde_json::from_value(invalid_value).expect("invalid graph diagnostics");
            summary.index_diagnostics = Some(invalid.clone());
            assert!(!index_diagnostics_show_index_effect(&invalid));
            assert!(!w4_external_index_diagnostics_no_full_scan(&summary));
        }

        for field in [
            "graph_maintenance_required_questions",
            "graph_incident_questions",
            "graph_read_path_mutation_delta",
        ] {
            let mut invalid_value = diagnostics_value.clone();
            invalid_value[field] = serde_json::json!(1);
            let invalid: W4ExternalNoisyIndexDiagnostics =
                serde_json::from_value(invalid_value).expect("unsafe graph diagnostics");
            summary.index_diagnostics = Some(invalid.clone());
            assert!(!index_diagnostics_show_index_effect(&invalid));
            assert!(!w4_external_index_diagnostics_no_full_scan(&summary));
        }

        let mut oracle_diagnostics = diagnostics;
        oracle_diagnostics.facet_index_used_questions = 1;
        oracle_diagnostics.facet_manifest_integrity_verified_questions = 1;
        let oracle = W4ExternalNoisyBenchmarkSummary {
            suite: "longmemeval_oracle".to_string(),
            questions: 2,
            index_diagnostics: Some(oracle_diagnostics),
            ..W4ExternalNoisyBenchmarkSummary::default()
        };
        assert!(w4_external_index_diagnostics_no_full_scan(&oracle));
    }

    #[test]
    fn detail_recomputation_rejects_tampered_w41_claim() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        accumulate_p7_w41_diagnostics(&mut P7DetailAggregate::default(), &row, &expected)
            .expect("untampered W4.1 detail");
        row["stage_diagnostics"]["first_any_hit_stage"] = serde_json::json!("source");

        assert!(
            accumulate_p7_w41_diagnostics(&mut P7DetailAggregate::default(), &row, &expected)
                .is_err()
        );
    }

    #[test]
    fn detail_recomputation_rejects_mixed_run_id() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["run_id"] = serde_json::json!("other-run");

        assert!(validate_p7_detail_identity(
            &row,
            "test_suite",
            "test-run",
            &expected,
            &mut BTreeSet::new(),
            &mut BTreeSet::new()
        )
        .is_err());
    }

    #[test]
    fn summary_recomputation_rejects_tampered_w41_aggregate() {
        let expected = expected_question("q-1", 0);
        let mut aggregate = P7DetailAggregate {
            samples: 1,
            questions: 1,
            evidence_questions: 1,
            ..P7DetailAggregate::default()
        };
        accumulate_p7_w41_diagnostics(&mut aggregate, &detail_row(&expected), &expected)
            .expect("exact W4.1 detail");
        let mut claimed = serde_json::json!({
            "samples": aggregate.samples,
            "questions": aggregate.questions,
            "evidence_questions": aggregate.evidence_questions,
            "any_evidence_hit": aggregate.any_evidence_hit,
            "all_evidence_hit": aggregate.all_evidence_hit,
            "write_errors": aggregate.write_errors,
            "recall_errors": aggregate.recall_errors,
            "stage_hit_counts": aggregate.stage_hit_counts,
            "index_diagnostics": aggregate.index_diagnostics,
            "w4_1_diagnostics": aggregate.w4_1_diagnostics,
            "facet_ablation": aggregate.facet_ablation,
            "p7_loss_ledger": aggregate.p7_loss_ledger,
            "p7_production_delivery": aggregate.p7_production_delivery,
        });
        validate_p7_detail_metrics(&claimed, &aggregate).expect("exact W4.1 aggregate");
        claimed["w4_1_diagnostics"]["gold_rank_sum"] = serde_json::json!(999);

        assert!(validate_p7_detail_metrics(&claimed, &aggregate).is_err());
    }

    #[test]
    fn sdk_build_fingerprint_uses_length_prefixed_contract_and_file_count() {
        let root =
            std::env::temp_dir().join(format!("bm-p7-sdk-fingerprint-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fingerprint root");
        let first = root.join("a");
        let second = root.join("bc");
        fs::write(&first, b"bc").expect("first input");
        fs::write(&second, b"a").expect("second input");

        let fingerprint = p7_fingerprint_files_with_contract(
            &root,
            &[first, second],
            P7_SDK_BUILD_FINGERPRINT_CONTRACT,
        )
        .expect("length-prefixed SDK fingerprint");

        assert_eq!(
            fingerprint,
            "032c6e5efb6729d27492bcca15f7938b6b50a265b6a527d44e6534911c5cbdbd"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn p7_build_fingerprint_input_order_remains_path_component_order() {
        let root =
            std::env::temp_dir().join(format!("bm-p7-component-order-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("nested")).expect("nested source root");
        fs::write(root.join("nested.rs"), b"module").expect("module source");
        fs::write(root.join("nested/a.rs"), b"nested").expect("nested source");

        let inputs = p7_fingerprint_inputs(&root, &["nested.rs", "nested"])
            .expect("P7 component-ordered inputs");

        assert_eq!(
            inputs
                .iter()
                .map(|path| path.strip_prefix(&root).expect("relative input"))
                .collect::<Vec<_>>(),
            vec![Path::new("nested/a.rs"), Path::new("nested.rs"),]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compiled_sdk_fingerprint_matches_independent_disk_recomputation() {
        let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("SDK workspace root");
        let inputs =
            p7_fingerprint_inputs(sdk_root, &P7_SDK_BUILD_INPUTS).expect("SDK build inputs");
        let disk = p7_fingerprint_files_with_contract(
            sdk_root,
            &inputs,
            P7_SDK_BUILD_FINGERPRINT_CONTRACT,
        )
        .expect("SDK disk fingerprint");

        assert_eq!(disk, P7_TRUSTED_SDK_BUILD_FINGERPRINT);
    }

    #[test]
    fn release_source_exclusions_never_hide_domain_source_directories() {
        let root = Path::new("/workspace");
        for relative in [
            "crates/core/src/memory/mod.rs",
            "crates/core/src/skills/mod.rs",
            "crates/replay/src/results/mod.rs",
        ] {
            assert!(
                !p7_release_gate_source_path_is_excluded(root, &root.join(relative), &[], &[])
                    .expect("domain source exclusion decision"),
                "domain source must remain release-governed: {relative}"
            );
        }
        assert!(p7_release_gate_source_path_is_excluded(
            root,
            &root.join("target/release/operator"),
            &[],
            &[],
        )
        .expect("build output exclusion decision"));
        for relative in [
            "memory/store.db",
            "results/run.json",
            "crates/sdk/memory/store.db",
        ] {
            assert!(
                p7_release_gate_source_path_is_excluded(
                    root,
                    &root.join(relative),
                    &[],
                    &P7_AGENT_MEMORY_RELEASE_GATE_EXCLUDED_DIRECTORIES,
                )
                .expect("owner output exclusion decision"),
                "owner output must remain outside release source: {relative}"
            );
        }
        assert!(!p7_release_gate_source_path_is_excluded(
            root,
            &root.join("skills/beetle-memory-development/SKILL.md"),
            &[],
            &P7_AGENT_MEMORY_RELEASE_GATE_EXCLUDED_DIRECTORIES,
        )
        .expect("tracked skill source exclusion decision"));
    }

    #[test]
    fn compiled_operator_fingerprint_matches_independent_disk_recomputation() {
        let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("SDK workspace root");
        let inputs = p7_operator_build_inputs(sdk_root).expect("operator build inputs");
        let disk = p7_fingerprint_files_with_contract(
            sdk_root,
            &inputs,
            P7_OPERATOR_BUILD_FINGERPRINT_CONTRACT,
        )
        .expect("operator disk fingerprint");

        assert_eq!(disk, P7_EMBEDDED_OPERATOR_BUILD_FINGERPRINT);
    }

    #[test]
    fn verifier_identity_requires_release_profile_and_bound_manifest_fields() {
        let identity = p7_current_verifier_identity();

        assert!(p7_verifier_identity_is_valid(&identity));
        assert_eq!(identity.build_profile, "release");
        assert!(is_sha256(&identity.operator_executable_sha256));
        assert!(is_sha256(&identity.release_manifest_sha256));
        assert_eq!(
            identity.source_anchor_sha256,
            identity.operator_build_fingerprint
        );
    }

    fn p7_gate_source_fixture(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let canonical_temp = fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
        let root = canonical_temp.join(format!("bm-p7-gate-source-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let sdk_root = root.join("agent-memory");
        let runner_root = root.join("runner");
        fs::create_dir_all(sdk_root.join("crates/core/src")).expect("core source owner");
        fs::create_dir_all(sdk_root.join("crates/sdk/src")).expect("SDK runtime source owner");
        fs::create_dir_all(sdk_root.join("crates/replay/src")).expect("SDK source owner");
        fs::create_dir_all(sdk_root.join("crates/replay/src/bin/bm-w4-external-noisy-wall"))
            .expect("operator-private frozen owner");
        fs::create_dir_all(sdk_root.join("apps/desktop/src-tauri"))
            .expect("desktop workspace owner");
        fs::create_dir_all(sdk_root.join("dev-docs")).expect("development truth owner");
        fs::create_dir_all(sdk_root.join("scripts")).expect("SDK scripts owner");
        fs::create_dir_all(runner_root.join("src")).expect("runner source owner");
        fs::create_dir_all(runner_root.join("tests")).expect("runner tests owner");
        for (path, bytes) in [
            (sdk_root.join("Cargo.toml"), b"sdk-manifest\n".as_slice()),
            (sdk_root.join("Cargo.lock"), b"sdk-lock\n".as_slice()),
            (
                sdk_root.join("crates/core/Cargo.toml"),
                b"core-manifest\n".as_slice(),
            ),
            (
                sdk_root.join("crates/core/src/lib.rs"),
                b"pub fn core() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/sdk/Cargo.toml"),
                b"runtime-manifest\n".as_slice(),
            ),
            (
                sdk_root.join("crates/sdk/src/lib.rs"),
                b"pub fn sdk() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/Cargo.toml"),
                b"replay-manifest\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/build.rs"),
                b"fn main() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/lib.rs"),
                b"pub fn replay() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/bench.rs"),
                b"pub fn benchmark() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/fixture.rs"),
                b"pub fn fixture() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/harness.rs"),
                b"pub fn harness() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/p7_process.rs"),
                b"pub fn process() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/p7_secure_fs.rs"),
                b"pub fn secure_fs() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/retained_artifact_fs.rs"),
                b"pub fn retained_artifact_fs() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/runner.rs"),
                b"pub fn runner() {}\n".as_slice(),
            ),
            (
                sdk_root.join("crates/replay/src/sealed_execution.rs"),
                b"pub fn sealed_execution() {}\n".as_slice(),
            ),
            (
                sdk_root.join(P7_FROZEN_RUNNER_IDENTITY_RELATIVE_PATH),
                b"frozen-v1\n".as_slice(),
            ),
            (
                sdk_root.join("scripts/check_w4_external_noisy_wall_operator.sh"),
                b"operator-gate-v1\n".as_slice(),
            ),
            (
                sdk_root.join("scripts/check_w4_external_noisy_wall_preflight.sh"),
                b"preflight-gate-v1\n".as_slice(),
            ),
            (
                sdk_root.join("scripts/check_memory_write_transaction_contract.sh"),
                b"transaction-gate-v1\n".as_slice(),
            ),
            (
                sdk_root.join("scripts/check_next_gen_memory_plan.sh"),
                b"next-gen-gate-v1\n".as_slice(),
            ),
            (
                runner_root.join("Cargo.toml"),
                b"runner-manifest\n".as_slice(),
            ),
            (runner_root.join("Cargo.lock"), b"runner-lock\n".as_slice()),
            (runner_root.join("build.rs"), b"fn main() {}\n".as_slice()),
            (
                runner_root.join("src/main.rs"),
                b"fn main() {}\n".as_slice(),
            ),
            (
                runner_root.join("run_full_p7_wall.sh"),
                b"wall-v1\n".as_slice(),
            ),
            (
                runner_root.join("run_p7_max_rss.sh"),
                b"rss-v1\n".as_slice(),
            ),
            (
                runner_root.join("tests/run_full_p7_wall_fake_runner_test.sh"),
                b"wall-test-v1\n".as_slice(),
            ),
            (
                runner_root.join("tests/run_p7_max_rss_fake_runner_test.sh"),
                b"rss-test-v1\n".as_slice(),
            ),
        ] {
            fs::write(path, bytes).expect("gate source fixture");
        }
        for relative in [
            "README.md",
            "governed-memory-facet-index-plan.md",
            "inhabited-subject-projection-refactor-plan.md",
            "long-term-memory-control-surface-plan.md",
            "memory-write-transaction-plan.md",
            "multi-subject-memory-space-plan.md",
            "next-gen-soul-memory-roadmap.md",
            "replay-sandbox-plan.md",
            "runtime-budget-refactor-plan.md",
            "sdk-host-integration-readiness-plan.md",
            "soul-and-subject-memory-boundary.md",
            "temporal-memory-graph-plan.md",
        ] {
            fs::write(sdk_root.join("dev-docs").join(relative), b"truth-v1\n")
                .expect("producer truth fixture");
        }
        (root, sdk_root, runner_root)
    }

    #[test]
    fn broad_gate_manifest_and_producer_semantic_manifest_have_independent_inputs() {
        let (root, sdk_root, runner_root) = p7_gate_source_fixture("exclusions");
        let broad = || {
            p7_release_gate_source_manifest(&sdk_root, &runner_root)
                .expect("broad gate source manifest")
                .source_fingerprint
        };
        let semantic = || {
            p7_producer_semantic_source_manifest(&sdk_root, &runner_root)
                .expect("producer semantic source manifest")
                .source_fingerprint
        };
        let baseline_broad = broad();
        let baseline_semantic = semantic();

        fs::write(
            sdk_root.join(P7_FROZEN_RUNNER_IDENTITY_RELATIVE_PATH),
            b"frozen-v2\n",
        )
        .expect("change frozen release anchor");
        for excluded in [
            sdk_root.join("results/report"),
            runner_root.join("target/noise"),
            runner_root.join("releases/frozen-runner"),
            runner_root.join(".git/index"),
        ] {
            fs::create_dir_all(excluded.parent().expect("excluded parent"))
                .expect("excluded directory");
            fs::write(excluded, b"excluded\n").expect("excluded fixture");
        }
        assert_eq!(
            broad(),
            baseline_broad,
            "frozen/output/VCS artifacts are not release gate inputs"
        );
        assert_eq!(semantic(), baseline_semantic);

        fs::write(
            sdk_root.join("crates/core/src/lib.rs"),
            b"pub fn core_v2() {}\n",
        )
        .expect("change explicit core source");
        assert_ne!(
            broad(),
            baseline_broad,
            "every explicit production source owner must be gate source"
        );
        assert_ne!(
            semantic(),
            baseline_semantic,
            "core source must bind producer semantics"
        );
        fs::write(
            sdk_root.join("crates/core/src/lib.rs"),
            b"pub fn core() {}\n",
        )
        .expect("restore core source");
        fs::create_dir_all(sdk_root.join("dev-docs")).expect("unrelated docs owner");
        fs::write(
            sdk_root.join("dev-docs/governed-memory-facet-index-plan.md"),
            b"p7-truth-v2\n",
        )
        .expect("write producer truth source");
        assert_ne!(
            broad(),
            baseline_broad,
            "broad release identity must bind truth consumed by its gates"
        );
        assert_eq!(
            semantic(),
            baseline_semantic,
            "truth-source review must not expand producer executable semantics"
        );
        fs::write(
            sdk_root.join("dev-docs/governed-memory-facet-index-plan.md"),
            b"truth-v1\n",
        )
        .expect("restore producer truth source");

        fs::write(
            sdk_root.join("crates/replay/src/bench.rs"),
            b"pub fn benchmark_v2() {}\n",
        )
        .expect("change neighboring replay source");
        let replay_source_broad = broad();
        let replay_source_semantic = semantic();
        assert_ne!(
            replay_source_broad, baseline_broad,
            "producer source identity must bind the bm-replay code linked into the runner"
        );
        assert_ne!(replay_source_semantic, baseline_semantic);

        fs::create_dir_all(sdk_root.join("crates/replay/src/docs"))
            .expect("included nested docs owner");
        fs::write(
            sdk_root.join("crates/replay/src/docs/gate-note"),
            b"included\n",
        )
        .expect("included nested docs source");
        let nested_docs_broad = broad();
        let nested_docs_semantic = semantic();
        assert_ne!(nested_docs_broad, replay_source_broad);
        assert_eq!(
            nested_docs_semantic, replay_source_semantic,
            "arbitrary neighboring files must not enter the minimal producer manifest"
        );

        fs::write(
            sdk_root.join("scripts/check_w4_external_noisy_wall_operator.sh"),
            b"operator-gate-v2\n",
        )
        .expect("change SDK gate source");
        let operator_broad = broad();
        let operator_semantic = semantic();
        assert_ne!(operator_broad, nested_docs_broad);
        assert_eq!(
            operator_semantic, nested_docs_semantic,
            "producer semantics must not bind verifier orchestration"
        );
        fs::write(
            sdk_root.join("scripts/check_next_gen_memory_plan.sh"),
            b"next-gen-gate-v2\n",
        )
        .expect("change producer contract gate source");
        let contract_broad = broad();
        let contract_semantic = semantic();
        assert_ne!(contract_broad, operator_broad);
        assert_eq!(contract_semantic, operator_semantic);
        fs::write(
            runner_root.join("tests/run_full_p7_wall_fake_runner_test.sh"),
            b"wall-test-v2\n",
        )
        .expect("change runner test source");
        assert_ne!(
            broad(),
            contract_broad,
            "broad gate manifest must bind the test it executes"
        );
        assert_eq!(semantic(), contract_semantic);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn p7_source_identities_bind_generation_neutral_execution_owners() {
        let (root, sdk_root, runner_root) = p7_gate_source_fixture("neutral-owners");
        let fingerprints = || {
            (
                p7_release_gate_source_manifest(&sdk_root, &runner_root)
                    .expect("broad gate source manifest")
                    .source_fingerprint,
                p7_producer_semantic_source_manifest(&sdk_root, &runner_root)
                    .expect("producer semantic source manifest")
                    .source_fingerprint,
            )
        };
        let baseline = fingerprints();

        for (relative, replacement) in [
            (
                "crates/replay/src/retained_artifact_fs.rs",
                b"pub fn retained_artifact_fs_v2() {}\n".as_slice(),
            ),
            (
                "crates/replay/src/sealed_execution.rs",
                b"pub fn sealed_execution_v2() {}\n".as_slice(),
            ),
        ] {
            let path = sdk_root.join(relative);
            let original = fs::read(&path).expect("read neutral owner fixture");
            fs::write(&path, replacement).expect("mutate neutral owner fixture");
            let mutated = fingerprints();
            assert_ne!(
                mutated.0, baseline.0,
                "{relative} must change the broad release source identity"
            );
            assert_ne!(
                mutated.1, baseline.1,
                "{relative} must change the producer semantic source identity"
            );
            fs::write(path, original).expect("restore neutral owner fixture");
            assert_eq!(fingerprints(), baseline);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replay_library_does_not_compile_the_frozen_runner_anchor() {
        let replay_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let library = fs::read_to_string(replay_root.join("src/lib.rs"))
            .expect("read bm-replay library owner");

        assert!(
            !library.contains("mod p7_frozen_runner_identity"),
            "the shared bm-replay library must be stateless so the measured runner cannot embed its own frozen SHA"
        );
    }

    #[test]
    fn legacy_merged_provenance_without_admission_is_rejected() {
        let legacy = serde_json::json!({
            "run_id": "run-1",
            "contract_version": P7_CONTRACT_VERSION,
            "sdk_report_schema_version": MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            "sdk_build_fingerprint": "1".repeat(64),
            "runner_build_fingerprint": "2".repeat(64),
            "runner_lock_fingerprint": "3".repeat(64),
            "executable_sha256": "4".repeat(64),
            "gate_attestation_sha256": "5".repeat(64),
            "gate_source_fingerprint": "6".repeat(64),
            "gate_ids": P7_REQUIRED_RELEASE_GATE_IDS,
            "build_profile": "release",
            "input_sha256": "7".repeat(64),
            "merged_detail_sha256": "8".repeat(64),
            "ordered_shard_digest_manifest": []
        });

        assert!(
            serde_json::from_value::<P7MergedProvenance>(legacy).is_err(),
            "old merged provenance without an immutable cohort admission must fail closed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn release_gate_source_fingerprint_rejects_symlinks_that_escape_the_owner() {
        use std::os::unix::fs::symlink;

        let (root, sdk_root, runner_root) = p7_gate_source_fixture("symlink");
        let outside = root.join("outside.sh");
        fs::write(&outside, b"outside\n").expect("outside source");
        symlink(&outside, sdk_root.join("crates/core/src/symlink.rs")).expect("source symlink");

        assert!(p7_release_gate_source_fingerprint(&sdk_root, &runner_root).is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn release_gate_source_manifest_binds_an_internal_symlink_and_its_target() {
        use std::os::unix::fs::symlink;

        let (root, sdk_root, runner_root) = p7_gate_source_fixture("internal-symlink");
        let target = sdk_root.join("crates/core/src/tool_real.rs");
        let link = sdk_root.join("crates/core/src/tool_link.rs");
        fs::write(&target, b"tool-v1\n").expect("internal symlink target");
        symlink("tool_real.rs", &link).expect("internal source symlink");

        let (manifest, receipt) =
            p7_release_gate_source_manifest_with_receipt(&sdk_root, &runner_root)
                .expect("internal symlink manifest");
        let regular_entries = manifest
            .entries
            .iter()
            .filter(|entry| entry.entry_kind == P7ReleaseSourceManifestEntryKind::RegularFile)
            .collect::<Vec<_>>();
        assert_eq!(receipt.unique_artifact_count, regular_entries.len() as u64);
        assert_eq!(receipt.full_read_pass_count, receipt.unique_artifact_count);
        assert_eq!(
            receipt.artifact_bytes_read,
            regular_entries
                .iter()
                .map(|entry| entry.byte_len)
                .sum::<u64>()
        );
        assert_eq!(receipt.admitted_artifact_bytes, receipt.artifact_bytes_read);
        assert_eq!(receipt.duplicate_artifact_count, 0);
        assert!(receipt.passed);
        assert!(manifest.entries.iter().any(|entry| {
            entry.owner == "agent-memory"
                && entry.relative_path == "crates/core/src/tool_link.rs"
                && entry.entry_kind == P7ReleaseSourceManifestEntryKind::SymbolicLink
        }));
        let baseline = manifest.source_fingerprint;

        fs::write(&target, b"tool-v2\n").expect("change internal symlink target");
        assert_ne!(
            p7_release_gate_source_fingerprint(&sdk_root, &runner_root)
                .expect("changed internal symlink target fingerprint"),
            baseline,
            "the manifest must bind the content consumed through an internal symlink"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn release_gate_source_manifest_rejects_a_symlink_into_an_excluded_directory() {
        use std::os::unix::fs::symlink;

        let (root, sdk_root, runner_root) = p7_gate_source_fixture("excluded-symlink-target");
        fs::create_dir_all(sdk_root.join("target")).expect("excluded target directory");
        fs::write(sdk_root.join("target/tool.sh"), b"unattested\n")
            .expect("excluded symlink target");
        symlink(
            "../../../target/tool.sh",
            sdk_root.join("crates/core/src/tool_link.rs"),
        )
        .expect("symlink into excluded source");

        assert!(
            p7_release_gate_source_manifest(&sdk_root, &runner_root).is_err(),
            "an included gate input must not reach content omitted from the source manifest"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn maximum_rss_evidence_rejects_legacy_schema_without_measurement_contract() {
        let legacy = serde_json::json!({
            "schema_version": "p7_maximum_rss_evidence_v1",
            "completed": true,
            "rss_gate_passed": true,
            "run_id": "rss-run",
            "suite": P7_MAXIMUM_RSS_SUITE,
            "dataset_file": "longmemeval_m_cleaned.json",
            "dataset_sha256": "1".repeat(64),
            "input_bytes": 1,
            "dataset_index": 0,
            "question_index": 0,
            "question_id": "q0",
            "question_sha256": "2".repeat(64),
            "maximum_rss_bytes": 1,
            "rss_limit_bytes": P7_MAXIMUM_RSS_LIMIT_BYTES,
            "preflight_report_sha256": "3".repeat(64),
            "runner_stdout_sha256": "4".repeat(64),
            "legacy_counter_sha256": "5".repeat(64),
            "detail_sha256": "6".repeat(64),
            "summary_sha256": "7".repeat(64),
            "preflight_validated_after_measurement": true,
            "preflight": {
                "run_id": "rss-run",
                "sdk_build_fingerprint": "8".repeat(64),
                "runner_build_fingerprint": "b".repeat(64),
                "runner_lock_fingerprint": "c".repeat(64),
                "executable_sha256": "d".repeat(64),
                "executable_canonical_path": "/tmp/runner",
                "build_profile": "release"
            }
        });

        assert!(serde_json::from_value::<P7MaximumRssEvidence>(legacy).is_err());
    }

    #[test]
    fn maximum_rss_measurement_contract_rejects_failed_child_and_receipt_drift() {
        let benchmark_root = Path::new("/tmp/p7-rss-contract");
        let preflight = P7RunnerPreflightReport {
            schema_version: P7_RUNNER_PREFLIGHT_SCHEMA_VERSION.to_string(),
            run_id: "rss-run".to_string(),
            sdk_build_fingerprint: "a".repeat(64),
            runner_build_fingerprint: "d".repeat(64),
            runner_lock_fingerprint: "e".repeat(64),
            executable_sha256: "f".repeat(64),
            executable_canonical_path: "/tmp/runner".to_string(),
            gate_attestation_sha256: "1".repeat(64),
            release_metadata_sha256: "2".repeat(64),
            gate_source_fingerprint: "3".repeat(64),
            gate_source_manifest_sha256: "4".repeat(64),
            gate_ids: P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
            build_profile: "release".to_string(),
        };
        let mut measurement = P7MaximumRssMeasurementReport {
            schema_version: P7_MAXIMUM_RSS_MEASUREMENT_SCHEMA_VERSION.to_string(),
            run_id: "rss-run".to_string(),
            child_exit_status: 0,
            child_executable_canonical_path: preflight.executable_canonical_path.clone(),
            child_executable_sha256: preflight.executable_sha256.clone(),
            child_args: p7_expected_maximum_rss_child_args(benchmark_root, "rss-run"),
            maximum_rss_bytes: 1024,
            supervisor_receipt: crate::p7_process::P7ProcessReceipt {
                schema_version: "p7_sealed_process_receipt_v1".to_string(),
                sealed_executable_sha256: Some(preflight.executable_sha256.clone()),
                pid: 42,
                process_group: 42,
                maximum_rss_bytes: 1024,
                elapsed_millis: 1_000,
            },
            runner_stdout: P7MeasuredArtifactIdentity {
                device: 1,
                inode: 2,
                byte_len: 3,
                sha256: "4".repeat(64),
            },
            runner_stderr: P7MeasuredArtifactIdentity {
                device: 1,
                inode: 3,
                byte_len: 4,
                sha256: "5".repeat(64),
            },
        };

        validate_p7_maximum_rss_measurement_contract(
            &measurement,
            benchmark_root,
            "rss-run",
            &preflight,
        )
        .expect("exact successful measurement contract");

        measurement.child_exit_status = 9;
        assert!(validate_p7_maximum_rss_measurement_contract(
            &measurement,
            benchmark_root,
            "rss-run",
            &preflight,
        )
        .is_err());
        measurement.child_exit_status = 0;
        measurement.supervisor_receipt.sealed_executable_sha256 = Some("2".repeat(64));
        assert!(validate_p7_maximum_rss_measurement_contract(
            &measurement,
            benchmark_root,
            "rss-run",
            &preflight,
        )
        .is_err());
        measurement.supervisor_receipt.sealed_executable_sha256 =
            Some(preflight.executable_sha256.clone());
        measurement.child_executable_sha256 = "3".repeat(64);
        assert!(validate_p7_maximum_rss_measurement_contract(
            &measurement,
            benchmark_root,
            "rss-run",
            &preflight,
        )
        .is_err());
        measurement.child_executable_sha256 = preflight.executable_sha256.clone();
        measurement.child_args.pop();
        assert!(validate_p7_maximum_rss_measurement_contract(
            &measurement,
            benchmark_root,
            "rss-run",
            &preflight,
        )
        .is_err());
    }

    #[test]
    fn artifact_snapshot_rejects_same_bytes_file_identity_replacement() {
        let temp_root = fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
        let root = temp_root.join(format!("bm-p7-artifact-snapshot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("artifact root");
        let path = root.join("artifact.json");
        fs::write(&path, b"stable\n").expect("initial artifact");
        let snapshot = P7RegularArtifactSnapshot::capture(&path, &root, &root)
            .expect("initial regular snapshot");
        let replacement = root.join("replacement.json");
        fs::write(&replacement, b"stable\n").expect("same-byte replacement");
        fs::rename(&replacement, &path).expect("replace artifact inode");

        assert!(snapshot.verify_unchanged().is_err());

        let digest_snapshot =
            P7RegularArtifactSnapshot::capture(&path, &root, &root).expect("replacement snapshot");
        fs::write(&path, b"changed\n").expect("change artifact bytes");
        assert!(digest_snapshot.verify_unchanged().is_err());

        let outside = temp_root.join(format!("bm-p7-artifact-outside-{}", std::process::id()));
        fs::write(&outside, b"outside\n").expect("outside artifact");
        assert!(P7RegularArtifactSnapshot::capture(&outside, &temp_root, &root).is_err());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(outside);
    }

    #[cfg(unix)]
    #[test]
    fn regular_file_contract_rejects_a_file_beneath_a_symlinked_parent() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("bm-p7-parent-symlink-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let real = root.join("real");
        let alias = root.join("alias");
        fs::create_dir_all(&real).expect("real parent");
        fs::write(real.join("artifact.json"), b"{}\n").expect("artifact");
        symlink(&real, &alias).expect("parent symlink");

        assert!(p7_require_regular_file(&alias.join("artifact.json")).is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn maximum_rss_summary_binds_trusted_dataset_and_frozen_preflight() {
        let root = Path::new("/tmp/p7-rss-contract");
        let input_path = root.join("data/longmemeval_m_cleaned.json");
        let detail_path = root.join(
            "results/runs/rss-run/longmemeval_m_cleaned.shard-0-of-1.limit-1.question-index-0.jsonl",
        );
        let summary_path = root.join(
            "results/runs/rss-run/longmemeval_m_cleaned.shard-0-of-1.limit-1.question-index-0.summary.json",
        );
        let dataset = p7_trusted_dataset(P7_MAXIMUM_RSS_SUITE).expect("trusted m_cleaned");
        let preflight = P7RunnerPreflightReport {
            schema_version: P7_RUNNER_PREFLIGHT_SCHEMA_VERSION.to_string(),
            run_id: "rss-run".to_string(),
            sdk_build_fingerprint: "a".repeat(64),
            runner_build_fingerprint: "d".repeat(64),
            runner_lock_fingerprint: "e".repeat(64),
            executable_sha256: "f".repeat(64),
            executable_canonical_path: "/tmp/runner".to_string(),
            gate_attestation_sha256: "1".repeat(64),
            release_metadata_sha256: "2".repeat(64),
            gate_source_fingerprint: "3".repeat(64),
            gate_source_manifest_sha256: "4".repeat(64),
            gate_ids: P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
            build_profile: "release".to_string(),
        };
        let detail_sha256 = "1".repeat(64);
        let mut summary = serde_json::json!({
            "run_id": "rss-run",
            "suite": P7_MAXIMUM_RSS_SUITE,
            "shard_index": 0,
            "shard_total": 1,
            "samples": 1,
            "questions": 1,
            "write_errors": 0,
            "recall_errors": 0,
            "limit": 1,
            "question_limit": null,
            "question_index": 0,
            "completed": true,
            "elapsed_secs": 1.0,
            "input_file": input_path.to_string_lossy(),
            "input_bytes": 123,
            "input_sha256": dataset.input_sha256,
            "detail_file": detail_path.to_string_lossy(),
            "summary_file": summary_path.to_string_lossy(),
            "producer": {
                "schema_version": P7_SHARD_PRODUCER_PROVENANCE_SCHEMA_VERSION,
                "execution_kind": "maximum_rss_diagnostic",
                "run_id": "rss-run",
                "contract_version": P7_CONTRACT_VERSION,
                "sdk_report_schema_version": MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
                "sdk_build_fingerprint": preflight.sdk_build_fingerprint,
                "runner_build_fingerprint": preflight.runner_build_fingerprint,
                "runner_lock_fingerprint": preflight.runner_lock_fingerprint,
                "executable_sha256": preflight.executable_sha256,
                "gate_attestation_sha256": preflight.gate_attestation_sha256,
                "release_metadata_sha256": preflight.release_metadata_sha256,
                "gate_source_fingerprint": preflight.gate_source_fingerprint,
                "gate_source_manifest_sha256": preflight.gate_source_manifest_sha256,
                "gate_ids": preflight.gate_ids,
                "cohort_admission_sha256": "",
                "build_profile": "release",
                "input_sha256": dataset.input_sha256,
                "detail_schema_version": P7_DETAIL_SCHEMA_VERSION,
                "detail_sha256": detail_sha256,
            }
        });
        let producer =
            serde_json::from_value::<P7ShardProducerProvenance>(summary["producer"].take())
                .expect("typed RSS producer fixture");
        summary["producer"] = serde_json::to_value(
            P7RecordedProducerIdentity::record(&producer).expect("record RSS producer fixture"),
        )
        .expect("serialize recorded RSS producer fixture");

        validate_p7_maximum_rss_summary_contract(
            &summary,
            "rss-run",
            dataset,
            &input_path,
            123,
            &detail_path,
            &summary_path,
            &detail_sha256,
            &preflight,
        )
        .expect("fully bound RSS summary");

        summary["input_sha256"] = serde_json::json!("9".repeat(64));
        assert!(validate_p7_maximum_rss_summary_contract(
            &summary,
            "rss-run",
            dataset,
            &input_path,
            123,
            &detail_path,
            &summary_path,
            &detail_sha256,
            &preflight,
        )
        .is_err());
        summary["input_sha256"] = serde_json::json!(dataset.input_sha256);
        summary["producer"]["executable_sha256"] = serde_json::json!("8".repeat(64));
        assert!(validate_p7_maximum_rss_summary_contract(
            &summary,
            "rss-run",
            dataset,
            &input_path,
            123,
            &detail_path,
            &summary_path,
            &detail_sha256,
            &preflight,
        )
        .is_err());
    }

    #[test]
    fn wall_release_report_requires_preflight_and_maximum_rss_evidence() {
        let report = evaluate_w4_external_noisy_wall(&[]);

        assert!(!report.release_gate_passed);
        assert!(report
            .blocked_reasons
            .contains(&"p7_runner_preflight_missing".to_string()));
        assert!(report
            .blocked_reasons
            .contains(&"p7_maximum_rss_evidence_missing".to_string()));
    }

    #[test]
    fn release_finalizer_cannot_promote_a_failed_benchmark_gate() {
        let run_id = "rss-run";
        let preflight = P7RunnerPreflightReport {
            schema_version: P7_RUNNER_PREFLIGHT_SCHEMA_VERSION.to_string(),
            run_id: run_id.to_string(),
            sdk_build_fingerprint: "a".repeat(64),
            runner_build_fingerprint: "d".repeat(64),
            runner_lock_fingerprint: "e".repeat(64),
            executable_sha256: "f".repeat(64),
            executable_canonical_path: "/tmp/runner".to_string(),
            gate_attestation_sha256: "1".repeat(64),
            release_metadata_sha256: "2".repeat(64),
            gate_source_fingerprint: "3".repeat(64),
            gate_source_manifest_sha256: "4".repeat(64),
            gate_ids: P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
            build_profile: "release".to_string(),
        };
        let dataset = p7_trusted_dataset(P7_MAXIMUM_RSS_SUITE).expect("trusted m_cleaned");
        let maximum_rss = P7MaximumRssEvidence {
            schema_version: P7_MAXIMUM_RSS_EVIDENCE_SCHEMA_VERSION.to_string(),
            completed: true,
            rss_gate_passed: true,
            run_id: run_id.to_string(),
            suite: P7_MAXIMUM_RSS_SUITE.to_string(),
            dataset_file: dataset.file_name.to_string(),
            dataset_sha256: dataset.input_sha256.to_string(),
            input_bytes: 1,
            dataset_index: P7_MAXIMUM_RSS_DATASET_INDEX,
            question_index: P7_MAXIMUM_RSS_QUESTION_INDEX,
            question_id: "q0".to_string(),
            question_sha256: "1".repeat(64),
            maximum_rss_bytes: 1,
            rss_limit_bytes: P7_MAXIMUM_RSS_LIMIT_BYTES,
            measurement_report_sha256: "7".repeat(64),
            measurement_child_exit_status: 0,
            measurement_elapsed_millis: 1_000,
            supervisor_receipt: crate::p7_process::P7ProcessReceipt {
                schema_version: "p7_sealed_process_receipt_v1".to_string(),
                sealed_executable_sha256: Some(preflight.executable_sha256.clone()),
                pid: 42,
                process_group: 42,
                maximum_rss_bytes: 1,
                elapsed_millis: 1_000,
            },
            measured_executable_canonical_path: preflight.executable_canonical_path.clone(),
            measured_executable_sha256: preflight.executable_sha256.clone(),
            preflight_report_sha256: "2".repeat(64),
            runner_stdout_sha256: "3".repeat(64),
            runner_stderr_sha256: "4".repeat(64),
            detail_sha256: "5".repeat(64),
            summary_sha256: "6".repeat(64),
            preflight_validated_after_measurement: true,
            preflight: preflight.clone(),
        };
        let report = W4ExternalNoisyWallReport {
            benchmark_gate_passed: false,
            run_id: Some(run_id.to_string()),
            blocked_reasons: vec![
                "p7_runner_preflight_missing".to_string(),
                "p7_maximum_rss_evidence_missing".to_string(),
            ],
            ..W4ExternalNoisyWallReport::default()
        };

        let finalized = finalize_w4_external_noisy_release_report(
            report,
            preflight.clone(),
            maximum_rss.clone(),
        )
        .expect("evidence binds to the cohort");

        assert!(!finalized.release_gate_passed);
        assert!(finalized
            .blocked_reasons
            .contains(&"p7_benchmark_gate_failed".to_string()));

        let mut over_limit = P7MaximumRssEvidence {
            rss_gate_passed: false,
            maximum_rss_bytes: P7_MAXIMUM_RSS_LIMIT_BYTES + 1,
            ..maximum_rss
        };
        over_limit.supervisor_receipt.maximum_rss_bytes = over_limit.maximum_rss_bytes;
        let benchmark_passed = W4ExternalNoisyWallReport {
            benchmark_gate_passed: true,
            run_id: Some(run_id.to_string()),
            blocked_reasons: vec![
                "p7_runner_preflight_missing".to_string(),
                "p7_maximum_rss_evidence_missing".to_string(),
            ],
            ..W4ExternalNoisyWallReport::default()
        };
        let over_limit_report =
            finalize_w4_external_noisy_release_report(benchmark_passed, preflight, over_limit)
                .expect("over-limit evidence remains a valid diagnostic report");
        assert!(!over_limit_report.release_gate_passed);
        assert!(over_limit_report.p7_maximum_rss_attached);
        assert!(!over_limit_report.p7_maximum_rss_within_limit);
        assert!(over_limit_report
            .blocked_reasons
            .contains(&"p7_maximum_rss_limit_exceeded".to_string()));
    }

    #[test]
    fn merged_summary_path_is_bound_to_results_runs_cohort() {
        let root = std::env::temp_dir().join(format!("bm-p7-run-path-{}", std::process::id()));
        let valid = root
            .join("results/runs/run-a")
            .join("locomo.merged.summary.json");
        assert_eq!(
            p7_benchmark_root_for_run(&valid, "run-a").expect("valid run path"),
            root
        );
        assert!(p7_benchmark_root_for_run(&valid, "run-b").is_err());
        assert!(p7_benchmark_root_for_run(
            &root.join("results/locomo.merged.summary.json"),
            "run-a"
        )
        .is_err());
        assert!(p7_benchmark_root_for_run(&valid, "../run-a").is_err());
    }

    #[test]
    fn delivery_integrity_failures_and_drop_reasons_are_kept_separate() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["integrity_failures"] =
            serde_json::json!(["projection_capsule_mismatch"]);
        row["final_projection_delivery_report"]["delivery_drop_reasons"] =
            serde_json::json!(["render_budget_exhausted"]);

        let aggregate = verify_rows(&[row], &[expected], "delivery-reasons")
            .expect("typed delivery reasons should verify");

        assert_eq!(
            aggregate
                .p7_production_delivery
                .blocked_reason_counts
                .get("projection_capsule_mismatch"),
            Some(&1)
        );
        assert_eq!(
            aggregate
                .p7_production_delivery
                .delivery_drop_reason_counts
                .get("render_budget_exhausted"),
            Some(&1)
        );
    }

    #[test]
    fn detail_recomputation_rejects_forged_final_projection_integrity() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["final_projection_integrity"]["raw_private_violation_count"] = serde_json::json!(1);

        assert!(verify_rows(&[row], &[expected], "projection-integrity-forgery").is_err());
    }

    #[test]
    fn detail_recomputation_independently_validates_sdk_projection_delivery_manifest() {
        let expected = expected_question("q-1", 0);

        let mut mismatched_prompt_set = detail_row(&expected);
        mismatched_prompt_set["sdk_projection_delivery_manifest"]["prompt_visible_entries"][1]
            ["content_sha256"] = serde_json::json!("4".repeat(64));
        assert!(verify_rows(
            &[mismatched_prompt_set],
            std::slice::from_ref(&expected),
            "projection-token-set-mismatch"
        )
        .is_err());

        let mut duplicate_capsule = detail_row(&expected);
        let first_capsule_entry =
            duplicate_capsule["sdk_projection_delivery_manifest"]["capsule_entries"][0].clone();
        duplicate_capsule["sdk_projection_delivery_manifest"]["capsule_entries"][1] =
            first_capsule_entry;
        assert!(verify_rows(
            &[duplicate_capsule],
            std::slice::from_ref(&expected),
            "projection-token-duplicate"
        )
        .is_err());

        let mut missing_owner_token = detail_row(&expected);
        missing_owner_token["sdk_projection_delivery_manifest"]["capsule_entries"][0]
            .as_object_mut()
            .expect("SDK capsule digest entry")
            .remove("owner_identity_token");
        assert!(verify_rows(
            &[missing_owner_token],
            std::slice::from_ref(&expected),
            "projection-sdk-manifest-missing-owner-token"
        )
        .is_err());

        let mut raw_owner_ref = detail_row(&expected);
        raw_owner_ref["sdk_projection_delivery_manifest"]["capsule_entries"][0]["owner_ref"] =
            serde_json::json!({"owner_plane": "long_term", "owner_id": "private-owner-id"});
        assert!(verify_rows(
            &[raw_owner_ref],
            std::slice::from_ref(&expected),
            "projection-sdk-manifest-raw-owner-ref"
        )
        .is_err());

        let mut mismatched_final_delivery = detail_row(&expected);
        mismatched_final_delivery["sdk_projection_delivery_manifest"]["capsule_entries"][1]
            ["candidate_id"] = serde_json::json!("candidate-3");
        assert!(verify_rows(
            &[mismatched_final_delivery],
            std::slice::from_ref(&expected),
            "projection-final-delivery-mismatch"
        )
        .is_err());

        let mut forged_render_receipt = detail_row(&expected);
        forged_render_receipt["sdk_projection_delivery_manifest"]["exact_render_match"] =
            serde_json::json!(false);
        assert!(verify_rows(
            &[forged_render_receipt],
            std::slice::from_ref(&expected),
            "projection-render-receipt-forgery"
        )
        .is_err());

        let mut duplicate_receipt = detail_row(&expected);
        let first_receipt =
            duplicate_receipt["sdk_projection_delivery_manifest"]["candidate_receipts"][0].clone();
        duplicate_receipt["sdk_projection_delivery_manifest"]["candidate_receipts"][1] =
            first_receipt;
        assert!(verify_rows(
            &[duplicate_receipt],
            std::slice::from_ref(&expected),
            "projection-render-receipt-duplicate"
        )
        .is_err());

        let mut forged_runner_bool = detail_row(&expected);
        forged_runner_bool["projection_delivery_proven"] = serde_json::json!(false);
        assert!(verify_rows(
            &[forged_runner_bool],
            &[expected],
            "projection-runner-bool-forgery"
        )
        .is_err());
    }

    #[test]
    fn detail_recomputation_rejects_self_consistent_sdk_manifest_without_runner_content_proof() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        for field in [
            "capsule_entries",
            "governed_block_entries",
            "prompt_visible_entries",
        ] {
            row["sdk_projection_delivery_manifest"][field][0]["content_sha256"] =
                serde_json::json!("7".repeat(64));
            row["sdk_projection_delivery_manifest"][field][1]["content_sha256"] =
                serde_json::json!("8".repeat(64));
        }
        row["sdk_projection_delivery_manifest"]["candidate_receipts"][0]["source_block_sha256"] =
            serde_json::json!("9".repeat(64));
        row["sdk_projection_delivery_manifest"]["candidate_receipts"][1]["source_block_sha256"] =
            serde_json::json!("a".repeat(64));
        row["sdk_projection_delivery_manifest"]["system_memory_block_sha256"] =
            serde_json::json!("b".repeat(64));
        row["sdk_projection_delivery_manifest"]["deterministic_envelope_sha256"] =
            serde_json::json!("c".repeat(64));

        assert!(verify_rows(
            &[row],
            std::slice::from_ref(&expected),
            "projection-self-consistent-sdk-manifest-forgery"
        )
        .is_err());

        let mut missing_observation = detail_row(&expected);
        missing_observation
            .as_object_mut()
            .expect("detail row")
            .remove("runner_projection_digest_observation");
        assert!(verify_rows(
            &[missing_observation],
            std::slice::from_ref(&expected),
            "projection-runner-observation-missing"
        )
        .is_err());

        let mut legacy_observation = detail_row(&expected);
        legacy_observation["runner_projection_digest_observation"]["schema_version"] =
            serde_json::json!("p7_runner_projection_digest_observation_v1");
        assert!(verify_rows(
            &[legacy_observation],
            std::slice::from_ref(&expected),
            "projection-runner-observation-legacy-schema"
        )
        .is_err());

        let mut missing_owner_token = detail_row(&expected);
        missing_owner_token["runner_projection_digest_observation"]["capsule_entries"][0]
            .as_object_mut()
            .expect("runner capsule digest entry")
            .remove("owner_identity_token");
        assert!(verify_rows(
            &[missing_owner_token],
            std::slice::from_ref(&expected),
            "projection-runner-observation-missing-owner-token"
        )
        .is_err());

        let mut raw_observation = detail_row(&expected);
        raw_observation["runner_projection_digest_observation"]["raw_content"] =
            serde_json::json!("private capsule content");
        assert!(verify_rows(
            &[raw_observation],
            &[expected],
            "projection-runner-observation-raw-content"
        )
        .is_err());
    }

    #[test]
    fn detail_recomputation_rejects_projection_owner_ref_mismatch() {
        let manifest = projection_delivery_manifest();
        let observation = projection_delivery_observation();

        validate_p7_runner_projection_digest_observation(&observation, &manifest)
            .expect("matching independently observed owner identity tokens must verify");

        for field in [
            "capsule_entries",
            "governed_block_entries",
            "prompt_visible_entries",
            "candidate_receipts",
        ] {
            let mut drifted = observation.clone();
            drifted[field][0]["owner_identity_token"] =
                serde_json::json!(projection_owner_identity_token('9'));
            assert!(validate_p7_runner_projection_digest_observation(&drifted, &manifest).is_err());
        }
    }

    #[test]
    fn detail_recomputation_rejects_forged_or_duplicate_surface_integrity() {
        let expected = expected_question("q-1", 0);
        let mut forged_arithmetic = detail_row(&expected);
        forged_arithmetic["final_projection_integrity"]["surface_reports"][0]
            ["protected_exact_echo_count"] = serde_json::json!(1);
        assert!(verify_rows(
            &[forged_arithmetic],
            std::slice::from_ref(&expected),
            "surface-arithmetic-forgery"
        )
        .is_err());

        let mut duplicate_surface = detail_row(&expected);
        duplicate_surface["final_projection_integrity"]["surface_reports"][1]["surface"] =
            serde_json::json!("prompt");
        assert!(verify_rows(
            &[duplicate_surface],
            &[expected],
            "duplicate-integrity-surface"
        )
        .is_err());
    }

    #[test]
    fn detail_recomputation_rejects_forged_summary_counter() {
        let expected = expected_question("q-1", 0);
        let aggregate = verify_rows(&[detail_row(&expected)], &[expected], "summary")
            .expect("exact detail should verify");
        let mut claimed = serde_json::json!({
            "samples": 1,
            "questions": 1,
            "evidence_questions": 1,
            "any_evidence_hit": 2,
            "all_evidence_hit": 1,
            "write_errors": 0,
            "recall_errors": 0,
            "stage_hit_counts": aggregate.stage_hit_counts,
            "w4_1_diagnostics": aggregate.w4_1_diagnostics,
            "facet_ablation": aggregate.facet_ablation,
            "p7_loss_ledger": aggregate.p7_loss_ledger,
            "p7_production_delivery": aggregate.p7_production_delivery,
            "index_diagnostics": aggregate.index_diagnostics,
        });

        assert!(validate_p7_detail_metrics(&claimed, &aggregate).is_err());
        claimed["any_evidence_hit"] = serde_json::json!(1);
        assert!(validate_p7_detail_metrics(&claimed, &aggregate).is_ok());
    }

    #[test]
    fn production_gate_requires_projection_proof_for_every_question() {
        let mut summary = W4ExternalNoisyBenchmarkSummary {
            questions: 2,
            p7_production_delivery: Some(W4ExternalNoisyP7ProductionDeliveryDiagnostics {
                questions_with_delivery_report: 2,
                eval_selected_matches_delivery_questions: 2,
                eval_rendered_matches_delivery_questions: 2,
                projection_selected_sources_proven_questions: 2,
                projection_delivery_proof_questions: 1,
                final_projection_integrity_questions: 2,
                final_projection_integrity_passed_questions: 2,
                schema_version_counts: [(MEMORY_RECALL_DELIVERY_SCHEMA_VERSION.to_string(), 2)]
                    .into_iter()
                    .collect(),
                ..W4ExternalNoisyP7ProductionDeliveryDiagnostics::default()
            }),
            ..W4ExternalNoisyBenchmarkSummary::default()
        };

        assert!(!p7_production_delivery_covers_summary(&summary));
        summary
            .p7_production_delivery
            .as_mut()
            .expect("delivery diagnostics")
            .projection_delivery_proof_questions = 2;
        assert!(p7_production_delivery_covers_summary(&summary));
    }

    #[test]
    fn release_identity_accepts_only_the_exact_governed_disk_identity() {
        let dataset = P7TrustedDataset {
            suite: "test_suite",
            file_name: "test.json",
            input_sha256: "1f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
        };
        let runner = P7FrozenRunnerIdentity {
            runner_build_fingerprint:
                "2f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            runner_lock_fingerprint:
                "3f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            executable_sha256: "4f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            gate_attestation_sha256:
                "6f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            release_metadata_sha256:
                "8f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            gate_source_fingerprint:
                "7f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            gate_source_manifest_sha256:
                "9f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
        };
        let producer_identity = P7ProducerIdentity {
            schema_version: P7_PRODUCER_IDENTITY_SCHEMA_VERSION.to_string(),
            contract_version: P7_CONTRACT_VERSION.to_string(),
            sdk_report_schema_version: MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            sdk_build_fingerprint: P7_TRUSTED_SDK_BUILD_FINGERPRINT.to_string(),
            runner_build_fingerprint: runner.runner_build_fingerprint.to_string(),
            runner_lock_fingerprint: runner.runner_lock_fingerprint.to_string(),
            executable_sha256: runner.executable_sha256.to_string(),
            build_profile: "release".to_string(),
            input_sha256: dataset.input_sha256.to_string(),
            detail_schema_version: P7_DETAIL_SCHEMA_VERSION.to_string(),
        };
        let mut provenance = P7MergedProvenance {
            schema_version: P7_MERGED_PROVENANCE_SCHEMA_VERSION.to_string(),
            run_id: "test-run".to_string(),
            contract_version: P7_CONTRACT_VERSION.to_string(),
            sdk_report_schema_version: MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            sdk_build_fingerprint: P7_TRUSTED_SDK_BUILD_FINGERPRINT.to_string(),
            runner_build_fingerprint: runner.runner_build_fingerprint.to_string(),
            runner_lock_fingerprint: runner.runner_lock_fingerprint.to_string(),
            executable_sha256: runner.executable_sha256.to_string(),
            gate_attestation_sha256: runner.gate_attestation_sha256.to_string(),
            release_metadata_sha256: runner.release_metadata_sha256.to_string(),
            gate_source_fingerprint: runner.gate_source_fingerprint.to_string(),
            gate_source_manifest_sha256: runner.gate_source_manifest_sha256.to_string(),
            gate_ids: P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
            cohort_admission_sha256: "a".repeat(64),
            build_profile: "release".to_string(),
            input_sha256: dataset.input_sha256.to_string(),
            detail_schema_version: P7_DETAIL_SCHEMA_VERSION.to_string(),
            producer_identity: P7RecordedProducerIdentity::record(&producer_identity)
                .expect("record producer identity"),
            merged_detail_sha256: "5".repeat(64),
            ordered_shard_digest_manifest: Vec::new(),
        };
        let disk = P7RunnerDiskIdentity {
            runner_build_fingerprint: runner.runner_build_fingerprint.to_string(),
            runner_lock_fingerprint: runner.runner_lock_fingerprint.to_string(),
            executable_sha256: runner.executable_sha256.to_string(),
            executable_canonical_path: PathBuf::from("/tmp/frozen-runner"),
            gate_attestation_sha256: runner.gate_attestation_sha256.to_string(),
            release_metadata_sha256: runner.release_metadata_sha256.to_string(),
            gate_source_fingerprint: runner.gate_source_fingerprint.to_string(),
            gate_source_manifest_sha256: runner.gate_source_manifest_sha256.to_string(),
            gate_ids: P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
        };

        assert!(validate_p7_release_identity_against_disk(&provenance, dataset, &disk).is_ok());
        let mut executable_drift = disk.clone();
        executable_drift.executable_sha256 = "6".repeat(64);
        assert!(
            validate_p7_release_identity_against_disk(&provenance, dataset, &executable_drift,)
                .is_err()
        );
        provenance.gate_attestation_sha256 = "8".repeat(64);
        assert!(validate_p7_release_identity_against_disk(&provenance, dataset, &disk).is_err());
        provenance.gate_attestation_sha256 = runner.gate_attestation_sha256.to_string();
        provenance.gate_source_fingerprint = "9".repeat(64);
        assert!(validate_p7_release_identity_against_disk(&provenance, dataset, &disk).is_err());
        provenance.gate_source_fingerprint = runner.gate_source_fingerprint.to_string();
        provenance.runner_build_fingerprint = "6".repeat(64);
        assert!(validate_p7_release_identity_against_disk(&provenance, dataset, &disk).is_err());
    }

    #[test]
    fn frozen_release_binding_rejects_attestation_source_and_gate_set_drift() {
        let frozen = P7FrozenRunnerIdentity {
            runner_build_fingerprint:
                "2f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            runner_lock_fingerprint:
                "3f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            executable_sha256: "4f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            gate_attestation_sha256:
                "6f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            release_metadata_sha256:
                "8f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            gate_source_fingerprint:
                "7f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            gate_source_manifest_sha256:
                "9f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
        };
        let disk = P7RunnerDiskIdentity {
            runner_build_fingerprint: frozen.runner_build_fingerprint.to_string(),
            runner_lock_fingerprint: frozen.runner_lock_fingerprint.to_string(),
            executable_sha256: frozen.executable_sha256.to_string(),
            executable_canonical_path: PathBuf::from("/tmp/frozen-runner"),
            gate_attestation_sha256: frozen.gate_attestation_sha256.to_string(),
            release_metadata_sha256: frozen.release_metadata_sha256.to_string(),
            gate_source_fingerprint: frozen.gate_source_fingerprint.to_string(),
            gate_source_manifest_sha256: frozen.gate_source_manifest_sha256.to_string(),
            gate_ids: P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
        };

        validate_p7_frozen_release_binding(frozen, &disk, frozen.gate_source_fingerprint)
            .expect("exact frozen release binding");
        assert!(validate_p7_frozen_release_binding(frozen, &disk, &"9".repeat(64)).is_err());
        let mut attestation_drift = disk.clone();
        attestation_drift.gate_attestation_sha256 = "a".repeat(64);
        assert!(validate_p7_frozen_release_binding(
            frozen,
            &attestation_drift,
            frozen.gate_source_fingerprint,
        )
        .is_err());
        let mut gate_drift = disk;
        gate_drift.gate_ids.swap(0, 1);
        assert!(validate_p7_frozen_release_binding(
            frozen,
            &gate_drift,
            frozen.gate_source_fingerprint,
        )
        .is_err());
    }

    #[test]
    fn p7_preflight_report_requires_run_id() {
        let report = serde_json::json!({
            "sdk_build_fingerprint": "a".repeat(64),
            "runner_build_fingerprint": "b".repeat(64),
            "runner_lock_fingerprint": "c".repeat(64),
            "executable_sha256": "d".repeat(64),
            "executable_canonical_path": "/tmp/frozen-runner",
            "build_profile": "release"
        });

        assert!(serde_json::from_value::<P7RunnerPreflightReport>(report).is_err());
    }

    #[test]
    fn p7_preflight_report_rejects_legacy_release_identity_schema() {
        let legacy = serde_json::json!({
            "run_id": "test-run",
            "sdk_build_fingerprint": "a".repeat(64),
            "runner_build_fingerprint": "d".repeat(64),
            "runner_lock_fingerprint": "e".repeat(64),
            "executable_sha256": "f".repeat(64),
            "executable_canonical_path": "/tmp/frozen-runner",
            "build_profile": "release"
        });

        assert!(serde_json::from_value::<P7RunnerPreflightReport>(legacy).is_err());
    }

    #[test]
    fn p7_provenance_rejects_legacy_release_identity_schema() {
        let legacy = serde_json::json!({
            "run_id": "test-run",
            "contract_version": P7_CONTRACT_VERSION,
            "sdk_report_schema_version": MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            "sdk_build_fingerprint": "a".repeat(64),
            "runner_build_fingerprint": "b".repeat(64),
            "runner_lock_fingerprint": "c".repeat(64),
            "executable_sha256": "d".repeat(64),
            "build_profile": "release",
            "input_sha256": "e".repeat(64),
            "merged_detail_sha256": "f".repeat(64),
            "ordered_shard_digest_manifest": []
        });

        assert!(serde_json::from_value::<P7MergedProvenance>(legacy).is_err());
    }

    #[test]
    fn p7_preflight_report_requires_canonical_runner_path() {
        let report = serde_json::json!({
            "run_id": "test-run",
            "sdk_build_fingerprint": "a".repeat(64),
            "runner_build_fingerprint": "b".repeat(64),
            "runner_lock_fingerprint": "c".repeat(64),
            "executable_sha256": "d".repeat(64),
            "build_profile": "release"
        });

        assert!(serde_json::from_value::<P7RunnerPreflightReport>(report).is_err());
    }

    #[test]
    fn p7_preflight_report_rejects_verifier_identity_fields() {
        let mut report = serde_json::json!({
            "schema_version": P7_RUNNER_PREFLIGHT_SCHEMA_VERSION,
            "run_id": "test-run",
            "sdk_build_fingerprint": "a".repeat(64),
            "runner_build_fingerprint": "b".repeat(64),
            "runner_lock_fingerprint": "c".repeat(64),
            "executable_sha256": "d".repeat(64),
            "executable_canonical_path": "/tmp/frozen-runner",
            "gate_attestation_sha256": "e".repeat(64),
            "release_metadata_sha256": "f".repeat(64),
            "gate_source_fingerprint": "1".repeat(64),
            "gate_source_manifest_sha256": "2".repeat(64),
            "gate_ids": P7_REQUIRED_RELEASE_GATE_IDS,
            "build_profile": "release"
        });
        serde_json::from_value::<P7RunnerPreflightReport>(report.clone())
            .expect("producer-only preflight shape");
        report["operator_build_fingerprint"] = serde_json::json!("e".repeat(64));
        report["orchestration_fingerprint"] = serde_json::json!("f".repeat(64));
        assert!(serde_json::from_value::<P7RunnerPreflightReport>(report).is_err());
    }

    struct P7TestReleaseBundle {
        root: PathBuf,
        executable_sha256: String,
        attestation_path: PathBuf,
        metadata_path: PathBuf,
        #[cfg(unix)]
        #[cfg(target_os = "linux")]
        source_manifest_path: PathBuf,
        attestation: P7ReleaseGateAttestation,
        metadata: P7ReleaseMetadata,
        source_manifest: P7ReleaseSourceManifest,
    }

    fn p7_test_json_bytes<T: Serialize>(value: &T) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(value).expect("serialize release fixture");
        bytes.push(b'\n');
        bytes
    }

    fn p7_test_gate_receipts(
        sdk_root: &Path,
        runner_root: &Path,
        source_fingerprint: &str,
        tools: &[P7ReleaseToolIdentity],
        environment_sha256: &str,
    ) -> Vec<P7ReleaseGateReceipt> {
        p7_required_release_gate_specs(sdk_root, runner_root)
            .expect("fixture production gate specs")
            .into_iter()
            .map(|spec| {
                let logical_name = spec.argv.first().expect("fixture gate argv");
                let tool = tools
                    .iter()
                    .find(|tool| tool.logical_name == *logical_name)
                    .expect("fixture gate tool");
                P7ReleaseGateReceipt {
                    gate_id: spec.gate_id.to_string(),
                    owner_root: spec.owner_root,
                    argv: std::iter::once(tool.canonical_path.clone())
                        .chain(spec.argv.into_iter().skip(1))
                        .collect(),
                    tool_sha256: tool.sha256.clone(),
                    environment_sha256: environment_sha256.to_string(),
                    exit_code: 0,
                    stdout_sha256: "a".repeat(64),
                    stderr_sha256: "b".repeat(64),
                    source_fingerprint_after: source_fingerprint.to_string(),
                }
            })
            .collect()
    }

    fn p7_test_gate_execution_identity(
    ) -> (Vec<P7ReleaseToolIdentity>, P7ReleaseEnvironmentAttestation) {
        let cargo = fs::canonicalize(env!("CARGO")).expect("canonical cargo");
        let bash = fs::canonicalize("/bin/bash").expect("canonical bash");
        let tools = [("bash", bash), ("cargo", cargo)]
            .into_iter()
            .map(|(logical_name, path)| P7ReleaseToolIdentity {
                logical_name: logical_name.to_string(),
                canonical_path: path.to_string_lossy().into_owned(),
                sha256: p7_sha256_file(&path).expect("fixture tool digest"),
                version: "fixture-version".to_string(),
            })
            .collect::<Vec<_>>();
        let variables = BTreeMap::from([
            ("CARGO_NET_OFFLINE".to_string(), "true".to_string()),
            ("LANG".to_string(), "C".to_string()),
            ("LC_ALL".to_string(), "C".to_string()),
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("RUST_BACKTRACE".to_string(), "0".to_string()),
        ]);
        let sha256 =
            p7_release_gate_environment_sha256(&variables).expect("fixture environment digest");
        (tools, P7ReleaseEnvironmentAttestation { variables, sha256 })
    }

    fn p7_test_release_bundle(label: &str) -> P7TestReleaseBundle {
        let canonical_temp = fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
        let root = canonical_temp.join(format!(
            "bm-p7-release-bundle-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let runner_root = root.join("runner");
        fs::create_dir_all(runner_root.join("src")).expect("runner source owner");
        fs::create_dir_all(runner_root.join("tests")).expect("runner test owner");
        for (path, bytes) in [
            (
                runner_root.join("Cargo.toml"),
                b"runner-manifest\n".as_slice(),
            ),
            (runner_root.join("Cargo.lock"), b"runner-lock\n".as_slice()),
            (runner_root.join("build.rs"), b"fn main() {}\n".as_slice()),
            (
                runner_root.join("src/main.rs"),
                b"fn main() {}\n".as_slice(),
            ),
            (
                runner_root.join("run_full_p7_wall.sh"),
                b"wall-v1\n".as_slice(),
            ),
            (
                runner_root.join("run_p7_max_rss.sh"),
                b"rss-v1\n".as_slice(),
            ),
            (
                runner_root.join("tests/run_full_p7_wall_fake_runner_test.sh"),
                b"wall-test-v1\n".as_slice(),
            ),
            (
                runner_root.join("tests/run_p7_max_rss_fake_runner_test.sh"),
                b"rss-test-v1\n".as_slice(),
            ),
        ] {
            fs::write(path, bytes).expect("runner release fixture");
        }
        let executable_bytes = b"release-executable-v1\n";
        let executable_sha256 = format!("{:x}", Sha256::digest(executable_bytes));
        let executable = p7_runner_release_executable_path(&root, &executable_sha256)
            .expect("content-addressed release path");
        let release_dir = executable.parent().expect("release owner");
        fs::create_dir_all(release_dir).expect("release owner directory");
        fs::write(&executable, executable_bytes).expect("release executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&executable)
                .expect("release executable metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions).expect("release executable mode");
        }
        let runner_inputs =
            p7_fingerprint_inputs(&runner_root, &P7_RUNNER_BUILD_INPUTS).expect("runner inputs");
        let identity = P7RunnerBuildIdentity {
            sdk_build_fingerprint: P7_TRUSTED_SDK_BUILD_FINGERPRINT.to_string(),
            runner_build_fingerprint: p7_fingerprint_files_with_contract(
                &runner_root,
                &runner_inputs,
                P7_RUNNER_BUILD_FINGERPRINT_CONTRACT,
            )
            .expect("runner build fingerprint"),
            runner_lock_fingerprint: p7_sha256_file(&runner_root.join("Cargo.lock"))
                .expect("runner lock fingerprint"),
            executable_sha256: executable_sha256.clone(),
            build_profile: "release".to_string(),
        };
        let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .and_then(|path| fs::canonicalize(path).ok())
            .expect("canonical SDK root");
        let canonical_runner_root = fs::canonicalize(&runner_root).expect("canonical runner root");
        let source_manifest = p7_release_gate_source_manifest(&sdk_root, &canonical_runner_root)
            .expect("fixture release source manifest");
        let source_fingerprint = source_manifest.source_fingerprint.clone();
        let source_manifest_path = release_dir.join(P7_RELEASE_GATE_SOURCE_MANIFEST_FILE_NAME);
        let source_manifest_bytes = p7_test_json_bytes(&source_manifest);
        fs::write(&source_manifest_path, &source_manifest_bytes).expect("release source manifest");
        let source_manifest_sha256 = format!("{:x}", Sha256::digest(&source_manifest_bytes));
        let (tools, environment) = p7_test_gate_execution_identity();
        let attestation = P7ReleaseGateAttestation {
            schema_version: P7_RELEASE_GATE_ATTESTATION_SCHEMA_VERSION.to_string(),
            orchestrator_contract: P7_RELEASE_GATE_ORCHESTRATOR_CONTRACT.to_string(),
            plan: p7_release_gate_plan(),
            identity: identity.clone(),
            source_fingerprint: source_fingerprint.clone(),
            source_manifest_sha256: source_manifest_sha256.clone(),
            gates: p7_test_gate_receipts(
                &sdk_root,
                &canonical_runner_root,
                &source_fingerprint,
                &tools,
                &environment.sha256,
            ),
            tools,
            environment,
        };
        let attestation_path = release_dir.join(P7_RELEASE_GATE_ATTESTATION_FILE_NAME);
        let attestation_bytes = p7_test_json_bytes(&attestation);
        fs::write(&attestation_path, &attestation_bytes).expect("release attestation");
        let metadata = P7ReleaseMetadata {
            schema_version: P7_RELEASE_METADATA_SCHEMA_VERSION.to_string(),
            canonical_executable_path: executable.to_string_lossy().into_owned(),
            identity,
            gate_attestation_sha256: format!("{:x}", Sha256::digest(&attestation_bytes)),
            gate_source_fingerprint: source_fingerprint,
            gate_source_manifest_sha256: source_manifest_sha256,
            gate_ids: P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
        };
        let metadata_path = release_dir.join(P7_RELEASE_METADATA_FILE_NAME);
        fs::write(&metadata_path, p7_test_json_bytes(&metadata)).expect("release metadata");
        P7TestReleaseBundle {
            root,
            executable_sha256,
            attestation_path,
            metadata_path,
            #[cfg(unix)]
            #[cfg(target_os = "linux")]
            source_manifest_path,
            attestation,
            metadata,
            source_manifest,
        }
    }

    #[cfg(unix)]
    #[cfg(target_os = "linux")]
    fn p7_test_write_release_governance(bundle: &mut P7TestReleaseBundle) {
        let release_dir =
            p7_runner_release_executable_path(&bundle.root, &bundle.executable_sha256)
                .expect("test release path")
                .parent()
                .expect("test release owner")
                .to_path_buf();
        bundle.attestation_path = release_dir.join(P7_RELEASE_GATE_ATTESTATION_FILE_NAME);
        bundle.metadata_path = release_dir.join(P7_RELEASE_METADATA_FILE_NAME);
        bundle.source_manifest_path = release_dir.join(P7_RELEASE_GATE_SOURCE_MANIFEST_FILE_NAME);
        let source_manifest_bytes = p7_test_json_bytes(&bundle.source_manifest);
        fs::write(&bundle.source_manifest_path, &source_manifest_bytes)
            .expect("write test release source manifest");
        let source_manifest_sha256 = format!("{:x}", Sha256::digest(&source_manifest_bytes));
        bundle.attestation.source_manifest_sha256 = source_manifest_sha256.clone();
        let attestation_bytes = p7_test_json_bytes(&bundle.attestation);
        fs::write(&bundle.attestation_path, &attestation_bytes)
            .expect("write test release attestation");
        bundle.metadata.identity = bundle.attestation.identity.clone();
        bundle.metadata.canonical_executable_path = release_dir
            .join(P7_RUNNER_RELEASE_FILE_NAME)
            .to_string_lossy()
            .into_owned();
        bundle.metadata.gate_attestation_sha256 =
            format!("{:x}", Sha256::digest(&attestation_bytes));
        bundle.metadata.gate_source_fingerprint = bundle.attestation.source_fingerprint.clone();
        bundle.metadata.gate_source_manifest_sha256 = source_manifest_sha256;
        bundle.metadata.gate_ids = bundle
            .attestation
            .gates
            .iter()
            .map(|gate| gate.gate_id.clone())
            .collect();
        fs::write(&bundle.metadata_path, p7_test_json_bytes(&bundle.metadata))
            .expect("write test release metadata");
    }

    #[cfg(unix)]
    #[cfg(target_os = "linux")]
    fn p7_test_republish_executable(bundle: &mut P7TestReleaseBundle, bytes: &[u8]) {
        bundle.executable_sha256 = format!("{:x}", Sha256::digest(bytes));
        let executable = p7_runner_release_executable_path(&bundle.root, &bundle.executable_sha256)
            .expect("republished executable path");
        fs::create_dir_all(executable.parent().expect("republished release owner"))
            .expect("republished release owner");
        fs::write(&executable, bytes).expect("republished executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&executable)
                .expect("republished executable metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions).expect("republished executable mode");
        }
        bundle.attestation.identity.executable_sha256 = bundle.executable_sha256.clone();
        p7_test_write_release_governance(bundle);
    }

    #[cfg(unix)]
    #[cfg(target_os = "linux")]
    fn p7_test_rebind_gate_source(bundle: &mut P7TestReleaseBundle, source_fingerprint: &str) {
        bundle.attestation.source_fingerprint = source_fingerprint.to_string();
        bundle.source_manifest.source_fingerprint = source_fingerprint.to_string();
        for receipt in &mut bundle.attestation.gates {
            receipt.source_fingerprint_after = source_fingerprint.to_string();
        }
        p7_test_write_release_governance(bundle);
    }

    #[test]
    fn runner_disk_identity_requires_exact_release_governance_bundle() {
        let bundle = p7_test_release_bundle("valid");
        let disk = p7_runner_disk_identity_for_release_sha(&bundle.root, &bundle.executable_sha256)
            .expect("valid release governance bundle");

        assert_eq!(
            disk.gate_attestation_sha256,
            bundle.metadata.gate_attestation_sha256
        );
        assert_eq!(
            disk.gate_source_fingerprint,
            bundle.source_manifest.source_fingerprint
        );
        assert_eq!(
            disk.gate_ids,
            P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
        );
        assert!(is_sha256(&disk.release_metadata_sha256));

        let _ = fs::remove_dir_all(bundle.root);
    }

    #[test]
    fn public_release_bundle_verifier_uses_explicit_source_locator_and_one_session() {
        let bundle = p7_test_release_bundle("public-verifier");
        let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .and_then(|path| fs::canonicalize(path).ok())
            .expect("canonical SDK root");
        let runner_source_root =
            fs::canonicalize(bundle.root.join("runner")).expect("canonical runner source root");
        let (verified, receipt) = verify_p7_published_release_bundle_with_receipt(
            &bundle.root,
            &sdk_root,
            &runner_source_root,
            &bundle.executable_sha256,
        )
        .expect("public retained release verifier");

        assert_eq!(
            verified.identity.runner_build_fingerprint,
            bundle.metadata.identity.runner_build_fingerprint
        );
        assert_eq!(
            verified.identity.runner_lock_fingerprint,
            bundle.metadata.identity.runner_lock_fingerprint
        );
        assert_eq!(
            verified.identity.executable_sha256,
            bundle.metadata.identity.executable_sha256
        );
        assert_eq!(
            verified.identity.gate_attestation_sha256,
            bundle.metadata.gate_attestation_sha256
        );
        assert_eq!(
            verified.executable_canonical_path,
            p7_runner_release_executable_path(&bundle.root, &bundle.executable_sha256)
                .expect("expected release executable")
        );
        assert!(receipt.passed);
        assert_eq!(receipt.unique_artifact_count, receipt.full_read_pass_count);
        assert_eq!(receipt.duplicate_artifact_count, 0);

        let _ = fs::remove_dir_all(bundle.root);
    }

    #[test]
    fn runner_disk_identity_rejects_metadata_and_gate_receipt_tampering() {
        let mut metadata_bundle = p7_test_release_bundle("metadata-tamper");
        metadata_bundle.metadata.gate_ids.swap(0, 1);
        fs::write(
            &metadata_bundle.metadata_path,
            p7_test_json_bytes(&metadata_bundle.metadata),
        )
        .expect("tampered release metadata");
        assert!(p7_runner_disk_identity_for_release_sha(
            &metadata_bundle.root,
            &metadata_bundle.executable_sha256,
        )
        .is_err());

        let mut identity_bundle = p7_test_release_bundle("identity-tamper");
        identity_bundle.metadata.identity.runner_lock_fingerprint = "9".repeat(64);
        fs::write(
            &identity_bundle.metadata_path,
            p7_test_json_bytes(&identity_bundle.metadata),
        )
        .expect("tampered release identity");
        assert!(p7_runner_disk_identity_for_release_sha(
            &identity_bundle.root,
            &identity_bundle.executable_sha256,
        )
        .is_err());

        let mut receipt_bundle = p7_test_release_bundle("receipt-tamper");
        receipt_bundle.attestation.gates[0].exit_code = 9;
        let attestation_bytes = p7_test_json_bytes(&receipt_bundle.attestation);
        fs::write(&receipt_bundle.attestation_path, &attestation_bytes)
            .expect("tampered release attestation");
        receipt_bundle.metadata.gate_attestation_sha256 =
            format!("{:x}", Sha256::digest(&attestation_bytes));
        fs::write(
            &receipt_bundle.metadata_path,
            p7_test_json_bytes(&receipt_bundle.metadata),
        )
        .expect("metadata rebound to failed receipt");
        assert!(p7_runner_disk_identity_for_release_sha(
            &receipt_bundle.root,
            &receipt_bundle.executable_sha256,
        )
        .is_err());

        let _ = fs::remove_dir_all(metadata_bundle.root);
        let _ = fs::remove_dir_all(identity_bundle.root);
        let _ = fs::remove_dir_all(receipt_bundle.root);
    }

    #[cfg(unix)]
    #[test]
    fn runner_disk_identity_rejects_symlinked_release_governance_file() {
        use std::os::unix::fs::symlink;

        let bundle = p7_test_release_bundle("attestation-symlink");
        let replacement = bundle.root.join("attestation-copy.json");
        fs::rename(&bundle.attestation_path, &replacement).expect("move attestation");
        symlink(&replacement, &bundle.attestation_path).expect("symlink attestation");

        assert!(
            p7_runner_disk_identity_for_release_sha(&bundle.root, &bundle.executable_sha256,)
                .is_err()
        );

        let _ = fs::remove_dir_all(bundle.root);
    }

    #[test]
    fn runner_disk_identity_tracks_exact_build_inputs_lock_and_executable() {
        let bundle = p7_test_release_bundle("identity-drift");
        let root = bundle.root;
        let executable_sha256 = bundle.executable_sha256;
        let runner = root.join("runner");
        let executable = p7_runner_release_executable_path(&root, &executable_sha256)
            .expect("content-addressed runner path");
        let original = p7_runner_disk_identity_for_release_sha(&root, &executable_sha256)
            .expect("initial runner identity");
        let producer = P7MergedProvenance {
            runner_build_fingerprint: original.runner_build_fingerprint.clone(),
            runner_lock_fingerprint: original.runner_lock_fingerprint.clone(),
            executable_sha256: original.executable_sha256.clone(),
            gate_attestation_sha256: original.gate_attestation_sha256.clone(),
            release_metadata_sha256: original.release_metadata_sha256.clone(),
            gate_source_fingerprint: original.gate_source_fingerprint.clone(),
            gate_source_manifest_sha256: original.gate_source_manifest_sha256.clone(),
            gate_ids: original.gate_ids.clone(),
            ..P7MergedProvenance::default()
        };
        validate_p7_runner_disk_provenance(&producer, &original)
            .expect("producer must match the exact runner bytes");

        fs::create_dir_all(runner.join("target/debug")).expect("debug target");
        fs::write(runner.join("target/debug/noise"), b"not a build input").expect("target noise");
        assert_eq!(
            p7_runner_disk_identity_for_release_sha(&root, &executable_sha256)
                .expect("identity after target noise"),
            original
        );

        fs::write(
            runner.join("src/main.rs"),
            b"fn main() { println!(\"v2\"); }\n",
        )
        .expect("changed runner source");
        assert!(p7_runner_disk_identity_for_release_sha(&root, &executable_sha256).is_err());

        fs::write(runner.join("src/main.rs"), b"fn main() {}\n").expect("restore runner source");
        fs::write(runner.join("Cargo.lock"), b"runner-lock-v2\n").expect("changed runner lock");
        assert!(p7_runner_disk_identity_for_release_sha(&root, &executable_sha256).is_err());
        fs::write(runner.join("Cargo.lock"), b"runner-lock\n").expect("restore runner lock");
        fs::write(&executable, b"binary-v2\n").expect("replace release bytes");
        assert!(p7_runner_disk_identity_for_release_sha(&root, &executable_sha256).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn runner_build_fingerprint_streams_each_length_prefixed_file() {
        struct BoundedReader {
            inner: std::io::Cursor<Vec<u8>>,
            largest_buffer: std::rc::Rc<std::cell::Cell<usize>>,
        }

        impl Read for BoundedReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.largest_buffer
                    .set(self.largest_buffer.get().max(buffer.len()));
                self.inner.read(buffer)
            }
        }

        let content = (0..(64 * 1024 * 3 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let largest_buffer = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut reader = BoundedReader {
            inner: std::io::Cursor::new(content.clone()),
            largest_buffer: largest_buffer.clone(),
        };
        let mut actual = Sha256::new();
        p7_hash_fingerprint_reader(&mut actual, content.len() as u64, &mut reader)
            .expect("streamed fingerprint field");

        let mut expected = Sha256::new();
        expected.update((content.len() as u64).to_le_bytes());
        expected.update(&content);
        assert_eq!(
            format!("{:x}", actual.finalize()),
            format!("{:x}", expected.finalize())
        );
        assert!(largest_buffer.get() <= 64 * 1024);
    }

    #[cfg(unix)]
    #[test]
    fn runner_preflight_never_executes_target_release_instead_of_content_addressed_release() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("bm-p7-alternate-executable-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let runner = root.join("runner");
        fs::create_dir_all(runner.join("src")).expect("runner src");
        fs::create_dir_all(runner.join("target/release")).expect("runner target");
        fs::write(runner.join("Cargo.toml"), b"[package]\nname='fixture'\n")
            .expect("runner manifest");
        fs::write(runner.join("Cargo.lock"), b"lock-v1\n").expect("runner lock");
        fs::write(runner.join("build.rs"), b"fn main() {}\n").expect("runner build script");
        fs::write(runner.join("src/main.rs"), b"fn main() {}\n").expect("runner source");
        fs::write(runner.join("run_full_p7_wall.sh"), b"wall\n").expect("wall script");
        fs::write(runner.join("run_p7_max_rss.sh"), b"rss\n").expect("RSS script");
        let marker = root.join("target-release-executed");
        let alternate = runner.join("target/release/beetle-memory-external-bench-runner");
        fs::write(
            &alternate,
            format!("#!/bin/sh\n: > '{}'\nexit 1\n", marker.display()),
        )
        .expect("alternate executable");
        let mut permissions = fs::metadata(&alternate)
            .expect("alternate executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&alternate, permissions).expect("alternate executable permissions");

        let executable_sha256 = p7_sha256_file(&alternate).expect("target executable digest");
        let runner_inputs =
            p7_fingerprint_inputs(&runner, &P7_RUNNER_BUILD_INPUTS).expect("runner build inputs");
        let frozen = P7FrozenRunnerIdentity {
            runner_build_fingerprint: Box::leak(
                p7_fingerprint_files_with_contract(
                    &runner,
                    &runner_inputs,
                    P7_RUNNER_BUILD_FINGERPRINT_CONTRACT,
                )
                .expect("runner build fingerprint")
                .into_boxed_str(),
            ),
            runner_lock_fingerprint: Box::leak(
                p7_sha256_file(&runner.join("Cargo.lock"))
                    .expect("runner lock fingerprint")
                    .into_boxed_str(),
            ),
            executable_sha256: Box::leak(executable_sha256.into_boxed_str()),
            gate_attestation_sha256: Box::leak("1".repeat(64).into_boxed_str()),
            release_metadata_sha256: Box::leak("3".repeat(64).into_boxed_str()),
            gate_source_fingerprint: Box::leak("2".repeat(64).into_boxed_str()),
            gate_source_manifest_sha256: Box::leak("4".repeat(64).into_boxed_str()),
        };
        let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("SDK root");

        assert!(
            preflight_p7_runner_release_with_frozen(&root, sdk_root, frozen, "test-run").is_err()
        );
        assert!(
            !marker.exists(),
            "target/release must never be executed as the frozen runner"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn runner_preflight_rejects_a_symlinked_frozen_executable() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("bm-p7-symlinked-runner-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let runner = root.join("runner");
        fs::create_dir_all(runner.join("src")).expect("runner src");
        fs::write(runner.join("Cargo.toml"), b"[package]\nname='fixture'\n")
            .expect("runner manifest");
        fs::write(runner.join("Cargo.lock"), b"lock-v1\n").expect("runner lock");
        fs::write(runner.join("build.rs"), b"fn main() {}\n").expect("runner build script");
        fs::write(runner.join("src/main.rs"), b"fn main() {}\n").expect("runner source");

        let alternate = root.join("alternate-runner");
        fs::write(&alternate, b"alternate executable bytes\n").expect("alternate executable");
        let executable_sha256 = p7_sha256_file(&alternate).expect("alternate digest");
        let executable = p7_runner_release_executable_path(&root, &executable_sha256)
            .expect("content-addressed runner path");
        fs::create_dir_all(executable.parent().expect("release parent"))
            .expect("release directory");
        symlink(&alternate, &executable).expect("symlinked frozen executable");

        assert!(p7_runner_disk_identity_for_release_sha(&root, &executable_sha256).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runner_preflight_rejects_stale_embedded_executable_identity() {
        let mut bundle = p7_test_release_bundle("stale-embedded-identity");
        let root = bundle.root.clone();
        let identity = bundle.attestation.identity.clone();
        let marker = root.join("identity-command-executed");
        let executable_body = format!(
            "#!/bin/sh\n: > '{}'\nprintf '%s\\n' '{{\"sdk_build_fingerprint\":\"{}\",\"runner_build_fingerprint\":\"{}\",\"runner_lock_fingerprint\":\"{}\",\"build_profile\":\"release\",\"executable_sha256\":\"{}\"}}'\n",
            marker.display(),
            identity.sdk_build_fingerprint,
            identity.runner_build_fingerprint,
            identity.runner_lock_fingerprint,
            "0".repeat(64),
        );
        p7_test_republish_executable(&mut bundle, executable_body.as_bytes());
        let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .and_then(|path| fs::canonicalize(path).ok())
            .expect("canonical SDK root");
        let fresh_gate_source = p7_release_gate_source_fingerprint(&sdk_root, &root.join("runner"))
            .expect("fresh gate source fingerprint");
        p7_test_rebind_gate_source(&mut bundle, &fresh_gate_source);
        let disk = p7_runner_disk_identity_for_release_sha(&root, &bundle.executable_sha256)
            .expect("runner disk identity");
        let frozen = P7FrozenRunnerIdentity {
            runner_build_fingerprint: Box::leak(
                disk.runner_build_fingerprint.clone().into_boxed_str(),
            ),
            runner_lock_fingerprint: Box::leak(
                disk.runner_lock_fingerprint.clone().into_boxed_str(),
            ),
            executable_sha256: Box::leak(disk.executable_sha256.clone().into_boxed_str()),
            gate_attestation_sha256: Box::leak(
                disk.gate_attestation_sha256.clone().into_boxed_str(),
            ),
            release_metadata_sha256: Box::leak(
                disk.release_metadata_sha256.clone().into_boxed_str(),
            ),
            gate_source_fingerprint: Box::leak(
                disk.gate_source_fingerprint.clone().into_boxed_str(),
            ),
            gate_source_manifest_sha256: Box::leak(
                disk.gate_source_manifest_sha256.clone().into_boxed_str(),
            ),
        };

        let error = preflight_p7_runner_release_with_frozen(&root, &sdk_root, frozen, "test-run")
            .expect_err("stale embedded identity must fail closed");
        assert!(
            marker.is_file(),
            "frozen runner identity was not executed: {error:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn release_shard_full_run_coordinates_reject_question_index() {
        let mut shard = serde_json::json!({
            "limit": null,
            "question_limit": null,
            "question_index": null
        });
        validate_p7_release_shard_full_run(&shard).expect("full release shard");

        shard["question_index"] = serde_json::json!(0);
        assert!(validate_p7_release_shard_full_run(&shard).is_err());
    }

    #[test]
    fn frozen_runner_identity_contract_is_structurally_valid() {
        let identity = P7FrozenRunnerIdentity {
            runner_build_fingerprint:
                "1111111111111111111111111111111111111111111111111111111111111111",
            runner_lock_fingerprint:
                "2222222222222222222222222222222222222222222222222222222222222222",
            executable_sha256: "3333333333333333333333333333333333333333333333333333333333333333",
            gate_attestation_sha256:
                "4444444444444444444444444444444444444444444444444444444444444444",
            release_metadata_sha256:
                "6666666666666666666666666666666666666666666666666666666666666666",
            gate_source_fingerprint:
                "5555555555555555555555555555555555555555555555555555555555555555",
            gate_source_manifest_sha256:
                "7777777777777777777777777777777777777777777777777777777777777777",
        };
        assert!(is_sha256(identity.runner_build_fingerprint));
        assert!(is_sha256(identity.runner_lock_fingerprint));
        assert!(is_sha256(identity.executable_sha256));
        assert!(is_sha256(identity.gate_attestation_sha256));
        assert!(is_sha256(identity.release_metadata_sha256));
        assert!(is_sha256(identity.gate_source_fingerprint));
        assert!(is_sha256(identity.gate_source_manifest_sha256));
    }

    #[test]
    fn producer_identity_preserves_recorded_detail_schema_without_verifier_backfill() {
        let provenance = P7MergedProvenance {
            detail_schema_version: "p7_question_detail_fixture_v0".to_string(),
            producer_identity: P7RecordedProducerIdentity::record(&P7ProducerIdentity {
                detail_schema_version: "p7_question_detail_fixture_v0".to_string(),
                ..P7ProducerIdentity {
                    schema_version: P7_PRODUCER_IDENTITY_SCHEMA_VERSION.to_string(),
                    contract_version: String::new(),
                    sdk_report_schema_version: 0,
                    sdk_build_fingerprint: String::new(),
                    runner_build_fingerprint: String::new(),
                    runner_lock_fingerprint: String::new(),
                    executable_sha256: String::new(),
                    build_profile: String::new(),
                    input_sha256: String::new(),
                    detail_schema_version: String::new(),
                }
            })
            .expect("record fixture producer identity"),
            ..P7MergedProvenance::default()
        };

        assert_eq!(
            p7_producer_identity(&provenance)
                .expect("recorded producer identity")
                .detail_schema_version,
            "p7_question_detail_fixture_v0"
        );
    }

    #[test]
    fn recorded_producer_identity_rejects_digest_tamper_and_noncanonical_original() {
        let mut recorded = P7RecordedProducerIdentity::record(&serde_json::json!({
            "a": 1,
            "b": 2,
        }))
        .expect("record producer identity fixture");
        recorded.canonical_identity_sha256 = "0".repeat(64);
        assert!(recorded.parse::<serde_json::Value>().is_err());

        let noncanonical = r#"{"b":2,"a":1}"#.to_string();
        let recorded = P7RecordedProducerIdentity {
            schema_version: P7_RECORDED_PRODUCER_IDENTITY_SCHEMA_VERSION.to_string(),
            canonical_identity_sha256: format!("{:x}", Sha256::digest(noncanonical.as_bytes())),
            canonical_identity: noncanonical,
        };
        assert!(recorded.parse::<serde_json::Value>().is_err());
    }

    #[test]
    fn verifier_release_manifest_binds_release_profile_content_address_and_source_anchor() {
        let executable_sha256 = "1".repeat(64);
        let source_anchor = "2".repeat(64);
        let manifest_sha256 = "3".repeat(64);
        let features = vec!["alpha".to_string(), "beta".to_string()];
        let manifest = P7VerifierReleaseManifest {
            schema_version: P7_VERIFIER_RELEASE_MANIFEST_SCHEMA_VERSION.to_string(),
            executable_file_name: "bm-w4-external-noisy-wall".to_string(),
            executable_sha256: executable_sha256.clone(),
            build_profile: "release".to_string(),
            build_features: features.clone(),
            verification_policy_contract: P7_VERIFICATION_POLICY_CONTRACT.to_string(),
            verification_schema_version: P7_VERIFICATION_RECEIPT_SCHEMA_VERSION.to_string(),
            source_anchor_sha256: source_anchor.clone(),
            frozen_anchor_sha256: P7_FROZEN_ANCHOR_SHA256.to_string(),
            anchor_generator_receipt_sha256: P7_FROZEN_ANCHOR_GENERATOR_RECEIPT_SHA256.to_string(),
        };
        p7_validate_verifier_release_manifest(
            &manifest,
            "bm-w4-external-noisy-wall",
            &executable_sha256,
            &executable_sha256,
            "release",
            &features,
            &source_anchor,
            &manifest_sha256,
        )
        .expect("exact verifier release manifest");

        for tampered in [
            P7VerifierReleaseManifest {
                build_profile: "debug".to_string(),
                ..manifest.clone()
            },
            P7VerifierReleaseManifest {
                source_anchor_sha256: "4".repeat(64),
                ..manifest.clone()
            },
            P7VerifierReleaseManifest {
                build_features: vec!["beta".to_string(), "alpha".to_string()],
                ..manifest.clone()
            },
        ] {
            assert!(p7_validate_verifier_release_manifest(
                &tampered,
                "bm-w4-external-noisy-wall",
                &executable_sha256,
                &executable_sha256,
                "release",
                &features,
                &source_anchor,
                &manifest_sha256,
            )
            .is_err());
        }
        assert!(p7_validate_verifier_release_manifest(
            &manifest,
            "bm-w4-external-noisy-wall",
            &executable_sha256,
            &"9".repeat(64),
            "release",
            &features,
            &source_anchor,
            &manifest_sha256,
        )
        .is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn v1_immutable_producer_cohort_passes_full_preflight_under_v2_verifier() {
        let mut bundle = p7_test_release_bundle("v1-producer-v2-verifier");
        let release_identity = bundle.attestation.identity.clone();
        let executable_body = format!(
            "#!/bin/sh\nsha=${{BM_P7_RETAINED_EXECUTABLE_SHA256:?}}\nprintf '%s\\n' '{{\"sdk_build_fingerprint\":\"{}\",\"runner_build_fingerprint\":\"{}\",\"runner_lock_fingerprint\":\"{}\",\"executable_sha256\":\"'\"$sha\"'\",\"build_profile\":\"release\"}}'\n",
            release_identity.sdk_build_fingerprint,
            release_identity.runner_build_fingerprint,
            release_identity.runner_lock_fingerprint,
        );
        p7_test_republish_executable(&mut bundle, executable_body.as_bytes());
        let disk = p7_runner_disk_identity_for_release_sha(&bundle.root, &bundle.executable_sha256)
            .expect("immutable V1 producer release");
        let preflight = P7RunnerPreflightReport {
            schema_version: P7_RUNNER_PREFLIGHT_SCHEMA_VERSION.to_string(),
            run_id: "test-run".to_string(),
            sdk_build_fingerprint: bundle.attestation.identity.sdk_build_fingerprint.clone(),
            runner_build_fingerprint: disk.runner_build_fingerprint.clone(),
            runner_lock_fingerprint: disk.runner_lock_fingerprint.clone(),
            executable_sha256: disk.executable_sha256.clone(),
            executable_canonical_path: disk
                .executable_canonical_path
                .to_string_lossy()
                .into_owned(),
            gate_attestation_sha256: disk.gate_attestation_sha256.clone(),
            release_metadata_sha256: disk.release_metadata_sha256.clone(),
            gate_source_fingerprint: disk.gate_source_fingerprint.clone(),
            gate_source_manifest_sha256: disk.gate_source_manifest_sha256.clone(),
            gate_ids: disk.gate_ids.clone(),
            build_profile: "release".to_string(),
        };
        validate_p7_producer_preflight_report(&bundle.root, "test-run", &preflight)
            .expect("V2 operator must validate the immutable producer preflight");
        let preflight_bytes = serde_json::to_vec(&preflight).expect("producer preflight bytes");
        assert!(!preflight_bytes
            .windows(b"operator_build_fingerprint".len())
            .any(|window| window == b"operator_build_fingerprint"));
        assert!(!preflight_bytes
            .windows(b"orchestration_fingerprint".len())
            .any(|window| window == b"orchestration_fingerprint"));

        let producer = P7ProducerIdentity {
            schema_version: P7_PRODUCER_IDENTITY_SCHEMA_VERSION.to_string(),
            contract_version: P7_CONTRACT_VERSION.to_string(),
            sdk_report_schema_version: MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            sdk_build_fingerprint: "1".repeat(64),
            runner_build_fingerprint: "2".repeat(64),
            runner_lock_fingerprint: "3".repeat(64),
            executable_sha256: "4".repeat(64),
            build_profile: "release".to_string(),
            input_sha256: "5".repeat(64),
            detail_schema_version: P7_DETAIL_SCHEMA_VERSION.to_string(),
        };
        let producer_digest = p7_json_digest(&producer).expect("producer digest");
        let cohort_entries = vec![(
            "locomo".to_string(),
            "6".repeat(64),
            producer_digest.clone(),
            "7".repeat(64),
        )];
        let first_verifier = p7_verifier_identity(
            &"8".repeat(64),
            &"a".repeat(64),
            "release",
            vec!["producer-v1".to_string()],
            &"c".repeat(64),
            &"8".repeat(64),
        );
        let next_verifier = p7_verifier_identity(
            &"9".repeat(64),
            &"b".repeat(64),
            "release",
            vec![
                "strict-locator-v2".to_string(),
                "typed-applicability-v2".to_string(),
            ],
            &"d".repeat(64),
            &"9".repeat(64),
        );

        let first = p7_verification_receipt_for_cohort_entries(&cohort_entries, &first_verifier)
            .expect("first verification receipt");
        let next = p7_verification_receipt_for_cohort_entries(&cohort_entries, &next_verifier)
            .expect("next verification receipt");

        assert_eq!(first.cohort_digest, next.cohort_digest);
        assert_eq!(cohort_entries[0].2, producer_digest);
        assert_ne!(first.verifier_digest, next.verifier_digest);
        assert_ne!(first.receipt_digest, next.receipt_digest);
        assert_eq!(
            serde_json::to_vec(&preflight).expect("unchanged producer preflight"),
            preflight_bytes
        );
        let _ = fs::remove_dir_all(bundle.root);
    }

    #[test]
    fn dataset_stream_rebuilds_identity_and_hashes_exact_bytes() {
        let path = std::env::temp_dir().join(format!("bm-p7-dataset-{}.json", std::process::id()));
        let bytes = br#"[{"question_id":"q-1","question":"Q","answer_session_ids":["D1"]}]"#;
        fs::write(&path, bytes).expect("write dataset fixture");
        let dataset = P7TrustedDataset {
            suite: "longmemeval_oracle",
            file_name: "fixture.json",
            input_sha256: "84c317644a0265265c91c7e13510dc5cd36c6634532904ee1484f2dbbc26bc00",
        };

        let mut file = File::open(&path).expect("open dataset fixture");
        let expected =
            load_p7_dataset_expectation(&mut file, dataset, 1).expect("dataset should verify");
        assert_eq!(expected.samples_by_shard, vec![1]);
        assert_eq!(expected.questions_by_shard[0][0].question_id, "q-1");

        fs::write(&path, [bytes.as_slice(), b"\n"].concat()).expect("tamper dataset bytes");
        let mut tampered = File::open(&path).expect("reopen tampered dataset fixture");
        assert!(load_p7_dataset_expectation(&mut tampered, dataset, 1).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn dataset_stream_enforces_object_allocation_ceiling_before_growth() {
        fn fixture(object_bytes: usize) -> Vec<u8> {
            let prefix = br#"{"padding":""#;
            let suffix = br#""}"#;
            let padding = object_bytes
                .checked_sub(prefix.len() + suffix.len())
                .expect("object ceiling must fit JSON framing");
            let mut bytes = Vec::with_capacity(object_bytes + 2);
            bytes.push(b'[');
            bytes.extend_from_slice(prefix);
            bytes.extend(std::iter::repeat_n(b'a', padding));
            bytes.extend_from_slice(suffix);
            bytes.push(b']');
            bytes
        }

        const LIMIT: usize = 64;
        let mut exact =
            P7JsonArrayObjectStream::with_object_limit(std::io::Cursor::new(fixture(LIMIT)), LIMIT);
        assert!(exact
            .next_object()
            .expect("exact-boundary object")
            .is_some());
        assert!(exact.next_object().expect("array terminator").is_none());
        exact.finish().expect("exact-boundary stream");

        let mut plus_one = P7JsonArrayObjectStream::with_object_limit(
            std::io::Cursor::new(fixture(LIMIT + 1)),
            LIMIT,
        );
        assert!(plus_one.next_object().is_err());
    }

    #[test]
    fn packaged_build_cannot_issue_a_p7_verifier_release_identity() {
        p7_validate_build_source_attestation(P7_WORKSPACE_BUILD_SOURCE_ATTESTATION)
            .expect("workspace source attestation");

        let error = p7_validate_build_source_attestation("packaged_unattested")
            .expect_err("packaged source must fail closed");

        assert!(error
            .to_string()
            .contains("requires an attested workspace source build"));
    }
}

fn p7_provenance_error(message: &'static str) -> Error {
    Error::Io {
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
        stage: "p7_provenance_verify_files",
    }
}

fn p7_preflight_error(message: &'static str) -> Error {
    Error::Io {
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
        stage: "p7_runner_preflight",
    }
}

fn w4_external_noisy_suite_report(
    summary: &W4ExternalNoisyBenchmarkSummary,
) -> W4ExternalNoisySuiteReport {
    let expected = w4_external_suite_expectation(&summary.suite);
    let shard_count = summary.shards.len();
    let shards_valid = expected.is_some_and(|expected| {
        let expected_names = (0..expected.shard_count)
            .map(|index| {
                format!(
                    "{}.shard-{index}-of-{}.summary.json",
                    summary.suite, expected.shard_count
                )
            })
            .collect::<Vec<_>>();
        let actual_names = summary
            .shards
            .iter()
            .map(|shard| shard.trim().to_string())
            .collect::<Vec<_>>();
        actual_names == expected_names
    });
    let row_counts_valid = expected.is_some_and(|expected| {
        summary.samples == expected.samples
            && summary.questions == expected.questions
            && summary.evidence_questions == expected.evidence_questions
            && summary.questions
                == summary
                    .evidence_questions
                    .saturating_add(summary.no_gold_questions)
    });
    let baseline = w4_external_suite_baseline(&summary.suite);
    let regressed_against_baseline = baseline.is_some_and(|baseline| {
        summary.any_evidence_hit < baseline.any_evidence_hit
            || summary.all_evidence_hit < baseline.all_evidence_hit
    });
    let improved_against_baseline = baseline.is_some_and(|baseline| {
        summary.any_evidence_hit > baseline.any_evidence_hit
            && summary.all_evidence_hit > baseline.all_evidence_hit
    });
    let stage_attributed_improvement =
        improved_against_baseline && stage_counts_show_graph_attributed_gain(summary, baseline);
    let index_effect_proven = stage_attributed_improvement
        && summary
            .index_diagnostics
            .as_ref()
            .is_some_and(index_diagnostics_show_index_effect);
    W4ExternalNoisySuiteReport {
        suite: summary.suite.clone(),
        run_id: summary.run_id.clone(),
        completed: summary.completed,
        samples: summary.samples,
        questions: summary.questions,
        evidence_questions: summary.evidence_questions,
        no_gold_questions: summary.no_gold_questions,
        any_evidence_hit: summary.any_evidence_hit,
        all_evidence_hit: summary.all_evidence_hit,
        write_errors: summary.write_errors,
        recall_errors: summary.recall_errors,
        shard_count,
        expected_shard_count: expected.map(|expected| expected.shard_count),
        shards_valid,
        expected_samples: expected.map(|expected| expected.samples),
        expected_questions: expected.map(|expected| expected.questions),
        expected_evidence_questions: expected.map(|expected| expected.evidence_questions),
        row_counts_valid,
        summary_sha256: summary.summary_sha256.clone(),
        runner_source_sha256: summary.runner_source_sha256.clone(),
        any_evidence_hit_bps: evidence_hit_bps(
            summary.any_evidence_hit,
            summary.evidence_questions,
        ),
        all_evidence_hit_bps: evidence_hit_bps(
            summary.all_evidence_hit,
            summary.evidence_questions,
        ),
        noisy_split: matches!(
            summary.suite.as_str(),
            "locomo" | "longmemeval_s_cleaned" | "longmemeval_m_cleaned"
        ),
        oracle_sanity_only: summary.suite == "longmemeval_oracle",
        baseline_any_evidence_hit: baseline.map(|baseline| baseline.any_evidence_hit),
        baseline_all_evidence_hit: baseline.map(|baseline| baseline.all_evidence_hit),
        regressed_against_baseline,
        improved_against_baseline,
        stage_hit_counts: summary.stage_hit_counts.clone(),
        index_diagnostics: summary.index_diagnostics.clone(),
        w4_1_diagnostics: summary.w4_1_diagnostics.clone(),
        facet_ablation: summary.facet_ablation.clone(),
        p7_loss_ledger: summary.p7_loss_ledger.clone(),
        p7_production_delivery: summary.p7_production_delivery.clone(),
        p7_provenance: summary.p7_provenance.clone(),
        stage_attributed_improvement,
        index_effect_proven,
        facet_ablation_effect_proven: w4_external_facet_ablation_proves_effect(summary),
        facet_ablation_no_render_growth: summary
            .facet_ablation
            .as_ref()
            .is_some_and(|diagnostics| diagnostics.render_growth == 0),
    }
}

fn w4_external_w41_diagnostics_cover_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.w4_1_diagnostics.as_ref() else {
        return false;
    };
    diagnostics.questions_with_w4_1_diagnostics == summary.questions
        && diagnostics.questions_with_w4_1_diagnostics > 0
        && !diagnostics.first_any_hit_stage_counts.is_empty()
        && !diagnostics.missing_gold_by_stage_counts.is_empty()
        && !diagnostics.question_type_counts.is_empty()
        && !diagnostics.evidence_count_buckets.is_empty()
        && diagnostics
            .gold_rank_found_count
            .saturating_add(diagnostics.gold_rank_missing_count)
            > 0
        && diagnostics.source_signature_count > 0
}

fn w4_external_facet_ablation_covers_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.facet_ablation.as_ref() else {
        return false;
    };
    diagnostics.questions_with_ablation_report == summary.evidence_questions
        && diagnostics
            .method_counts
            .get(P7_ABLATION_METHOD)
            .copied()
            .unwrap_or(0)
            == summary.evidence_questions
        && P7_REQUIRED_ABLATION_SLICES.iter().all(|slice| {
            diagnostics
                .required_slice_counts
                .get(*slice)
                .copied()
                .unwrap_or(0)
                == summary.evidence_questions
                && diagnostics
                    .report_available_slice_counts
                    .get(*slice)
                    .copied()
                    .unwrap_or(0)
                    == summary.evidence_questions
        })
}

fn p7_loss_ledger_covers_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    summary.p7_loss_ledger.as_ref().is_some_and(|diagnostics| {
        diagnostics.questions_with_loss_ledger == summary.questions
            && diagnostics.questions_with_loss_ledger > 0
            && diagnostics.eval_truncated_count == 0
            && diagnostics.eval_blocked_reason_counts.is_empty()
    })
}

fn p7_suite_quality_threshold_met(
    summaries: &[W4ExternalNoisyBenchmarkSummary],
    suite: &str,
    selected_stage: bool,
) -> bool {
    let Some(summary) = summaries.iter().find(|summary| summary.suite == suite) else {
        return false;
    };
    let Some(stage) = summary.stage_hit_counts.as_ref() else {
        return false;
    };
    let (minimum_any, minimum_all) = match (suite, selected_stage) {
        ("locomo", true) => (931, 818),
        ("longmemeval_s_cleaned", true) => (475, 405),
        ("longmemeval_m_cleaned", true) => (225, 124),
        ("locomo", false) => (139, 111),
        ("longmemeval_s_cleaned", false) => (281, 142),
        ("longmemeval_m_cleaned", false) => (43, 21),
        _ => return false,
    };
    let (actual_any, actual_all) = if selected_stage {
        (
            stage.projection_selected_any_evidence_hit,
            stage.projection_selected_all_evidence_hit,
        )
    } else {
        (
            stage.rendered_any_evidence_hit,
            stage.rendered_all_evidence_hit,
        )
    };
    actual_any >= minimum_any && actual_all >= minimum_all
}

fn p7_ablation_proves_suite_effect(
    summaries: &[W4ExternalNoisyBenchmarkSummary],
    suite: &str,
) -> bool {
    let Some(summary) = summaries.iter().find(|summary| summary.suite == suite) else {
        return false;
    };
    let Some(diagnostics) = summary.facet_ablation.as_ref() else {
        return false;
    };
    if !w4_external_facet_ablation_covers_summary(summary)
        || !diagnostics.blocked_reason_counts.is_empty()
        || diagnostics.render_growth != 0
        || diagnostics
            .rendered_evidence_hit_delta
            .get("render_capsule_off")
            .copied()
            .unwrap_or(0)
            <= 0
    {
        return false;
    }
    if matches!(suite, "locomo" | "longmemeval_m_cleaned") {
        diagnostics
            .selected_evidence_hit_delta
            .get("delivery_relevance_fusion_off")
            .copied()
            .unwrap_or(0)
            > 0
    } else {
        true
    }
}

fn p7_production_delivery_covers_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    summary
        .p7_production_delivery
        .as_ref()
        .is_some_and(|diagnostics| {
            diagnostics.questions_with_delivery_report == summary.questions
                && diagnostics.eval_selected_matches_delivery_questions == summary.questions
                && diagnostics.eval_rendered_matches_delivery_questions == summary.questions
                && diagnostics.projection_selected_sources_proven_questions == summary.questions
                && diagnostics.projection_delivery_proof_questions == summary.questions
                && diagnostics.final_projection_integrity_questions == summary.questions
                && diagnostics.final_projection_integrity_passed_questions == summary.questions
                && diagnostics
                    .schema_version_counts
                    .get(&MEMORY_RECALL_DELIVERY_SCHEMA_VERSION.to_string())
                    .copied()
                    .unwrap_or(0)
                    == summary.questions
                && diagnostics.blocked_reason_counts.is_empty()
        })
}

fn p7_production_delivery_has_no_privacy_regression(
    summary: &W4ExternalNoisyBenchmarkSummary,
) -> bool {
    summary
        .p7_production_delivery
        .as_ref()
        .is_some_and(|diagnostics| {
            diagnostics.privacy_leak_count == 0
                && diagnostics.cross_subject_leak_count == 0
                && diagnostics.raw_soul_private_material_count == 0
                && diagnostics.final_projection_raw_private_violation_count == 0
        })
}

fn p7_provenance_valid_for_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(provenance) = summary.p7_provenance.as_ref() else {
        return false;
    };
    let Some(dataset) = p7_trusted_dataset(&summary.suite) else {
        return false;
    };
    if provenance.contract_version != P7_CONTRACT_VERSION
        || !p7_valid_run_id(&summary.run_id)
        || provenance.run_id != summary.run_id
        || provenance.sdk_report_schema_version != MEMORY_RECALL_DELIVERY_SCHEMA_VERSION
        || provenance.sdk_build_fingerprint != P7_TRUSTED_SDK_BUILD_FINGERPRINT
        || !is_sha256(&provenance.runner_build_fingerprint)
        || !is_sha256(&provenance.runner_lock_fingerprint)
        || !is_sha256(&provenance.executable_sha256)
        || provenance.gate_ids != P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec()
        || !is_sha256(&provenance.gate_attestation_sha256)
        || !is_sha256(&provenance.gate_source_fingerprint)
        || provenance.build_profile != "release"
        || provenance.input_sha256 != dataset.input_sha256
        || !p7_detail_schema_supported(&provenance.detail_schema_version)
        || !is_sha256(&provenance.merged_detail_sha256)
        || !summary.summary_sha256.as_deref().is_some_and(is_sha256)
        || summary.runner_source_sha256.as_deref()
            != Some(provenance.runner_build_fingerprint.as_str())
        || !summary.operator_content_hash_verified
        || provenance.ordered_shard_digest_manifest.len() != summary.shards.len()
    {
        return false;
    }
    provenance
        .ordered_shard_digest_manifest
        .iter()
        .zip(summary.shards.iter())
        .all(|(digest, shard)| {
            digest.run_id == summary.run_id
                && digest.shard == *shard
                && is_sha256(&digest.summary_sha256)
                && is_sha256(&digest.detail_sha256)
        })
}

fn p7_trusted_dataset(suite: &str) -> Option<P7TrustedDataset> {
    P7_TRUSTED_DATASETS
        .iter()
        .copied()
        .find(|dataset| dataset.suite == suite)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn w4_external_facet_ablation_proves_effect(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.facet_ablation.as_ref() else {
        return false;
    };
    w4_external_facet_ablation_covers_summary(summary)
        && diagnostics.delivery_contribution_proven_questions > 0
        && diagnostics.blocked_reason_counts.is_empty()
        && diagnostics
            .delivery_contribution_proven_slice_counts
            .get("facet_off")
            .copied()
            .unwrap_or(0)
            > 0
}

fn stage_counts_show_graph_attributed_gain(
    summary: &W4ExternalNoisyBenchmarkSummary,
    baseline: Option<W4ExternalSuiteBaseline>,
) -> bool {
    let Some(baseline) = baseline else {
        return false;
    };
    let Some(stage) = summary.stage_hit_counts.as_ref() else {
        return false;
    };
    let any_gain_after_source = stage.expanded_any_evidence_hit > stage.source_any_evidence_hit
        || stage.reranked_any_evidence_hit > stage.source_any_evidence_hit
        || stage.selected_any_evidence_hit > stage.source_any_evidence_hit
        || stage.rendered_any_evidence_hit > stage.source_any_evidence_hit;
    let all_gain_after_source = stage.expanded_all_evidence_hit > stage.source_all_evidence_hit
        || stage.reranked_all_evidence_hit > stage.source_all_evidence_hit
        || stage.selected_all_evidence_hit > stage.source_all_evidence_hit
        || stage.rendered_all_evidence_hit > stage.source_all_evidence_hit;
    stage.selected_any_evidence_hit > baseline.any_evidence_hit
        && stage.selected_all_evidence_hit > baseline.all_evidence_hit
        && stage.rendered_any_evidence_hit > baseline.any_evidence_hit
        && stage.rendered_all_evidence_hit > baseline.all_evidence_hit
        && any_gain_after_source
        && all_gain_after_source
}

fn index_diagnostics_show_index_effect(diagnostics: &W4ExternalNoisyIndexDiagnostics) -> bool {
    diagnostics.questions_with_index_report > 0
        && diagnostics.index_used_questions > 0
        && diagnostics.index_used_questions <= diagnostics.questions_with_index_report
        && diagnostics.fallback_full_scan_questions == 0
        && diagnostics.matched_source_anchor_count > 0
        && diagnostics.indexed_neighbor_count > 0
        && diagnostics.failure_count == 0
        && graph_v2_index_release_conditions_hold(diagnostics)
        && diagnostics.facet_questions_with_index_report > 0
        && diagnostics.facet_index_used_questions == diagnostics.facet_questions_with_index_report
        && diagnostics.facet_report_only_questions == 0
        && diagnostics.facet_fallback_full_scan_questions == 0
        && diagnostics.facet_posting_key_lookup_count >= diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_matched_posting_count
            == diagnostics.facet_posting_doc_read_count
        && diagnostics.facet_owner_key_lookup_count == diagnostics.facet_owner_doc_read_count
        && diagnostics.facet_zero_posting_key_lookup_questions == 0
        && diagnostics.facet_clean_zero_hit_questions <= diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_integrity_verified_questions
            == diagnostics.facet_questions_with_index_report
        && diagnostics.facet_manifest_integrity_failure_count == 0
        && diagnostics.facet_failure_count == 0
}

fn w4_external_index_diagnostics_no_full_scan(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.index_diagnostics.as_ref() else {
        return true;
    };
    let facet_used_requirement_holds = if summary.suite == "longmemeval_oracle" {
        diagnostics.facet_index_used_questions > 0
            && diagnostics.facet_index_used_questions <= summary.questions
    } else {
        diagnostics.facet_index_used_questions == summary.questions
    };
    diagnostics.questions_with_index_report == summary.questions
        && diagnostics.index_used_questions > 0
        && diagnostics.fallback_full_scan_questions == 0
        && diagnostics.failure_count == 0
        && diagnostics.matched_source_anchor_count > 0
        && diagnostics.indexed_neighbor_count > 0
        && graph_v2_index_release_conditions_hold(diagnostics)
        && diagnostics.facet_questions_with_index_report == summary.questions
        && facet_used_requirement_holds
        && diagnostics.facet_report_only_questions == 0
        && diagnostics.facet_fallback_full_scan_questions == 0
        && diagnostics.facet_posting_key_lookup_count >= diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_matched_posting_count
            == diagnostics.facet_posting_doc_read_count
        && diagnostics.facet_owner_key_lookup_count == diagnostics.facet_owner_doc_read_count
        && diagnostics.facet_zero_posting_key_lookup_questions == 0
        && diagnostics.facet_clean_zero_hit_questions <= diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_integrity_verified_questions
            == diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_integrity_failure_count == 0
        && diagnostics.facet_failure_count == 0
}

fn graph_v2_index_release_conditions_hold(diagnostics: &W4ExternalNoisyIndexDiagnostics) -> bool {
    diagnostics.graph_manifest_contract_verified_questions == diagnostics.index_used_questions
        && diagnostics.graph_selected_dependency_chain_verified_questions
            == diagnostics.index_used_questions
        && diagnostics.graph_manifest_generation_present_questions
            == diagnostics.index_used_questions
        && diagnostics.graph_revision_present_questions == diagnostics.index_used_questions
        && diagnostics.graph_scope_digest_present_questions == diagnostics.index_used_questions
        && diagnostics.graph_maintenance_required_questions == 0
        && diagnostics.graph_incident_questions == 0
        && diagnostics.graph_read_path_mutation_delta == 0
}

#[derive(Clone, Copy)]
struct W4ExternalSuiteExpectation {
    samples: usize,
    questions: usize,
    evidence_questions: usize,
    shard_count: usize,
}

#[derive(Clone, Copy)]
struct W4ExternalSuiteBaseline {
    any_evidence_hit: usize,
    all_evidence_hit: usize,
}

fn w4_external_suite_expectation(suite: &str) -> Option<W4ExternalSuiteExpectation> {
    match suite {
        "locomo" => Some(W4ExternalSuiteExpectation {
            samples: 10,
            questions: 1986,
            evidence_questions: 1982,
            shard_count: 10,
        }),
        "longmemeval_oracle" | "longmemeval_s_cleaned" | "longmemeval_m_cleaned" => {
            Some(W4ExternalSuiteExpectation {
                samples: 500,
                questions: 500,
                evidence_questions: 500,
                shard_count: if suite == "longmemeval_m_cleaned" {
                    8
                } else {
                    1
                },
            })
        }
        _ => None,
    }
}

fn evidence_hit_bps(hits: usize, evidence_questions: usize) -> u32 {
    if evidence_questions == 0 {
        return 0;
    }
    ((hits.saturating_mul(10_000)) / evidence_questions).min(u32::MAX as usize) as u32
}

fn w4_external_suite_baseline(suite: &str) -> Option<W4ExternalSuiteBaseline> {
    match suite {
        "locomo" => Some(W4ExternalSuiteBaseline {
            any_evidence_hit: 297,
            all_evidence_hit: 189,
        }),
        "longmemeval_s_cleaned" => Some(W4ExternalSuiteBaseline {
            any_evidence_hit: 451,
            all_evidence_hit: 353,
        }),
        "longmemeval_m_cleaned" => Some(W4ExternalSuiteBaseline {
            any_evidence_hit: 104,
            all_evidence_hit: 33,
        }),
        _ => None,
    }
}

pub fn load_memory_benchmark_fixture_dir(
    root: impl AsRef<Path>,
) -> Result<Vec<MemoryBenchmarkFixture>> {
    let mut files = Vec::new();
    collect_json_files(root.as_ref(), &mut files)?;
    files.sort();

    let mut fixtures = Vec::with_capacity(files.len());
    for file in files {
        let body = fs::read_to_string(&file)
            .map_err(|source| Error::io("memory_benchmark_fixture_read", source))?;
        let fixture = serde_json::from_str::<MemoryBenchmarkFixture>(&body).map_err(|source| {
            Error::Other {
                source: Box::new(source),
                stage: "memory_benchmark_fixture_parse",
            }
        })?;
        fixtures.push(fixture);
    }
    Ok(fixtures)
}

pub fn run_memory_benchmark_wall(fixtures: &[MemoryBenchmarkFixture]) -> MemoryBenchmarkReport {
    let class_coverage = MemoryBenchmarkClass::ALL
        .iter()
        .copied()
        .map(|class| MemoryBenchmarkClassCoverage {
            class,
            compact_fixtures: fixtures
                .iter()
                .filter(|fixture| {
                    fixture.class == class && fixture.mode == MemoryBenchmarkMode::Compact
                })
                .count(),
            full_fixtures: fixtures
                .iter()
                .filter(|fixture| {
                    fixture.class == class && fixture.mode == MemoryBenchmarkMode::Full
                })
                .count(),
        })
        .collect::<Vec<_>>();

    let semantic_coverage = MemoryBenchmarkSemanticDimension::ALL
        .iter()
        .copied()
        .map(|dimension| MemoryBenchmarkSemanticCoverage {
            dimension,
            fixture_count: fixtures
                .iter()
                .filter(|fixture| fixture.semantic_contract.dimensions.contains(&dimension))
                .count(),
        })
        .collect::<Vec<_>>();

    let mut missing_classes = Vec::new();
    for coverage in &class_coverage {
        if coverage.compact_fixtures == 0 {
            missing_classes.push(MemoryBenchmarkMissingClass {
                class: coverage.class,
                mode: MemoryBenchmarkMode::Compact,
            });
        }
        if coverage.full_fixtures == 0 {
            missing_classes.push(MemoryBenchmarkMissingClass {
                class: coverage.class,
                mode: MemoryBenchmarkMode::Full,
            });
        }
    }

    let mut semantic_failures = fixtures
        .iter()
        .flat_map(validate_memory_benchmark_semantics)
        .collect::<Vec<_>>();
    for coverage in &semantic_coverage {
        if coverage.fixture_count == 0 {
            semantic_failures.push(memory_benchmark_suite_semantic_failure(
                Some(coverage.dimension),
                "semantic_dimension_coverage",
                "expected at least one fixture covering semantic dimension",
            ));
        }
    }

    let failures = fixtures
        .iter()
        .flat_map(validate_memory_benchmark_fixture)
        .collect::<Vec<_>>();
    let mut failed_fixture_ids = failures
        .iter()
        .map(|failure| failure.fixture_id.as_str())
        .collect::<BTreeSet<_>>();
    failed_fixture_ids.extend(
        semantic_failures
            .iter()
            .filter(|failure| failure.fixture_id != "__suite__")
            .map(|failure| failure.fixture_id.as_str()),
    );
    let failed_fixture_count = failed_fixture_ids.len();
    let soul_kernel_judge = build_soul_kernel_benchmark_judge(fixtures);
    let subject_projection_judge = build_subject_projection_benchmark_judge(fixtures);
    let agent_tool_experience_judge = build_agent_tool_experience_benchmark_judge(fixtures);
    let w4_eval_recall_judge = build_w4_eval_recall_benchmark_judge(fixtures);
    let passed = failures.is_empty()
        && semantic_failures.is_empty()
        && soul_kernel_judge.release_gate_passed
        && subject_projection_judge.release_gate_passed
        && agent_tool_experience_judge.release_gate_passed
        && w4_eval_recall_judge.release_gate_passed;

    MemoryBenchmarkReport {
        suite: "memory_benchmark_wall".to_string(),
        total_fixtures: fixtures.len(),
        passed_fixtures: fixtures.len().saturating_sub(failed_fixture_count),
        baseline: calculate_memory_benchmark_baseline(fixtures),
        class_coverage,
        missing_classes,
        semantic_coverage,
        soul_kernel_judge,
        subject_projection_judge,
        agent_tool_experience_judge,
        w4_eval_recall_judge,
        failures,
        semantic_failures,
        passed,
    }
    .with_missing_class_gate()
}

fn build_soul_kernel_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> SoulKernelBenchmarkJudgeReport {
    let soul_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture.class == MemoryBenchmarkClass::SoulRegression)
        .collect::<Vec<_>>();
    let fixture_ids = soul_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let growth_proposal_contract_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_growth_proposal",
        "SoulGrowthProposal",
    );
    let regression_suite_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_regression_suite",
        "SoulRegressionSuite",
    );
    let feedback_report_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_feedback_report",
        "SoulFeedbackReport",
    );
    let compact_digest_covered =
        any_fixture_has_key_or_surface(&soul_fixtures, "soul_compact_digest", "SoulCompactDigest");
    let no_roleplay_gate_passed = any_fixture_has_key(&soul_fixtures, "roleplay_prompt_rejected")
        && !any_fixture_has_marker(&soul_fixtures, "append persona prompt")
        && !any_fixture_has_marker(&soul_fixtures, "just pretend to be");
    let life_slot_gate_passed = any_fixture_has_key(&soul_fixtures, "soul_life_facets")
        && any_fixture_has_key(&soul_fixtures, "self_owned_update_candidates");
    let work_integrity_gate_passed = any_fixture_has_key(&soul_fixtures, "work_integrity_covenant")
        || any_fixture_has_surface(&soul_fixtures, "Work Integrity Covenant");
    let privacy_zero_gate_passed = fixtures.iter().all(|fixture| {
        fixture.metrics.privacy_violation_count <= fixture.thresholds.max_privacy_violation_count
            && fixture.metrics.soul_regression_count <= fixture.thresholds.max_soul_regression_count
    });

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        growth_proposal_contract_covered,
        "soul_growth_proposal_contract_missing",
    );
    push_missing(
        &mut blocked_reasons,
        regression_suite_covered,
        "soul_regression_suite_missing",
    );
    push_missing(
        &mut blocked_reasons,
        feedback_report_covered,
        "soul_feedback_report_missing",
    );
    push_missing(
        &mut blocked_reasons,
        compact_digest_covered,
        "soul_compact_digest_missing",
    );
    push_missing(
        &mut blocked_reasons,
        no_roleplay_gate_passed,
        "no_roleplay_host_mount_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        life_slot_gate_passed,
        "soul_life_slot_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        work_integrity_gate_passed,
        "work_integrity_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        privacy_zero_gate_passed,
        "soul_privacy_zero_gate_failed",
    );

    SoulKernelBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        growth_proposal_contract_covered,
        regression_suite_covered,
        feedback_report_covered,
        compact_digest_covered,
        no_roleplay_gate_passed,
        life_slot_gate_passed,
        work_integrity_gate_passed,
        privacy_zero_gate_passed,
        blocked_reasons,
    }
}

fn build_agent_tool_experience_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> AgentToolExperienceBenchmarkJudgeReport {
    let tool_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture.class == MemoryBenchmarkClass::AgentToolExperience)
        .collect::<Vec<_>>();
    let fixture_ids = tool_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let no_experience_empty_hints_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_no_experience_empty_hints")
            && any_fixture_has_marker(&tool_fixtures, "no_governed_tool_experience");
    let governed_experience_hint_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_governed_hint")
            && any_fixture_has_key(&tool_fixtures, "agent_tool_hints");
    let schema_drift_rejection_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_registry_fingerprint_mismatch")
            || any_fixture_has_key(&tool_fixtures, "agent_tool_experience_stale_schema");
    let private_observation_not_public_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_private_observation_excluded")
            && any_fixture_has_marker(&tool_fixtures, "private observation not projected");
    let gateway_no_cold_route_covered =
        any_fixture_has_key(&tool_fixtures, "gateway_host_tools_no_cold_route")
            && any_fixture_has_marker(&tool_fixtures, "host fallback required");
    let compact_registry_forbidden_covered = tool_fixtures.iter().any(|fixture| {
        fixture.mode == MemoryBenchmarkMode::Compact
            && any_fixture_has_key(&[*fixture], "agent_tool_registry_forbidden_by_profile")
    });
    let host_execution_boundary_covered =
        any_fixture_has_key(&tool_fixtures, "host_execution_required")
            && any_fixture_has_surface(&tool_fixtures, "HostToolRegistry");

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        no_experience_empty_hints_covered,
        "agent_tool_no_experience_empty_hints_missing",
    );
    push_missing(
        &mut blocked_reasons,
        governed_experience_hint_covered,
        "agent_tool_governed_hint_missing",
    );
    push_missing(
        &mut blocked_reasons,
        schema_drift_rejection_covered,
        "agent_tool_schema_drift_rejection_missing",
    );
    push_missing(
        &mut blocked_reasons,
        private_observation_not_public_covered,
        "agent_tool_private_observation_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        gateway_no_cold_route_covered,
        "gateway_host_tools_no_cold_route_missing",
    );
    push_missing(
        &mut blocked_reasons,
        compact_registry_forbidden_covered,
        "agent_tool_compact_registry_forbidden_missing",
    );
    push_missing(
        &mut blocked_reasons,
        host_execution_boundary_covered,
        "agent_tool_host_execution_boundary_missing",
    );

    AgentToolExperienceBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        no_experience_empty_hints_covered,
        governed_experience_hint_covered,
        schema_drift_rejection_covered,
        private_observation_not_public_covered,
        gateway_no_cold_route_covered,
        compact_registry_forbidden_covered,
        host_execution_boundary_covered,
        blocked_reasons,
    }
}

fn build_w4_eval_recall_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> W4EvalRecallBenchmarkJudgeReport {
    let w4_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture_declares_w4_eval_recall(fixture))
        .collect::<Vec<_>>();
    let fixture_ids = w4_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let fixture_count = w4_fixtures.len();
    let required_k = [5_usize, 10, 20, 50];
    let required_k_covered = !w4_fixtures.is_empty()
        && w4_fixtures.iter().all(|fixture| {
            fixture.eval_recall.as_ref().is_some_and(|eval| {
                required_k
                    .iter()
                    .all(|k| eval.metrics.recall_at_k.iter().any(|entry| entry.k == *k))
            })
        });
    let missing_evidence_reported = !w4_fixtures.is_empty()
        && w4_fixtures
            .iter()
            .all(|fixture| w4_missing_evidence_contract_holds(fixture));
    let source_expanded_selected_split_covered = w4_fixtures.iter().any(|fixture| {
        fixture
            .eval_recall
            .as_ref()
            .is_some_and(w4_eval_has_report_split)
    });
    let w4_1_diagnostic_schema_covered = !w4_fixtures.is_empty()
        && w4_fixtures.iter().all(|fixture| {
            fixture
                .eval_recall
                .as_ref()
                .is_some_and(w4_1_diagnostic_contract_holds)
        });
    let w4_1_candidate_pool_split_covered = !w4_fixtures.is_empty()
        && w4_fixtures.iter().all(|fixture| {
            fixture
                .eval_recall
                .as_ref()
                .is_some_and(w4_1_candidate_pool_split_holds)
        });
    let mrr_covered = !w4_fixtures.is_empty()
        && w4_fixtures.iter().all(|fixture| {
            fixture.eval_recall.as_ref().is_some_and(|eval| {
                eval.metrics.mrr_bps > 0
                    || eval.metrics.recall_at_k.iter().all(|entry| {
                        !entry.any_evidence_hit && entry.matched_evidence_refs.is_empty()
                    })
            })
        });
    let noisy_external_wall_required = true;

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        fixture_count > 0,
        "w4_eval_recall_fixture_missing",
    );
    push_missing(
        &mut blocked_reasons,
        required_k_covered,
        "w4_eval_recall_required_k_missing",
    );
    push_missing(
        &mut blocked_reasons,
        missing_evidence_reported,
        "w4_eval_recall_missing_evidence_report_missing",
    );
    push_missing(
        &mut blocked_reasons,
        source_expanded_selected_split_covered,
        "w4_eval_recall_report_split_missing",
    );
    push_missing(
        &mut blocked_reasons,
        w4_1_diagnostic_schema_covered,
        "w4_eval_recall_w4_1_diagnostics_missing",
    );
    push_missing(
        &mut blocked_reasons,
        w4_1_candidate_pool_split_covered,
        "w4_eval_recall_candidate_pool_split_missing",
    );
    push_missing(
        &mut blocked_reasons,
        mrr_covered,
        "w4_eval_recall_mrr_missing",
    );
    blocked_reasons.sort();
    blocked_reasons.dedup();

    W4EvalRecallBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        fixture_count,
        required_k_covered,
        missing_evidence_reported,
        source_expanded_selected_split_covered,
        w4_1_diagnostic_schema_covered,
        w4_1_candidate_pool_split_covered,
        mrr_covered,
        noisy_external_wall_required,
        blocked_reasons,
    }
}

fn fixture_declares_w4_eval_recall(fixture: &MemoryBenchmarkFixture) -> bool {
    fixture
        .semantic_contract
        .provided_keys
        .iter()
        .chain(fixture.semantic_contract.required_keys.iter())
        .any(|key| key == "w4_eval_recall")
        || fixture.eval_recall.is_some()
}

fn w4_eval_has_report_split(eval: &MemoryBenchmarkEvalRecall) -> bool {
    !eval.source_candidates.is_empty()
        && !eval.expanded_candidates.is_empty()
        && !eval.selected_candidates.is_empty()
        && eval.expanded_candidates.iter().any(|candidate| {
            !eval
                .source_candidates
                .iter()
                .any(|source| source == candidate)
        })
}

fn w4_1_candidate_pool_split_holds(eval: &MemoryBenchmarkEvalRecall) -> bool {
    !eval.graph_anchor_candidates.is_empty()
        && !eval.eval_candidate_pool.is_empty()
        && !eval.rendered_candidates.is_empty()
        && eval.eval_candidate_pool.iter().all(|candidate| {
            eval.source_candidates
                .iter()
                .any(|source| source == candidate)
                || eval
                    .graph_anchor_candidates
                    .iter()
                    .any(|anchor| anchor == candidate)
                || eval
                    .expanded_candidates
                    .iter()
                    .any(|expanded| expanded == candidate)
                || eval
                    .selected_candidates
                    .iter()
                    .any(|selected| selected == candidate)
        })
        && eval.eval_candidate_pool.iter().any(|candidate| {
            !eval
                .rendered_candidates
                .iter()
                .any(|rendered| rendered == candidate)
        })
}

fn w4_1_diagnostic_contract_holds(eval: &MemoryBenchmarkEvalRecall) -> bool {
    let expected = &eval.expected_evidence_refs;
    let diagnostics = &eval.diagnostics;
    let rendered_refs_match = !eval.rendered_evidence_refs.is_empty()
        && eval.rendered_evidence_refs.iter().all(|evidence_ref| {
            eval.evidence_ref_index.iter().any(|entry| {
                eval.rendered_candidates
                    .iter()
                    .any(|candidate| candidate == &entry.candidate_id)
                    && entry
                        .evidence_refs
                        .iter()
                        .any(|indexed_ref| indexed_ref == evidence_ref)
            })
        });
    diagnostics.evidence_count == expected.len()
        && !expected.is_empty()
        && !diagnostics.first_any_hit_stage.trim().is_empty()
        && !diagnostics.first_all_hit_stage.trim().is_empty()
        && stage_evidence_refs_cover(&diagnostics.matched_gold_by_stage, "expanded", expected)
        && diagnostics
            .missing_gold_by_stage
            .iter()
            .any(|stage| !stage.stage.trim().is_empty())
        && expected.iter().all(|evidence_ref| {
            diagnostics.gold_rank_by_stage.iter().any(|rank| {
                rank.evidence_ref == *evidence_ref
                    && !rank.stage.trim().is_empty()
                    && rank.rank.is_some()
            })
        })
        && !diagnostics.source_anchor_ids.is_empty()
        && !diagnostics.graph_anchor_candidate_ids.is_empty()
        && !diagnostics.expanded_node_ids.is_empty()
        && !diagnostics.graph_neighbor_ids.is_empty()
        && expected.iter().all(|evidence_ref| {
            diagnostics.graph_distance_to_gold.iter().any(|distance| {
                distance.evidence_ref == *evidence_ref && distance.distance.is_some()
            })
        })
        && rendered_refs_match
}

fn stage_evidence_refs_cover(
    stages: &[MemoryBenchmarkEvalRecallStageEvidenceRefs],
    stage: &str,
    expected_evidence_refs: &[String],
) -> bool {
    stages.iter().any(|entry| {
        entry.stage == stage
            && expected_evidence_refs
                .iter()
                .all(|expected| entry.evidence_refs.iter().any(|actual| actual == expected))
    })
}

fn w4_missing_evidence_contract_holds(fixture: &MemoryBenchmarkFixture) -> bool {
    let Some(eval) = fixture.eval_recall.as_ref() else {
        return false;
    };
    let matched = eval
        .metrics
        .recall_at_k
        .iter()
        .flat_map(|entry| entry.matched_evidence_refs.iter())
        .collect::<BTreeSet<_>>();
    let unmatched_expected = eval
        .expected_evidence_refs
        .iter()
        .filter(|expected| {
            !matched
                .iter()
                .any(|actual| actual.as_str() == expected.as_str())
        })
        .collect::<Vec<_>>();
    unmatched_expected.is_empty() || !eval.missing_evidence_refs.is_empty()
}

fn build_subject_projection_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> SubjectProjectionBenchmarkJudgeReport {
    let projection_fixtures = fixtures
        .iter()
        .filter(|fixture| {
            fixture.class == MemoryBenchmarkClass::SubjectProjection
                || fixture.class == MemoryBenchmarkClass::PrivacyRefusal
        })
        .collect::<Vec<_>>();
    let fixture_ids = projection_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let projection_report_covered =
        any_fixture_has_key_or_surface(
            &projection_fixtures,
            "subject_projection_report",
            "SubjectProjectionReport",
        ) || any_fixture_has_surface(&projection_fixtures, "MemoryProjectionReport");
    let budget_compiler_covered = any_fixture_has_key(&projection_fixtures, "budget_decisions")
        || any_fixture_has_key(&projection_fixtures, "projection_budget_compiler")
        || any_fixture_has_surface(&projection_fixtures, "ProjectionBudgetCompiler");
    let faithfulness_gate_passed =
        projection_fixtures.iter().all(|fixture| {
            fixture.metrics.projection_faithfulness_bps
                >= fixture.thresholds.min_projection_faithfulness_bps
        }) && (any_fixture_has_key(&projection_fixtures, "projection_faithfulness_check")
            || any_fixture_has_surface(&projection_fixtures, "ProjectionFaithfulnessCheck"));
    let private_disclosure_integrity_gate_passed =
        any_fixture_has_key(&projection_fixtures, "private_disclosure_integrity_report")
            || any_fixture_has_key(&projection_fixtures, "private_raw_absent")
            || any_fixture_has_surface(&projection_fixtures, "PrivateDisclosureIntegrityReport");
    let gateway_raw_audit_redaction_covered =
        any_fixture_has_key(&projection_fixtures, "gateway_raw_audit_redacted")
            || any_fixture_has_key(&projection_fixtures, "redacted_private_envelope")
            || any_fixture_has_surface(&projection_fixtures, "RawProjectionAudit");
    let raw_audit_disabled_reason_covered =
        any_fixture_has_key(&projection_fixtures, "raw_audit_disabled_reason")
            || any_fixture_has_marker(
                &projection_fixtures,
                "raw audit unavailable reason when disabled",
            )
            || any_fixture_has_marker(&projection_fixtures, "raw_projection_recording_disabled");
    let cross_surface_consistency_passed =
        any_fixture_has_key(&projection_fixtures, "cross_surface_consistency")
            && all_projection_fixtures_have_required_surface_context(&projection_fixtures);
    let benchmark_judge_attached = projection_fixtures.iter().any(|fixture| {
        matches!(
            fixture.evaluation_source,
            MemoryBenchmarkEvaluationSource::RuntimeReplay
                | MemoryBenchmarkEvaluationSource::GoldenJudge
        )
    });

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        projection_report_covered,
        "subject_projection_report_missing",
    );
    push_missing(
        &mut blocked_reasons,
        budget_compiler_covered,
        "projection_budget_compiler_missing",
    );
    push_missing(
        &mut blocked_reasons,
        faithfulness_gate_passed,
        "projection_faithfulness_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        private_disclosure_integrity_gate_passed,
        "private_disclosure_integrity_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        gateway_raw_audit_redaction_covered,
        "gateway_raw_audit_redaction_missing",
    );
    push_missing(
        &mut blocked_reasons,
        raw_audit_disabled_reason_covered,
        "raw_audit_disabled_reason_missing",
    );
    push_missing(
        &mut blocked_reasons,
        cross_surface_consistency_passed,
        "cross_surface_consistency_missing",
    );
    push_missing(
        &mut blocked_reasons,
        benchmark_judge_attached,
        "runtime_or_golden_benchmark_judge_missing",
    );

    SubjectProjectionBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        projection_report_covered,
        budget_compiler_covered,
        faithfulness_gate_passed,
        private_disclosure_integrity_gate_passed,
        gateway_raw_audit_redaction_covered,
        raw_audit_disabled_reason_covered,
        cross_surface_consistency_passed,
        benchmark_judge_attached,
        blocked_reasons,
    }
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(root).map_err(|source| Error::io("memory_benchmark_fixture_list", source))?
    {
        let entry =
            entry.map_err(|source| Error::io("memory_benchmark_fixture_list_entry", source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(())
}

fn validate_memory_benchmark_fixture(
    fixture: &MemoryBenchmarkFixture,
) -> Vec<MemoryBenchmarkFailure> {
    let mut failures = Vec::new();
    push_min_bps_failure(
        &mut failures,
        fixture,
        "accuracy_bps",
        fixture.metrics.accuracy_bps,
        fixture.thresholds.min_accuracy_bps,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "evidence_precision_bps",
        fixture.metrics.evidence_precision_bps,
        fixture.thresholds.min_evidence_precision_bps,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "projection_faithfulness_bps",
        fixture.metrics.projection_faithfulness_bps,
        fixture.thresholds.min_projection_faithfulness_bps,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "privacy_violation_count",
        fixture.metrics.privacy_violation_count,
        fixture.thresholds.max_privacy_violation_count,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "stale_memory_false_positive_count",
        fixture.metrics.stale_memory_false_positive_count,
        fixture.thresholds.max_stale_memory_false_positive_count,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "procedural_reuse_success_bps",
        fixture.metrics.procedural_reuse_success_bps,
        fixture.thresholds.min_procedural_reuse_success_bps,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "soul_regression_count",
        fixture.metrics.soul_regression_count,
        fixture.thresholds.max_soul_regression_count,
    );
    if let Some(max_latency_ms) = fixture.thresholds.max_latency_ms {
        push_max_u32_failure(
            &mut failures,
            fixture,
            "latency_ms",
            fixture.metrics.latency_ms,
            max_latency_ms,
        );
    }
    if let Some(max_token_budget) = fixture.thresholds.max_token_budget {
        push_max_u32_failure(
            &mut failures,
            fixture,
            "token_budget",
            fixture.metrics.token_budget,
            max_token_budget,
        );
    }
    if let Some(max_memory_bytes) = fixture.thresholds.max_memory_bytes {
        if fixture.metrics.memory_bytes > max_memory_bytes {
            failures.push(memory_benchmark_failure(
                fixture,
                "memory_bytes",
                format!(
                    "expected at most {}, got {}",
                    max_memory_bytes, fixture.metrics.memory_bytes
                ),
            ));
        }
    }
    failures.extend(validate_w4_eval_recall_fixture(fixture));
    failures
}

fn validate_w4_eval_recall_fixture(
    fixture: &MemoryBenchmarkFixture,
) -> Vec<MemoryBenchmarkFailure> {
    if !fixture_declares_w4_eval_recall(fixture) {
        return Vec::new();
    }

    let Some(eval) = fixture.eval_recall.as_ref() else {
        return vec![memory_benchmark_failure(
            fixture,
            "w4_eval_recall_contract",
            "w4_eval_recall fixture is declared but eval_recall payload is missing",
        )];
    };

    let mut missing = Vec::new();
    push_w4_missing(&mut missing, !eval.suite.trim().is_empty(), "suite");
    push_w4_missing(&mut missing, !eval.split.trim().is_empty(), "split");
    push_w4_missing(
        &mut missing,
        !eval.question_id.trim().is_empty(),
        "question_id",
    );
    push_w4_missing(
        &mut missing,
        !eval.question_type.trim().is_empty(),
        "question_type",
    );
    push_w4_missing(
        &mut missing,
        !eval.expected_evidence_refs.is_empty(),
        "expected_evidence_refs",
    );
    push_w4_missing(
        &mut missing,
        !eval.source_candidates.is_empty(),
        "source_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.graph_anchor_candidates.is_empty(),
        "graph_anchor_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.expanded_candidates.is_empty(),
        "expanded_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.eval_candidate_pool.is_empty(),
        "eval_candidate_pool",
    );
    push_w4_missing(
        &mut missing,
        !eval.selected_candidates.is_empty(),
        "selected_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.rendered_candidates.is_empty(),
        "rendered_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.rendered_block_preview.trim().is_empty(),
        "rendered_block_preview",
    );
    push_w4_missing(
        &mut missing,
        !eval.rendered_evidence_refs.is_empty(),
        "rendered_evidence_refs",
    );
    push_w4_missing(
        &mut missing,
        !eval.evidence_ref_index.is_empty(),
        "evidence_ref_index",
    );
    let required_k = [5_usize, 10, 20, 50];
    for k in required_k {
        push_w4_missing(
            &mut missing,
            eval.metrics.recall_at_k.iter().any(|entry| entry.k == k),
            format!("recall_at_k:{k}"),
        );
    }
    push_w4_missing(
        &mut missing,
        w4_missing_evidence_contract_holds(fixture),
        "missing_evidence_refs",
    );
    push_w4_missing(
        &mut missing,
        eval.metrics.mrr_bps > 0
            || eval
                .metrics
                .recall_at_k
                .iter()
                .all(|entry| !entry.any_evidence_hit && entry.matched_evidence_refs.is_empty()),
        "mrr_bps",
    );
    push_w4_missing(
        &mut missing,
        w4_1_candidate_pool_split_holds(eval),
        "w4_1_candidate_pool_split",
    );
    push_w4_missing(
        &mut missing,
        w4_1_diagnostic_contract_holds(eval),
        "w4_1_diagnostics",
    );

    if missing.is_empty() {
        Vec::new()
    } else {
        vec![memory_benchmark_failure(
            fixture,
            "w4_eval_recall_contract",
            format!("missing or invalid {}", missing.join(", ")),
        )]
    }
}

fn push_w4_missing(missing: &mut Vec<String>, condition: bool, field: impl Into<String>) {
    if !condition {
        missing.push(field.into());
    }
}

fn validate_memory_benchmark_semantics(
    fixture: &MemoryBenchmarkFixture,
) -> Vec<MemoryBenchmarkSemanticFailure> {
    let contract = &fixture.semantic_contract;
    if contract.is_empty() {
        return Vec::new();
    }

    let mut failures = Vec::new();
    if contract.dimensions.is_empty() {
        failures.push(memory_benchmark_semantic_failure(
            fixture,
            None,
            "semantic_dimension",
            "semantic contract must declare at least one gate dimension",
        ));
    }

    for required_key in &contract.required_keys {
        if !contract.provided_keys.contains(required_key) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_required_key",
                format!("missing required key {required_key}"),
            ));
        }
    }
    for forbidden_key in &contract.forbidden_keys {
        if contract.provided_keys.contains(forbidden_key) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_forbidden_key",
                format!("forbidden key {forbidden_key} is present"),
            ));
        }
    }
    for required_marker in &contract.required_markers {
        if !contract.observed_markers.contains(required_marker) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_required_marker",
                format!("missing required marker {required_marker}"),
            ));
        }
    }
    for forbidden_marker in &contract.forbidden_markers {
        if contract.observed_markers.contains(forbidden_marker) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_forbidden_marker",
                format!("forbidden marker {forbidden_marker} is present"),
            ));
        }
    }

    failures
}

fn any_fixture_has_key(fixtures: &[&MemoryBenchmarkFixture], key: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .semantic_contract
            .provided_keys
            .iter()
            .chain(fixture.semantic_contract.required_keys.iter())
            .any(|candidate| candidate == key)
    })
}

fn any_fixture_has_surface(fixtures: &[&MemoryBenchmarkFixture], surface: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .scenario
            .expected_surfaces
            .iter()
            .any(|candidate| candidate == surface)
    })
}

fn any_fixture_has_key_or_surface(
    fixtures: &[&MemoryBenchmarkFixture],
    key: &str,
    surface: &str,
) -> bool {
    any_fixture_has_key(fixtures, key) || any_fixture_has_surface(fixtures, surface)
}

fn any_fixture_has_marker(fixtures: &[&MemoryBenchmarkFixture], marker: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .semantic_contract
            .observed_markers
            .iter()
            .chain(fixture.semantic_contract.required_markers.iter())
            .any(|candidate| candidate == marker)
    })
}

fn all_projection_fixtures_have_required_surface_context(
    fixtures: &[&MemoryBenchmarkFixture],
) -> bool {
    fixtures.iter().all(|fixture| {
        !fixture.scenario.expected_surfaces.is_empty()
            && !fixture.scenario.evidence_refs.is_empty()
            && fixture.metrics.projection_faithfulness_bps
                >= fixture.thresholds.min_projection_faithfulness_bps
            && fixture.metrics.privacy_violation_count
                <= fixture.thresholds.max_privacy_violation_count
    })
}

fn push_missing(blocked_reasons: &mut Vec<String>, condition: bool, reason: &str) {
    if !condition {
        blocked_reasons.push(reason.to_string());
    }
}

fn calculate_memory_benchmark_baseline(
    fixtures: &[MemoryBenchmarkFixture],
) -> MemoryBenchmarkBaseline {
    if fixtures.is_empty() {
        return MemoryBenchmarkBaseline::default();
    }
    let len = fixtures.len() as u32;
    MemoryBenchmarkBaseline {
        accuracy_bps: average_bps(fixtures.iter().map(|fixture| fixture.metrics.accuracy_bps)),
        evidence_precision_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.evidence_precision_bps),
        ),
        projection_faithfulness_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.projection_faithfulness_bps),
        ),
        privacy_violation_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.privacy_violation_count)
            .sum(),
        stale_memory_false_positive_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.stale_memory_false_positive_count)
            .sum(),
        procedural_reuse_success_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.procedural_reuse_success_bps),
        ),
        soul_regression_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.soul_regression_count)
            .sum(),
        latency_ms: fixtures
            .iter()
            .map(|fixture| fixture.metrics.latency_ms)
            .sum::<u32>()
            / len,
        token_budget: fixtures
            .iter()
            .map(|fixture| fixture.metrics.token_budget)
            .sum::<u32>()
            / len,
        memory_bytes: fixtures
            .iter()
            .map(|fixture| fixture.metrics.memory_bytes)
            .sum::<u64>()
            / fixtures.len() as u64,
    }
}

fn average_bps(values: impl Iterator<Item = u16>) -> u16 {
    let mut total = 0u32;
    let mut count = 0u32;
    for value in values {
        total += u32::from(value);
        count += 1;
    }
    if count == 0 {
        return 0;
    }
    (total / count) as u16
}

fn push_min_bps_failure(
    failures: &mut Vec<MemoryBenchmarkFailure>,
    fixture: &MemoryBenchmarkFixture,
    stage: &str,
    got: u16,
    min: u16,
) {
    if got < min {
        failures.push(memory_benchmark_failure(
            fixture,
            stage,
            format!("expected at least {min}, got {got}"),
        ));
    }
}

fn push_max_u32_failure(
    failures: &mut Vec<MemoryBenchmarkFailure>,
    fixture: &MemoryBenchmarkFixture,
    stage: &str,
    got: u32,
    max: u32,
) {
    if got > max {
        failures.push(memory_benchmark_failure(
            fixture,
            stage,
            format!("expected at most {max}, got {got}"),
        ));
    }
}

fn memory_benchmark_failure(
    fixture: &MemoryBenchmarkFixture,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkFailure {
    MemoryBenchmarkFailure {
        fixture_id: fixture.fixture_id.clone(),
        class: fixture.class,
        mode: fixture.mode,
        profile: fixture.profile,
        stage: stage.into(),
        reason: reason.into(),
    }
}

fn memory_benchmark_semantic_failure(
    fixture: &MemoryBenchmarkFixture,
    dimension: Option<MemoryBenchmarkSemanticDimension>,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkSemanticFailure {
    MemoryBenchmarkSemanticFailure {
        fixture_id: fixture.fixture_id.clone(),
        dimension,
        stage: stage.into(),
        reason: reason.into(),
    }
}

fn memory_benchmark_suite_semantic_failure(
    dimension: Option<MemoryBenchmarkSemanticDimension>,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkSemanticFailure {
    MemoryBenchmarkSemanticFailure {
        fixture_id: "__suite__".to_string(),
        dimension,
        stage: stage.into(),
        reason: reason.into(),
    }
}

impl MemoryBenchmarkReport {
    fn with_missing_class_gate(mut self) -> Self {
        if !self.missing_classes.is_empty() {
            self.passed = false;
        }
        self
    }
}
