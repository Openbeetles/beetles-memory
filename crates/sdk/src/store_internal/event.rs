use std::collections::BTreeMap;

use bm_core::{Error, Result};
use serde::{Deserialize, Serialize};

use crate::store_internal::schema::STORE_SCHEMA_VERSION;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStoreEventKind {
    MemoryWrite,
    MemoryMerge,
    MemoryControl,
    MemoryDelete,
    MemoryPolicy,
    MemoryMaintenance,
    MemoryProjection,
    RuntimeLifecycle,
    OperatorAction,
}

impl MemoryStoreEventKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::MemoryWrite => "memory.write",
            Self::MemoryMerge => "memory.merge",
            Self::MemoryControl => "memory.control",
            Self::MemoryDelete => "memory.delete",
            Self::MemoryPolicy => "memory.policy",
            Self::MemoryMaintenance => "memory.maintenance",
            Self::MemoryProjection => "memory.projection",
            Self::RuntimeLifecycle => "runtime.lifecycle",
            Self::OperatorAction => "operator.action",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StorePhysicalOwningScope {
    Subject { mounted_subject_id: String },
    SharedProgram,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoreEventScope {
    pub agent_id: String,
    pub owner_id: String,
    pub channel: String,
    pub chat_id: String,
    pub memory_space_id: String,
    pub subject_id: String,
    pub physical_owning_scope: StorePhysicalOwningScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

impl StoreEventScope {
    pub fn new(
        agent_id: impl Into<String>,
        owner_id: impl Into<String>,
        channel: impl Into<String>,
        chat_id: impl Into<String>,
    ) -> Self {
        let owner_id = owner_id.into();
        Self {
            agent_id: agent_id.into(),
            memory_space_id: owner_id.clone(),
            subject_id: owner_id.clone(),
            physical_owning_scope: StorePhysicalOwningScope::Subject {
                mounted_subject_id: owner_id.clone(),
            },
            owner_id,
            channel: channel.into(),
            chat_id: chat_id.into(),
            conversation_id: None,
        }
    }

    pub fn system(operation: impl Into<String>) -> Self {
        Self {
            agent_id: "system".to_string(),
            owner_id: "system".to_string(),
            channel: "runtime".to_string(),
            chat_id: operation.into(),
            memory_space_id: "system".to_string(),
            subject_id: "system".to_string(),
            physical_owning_scope: StorePhysicalOwningScope::Subject {
                mounted_subject_id: "system".to_string(),
            },
            conversation_id: None,
        }
    }

    pub fn with_memory_space(mut self, memory_space_id: impl Into<String>) -> Self {
        self.memory_space_id = memory_space_id.into();
        self
    }

    pub fn with_subject(mut self, subject_id: impl Into<String>) -> Self {
        self.subject_id = subject_id.into();
        self.physical_owning_scope = StorePhysicalOwningScope::Subject {
            mounted_subject_id: self.subject_id.clone(),
        };
        self
    }

    pub fn with_shared_program(mut self) -> Self {
        self.physical_owning_scope = StorePhysicalOwningScope::SharedProgram;
        self
    }

    pub fn with_conversation(mut self, conversation_id: impl Into<String>) -> Self {
        self.conversation_id = Some(conversation_id.into());
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryStoreEvent {
    pub event_id: String,
    pub kind: MemoryStoreEventKind,
    pub kind_name: String,
    pub scope: StoreEventScope,
    pub plane: String,
    pub record_key: String,
    pub content_hash: String,
    pub schema_version: u32,
    pub timestamp_unix_secs: u64,
    pub payload: BTreeMap<String, String>,
}

impl MemoryStoreEvent {
    pub fn new(
        event_id: impl Into<String>,
        kind: MemoryStoreEventKind,
        scope: StoreEventScope,
        timestamp_unix_secs: u64,
    ) -> Self {
        let kind_name = kind.as_str().to_string();
        Self {
            event_id: event_id.into(),
            kind,
            kind_name,
            scope,
            plane: String::new(),
            record_key: String::new(),
            content_hash: String::new(),
            schema_version: STORE_SCHEMA_VERSION,
            timestamp_unix_secs,
            payload: BTreeMap::new(),
        }
    }

    pub fn with_plane(mut self, plane: impl Into<String>) -> Self {
        self.plane = plane.into();
        self
    }

    pub fn with_record_key(mut self, record_key: impl Into<String>) -> Self {
        self.record_key = record_key.into();
        self
    }

    pub fn with_content_hash(mut self, content_hash: impl Into<String>) -> Self {
        self.content_hash = content_hash.into();
        self
    }

    pub fn with_payload(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.payload.insert(key.into(), value.into());
        self
    }

    pub(crate) fn validate_current_schema(&self, stage: &'static str) -> Result<()> {
        if self.schema_version != STORE_SCHEMA_VERSION {
            return Err(Error::config(
                stage,
                format!("unsupported event schema version {}", self.schema_version),
            ));
        }
        if self.event_id.trim().is_empty()
            || self.kind_name != self.kind.as_str()
            || self.scope.memory_space_id.trim().is_empty()
            || self.scope.subject_id.trim().is_empty()
        {
            return Err(Error::config(
                stage,
                "event identity, kind, memory space, and subject must be canonical",
            ));
        }
        Ok(())
    }
}

pub trait StoreEventLog: Send + Sync {
    #[cfg(feature = "nonproduction-replay-harness")]
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()>;
    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_schema_is_exact_and_unknown_fields_are_rejected() {
        let event = MemoryStoreEvent::new(
            "event-1",
            MemoryStoreEventKind::MemoryWrite,
            StoreEventScope::new("agent", "owner", "channel", "chat")
                .with_memory_space("space")
                .with_subject("subject"),
            7,
        );
        assert!(event.validate_current_schema("event_test").is_ok());

        let mut legacy = serde_json::to_value(&event).expect("event value");
        legacy["schema_version"] = serde_json::json!(5);
        let legacy: MemoryStoreEvent = serde_json::from_value(legacy).expect("legacy shape");
        assert!(legacy.validate_current_schema("event_test").is_err());

        let mut unknown = serde_json::to_value(&event).expect("event value");
        unknown["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<MemoryStoreEvent>(unknown).is_err());
    }
}
