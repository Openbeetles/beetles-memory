//! Content-addressed P8 harness release staging publisher.
//!
//! The caller must already hold the sealed SourcePublisher execution authority. The child may
//! commit an immutable release directory with one atomic no-replace rename, but it can only return
//! a draft. The parent-owned trusted supervisor receipt must attest process exit and pipe closure
//! before any final publication authority exists.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Write},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    retained_artifact_fs::RetainedArtifactDirectory,
    sealed_execution::{RetainedExecutable, PEER_CHANNEL_MAX_MESSAGE_BYTES},
};

use super::{
    deserialize_p8_quality_artifact, domain_separated_sha256, has_typed_sha256_prefix,
    source_release::{P8HarnessExecutableRoleV1, P8HarnessReleaseManifestV1, P8HarnessReleaseRef},
    trusted_execution::P8QualityExecutionAuthority,
    P8QualityDigest,
};

const P8_HARNESS_PUBLICATION_DRAFT_SCHEMA: &str =
    "beetle-memory.p8.quality-harness-publication-draft.v1";
const MANIFEST_FILE_NAME: &str = "harness-release.json";

use super::trusted_execution::publication::P8AdmittedPublisherCommit;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct P8HarnessPublicationDraftRef(String);

impl P8HarnessPublicationDraftRef {
    fn derive(value: &impl Serialize) -> Self {
        let bytes = serde_json::to_vec(value)
            .expect("P8 harness publication draft serialization must be infallible");
        Self(format!(
            "p8_harness_publication_draft:sha256:{}",
            domain_separated_sha256(
                "p8_quality_harness_publication_draft_v1",
                &[bytes.as_slice()]
            )
        ))
    }
}

impl<'de> Deserialize<'de> for P8HarnessPublicationDraftRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = String::deserialize(deserializer)?;
        if has_typed_sha256_prefix(&value, "p8_harness_publication_draft:sha256:") {
            Ok(Self(value))
        } else {
            Err(D::Error::custom(
                "invalid P8 harness publication draft identity",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8HarnessPublicationDraftStateV1 {
    CommittedAwaitingParentClosure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HarnessPublicationDraftV1 {
    schema: String,
    draft_state: P8HarnessPublicationDraftStateV1,
    harness_release_digest: P8HarnessReleaseRef,
    manifest_digest: P8QualityDigest,
    publisher_executable_digest: P8QualityDigest,
    exact_file_names: Vec<String>,
    release_directory_device: u64,
    release_directory_inode: u64,
    draft_digest: P8HarnessPublicationDraftRef,
}

impl P8HarnessPublicationDraftV1 {
    pub(crate) fn validate_against(&self, release: &P8HarnessReleaseManifestV1) -> io::Result<()> {
        let manifest_bytes = serde_json::to_vec(release)
            .map_err(|_| invalid_data("P8 harness release serialization failed"))?;
        let expected_manifest_digest =
            P8QualityDigest::derive("p8_harness_release_manifest_bytes_v1", &manifest_bytes);
        if self.schema != P8_HARNESS_PUBLICATION_DRAFT_SCHEMA
            || self.draft_state != P8HarnessPublicationDraftStateV1::CommittedAwaitingParentClosure
            || &self.harness_release_digest != release.release_digest()
            || self.manifest_digest != expected_manifest_digest
            || release.role_executable_digest(P8HarnessExecutableRoleV1::SourcePublisher)
                != Some(&self.publisher_executable_digest)
            || self.exact_file_names != expected_file_names()
            || self.draft_digest != self.derived_digest()
        {
            return Err(invalid_data("P8 harness publication draft is invalid"));
        }
        Ok(())
    }

    fn derived_digest(&self) -> P8HarnessPublicationDraftRef {
        P8HarnessPublicationDraftRef::derive(&(
            &self.schema,
            self.draft_state,
            &self.harness_release_digest,
            &self.manifest_digest,
            &self.publisher_executable_digest,
            &self.exact_file_names,
            self.release_directory_device,
            self.release_directory_inode,
        ))
    }

    pub(crate) fn draft_digest(&self) -> &P8HarnessPublicationDraftRef {
        &self.draft_digest
    }

    pub(crate) fn release_directory_identity(&self) -> (u64, u64) {
        (self.release_directory_device, self.release_directory_inode)
    }
}

pub(crate) struct P8PreparedHarnessReleaseStage<'a> {
    root: &'a RetainedArtifactDirectory,
    release: &'a P8HarnessReleaseManifestV1,
    stage: RetainedArtifactDirectory,
    stage_name: String,
    manifest_digest: P8QualityDigest,
    publisher_digest: P8QualityDigest,
}

impl P8PreparedHarnessReleaseStage<'_> {
    pub(crate) fn stage_name(&self) -> &str {
        &self.stage_name
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn stage_directory_file(&self) -> io::Result<File> {
        self.stage.try_clone_directory_file()
    }

    pub(crate) fn physical_identity(&self) -> io::Result<(u64, u64)> {
        self.stage.unix_physical_identity()
    }

    pub(crate) fn abort(mut self) -> io::Result<()> {
        if self.is_uncommitted_stage() {
            discard_stage(self.root, &self.stage, &self.stage_name)?;
            self.stage_name.clear();
        }
        Ok(())
    }

    fn is_uncommitted_stage(&self) -> bool {
        self.stage.path().file_name().and_then(|name| name.to_str())
            == Some(self.stage_name.as_str())
    }
}

impl Drop for P8PreparedHarnessReleaseStage<'_> {
    fn drop(&mut self) {
        if self.is_uncommitted_stage()
            && discard_stage(self.root, &self.stage, &self.stage_name).is_err()
        {
            // A pre-commit transaction may never silently leave an unknown staging tree.
            std::process::abort();
        }
    }
}

pub(crate) fn prepare_harness_release_stage<'a>(
    authority: &mut P8QualityExecutionAuthority,
    root: &'a RetainedArtifactDirectory,
    release: &'a P8HarnessReleaseManifestV1,
    mut role_executables: BTreeMap<P8HarnessExecutableRoleV1, RetainedExecutable>,
    stage_name: String,
) -> io::Result<P8PreparedHarnessReleaseStage<'a>> {
    authority.verify()?;
    if authority.role() != P8HarnessExecutableRoleV1::SourcePublisher {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "only the sealed P8 SourcePublisher may publish a harness release",
        ));
    }
    if !release.validate_contract().is_empty() || !release.has_exact_engineering_sealed_processes()
    {
        return Err(invalid_data(
            "P8 harness release lacks exact engineering sealed process evidence",
        ));
    }
    if role_executables
        .keys()
        .copied()
        .ne(P8HarnessExecutableRoleV1::ALL)
    {
        return Err(invalid_input(
            "P8 publisher role executable inputs are not exact",
        ));
    }
    for role in P8HarnessExecutableRoleV1::ALL {
        let expected = release
            .role_executable_digest(role)
            .ok_or_else(|| invalid_data("harness release role executable is missing"))?;
        let retained = role_executables
            .get_mut(&role)
            .ok_or_else(|| invalid_data("P8 retained publisher role is missing"))?;
        let identity = retained.copy_to_verified(&mut io::sink())?;
        let actual = P8QualityDigest::parse(format!("sha256:{}", identity.sha256()))
            .map_err(|_| invalid_data("role executable SHA256 is invalid"))?;
        if &actual != expected
            || (role == P8HarnessExecutableRoleV1::SourcePublisher
                && actual != authority.executable_digest())
        {
            return Err(invalid_data(
                "retained publisher role differs from harness release",
            ));
        }
    }
    let mut stage = Some(root.create_new_subdirectory(&stage_name)?);
    let prepared = (|| {
        let staged = stage
            .as_ref()
            .ok_or_else(|| invalid_data("P8 prepared stage was already consumed"))?;
        let manifest_bytes = serde_json::to_vec(release)
            .map_err(|_| invalid_data("P8 harness release serialization failed"))?;
        write_new_stage_file(staged, MANIFEST_FILE_NAME, &manifest_bytes)?;

        let publisher_digest = authority.executable_digest();
        if release.role_executable_digest(P8HarnessExecutableRoleV1::SourcePublisher)
            != Some(&publisher_digest)
        {
            return Err(invalid_data(
                "sealed publisher executable differs from harness release",
            ));
        }
        let mut publisher_file = staged.create_new_terminal_stage(
            P8HarnessExecutableRoleV1::SourcePublisher.executable_file_name(),
        )?;
        authority.copy_executable_to(&mut publisher_file)?;
        publisher_file.sync_all()?;

        for role in P8HarnessExecutableRoleV1::ALL {
            if role == P8HarnessExecutableRoleV1::SourcePublisher {
                continue;
            }
            let expected = release
                .role_executable_digest(role)
                .ok_or_else(|| invalid_data("harness release role executable is missing"))?;
            let retained = role_executables
                .get_mut(&role)
                .ok_or_else(|| invalid_data("retained role executable is missing"))?;
            let mut destination = staged.create_new_terminal_stage(role.executable_file_name())?;
            let identity = retained.copy_to_verified(&mut destination)?;
            destination.sync_all()?;
            let actual = P8QualityDigest::parse(format!("sha256:{}", identity.sha256()))
                .map_err(|_| invalid_data("role executable SHA256 is invalid"))?;
            if &actual != expected {
                return Err(invalid_data(
                    "role executable bytes differ from harness release",
                ));
            }
        }

        validate_staged_release(staged, release)?;
        staged.sync_exact_regular_files()?;
        validate_staged_release(staged, release)?;
        Ok(P8PreparedHarnessReleaseStage {
            root,
            release,
            stage: stage
                .take()
                .ok_or_else(|| invalid_data("P8 prepared stage disappeared"))?,
            stage_name: stage_name.clone(),
            manifest_digest: P8QualityDigest::derive(
                "p8_harness_release_manifest_bytes_v1",
                &manifest_bytes,
            ),
            publisher_digest,
        })
    })();
    if let Err(error) = prepared {
        if let Some(staged) = &stage {
            discard_stage(root, staged, &stage_name)?;
        }
        return Err(error);
    }
    prepared
}

pub(crate) fn commit_prepared_harness_release_no_replace(
    permit: P8AdmittedPublisherCommit,
    authority: &mut P8QualityExecutionAuthority,
    mut prepared: P8PreparedHarnessReleaseStage<'_>,
) -> io::Result<P8HarnessPublicationDraftV1> {
    let commit_result = (|| {
        permit.verify_live()?;
        authority.verify()?;
        let (stage_device, stage_inode) = prepared.stage.unix_physical_identity()?;
        let expected_release_binding = P8QualityDigest::derive(
            "p8_harness_publisher_commit_plan_binding_v1",
            prepared.release.release_digest(),
        );
        let expected_stage_binding =
            prepared_harness_stage_binding(prepared.release, stage_device, stage_inode);
        if permit.release_binding() != &expected_release_binding
            || permit.stage_binding() != &expected_stage_binding
            || authority.role() != P8HarnessExecutableRoleV1::SourcePublisher
            || authority.executable_digest() != prepared.publisher_digest
        {
            return Err(invalid_input(
                "P8 publisher CommitPermit differs from the prepared stage",
            ));
        }
        validate_staged_release(&prepared.stage, prepared.release)?;
        prepared.root.install_directory_no_replace_terminal(
            &mut prepared.stage,
            &prepared.stage_name,
            prepared.release.content_address(),
            |staged| {
                validate_staged_release(staged, prepared.release)?;
                permit.verify_live()
            },
        )?;
        validate_staged_release(&prepared.stage, prepared.release)
    })();
    if let Err(error) = commit_result {
        if prepared.is_uncommitted_stage() {
            prepared.abort()?;
        }
        return Err(error);
    }
    let (device, inode) = prepared.stage.unix_physical_identity()?;
    let mut receipt = P8HarnessPublicationDraftV1 {
        schema: P8_HARNESS_PUBLICATION_DRAFT_SCHEMA.into(),
        draft_state: P8HarnessPublicationDraftStateV1::CommittedAwaitingParentClosure,
        harness_release_digest: prepared.release.release_digest().clone(),
        manifest_digest: prepared.manifest_digest.clone(),
        publisher_executable_digest: prepared.publisher_digest.clone(),
        exact_file_names: expected_file_names(),
        release_directory_device: device,
        release_directory_inode: inode,
        draft_digest: P8HarnessPublicationDraftRef::derive(&()),
    };
    receipt.draft_digest = receipt.derived_digest();
    receipt.validate_against(prepared.release)?;
    authority.verify()?;
    Ok(receipt)
}

pub(crate) fn verify_staged_harness_release(
    stage: &RetainedArtifactDirectory,
    release: &P8HarnessReleaseManifestV1,
) -> io::Result<(u64, u64)> {
    validate_staged_release(stage, release)?;
    stage.sync_exact_regular_files()?;
    validate_staged_release(stage, release)?;
    stage.unix_physical_identity()
}

pub(crate) fn prepared_harness_stage_binding(
    release: &P8HarnessReleaseManifestV1,
    device: u64,
    inode: u64,
) -> P8QualityDigest {
    P8QualityDigest::derive(
        "p8_harness_prepared_stage_binding_v1",
        &(
            release.release_digest(),
            expected_file_names(),
            P8HarnessExecutableRoleV1::ALL.map(|role| {
                (
                    role,
                    release
                        .role_executable_digest(role)
                        .expect("validated release has every role")
                        .clone(),
                )
            }),
            device,
            inode,
        ),
    )
}

pub(crate) fn verify_harness_publication_draft(
    root: &RetainedArtifactDirectory,
    release: &P8HarnessReleaseManifestV1,
    draft: &P8HarnessPublicationDraftV1,
) -> io::Result<()> {
    draft.validate_against(release)?;
    let (device, inode) = verify_committed_harness_release(root, release)?;
    if device != draft.release_directory_device || inode != draft.release_directory_inode {
        return Err(invalid_data(
            "committed harness release physical identity drifted",
        ));
    }
    Ok(())
}

pub(crate) fn verify_committed_harness_release(
    root: &RetainedArtifactDirectory,
    release: &P8HarnessReleaseManifestV1,
) -> io::Result<(u64, u64)> {
    let directory = root.open_existing_subdirectory(release.content_address())?;
    validate_staged_release(&directory, release)?;
    directory.unix_physical_identity()
}

pub(crate) fn verify_retained_harness_release(
    directory: &RetainedArtifactDirectory,
    release: &P8HarnessReleaseManifestV1,
) -> io::Result<(u64, u64)> {
    validate_staged_release(directory, release)?;
    directory.unix_physical_identity()
}

fn validate_staged_release(
    stage: &RetainedArtifactDirectory,
    release: &P8HarnessReleaseManifestV1,
) -> io::Result<()> {
    if stage
        .exact_regular_file_names()?
        .into_iter()
        .collect::<Vec<_>>()
        != expected_file_names()
    {
        return Err(invalid_data(
            "harness release directory file set is not exact",
        ));
    }
    let manifest = stage.open_existing_read_stable_file(MANIFEST_FILE_NAME)?;
    let manifest_bytes = read_bounded(
        manifest,
        u64::try_from(PEER_CHANNEL_MAX_MESSAGE_BYTES)
            .map_err(|_| invalid_data("peer channel byte limit is not representable"))?,
    )?;
    let observed: P8HarnessReleaseManifestV1 = deserialize_p8_quality_artifact(&manifest_bytes)
        .map_err(|_| invalid_data("published harness manifest is invalid"))?;
    if &observed != release || !observed.validate_contract().is_empty() {
        return Err(invalid_data(
            "published harness manifest differs from admitted release",
        ));
    }
    for role in P8HarnessExecutableRoleV1::ALL {
        let file_name = role.executable_file_name();
        let mut file = stage.open_existing_read_stable_file(file_name)?;
        if file.metadata()?.len() == 0 {
            return Err(invalid_data("published role executable is empty"));
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        stage.verify_file_identity(file_name, &file)?;
        let actual = P8QualityDigest::parse(format!("sha256:{:x}", hasher.finalize()))
            .map_err(|_| invalid_data("published role executable SHA256 is invalid"))?;
        if release.role_executable_digest(role) != Some(&actual) {
            return Err(invalid_data(
                "published role executable differs from harness release",
            ));
        }
    }
    Ok(())
}

fn write_new_stage_file(
    stage: &RetainedArtifactDirectory,
    file_name: &str,
    bytes: &[u8],
) -> io::Result<()> {
    let mut file = stage.create_new_terminal_stage(file_name)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn read_bounded(mut file: File, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(
            limit
                .checked_add(1)
                .ok_or_else(|| invalid_data("P8 publication read limit overflow"))?,
        )
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(invalid_data("P8 publication file exceeds its limit"));
    }
    Ok(bytes)
}

fn expected_file_names() -> Vec<String> {
    let mut names = P8HarnessExecutableRoleV1::ALL
        .into_iter()
        .map(|role| role.executable_file_name().to_string())
        .collect::<Vec<_>>();
    names.push(MANIFEST_FILE_NAME.to_string());
    names.sort();
    names
}

fn discard_stage(
    root: &RetainedArtifactDirectory,
    stage: &RetainedArtifactDirectory,
    stage_name: &str,
) -> io::Result<()> {
    let names = stage.exact_regular_file_names()?;
    for name in names {
        let file = stage.open_existing_terminal_stage(&name)?;
        stage.discard_same_file(&file, &name)?;
    }
    root.discard_empty_same_directory(stage, stage_name)
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_publication_file_set_has_one_manifest_and_four_distinct_roles() {
        let names = expected_file_names();
        assert_eq!(names.len(), 5);
        assert_eq!(
            names
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            names.len()
        );
        assert!(names.contains(&MANIFEST_FILE_NAME.to_string()));
        assert!(!names.iter().any(|name| name.contains("p7")));
    }
}
