use bm_core::memory::MemoryPrivacyClass;
use bm_core::skills::{
    agent_tool_experience_head_key, agent_tool_experience_material_key,
    agent_tool_experience_scope_manifest_key, canonical_agent_tool_experience_owner_id,
    validate_agent_tool_experience_owner_history, validate_agent_tool_experience_scope_closure,
    AgentToolExperienceBodyV1, AgentToolExperienceFocusV1, AgentToolExperienceHeadBindingV1,
    AgentToolExperienceOwnerHeadV3, AgentToolExperienceOwnerLocatorV3,
    AgentToolExperienceOwningScopeV1, AgentToolExperienceRetainedRevisionDigestV3,
    AgentToolExperienceRevisionMaterialV3, AgentToolExperienceScopeManifestV1,
    AgentToolExperienceStatus, AgentToolMethodSourceRefV1, AgentToolRegistryScope,
};

fn scope(subject: &str) -> AgentToolExperienceOwningScopeV1 {
    AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: subject.to_string(),
    }
}

fn material(
    memory_space_id: &str,
    mounted_subject_id: &str,
    revision: u64,
    predecessor: Option<AgentToolExperienceRetainedRevisionDigestV3>,
) -> AgentToolExperienceRevisionMaterialV3 {
    let mut body = AgentToolExperienceBodyV1::Method {
        task_signature: "extract_pdf_text_for_release_notes".into(),
        procedure: "1. Inspect the PDF document.\n2. Extract and verify the release notes.".into(),
        constraints: vec!["filesystem.read".into()],
        sources: vec![],
    };
    let method_digest = body.method_content_digest().unwrap().unwrap();
    let AgentToolExperienceBodyV1::Method { sources, .. } = &mut body else {
        unreachable!()
    };
    sources.push(AgentToolMethodSourceRefV1 {
        source_job_id: format!("source-{revision}"),
        method_id: "method-a".into(),
        method_digest,
        execution_refs: vec![],
    });
    AgentToolExperienceRevisionMaterialV3::build(
        memory_space_id,
        scope(mounted_subject_id),
        "host-tools",
        AgentToolRegistryScope::Project {
            project_id: "project-a".to_string(),
        },
        "pdf.extract",
        "schema-pdf-v1",
        revision,
        body,
        AgentToolExperienceStatus::Active,
        MemoryPrivacyClass::SharedWithSubject,
        100,
        100 + revision,
        predecessor,
    )
    .expect("canonical material")
}

#[test]
fn owner_identity_is_exactly_bound_to_space_subject_and_registry_scope() {
    let subject_a = material("space-a", "subject-a", 1, None);
    let subject_b = material("space-a", "subject-b", 1, None);
    let space_b = material("space-b", "subject-a", 1, None);

    assert_ne!(subject_a.owner_ref, subject_b.owner_ref);
    assert_ne!(subject_a.owner_ref, space_b.owner_ref);
    assert_ne!(subject_a.physical_key, subject_b.physical_key);

    let owner_id = canonical_agent_tool_experience_owner_id(
        "space-a",
        &scope("subject-a"),
        "host-tools",
        &AgentToolRegistryScope::Project {
            project_id: "project-a".to_string(),
        },
        "pdf.extract",
        "schema-pdf-v1",
        &AgentToolExperienceFocusV1::Method {
            task_signature: "extract_pdf_text_for_release_notes",
        },
    )
    .expect("owner id");
    assert_eq!(subject_a.owner_ref.owner_id, owner_id);

    let other_registry_scope = canonical_agent_tool_experience_owner_id(
        "space-a",
        &scope("subject-a"),
        "host-tools",
        &AgentToolRegistryScope::Project {
            project_id: "project-b".to_string(),
        },
        "pdf.extract",
        "schema-pdf-v1",
        &AgentToolExperienceFocusV1::Method {
            task_signature: "extract_pdf_text_for_release_notes",
        },
    )
    .expect("other scoped owner id");
    assert_ne!(owner_id, other_registry_scope);
}

#[test]
fn revision_history_and_locator_are_canonical_and_tamper_evident() {
    let first = material("space-a", "subject-a", 1, None);
    let retained_first =
        AgentToolExperienceRetainedRevisionDigestV3::from_material(&first).expect("retained first");
    let second = material("space-a", "subject-a", 2, Some(retained_first.clone()));
    validate_agent_tool_experience_owner_history(&[first.clone(), second.clone()])
        .expect("valid history");

    let locator = AgentToolExperienceOwnerLocatorV3::from_material(&second);
    let encoded = serde_json::to_value(&locator).expect("locator json");
    assert_eq!(encoded["memory_space_id"], "space-a");
    assert_eq!(encoded["owner_revision"], 2);
    assert!(serde_json::from_value::<AgentToolExperienceOwnerLocatorV3>(encoded).is_ok());

    let mut wrong_predecessor = second;
    wrong_predecessor.predecessor = None;
    assert!(validate_agent_tool_experience_owner_history(&[first, wrong_predecessor]).is_err());
    assert!(AgentToolExperienceOwnerLocatorV3::try_new(
        " space-a",
        scope("subject-a"),
        retained_first.owner_ref.owner_id,
        1,
    )
    .is_err());
}

#[test]
fn head_and_manifest_validate_the_exact_owner_closure() {
    let first = material("space-a", "subject-a", 1, None);
    let retained = AgentToolExperienceRetainedRevisionDigestV3::from_material(&first)
        .expect("retained revision");
    let head = AgentToolExperienceOwnerHeadV3::build(
        "space-a",
        scope("subject-a"),
        first.owner_ref.clone(),
        1,
        vec![retained],
    )
    .expect("head");
    let binding =
        AgentToolExperienceHeadBindingV1::from_head_and_material(&head, &first).expect("binding");
    let manifest = AgentToolExperienceScopeManifestV1::build(
        1,
        "space-a",
        scope("subject-a"),
        vec![binding.clone()],
        8,
    )
    .expect("manifest");

    validate_agent_tool_experience_scope_closure(
        &manifest,
        std::slice::from_ref(&head),
        std::slice::from_ref(&first),
        8,
    )
    .expect("exact closure");
    assert_eq!(
        first.physical_key,
        agent_tool_experience_material_key("space-a", &scope("subject-a"), &first.owner_ref, 1)
            .expect("material key")
    );
    assert_eq!(
        head.physical_key,
        agent_tool_experience_head_key("space-a", &scope("subject-a"), &head.owner_ref)
            .expect("head key")
    );
    assert_eq!(
        manifest.physical_key,
        agent_tool_experience_scope_manifest_key("space-a", &scope("subject-a"))
            .expect("manifest key")
    );

    let mut tampered = first;
    let AgentToolExperienceBodyV1::Method { procedure, .. } = &mut tampered.body else {
        unreachable!()
    };
    procedure.push_str(" tampered");
    assert!(
        validate_agent_tool_experience_scope_closure(&manifest, &[head], &[tampered], 8,).is_err()
    );

    assert!(AgentToolExperienceScopeManifestV1::build(
        2,
        "space-a",
        scope("subject-a"),
        vec![binding.clone(), binding],
        8,
    )
    .is_err());
}

#[test]
fn head_cannot_drop_the_initial_revision_from_retained_history() {
    let first = material("space-a", "subject-a", 1, None);
    let retained_first =
        AgentToolExperienceRetainedRevisionDigestV3::from_material(&first).expect("retained first");
    let second = material("space-a", "subject-a", 2, Some(retained_first));
    let retained_second = AgentToolExperienceRetainedRevisionDigestV3::from_material(&second)
        .expect("retained second");

    assert!(AgentToolExperienceOwnerHeadV3::build(
        "space-a",
        scope("subject-a"),
        second.owner_ref,
        2,
        vec![retained_second],
    )
    .is_err());
}
