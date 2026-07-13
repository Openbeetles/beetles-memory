use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{StoreBackendConfig, StorePlatform};

#[test]
fn file_store_treats_logical_keys_as_portable_store_keys() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-path-portability-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let platform = StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::DesktopWindowsEmbeddedSdk).unwrap(),
    )
    .unwrap();

    let fs = platform.state_fs();
    fs.write("windows\\logical\\state.json", b"win").unwrap();
    fs.write("linux/logical/state.json", b"linux").unwrap();

    assert_eq!(
        fs.read("windows\\logical\\state.json").unwrap(),
        Some(b"win".to_vec())
    );
    assert_eq!(
        fs.read("linux/logical/state.json").unwrap(),
        Some(b"linux".to_vec())
    );
    assert!(root.join("manifest.json").exists());
}
