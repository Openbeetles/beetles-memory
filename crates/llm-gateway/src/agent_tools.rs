use bm_sdk::{
    fingerprint_agent_tool_registry, AgentToolDescriptor, AgentToolRegistryOwner,
    AgentToolRegistrySnapshot,
};
use serde_json::Value;

const GATEWAY_TOOL_REGISTRY_ID: &str = "gateway-host-tools";

pub(crate) fn request_scoped_agent_tool_registry(
    namespace: &str,
    tools: Option<&Value>,
) -> Option<AgentToolRegistrySnapshot> {
    let tools = tools?.as_array()?;
    if tools.is_empty() {
        return None;
    }
    let mut descriptors = Vec::new();
    for tool in tools {
        let Some(tool_id) = tool_id(tool) else {
            continue;
        };
        let mut descriptor =
            AgentToolDescriptor::compact(tool_id.clone(), tool_id, schema_fingerprint(tool));
        descriptor.tool_groups = vec!["gateway_request".to_string()];
        descriptors.push(descriptor);
    }
    if descriptors.is_empty() {
        return None;
    }
    let mut snapshot =
        AgentToolRegistrySnapshot::compact(GATEWAY_TOOL_REGISTRY_ID, namespace, descriptors, 0);
    snapshot.owner = AgentToolRegistryOwner::RequestScopedGateway;
    snapshot.fingerprint = fingerprint_agent_tool_registry(&snapshot);
    Some(snapshot)
}

fn tool_id(tool: &Value) -> Option<String> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .or_else(|| tool.get("name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn schema_fingerprint(tool: &Value) -> String {
    let rendered = serde_json::to_string(tool).unwrap_or_default();
    format!("fnv1a64:{:016x}", fnv1a64(rendered.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
