#![allow(dead_code)]

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryEntry};
use bm_core::skills::{
    canonical_runtime_skill_owner_id, canonical_runtime_skill_owner_key,
    runtime_skill_scope_manifest_key, RuntimeSkillOwnerBinding, RuntimeSkillOwnerRecord,
    RuntimeSkillScopeManifest,
};
#[cfg(feature = "sqlite-store")]
use bm_sdk::nonproduction_replay_harness::SqliteStoreEngine;
use bm_sdk::nonproduction_replay_harness::{
    FileStoreEngine, StoreCapacityBudget, StorePlatform, StoreRepairReport, StoreSchemaManifest,
    RUNTIME_SKILL_RECORD_NAMESPACE, RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
};
use bm_sdk::{
    GovernedRuntimeSkillWriteInput, MemoryCapabilityPolicy, MemoryClock, MemoryIdentity,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryPrivacyPolicy, MemoryRuntime, MemoryScope, MemoryStoreHandle,
    MemoryWriteRequest, NoopMemoryAuditSink, ParsedLongTermMemoryExtraction, Result,
    RuntimeLifecycleModeInput, RuntimeSkillCreationRef, RuntimeSkillOwningScope, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub fn open_store(config: StoreBackendConfig) -> Result<StorePlatform> {
    StorePlatform::open(config)
}

pub const fn native_persistent_profile() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosEmbeddedSdk
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::ServerLinuxMemoryGateway
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        compile_error!("store contract tests require a supported persistent host target");
    }
}

pub fn open_store_in_memory(config: StoreBackendConfig) -> Result<StorePlatform> {
    if config.backend() != bm_sdk::nonproduction_replay_harness::StoreBackendKind::InMemory {
        return Err(bm_sdk::Error::config(
            "store_backend_config",
            "open_store_in_memory requires in-memory backend config",
        ));
    }
    open_store(config)
}

struct FixedMemoryClock {
    now_secs: u64,
}

impl MemoryClock for FixedMemoryClock {
    fn now_secs(&self) -> u64 {
        self.now_secs
    }
}

fn runtime_for_scope(
    platform: &StorePlatform,
    memory_space_id: &str,
    now_secs: u64,
) -> MemoryRuntime {
    let owner_id = memory_space_id
        .strip_prefix("space:")
        .filter(|owner_id| !owner_id.is_empty())
        .expect("store fixture memory space must be canonical space:<owner>");
    let mounted_subject_id = platform.config().event_scope().subject_id.clone();
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("store-contract", owner_id).expect("fixture identity"))
        .subject_id(&mounted_subject_id)
        .scope(MemoryScope::new("test", "chat-a").expect("fixture scope"))
        .store(MemoryStoreHandle::from_nonproduction_store_platform(
            platform.clone(),
        ))
        .clock(Arc::new(FixedMemoryClock { now_secs }))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("fixture runtime");
    assert_eq!(runtime.memory_space_id(), memory_space_id);
    assert_eq!(runtime.subject_id(), mounted_subject_id);
    runtime
}

pub fn seed_runtime_skill(platform: &StorePlatform, name: &str) -> RuntimeSkillOwnerRecord {
    let write = RuntimeSkillWrite {
        name: name.to_string(),
        topic: "store persistence".to_string(),
        title: "Store persistence".to_string(),
        summary: "Persist a typed runtime skill owner across reopen.".to_string(),
        content: "1. Write through the governed skill owner.\n2. Reopen and verify bytes."
            .to_string(),
        citations: vec!["store-contract:test".to_string()],
        source_chat_id: Some("chat-a".to_string()),
        observed_at: 100,
    };
    let runtime = runtime_for_scope(platform, "space:test", 100);
    let owning_scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: runtime.subject_id().to_string(),
    };
    let candidate_ref = format!("store-contract:runtime-skill:{name}");
    let verification_receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(format!("{candidate_ref}\n{}\n{}", write.title, write.content).as_bytes())
    );
    let creation_ref = RuntimeSkillCreationRef::ReplayPromotion {
        candidate_ref,
        verification_receipt_digest,
    };
    let report = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![GovernedRuntimeSkillWriteInput {
                write,
                creation_ref: creation_ref.clone(),
                privacy_class: MemoryPrivacyClass::SharedWithSubject,
            }],
            owning_scope: owning_scope.clone(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed typed runtime skill");
    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let owner_id =
        canonical_runtime_skill_owner_id(runtime.memory_space_id(), &owning_scope, &creation_ref)
            .expect("canonical runtime skill owner id");
    let owner_key =
        canonical_runtime_skill_owner_key(runtime.memory_space_id(), &owning_scope, &owner_id)
            .expect("canonical runtime skill owner key");
    let docs = platform
        .read_json_docs_by_keys(RUNTIME_SKILL_RECORD_NAMESPACE, &[owner_key])
        .expect("read exact typed runtime skill owner");
    assert_eq!(docs.len(), 1);
    let owner: RuntimeSkillOwnerRecord =
        serde_json::from_value(docs[0].value.clone()).expect("decode typed runtime skill owner");
    assert!(owner.validate_contract().accepted);

    let manifest_key = runtime_skill_scope_manifest_key(runtime.memory_space_id(), &owning_scope)
        .expect("canonical runtime skill scope manifest key");
    let manifest_docs = platform
        .read_json_docs_by_keys(RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE, &[manifest_key])
        .expect("read exact runtime skill scope manifest");
    assert_eq!(manifest_docs.len(), 1);
    let manifest: RuntimeSkillScopeManifest =
        serde_json::from_value(manifest_docs[0].value.clone())
            .expect("decode runtime skill scope manifest");
    manifest
        .validate_exact(
            runtime.memory_space_id(),
            &owning_scope,
            [RuntimeSkillOwnerBinding::from_record(&owner).expect("runtime skill owner binding")],
            platform.capacity().kv_max_entries,
        )
        .expect("validate exact runtime skill owner closure");
    owner
}

pub fn read_runtime_skill_owner(
    platform: &StorePlatform,
    physical_key: &str,
) -> RuntimeSkillOwnerRecord {
    let docs = platform
        .read_json_docs_by_keys(RUNTIME_SKILL_RECORD_NAMESPACE, &[physical_key.to_string()])
        .expect("read typed runtime skill owner by key");
    assert_eq!(docs.len(), 1);
    serde_json::from_value(docs[0].value.clone()).expect("decode typed runtime skill owner")
}

fn runtime_capacity_for_profile(profile: ProfileId) -> Result<StoreCapacityBudget> {
    open_store(StoreBackendConfig::in_memory(profile)?).map(|platform| platform.capacity())
}

pub fn open_file_engine(
    config: &StoreBackendConfig,
) -> Result<(FileStoreEngine, StoreRepairReport, StoreSchemaManifest)> {
    FileStoreEngine::open_with_capacity(config, runtime_capacity_for_profile(config.profile())?)
}

#[cfg(feature = "sqlite-store")]
pub fn open_sqlite_engine(
    config: &StoreBackendConfig,
) -> Result<(SqliteStoreEngine, StoreSchemaManifest)> {
    SqliteStoreEngine::open_with_capacity(config, runtime_capacity_for_profile(config.profile())?)
}

pub fn seed_scoped_long_term(
    platform: &StorePlatform,
    memory_space_id: &str,
    draft: &LongTermMemoryDraft,
    now_secs: u64,
) -> LongTermMemoryEntry {
    let runtime = runtime_for_scope(platform, memory_space_id, now_secs);
    let report = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![draft.clone()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed scoped long-term owner");
    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let entries = platform
        .scoped_long_term_memory_read_store(
            memory_space_id,
            &platform.config().event_scope().subject_id,
        )
        .expect("scoped long-term read store")
        .list(usize::MAX)
        .expect("list seeded long-term owners");
    entries
        .iter()
        .find(|entry| entry.content == draft.content)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "seeded long-term owner missing from {memory_space_id}; expected topic={:?} content={:?}; actual={entries:#?}",
                draft.topic, draft.content
            )
        })
}

pub fn delete_scoped_long_term(
    platform: &StorePlatform,
    memory_space_id: &str,
    entry: &LongTermMemoryEntry,
) {
    let runtime = runtime_for_scope(
        platform,
        memory_space_id,
        entry.updated_at.saturating_add(1),
    );
    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(entry.id.clone()),
            },
            reason: "store contract scoped deletion".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete scoped long-term owner");
    assert!(report.accepted);
}
