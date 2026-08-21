use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

use super::LongTermControlOperation;

pub const MEMORY_MUTATION_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const MEMORY_MUTATION_RECEIPT_NAMESPACE: &str = "memory_mutation_receipts";
pub const MEMORY_MUTATION_AUDIT_NAMESPACE: &str = "memory_mutation_audits";
const OPERATION_STORAGE_KEY_DOMAIN: &str = "memory_mutation_operation_storage_key_v1";
const OPERATION_ID_DIGEST_DOMAIN: &str = "memory_mutation_operation_id_v1";
const MAX_OPERATION_COMPONENT_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationOperationIdentity {
    operation_id_digest: String,
    memory_space_id: String,
    mounted_subject_id: String,
    actor_subject_id: String,
    operation_kind: MemoryMutationOperationKind,
}

impl MemoryMutationOperationIdentity {
    pub fn new(
        operation_id: impl Into<String>,
        memory_space_id: impl Into<String>,
        mounted_subject_id: impl Into<String>,
        actor_subject_id: impl Into<String>,
        operation_kind: MemoryMutationOperationKind,
    ) -> Result<Self> {
        let operation_id = operation_id.into();
        validate_component(&operation_id, "operation_id")?;
        let identity = Self {
            operation_id_digest: domain_separated_sha256(
                OPERATION_ID_DIGEST_DOMAIN,
                &[operation_id.as_bytes()],
            ),
            memory_space_id: memory_space_id.into(),
            mounted_subject_id: mounted_subject_id.into(),
            actor_subject_id: actor_subject_id.into(),
            operation_kind,
        };
        identity.validate_contract()?;
        Ok(identity)
    }

    pub fn operation_id_digest(&self) -> &str {
        &self.operation_id_digest
    }

    pub fn memory_space_id(&self) -> &str {
        &self.memory_space_id
    }

    pub fn mounted_subject_id(&self) -> &str {
        &self.mounted_subject_id
    }

    pub fn actor_subject_id(&self) -> &str {
        &self.actor_subject_id
    }

    pub fn operation_kind(&self) -> MemoryMutationOperationKind {
        self.operation_kind
    }

    pub fn storage_key(&self) -> String {
        let operation_kind = self.operation_kind.canonical_key();
        domain_separated_sha256(
            OPERATION_STORAGE_KEY_DOMAIN,
            &[
                self.memory_space_id.as_bytes(),
                self.mounted_subject_id.as_bytes(),
                self.actor_subject_id.as_bytes(),
                operation_kind.as_bytes(),
                self.operation_id_digest.as_bytes(),
            ],
        )
    }

    pub fn validate_contract(&self) -> Result<()> {
        validate_sha256_digest(&self.operation_id_digest, "operation_id_digest")?;
        validate_component(&self.memory_space_id, "memory_space_id")?;
        validate_component(&self.mounted_subject_id, "mounted_subject_id")?;
        validate_component(&self.actor_subject_id, "actor_subject_id")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "plane", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryMutationOperationKind {
    Write,
    LongTermControl { operation: LongTermControlOperation },
}

impl MemoryMutationOperationKind {
    fn canonical_key(self) -> String {
        match self {
            Self::Write => "write".to_string(),
            Self::LongTermControl { operation } => {
                format!("long_term_control:{}", operation.as_str())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMutationEffect {
    Changed,
    Noop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryMutationReplayDecision {
    Replay,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationReceipt {
    pub schema_version: u32,
    pub identity: MemoryMutationOperationIdentity,
    pub intent_digest: String,
    pub effect_plan_digest: String,
    pub transaction_id: String,
    pub effect: MemoryMutationEffect,
    pub changed_count: u64,
    pub audit_record_id: String,
    pub committed_at_unix_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationAuditRecord {
    pub schema_version: u32,
    pub audit_record_id: String,
    pub identity: MemoryMutationOperationIdentity,
    pub intent_digest: String,
    pub effect_plan_digest: String,
    pub transaction_id: String,
    pub effect: MemoryMutationEffect,
    pub changed_count: u64,
    pub actor_subject_id: String,
    pub committed_at_unix_secs: u64,
}

impl MemoryMutationAuditRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identity: MemoryMutationOperationIdentity,
        intent_digest: impl Into<String>,
        effect_plan_digest: impl Into<String>,
        transaction_id: impl Into<String>,
        effect: MemoryMutationEffect,
        changed_count: usize,
        actor_subject_id: impl Into<String>,
        committed_at_unix_secs: u64,
    ) -> Result<Self> {
        let changed_count = u64::try_from(changed_count).map_err(|_| {
            Error::invalid_input(
                "memory_mutation_audit_record",
                "changed_count exceeds the durable receipt width",
            )
        })?;
        let record = Self {
            schema_version: MEMORY_MUTATION_RECEIPT_SCHEMA_VERSION,
            audit_record_id: identity.storage_key(),
            identity,
            intent_digest: intent_digest.into(),
            effect_plan_digest: effect_plan_digest.into(),
            transaction_id: transaction_id.into(),
            effect,
            changed_count,
            actor_subject_id: actor_subject_id.into(),
            committed_at_unix_secs,
        };
        record.validate_contract()?;
        Ok(record)
    }

    pub fn validate_contract(&self) -> Result<()> {
        let receipt = MemoryMutationReceipt {
            schema_version: self.schema_version,
            identity: self.identity.clone(),
            intent_digest: self.intent_digest.clone(),
            effect_plan_digest: self.effect_plan_digest.clone(),
            transaction_id: self.transaction_id.clone(),
            effect: self.effect,
            changed_count: self.changed_count,
            audit_record_id: self.audit_record_id.clone(),
            committed_at_unix_secs: self.committed_at_unix_secs,
        };
        receipt.validate_contract()?;
        validate_component(&self.actor_subject_id, "actor_subject_id")?;
        if self.audit_record_id != self.identity.storage_key() {
            return Err(Error::invalid_input(
                "memory_mutation_audit_record",
                "audit_record_id must match the canonical operation storage key",
            ));
        }
        if self.actor_subject_id != self.identity.actor_subject_id {
            return Err(Error::invalid_input(
                "memory_mutation_audit_record",
                "audit actor must match the operation identity actor",
            ));
        }
        Ok(())
    }
}

impl MemoryMutationReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identity: MemoryMutationOperationIdentity,
        intent_digest: impl Into<String>,
        effect_plan_digest: impl Into<String>,
        transaction_id: impl Into<String>,
        effect: MemoryMutationEffect,
        changed_count: usize,
        committed_at_unix_secs: u64,
    ) -> Result<Self> {
        let changed_count = u64::try_from(changed_count).map_err(|_| {
            Error::invalid_input(
                "memory_mutation_receipt",
                "changed_count exceeds the durable receipt width",
            )
        })?;
        let receipt = Self {
            schema_version: MEMORY_MUTATION_RECEIPT_SCHEMA_VERSION,
            audit_record_id: identity.storage_key(),
            identity,
            intent_digest: intent_digest.into(),
            effect_plan_digest: effect_plan_digest.into(),
            transaction_id: transaction_id.into(),
            effect,
            changed_count,
            committed_at_unix_secs,
        };
        receipt.validate_contract()?;
        Ok(receipt)
    }

    pub fn classify_replay(
        &self,
        identity: &MemoryMutationOperationIdentity,
        intent_digest: &str,
    ) -> Result<MemoryMutationReplayDecision> {
        self.validate_contract()?;
        identity.validate_contract()?;
        validate_sha256_digest(intent_digest, "intent_digest")?;
        if &self.identity != identity || self.intent_digest != intent_digest {
            return Err(Error::conflict(
                "memory_mutation_receipt_replay",
                "operation identity is already committed for a different mutation intent",
            ));
        }
        Ok(MemoryMutationReplayDecision::Replay)
    }

    pub fn validate_contract(&self) -> Result<()> {
        if self.schema_version != MEMORY_MUTATION_RECEIPT_SCHEMA_VERSION {
            return Err(Error::invalid_input(
                "memory_mutation_receipt",
                "unsupported mutation receipt schema version",
            ));
        }
        self.identity.validate_contract()?;
        validate_sha256_digest(&self.intent_digest, "intent_digest")?;
        validate_sha256_digest(&self.effect_plan_digest, "effect_plan_digest")?;
        validate_sha256_digest(&self.transaction_id, "transaction_id")?;
        if self.audit_record_id != self.identity.storage_key() {
            return Err(Error::invalid_input(
                "memory_mutation_receipt",
                "audit_record_id must match the canonical operation storage key",
            ));
        }
        if self.committed_at_unix_secs == 0 {
            return Err(Error::invalid_input(
                "memory_mutation_receipt",
                "committed_at_unix_secs must be greater than zero",
            ));
        }
        match (self.effect, self.changed_count) {
            (MemoryMutationEffect::Changed, 0) | (MemoryMutationEffect::Noop, 1..) => {
                Err(Error::invalid_input(
                    "memory_mutation_receipt",
                    "mutation effect and changed_count disagree",
                ))
            }
            _ => Ok(()),
        }
    }
}

fn validate_component(value: &str, field: &'static str) -> Result<()> {
    if value.is_empty()
        || value != value.trim()
        || value.len() > MAX_OPERATION_COMPONENT_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(Error::invalid_input(
            "memory_mutation_operation_identity",
            format!("{field} must be a non-empty canonical component"),
        ));
    }
    Ok(())
}

fn validate_sha256_digest(value: &str, field: &'static str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(Error::invalid_input(
            "memory_mutation_receipt",
            format!("{field} must be a canonical sha256 digest"),
        ));
    };
    if hex.len() != 64
        || !hex
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(Error::invalid_input(
            "memory_mutation_receipt",
            format!("{field} must be a canonical sha256 digest"),
        ));
    }
    Ok(())
}

fn domain_separated_sha256(domain: &str, fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain.len().to_be_bytes());
    hasher.update(domain.as_bytes());
    for field in fields {
        hasher.update(field.len().to_be_bytes());
        hasher.update(field);
    }
    format!("sha256:{:x}", hasher.finalize())
}
