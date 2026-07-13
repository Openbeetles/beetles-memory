use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

const SDK_BUILD_FINGERPRINT_CONTRACT: &str = "p7_sdk_build_inputs_sha256_v2";

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("bm-replay must remain under <repo>/crates/replay")?;
    let sdk_inputs = fingerprint_inputs(
        repo_root,
        &[
            "Cargo.toml",
            "Cargo.lock",
            "crates/core/Cargo.toml",
            "crates/core/src",
            "crates/sdk/Cargo.toml",
            "crates/sdk/src",
        ],
    )?;
    for file in &sdk_inputs {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    println!(
        "cargo:rustc-env=BM_P7_TRUSTED_SDK_BUILD_FINGERPRINT={}",
        fingerprint_files(repo_root, &sdk_inputs)?
    );
    Ok(())
}

fn fingerprint_inputs(root: &Path, relatives: &[&str]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    for relative in relatives {
        collect_regular_files(&root.join(relative), &mut files)?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_regular_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let child = entry.path();
        if child.is_dir() {
            collect_regular_files(&child, files)?;
        } else if child.is_file() {
            files.push(child);
        }
    }
    Ok(())
}

fn fingerprint_files(root: &Path, files: &[PathBuf]) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hash_fingerprint_field(&mut hasher, SDK_BUILD_FINGERPRINT_CONTRACT.as_bytes())?;
    hasher.update(u64::try_from(files.len())?.to_le_bytes());
    for file in files {
        let relative = file.strip_prefix(root)?;
        hash_fingerprint_field(&mut hasher, relative.to_string_lossy().as_bytes())?;
        hash_fingerprint_field(&mut hasher, &fs::read(file)?)?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_fingerprint_field(hasher: &mut Sha256, value: &[u8]) -> Result<(), Box<dyn Error>> {
    hasher.update(u64::try_from(value.len())?.to_le_bytes());
    hasher.update(value);
    Ok(())
}
