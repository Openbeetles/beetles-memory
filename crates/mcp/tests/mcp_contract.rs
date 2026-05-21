use bm_adapter::AdapterOperation;
use bm_mcp::tool_specs;

#[test]
fn tool_schema_maps_to_adapter_commands_not_memory_planes() {
    let tools = tool_specs();
    assert!(tools
        .iter()
        .any(|tool| tool.name == "memory_recall" && tool.operation == AdapterOperation::Recall));
    assert!(tools
        .iter()
        .any(|tool| tool.name == "memory_project" && tool.operation == AdapterOperation::Project));
    for tool in tools {
        assert!(!tool.schema_fields.contains(&"plane".to_string()));
        assert!(!tool.schema_fields.contains(&"store_schema".to_string()));
    }
}

#[test]
fn tool_results_forbid_private_raw_material() {
    for tool in tool_specs() {
        assert!(!tool.private_raw_allowed);
    }
}
