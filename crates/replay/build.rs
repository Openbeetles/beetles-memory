mod build_support;

use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use build_support::{
    collect_regular_files, read_regular_file_stable, sort_regular_files_relative_to,
    P7_OPERATOR_BUILD_FINGERPRINT_CONTRACT, P7_OPERATOR_BUILD_INPUTS,
    P7_SDK_BUILD_FINGERPRINT_CONTRACT, P7_SDK_BUILD_INPUTS,
};

const P8_OPERATOR_BUILD_FINGERPRINT_CONTRACT: &str = "p8_operator_build_inputs_sha256_v1";
const P8_VERIFIER_SOURCE_FINGERPRINT_CONTRACT: &str = "p8_verifier_source_inputs_sha256_v1";
const P8_WORKSPACE_SEMANTIC_FINGERPRINT_CONTRACT: &str = "p8_workspace_semantic_inputs_sha256_v1";
const P8_CORE_VALIDATOR_FINGERPRINT_CONTRACT: &str = "p8_core_validator_inputs_sha256_v1";
const P8_SDK_VALIDATOR_FINGERPRINT_CONTRACT: &str = "p8_sdk_validator_inputs_sha256_v1";
const P8_POST_IMAGE_VALIDATOR_FINGERPRINT_CONTRACT: &str =
    "p8_post_image_validator_inputs_sha256_v1";
const P8_ROOT_MANIFEST_FINGERPRINT_CONTRACT: &str = "p8_root_manifest_sha256_v1";
const P8_LOCK_FINGERPRINT_CONTRACT: &str = "p8_lock_sha256_v1";
const P8_TOOLCHAIN_FINGERPRINT_CONTRACT: &str = "p8_toolchain_identity_sha256_v1";
const PACKAGED_BUILD_FINGERPRINT_CONTRACT: &str = "p7_packaged_unattested_inputs_sha256_v1";
const WORKSPACE_BUILD_SOURCE_ATTESTATION: &str = "workspace_source";
const PACKAGED_BUILD_SOURCE_ATTESTATION: &str = "packaged_unattested";
const WORKSPACE_OPERATOR_FROZEN_IDENTITY_RELATIVE_PATH: &str =
    "crates/replay/src/bin/bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs";
const CRATE_OPERATOR_FROZEN_IDENTITY_RELATIVE_PATH: &str =
    "src/bin/bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs";
const P8_FROZEN_QUALITY_POLICY_RELATIVE_PATH: &str =
    "src/bin/bm-p8-quality-operator/p8_frozen_quality_policy.rs";
const PACKAGED_BUILD_INPUTS: [&str; 4] = ["Cargo.toml", "build.rs", "build_support.rs", "src"];
const P8_CORE_VALIDATOR_INPUTS: [&str; 3] = [
    "crates/core/Cargo.toml",
    "crates/core/src",
    "crates/core/tests",
];
const P8_SDK_VALIDATOR_INPUTS: [&str; 3] = [
    "crates/sdk/Cargo.toml",
    "crates/sdk/src",
    "crates/sdk/tests",
];
const P8_POST_IMAGE_VALIDATOR_INPUTS: [&str; 9] = [
    "crates/core/Cargo.toml",
    "crates/core/src",
    "crates/core/tests",
    "crates/sdk/Cargo.toml",
    "crates/sdk/src",
    "crates/sdk/tests",
    "crates/store-contract-tests/Cargo.toml",
    "crates/store-contract-tests/src",
    "crates/store-contract-tests/tests",
];
const P8_VERIFIER_SOURCE_INPUTS: [&str; 24] = [
    "Cargo.toml",
    "build.rs",
    "build_support.rs",
    "src/lib.rs",
    "src/bounded_process.rs",
    "src/bounded_process/linux_cgroup_v2.rs",
    "src/retained_artifact_fs.rs",
    "src/p8_artifact_dir.rs",
    "src/p8_gate_parent.rs",
    "src/p8_process_authority.rs",
    "src/p8_quality_process.rs",
    "src/p8_quality",
    "src/p8_semantic.rs",
    "src/p8_semantic_operator.rs",
    "src/sealed_execution.rs",
    "src/bin/bm-p8-semantic-gate-parent.rs",
    "src/bin/bm-p8-semantic-operator.rs",
    "src/bin/bm-p8-quality-source-publisher.rs",
    "src/bin/bm-p8-quality-runner.rs",
    "src/bin/bm-p8-quality-supervisor.rs",
    "src/bin/bm-p8-quality-operator.rs",
    "tests/p8_common_source_build_support.rs",
    "tests/p8_quality_public_surface.rs",
    "tests/p8_quality_trusted_source_t3.rs",
];
const P8_COMMON_SOURCE_EXCLUDED_INPUTS: [&str; 1] =
    ["src/bin/bm-p8-quality-operator/p8_frozen_quality_policy.rs"];
const P8_COMMON_SOURCE_INVENTORY_RULE_CONTRACT: &str =
    "p8_common_harness_workspace_inventory_rule_v1";
const FROZEN_ANCHOR_GENERATOR_CONTRACT: &str = "p7_frozen_anchor_generator_receipt_v1";
const P8_FROZEN_POLICY_GENERATOR_CONTRACT: &str = "p8_frozen_quality_policy_generator_receipt_v1";

fn main() -> Result<(), Box<dyn Error>> {
    let cargo_manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let candidate_repo_root = cargo_manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("bm-replay must remain under <repo>/crates/replay")?;
    let workspace_checkout = candidate_repo_root.join("Cargo.lock").is_file()
        && candidate_repo_root.join("crates/core/src").is_dir()
        && candidate_repo_root.join("crates/sdk/src").is_dir()
        && candidate_repo_root.join("crates/replay") == cargo_manifest_dir;
    // Cargo may stage a packaged crate below the macOS `/var` compatibility
    // alias while filesystem canonicalization returns `/private/var`. Resolve
    // that Cargo-owned package root once; child inputs still reject symlinks
    // and retain exact stable-file identity checks.
    let manifest_dir = if workspace_checkout {
        cargo_manifest_dir
    } else {
        fs::canonicalize(cargo_manifest_dir)?
    };
    let candidate_repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("bm-replay must remain under <repo>/crates/replay")?;
    let (root, sdk_inputs, operator_inputs, sdk_contract, operator_contract, anchor, attestation) =
        if workspace_checkout {
            let sdk_inputs = p7_fingerprint_inputs(candidate_repo_root, &P7_SDK_BUILD_INPUTS)?;
            let operator_inputs =
                p7_fingerprint_inputs(candidate_repo_root, &P7_OPERATOR_BUILD_INPUTS)?;
            (
                candidate_repo_root,
                sdk_inputs,
                operator_inputs,
                P7_SDK_BUILD_FINGERPRINT_CONTRACT,
                P7_OPERATOR_BUILD_FINGERPRINT_CONTRACT,
                candidate_repo_root.join(WORKSPACE_OPERATOR_FROZEN_IDENTITY_RELATIVE_PATH),
                WORKSPACE_BUILD_SOURCE_ATTESTATION,
            )
        } else {
            let packaged_inputs = p7_fingerprint_inputs(&manifest_dir, &PACKAGED_BUILD_INPUTS)?;
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
    if workspace_checkout {
        emit_rerun_if_changed(root, &P7_SDK_BUILD_INPUTS);
        emit_rerun_if_changed(root, &P7_OPERATOR_BUILD_INPUTS);
    } else {
        emit_rerun_if_changed(root, &PACKAGED_BUILD_INPUTS);
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
    let p8_source_inputs = p8_fingerprint_inputs(&manifest_dir, &P8_VERIFIER_SOURCE_INPUTS)?;
    emit_rerun_if_changed(&manifest_dir, &P8_VERIFIER_SOURCE_INPUTS);
    let p8_source_fingerprint = fingerprint_files(
        &manifest_dir,
        &p8_source_inputs,
        P8_VERIFIER_SOURCE_FINGERPRINT_CONTRACT,
    )?;
    generate_p8_common_source_inventory(
        &manifest_dir,
        &p8_source_inputs,
        workspace_checkout && cfg!(unix),
        &p8_source_fingerprint,
        &PathBuf::from(env::var("OUT_DIR")?),
    )?;
    println!("cargo:rustc-env=BM_P8_VERIFIER_SOURCE_FINGERPRINT={p8_source_fingerprint}");
    let root_manifest_fingerprint = fingerprint_files(
        root,
        &[root.join("Cargo.toml")],
        P8_ROOT_MANIFEST_FINGERPRINT_CONTRACT,
    )?;
    let lock_fingerprint = fingerprint_files(
        root,
        &[root.join("Cargo.lock")],
        P8_LOCK_FINGERPRINT_CONTRACT,
    )?;
    let toolchain_fingerprint = fingerprint_toolchain()?;
    println!("cargo:rustc-env=BM_P8_ROOT_MANIFEST_FINGERPRINT={root_manifest_fingerprint}");
    println!("cargo:rustc-env=BM_P8_LOCK_FINGERPRINT={lock_fingerprint}");
    println!("cargo:rustc-env=BM_P8_TOOLCHAIN_FINGERPRINT={toolchain_fingerprint}");
    let (
        p8_core_validator_fingerprint,
        p8_sdk_validator_fingerprint,
        p8_post_image_validator_fingerprint,
    ) = if workspace_checkout {
        let core_inputs = p8_fingerprint_inputs(root, &P8_CORE_VALIDATOR_INPUTS)?;
        let sdk_validator_inputs = p8_fingerprint_inputs(root, &P8_SDK_VALIDATOR_INPUTS)?;
        let post_image_inputs = p8_fingerprint_inputs(root, &P8_POST_IMAGE_VALIDATOR_INPUTS)?;
        emit_rerun_if_changed(root, &P8_CORE_VALIDATOR_INPUTS);
        emit_rerun_if_changed(root, &P8_SDK_VALIDATOR_INPUTS);
        emit_rerun_if_changed(root, &P8_POST_IMAGE_VALIDATOR_INPUTS);
        (
            fingerprint_files(root, &core_inputs, P8_CORE_VALIDATOR_FINGERPRINT_CONTRACT)?,
            fingerprint_files(
                root,
                &sdk_validator_inputs,
                P8_SDK_VALIDATOR_FINGERPRINT_CONTRACT,
            )?,
            fingerprint_files(
                root,
                &post_image_inputs,
                P8_POST_IMAGE_VALIDATOR_FINGERPRINT_CONTRACT,
            )?,
        )
    } else {
        (
            p8_source_fingerprint.clone(),
            p8_source_fingerprint.clone(),
            p8_source_fingerprint.clone(),
        )
    };
    println!("cargo:rustc-env=BM_P8_CORE_VALIDATOR_FINGERPRINT={p8_core_validator_fingerprint}");
    println!("cargo:rustc-env=BM_P8_SDK_VALIDATOR_FINGERPRINT={p8_sdk_validator_fingerprint}");
    println!(
        "cargo:rustc-env=BM_P8_POST_IMAGE_VALIDATOR_FINGERPRINT={p8_post_image_validator_fingerprint}"
    );
    println!(
        "cargo:rustc-env=BM_P8_VALIDATOR_SOURCE_ATTESTATION={}",
        if workspace_checkout {
            WORKSPACE_BUILD_SOURCE_ATTESTATION
        } else {
            PACKAGED_BUILD_SOURCE_ATTESTATION
        }
    );
    let p8_operator_fingerprint = if workspace_checkout {
        let workspace_semantic_inputs = p8_fingerprint_inputs(root, &P7_SDK_BUILD_INPUTS)?;
        emit_rerun_if_changed(root, &P7_SDK_BUILD_INPUTS);
        let workspace_semantic_fingerprint = fingerprint_files(
            root,
            &workspace_semantic_inputs,
            P8_WORKSPACE_SEMANTIC_FINGERPRINT_CONTRACT,
        )?;
        fingerprint_components(
            P8_OPERATOR_BUILD_FINGERPRINT_CONTRACT,
            &[&p8_source_fingerprint, &workspace_semantic_fingerprint],
        )?
    } else {
        fingerprint_components(
            P8_OPERATOR_BUILD_FINGERPRINT_CONTRACT,
            &[&p8_source_fingerprint, PACKAGED_BUILD_SOURCE_ATTESTATION],
        )?
    };
    println!(
        "cargo:rustc-env=BM_P8_OPERATOR_BUILD_FINGERPRINT={}",
        p8_operator_fingerprint
    );
    generate_frozen_anchor_receipt(&anchor, &operator_fingerprint)?;
    generate_p8_frozen_policy_receipt(
        &manifest_dir.join(P8_FROZEN_QUALITY_POLICY_RELATIVE_PATH),
        &p8_operator_fingerprint,
    )?;
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
    println!(
        "cargo:rustc-env=BM_P8_OPERATOR_BUILD_PROFILE={}",
        env::var("PROFILE")?
    );
    println!(
        "cargo:rustc-env=BM_P8_OPERATOR_BUILD_FEATURES={}",
        features.join(",")
    );
    println!(
        "cargo:rustc-env=BM_P8_OPERATOR_BUILD_TARGET={}",
        env::var("TARGET")?
    );
    println!(
        "cargo:rustc-env=BM_P8_OPERATOR_BUILD_HOST={}",
        env::var("HOST")?
    );
    println!("cargo:rustc-env=BM_P8_BUILD_SOURCE_ATTESTATION={attestation}");
    Ok(())
}

fn p7_fingerprint_inputs(root: &Path, relatives: &[&str]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    if fs::canonicalize(root)? != root {
        return Err(format!("source root must already be canonical: {}", root.display()).into());
    }
    let mut files = Vec::new();
    for relative in relatives {
        collect_regular_files(&root.join(relative), &mut files)?;
    }
    // P7 fingerprints predate the P8 portable inventory and bind Rust Path
    // component ordering. Keep that contract exact instead of silently changing
    // existing P7 identities to the P8 relative-string ordering.
    files.sort();
    if files.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("P7 source input selectors overlap or contain duplicates".into());
    }
    Ok(files)
}

fn emit_rerun_if_changed(root: &Path, relatives: &[&str]) {
    for relative in relatives {
        println!("cargo:rerun-if-changed={}", root.join(relative).display());
    }
}

fn p8_fingerprint_inputs(root: &Path, relatives: &[&str]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    if fs::canonicalize(root)? != root {
        return Err(format!("source root must already be canonical: {}", root.display()).into());
    }
    let mut files = Vec::new();
    for relative in relatives {
        collect_regular_files(&root.join(relative), &mut files)?;
    }
    sort_regular_files_relative_to(root, &mut files)?;
    if files.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("source input selectors overlap or contain duplicates".into());
    }
    Ok(files)
}

fn generate_frozen_anchor_receipt(
    anchor_path: &Path,
    generator_fingerprint: &str,
) -> Result<(), Box<dyn Error>> {
    let (anchor_sha256, receipt_sha256) = generate_anchor_receipt(
        anchor_path,
        generator_fingerprint,
        FROZEN_ANCHOR_GENERATOR_CONTRACT,
    )?;
    println!("cargo:rustc-env=BM_P7_FROZEN_ANCHOR_SHA256={anchor_sha256}");
    println!("cargo:rustc-env=BM_P7_FROZEN_ANCHOR_GENERATOR_RECEIPT_SHA256={receipt_sha256}");
    Ok(())
}

fn generate_p8_frozen_policy_receipt(
    anchor_path: &Path,
    generator_fingerprint: &str,
) -> Result<(), Box<dyn Error>> {
    let (anchor_sha256, receipt_sha256) = generate_anchor_receipt(
        anchor_path,
        generator_fingerprint,
        P8_FROZEN_POLICY_GENERATOR_CONTRACT,
    )?;
    println!("cargo:rerun-if-changed={}", anchor_path.display());
    println!("cargo:rustc-env=BM_P8_FROZEN_POLICY_SHA256={anchor_sha256}");
    println!("cargo:rustc-env=BM_P8_FROZEN_POLICY_GENERATOR_RECEIPT_SHA256={receipt_sha256}");
    Ok(())
}

fn generate_anchor_receipt(
    anchor_path: &Path,
    generator_fingerprint: &str,
    generator_contract: &str,
) -> Result<(String, String), Box<dyn Error>> {
    let anchor = read_regular_file_stable(anchor_path)?;
    let anchor_sha256 = format!("{:x}", Sha256::digest(&anchor));
    let mut receipt = Sha256::new();
    hash_fingerprint_field(&mut receipt, generator_contract.as_bytes())?;
    hash_fingerprint_field(&mut receipt, generator_fingerprint.as_bytes())?;
    hash_fingerprint_field(&mut receipt, anchor_sha256.as_bytes())?;
    let receipt_sha256 = format!("{:x}", receipt.finalize());
    Ok((anchor_sha256, receipt_sha256))
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
        hash_fingerprint_field(&mut hasher, &read_regular_file_stable(file)?)?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn fingerprint_components(contract: &str, components: &[&str]) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hash_fingerprint_field(&mut hasher, contract.as_bytes())?;
    hasher.update(u64::try_from(components.len())?.to_le_bytes());
    for component in components {
        hash_fingerprint_field(&mut hasher, component.as_bytes())?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn fingerprint_toolchain() -> Result<String, Box<dyn Error>> {
    let rustc = env::var_os("RUSTC").ok_or("RUSTC is unavailable to bm-replay build")?;
    let cargo = env::var_os("CARGO").ok_or("CARGO is unavailable to bm-replay build")?;
    let rustc_output = Command::new(rustc).arg("-Vv").output()?;
    let cargo_output = Command::new(cargo).arg("-Vv").output()?;
    if !rustc_output.status.success()
        || !rustc_output.stderr.is_empty()
        || !cargo_output.status.success()
        || !cargo_output.stderr.is_empty()
    {
        return Err("P8 toolchain identity command failed".into());
    }
    let mut hasher = Sha256::new();
    hash_fingerprint_field(&mut hasher, P8_TOOLCHAIN_FINGERPRINT_CONTRACT.as_bytes())?;
    hash_fingerprint_field(&mut hasher, &rustc_output.stdout)?;
    hash_fingerprint_field(&mut hasher, &cargo_output.stdout)?;
    hash_fingerprint_field(&mut hasher, env::var("TARGET")?.as_bytes())?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn generate_p8_common_source_inventory(
    manifest_dir: &Path,
    files: &[PathBuf],
    workspace_attested: bool,
    aggregate_fingerprint: &str,
    output_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut rule_hasher = Sha256::new();
    hash_fingerprint_field(
        &mut rule_hasher,
        P8_COMMON_SOURCE_INVENTORY_RULE_CONTRACT.as_bytes(),
    )?;
    for input in P8_VERIFIER_SOURCE_INPUTS {
        hash_fingerprint_field(&mut rule_hasher, input.as_bytes())?;
    }
    for excluded in P8_COMMON_SOURCE_EXCLUDED_INPUTS {
        hash_fingerprint_field(&mut rule_hasher, excluded.as_bytes())?;
    }
    let rule_sha256 = format!("{:x}", rule_hasher.finalize());

    let mut generated = String::new();
    generated.push_str(&format!(
        "pub(super) const WORKSPACE_ATTESTED: bool = {workspace_attested};\n"
    ));
    generated.push_str(&format!(
        "pub(super) const INVENTORY_RULE_SHA256: &str = {rule_sha256:?};\n"
    ));
    generated.push_str(&format!(
        "pub(super) const AGGREGATE_FINGERPRINT_SHA256: &str = {aggregate_fingerprint:?};\n"
    ));
    generated.push_str("pub(super) const SOURCE_SELECTORS: &[&str] = &[\n");
    for selector in P8_VERIFIER_SOURCE_INPUTS {
        generated.push_str(&format!("    {selector:?},\n"));
    }
    generated.push_str("];\n");
    generated.push_str("pub(super) const EXCLUDED_RELATIVE_PATHS: &[&str] = &[\n");
    for excluded in P8_COMMON_SOURCE_EXCLUDED_INPUTS {
        generated.push_str(&format!("    {excluded:?},\n"));
    }
    generated.push_str("];\n");
    generated.push_str("pub(super) const SOURCE_INPUTS: &[(&str, &str, u64, &str)] = &[\n");
    for file in files {
        let relative = file
            .strip_prefix(manifest_dir)?
            .to_str()
            .ok_or("P8 common source path must be UTF-8")?
            .replace('\\', "/");
        if P8_COMMON_SOURCE_EXCLUDED_INPUTS.contains(&relative.as_str()) {
            return Err("P8 frozen policy anchor entered common source inventory".into());
        }
        let role = p8_common_source_component(&relative);
        let bytes = read_regular_file_stable(file)?;
        let byte_len = u64::try_from(bytes.len())?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        generated.push_str(&format!(
            "    ({role:?}, {relative:?}, {byte_len}, {sha256:?}),\n"
        ));
    }
    generated.push_str("];\n");
    fs::write(output_dir.join("p8_common_source_inventory.rs"), generated)?;
    Ok(())
}

fn p8_common_source_component(relative_path: &str) -> &'static str {
    match relative_path {
        "src/bin/bm-p8-quality-runner.rs" => "quality_runner",
        "src/bin/bm-p8-quality-operator.rs" => "quality_operator",
        "src/bin/bm-p8-quality-source-publisher.rs" => "source_publisher",
        "src/bin/bm-p8-quality-supervisor.rs" => "trusted_supervisor",
        _ => "replay_quality",
    }
}

fn hash_fingerprint_field(hasher: &mut Sha256, value: &[u8]) -> Result<(), Box<dyn Error>> {
    hasher.update(u64::try_from(value.len())?.to_le_bytes());
    hasher.update(value);
    Ok(())
}
