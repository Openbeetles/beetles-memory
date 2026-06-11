#[cfg(feature = "sqlite-index")]
struct EnvRestore {
    key: &'static str,
    old: Option<std::ffi::OsString>,
}

#[cfg(feature = "sqlite-index")]
impl EnvRestore {
    fn new(key: &'static str) -> Self {
        let old = std::env::var_os(key);
        Self { key, old }
    }
}

#[cfg(feature = "sqlite-index")]
impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(value) = self.old.as_ref() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[cfg(feature = "sqlite-index")]
#[test]
fn sqlite_index_state_dir_requires_explicit_absolute_path() {
    let _restore = EnvRestore::new("BEETLE_MEMORY_STATE_DIR");

    std::env::remove_var("BEETLE_MEMORY_STATE_DIR");
    assert_eq!(
        bm_core::platform::sqlite_index_state_dir().expect("missing state dir should not error"),
        None
    );

    std::env::set_var("BEETLE_MEMORY_STATE_DIR", "relative-memory");
    let error =
        bm_core::platform::sqlite_index_state_dir().expect_err("relative state dir must fail");
    assert!(error.to_string().contains("absolute"));

    let absolute = std::env::temp_dir().join(format!(
        "beetle-memory-sqlite-index-contract-{}",
        std::process::id()
    ));
    std::env::set_var("BEETLE_MEMORY_STATE_DIR", &absolute);
    assert_eq!(
        bm_core::platform::sqlite_index_state_dir().expect("absolute state dir should be accepted"),
        Some(absolute)
    );
}
