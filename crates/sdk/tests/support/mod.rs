#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use bm_core::llm::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy,
    ToolSpec,
};
use bm_core::memory::{
    AutonomyStrategy, ContinuityCapsule, ContinuityCapsuleDraft, ContinuityCapsuleScopeKind,
    ContinuityCapsuleWriteOutcome, CoreRevisionLedger, ExecutionState, FeltSignificance,
    InnerConflict, InnerLife, LongTermMemoryConfidence, LongTermMemoryEntry,
    LongTermMemoryExtractionState, LongTermMemoryFreshness, LongTermMemoryKind,
    LongTermMemoryQuery, LongTermMemorySlot, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryStaleHint, LongTermMemoryStore, MentalPrivacyState, OuterVoice,
    PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenDocRecord, RelationshipConstitution,
    RelationshipPortfolio, RelationshipTopology, SelfAuthoredCore, SelfContinuity, SelfModel,
    SessionMessage, TemperamentContinuity, TurnContinuityEvidence, TurnLedger, WorldSense,
};
use bm_core::platform::ResponseBody;
use bm_core::reminder::ReminderItem;
use bm_core::task::{TaskItem, TaskQuery};
use bm_core::task_execution::{TaskArtifactRecord, TaskLearningRecord, TaskRunRecord};
use bm_sdk::{
    ActiveWorkRecord, ActiveWorkStore, AutonomyStrategyStore, ContinuityCapsuleStore,
    CoreRevisionLedgerStore, ExecutionStateStore, FeltSignificanceStore, InnerConflictStore,
    InnerLifeStore, LongTermMemoryExtractionStateStore, MemoryCapabilityPolicy, MemoryClock,
    MemoryIdentity, MemoryPrivacyPolicy, MemoryRuntime, MemoryScope, MemoryStore,
    MentalPrivacyStore, NoopMemoryAuditSink, OuterVoiceStore, Platform, PlatformMemorySystemKind,
    PrivateDocStore, PrivateGardenStore, ProfileId, RelationshipConstitutionStore,
    RelationshipPortfolioStore, RelationshipTopologyStore, RemindAtStore, Result,
    SelfAuthoredCoreStore, SelfContinuityStore, SelfModelStore, SessionStore, SessionSummaryStore,
    SkillMetaStore, SkillStorage, StateFs, TaskArtifactStore, TaskLearningStore, TaskRunStore,
    TaskStore, TemperamentContinuityStore, TurnContinuityEvidenceStore, TurnLedgerStore,
    WorldSenseStore,
};

#[derive(Clone)]
pub struct HostMemoryPlatform {
    stores: Arc<HostMemoryStores>,
}

impl Default for HostMemoryPlatform {
    fn default() -> Self {
        Self {
            stores: Arc::new(HostMemoryStores::default()),
        }
    }
}

impl HostMemoryPlatform {
    pub fn seeded() -> Self {
        let platform = Self::default();
        platform.stores.long_term.lock().unwrap().insert(
            "ltm-project-release-safety".to_string(),
            LongTermMemoryEntry {
                id: "ltm-project-release-safety".to_string(),
                kind: LongTermMemoryKind::Project,
                topic: "release safety".to_string(),
                content: "Verify release artifacts before publishing.".to_string(),
                keywords: vec!["release".to_string(), "artifact".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: LongTermMemorySourceType::Conversation,
                source_scope: LongTermMemorySourceScope::Chat,
                confidence: LongTermMemoryConfidence::High,
                freshness: LongTermMemoryFreshness::Stable,
                stale_hint: LongTermMemoryStaleHint::None,
                supporting_citations: vec!["seeded sdk test".to_string()],
                evidence_count: 1,
                created_at: 1_800_000_000,
                updated_at: 1_800_000_000,
                observed_at: 1_800_000_000,
                last_confirmed_at: 1_800_000_000,
                source_revision: 1,
                last_used_at: 0,
            },
        );
        platform
    }
}

impl Platform for HostMemoryPlatform {
    fn memory_system_kind(&self) -> PlatformMemorySystemKind {
        PlatformMemorySystemKind::SdkEmbedded
    }

    fn state_fs(&self) -> Arc<dyn StateFs> {
        self.stores.clone()
    }

    fn skill_storage(&self) -> Arc<dyn SkillStorage> {
        self.stores.clone()
    }

    fn skill_meta_store(&self) -> Arc<dyn SkillMetaStore> {
        self.stores.clone()
    }

    fn active_work_store(&self) -> Arc<dyn ActiveWorkStore> {
        self.stores.clone()
    }

    fn memory_store(&self) -> Arc<dyn MemoryStore> {
        self.stores.clone()
    }

    fn session_store(&self) -> Arc<dyn SessionStore> {
        self.stores.clone()
    }

    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore> {
        self.stores.clone()
    }

    fn long_term_memory_store(&self) -> Arc<dyn LongTermMemoryStore> {
        self.stores.clone()
    }

    fn long_term_memory_extraction_state_store(
        &self,
    ) -> Arc<dyn LongTermMemoryExtractionStateStore> {
        self.stores.clone()
    }

    fn continuity_capsule_store(&self) -> Arc<dyn ContinuityCapsuleStore> {
        self.stores.clone()
    }

    fn turn_ledger_store(&self) -> Arc<dyn TurnLedgerStore> {
        self.stores.clone()
    }

    fn self_model_store(&self) -> Arc<dyn SelfModelStore> {
        self.stores.clone()
    }

    fn self_authored_core_store(&self) -> Arc<dyn SelfAuthoredCoreStore> {
        self.stores.clone()
    }

    fn core_revision_ledger_store(&self) -> Arc<dyn CoreRevisionLedgerStore> {
        self.stores.clone()
    }

    fn self_continuity_store(&self) -> Arc<dyn SelfContinuityStore> {
        self.stores.clone()
    }

    fn relationship_constitution_store(&self) -> Arc<dyn RelationshipConstitutionStore> {
        self.stores.clone()
    }

    fn relationship_portfolio_store(&self) -> Arc<dyn RelationshipPortfolioStore> {
        self.stores.clone()
    }

    fn relationship_topology_store(&self) -> Arc<dyn RelationshipTopologyStore> {
        self.stores.clone()
    }

    fn execution_state_store(&self) -> Arc<dyn ExecutionStateStore> {
        self.stores.clone()
    }

    fn world_sense_store(&self) -> Arc<dyn WorldSenseStore> {
        self.stores.clone()
    }

    fn outer_voice_store(&self) -> Arc<dyn OuterVoiceStore> {
        self.stores.clone()
    }

    fn autonomy_strategy_store(&self) -> Arc<dyn AutonomyStrategyStore> {
        self.stores.clone()
    }

    fn inner_life_store(&self) -> Arc<dyn InnerLifeStore> {
        self.stores.clone()
    }

    fn felt_significance_store(&self) -> Arc<dyn FeltSignificanceStore> {
        self.stores.clone()
    }

    fn temperament_continuity_store(&self) -> Arc<dyn TemperamentContinuityStore> {
        self.stores.clone()
    }

    fn inner_conflict_store(&self) -> Arc<dyn InnerConflictStore> {
        self.stores.clone()
    }

    fn mental_privacy_store(&self) -> Arc<dyn MentalPrivacyStore> {
        self.stores.clone()
    }

    fn private_doc_store(&self) -> Arc<dyn PrivateDocStore> {
        self.stores.clone()
    }

    fn private_garden_store(&self) -> Arc<dyn PrivateGardenStore> {
        self.stores.clone()
    }

    fn turn_continuity_evidence_store(&self) -> Arc<dyn TurnContinuityEvidenceStore> {
        self.stores.clone()
    }

    fn remind_at_store(&self) -> Arc<dyn RemindAtStore> {
        self.stores.clone()
    }

    fn task_store(&self) -> Arc<dyn TaskStore> {
        self.stores.clone()
    }

    fn task_run_store(&self) -> Arc<dyn TaskRunStore> {
        self.stores.clone()
    }

    fn task_artifact_store(&self) -> Arc<dyn TaskArtifactStore> {
        self.stores.clone()
    }

    fn task_learning_store(&self) -> Arc<dyn TaskLearningStore> {
        self.stores.clone()
    }
}

#[derive(Default)]
struct HostMemoryStores {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    skills: Mutex<BTreeMap<String, Vec<u8>>>,
    skill_order: Mutex<Vec<String>>,
    skill_disabled: Mutex<BTreeSet<String>>,
    sessions: Mutex<BTreeMap<String, Vec<SessionMessage>>>,
    memory: Mutex<String>,
    daily_notes: Mutex<BTreeMap<String, String>>,
    summaries: Mutex<BTreeMap<String, (String, usize)>>,
    long_term: Mutex<BTreeMap<String, LongTermMemoryEntry>>,
    extraction_states: Mutex<BTreeMap<String, LongTermMemoryExtractionState>>,
}

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

pub fn test_runtime(platform: Arc<dyn Platform>, profile: ProfileId) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(profile)
        .platform(platform)
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

impl StateFs for HostMemoryStores {
    fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.files.lock().unwrap().get(rel_path).cloned())
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(rel_path.to_string(), data.to_vec());
        Ok(())
    }

    fn remove(&self, rel_path: &str) -> Result<()> {
        self.files.lock().unwrap().remove(rel_path);
        Ok(())
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
        let prefix = rel_path.trim_end_matches('/');
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let mut names = self
            .files
            .lock()
            .unwrap()
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix).map(ToString::to_string))
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }
}

impl SkillStorage for HostMemoryStores {
    fn list_names(&self) -> Result<Vec<String>> {
        Ok(self.skills.lock().unwrap().keys().cloned().collect())
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        Ok(self
            .skills
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_default())
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        self.skills
            .lock()
            .unwrap()
            .insert(name.to_string(), content.to_vec());
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        self.skills.lock().unwrap().remove(name);
        Ok(())
    }
}

impl SkillMetaStore for HostMemoryStores {
    fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
        Ok((
            self.skill_order.lock().unwrap().clone(),
            self.skill_disabled
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .collect(),
        ))
    }

    fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
        *self.skill_order.lock().unwrap() = order.to_vec();
        *self.skill_disabled.lock().unwrap() = disabled.iter().cloned().collect();
        Ok(())
    }
}

impl ActiveWorkStore for HostMemoryStores {
    fn get(&self, _chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
        Ok(None)
    }

    fn set(&self, _chat_id: &str, _record: &ActiveWorkRecord) -> Result<()> {
        Ok(())
    }
}

impl MemoryStore for HostMemoryStores {
    fn get_memory(&self) -> Result<String> {
        Ok(self.memory.lock().unwrap().clone())
    }

    fn set_memory(&self, content: &str) -> Result<()> {
        *self.memory.lock().unwrap() = content.to_string();
        Ok(())
    }

    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
        let mut names = self
            .daily_notes
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        names.sort_by(|left, right| right.cmp(left));
        names.truncate(recent_n);
        Ok(names)
    }

    fn get_daily_note(&self, name: &str) -> Result<String> {
        Ok(self
            .daily_notes
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_default())
    }

    fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
        self.daily_notes
            .lock()
            .unwrap()
            .insert(name.to_string(), content.to_string());
        Ok(())
    }
}

impl SessionStore for HostMemoryStores {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.sessions
            .lock()
            .unwrap()
            .entry(chat_id.to_string())
            .or_default()
            .push(SessionMessage {
                role: role.to_string(),
                content: content.to_string(),
            });
        Ok(())
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let messages = self
            .sessions
            .lock()
            .unwrap()
            .get(chat_id)
            .cloned()
            .unwrap_or_default();
        Ok(messages
            .into_iter()
            .rev()
            .take(n)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.sessions.lock().unwrap().remove(chat_id);
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        Ok(self.sessions.lock().unwrap().keys().cloned().collect())
    }
}

impl SessionSummaryStore for HostMemoryStores {
    fn get(&self, chat_id: &str) -> Result<Option<String>> {
        Ok(self
            .summaries
            .lock()
            .unwrap()
            .get(chat_id)
            .map(|(summary, _)| summary.clone()))
    }

    fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
        self.summaries
            .lock()
            .unwrap()
            .insert(chat_id.to_string(), (summary.to_string(), 0));
        Ok(())
    }

    fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
        self.summaries
            .lock()
            .unwrap()
            .insert(chat_id.to_string(), (summary.to_string(), message_count));
        Ok(())
    }

    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        Ok(self.summaries.lock().unwrap().get(chat_id).cloned())
    }
}

impl LongTermMemoryStore for HostMemoryStores {
    fn upsert_many(
        &self,
        drafts: &[bm_core::memory::LongTermMemoryDraft],
        now_secs: u64,
    ) -> Result<usize> {
        let mut changed = 0usize;
        let mut records = self.long_term.lock().unwrap();
        for draft in drafts {
            let Some(draft) = draft.normalized() else {
                continue;
            };
            let Some(id) = draft.stable_id() else {
                continue;
            };
            records.insert(
                id.clone(),
                LongTermMemoryEntry {
                    id,
                    kind: draft.kind,
                    topic: draft.topic,
                    content: draft.content,
                    keywords: draft.keywords,
                    source_chat_id: draft.source_chat_id,
                    source_type: draft.source_type.unwrap_or_default(),
                    source_scope: draft.source_scope.unwrap_or_default(),
                    confidence: draft.confidence.unwrap_or_default(),
                    freshness: draft.freshness.unwrap_or(LongTermMemoryFreshness::Stable),
                    stale_hint: draft.stale_hint.unwrap_or_default(),
                    supporting_citations: draft.supporting_citations,
                    evidence_count: draft.evidence_count.unwrap_or(1),
                    created_at: now_secs,
                    updated_at: now_secs,
                    observed_at: draft.observed_at.unwrap_or(now_secs),
                    last_confirmed_at: draft.last_confirmed_at.unwrap_or(now_secs),
                    source_revision: draft.source_revision.unwrap_or(1),
                    last_used_at: 0,
                },
            );
            changed += 1;
        }
        Ok(changed)
    }

    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let normalized_query = query.to_ascii_lowercase();
        let mut records = self
            .long_term
            .lock()
            .unwrap()
            .values()
            .filter(|entry| {
                source_chat_id
                    .map(|chat_id| entry.source_chat_id.as_deref() == Some(chat_id))
                    .unwrap_or(true)
                    && (entry.topic.to_ascii_lowercase().contains(&normalized_query)
                        || entry
                            .content
                            .to_ascii_lowercase()
                            .contains(&normalized_query))
            })
            .cloned()
            .collect::<Vec<_>>();
        records.truncate(limit);
        Ok(records)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        Ok(self.long_term.lock().unwrap().get(id).cloned())
    }

    fn query(&self, query: &LongTermMemoryQuery) -> Result<Vec<LongTermMemoryEntry>> {
        let mut records = self
            .long_term
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let query = query.normalized();
        if let Some(kind) = query.kind {
            records.retain(|entry| entry.kind == kind);
        }
        if let Some(topic) = query.topic {
            records.retain(|entry| entry.topic.contains(&topic));
        }
        records.truncate(query.limit);
        Ok(records)
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut records = self
            .long_term
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        records.truncate(limit);
        Ok(records)
    }

    fn delete(&self, id: &str) -> Result<bool> {
        Ok(self.long_term.lock().unwrap().remove(id).is_some())
    }

    fn delete_slot(&self, slot: &LongTermMemorySlot) -> Result<bool> {
        let Some(id) = slot.stable_id() else {
            return Ok(false);
        };
        LongTermMemoryStore::delete(self, &id)
    }

    fn count(&self) -> Result<usize> {
        Ok(self.long_term.lock().unwrap().len())
    }
}

impl LongTermMemoryExtractionStateStore for HostMemoryStores {
    fn get(&self, chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
        Ok(self.extraction_states.lock().unwrap().get(chat_id).cloned())
    }

    fn set(&self, chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()> {
        self.extraction_states
            .lock()
            .unwrap()
            .insert(chat_id.to_string(), state.clone());
        Ok(())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.extraction_states.lock().unwrap().remove(chat_id);
        Ok(())
    }
}

impl ContinuityCapsuleStore for HostMemoryStores {
    fn upsert_many(
        &self,
        drafts: &[ContinuityCapsuleDraft],
        _now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome> {
        Ok(ContinuityCapsuleWriteOutcome {
            considered: drafts.len(),
            upserted: drafts.len(),
            total: drafts.len(),
            ..ContinuityCapsuleWriteOutcome::default()
        })
    }

    fn get(&self, _capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
        Ok(None)
    }

    fn list(&self, _limit: usize) -> Result<Vec<ContinuityCapsule>> {
        Ok(Vec::new())
    }

    fn count(&self) -> Result<usize> {
        Ok(0)
    }

    fn list_for_scope(
        &self,
        _scope_kind: ContinuityCapsuleScopeKind,
        _scope_id: &str,
        _limit: usize,
    ) -> Result<Vec<ContinuityCapsule>> {
        Ok(Vec::new())
    }
}

impl TurnLedgerStore for HostMemoryStores {
    fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
        Ok(None)
    }

    fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
        Ok(())
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }
}

macro_rules! impl_optional_store {
    ($trait_name:ident, $value_type:ty) => {
        impl $trait_name for HostMemoryStores {
            fn get(&self, _scope_id: &str) -> Result<Option<$value_type>> {
                Ok(None)
            }

            fn set(&self, _scope_id: &str, _value: &$value_type) -> Result<()> {
                Ok(())
            }

            fn clear(&self, _scope_id: &str) -> Result<()> {
                Ok(())
            }
        }
    };
}

impl_optional_store!(SelfModelStore, SelfModel);
impl_optional_store!(SelfAuthoredCoreStore, SelfAuthoredCore);
impl_optional_store!(CoreRevisionLedgerStore, CoreRevisionLedger);
impl_optional_store!(SelfContinuityStore, SelfContinuity);
impl_optional_store!(RelationshipConstitutionStore, RelationshipConstitution);
impl_optional_store!(RelationshipPortfolioStore, RelationshipPortfolio);
impl_optional_store!(RelationshipTopologyStore, RelationshipTopology);
impl_optional_store!(ExecutionStateStore, ExecutionState);
impl_optional_store!(WorldSenseStore, WorldSense);
impl_optional_store!(OuterVoiceStore, OuterVoice);
impl_optional_store!(AutonomyStrategyStore, AutonomyStrategy);
impl_optional_store!(InnerLifeStore, InnerLife);
impl_optional_store!(FeltSignificanceStore, FeltSignificance);
impl_optional_store!(TemperamentContinuityStore, TemperamentContinuity);
impl_optional_store!(InnerConflictStore, InnerConflict);
impl_optional_store!(PrivateDocStore, PrivateDocWorkspace);
impl_optional_store!(MentalPrivacyStore, MentalPrivacyState);

impl PrivateGardenStore for HostMemoryStores {
    fn list(&self, _chat_id: &str, _limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
        Ok(Vec::new())
    }

    fn read(&self, _chat_id: &str, _doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
        Ok(None)
    }

    fn write(
        &self,
        _chat_id: &str,
        _doc_path: &str,
        _content: &str,
        _now_secs: u64,
    ) -> Result<PrivateGardenDocRecord> {
        Err(bm_sdk::Error::storage_stage("test_private_garden_write"))
    }

    fn move_doc(
        &self,
        _chat_id: &str,
        _from_path: &str,
        _to_path: &str,
        _now_secs: u64,
    ) -> Result<Option<PrivateGardenDocRecord>> {
        Ok(None)
    }

    fn delete(&self, _chat_id: &str, _doc_path: &str) -> Result<bool> {
        Ok(false)
    }
}

impl TurnContinuityEvidenceStore for HostMemoryStores {
    fn append(&self, _chat_id: &str, _evidence: &TurnContinuityEvidence) -> Result<()> {
        Ok(())
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    fn list_recent(&self, _chat_id: &str, _limit: usize) -> Result<Vec<TurnContinuityEvidence>> {
        Ok(Vec::new())
    }
}

impl RemindAtStore for HostMemoryStores {
    fn get(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<Option<ReminderItem>> {
        Ok(None)
    }

    fn upsert(&self, _reminder: &ReminderItem) -> Result<()> {
        Ok(())
    }

    fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
        Ok(false)
    }

    fn list_due(&self, _now_unix_secs: u64, _limit: usize) -> Result<Vec<ReminderItem>> {
        Ok(Vec::new())
    }

    fn delete_due(&self, _reminder: &ReminderItem) -> Result<bool> {
        Ok(false)
    }

    fn list_upcoming(
        &self,
        _channel: &str,
        _chat_id: &str,
        _now_unix_secs: u64,
        _limit: usize,
    ) -> Result<Vec<ReminderItem>> {
        Ok(Vec::new())
    }
}

impl TaskStore for HostMemoryStores {
    fn list(&self, _channel: &str, _chat_id: &str, _query: TaskQuery) -> Result<Vec<TaskItem>> {
        Ok(Vec::new())
    }

    fn get(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<Option<TaskItem>> {
        Ok(None)
    }

    fn upsert(&self, _task: &TaskItem) -> Result<()> {
        Ok(())
    }

    fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
        Ok(false)
    }

    fn list_due_unnotified(&self, _now_unix_secs: u64, _limit: usize) -> Result<Vec<TaskItem>> {
        Ok(Vec::new())
    }

    fn mark_due_notified(&self, _task: &TaskItem, _notified_at_unix_secs: u64) -> Result<bool> {
        Ok(false)
    }
}

impl TaskRunStore for HostMemoryStores {
    fn get(&self, _run_id: &str) -> Result<Option<TaskRunRecord>> {
        Ok(None)
    }

    fn upsert(&self, _record: &TaskRunRecord) -> Result<()> {
        Ok(())
    }

    fn list_recent(&self, _limit: usize) -> Result<Vec<TaskRunRecord>> {
        Ok(Vec::new())
    }

    fn list_active_for_chat(
        &self,
        _channel: &str,
        _chat_id: &str,
        _limit: usize,
    ) -> Result<Vec<TaskRunRecord>> {
        Ok(Vec::new())
    }
}

impl TaskArtifactStore for HostMemoryStores {
    fn put(&self, _record: &TaskArtifactRecord) -> Result<()> {
        Ok(())
    }

    fn list_for_run(&self, _run_id: &str, _limit: usize) -> Result<Vec<TaskArtifactRecord>> {
        Ok(Vec::new())
    }
}

impl TaskLearningStore for HostMemoryStores {
    fn get(&self, _learning_id: &str) -> Result<Option<TaskLearningRecord>> {
        Ok(None)
    }

    fn upsert(&self, _record: &TaskLearningRecord) -> Result<()> {
        Ok(())
    }

    fn list_recent(&self, _limit: usize) -> Result<Vec<TaskLearningRecord>> {
        Ok(Vec::new())
    }

    fn list_for_chat(
        &self,
        _channel: &str,
        _chat_id: &str,
        _limit: usize,
    ) -> Result<Vec<TaskLearningRecord>> {
        Ok(Vec::new())
    }

    fn list_for_run(&self, _run_id: &str, _limit: usize) -> Result<Vec<TaskLearningRecord>> {
        Ok(Vec::new())
    }
}
