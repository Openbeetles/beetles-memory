use std::collections::BTreeSet;
use std::process::Command;

use sha2::{Digest, Sha256};

const ROLE_BINARIES: [(&str, &str); 4] = [
    (
        "source_publisher",
        env!("CARGO_BIN_EXE_bm-p8-quality-source-publisher"),
    ),
    ("quality_runner", env!("CARGO_BIN_EXE_bm-p8-quality-runner")),
    (
        "quality_operator",
        env!("CARGO_BIN_EXE_bm-p8-quality-operator"),
    ),
    (
        "trusted_supervisor",
        env!("CARGO_BIN_EXE_bm-p8-quality-supervisor"),
    ),
];

#[test]
fn p8_quality_roles_are_four_distinct_executables_and_reject_pathname_authority() {
    let identities = ROLE_BINARIES
        .iter()
        .map(|(role, path)| {
            let bytes = std::fs::read(path).expect("read role executable");
            assert!(!bytes.is_empty(), "{role}");
            format!("{:x}", Sha256::digest(bytes))
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(identities.len(), ROLE_BINARIES.len());

    for (role, path) in ROLE_BINARIES {
        let output = Command::new(path)
            .arg("--p8-quality-role-self-test")
            .env("BM_P7_RETAINED_EXECUTABLE_FD", "3")
            .env("BM_P7_RETAINED_EXECUTABLE_PATH", path)
            .env("BM_P7_RETAINED_EXECUTABLE_SHA256", "0".repeat(64))
            .output()
            .expect("run role directly");
        assert!(!output.status.success(), "{role}");
        assert!(output.stdout.is_empty(), "{role}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        #[cfg(target_os = "linux")]
        assert!(stderr.contains("descriptor is missing"), "{role}: {stderr}");
        #[cfg(not(target_os = "linux"))]
        assert!(
            stderr.contains("available only on Linux"),
            "{role}: {stderr}"
        );
    }
}
