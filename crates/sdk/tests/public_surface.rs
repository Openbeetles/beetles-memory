#![allow(dead_code, unused_imports)]

use std::sync::Arc;

use bm_sdk::{
    ActiveWorkStore, AutonomyStrategyStore, ContinuityCapsuleStore, CoreRevisionLedgerStore,
    ExecutionStateStore, FeltSignificanceStore, InnerConflictStore, InnerLifeStore,
    LongTermMemoryExtractionStateStore, LongTermMemoryStore, MemoryProfile,
    MemoryRuntimeSystemKind, MemoryStore, MentalPrivacyStore, OuterVoiceStore, Platform,
    PlatformMemorySystemKind, PostReplyMemoryMaintenanceContext, PrivateDocStore,
    PrivateGardenStore, PromptMemoryContextParams, PromptParticipationPlan,
    RelationshipConstitutionStore, RelationshipPortfolioStore, RelationshipTopologyStore,
    RemindAtStore, SelfAuthoredCoreStore, SelfContinuityStore, SelfModelStore, SessionStore,
    SessionSummaryStore, SkillMetaStore, SkillStorage, StateFs, TaskArtifactStore,
    TaskLearningStore, TaskRunStore, TaskStore, TemperamentContinuityStore,
    TurnContinuityEvidenceStore, TurnLedgerStore, WorldSenseStore,
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
}

fn prompt_context_contract_is_sdk_importable<'a>(
    params: PromptMemoryContextParams<'a>,
) -> PromptMemoryContextParams<'a> {
    params
}

fn post_reply_context_contract_is_sdk_importable<'a>(
    ctx: PostReplyMemoryMaintenanceContext<'a>,
) -> PostReplyMemoryMaintenanceContext<'a> {
    ctx
}

fn task_store_traits_are_sdk_importable(
    _task_store: &dyn TaskStore,
    _run_store: &dyn TaskRunStore,
    _artifact_store: &dyn TaskArtifactStore,
    _learning_store: &dyn TaskLearningStore,
) {
}

#[test]
fn profile_and_system_kind_aliases_are_unambiguous() {
    let runtime_kind: MemoryRuntimeSystemKind = MemoryProfile::Embedded.memory_system_kind();
    assert_eq!(runtime_kind, MemoryRuntimeSystemKind::EspCompact);
    assert_eq!(runtime_kind.memory_profile(), MemoryProfile::Embedded);

    let platform_kind = HostPlatform.memory_system_kind();
    assert_eq!(platform_kind, PlatformMemorySystemKind::SdkEmbedded);
    assert_eq!(platform_kind.as_str(), "sdk_embedded");
}

#[test]
fn prompt_participation_plan_is_available_from_sdk() {
    let plan = PromptParticipationPlan::embedded_first_turn_default();
    assert!(plan.load_l1_constitutional);
    assert!(!plan.load_l2_governed_recall);
}
