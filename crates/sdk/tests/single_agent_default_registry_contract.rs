use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, primary_human_subject_id,
    system_governor_subject_id, MemoryIdentity, MemoryRuntime, MemoryScope, ProfileId,
    StoreBackendConfig, StorePlatform, SubjectDescriptor, SubjectKind, SubjectRegistry,
    SubjectScopedRuntime,
};

#[test]
fn runtime_builder_mounts_single_agent_default_registry() {
    let store = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("store config"),
    )
    .expect("store");

    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(store)
        .build()
        .expect("runtime");

    assert_eq!(
        runtime.memory_space_id(),
        default_memory_space_id("owner-default")
    );
    assert_eq!(runtime.subject_id(), default_agent_subject_id("agent-main"));
    assert_eq!(runtime.scoped_runtime().agent_id, "agent-main");
    assert_eq!(
        runtime.scoped_runtime().mounted_subject_id,
        default_agent_subject_id("agent-main")
    );
    assert_eq!(
        runtime.scoped_runtime().actor_subject_id,
        default_agent_subject_id("agent-main")
    );

    let registry = runtime.subject_registry();
    assert_eq!(registry.subjects.len(), 3);
    assert_eq!(
        registry
            .subject(&system_governor_subject_id("owner-default"))
            .expect("system")
            .kind,
        SubjectKind::SystemGovernor
    );
    assert_eq!(
        registry
            .subject(&primary_human_subject_id("owner-default"))
            .expect("human")
            .kind,
        SubjectKind::HumanUser
    );
    assert_eq!(
        registry
            .subject(&default_agent_subject_id("agent-main"))
            .expect("agent")
            .kind,
        SubjectKind::AgentPersona
    );
}

#[test]
fn default_relationship_graph_uses_mounted_subject_when_registry_has_multiple_agents() {
    let store = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("store config"),
    )
    .expect("store");
    let mut registry =
        SubjectRegistry::single_agent_default("owner-custom", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Second Agent",
        ))
        .expect("upsert subject");

    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-b", "owner-custom").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(store)
        .subject_registry(registry)
        .build()
        .expect("runtime");

    let mounted = default_agent_subject_id("agent-b");
    assert_eq!(runtime.subject_id(), mounted);
    assert!(
        runtime
            .subject_relationship_graph()
            .edges
            .iter()
            .any(|edge| edge.to_subject_id == mounted || edge.from_subject_id == mounted),
        "default graph must be scoped around the mounted agent subject"
    );
}

#[test]
fn custom_scoped_runtime_drives_default_relationship_graph() {
    let store = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("store config"),
    )
    .expect("store");
    let mut registry =
        SubjectRegistry::single_agent_default("owner-custom", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Second Agent",
        ))
        .expect("upsert subject");

    let mounted = default_agent_subject_id("agent-b");
    let unmounted = default_agent_subject_id("agent-a");
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-a", "owner-custom").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(store)
        .subject_registry(registry)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id("owner-custom"),
            mounted_subject_id: mounted.clone(),
            actor_subject_id: mounted.clone(),
            agent_id: "agent-b".to_string(),
            relationship_scope: None,
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .build()
        .expect("runtime");

    assert_eq!(runtime.subject_id(), mounted);
    assert!(runtime
        .subject_relationship_graph()
        .edges
        .iter()
        .any(|edge| edge.to_subject_id == mounted || edge.from_subject_id == mounted));
    assert!(!runtime
        .subject_relationship_graph()
        .edges
        .iter()
        .any(|edge| edge.to_subject_id == unmounted || edge.from_subject_id == unmounted));
}

#[test]
fn single_agent_default_public_surface_has_no_beetle_agent_cbd_roles() {
    let registry =
        bm_sdk::SubjectRegistry::single_agent_default("owner-a", "agent-a").expect("registry");
    let rendered = format!("{registry:?}");

    for forbidden in [
        "CEO",
        "技术总监",
        "市场总监",
        "行政总监",
        "财务总监",
        "仓库管理员",
    ] {
        assert!(!rendered.contains(forbidden), "{forbidden}");
    }
    assert!(!rendered.contains("RoleKey"));
    assert!(!rendered.contains("RoleMemoryLane"));
}
