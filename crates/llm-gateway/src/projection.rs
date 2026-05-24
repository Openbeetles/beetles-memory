pub(crate) const MEMORY_PROJECTION_TAG_OPEN: &str = r#"<beetle-memory-projection version="1">"#;
pub(crate) const MEMORY_PROJECTION_TAG_CLOSE: &str = "</beetle-memory-projection>";

pub(crate) fn render_model_facing_projection(memory_block: &str) -> Option<String> {
    let memory_block = memory_block.trim();
    if memory_block.is_empty() {
        None
    } else {
        Some(format!(
            "{MEMORY_PROJECTION_TAG_OPEN}\n{memory_block}\n{MEMORY_PROJECTION_TAG_CLOSE}"
        ))
    }
}
