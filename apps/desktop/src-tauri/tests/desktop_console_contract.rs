use std::time::{SystemTime, UNIX_EPOCH};

use bm_desktop::{DesktopConsoleRequest, DesktopConsoleState};
use serde_json::Value;

#[test]
fn desktop_console_serves_skills_without_http_listener() {
    let state = DesktopConsoleState::open_for_data_dir(test_store_dir("skills-list")).unwrap();

    let response = state
        .handle_console_request(DesktopConsoleRequest::get("/console/skills"))
        .unwrap();

    assert_eq!(response.status_code, 200);
    assert!(response.body.contains(r#""status":"accepted""#));
    assert!(response.body.contains(r#""skills""#));
}

#[test]
fn desktop_console_serves_ollama_transparent_status_without_404() {
    let state = DesktopConsoleState::open_for_data_dir(test_store_dir("ollama-transparent-status"))
        .unwrap();

    let response = state
        .handle_console_request(DesktopConsoleRequest::get(
            "/console/ollama-transparent/status",
        ))
        .unwrap();

    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("status json");
    assert_eq!(body["status"], "accepted");
    assert!(body.get("ollamaTransparent").is_some(), "{}", response.body);
}

#[test]
fn desktop_console_mutates_skills_through_entry_runtime() {
    let state = DesktopConsoleState::open_for_data_dir(test_store_dir("skills-mutation")).unwrap();

    let mutation = state
        .handle_console_request(DesktopConsoleRequest::post_json(
            "/console/skills",
            r#"{
              "title":"Desktop direct skill",
              "topic":"desktop_console",
              "summary":"Desktop commands must use the in-process entry runtime.",
              "procedure":"1. open the Tauri app\n2. call the shared console API\n3. verify the returned report",
              "citations":["desktop contract test"]
            }"#,
        ))
        .unwrap();
    assert_eq!(mutation.status_code, 200);
    assert!(
        mutation.body.contains(r#""accepted":true"#),
        "{}",
        mutation.body
    );

    let list = state
        .handle_console_request(DesktopConsoleRequest::get("/console/skills?query=desktop"))
        .unwrap();
    assert_eq!(list.status_code, 200);
    assert!(list.body.contains("Desktop direct skill"));
    assert!(list.body.contains(r#""userProvided":1"#));
}

fn test_store_dir(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("bm-desktop-{label}-{nanos}"))
}
