use std::collections::BTreeMap;

use bm_core::Result;
use serde::{Deserialize, Serialize};

use crate::schema::STORE_SCHEMA_VERSION;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStoreEventKind {
    MemoryWrite,
    MemoryMerge,
    MemoryDelete,
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
            Self::MemoryDelete => "memory.delete",
            Self::MemoryMaintenance => "memory.maintenance",
            Self::MemoryProjection => "memory.projection",
            Self::RuntimeLifecycle => "runtime.lifecycle",
            Self::OperatorAction => "operator.action",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreEventScope {
    pub agent_id: String,
    pub owner_id: String,
    pub channel: String,
    pub chat_id: String,
}

impl StoreEventScope {
    pub fn new(
        agent_id: impl Into<String>,
        owner_id: impl Into<String>,
        channel: impl Into<String>,
        chat_id: impl Into<String>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            owner_id: owner_id.into(),
            channel: channel.into(),
            chat_id: chat_id.into(),
        }
    }

    pub fn system(operation: impl Into<String>) -> Self {
        Self {
            agent_id: "system".to_string(),
            owner_id: "system".to_string(),
            channel: "runtime".to_string(),
            chat_id: operation.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
    #[serde(default)]
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
}

pub trait StoreEventLog: Send + Sync {
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()>;
    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>>;
}
