//! A2A bridge contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A2aPermission {
    MemoryReport,
    Executor,
    Tool,
    Workflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A2aBridgeMessageSpec {
    pub name: &'static str,
    pub operation: Option<AdapterOperation>,
    pub permissions: Vec<A2aPermission>,
}

pub fn merge_peer_visibility(local_visible: bool, peer_visible: bool) -> bool {
    local_visible && peer_visible
}

pub fn bridge_message_specs() -> Vec<A2aBridgeMessageSpec> {
    vec![
        message("peer_capability", None),
        message("memory_write_candidate", Some(AdapterOperation::Write)),
        message("memory_recall_request", Some(AdapterOperation::Recall)),
        message("memory_projection_request", Some(AdapterOperation::Project)),
        message("memory_report", None),
        message("memory_migration_chunk", Some(AdapterOperation::Import)),
        message("runtime_lifecycle_event", None),
    ]
}

fn message(name: &'static str, operation: Option<AdapterOperation>) -> A2aBridgeMessageSpec {
    A2aBridgeMessageSpec {
        name,
        operation,
        permissions: vec![A2aPermission::MemoryReport],
    }
}
