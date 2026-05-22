#![cfg(feature = "server-stdio")]

use std::process::Command;

#[test]
fn bm_mcp_server_usage_is_structured_and_non_panicking() {
    let bin = std::env::var("CARGO_BIN_EXE_bm-mcp-server").expect("bm-mcp-server binary path");
    let output = Command::new(bin)
        .arg("--help")
        .output()
        .expect("run bm-mcp-server --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains(r#""binary":"bm-mcp-server""#), "{stdout}");
    assert!(stdout.contains(r#""stdio""#), "{stdout}");
    assert!(stdout.contains(r#""http""#), "{stdout}");
    assert!(stdout.contains("127.0.0.1:8788"), "{stdout}");
    assert!(stdout.contains("--store-file"), "{stdout}");
    assert!(stdout.contains("--store-sqlite"), "{stdout}");
    assert!(stdout.contains("BM_MEMORY_STORE_FILE"), "{stdout}");
    assert!(!stdout.contains("panicked"), "{stdout}");
}
