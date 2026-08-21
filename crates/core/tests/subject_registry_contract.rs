use bm_core::memory::{
    default_agent_subject_id, default_memory_space_id, primary_human_subject_id,
    system_governor_subject_id, SubjectDescriptor, SubjectKind, SubjectLifecycleState,
    SubjectRegistry, SubjectRelationshipGraph, SubjectRelationshipKind, SubjectSoulSurface,
    SubjectVisibility,
};

#[test]
fn registry_rejects_noncanonical_subject_ids_at_mutation_and_validation_boundaries() {
    let mut registry = SubjectRegistry::empty("space:canonical");
    let mut subject = SubjectDescriptor::new(
        "human:user",
        SubjectKind::HumanUser,
        "Human",
        bm_core::memory::SubjectVisibility::Visible,
    );
    subject.subject_id = " human:user ".to_string();
    assert_eq!(
        registry.upsert_subject(subject),
        Err("subject_id_non_canonical".to_string())
    );

    let mut valid =
        SubjectRegistry::single_agent_default("owner-a", "agent-a").expect("canonical registry");
    valid.subjects[0].subject_id = " system:owner-a ".to_string();
    let validation = valid.validate_contract();
    assert!(!validation.accepted);
    assert_eq!(validation.reason, "subject_id_non_canonical");
}

#[test]
fn single_agent_default_registry_hides_multi_subject_internals() {
    let registry =
        SubjectRegistry::single_agent_default("owner default", "agent-main").expect("registry");

    assert_eq!(
        registry.memory_space_id,
        default_memory_space_id("owner default")
    );
    assert_eq!(registry.subjects.len(), 3);
    assert_eq!(
        registry.system_governor().expect("system").subject_id,
        system_governor_subject_id("owner default")
    );
    assert_eq!(
        registry.primary_human_user().expect("human").subject_id,
        primary_human_subject_id("owner default")
    );
    assert_eq!(
        registry.default_agent().expect("agent").subject_id,
        default_agent_subject_id("agent-main")
    );
    assert!(registry.validate_contract().accepted);
}

#[test]
fn registry_keeps_agent_soul_as_first_class_subject_property() {
    let registry = SubjectRegistry::single_agent_default("owner-a", "agent-a").expect("registry");
    let agent = registry.default_agent().expect("agent subject");
    let soul = agent.soul_binding.as_ref().expect("agent soul binding");

    assert_eq!(agent.kind, SubjectKind::AgentPersona);
    assert!(soul
        .surfaces
        .contains(&SubjectSoulSurface::SelfAuthoredCore));
    assert!(soul.surfaces.contains(&SubjectSoulSurface::SelfContinuity));
    assert!(soul.surfaces.contains(&SubjectSoulSurface::PrivateGarden));
    assert!(soul.surfaces.contains(&SubjectSoulSurface::InnerLife));
    assert!(soul
        .surfaces
        .contains(&SubjectSoulSurface::RelationshipExperience));
    assert!(soul.surfaces.contains(&SubjectSoulSurface::SoulFeedback));
    assert!(soul
        .surfaces
        .contains(&SubjectSoulSurface::GrowthRevisionLedger));
}

#[test]
fn registry_rejects_soul_as_metadata_or_system_property() {
    let mut descriptor = SubjectDescriptor::new(
        "agent:bad",
        SubjectKind::AgentPersona,
        "Bad Agent",
        SubjectVisibility::Visible,
    );
    descriptor
        .metadata
        .insert("soul".to_string(), "metadata cannot own soul".to_string());
    descriptor.lifecycle_state = SubjectLifecycleState::Active;

    let mut registry = SubjectRegistry::empty(default_memory_space_id("owner-a"));
    registry
        .upsert_subject(descriptor)
        .expect("insert descriptor");

    assert_eq!(
        registry.validate_contract().reason,
        "subject_soul_metadata_forbidden"
    );

    let mut system = SubjectDescriptor::new(
        system_governor_subject_id("owner-a"),
        SubjectKind::SystemGovernor,
        "System Governor",
        SubjectVisibility::Hidden,
    );
    system.soul_binding = Some(Default::default());
    let mut registry = SubjectRegistry::empty(default_memory_space_id("owner-a"));
    registry.upsert_subject(system).expect("insert system");
    assert_eq!(
        registry.validate_contract().reason,
        "system_governor_soul_binding_forbidden"
    );
}

#[test]
fn relationship_graph_is_subject_scoped_without_role_lanes() {
    let registry = SubjectRegistry::single_agent_default("owner-a", "agent-a").expect("registry");
    let graph = SubjectRelationshipGraph::single_agent_default(&registry).expect("graph");

    assert_eq!(graph.memory_space_id, registry.memory_space_id);
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == SubjectRelationshipKind::CollaboratesWith));
    assert!(graph.validate_against_registry(&registry).accepted);

    let rendered = format!("{registry:?}\n{graph:?}");
    assert!(!rendered.contains("RoleKey"));
    assert!(!rendered.contains("RoleMemoryLane"));
}
