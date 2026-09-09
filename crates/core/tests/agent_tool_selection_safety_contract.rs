use bm_core::feature_gate::ProfileId;
use bm_core::memory::{MemoryPrivacyClass, ProceduralApplicabilityContextV1};
use bm_core::skills::*;
use std::slice::from_ref;

fn registry() -> AgentToolRegistrySnapshot {
    AgentToolRegistrySnapshot::compact(
        "tools",
        "tools",
        vec![AgentToolDescriptor::compact(
            "search",
            "Weather search",
            "schema-v1",
        )],
        100,
    )
}

fn material(signature: &str, guidance: &str) -> AgentToolExperienceRevisionMaterialV2 {
    AgentToolExperienceRevisionMaterialV2::build(
        "space",
        AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: "agent".into(),
        },
        "tools",
        AgentToolRegistryScope::Global,
        "search",
        "schema-v1",
        signature,
        1,
        "Governed procedure",
        guidance,
        vec![],
        2,
        2,
        0,
        AgentToolOutcome::Succeeded,
        AgentToolExperienceConfidence::High,
        AgentToolExperienceStatus::Active,
        vec!["private-evidence-marker".into()],
        MemoryPrivacyClass::SharedWithSubject,
        100,
        100,
        None,
    )
    .unwrap()
}

fn select(
    query: &str,
    materials: &[AgentToolExperienceRevisionMaterialV2],
    registries: &[AgentToolRegistrySnapshot],
    refs: &[AgentToolRegistryRef],
) -> AgentToolSelectionReport {
    let scope = AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: "agent".into(),
    };
    let heads = materials
        .iter()
        .map(|material| {
            AgentToolExperienceOwnerHeadV2::build(
                "space",
                scope.clone(),
                material.owner_ref.clone(),
                1,
                vec![AgentToolExperienceRetainedRevisionDigestV2::from_material(material).unwrap()],
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    select_subject_agent_tool_hints(AgentToolExperienceSelectionInput {
        query,
        memory_space_id: "space",
        owning_scope: &scope,
        heads: &heads,
        materials,
        registries,
        registry_refs: refs,
        applicability: &ProceduralApplicabilityContextV1::try_new(None, None, None).unwrap(),
        as_of_time: None,
        max_hints: 8,
    })
    .unwrap()
}

fn assert_safe_rejected(report: &AgentToolSelectionReport, reason: &str) {
    assert!(report.tool_hints.is_empty());
    assert!(report
        .audit
        .rejected
        .iter()
        .any(|value| value.reason == reason));
    let encoded = serde_json::to_string(report).unwrap();
    assert!(!encoded.contains("secret-guidance-marker"));
    assert!(!encoded.contains("private-evidence-marker"));
}

#[test]
fn registry_empty_fingerprint_is_not_a_wildcard_authority() {
    let mut registry = registry();
    validate_agent_tool_registry_snapshot(ProfileId::DesktopMacosEmbeddedSdk, &registry).unwrap();
    registry.fingerprint.clear();
    assert!(
        validate_agent_tool_registry_snapshot(ProfileId::DesktopMacosEmbeddedSdk, &registry)
            .is_err()
    );
    let report = select(
        "secret-guidance-marker",
        &[material("task", "secret-guidance-marker")],
        &[registry],
        &[],
    );
    assert_safe_rejected(&report, AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH);
}

#[test]
fn private_and_scope_denials_have_safe_nonempty_audit() {
    let registry = registry();
    let shared = material("task", "secret-guidance-marker");
    assert!(!select(
        "secret-guidance-marker",
        from_ref(&shared),
        from_ref(&registry),
        &[]
    )
    .tool_hints
    .is_empty());
    let mut private = shared;
    private.privacy_class = MemoryPrivacyClass::PrivateGarden;
    private.content_digest = private.canonical_content_digest().unwrap();
    assert_safe_rejected(
        &select(
            "secret-guidance-marker",
            &[private],
            from_ref(&registry),
            &[],
        ),
        "agent_tool_experience_privacy_blocked",
    );
    let mut scoped = material("task", "secret-guidance-marker");
    scoped.registry_scope = AgentToolRegistryScope::Project {
        project_id: "other-project".into(),
    };
    // Rebuild the complete canonical owner when changing applicability.
    scoped = AgentToolExperienceRevisionMaterialV2::build(
        "space",
        scoped.owning_scope,
        "tools",
        scoped.registry_scope,
        "search",
        "schema-v1",
        "task",
        1,
        "Governed procedure",
        "secret-guidance-marker",
        vec![],
        2,
        2,
        0,
        AgentToolOutcome::Succeeded,
        AgentToolExperienceConfidence::High,
        AgentToolExperienceStatus::Active,
        vec![],
        MemoryPrivacyClass::SharedWithSubject,
        100,
        100,
        None,
    )
    .unwrap();
    assert_safe_rejected(
        &select("secret-guidance-marker", &[scoped], &[registry], &[]),
        "agent_tool_experience_applicability_blocked",
    );
}

#[test]
fn registry_and_reference_denials_are_explicit_and_safe() {
    let registry = registry();
    let material = material("task", "secret-guidance-marker");
    assert_safe_rejected(
        &select("secret-guidance-marker", from_ref(&material), &[], &[]),
        "agent_tool_registry_not_found",
    );
    let mut reference = registry.registry_ref();
    reference.fingerprint.clear();
    assert_safe_rejected(
        &select(
            "secret-guidance-marker",
            &[material],
            &[registry],
            &[reference],
        ),
        "agent_tool_registry_ref_mismatch",
    );
}

#[test]
fn query_uses_governed_text_but_never_registry_names_or_signature_fragments() {
    let registry = registry();
    let material = material(
        "opaque:weather:123",
        "Read release notes from PDF documents",
    );
    assert!(!select(
        "PDF documents",
        from_ref(&material),
        from_ref(&registry),
        &[]
    )
    .tool_hints
    .is_empty());
    assert!(!select(
        "opaque:weather:123",
        from_ref(&material),
        from_ref(&registry),
        &[]
    )
    .tool_hints
    .is_empty());
    assert_safe_rejected(
        &select("weather", from_ref(&material), from_ref(&registry), &[]),
        "agent_tool_experience_query_unrelated",
    );
    assert_safe_rejected(
        &select("", &[material], &[registry], &[]),
        "agent_tool_experience_query_unrelated",
    );
}

#[test]
fn exact_task_identity_outranks_only_textual_relevance() {
    let registry = registry();
    let exact = material("PDF", "Read PDF documents");
    let textual = material("other-task", "Read PDF documents");
    let report = select("PDF", &[textual, exact.clone()], &[registry], &[]);
    assert_eq!(report.tool_hints.len(), 2);
    assert_eq!(report.tool_hints[0].experience_id, exact.owner_ref.owner_id);
}

#[test]
fn ambiguous_registry_identity_does_not_select_first_matching_capability() {
    let enabled = registry();
    let mut disabled = enabled.clone();
    disabled.tools[0].disabled = true;
    disabled.fingerprint = fingerprint_agent_tool_registry(&disabled);
    let report = select(
        "PDF",
        &[material("task", "Read PDF documents")],
        &[enabled, disabled],
        &[],
    );
    assert_safe_rejected(&report, "agent_tool_registry_identity_ambiguous");
}

#[test]
fn registry_identity_and_scope_require_canonical_values_without_rewriting_text() {
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    let mut natural = registry();
    natural.tools[0].display_name = "  阅读 PDF / 自然语言名称  ".into();
    natural.fingerprint = fingerprint_agent_tool_registry(&natural);
    validate_agent_tool_registry_snapshot(profile, &natural).unwrap();
    let mut invalid = Vec::new();
    for value in [" tools", "tools ", "tools\ncontrol"] {
        let mut candidate = registry();
        candidate.registry_id = value.into();
        invalid.push(candidate);
    }
    for value in [" search", "search ", "search\ncontrol"] {
        let mut candidate = registry();
        candidate.tools[0].tool_id = value.into();
        invalid.push(candidate);
    }
    for value in [" schema-v1", "schema-v1 ", "schema\ncontrol"] {
        let mut candidate = registry();
        candidate.tools[0].schema_fingerprint = value.into();
        invalid.push(candidate);
    }
    for value in ["", " scoped", "scoped ", "scope\ncontrol"] {
        for scope in [
            AgentToolRegistryScope::Project {
                project_id: value.into(),
            },
            AgentToolRegistryScope::Workspace {
                workspace_id: value.into(),
            },
            AgentToolRegistryScope::Conversation {
                conversation_id: value.into(),
            },
        ] {
            let mut candidate = registry();
            candidate.scope = scope;
            invalid.push(candidate);
        }
    }
    for mut candidate in invalid {
        candidate.fingerprint = fingerprint_agent_tool_registry(&candidate);
        assert!(
            validate_agent_tool_registry_snapshot(profile, &candidate).is_err(),
            "noncanonical identity was accepted: {candidate:?}"
        );
    }
}
