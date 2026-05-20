#![allow(dead_code)]

use std::sync::Arc;

use bm_core::llm::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy,
    ToolSpec,
};
use bm_core::memory::LongTermMemoryKind;
use bm_core::platform::{Platform as _, ResponseBody};
use bm_sdk::{
    LongTermMemoryDraft, MemoryCapabilityPolicy, MemoryClock, MemoryIdentity, MemoryPrivacyPolicy,
    MemoryRuntime, MemoryScope, NoopMemoryAuditSink, ProfileId, Result, StoreBackendConfig,
    StorePlatform,
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

pub fn empty_store_platform(profile: ProfileId) -> StorePlatform {
    StorePlatform::open_in_memory(StoreBackendConfig::in_memory(profile).expect("store config"))
        .expect("store platform")
}

pub fn seeded_store_platform(profile: ProfileId) -> StorePlatform {
    let platform = empty_store_platform(profile);
    platform
        .long_term_memory_store()
        .upsert_many(
            &[LongTermMemoryDraft {
                kind: LongTermMemoryKind::Project,
                topic: "release safety".to_string(),
                content: "Verify release artifacts before publishing.".to_string(),
                keywords: vec!["release".to_string(), "artifact".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: None,
                source_scope: None,
                confidence: None,
                freshness: None,
                stale_hint: None,
                supporting_citations: vec!["seeded sdk test".to_string()],
                evidence_count: Some(1),
                observed_at: Some(1_800_000_000),
                last_confirmed_at: Some(1_800_000_000),
                source_revision: Some(1),
            }],
            1_800_000_000,
        )
        .expect("seed long-term memory");
    platform
}

pub fn test_runtime(platform: StorePlatform, profile: ProfileId) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(profile)
        .store_platform(platform)
        .clock(Arc::new(FixedMemoryClock::new(1_800_000_000)))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime")
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
