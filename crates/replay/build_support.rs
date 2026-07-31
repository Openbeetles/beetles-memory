use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

// This source is compiled independently by the build script, the library, and
// build-support integration tests; each compilation consumes a different
// subset of the shared declarations.
#[allow(dead_code)]
pub(crate) const P7_SDK_BUILD_FINGERPRINT_CONTRACT: &str = "p7_sdk_build_inputs_sha256_v2";
#[allow(dead_code)]
pub(crate) const P7_SDK_BUILD_INPUTS: [&str; 6] = [
    "Cargo.toml",
    "Cargo.lock",
    "crates/core/Cargo.toml",
    "crates/core/src",
    "crates/sdk/Cargo.toml",
    "crates/sdk/src",
];
#[allow(dead_code)]
pub(crate) const P7_OPERATOR_BUILD_FINGERPRINT_CONTRACT: &str =
    "p7_operator_build_inputs_sha256_v1";
#[allow(dead_code)]
pub(crate) const P7_OPERATOR_BUILD_INPUTS: [&str; 16] = [
    "Cargo.toml",
    "Cargo.lock",
    "crates/replay/Cargo.toml",
    "crates/replay/build.rs",
    "crates/replay/build_support.rs",
    "crates/replay/src/bench.rs",
    "crates/replay/src/fixture.rs",
    "crates/replay/src/harness.rs",
    "crates/replay/src/lib.rs",
    "crates/replay/src/p7_process.rs",
    "crates/replay/src/p7_secure_fs.rs",
    "crates/replay/src/retained_artifact_fs.rs",
    "crates/replay/src/runner.rs",
    "crates/replay/src/sealed_execution.rs",
    "crates/replay/src/bin/bm-p7-retained-launch.rs",
    "crates/replay/src/bin/bm-w4-external-noisy-wall.rs",
];

pub(crate) fn collect_regular_files(
    path: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(format!("source input must not be a symlink: {}", path.display()).into());
    }
    if metadata.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!(
            "source input must be a regular file or directory: {}",
            path.display()
        )
        .into());
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        collect_regular_files(&entry.path(), files)?;
    }
    Ok(())
}

pub(crate) fn sort_regular_files_relative_to(
    root: &Path,
    files: &mut [PathBuf],
) -> Result<(), Box<dyn Error>> {
    if files.iter().any(|file| file.strip_prefix(root).is_err()) {
        return Err(format!("source input escaped the retained root: {}", root.display()).into());
    }
    files.sort_by(|left, right| {
        left.strip_prefix(root)
            .expect("validated source input stays below root")
            .to_string_lossy()
            .cmp(
                &right
                    .strip_prefix(root)
                    .expect("validated source input stays below root")
                    .to_string_lossy(),
            )
    });
    Ok(())
}

pub(crate) fn read_regular_file_stable(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let before = fs::symlink_metadata(path)?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(format!(
            "source input is not a regular non-symlink file: {}",
            path.display()
        )
        .into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(path)?;
    let retained = file.metadata()?;
    if !retained.is_file() || !same_build_file_identity(&before, &retained) {
        return Err(format!(
            "source input identity changed while opening: {}",
            path.display()
        )
        .into());
    }
    let mut first = Vec::new();
    file.read_to_end(&mut first)?;
    file.seek(SeekFrom::Start(0))?;
    let mut second = Vec::new();
    file.read_to_end(&mut second)?;
    let after = fs::symlink_metadata(path)?;
    if first != second
        || after.file_type().is_symlink()
        || !after.is_file()
        || !same_build_file_identity(&retained, &after)
        || retained.len() != u64::try_from(first.len())?
        || after.len() != retained.len()
    {
        return Err(format!("source input changed while hashing: {}", path.display()).into());
    }
    Ok(first)
}

#[cfg(unix)]
fn same_build_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_build_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
        && left.created().ok() == right.created().ok()
}
