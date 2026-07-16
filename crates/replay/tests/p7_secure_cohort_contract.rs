#[cfg(unix)]
use bm_replay::open_p7_cohort_artifact_owner;
use bm_replay::{initialize_p7_cohort, P7ArtifactPublishOutcome};
#[cfg(unix)]
use std::io::Read;
use std::{fs, io::Write, path::PathBuf};

fn fixture_root(label: &str) -> PathBuf {
    let canonical_temp = fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
    let root = canonical_temp.join(format!("bm-p7-secure-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir(&root).expect("fixture root");
    root
}

#[test]
fn cohort_initialization_is_exclusive_and_handle_scoped() {
    let root = fixture_root("exclusive");
    let cohort = initialize_p7_cohort(&root, "run-1").expect("create cohort once");
    assert_eq!(cohort.path(), root.join("results/runs/run-1"));
    assert!(cohort.path().is_dir());
    assert!(initialize_p7_cohort(&root, "run-1").is_err());

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn cohort_initialization_rejects_symlink_before_outside_write() {
    use std::os::unix::fs::symlink;

    let root = fixture_root("symlink");
    let outside = fixture_root("outside");
    symlink(&outside, root.join("results")).expect("symlinked results owner");

    assert!(initialize_p7_cohort(&root, "run-1").is_err());
    assert!(
        fs::read_dir(&outside)
            .expect("outside directory")
            .next()
            .is_none(),
        "unsafe cohort initialization wrote through a symlink"
    );

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[cfg(unix)]
#[test]
fn retained_attempt_owner_survives_path_replacement_without_writing_outside() {
    use std::os::unix::fs::symlink;

    let root = fixture_root("retained-owner");
    let cohort = initialize_p7_cohort(&root, "run-1").expect("create cohort");
    let logs = cohort
        .open_or_create_directory("logs")
        .expect("open logs owner");
    let attempt = logs
        .create_directory("attempt-1")
        .expect("create attempt owner");
    let original_path = root.join("results/runs/run-1/logs/attempt-1");
    let retained_path = root.join("results/runs/run-1/logs/attempt-retained");
    let outside = root.join("outside");
    fs::create_dir(&outside).expect("outside directory");
    fs::rename(&original_path, &retained_path).expect("move retained attempt directory");
    symlink(&outside, &original_path).expect("replace attempt path with symlink");

    let mut artifact = attempt
        .create_new_file("shard.log")
        .expect("descriptor-relative artifact");
    artifact
        .write_all(b"retained-owner\n")
        .expect("write artifact");
    artifact.sync_all().expect("sync artifact");

    assert_eq!(
        fs::read(retained_path.join("shard.log")).expect("retained owner artifact"),
        b"retained-owner\n"
    );
    assert!(
        !outside.join("shard.log").exists(),
        "path replacement redirected a handle-owned artifact"
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn cohort_file_creation_rejects_preexisting_leaf_symlink() {
    use std::os::unix::fs::symlink;

    let root = fixture_root("file-symlink");
    let cohort = initialize_p7_cohort(&root, "run-1").expect("create cohort");
    let outside = root.join("outside");
    fs::write(&outside, b"outside").expect("outside fixture");
    symlink(&outside, cohort.path().join("evidence.json")).expect("symlinked evidence path");

    assert!(cohort.create_new_file("evidence.json").is_err());
    assert_eq!(fs::read(&outside).expect("outside preserved"), b"outside");
    let mut file = cohort
        .create_new_file("fresh.json")
        .expect("exclusive regular file");
    file.write_all(b"fresh").expect("write secure file");
    drop(file);
    assert!(cohort.create_new_file("fresh.json").is_err());
    let mut reopened = open_p7_cohort_artifact_owner(&root, "run-1")
        .expect("reopen cohort owner")
        .open_existing_file("fresh.json")
        .expect("open secure existing file");
    let mut body = String::new();
    reopened
        .read_to_string(&mut body)
        .expect("read secure file");
    assert_eq!(body, "fresh");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn staged_artifact_publish_is_atomic_no_clobber_and_resumable() {
    let root = fixture_root("staged-publish");
    let cohort = initialize_p7_cohort(&root, "run-1").expect("create cohort");

    assert!(cohort
        .publish_immutable_bytes(b"invalid\n", "invalid.tmp", "../escaped.json")
        .is_err());
    assert!(!cohort.path().join("invalid.tmp").exists());

    let mut staged = cohort
        .create_new_file("artifact.json.tmp-1")
        .expect("create staged artifact");
    staged.write_all(b"governed\n").expect("write staged");
    assert_eq!(
        cohort
            .publish_staged_file(staged, "artifact.json.tmp-1", "artifact.json")
            .expect("publish staged artifact"),
        P7ArtifactPublishOutcome::Published
    );
    assert!(!cohort.path().join("artifact.json.tmp-1").exists());
    assert_eq!(
        fs::read(cohort.path().join("artifact.json")).expect("published artifact"),
        b"governed\n"
    );

    let mut identical = cohort
        .create_new_file("artifact.json.tmp-2")
        .expect("create identical staged artifact");
    identical
        .write_all(b"governed\n")
        .expect("write identical staged");
    assert_eq!(
        cohort
            .publish_staged_file(identical, "artifact.json.tmp-2", "artifact.json")
            .expect("reuse identical artifact"),
        P7ArtifactPublishOutcome::ReusedIdentical
    );
    assert!(!cohort.path().join("artifact.json.tmp-2").exists());

    let mut different = cohort
        .create_new_file("artifact.json.tmp-3")
        .expect("create different staged artifact");
    different
        .write_all(b"different\n")
        .expect("write different staged");
    let error = cohort
        .publish_staged_file(different, "artifact.json.tmp-3", "artifact.json")
        .expect_err("different immutable artifact must fail closed");
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert!(!cohort.path().join("artifact.json.tmp-3").exists());
    assert_eq!(
        fs::read(cohort.path().join("artifact.json")).expect("original preserved"),
        b"governed\n"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn bundle_writer_lock_is_exclusive_and_fails_fast() {
    let root = fixture_root("bundle-lock");
    let cohort = initialize_p7_cohort(&root, "run-1").expect("create cohort");
    let first = cohort
        .lock_bundle("locomo.shard-0-of-1.lock")
        .expect("first bundle writer lock");
    let error = match cohort.lock_bundle("locomo.shard-0-of-1.lock") {
        Ok(_) => panic!("concurrent bundle writer must fail fast"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    drop(first);
    cohort
        .lock_bundle("locomo.shard-0-of-1.lock")
        .expect("bundle lock is released with retained guard");

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn staged_publish_uses_the_retained_owner_after_path_replacement() {
    use std::os::unix::fs::symlink;

    let root = fixture_root("retained-publish");
    let cohort = initialize_p7_cohort(&root, "run-1").expect("create cohort");
    let original_path = cohort.path().to_path_buf();
    let retained_path = root.join("results/runs/run-retained");
    let outside = root.join("outside");
    fs::create_dir(&outside).expect("outside directory");
    let mut staged = cohort
        .create_new_file("report.json.tmp")
        .expect("create retained staged artifact");
    staged.write_all(b"retained\n").expect("write staged");
    fs::rename(&original_path, &retained_path).expect("move retained cohort");
    symlink(&outside, &original_path).expect("replace cohort path");

    cohort
        .publish_staged_file(staged, "report.json.tmp", "report.json")
        .expect("publish through retained owner");
    assert_eq!(
        fs::read(retained_path.join("report.json")).expect("retained publication"),
        b"retained\n"
    );
    assert!(!outside.join("report.json").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn windows_secure_fs_source_uses_handle_relative_no_reparse_primitives() {
    let source =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/p7_secure_fs.rs"))
            .expect("read secure FS owner");
    let windows = source
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
    assert!(
        !windows.contains("OpenOptions"),
        "Windows cohort artifacts must not fall back to path-based OpenOptions"
    );
}

#[cfg(windows)]
#[test]
fn windows_cohort_owner_creates_nested_no_clobber_artifacts() {
    let root = fixture_root("windows-owner");
    let cohort = initialize_p7_cohort(&root, "run-1").expect("create Windows cohort");
    let logs = cohort
        .open_or_create_directory("logs")
        .expect("open Windows logs owner");
    let attempt = logs
        .create_directory("attempt-1")
        .expect("create Windows attempt owner");
    let mut file = attempt
        .create_new_file("shard.log")
        .expect("create Windows artifact");
    file.write_all(b"windows-handle-owner\n")
        .expect("write Windows artifact");
    file.sync_all().expect("sync Windows artifact");
    assert!(attempt.create_new_file("shard.log").is_err());

    let _ = fs::remove_dir_all(root);
}
