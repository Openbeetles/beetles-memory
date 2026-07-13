#![allow(dead_code)]

use std::sync::Arc;

use bm_core::llm::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy,
    ToolSpec,
};
use bm_core::memory::LongTermMemoryKind;
use bm_core::platform::ResponseBody;
use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, LongTermMemoryDraft, MemoryCapabilityPolicy,
    MemoryClock, MemoryIdentity, MemoryPrivacyClass, MemoryPrivacyPolicy, MemoryRuntime,
    MemoryScope, MemoryStoreHandle, MemoryWriteRequest, NoopMemoryAuditSink,
    ParsedLongTermMemoryExtraction, ProfileId, Result, RuntimeBudgetReport, StoreBackendConfig,
    SubjectDescriptor, SubjectRegistry, SubjectScopedRuntime,
};

struct FixedMemoryClock {
    now_secs: u64,
}

impl FixedMemoryClock {
    fn new(now_secs: u64) -> Self {
        Self { now_secs }
    }
}

impl MemoryClock for FixedMemoryClock {
    fn now_secs(&self) -> u64 {
        self.now_secs
    }
}

pub fn empty_store_platform(profile: ProfileId) -> MemoryStoreHandle {
    MemoryStoreHandle::open_in_memory(StoreBackendConfig::in_memory(profile).expect("store config"))
        .expect("store platform")
}

pub fn seeded_store_platform(profile: ProfileId) -> MemoryStoreHandle {
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform.clone(), profile);
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "release safety".to_string(),
                    content: "Verify release artifacts before publishing.".to_string(),
                    keywords: vec!["release".to_string(), "artifact".to_string()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("chat-1".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["seeded sdk test".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed long-term memory");
    platform
}

pub fn test_runtime_with_scope(
    platform: MemoryStoreHandle,
    profile: ProfileId,
    channel: &str,
    chat_id: &str,
) -> MemoryRuntime {
    test_runtime_with_identity_scope(
        platform,
        profile,
        "agent-main",
        "owner-default",
        channel,
        chat_id,
    )
}

pub fn test_runtime_with_identity_scope(
    platform: MemoryStoreHandle,
    profile: ProfileId,
    agent_id: &str,
    owner_id: &str,
    channel: &str,
    chat_id: &str,
) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, owner_id).expect("identity"))
        .scope(MemoryScope::new(channel, chat_id).expect("scope"))
        .profile(profile)
        .store(platform)
        .clock(Arc::new(FixedMemoryClock::new(1_800_000_000)))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime")
}

pub fn test_runtime_with_scope_and_subject(
    platform: MemoryStoreHandle,
    profile: ProfileId,
    channel: &str,
    chat_id: &str,
    subject_id: &str,
) -> MemoryRuntime {
    test_runtime_with_identity_scope_and_subject(
        platform,
        profile,
        "agent-main",
        "owner-default",
        subject_id,
        channel,
        chat_id,
    )
}

pub fn test_runtime_with_identity_scope_and_subject(
    platform: MemoryStoreHandle,
    profile: ProfileId,
    agent_id: &str,
    owner_id: &str,
    subject_id: &str,
    channel: &str,
    chat_id: &str,
) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, owner_id).expect("identity"))
        .subject_id(subject_id)
        .scope(MemoryScope::new(channel, chat_id).expect("scope"))
        .profile(profile)
        .store(platform)
        .clock(Arc::new(FixedMemoryClock::new(1_800_000_000)))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime")
}

pub fn test_runtime_with_scope_subject_and_budget(
    platform: MemoryStoreHandle,
    profile: ProfileId,
    channel: &str,
    chat_id: &str,
    subject_id: &str,
    runtime_budget: RuntimeBudgetReport,
) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .subject_id(subject_id)
        .scope(MemoryScope::new(channel, chat_id).expect("scope"))
        .profile(profile)
        .store(platform)
        .runtime_budget(runtime_budget)
        .clock(Arc::new(FixedMemoryClock::new(1_800_000_000)))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime")
}

pub fn test_runtime_with_delegated_actor(
    platform: MemoryStoreHandle,
    profile: ProfileId,
    mounted_agent_id: &str,
    actor_subject_id: &str,
    chat_id: &str,
) -> MemoryRuntime {
    let owner_id = "owner-default";
    let mounted_subject_id = default_agent_subject_id(mounted_agent_id);
    let mut registry =
        SubjectRegistry::single_agent_default(owner_id, mounted_agent_id).expect("registry");
    if registry.subject(actor_subject_id).is_none() {
        registry
            .upsert_subject(SubjectDescriptor::agent_persona(
                actor_subject_id,
                "Delegated Actor",
            ))
            .expect("delegated actor subject");
    }
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(mounted_agent_id, owner_id).expect("identity"))
        .scope(MemoryScope::new("llm.gateway", chat_id).expect("scope"))
        .profile(profile)
        .store(platform)
        .subject_registry(registry)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id(owner_id),
            mounted_subject_id,
            actor_subject_id: actor_subject_id.to_string(),
            agent_id: mounted_agent_id.to_string(),
            relationship_scope: None,
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .clock(Arc::new(FixedMemoryClock::new(1_800_000_000)))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("delegated runtime")
}

pub fn test_runtime_with_chat(
    platform: MemoryStoreHandle,
    profile: ProfileId,
    chat_id: &str,
) -> MemoryRuntime {
    test_runtime_with_scope(platform, profile, "local", chat_id)
}

pub fn test_runtime(platform: MemoryStoreHandle, profile: ProfileId) -> MemoryRuntime {
    test_runtime_with_chat(platform, profile, "chat-1")
}

#[derive(Default)]
pub struct StaticHttpClient;

impl LlmHttpClient for StaticHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

pub struct StaticLlmClient {
    content: String,
}

impl StaticLlmClient {
    pub fn summary_response(content: &str) -> Self {
        Self {
            content: content.to_string(),
        }
    }
}

impl LlmClient for StaticLlmClient {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: self.content.clone(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}
