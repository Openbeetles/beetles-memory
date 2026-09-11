use bm_core::memory::SessionMessage;

#[test]
fn session_message_rejects_old_role_content_only_shape() {
    let legacy_json = r#"{"role":"user","content":"legacy"}"#;
    let parsed = serde_json::from_str::<SessionMessage>(legacy_json);
    assert!(parsed.is_err());
}
