#![allow(dead_code)]

use bm_sdk::{
    MemoryCapabilityCatalog, MemoryCapabilityPolicy, MemoryIdentity, MemoryProfile, MemoryRuntime,
    MemoryRuntimeSystemKind, MemoryScope, MemoryWriteRequest, PostReplyMemoryMaintenanceContext,
    ProfileId, PromptMemoryContextParams, PromptParticipationPlan, StoreBackendConfig,
    StorePlatform,
};

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

fn sdk_runtime_contract_types_are_importable(
    _runtime: Option<MemoryRuntime>,
    _catalog: Option<MemoryCapabilityCatalog>,
    _policy: MemoryCapabilityPolicy,
    _identity: MemoryIdentity,
    _scope: MemoryScope,
    _write: Option<MemoryWriteRequest>,
) {
}

#[test]
fn profile_and_system_kind_aliases_are_unambiguous() {
    let runtime_kind: MemoryRuntimeSystemKind = MemoryProfile::Embedded.memory_system_kind();
    assert_eq!(runtime_kind, MemoryRuntimeSystemKind::EspCompact);
    assert_eq!(runtime_kind.memory_profile(), MemoryProfile::Embedded);
}

#[test]
fn sdk_runtime_uses_store_platform_as_public_store_entry() {
    let store = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();

    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
        .subject_id("subject-default")
        .scope(MemoryScope::new("local", "chat-1").unwrap())
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(store)
        .build()
        .unwrap();

    assert_eq!(runtime.identity().agent_id, "agent-main");
    assert_eq!(runtime.subject_id(), "subject-default");
    assert_eq!(runtime.scope().chat_id, "chat-1");
    assert_eq!(
        runtime.capabilities().profile,
        ProfileId::ServerLinuxDevFull
    );
}

#[test]
fn prompt_participation_plan_is_available_from_sdk() {
    let plan = PromptParticipationPlan::embedded_first_turn_default();
    assert!(plan.load_l1_constitutional);
    assert!(!plan.load_l2_governed_recall);
}
