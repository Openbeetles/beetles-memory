#![cfg(all(feature = "nonproduction-replay-harness", feature = "sqlite-store"))]

mod support;

use bm_sdk::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

struct Clock(AtomicU64);
impl MemoryClock for Clock {
    fn now_secs(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct NoProvider;
impl GovernanceExecutionPort for NoProvider {
    fn execute(
        &mut self,
        _: &AuthorizedGovernanceEnvelope,
        _: &ImmutableGovernanceExecutionBinding,
        _: &GovernanceEgressAuthority,
        _: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        panic!("procedural usage must not invoke a Provider")
    }
}

fn project(runtime: &MemoryRuntime, turn_id: &str) -> MemoryProjectionOutput {
    runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: turn_id.into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "unpack archive".into(),
            system_max_len: 16_384,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("formal projection")
}

fn current(runtime: &MemoryRuntime) -> RuntimeSkillSummary {
    let mut report = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: runtime.subject_id().into(),
            },
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 10,
        })
        .expect("current RuntimeSkill");
    assert_eq!(report.skills.len(), 1);
    report.skills.remove(0)
}

fn seed(runtime: &MemoryRuntime) {
    runtime.seed_runtime_skills_for_replay(vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
        name: "runtime_skill__unpack_archive".into(), topic: "unpack archive".into(), title: "Unpack archive safely".into(),
        summary: "Unpack archive with validation".into(), content: "1. Inspect the archive manifest\n2. Unpack archive into the selected workspace\n3. Verify extracted files".into(),
        citations: vec!["synthetic governed archive fixture".into()], source_chat_id: Some("archive-chat".into()),
        observed_at: runtime.config().clock.now_secs(),
    })], RuntimeSkillOwningScope::Subject { mounted_subject_id: runtime.subject_id().into() }).unwrap();
}

#[test]
fn public_archive_preserves_local_runtime_owners_without_cross_store_cloning_on_three_backends() {
    let path = std::env::temp_dir().join(format!(
        "bm-runtime-public-protection-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    let profile = support::host_test_profile();
    let configs = [
        StoreBackendConfig::in_memory(profile).unwrap(),
        StoreBackendConfig::file(path.join("file"), profile).unwrap(),
        StoreBackendConfig::sqlite(path.join("sqlite.db"), profile).unwrap(),
    ];
    for config in configs {
        let store = support::open_memory_store(config.clone()).unwrap();
        let runtime = support::test_runtime_with_identity_scope(
            store.clone(),
            profile,
            "archive-agent",
            "archive-owner",
            "sdk.direct",
            "archive-chat",
        );
        seed(&runtime);
        let original = current(&runtime);
        assert!(original.enabled);
        let scope =
            MemoryArchiveScope::subject(runtime.memory_space_id(), runtime.subject_id()).unwrap();
        let mut archives = Vec::new();
        for policy in [
            MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            MemorySpacePrivateMaterialPolicy::IncludePrivate,
        ] {
            let archive = runtime
                .export_memory_space(MemorySpaceExportRequest {
                    scope: scope.clone(),
                    private_material_policy: policy,
                })
                .unwrap()
                .archive;
            for namespace in ["runtime_skill_records", "runtime_skill_scope_manifests"] {
                assert!(
                    !archive.contains_json_namespace(namespace),
                    "public archive never carries raw Runtime owner: {namespace}"
                );
                assert!(
                    !archive.contains_event_plane(namespace),
                    "public archive never carries authoritative Runtime events"
                );
            }
            archives.push((policy, archive));
        }
        runtime
            .retire_runtime_skill(RuntimeSkillRetireRequest {
                locator: original.locator,
                observed_at: 1_800_000_000,
            })
            .unwrap();
        let retired = current(&runtime);
        assert!(!retired.enabled);
        let before = store.export_replay_snapshot().unwrap();
        let owner_docs = before
            .json_docs
            .iter()
            .filter(|doc| {
                matches!(
                    doc.namespace.as_str(),
                    "runtime_skill_records" | "runtime_skill_scope_manifests"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let owner_events = before
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event.plane.as_str(),
                    "runtime_skill_records" | "runtime_skill_scope_manifests"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        for namespace in ["runtime_skill_records", "runtime_skill_scope_manifests"] {
            assert!(
                owner_docs.iter().any(|doc| doc.namespace == namespace),
                "real owner closure positive: {namespace}"
            );
        }
        assert!(
            !owner_events.is_empty(),
            "real authoritative Runtime events positive"
        );
        for (policy, archive) in archives {
            runtime
                .import_memory_space(MemorySpaceImportRequest {
                    scope: scope.clone(),
                    expected_private_material_policy: policy,
                    archive: archive.clone(),
                })
                .unwrap();
            let after = store.export_replay_snapshot().unwrap();
            assert_eq!(
                after
                    .json_docs
                    .iter()
                    .filter(|doc| matches!(
                        doc.namespace.as_str(),
                        "runtime_skill_records" | "runtime_skill_scope_manifests"
                    ))
                    .cloned()
                    .collect::<Vec<_>>(),
                owner_docs
            );
            assert_eq!(
                after
                    .events
                    .iter()
                    .filter(|event| matches!(
                        event.plane.as_str(),
                        "runtime_skill_records" | "runtime_skill_scope_manifests"
                    ))
                    .cloned()
                    .collect::<Vec<_>>(),
                owner_events
            );
            let empty_store = support::empty_store_platform(profile);
            let destination = support::test_runtime_with_identity_scope(
                empty_store,
                profile,
                "archive-agent",
                "archive-owner",
                "sdk.direct",
                "archive-chat",
            );
            destination
                .import_memory_space(MemorySpaceImportRequest {
                    scope: scope.clone(),
                    expected_private_material_policy: policy,
                    archive,
                })
                .unwrap();
            assert!(
                destination
                    .list_runtime_skills(RuntimeSkillListRequest {
                        owning_scope: RuntimeSkillOwningScope::Subject {
                            mounted_subject_id: destination.subject_id().into()
                        },
                        query: None,
                        include_disabled: true,
                        include_retired: true,
                        limit: 10
                    })
                    .unwrap()
                    .skills
                    .is_empty(),
                "public archive cannot create a cloned Runtime owner in another Store"
            );
        }
        drop(runtime);
        drop(store);
        if config.backend() != StoreBackendKind::InMemory {
            let reopened = support::open_memory_store(config).unwrap();
            let reopened_runtime = support::test_runtime_with_identity_scope(
                reopened.clone(),
                profile,
                "archive-agent",
                "archive-owner",
                "sdk.direct",
                "archive-chat",
            );
            assert_eq!(
                current(&reopened_runtime).locator.owner_revision(),
                retired.locator.owner_revision()
            );
            assert!(!current(&reopened_runtime).enabled);
            let reopened_snapshot = reopened.export_replay_snapshot().unwrap();
            assert_eq!(
                reopened_snapshot
                    .json_docs
                    .iter()
                    .filter(|doc| matches!(
                        doc.namespace.as_str(),
                        "runtime_skill_records" | "runtime_skill_scope_manifests"
                    ))
                    .cloned()
                    .collect::<Vec<_>>(),
                owner_docs
            );
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn old_public_archive_cannot_reactivate_a_withdrawn_runtime_skill() {
    let path = std::env::temp_dir().join(format!(
        "bm-runtime-archive-lifecycle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    let config =
        StoreBackendConfig::sqlite(path.join("store.sqlite3"), support::host_test_profile())
            .unwrap();
    let store = support::open_memory_store(config.clone()).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let runtime = Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("archive-agent", "archive-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "archive-chat").unwrap())
            .store(store.clone())
            .clock(clock.clone())
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
            .build()
            .unwrap(),
    );
    seed(&runtime);
    clock.0.fetch_add(2, Ordering::SeqCst);
    let positive = project(&runtime, "usage-source");
    let selection = positive.report().selection_receipt().unwrap().clone();
    assert_eq!(
        selection.runtime_skills.len(),
        1,
        "nonempty real delivery positive"
    );
    let selected = selection.runtime_skills[0].clone();
    let finalized = runtime
        .finalize_turn(MemoryTurnFinalizeRequest {
            turn: CanonicalTurnDelta {
                turn_id: "usage-source".into(),
                conversation: ConversationScope {
                    channel: "sdk.direct".into(),
                    chat_id: "archive-chat".into(),
                    conversation_id: Some("archive-chat".into()),
                },
                subject: runtime.subject_id().into(),
                delivery_status: MemoryTurnDeliveryStatus::Delivered,
                source: MemoryTurnSource {
                    ingress: IngressKind::User,
                    channel: "sdk.direct".into(),
                    provider: None,
                    protocol: MemoryTurnProtocol::Native,
                    endpoint: None,
                    model_alias: None,
                    model_resolved: None,
                    request_id: None,
                    client_conversation_hint: None,
                },
                actor: None,
                input_messages: vec![TranscriptInputMessage::user("unpack archive")],
                assistant_message: Some(TranscriptInputMessage::assistant(
                    "archive unpack completed",
                )),
                tool_observations: Vec::new(),
                external_content_used: false,
                candidate_ids: Vec::new(),
            },
            learning: PostTurnLearningInputV1 {
                selection_receipt: Some(selection),
                runtime_skill_feedback: vec![RuntimeSkillUsageFeedbackV1 {
                    locator: selected.locator,
                    selected_content_digest: selected.content_digest,
                    outcome: ProceduralExecutionOutcomeV1::Succeeded,
                    observation_ref: "synthetic-actual-usage".into(),
                }],
                ..PostTurnLearningInputV1::empty()
            },
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .unwrap();
    let engine = MemoryLearningEngine::attach(runtime.clone()).unwrap();
    let outcome = engine
        .run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "archive-worker".into(),
                lease_duration_secs: 60,
            },
            &mut NoProvider,
        )
        .unwrap();
    let MemoryLearningCycleOutcome::ProceduralCompleted(completed) = outcome else {
        panic!("official worker must complete: {outcome:?}")
    };
    assert_eq!(
        Some(completed.job.job_id),
        finalized.procedural_learning.job_id
    );
    assert_eq!(completed.receipt.accepted_count, 1);
    assert_eq!(completed.receipt.changed_count, 1);
    assert_eq!(current(&runtime).validated_success_count, 1);
    let scope =
        MemoryArchiveScope::subject(runtime.memory_space_id(), runtime.subject_id()).unwrap();
    let archive = runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .unwrap()
        .archive;
    clock.0.fetch_add(2, Ordering::SeqCst);
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: "archive-chat".into(),
            turn_id: Some("usage-source".into()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "synthetic evidence withdrawal".into(),
        })
        .unwrap();
    let retired = current(&runtime);
    assert!(!retired.enabled);
    assert!(project(&runtime, "after-withdrawal")
        .report()
        .selection_receipt()
        .unwrap()
        .runtime_skills
        .is_empty());
    runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive,
        })
        .unwrap();
    let restored = current(&runtime);
    assert_eq!(
        restored.locator.owner_revision(),
        retired.locator.owner_revision(),
        "public restore cannot roll back authoritative usage/withdrawal revision"
    );
    assert!(
        !restored.enabled,
        "public restore cannot reactivate withdrawn skill"
    );
    assert!(project(&runtime, "after-old-archive-restore")
        .report()
        .selection_receipt()
        .unwrap()
        .runtime_skills
        .is_empty());
    drop(engine);
    drop(runtime);
    drop(store);
    let reopened = support::open_memory_store(config).expect("reopen restored Store");
    let reopened_runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("archive-agent", "archive-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "archive-chat").unwrap())
        .store(reopened.clone())
        .clock(clock)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .build()
        .unwrap();
    assert!(!current(&reopened_runtime).enabled);
    assert!(project(&reopened_runtime, "reopened-after-restore")
        .report()
        .selection_receipt()
        .unwrap()
        .runtime_skills
        .is_empty());
    drop(reopened_runtime);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}
