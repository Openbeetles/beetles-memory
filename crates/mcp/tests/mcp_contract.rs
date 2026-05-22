use bm_adapter::AdapterOperation;
use bm_mcp::{resource_specs, tool_specs};

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

#[test]
fn resource_schema_exposes_only_safe_memory_resources() {
    let resources = resource_specs();
    let uris = resources
        .iter()
        .map(|resource| resource.uri)
        .collect::<Vec<_>>();

    assert_eq!(
        uris,
        vec![
            "memory://profile",
            "memory://scope",
            "memory://projection-preview"
        ]
    );
    for resource in resources {
        assert!(!resource.private_raw_allowed);
        assert!(!resource.uri.contains("raw"));
        assert!(!resource.name.contains("raw"));
    }
}

#[test]
fn write_candidate_tool_schema_matches_adapter_payload_not_placeholder_candidate() {
    let tools = tool_specs();
    let write = tools
        .iter()
        .find(|tool| tool.name == "memory_write_candidate")
        .expect("memory_write_candidate tool");

    assert!(write.schema_fields.contains(&"name".to_string()));
    assert!(write.schema_fields.contains(&"topic".to_string()));
    assert!(write.schema_fields.contains(&"title".to_string()));
    assert!(write.schema_fields.contains(&"summary".to_string()));
    assert!(write.schema_fields.contains(&"content".to_string()));
    assert!(!write.schema_fields.contains(&"candidate".to_string()));
}
