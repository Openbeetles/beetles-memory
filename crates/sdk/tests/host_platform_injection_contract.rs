#[test]
fn arbitrary_platform_injection_cannot_bypass_scoped_store_routing() {
    let runtime_source = include_str!("../src/runtime.rs");
    assert!(!runtime_source.contains("pub fn platform("));
    assert!(!runtime_source.contains("pub fn store_platform("));
    assert!(runtime_source.contains("pub fn store(mut self, handle: MemoryStoreHandle)"));
}
