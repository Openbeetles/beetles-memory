//! Atomic, no-replace P8 quality artifact bundles.
//!
//! Runner and operator schemas remain separate, but both publish through this one physical
//! transaction owner. A final bundle appears only after its exact file set and bytes have been
//! re-read through retained handles.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::retained_artifact_fs::RetainedArtifactDirectory;

use super::execution_plan::P8QualityExecutionPlanV1;
use super::{
    deserialize_p8_quality_artifact, reject_p8_quality_raw_sentinels, P8QualityArtifactBundleRef,
    P8QualityContractFailure, P8QualityDigest, P8QualityRunRef,
};

const MANIFEST_FILE_NAME: &str = "bundle-manifest.json";
const BUNDLE_SCHEMA: &str = "beetle-memory.p8.quality-artifact-bundle.v1";
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8QualityBundleKindV1 {
    RunnerShard,
    RunnerCohort,
    OperatorReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityBundleFileV1 {
    file_name: String,
    byte_length: u64,
    content_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityArtifactBundleManifestV1 {
    schema: String,
    run_id: P8QualityRunRef,
    kind: P8QualityBundleKindV1,
    files: Vec<P8QualityBundleFileV1>,
    bundle_digest: P8QualityArtifactBundleRef,
}

impl P8QualityArtifactBundleManifestV1 {
    pub(super) fn run_id(&self) -> &P8QualityRunRef {
        &self.run_id
    }

    pub(super) const fn kind(&self) -> P8QualityBundleKindV1 {
        self.kind
    }

    fn build(
        run_id: P8QualityRunRef,
        kind: P8QualityBundleKindV1,
        files: &BTreeMap<String, Vec<u8>>,
    ) -> io::Result<Self> {
        validate_input_files(files)?;
        let mut value = Self {
            schema: BUNDLE_SCHEMA.into(),
            run_id,
            kind,
            files: files
                .iter()
                .map(|(file_name, bytes)| {
                    Ok(P8QualityBundleFileV1 {
                        file_name: file_name.clone(),
                        byte_length: u64::try_from(bytes.len())
                            .map_err(|_| invalid_data("quality artifact length overflow"))?,
                        content_digest: content_digest(bytes),
                    })
                })
                .collect::<io::Result<Vec<_>>>()?,
            bundle_digest: P8QualityArtifactBundleRef::derive(&()),
        };
        value.bundle_digest = value.derived_digest();
        value.validate_contract().map_err(contract_error)?;
        Ok(value)
    }

    fn validate_contract(&self) -> Result<(), P8QualityContractFailure> {
        if self.schema != BUNDLE_SCHEMA {
            return Err(P8QualityContractFailure::SchemaMismatch);
        }
        let names = self
            .files
            .iter()
            .map(|entry| entry.file_name.as_str())
            .collect::<Vec<_>>();
        if names.is_empty()
            || !names.windows(2).all(|window| window[0] < window[1])
            || names.contains(&MANIFEST_FILE_NAME)
            || self.files.iter().any(|entry| {
                entry.byte_length == 0
                    || entry.byte_length > MAX_FILE_BYTES
                    || !is_one_component(&entry.file_name)
            })
        {
            return Err(P8QualityContractFailure::ArtifactSetMismatch);
        }
        if self.bundle_digest != self.derived_digest() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn derived_digest(&self) -> P8QualityArtifactBundleRef {
        P8QualityArtifactBundleRef::derive(&(&self.schema, &self.run_id, self.kind, &self.files))
    }

    fn exact_file_names(&self) -> BTreeSet<String> {
        self.files
            .iter()
            .map(|entry| entry.file_name.clone())
            .chain([MANIFEST_FILE_NAME.to_string()])
            .collect()
    }
}

pub(crate) fn publish_quality_bundle_no_replace(
    root: &RetainedArtifactDirectory,
    stage_name: &str,
    final_name: &str,
    run_id: P8QualityRunRef,
    kind: P8QualityBundleKindV1,
    files: BTreeMap<String, Vec<u8>>,
) -> io::Result<P8QualityArtifactBundleManifestV1> {
    let manifest = P8QualityArtifactBundleManifestV1::build(run_id, kind, &files)?;
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|_| invalid_data("quality bundle manifest serialization failed"))?;
    reject_p8_quality_raw_sentinels(&manifest_bytes)
        .map_err(|_| invalid_data("quality bundle manifest contains raw material"))?;

    root.verify_subdirectory_absent(final_name)?;
    let mut stage = root.create_new_subdirectory(stage_name)?;
    let prepared = (|| {
        for (file_name, bytes) in &files {
            validate_artifact_json(file_name, bytes)?;
            write_new_file(&stage, file_name, bytes)?;
        }
        write_new_file(&stage, MANIFEST_FILE_NAME, &manifest_bytes)?;
        verify_quality_bundle(&stage, &manifest)?;
        root.install_directory_no_replace_terminal(
            &mut stage,
            stage_name,
            final_name,
            |retained| verify_quality_bundle(retained, &manifest),
        )?;
        verify_quality_bundle(&stage, &manifest)
    })();

    if let Err(error) = prepared {
        if stage.path().file_name().and_then(|name| name.to_str()) == Some(stage_name) {
            discard_stage(root, &stage, stage_name)?;
        }
        return Err(error);
    }
    Ok(manifest)
}

pub(crate) fn publish_quality_runner_bundle_no_replace(
    root: &RetainedArtifactDirectory,
    stage_name: &str,
    final_name: &str,
    execution_plan: &P8QualityExecutionPlanV1,
    files: BTreeMap<String, Vec<u8>>,
) -> io::Result<P8QualityArtifactBundleManifestV1> {
    let observed = files.keys().cloned().collect::<BTreeSet<_>>();
    let expected = execution_plan
        .exact_artifact_names()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if observed != expected {
        return Err(invalid_input(
            "runner artifact files differ from the execution plan exact set",
        ));
    }
    publish_quality_bundle_no_replace(
        root,
        stage_name,
        final_name,
        execution_plan.run_id().clone(),
        P8QualityBundleKindV1::RunnerCohort,
        files,
    )
}

pub(crate) fn open_verified_quality_bundle(
    root: &RetainedArtifactDirectory,
    final_name: &str,
) -> io::Result<(P8QualityArtifactBundleManifestV1, BTreeMap<String, Vec<u8>>)> {
    let directory = root.open_existing_subdirectory(final_name)?;
    let manifest_bytes = read_bounded(
        directory.open_existing_read_stable_file(MANIFEST_FILE_NAME)?,
        MAX_FILE_BYTES,
    )?;
    reject_p8_quality_raw_sentinels(&manifest_bytes)
        .map_err(|_| invalid_data("quality bundle manifest contains raw material"))?;
    let manifest: P8QualityArtifactBundleManifestV1 =
        deserialize_p8_quality_artifact(&manifest_bytes)
            .map_err(|_| invalid_data("quality bundle manifest is not strict JSON"))?;
    verify_quality_bundle(&directory, &manifest)?;
    let mut files = BTreeMap::new();
    for entry in &manifest.files {
        let bytes = read_bounded(
            directory.open_existing_read_stable_file(&entry.file_name)?,
            MAX_FILE_BYTES,
        )?;
        validate_artifact_json(&entry.file_name, &bytes)?;
        files.insert(entry.file_name.clone(), bytes);
    }
    Ok((manifest, files))
}

pub(crate) fn open_verified_quality_runner_bundle(
    root: &RetainedArtifactDirectory,
    final_name: &str,
    execution_plan: &P8QualityExecutionPlanV1,
) -> io::Result<(P8QualityArtifactBundleManifestV1, BTreeMap<String, Vec<u8>>)> {
    let (manifest, files) = open_verified_quality_bundle(root, final_name)?;
    if manifest.kind != P8QualityBundleKindV1::RunnerCohort
        || &manifest.run_id != execution_plan.run_id()
        || files.keys().cloned().collect::<BTreeSet<_>>()
            != execution_plan
                .exact_artifact_names()
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
    {
        return Err(invalid_data(
            "retained runner bundle differs from the execution plan",
        ));
    }
    Ok((manifest, files))
}

fn verify_quality_bundle(
    directory: &RetainedArtifactDirectory,
    manifest: &P8QualityArtifactBundleManifestV1,
) -> io::Result<()> {
    manifest.validate_contract().map_err(contract_error)?;
    if directory.exact_regular_file_names()? != manifest.exact_file_names() {
        return Err(invalid_data(
            "quality bundle file set differs from its manifest",
        ));
    }
    let manifest_bytes = read_bounded(
        directory.open_existing_read_stable_file(MANIFEST_FILE_NAME)?,
        MAX_FILE_BYTES,
    )?;
    let observed_manifest: P8QualityArtifactBundleManifestV1 =
        deserialize_p8_quality_artifact(&manifest_bytes)
            .map_err(|_| invalid_data("quality bundle manifest is invalid"))?;
    if &observed_manifest != manifest {
        return Err(invalid_data("quality bundle manifest bytes drifted"));
    }
    let mut total = u64::try_from(manifest_bytes.len())
        .map_err(|_| invalid_data("quality bundle size overflow"))?;
    for entry in &manifest.files {
        let bytes = read_bounded(
            directory.open_existing_read_stable_file(&entry.file_name)?,
            MAX_FILE_BYTES,
        )?;
        total = total
            .checked_add(
                u64::try_from(bytes.len())
                    .map_err(|_| invalid_data("quality bundle size overflow"))?,
            )
            .ok_or_else(|| invalid_data("quality bundle size overflow"))?;
        if total > MAX_BUNDLE_BYTES
            || u64::try_from(bytes.len()).ok() != Some(entry.byte_length)
            || content_digest(&bytes) != entry.content_digest
        {
            return Err(invalid_data(
                "quality bundle content differs from its manifest",
            ));
        }
        validate_artifact_json(&entry.file_name, &bytes)?;
    }
    directory.sync_exact_regular_files()?;
    directory.verify_unchanged()
}

fn validate_input_files(files: &BTreeMap<String, Vec<u8>>) -> io::Result<()> {
    if files.is_empty()
        || files.contains_key(MANIFEST_FILE_NAME)
        || files.keys().any(|name| !is_one_component(name))
    {
        return Err(invalid_input("quality artifact file set is invalid"));
    }
    let mut total = 0_u64;
    for (file_name, bytes) in files {
        let length = u64::try_from(bytes.len())
            .map_err(|_| invalid_data("quality artifact length overflow"))?;
        if length == 0 || length > MAX_FILE_BYTES {
            return Err(invalid_data("quality artifact length is invalid"));
        }
        total = total
            .checked_add(length)
            .ok_or_else(|| invalid_data("quality bundle size overflow"))?;
        if total > MAX_BUNDLE_BYTES {
            return Err(invalid_data("quality bundle exceeds its byte limit"));
        }
        validate_artifact_json(file_name, bytes)?;
    }
    Ok(())
}

fn validate_artifact_json(file_name: &str, bytes: &[u8]) -> io::Result<()> {
    if file_name.ends_with(".jsonl") {
        if !bytes.ends_with(b"\n") {
            return Err(invalid_data("quality JSONL must end at a record boundary"));
        }
        for line in bytes.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            reject_p8_quality_raw_sentinels(line)
                .map_err(|_| invalid_data("quality JSONL contains forbidden raw material"))?;
            deserialize_p8_quality_artifact::<serde_json::Value>(line)
                .map_err(|_| invalid_data("quality artifact has an invalid strict JSONL record"))?;
        }
    } else if file_name.ends_with(".json") {
        reject_p8_quality_raw_sentinels(bytes)
            .map_err(|_| invalid_data("quality artifact contains forbidden raw material"))?;
        deserialize_p8_quality_artifact::<serde_json::Value>(bytes)
            .map_err(|_| invalid_data("quality artifact is not strict JSON"))?;
    } else {
        return Err(invalid_data("quality artifact extension is not admitted"));
    }
    Ok(())
}

fn write_new_file(
    directory: &RetainedArtifactDirectory,
    file_name: &str,
    bytes: &[u8],
) -> io::Result<()> {
    let mut file = directory.create_new_terminal_stage(file_name)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn read_bounded(mut file: File, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(
            limit
                .checked_add(1)
                .ok_or_else(|| invalid_data("quality artifact read limit overflow"))?,
        )
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(invalid_data("quality artifact exceeds its read limit"));
    }
    Ok(bytes)
}

fn discard_stage(
    root: &RetainedArtifactDirectory,
    stage: &RetainedArtifactDirectory,
    stage_name: &str,
) -> io::Result<()> {
    for name in stage.exact_regular_file_names()? {
        let file = stage.open_existing_terminal_stage(&name)?;
        stage.discard_same_file(&file, &name)?;
    }
    root.discard_empty_same_directory(stage, stage_name)
}

fn content_digest(bytes: &[u8]) -> P8QualityDigest {
    P8QualityDigest::derive("p8_quality_artifact_file_bytes_v1", &bytes)
}

fn is_one_component(value: &str) -> bool {
    let mut components = std::path::Path::new(value).components();
    !value.is_empty()
        && matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn contract_error(failure: P8QualityContractFailure) -> io::Error {
    invalid_data(format!("quality artifact contract failed: {failure:?}"))
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp")
            .join(format!(
                "bm-p8-quality-{label}-{}-{nonce}",
                std::process::id()
            ))
    }

    #[test]
    fn quality_bundle_is_atomic_no_replace_and_independently_reopened() {
        let path = fixture_root("bundle");
        fs::create_dir(&path).expect("root");
        let root = RetainedArtifactDirectory::open_root(&path).expect("retained root");
        let run_id = P8QualityRunRef::derive(&"run");
        let files = BTreeMap::from([(
            "shard-00000.json".to_string(),
            serde_json::to_vec(&serde_json::json!({"trial":"closed"})).expect("json"),
        )]);
        publish_quality_bundle_no_replace(
            &root,
            "first.stage",
            "run-shard-00000",
            run_id.clone(),
            P8QualityBundleKindV1::RunnerShard,
            files.clone(),
        )
        .expect("first publication");
        let (_, observed) =
            open_verified_quality_bundle(&root, "run-shard-00000").expect("reopen bundle");
        assert_eq!(observed, files);
        assert!(publish_quality_bundle_no_replace(
            &root,
            "second.stage",
            "run-shard-00000",
            run_id,
            P8QualityBundleKindV1::RunnerShard,
            BTreeMap::from([(
                "shard-00000.json".to_string(),
                br#"{"trial":"replacement"}"#.to_vec(),
            )]),
        )
        .is_err());
        assert!(!path.join("second.stage").exists());
        drop(root);
        fs::remove_dir_all(path).expect("cleanup");
    }

    #[test]
    fn quality_bundle_rejects_duplicate_json_keys_and_raw_sentinels_before_stage() {
        let path = fixture_root("raw");
        fs::create_dir(&path).expect("root");
        let root = RetainedArtifactDirectory::open_root(&path).expect("retained root");
        for bytes in [
            br#"{"trial":1,"trial":2}"#.to_vec(),
            br#"{"trial":"raw-soul-sentinel"}"#.to_vec(),
        ] {
            assert!(publish_quality_bundle_no_replace(
                &root,
                "rejected.stage",
                "rejected",
                P8QualityRunRef::derive(&"run"),
                P8QualityBundleKindV1::RunnerShard,
                BTreeMap::from([("shard.json".to_string(), bytes)]),
            )
            .is_err());
            assert!(!path.join("rejected.stage").exists());
        }
        drop(root);
        fs::remove_dir_all(path).expect("cleanup");
    }

    #[test]
    fn runner_bundle_requires_plan_exact_json_and_jsonl_set() {
        let dataset = super::super::execution_plan::admit_zero_origin_tiny_dataset_manifest(
            include_bytes!("../../fixtures/p8-quality-tiny/manifest.json"),
        )
        .expect("tiny dataset");
        let experiment = super::super::tests::fixture_plan_for_zero_origin_dataset(
            super::super::P8QualityPurpose::BaselineEstablishment,
            &dataset,
        );
        let generation =
            super::super::execution_plan::P8SupervisorOwnedRunGeneration::mint_for_supervisor(
                P8QualityDigest::derive("p8_bundle_test_supervisor_v1", &"session"),
                1,
                [23; 32],
            )
            .expect("generation");
        let execution_plan = P8QualityExecutionPlanV1::derive(&experiment, &dataset, generation)
            .expect("execution plan");
        let files = execution_plan
            .exact_artifact_names()
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    if name.ends_with(".jsonl") {
                        b"{}\n".to_vec()
                    } else {
                        b"{}".to_vec()
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let path = fixture_root("runner-exact");
        fs::create_dir(&path).expect("root");
        let root = RetainedArtifactDirectory::open_root(&path).expect("retained root");

        let mut missing = files.clone();
        missing.remove("cohort-manifest.json");
        assert!(publish_quality_runner_bundle_no_replace(
            &root,
            "missing.stage",
            "missing",
            &execution_plan,
            missing,
        )
        .is_err());
        assert!(!path.join("missing.stage").exists());

        publish_quality_runner_bundle_no_replace(
            &root,
            "runner.stage",
            "runner-cohort",
            &execution_plan,
            files.clone(),
        )
        .expect("publish exact runner bundle");
        let (_, observed) =
            open_verified_quality_runner_bundle(&root, "runner-cohort", &execution_plan)
                .expect("reopen exact runner bundle");
        assert_eq!(observed, files);
        drop(root);
        fs::remove_dir_all(path).expect("cleanup");
    }
}
