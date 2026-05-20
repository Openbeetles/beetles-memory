#![allow(dead_code)]

use std::sync::Arc;

use bm_sdk::{
    ActiveWorkStore, AutonomyStrategyStore, ContinuityCapsuleStore, CoreRevisionLedgerStore,
    ExecutionStateStore, FeltSignificanceStore, InnerConflictStore, InnerLifeStore,
    LongTermMemoryExtractionStateStore, LongTermMemoryStore, MemoryCapabilityPolicy, MemoryClock,
    MemoryIdentity, MemoryPrivacyPolicy, MemoryRuntime, MemoryScope, MemoryStore,
    MentalPrivacyStore, NoopMemoryAuditSink, OuterVoiceStore, Platform, PlatformMemorySystemKind,
    PrivateDocStore, PrivateGardenStore, ProfileId, RelationshipConstitutionStore,
    RelationshipPortfolioStore, RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreStore,
    SelfContinuityStore, SelfModelStore, SessionStore, SessionSummaryStore, SkillMetaStore,
    SkillStorage, StateFs, TaskArtifactStore, TaskLearningStore, TaskRunStore, TaskStore,
    TemperamentContinuityStore, TurnContinuityEvidenceStore, TurnLedgerStore, WorldSenseStore,
};

struct HostPlatform;

impl Platform for HostPlatform {
    fn memory_system_kind(&self) -> PlatformMemorySystemKind {
        PlatformMemorySystemKind::SdkEmbedded
    }

    fn state_fs(&self) -> Arc<dyn StateFs> {
        unimplemented!()
    }

    fn skill_storage(&self) -> Arc<dyn SkillStorage> {
        unimplemented!()
    }

    fn skill_meta_store(&self) -> Arc<dyn SkillMetaStore> {
        unimplemented!()
    }

    fn active_work_store(&self) -> Arc<dyn ActiveWorkStore> {
        unimplemented!()
    }

    fn memory_store(&self) -> Arc<dyn MemoryStore> {
        unimplemented!()
    }

    fn session_store(&self) -> Arc<dyn SessionStore> {
        unimplemented!()
    }

    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore> {
        unimplemented!()
    }

    fn long_term_memory_store(&self) -> Arc<dyn LongTermMemoryStore> {
        unimplemented!()
    }

    fn long_term_memory_extraction_state_store(
        &self,
    ) -> Arc<dyn LongTermMemoryExtractionStateStore> {
        unimplemented!()
    }

    fn continuity_capsule_store(&self) -> Arc<dyn ContinuityCapsuleStore> {
        unimplemented!()
    }

    fn turn_ledger_store(&self) -> Arc<dyn TurnLedgerStore> {
        unimplemented!()
    }

    fn self_model_store(&self) -> Arc<dyn SelfModelStore> {
        unimplemented!()
    }

    fn self_authored_core_store(&self) -> Arc<dyn SelfAuthoredCoreStore> {
        unimplemented!()
    }

    fn core_revision_ledger_store(&self) -> Arc<dyn CoreRevisionLedgerStore> {
        unimplemented!()
    }

    fn self_continuity_store(&self) -> Arc<dyn SelfContinuityStore> {
        unimplemented!()
    }

    fn relationship_constitution_store(&self) -> Arc<dyn RelationshipConstitutionStore> {
        unimplemented!()
    }

    fn relationship_portfolio_store(&self) -> Arc<dyn RelationshipPortfolioStore> {
        unimplemented!()
    }

    fn relationship_topology_store(&self) -> Arc<dyn RelationshipTopologyStore> {
        unimplemented!()
    }

    fn execution_state_store(&self) -> Arc<dyn ExecutionStateStore> {
        unimplemented!()
    }

    fn world_sense_store(&self) -> Arc<dyn WorldSenseStore> {
        unimplemented!()
    }

    fn outer_voice_store(&self) -> Arc<dyn OuterVoiceStore> {
        unimplemented!()
    }

    fn autonomy_strategy_store(&self) -> Arc<dyn AutonomyStrategyStore> {
        unimplemented!()
    }

    fn inner_life_store(&self) -> Arc<dyn InnerLifeStore> {
        unimplemented!()
    }

    fn felt_significance_store(&self) -> Arc<dyn FeltSignificanceStore> {
        unimplemented!()
    }

    fn temperament_continuity_store(&self) -> Arc<dyn TemperamentContinuityStore> {
        unimplemented!()
    }

    fn inner_conflict_store(&self) -> Arc<dyn InnerConflictStore> {
        unimplemented!()
    }

    fn mental_privacy_store(&self) -> Arc<dyn MentalPrivacyStore> {
        unimplemented!()
    }

    fn private_doc_store(&self) -> Arc<dyn PrivateDocStore> {
        unimplemented!()
    }

    fn private_garden_store(&self) -> Arc<dyn PrivateGardenStore> {
        unimplemented!()
    }

    fn turn_continuity_evidence_store(&self) -> Arc<dyn TurnContinuityEvidenceStore> {
        unimplemented!()
    }

    fn remind_at_store(&self) -> Arc<dyn RemindAtStore> {
        unimplemented!()
    }

    fn task_store(&self) -> Arc<dyn TaskStore> {
        unimplemented!()
    }

    fn task_run_store(&self) -> Arc<dyn TaskRunStore> {
        unimplemented!()
    }

    fn task_artifact_store(&self) -> Arc<dyn TaskArtifactStore> {
        unimplemented!()
    }

    fn task_learning_store(&self) -> Arc<dyn TaskLearningStore> {
        unimplemented!()
    }
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

#[test]
fn runtime_builder_rejects_empty_identity_and_scope() {
    let err = MemoryIdentity::new("", "owner").expect_err("empty agent id rejected");
    assert_eq!(err.stage(), "memory_identity");

    let err = MemoryScope::new("local", "").expect_err("empty chat id rejected");
    assert_eq!(err.stage(), "memory_scope");
}

#[test]
fn runtime_builder_exposes_capabilities_without_calling_core_directly() {
    let platform = Arc::new(HostPlatform);
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(ProfileId::EspEmbeddedSdk)
        .platform(platform)
        .clock(Arc::new(FixedMemoryClock::new(1_800_000_000)))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime");

    assert_eq!(runtime.identity().agent_id, "agent-main");
    assert_eq!(runtime.scope().chat_id, "chat-1");
    assert_eq!(runtime.capabilities().profile, ProfileId::EspEmbeddedSdk);
}
