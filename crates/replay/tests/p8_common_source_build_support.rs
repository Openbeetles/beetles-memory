#[path = "../build_support.rs"]
mod build_support;

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn fresh_temp_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bm-p8-common-source-{label}-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn common_source_materializer_reads_exact_regular_files_in_canonical_order() {
    let root = fresh_temp_root("regular");
    fs::create_dir(&root).expect("root");
    fs::create_dir(root.join("nested")).expect("nested");
    fs::write(root.join("z.rs"), b"z").expect("z");
    fs::write(root.join("nested.rs"), b"module").expect("module");
    fs::write(root.join("nested/a.rs"), b"a").expect("a");

    let mut files = Vec::new();
    build_support::collect_regular_files(&root, &mut files).expect("collect");
    build_support::sort_regular_files_relative_to(&root, &mut files).expect("sort");
    assert_eq!(
        files
            .iter()
            .map(|path| path.strip_prefix(&root).expect("relative"))
            .collect::<Vec<_>>(),
        vec![
            std::path::Path::new("nested.rs"),
            std::path::Path::new("nested/a.rs"),
            std::path::Path::new("z.rs")
        ]
    );
    assert_eq!(
        build_support::read_regular_file_stable(&files[0]).expect("read"),
        b"module"
    );

    fs::remove_dir_all(&root).expect("cleanup");
}

#[cfg(unix)]
#[test]
fn common_source_materializer_rejects_file_and_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let root = fresh_temp_root("symlink");
    fs::create_dir(&root).expect("root");
    fs::create_dir(root.join("real-dir")).expect("real dir");
    fs::write(root.join("real.rs"), b"real").expect("real file");
    symlink(root.join("real.rs"), root.join("file-link.rs")).expect("file symlink");
    symlink(root.join("real-dir"), root.join("dir-link")).expect("directory symlink");

    assert!(build_support::read_regular_file_stable(&root.join("file-link.rs")).is_err());
    let mut files = Vec::new();
    assert!(build_support::collect_regular_files(&root, &mut files).is_err());

    fs::remove_dir_all(&root).expect("cleanup");
}
