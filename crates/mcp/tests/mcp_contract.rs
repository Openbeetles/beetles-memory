use bm_adapter::AdapterOperation;
use bm_mcp::{resource_specs, tool_specs};

#[test]
fn tool_schema_maps_to_adapter_commands_not_memory_planes() {
    let tools = tool_specs();
    assert_eq!(
        tools.iter().map(|tool| tool.name).collect::<Vec<_>>(),
        vec![
            "memory_capabilities",
            "memory_recall",
            "memory_project",
            "memory_inspect",
            "memory_replay",
            "memory_write_candidate",
            "memory_long_term_list",
            "memory_long_term_detail",
            "memory_long_term_mutate",
            "memory_long_term_policy",
            "memory_transcript_attr_write",
        ]
    );
    assert!(tools
        .iter()
        .any(|tool| tool.name == "memory_recall" && tool.operation == AdapterOperation::Recall));
    assert!(tools
        .iter()
        .any(|tool| tool.name == "memory_project" && tool.operation == AdapterOperation::Project));
    assert!(tools.iter().any(|tool| {
        tool.name == "memory_long_term_list" && tool.operation == AdapterOperation::LongTermList
    }));
    assert!(tools.iter().any(|tool| {
        tool.name == "memory_long_term_mutate" && tool.operation == AdapterOperation::LongTermMutate
    }));
    assert!(tools.iter().any(|tool| {
        tool.name == "memory_long_term_policy" && tool.operation == AdapterOperation::LongTermPolicy
    }));
    assert!(tools.iter().any(|tool| {
        tool.name == "memory_transcript_attr_write"
            && tool.operation == AdapterOperation::TranscriptAttrWrite
            && tool.schema_fields.contains(&"attrs".to_string())
    }));
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
fn transcript_attr_mcp_tool_schema_is_declared_as_thin_adapter_operation() {
    let tool = tool_specs()
        .into_iter()
        .find(|tool| tool.name == "memory_transcript_attr_write")
        .expect("transcript attr MCP tool");

    assert_eq!(tool.operation, AdapterOperation::TranscriptAttrWrite);
    assert!(tool.schema_fields.contains(&"attrs".to_string()));
    assert!(tool.schema_fields.contains(&"memory_space_id".to_string()));
    assert!(!tool.private_raw_allowed);
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
