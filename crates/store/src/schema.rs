use bm_core::feature_gate::ProfileId;
use bm_core::platform::MemorySystemKind;
use bm_core::{Error, Result};
use serde::{Deserialize, Serialize};

use crate::config::{profile_memory_system_kind, StoreBackendKind};

pub const STORE_SCHEMA_ID: &str = "beetle_memory_store_schema_v1";
pub const STORE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreSchemaManifest {
    pub schema_id: String,
    pub schema_version: u32,
    pub backend: String,
    pub profile: String,
    pub memory_system_kind: String,
    pub created_at_unix_secs: u64,
    pub last_opened_at_unix_secs: u64,
}

impl StoreSchemaManifest {
    pub fn new(backend: StoreBackendKind, profile: ProfileId, now_secs: u64) -> Self {
        Self {
            schema_id: STORE_SCHEMA_ID.to_string(),
            schema_version: STORE_SCHEMA_VERSION,
            backend: backend.as_str().to_string(),
            profile: profile.as_str().to_string(),
            memory_system_kind: profile_memory_system_kind(profile).as_str().to_string(),
            created_at_unix_secs: now_secs,
            last_opened_at_unix_secs: now_secs,
        }
    }

    pub fn touch_opened(&mut self, now_secs: u64) {
        self.last_opened_at_unix_secs = now_secs;
    }

    pub fn validate_against(
        &self,
        backend: StoreBackendKind,
        profile: ProfileId,
        memory_system_kind: MemorySystemKind,
        stage: &'static str,
    ) -> Result<()> {
        if self.schema_id != STORE_SCHEMA_ID {
            return Err(Error::config(
                stage,
                format!("unsupported schema {}", self.schema_id),
            ));
        }
        if self.schema_version != STORE_SCHEMA_VERSION {
            return Err(Error::config(
                stage,
                format!("unsupported schema version {}", self.schema_version),
            ));
        }
        if self.backend != backend.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "backend mismatch: manifest={}, config={}",
                    self.backend,
                    backend.as_str()
                ),
            ));
        }
        if self.profile != profile.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "profile mismatch: manifest={}, config={}",
                    self.profile,
                    profile.as_str()
                ),
            ));
        }
        if self.memory_system_kind != memory_system_kind.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "memory system kind mismatch: manifest={}, config={}",
                    self.memory_system_kind,
                    memory_system_kind.as_str()
                ),
            ));
        }
        Ok(())
    }
}
