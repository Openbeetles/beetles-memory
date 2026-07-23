use std::{fs, path::PathBuf};

fn source(relative: &str) -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative))
        .expect("read P7 secure filesystem source")
}

#[test]
fn raw_cohort_write_owner_is_not_part_of_the_public_replay_surface() {
    let library = source("src/lib.rs");
    let secure_fs = source("src/p7_secure_fs.rs");

    assert!(secure_fs.contains("pub(crate) struct P7CohortArtifactOwner"));
    assert!(!library.contains("P7CohortArtifactOwner,"));
    assert!(!library.contains("initialize_p7_cohort,"));
    assert!(!library.contains("open_p7_cohort_artifact_owner,"));
    assert!(library.contains("P7AuthorityBoundArtifactTransaction"));
    assert!(library.contains("P7AuthorityBoundReleaseTransaction"));

    let retained_owner = secure_fs
        .split_once("impl P7RetainedDirectoryOwner {")
        .and_then(|(_, source)| source.split_once("\n}\n\n#[derive").map(|(owner, _)| owner))
        .expect("retained directory owner implementation");
    for forbidden in [
        "pub fn open_or_create_directory",
        "pub fn create_directory",
        "pub fn create_new_file",
        "pub fn lock_bundle",
    ] {
        assert!(
            !retained_owner.contains(forbidden),
            "raw retained owner exposes {forbidden}"
        );
    }
    assert!(!secure_fs.contains("pub fn copy_execution_to"));
}

#[test]
fn authority_bound_transactions_own_each_final_write_boundary() {
    let secure_fs = source("src/p7_secure_fs.rs");

    for required in [
        "pub fn create_staged_file",
        "P7 bundle cleanup requires the matching cohort bundle lock",
        "verify_same_directory(&bundle_guard.directory)",
        "verify_external_write_authority",
        "publish_staged_file_with_authority",
        "install_staged_directory",
        "discard_staged_file",
        "discard_empty_directory",
    ] {
        assert!(
            secure_fs.contains(required),
            "authority-bound transaction is missing {required}"
        );
    }

    let artifact_transaction = secure_fs
        .split_once("impl<'authority> P7AuthorityBoundArtifactTransaction<'authority> {")
        .and_then(|(_, source)| {
            source
                .split_once("\n}\n\nimpl<'authority> P7AuthorityBoundReleaseTransaction")
                .map(|(transaction, _)| transaction)
        })
        .expect("authority-bound artifact transaction implementation");
    assert!(!artifact_transaction.contains("pub fn create_new_file"));
    assert!(!artifact_transaction.contains("discard_uncommitted_file"));
}

#[test]
fn windows_secure_fs_source_uses_handle_relative_no_reparse_primitives() {
    let secure_fs = source("src/p7_secure_fs.rs");
    let windows = secure_fs
        .split_once("#[cfg(windows)]\nmod platform")
        .map(|(_, source)| source)
        .expect("Windows secure FS module");

    for required in [
        "NtCreateFile",
        "RootDirectory",
        "OBJ_DONT_REPARSE",
        "FILE_OPEN_REPARSE_POINT",
        "FILE_CREATE",
        "GetFileInformationByHandleEx",
        "FileIdInfo",
        "VolumeSerialNumber",
        "SetFileInformationByHandle",
        "FileRenameInfo",
        "FILE_SHARE_READ",
        "LOCKFILE_FAIL_IMMEDIATELY",
    ] {
        assert!(
            windows.contains(required),
            "Windows secure FS is missing {required}"
        );
    }
    assert!(!windows.contains("OpenOptions"));

    let public_read = windows
        .split_once("pub(super) fn open_existing_file")
        .and_then(|(_, source)| {
            source
                .split_once("pub(super) fn open_existing_deletable_file")
                .map(|(body, _)| body)
        })
        .expect("Windows read-only retained file open");
    assert!(!public_read.contains("DELETE"));
    let cleanup_read = windows
        .split_once("pub(super) fn open_existing_deletable_file")
        .and_then(|(_, source)| {
            source
                .split_once("pub(super) fn open_existing_executable")
                .map(|(body, _)| body)
        })
        .expect("Windows cleanup-only retained file open");
    assert!(cleanup_read.contains("FILE_GENERIC_READ | DELETE | SYNCHRONIZE"));
}
