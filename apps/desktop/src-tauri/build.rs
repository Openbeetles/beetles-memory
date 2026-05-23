fn main() {
    if std::env::var("PROFILE").ok().as_deref() != Some("release")
        && std::env::var_os("TAURI_CONFIG").is_none()
    {
        std::env::set_var("TAURI_CONFIG", r#"{"bundle":{"externalBin":[]}}"#);
    }
    tauri_build::build();
}
