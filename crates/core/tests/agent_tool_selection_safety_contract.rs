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

fn material(signature: &str, guidance: &str) -> AgentToolExperienceRevisionMaterialV3 {
    let mut body = AgentToolExperienceBodyV1::Method {
        task_signature: signature.into(),
        procedure: format!("1. {guidance}.\n2. Verify the result."),
        constraints: vec![],
        sources: vec![],
    };
    let method_digest = body.method_content_digest().unwrap().unwrap();
    let AgentToolExperienceBodyV1::Method { sources, .. } = &mut body else {
        unreachable!()
    };
    sources.push(AgentToolMethodSourceRefV1 {
        source_job_id: "private-evidence-marker".into(),
        method_id: "method-a".into(),
        method_digest,
        execution_refs: vec![],
    });
    AgentToolExperienceRevisionMaterialV3::build(
        "space",
        AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: "agent".into(),
        },
        "tools",
        AgentToolRegistryScope::Global,
        "search",
        "schema-v1",
        1,
        body,
        AgentToolExperienceStatus::Active,
        MemoryPrivacyClass::SharedWithSubject,
        100,
        100,
        None,
    )
    .unwrap()
}

fn select(
    query: &str,
    materials: &[AgentToolExperienceRevisionMaterialV3],
    registries: &[AgentToolRegistrySnapshot],
    refs: &[AgentToolRegistryRef],
) -> AgentToolSelectionReport {
    let scope = AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: "agent".into(),
    };
    let heads = materials
        .iter()
        .map(|material| {
            AgentToolExperienceOwnerHeadV3::build(
                "space",
                scope.clone(),
                material.owner_ref.clone(),
                1,
                vec![AgentToolExperienceRetainedRevisionDigestV3::from_material(material).unwrap()],
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let subject_scope = bm_core::memory::ProceduralSubjectScopeV1 {
        memory_space_id: "space".into(),
        mounted_subject_id: "agent".into(),
    };
    let source_scope = bm_core::memory::ProceduralProducerScopeV1 {
        memory_space_id: "space".into(),
        mounted_subject_id: "agent".into(),
        channel_id: "sdk.direct".into(),
        chat_id: "selection-test".into(),
    };
    let root = bm_core::memory::ProceduralSubjectValidityRootV1::initialize(
        subject_scope,
        source_scope,
        8,
    )
    .unwrap();
    let job = bm_core::memory::ProceduralFeedbackJobV2::pending(
        bm_core::memory::ProceduralFeedbackIdentityV1::new(
            "space",
            "agent",
            "sdk.direct",
            "selection-test",
            "selection-test",
            "turn-a",
        )
        .unwrap(),
        1,
        format!("sha256:{}", "a".repeat(64)),
        format!("sha256:{}", "b".repeat(64)),
        1,
        5,
        100,
    )
    .unwrap();
    let applications = [
        bm_core::memory::ProceduralFeedbackApplicationLedgerV2::build(
            &job,
            materials
                .iter()
                .map(|material| {
                    bm_core::memory::ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                        owner_revision: material.owner_revision_ref(),
                        content_digest: material.content_digest.clone(),
                    }
                })
                .collect(),
            vec![],
            vec![],
            1,
            100,
        )
        .unwrap(),
    ];
    let authority =
        bm_core::memory::ProceduralReadAuthorityV1::try_new(&root, None, &applications).unwrap();
    let owners = heads
        .iter()
        .map(|head| {
            let history = materials
                .iter()
                .filter(|material| material.owner_ref == head.owner_ref)
                .cloned()
                .collect::<Vec<_>>();
            AgentToolExperienceReadProjectionV1::try_new(head, &history, &authority, None).unwrap()
        })
        .collect::<Vec<_>>();
    select_subject_agent_tool_hints(AgentToolExperienceSelectionInput {
        query,
        memory_space_id: "space",
        owning_scope: &scope,
        owners: &owners,
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
fn pfi2_verified_projection_selects_before_authorizing_and_never_carries_denied_history() {
    use bm_core::memory::*;
    let first = material("weather-task", "A private-history method marker");
    let next = |previous: &AgentToolExperienceRevisionMaterialV3, time| {
        AgentToolExperienceRevisionMaterialV3::build(
            &previous.memory_space_id,
            previous.owning_scope.clone(),
            &previous.registry_id,
            previous.registry_scope.clone(),
            &previous.tool_id,
            &previous.schema_fingerprint,
            previous.owner_revision + 1,
            previous.body.clone(),
            previous.status,
            previous.privacy_class,
            previous.created_at,
            time,
            Some(AgentToolExperienceRetainedRevisionDigestV3::from_material(previous).unwrap()),
        )
        .unwrap()
    };
    let second = next(&first, 200);
    let third = next(&second, 300);
    let history = vec![first.clone(), second, third.clone()];
    let head = AgentToolExperienceOwnerHeadV3::build(
        "space",
        first.owning_scope.clone(),
        first.owner_ref.clone(),
        3,
        history
            .iter()
            .map(|material| {
                AgentToolExperienceRetainedRevisionDigestV3::from_material(material).unwrap()
            })
            .collect(),
    )
    .unwrap();
    let root = ProceduralSubjectValidityRootV1::initialize(
        ProceduralSubjectScopeV1 {
            memory_space_id: "space".into(),
            mounted_subject_id: "agent".into(),
        },
        ProceduralProducerScopeV1 {
            memory_space_id: "space".into(),
            mounted_subject_id: "agent".into(),
            channel_id: "sdk.direct".into(),
            chat_id: "chat".into(),
        },
        8,
    )
    .unwrap();
    let application = |material: &AgentToolExperienceRevisionMaterialV3| {
        let job = ProceduralFeedbackJobV2::pending(
            ProceduralFeedbackIdentityV1::new(
                "space",
                "agent",
                "sdk.direct",
                "chat",
                "chat",
                &format!("turn-{}", material.owner_revision),
            )
            .unwrap(),
            material.owner_revision,
            format!("sha256:{}", "a".repeat(64)),
            format!("sha256:{}", "b".repeat(64)),
            1,
            5,
            100,
        )
        .unwrap();
        ProceduralFeedbackApplicationLedgerV2::build(
            &job,
            vec![ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                owner_revision: material.owner_revision_ref(),
                content_digest: material.content_digest.clone(),
            }],
            vec![],
            vec![],
            1,
            100,
        )
        .unwrap()
    };
    let applications = [application(&first), application(&third)];
    let authority = ProceduralReadAuthorityV1::try_new(&root, None, &applications).unwrap();
    let positive =
        AgentToolExperienceReadProjectionV1::try_new(&head, &history, &authority, Some(150))
            .unwrap();
    assert_eq!(positive.material(), Some(&first));
    let denied =
        AgentToolExperienceReadProjectionV1::try_new(&head, &history, &authority, Some(250))
            .unwrap();
    assert!(
        denied.material().is_none(),
        "rev2 is denied; do not fall back to rev1"
    );
    assert_eq!(
        denied.rejection(),
        Some("agent_tool_experience_authority_withdrawn")
    );
    assert!(!format!("{denied:?}").contains("private-history method marker"));
    let only_old = [application(&first)];
    let authority = ProceduralReadAuthorityV1::try_new(&root, None, &only_old).unwrap();
    assert!(
        AgentToolExperienceReadProjectionV1::try_new(&head, &history, &authority, Some(150))
            .unwrap()
            .material()
            .is_none(),
        "an allowed historical revision cannot bypass denied current authority"
    );
    let mut broken = history;
    broken[1].updated_at = 201;
    assert!(
        AgentToolExperienceReadProjectionV1::try_new(&head, &broken, &authority, Some(250))
            .is_err(),
        "normal withdrawal cannot hide structural corruption of the denied history"
    );
}

#[test]
fn candidate_method_never_enters_a_model_hint() {
    let registry = registry();
    let mut candidate = material("task", "secret-guidance-marker");
    assert!(!select(
        "secret-guidance-marker",
        from_ref(&candidate),
        from_ref(&registry),
        &[]
    )
    .tool_hints
    .is_empty());
    candidate.status = AgentToolExperienceStatus::Candidate;
    candidate.content_digest = candidate.canonical_content_digest().unwrap();
    let report = select("secret-guidance-marker", &[candidate], &[registry], &[]);
    assert!(
        report.tool_hints.is_empty(),
        "an admitted proposal is not a governed active method"
    );
    let wire = serde_json::to_string(&report).unwrap();
    assert!(!wire.contains("secret-guidance-marker"));
    assert!(!wire.contains("private-evidence-marker"));
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
    scoped = AgentToolExperienceRevisionMaterialV3::build(
        "space",
        scoped.owning_scope,
        "tools",
        scoped.registry_scope,
        "search",
        "schema-v1",
        1,
        scoped.body,
        AgentToolExperienceStatus::Active,
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
