#[cfg(feature = "nonproduction-replay-harness")]
use bm_sdk::{
    primary_human_subject_id, GovernedRuntimeSkillWriteInput, LongTermMemoryQuery,
    MemoryLongTermControlView, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermTarget, MemoryPrivacyClass, MemoryWriteRequest,
    RuntimeSkillCreationRef, RuntimeSkillOwningScope, RuntimeSkillWrite, RuntimeSkillWriteSource,
    SubjectSoulFoundingCharterSeedV1, SubjectSoulProvisionIntentV1, SubjectSoulReadSelectorV1,
};
use bm_sdk::{
    GovernedScopeArchiveEntry, GovernedScopeArchiveRootV1, MemoryArchiveScope, MemorySpaceArchive,
    MemorySpaceExportRequest, MemorySpaceImportRequest, MemorySpacePrivateMaterialPolicy,
    ProfileId,
};
use serde_json::json;

#[cfg(feature = "nonproduction-replay-harness")]
mod support;

fn subject_scope() -> MemoryArchiveScope {
    MemoryArchiveScope::subject("space:alpha", "subject:owner").expect("canonical subject scope")
}

fn shared_program_scope() -> MemoryArchiveScope {
    MemoryArchiveScope::shared_program("space:alpha").expect("canonical shared-program scope")
}

fn representative_entries() -> Vec<GovernedScopeArchiveEntry> {
    vec![
        GovernedScopeArchiveEntry::json(
            "long_term_head_manifests",
            "head:alpha",
            &json!({"z": 2, "a": 1}),
        )
        .expect("canonical JSON archive entry"),
        GovernedScopeArchiveEntry::event(
            "long_term_control_audits",
            "event:alpha",
            &json!({"kind": "updated"}),
        )
        .expect("canonical event archive entry"),
    ]
}

#[test]
fn production_archive_surface_is_typed_and_root_bound() {
    fn assert_export_request(request: MemorySpaceExportRequest) {
        assert_eq!(request.scope, subject_scope());
        assert_eq!(
            request.private_material_policy,
            MemorySpacePrivateMaterialPolicy::ExcludePrivate
        );
    }

    fn assert_archive_root(archive: &MemorySpaceArchive) {
        let root: &GovernedScopeArchiveRootV1 = archive.root();
        assert_eq!(root.scope, subject_scope());
        assert_eq!(
            root.private_material_policy,
            MemorySpacePrivateMaterialPolicy::ExcludePrivate
        );
    }

    fn assert_import_request(request: &MemorySpaceImportRequest) {
        assert_eq!(request.scope, subject_scope());
        assert_eq!(
            request.expected_private_material_policy,
            MemorySpacePrivateMaterialPolicy::ExcludePrivate
        );
    }

    let _ = assert_export_request as fn(MemorySpaceExportRequest);
    let _ = assert_archive_root as fn(&MemorySpaceArchive);
    let _ = assert_import_request as fn(&MemorySpaceImportRequest);
}

#[test]
fn archive_scope_identity_is_exact_and_rejects_aliases() {
    let subject = subject_scope();
    let same_subject =
        MemoryArchiveScope::subject("space:alpha", "subject:owner").expect("same subject scope");
    subject
        .validate_exact_identity(&same_subject)
        .expect("the same canonical subject identity must match");

    let shared_program = shared_program_scope();
    let same_shared_program =
        MemoryArchiveScope::shared_program("space:alpha").expect("same shared-program scope");
    shared_program
        .validate_exact_identity(&same_shared_program)
        .expect("the same canonical shared-program identity must match");

    assert!(MemoryArchiveScope::subject(" space:alpha", "subject:owner").is_err());
    assert!(MemoryArchiveScope::subject("space:alpha", "subject:owner ").is_err());
    assert!(MemoryArchiveScope::shared_program("space:alpha ").is_err());

    let different_subject =
        MemoryArchiveScope::subject("space:alpha", "subject:other").expect("other subject scope");
    assert!(subject.validate_exact_identity(&different_subject).is_err());
    assert!(subject.validate_exact_identity(&shared_program).is_err());
    assert!(shared_program.validate_exact_identity(&subject).is_err());
}

#[test]
fn archive_root_uses_canonical_order_independent_sha_and_exact_counts() {
    let entries = representative_entries();
    let root = GovernedScopeArchiveRootV1::build(
        subject_scope(),
        MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        entries.clone(),
    )
    .expect("canonical archive root");

    assert_eq!(root.json_doc_count, 1);
    assert_eq!(root.event_count, 1);
    assert_eq!(root.json_bytes, 13);
    assert_eq!(root.event_bytes, 18);
    assert_eq!(root.closure_sha256.len(), 64);
    assert!(root
        .closure_sha256
        .bytes()
        .all(|byte: u8| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));

    let reversed = GovernedScopeArchiveRootV1::build(
        subject_scope(),
        MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        entries.into_iter().rev().collect::<Vec<_>>(),
    )
    .expect("entry order must not affect the canonical root");
    assert_eq!(root, reversed);

    let differently_ordered_json = vec![
        GovernedScopeArchiveEntry::json(
            "long_term_head_manifests",
            "head:alpha",
            &serde_json::from_str(r#"{"a":1,"z":2}"#).expect("JSON fixture"),
        )
        .expect("canonical JSON archive entry"),
        GovernedScopeArchiveEntry::event(
            "long_term_control_audits",
            "event:alpha",
            &json!({"kind": "updated"}),
        )
        .expect("canonical event archive entry"),
    ];
    let canonical_equivalent = GovernedScopeArchiveRootV1::build(
        subject_scope(),
        MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        differently_ordered_json,
    )
    .expect("object field order must not affect the canonical root");
    assert_eq!(root, canonical_equivalent);

    let mut forged = serde_json::to_value(&root).expect("serialize fixed-size archive root");
    forged
        .as_object_mut()
        .expect("archive root JSON object")
        .insert("legacy_alias".into(), serde_json::Value::Bool(true));
    assert!(serde_json::from_value::<GovernedScopeArchiveRootV1>(forged).is_err());
}

#[test]
fn archive_root_identity_is_profile_independent_but_scope_and_policy_bound() {
    fn build_for_profile(
        _operation_profile: ProfileId,
        scope: MemoryArchiveScope,
    ) -> GovernedScopeArchiveRootV1 {
        GovernedScopeArchiveRootV1::build(
            scope,
            MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            representative_entries(),
        )
        .expect("archive root")
    }

    let desktop = build_for_profile(ProfileId::DesktopMacosDevFull, subject_scope());
    let embedded = build_for_profile(ProfileId::EspEmbeddedSdk, subject_scope());
    assert_eq!(desktop, embedded);

    let shared_program = build_for_profile(ProfileId::DesktopMacosDevFull, shared_program_scope());
    assert_ne!(desktop.closure_sha256, shared_program.closure_sha256);

    let include_private = GovernedScopeArchiveRootV1::build(
        subject_scope(),
        MemorySpacePrivateMaterialPolicy::IncludePrivate,
        representative_entries(),
    )
    .expect("private archive root");
    assert_ne!(desktop.closure_sha256, include_private.closure_sha256);
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn production_subject_archive_binds_root_and_rejects_forgery_before_replace() {
    let profile = support::host_test_profile();
    let source_runtime = support::test_runtime(support::seeded_store_platform(profile), profile);
    let scope = MemoryArchiveScope::subject(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("runtime Subject archive scope");
    let exported = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("production typed archive export");
    assert_eq!(exported.archive.root().scope, scope);
    assert_eq!(
        exported.archive.root().private_material_policy,
        MemorySpacePrivateMaterialPolicy::IncludePrivate
    );

    let forged_root = GovernedScopeArchiveRootV1::build(
        scope.clone(),
        MemorySpacePrivateMaterialPolicy::IncludePrivate,
        representative_entries(),
    )
    .expect("forged but structurally valid root");
    assert_ne!(forged_root, *exported.archive.root());
    let forged_archive = exported
        .archive
        .with_replaced_root_for_nonproduction_harness(forged_root);

    let target_platform = support::empty_store_platform(profile);
    let target_runtime = support::test_runtime(target_platform.clone(), profile);
    let before = target_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("target before snapshot");
    let error = target_runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope: scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: forged_archive,
        })
        .expect_err("forged archive root must fail before replacement");
    assert!(error
        .to_string()
        .contains("archive root does not match its canonical payload closure"));
    let after = target_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("target after rejected snapshot");
    assert_eq!(after, before);

    target_runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        })
        .expect("valid archive must replace the exact Subject scope");
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn include_private_subject_archive_excludes_and_preserves_subject_global_soul_owners() {
    const SUBJECT_GLOBAL_SOUL_NAMESPACES: &[&str] = &[
        "self_model",
        "self_authored_core",
        "core_revision_ledger",
        "self_continuity",
        "relationship_portfolio",
        "relationship_topology",
        "autonomy_strategy",
        "inner_life",
        "felt_significance",
        "temperament_continuity",
        "inner_conflict",
        "mental_privacy",
        "private_doc",
        "private_garden",
        "outer_voice",
        "subject_soul_lifecycle_heads",
        "subject_soul_revision_materials",
        "subject_soul_scope_manifests",
        "subject_soul_generation_tombstones",
        "subject_soul_relationship_projections",
        "subject_soul_operation_results",
    ];

    fn provision(runtime: &bm_sdk::MemoryRuntime, operation_id: &str, identity_anchor: &str) {
        runtime
            .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
                operation_id: operation_id.to_string(),
                human_actor_subject_id: primary_human_subject_id("owner-default"),
                charter: Box::new(
                    SubjectSoulFoundingCharterSeedV1 {
                        identity_anchor: Some(identity_anchor.to_string()),
                        character_tendencies: vec![
                            "preserve governed archive ownership".to_string()
                        ],
                        priority_constitution: vec![
                            "keep Soul outside generic archives".to_string()
                        ],
                        non_negotiables: vec![
                            "never import Soul through a generic memory-space archive".to_string(),
                        ],
                        default_response_mode: None,
                        default_initiative_posture: None,
                        default_relationship_posture: None,
                        boundary_doctrine: None,
                        truth_seeking_commitment: None,
                        self_preservation_doctrine: None,
                        repair_doctrine: None,
                        change_principle: None,
                    }
                    .canonicalize()
                    .expect("canonical archive Soul seed"),
                ),
                source_asserted_at: Some(1_700_000_000),
            })
            .expect("provision typed Soul root");
    }

    let profile = support::host_test_profile();
    let source_runtime = support::test_runtime(support::empty_store_platform(profile), profile);
    provision(
        &source_runtime,
        "archive-source-soul",
        "SOURCE-SOUL-MUST-NOT-ENTER-ARCHIVE",
    );
    let scope = MemoryArchiveScope::subject(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("source Subject archive scope");
    let archive = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("source archive");
    for namespace in SUBJECT_GLOBAL_SOUL_NAMESPACES {
        assert!(
            !archive.archive.contains_json_namespace(namespace),
            "subject-global Soul namespace {namespace} must not enter a memory-space archive"
        );
    }

    let target_platform = support::empty_store_platform(profile);
    let target_runtime = support::test_runtime(target_platform.clone(), profile);
    provision(
        &target_runtime,
        "archive-target-soul",
        "TARGET-SOUL-MUST-SURVIVE-IMPORT",
    );
    let target_soul_before = target_runtime
        .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
        .expect("target safe Soul before generic import");
    let before = target_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("target before import");
    target_runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: archive.archive,
        })
        .expect("replace exact memory-space projection");
    let after = target_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("target after import");
    assert_eq!(
        target_runtime
            .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
            .expect("target safe Soul after generic import"),
        target_soul_before,
        "generic Subject archive import must leave the verified Soul root unchanged"
    );
    for namespace in SUBJECT_GLOBAL_SOUL_NAMESPACES {
        let before_docs = before
            .json_docs
            .iter()
            .filter(|document| document.namespace == *namespace)
            .collect::<Vec<_>>();
        let after_docs = after
            .json_docs
            .iter()
            .filter(|document| document.namespace == *namespace)
            .collect::<Vec<_>>();
        assert_eq!(
            after_docs, before_docs,
            "memory-space replace must preserve subject-global Soul namespace {namespace}"
        );
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn production_shared_program_archive_replaces_only_shared_program_owners() {
    fn write_skill(
        runtime: &bm_sdk::MemoryRuntime,
        owning_scope: RuntimeSkillOwningScope,
        name: &str,
    ) {
        let report = runtime
            .write(MemoryWriteRequest::Procedural {
                writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                    name: name.to_string(),
                    topic: "archive scope".to_string(),
                    title: format!("{name} title"),
                    summary: format!("{name} summary"),
                    content: format!("1. execute {name}\n2. verify {name}"),
                    citations: vec![format!("governed:{name}")],
                    source_chat_id: Some(format!("chat:{name}")),
                    observed_at: 1_800_000_000,
                })],
                owning_scope,
                source: RuntimeSkillWriteSource::Manual,
            })
            .expect("typed runtime skill write");
        assert!(report.accepted);
    }

    let profile = support::host_test_profile();
    let source_runtime = support::test_runtime(support::empty_store_platform(profile), profile);
    write_skill(
        &source_runtime,
        RuntimeSkillOwningScope::SharedProgram,
        "shared_program_archive",
    );
    let shared_scope =
        MemoryArchiveScope::shared_program(source_runtime.memory_space_id()).unwrap();
    let shared_archive = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: shared_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("SharedProgram export");
    assert_eq!(shared_archive.archive.root().scope, shared_scope);

    let target_platform = support::empty_store_platform(profile);
    let target_runtime = support::test_runtime(target_platform.clone(), profile);
    write_skill(
        &target_runtime,
        support::runtime_skill_subject_scope(),
        "subject_archive_sibling",
    );
    let subject_scope = MemoryArchiveScope::subject(
        target_runtime.memory_space_id(),
        target_runtime.subject_id(),
    )
    .unwrap();
    let subject_before = target_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: subject_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("Subject archive before SharedProgram restore")
        .archive
        .root()
        .clone();

    target_runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope: shared_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: shared_archive.archive,
        })
        .expect("SharedProgram restore");

    let subject_after = target_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: subject_scope,
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("Subject archive after SharedProgram restore")
        .archive
        .root()
        .clone();
    assert_eq!(subject_after, subject_before);

    let owners = target_platform
        .replay_harness()
        .read_json_namespace("runtime_skill_records")
        .expect("typed RuntimeSkill owners");
    assert_eq!(owners.len(), 2);
    let owning_kinds = owners
        .iter()
        .map(|doc| {
            doc.value["owning_scope"]["kind"]
                .as_str()
                .expect("typed owning scope kind")
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        owning_kinds,
        std::collections::BTreeSet::from(["shared_program", "subject"])
    );
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn exclude_private_rebuilds_runtime_skill_scope_closure_without_raw_owner() {
    let profile = support::host_test_profile();
    let source_runtime = support::test_runtime(support::empty_store_platform(profile), profile);
    let write = source_runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![GovernedRuntimeSkillWriteInput {
                write: RuntimeSkillWrite {
                    name: "private_archive_skill".to_string(),
                    topic: "private archive".to_string(),
                    title: "Never disclose this title".to_string(),
                    summary: "Never disclose this summary".to_string(),
                    content: "PRIVATE_RUNTIME_SKILL_SENTINEL\n1. inspect the governed request and verify its typed authority\n2. execute only within the bound memory scope\n3. validate the exact post-image before reporting success".to_string(),
                    citations: vec!["governed:private".to_string()],
                    source_chat_id: Some("chat:private".to_string()),
                    observed_at: 1_800_000_000,
                },
                creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                    candidate_ref: "test:private-archive-skill".to_string(),
                    verification_receipt_digest:
                        "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                            .to_string(),
                },
                privacy_class: MemoryPrivacyClass::SoulPrivate,
            }],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("private RuntimeSkill write");
    assert!(write.accepted, "{write:?}");

    let scope = MemoryArchiveScope::subject(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .unwrap();
    let exported = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        })
        .expect("private-excluding archive must rebuild the typed scope closure");
    assert!(!exported
        .archive
        .contains_json_namespace("runtime_skill_records"));

    let target_platform = support::empty_store_platform(profile);
    let target_runtime = support::test_runtime(target_platform.clone(), profile);
    target_runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: exported.archive,
        })
        .expect("redacted archive remains exactly restorable");
    assert!(target_platform
        .replay_harness()
        .read_json_namespace("runtime_skill_records")
        .expect("target runtime owners")
        .is_empty());
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn exclude_private_rebuilds_long_term_version_closure_without_private_history() {
    const PRIVATE_SENTINEL: &str = "Verify release artifacts before publishing.";

    let profile = support::host_test_profile();
    let source_platform = support::seeded_store_platform(profile);
    let source_runtime = support::test_runtime(source_platform.clone(), profile);
    let source_records = source_runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                limit: 20,
                ..LongTermMemoryQuery::default()
            },
            cursor: None,
            limit: 20,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("source long-term records");
    let private_owner_id = source_records
        .records
        .iter()
        .find(|record| record.record.content == PRIVATE_SENTINEL)
        .expect("seeded private-owner candidate")
        .record
        .id
        .clone();
    let privacy_transition = source_runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangePrivacy {
                target: MemoryLongTermTarget::RecordId(private_owner_id.clone()),
                privacy: MemoryPrivacyClass::SoulPrivate,
            },
            reason: "exercise typed archive private-history closure".to_string(),
            dry_run: false,
            mode_input: bm_sdk::RuntimeLifecycleModeInput::default(),
        })
        .expect("move owner behind SoulPrivate");
    assert!(privacy_transition.accepted);

    let scope = MemoryArchiveScope::subject(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .unwrap();
    let exported = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        })
        .expect("private-excluding archive must rebuild long-term root/head/material closure");

    let target_runtime = support::test_runtime(support::empty_store_platform(profile), profile);
    target_runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: exported.archive,
        })
        .expect("redacted long-term archive remains exactly restorable");
    let target_records = target_runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                limit: 20,
                ..LongTermMemoryQuery::default()
            },
            cursor: None,
            limit: 20,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("target long-term records");
    assert!(target_records.records.iter().all(|record| {
        record.record.id != private_owner_id && record.record.content != PRIVATE_SENTINEL
    }));
}
