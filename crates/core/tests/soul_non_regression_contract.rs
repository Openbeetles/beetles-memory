use bm_core::memory::{
    redact_private_soul_graph_material, SubjectDescriptor, SubjectKind, SubjectRegistry,
    SubjectSoulBinding, SubjectSoulSurface, SubjectVisibility,
};

#[test]
fn independent_agent_subjects_have_independent_soul_bindings() {
    let mut registry = SubjectRegistry::empty("space:multi-agent");
    let mut alpha = SubjectDescriptor::new(
        "agent:alpha",
        SubjectKind::AgentPersona,
        "Agent Alpha",
        SubjectVisibility::Visible,
    );
    alpha.soul_binding = Some(SubjectSoulBinding::agent_persona("agent:alpha"));
    let mut beta = SubjectDescriptor::new(
        "agent:beta",
        SubjectKind::AgentPersona,
        "Agent Beta",
        SubjectVisibility::Visible,
    );
    beta.soul_binding = Some(SubjectSoulBinding::agent_persona("agent:beta"));

    registry.upsert_subject(alpha).expect("agent alpha");
    registry.upsert_subject(beta).expect("agent beta");

    let alpha_soul = registry
        .subject("agent:alpha")
        .and_then(|subject| subject.soul_binding.as_ref())
        .expect("agent alpha soul");
    let beta_soul = registry
        .subject("agent:beta")
        .and_then(|subject| subject.soul_binding.as_ref())
        .expect("agent beta soul");

    assert_ne!(alpha_soul.soul_id, beta_soul.soul_id);
    assert!(alpha_soul
        .surfaces
        .contains(&SubjectSoulSurface::PrivateGarden));
    assert!(beta_soul
        .surfaces
        .contains(&SubjectSoulSurface::PrivateGarden));
    assert!(registry.validate_contract().accepted);
}

#[test]
fn graph_redaction_blocks_raw_inner_life_and_private_garden_material() {
    let report = redact_private_soul_graph_material(
        "temporal_graph",
        &[
            "soul-revision:agent:alpha:42",
            "inner_life raw: I secretly fear this deployment",
            "private_garden note: hidden draft",
            "protected_subject:agent:finance",
        ],
    );

    assert_eq!(report.checked_refs.len(), 4);
    assert_eq!(report.raw_private_leak_count, 2);
    assert_eq!(
        report.redacted_refs,
        vec![
            "redacted_ref:1:private_material".to_string(),
            "redacted_ref:2:private_material".to_string()
        ]
    );
    let rendered = format!("{report:?}");
    assert!(!rendered.contains("I secretly fear this deployment"));
    assert!(!rendered.contains("hidden draft"));
}

#[test]
fn graph_redaction_treats_private_surface_names_as_structured_private_material() {
    let report = redact_private_soul_graph_material("private garden", &["journal/today.md"]);

    assert_eq!(report.surface, "redacted_surface:private_material");
    assert_eq!(report.raw_private_leak_count, 1);
    assert_eq!(
        report.checked_refs,
        vec!["redacted_ref:0:private_material".to_string()]
    );
}
