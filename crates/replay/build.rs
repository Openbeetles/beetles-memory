use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

const SDK_BUILD_FINGERPRINT_CONTRACT: &str = "p7_sdk_build_inputs_sha256_v2";
const OPERATOR_BUILD_FINGERPRINT_CONTRACT: &str = "p7_operator_build_inputs_sha256_v1";
const PACKAGED_BUILD_FINGERPRINT_CONTRACT: &str = "p7_packaged_unattested_inputs_sha256_v1";
const WORKSPACE_BUILD_SOURCE_ATTESTATION: &str = "workspace_source";
const PACKAGED_BUILD_SOURCE_ATTESTATION: &str = "packaged_unattested";
const WORKSPACE_OPERATOR_FROZEN_IDENTITY_RELATIVE_PATH: &str =
    "crates/replay/src/bin/bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs";
const CRATE_OPERATOR_FROZEN_IDENTITY_RELATIVE_PATH: &str =
    "src/bin/bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs";
const OPERATOR_BUILD_INPUTS: [&str; 13] = [
    "Cargo.toml",
    "Cargo.lock",
    "crates/replay/Cargo.toml",
    "crates/replay/build.rs",
    "crates/replay/src/bench.rs",
    "crates/replay/src/fixture.rs",
    "crates/replay/src/harness.rs",
    "crates/replay/src/lib.rs",
    "crates/replay/src/p7_process.rs",
    "crates/replay/src/p7_secure_fs.rs",
    "crates/replay/src/runner.rs",
    "crates/replay/src/bin/bm-p7-retained-launch.rs",
    "crates/replay/src/bin/bm-w4-external-noisy-wall.rs",
];
const FROZEN_ANCHOR_GENERATOR_CONTRACT: &str = "p7_frozen_anchor_generator_receipt_v1";

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let candidate_repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("bm-replay must remain under <repo>/crates/replay")?;
    let workspace_checkout = candidate_repo_root.join("Cargo.lock").is_file()
        && candidate_repo_root.join("crates/core/src").is_dir()
        && candidate_repo_root.join("crates/sdk/src").is_dir()
        && candidate_repo_root.join("crates/replay") == manifest_dir;
    let (root, sdk_inputs, operator_inputs, sdk_contract, operator_contract, anchor, attestation) =
        if workspace_checkout {
            let sdk_inputs = fingerprint_inputs(
                candidate_repo_root,
                &[
                    "Cargo.toml",
                    "Cargo.lock",
                    "crates/core/Cargo.toml",
                    "crates/core/src",
                    "crates/sdk/Cargo.toml",
                    "crates/sdk/src",
                ],
            )?;
            let operator_inputs = fingerprint_inputs(candidate_repo_root, &OPERATOR_BUILD_INPUTS)?;
            (
                candidate_repo_root,
                sdk_inputs,
                operator_inputs,
                SDK_BUILD_FINGERPRINT_CONTRACT,
                OPERATOR_BUILD_FINGERPRINT_CONTRACT,
                candidate_repo_root.join(WORKSPACE_OPERATOR_FROZEN_IDENTITY_RELATIVE_PATH),
                WORKSPACE_BUILD_SOURCE_ATTESTATION,
            )
        } else {
            let packaged_inputs =
                fingerprint_inputs(&manifest_dir, &["Cargo.toml", "build.rs", "src"])?;
            (
                manifest_dir.as_path(),
                packaged_inputs.clone(),
                packaged_inputs,
                PACKAGED_BUILD_FINGERPRINT_CONTRACT,
                PACKAGED_BUILD_FINGERPRINT_CONTRACT,
                manifest_dir.join(CRATE_OPERATOR_FROZEN_IDENTITY_RELATIVE_PATH),
                PACKAGED_BUILD_SOURCE_ATTESTATION,
            )
        };
    for file in &sdk_inputs {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    for file in &operator_inputs {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    println!(
        "cargo:rustc-env=BM_P7_TRUSTED_SDK_BUILD_FINGERPRINT={}",
        fingerprint_files(root, &sdk_inputs, sdk_contract)?
    );
    let operator_fingerprint = fingerprint_files(root, &operator_inputs, operator_contract)?;
    println!(
        "cargo:rustc-env=BM_P7_OPERATOR_BUILD_FINGERPRINT={}",
        operator_fingerprint
    );
    generate_frozen_anchor_receipt(&anchor, &operator_fingerprint)?;
    println!("cargo:rustc-env=BM_P7_BUILD_SOURCE_ATTESTATION={attestation}");
    println!(
        "cargo:rustc-env=BM_P7_OPERATOR_BUILD_PROFILE={}",
        env::var("PROFILE")?
    );
    let mut features = env::vars()
        .filter_map(|(name, value)| {
            if value != "1" {
                return None;
            }
            name.strip_prefix("CARGO_FEATURE_")
                .map(str::to_ascii_lowercase)
        })
        .collect::<Vec<_>>();
    features.sort();
    features.dedup();
    println!(
        "cargo:rustc-env=BM_P7_OPERATOR_BUILD_FEATURES={}",
        features.join(",")
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

fn generate_frozen_anchor_receipt(
    anchor_path: &Path,
    generator_fingerprint: &str,
) -> Result<(), Box<dyn Error>> {
    let anchor = fs::read(anchor_path)?;
    let anchor_sha256 = format!("{:x}", Sha256::digest(&anchor));
    let mut receipt = Sha256::new();
    hash_fingerprint_field(&mut receipt, FROZEN_ANCHOR_GENERATOR_CONTRACT.as_bytes())?;
    hash_fingerprint_field(&mut receipt, generator_fingerprint.as_bytes())?;
    hash_fingerprint_field(&mut receipt, anchor_sha256.as_bytes())?;
    let receipt_sha256 = format!("{:x}", receipt.finalize());
    println!("cargo:rustc-env=BM_P7_FROZEN_ANCHOR_SHA256={anchor_sha256}");
    println!("cargo:rustc-env=BM_P7_FROZEN_ANCHOR_GENERATOR_RECEIPT_SHA256={receipt_sha256}");
    Ok(())
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

fn fingerprint_files(
    root: &Path,
    files: &[PathBuf],
    contract: &str,
) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hash_fingerprint_field(&mut hasher, contract.as_bytes())?;
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
