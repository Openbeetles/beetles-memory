mod support;

use bm_core::feature_gate::ProfileId;
use bm_core::memory::SubjectSoulOwnedDocumentV1;
use bm_sdk::nonproduction_replay_harness::{
    MemoryStoreEventKind, StoreBackendConfig, StoreCapacityBudget, StoreEngine, StoreEventScope,
    StoreJsonPrecondition, StoreMutation, StoreMutationBatch, STORE_SCHEMA_ID,
    STORE_SCHEMA_VERSION,
};
use bm_sdk::{
    default_agent_subject_id, primary_human_subject_id, system_governor_subject_id,
    HumanSoulLifecycleConfirmationV1, MemoryIdentity, MemoryRuntime, MemoryScope,
    MemoryStoreHandle, RelationshipAccessConstraintV1, RelationshipDisclosureCeilingV1,
    RelationshipSourceClausesV1, RelationshipSourceControlAuthorityV1,
    RelationshipSourceControlErrorKeyV1, RelationshipSourceControlIntentActionV1,
    RelationshipSourceControlIntentV1, RelationshipSourceControlOutcomeV1,
    RelationshipSourceExpectedStateV1, RelationshipSourceReadRequestV1,
    RelationshipSourceReadSelectorV1, SubjectRegistry, SubjectRelationshipGraph,
    SubjectRelationshipKind, SubjectSoulExpectedStateV1, SubjectSoulFoundingCharterSeedV1,
    SubjectSoulLifecycleActionV1, SubjectSoulLifecycleAuthorityV1, SubjectSoulLifecycleErrorKey,
    SubjectSoulLifecycleMutationRequestV1, SubjectSoulLifecycleStateV1,
    SubjectSoulMutationOutcomeV1, SubjectSoulProvisionIntentV1, SubjectSoulReadOutcomeV1,
    SubjectSoulReadRequestV1, SubjectSoulReadSelectorV1, SubjectSoulReadViewV1,
    SubjectSoulTerminalActionV1, VerifiedSubjectSoulReadViewV1,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temp_root(backend: &str, scenario: &str) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-{backend}-subject-soul-{scenario}-{}-{sequence}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn orphan_subject_soul_value() -> serde_json::Value {
    json!({
        "memory_space_id": "space:test",
        "subject_id": "subject:agent-a",
        "soul_id": "soul-a",
        "generation": 1,
        "body": "orphan subject Soul material"
    })
}

fn founding_intent(operation_id: &str) -> SubjectSoulProvisionIntentV1 {
    SubjectSoulProvisionIntentV1::Founding {
        operation_id: operation_id.to_string(),
        human_actor_subject_id: primary_human_subject_id("test"),
        charter: Box::new(
            SubjectSoulFoundingCharterSeedV1 {
                identity_anchor: Some("A persistent independent collaborator".to_string()),
                character_tendencies: vec!["verify before claiming".to_string()],
                priority_constitution: vec!["truth before fluency".to_string()],
                non_negotiables: vec!["never fabricate evidence".to_string()],
                default_response_mode: Some("direct and evidence-led".to_string()),
                default_initiative_posture: None,
                default_relationship_posture: None,
                boundary_doctrine: None,
                truth_seeking_commitment: Some("state uncertainty explicitly".to_string()),
                self_preservation_doctrine: None,
                repair_doctrine: None,
                change_principle: None,
            }
            .canonicalize()
            .expect("canonical founding seed"),
        ),
        source_asserted_at: None,
    }
}

fn relationship_runtime_for_scope(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
    relationship_id: &str,
) -> MemoryRuntime {
    let owner = "test";
    let agent = "store-contract";
    let registry = SubjectRegistry::single_agent_default(owner, agent).expect("subject registry");
    let mounted = default_agent_subject_id(agent);
    let human = primary_human_subject_id(owner);
    let mut graph = SubjectRelationshipGraph::single_agent_default(&registry)
        .expect("subject relationship graph");
    for edge in &mut graph.edges {
        if (edge.from_subject_id == mounted && edge.to_subject_id == human)
            || (edge.from_subject_id == human && edge.to_subject_id == mounted)
        {
            edge.relationship_id = Some(relationship_id.to_string());
        }
    }
    assert!(graph.edges.iter().any(|edge| {
        edge.relationship_id.as_deref() == Some(relationship_id)
            && edge.kind == SubjectRelationshipKind::CollaboratesWith
    }));
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent, owner).expect("runtime identity"))
        .scope(MemoryScope::new("test", "chat-a").expect("runtime scope"))
        .store(MemoryStoreHandle::from_nonproduction_store_platform(
            platform.clone(),
        ))
        .subject_registry(registry)
        .subject_relationship_graph(graph)
        .build()
        .expect("relationship runtime");
    assert_eq!(runtime.memory_space_id(), "space:test");
    runtime
}

fn relationship_clauses(extra_repair: Option<&str>) -> RelationshipSourceClausesV1 {
    let mut repair_commitments = vec!["repair before escalation".to_string()];
    if let Some(extra) = extra_repair {
        repair_commitments.push(extra.to_string());
        repair_commitments.sort();
    }
    RelationshipSourceClausesV1 {
        disclosure_ceiling: RelationshipDisclosureCeilingV1::GovernedSummary,
        access_constraints: vec![
            RelationshipAccessConstraintV1::NoPrivateRaw,
            RelationshipAccessConstraintV1::GovernedDisclosureOnly,
        ],
        truth_commitments: vec!["state uncertainty".to_string()],
        mutual_boundary_commitments: vec!["respect explicit refusal".to_string()],
        repair_commitments,
    }
}

fn create_relationship_root(
    runtime: &MemoryRuntime,
    relationship_id: &str,
) -> bm_sdk::RelationshipSourceControlReportV1 {
    let human = primary_human_subject_id("test");
    runtime
        .control_relationship_source(RelationshipSourceControlIntentV1 {
            operation_id: format!("{relationship_id}:create"),
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            counterparty_subject_ids: vec![human.clone()],
            expected_state: runtime
                .relationship_source_pristine_expected_state(relationship_id)
                .expect("pinned relationship pristine proof"),
            authority: RelationshipSourceControlAuthorityV1::HumanUser {
                actor_subject_id: human,
            },
            action: RelationshipSourceControlIntentActionV1::Create {
                clauses: relationship_clauses(None),
                source_asserted_at: Some(100),
                evidence_digest: "c".repeat(64),
            },
        })
        .expect("create relationship source and Soul projection")
}

fn assert_active_double_root_snapshot(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
    relationship_revision: u64,
) {
    let snapshot = platform
        .export_store_snapshot()
        .expect("export active double-root snapshot");
    for namespace in [
        "subject_soul_lifecycle_heads",
        "subject_soul_scope_manifests",
        "relationship_source_constitutions",
        "relationship_source_scope_manifests",
        "subject_soul_relationship_projections",
    ] {
        assert!(
            snapshot
                .json_docs
                .iter()
                .any(|document| document.namespace == namespace),
            "active double-root closure is missing {namespace}"
        );
    }
    let projection = snapshot
        .json_docs
        .iter()
        .find(|document| document.namespace == "subject_soul_relationship_projections")
        .expect("relationship projection document");
    assert_eq!(
        projection.value["relationship_source_revision"],
        json!(relationship_revision)
    );
}

fn run_active_double_root_reopen_and_stale_zero_change(config: StoreBackendConfig) {
    let relationship_id = "relationship:store-double-root";
    {
        let platform = support::open_store(config.clone()).expect("open double-root store");
        let runtime = relationship_runtime_for_scope(&platform, relationship_id);
        runtime
            .provision_subject_soul(founding_intent("store-double-root-founding"))
            .expect("provision active Soul before relationship source");
        let created = create_relationship_root(&runtime, relationship_id);
        assert_eq!(
            created.outcome,
            RelationshipSourceControlOutcomeV1::Committed
        );
        assert_active_double_root_snapshot(&platform, 1);
    }

    {
        let platform =
            support::open_store(config.clone()).expect("reopen active double-root store");
        let runtime = relationship_runtime_for_scope(&platform, relationship_id);
        let current = runtime
            .read_relationship_source(RelationshipSourceReadRequestV1 {
                memory_space_id: runtime.memory_space_id().to_string(),
                relationship_id: relationship_id.to_string(),
                mounted_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
                selector: RelationshipSourceReadSelectorV1::Current,
            })
            .expect("read verified relationship double root after reopen");
        assert_eq!(current.current_revision, 1);
        let stale_expected = RelationshipSourceExpectedStateV1::Exact {
            revision: current.current_revision,
            state: current.current_state.expect("current relationship state"),
            source_digest: current.current_source_digest.clone(),
            manifest_digest: current.current_manifest_digest.clone(),
        };
        let human = primary_human_subject_id("test");
        let successor = runtime
            .control_relationship_source(RelationshipSourceControlIntentV1 {
                operation_id: "store-double-root-update-winner".to_string(),
                memory_space_id: runtime.memory_space_id().to_string(),
                relationship_id: relationship_id.to_string(),
                mounted_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
                counterparty_subject_ids: vec![human.clone()],
                expected_state: stale_expected.clone(),
                authority: RelationshipSourceControlAuthorityV1::HumanUser {
                    actor_subject_id: human.clone(),
                },
                action: RelationshipSourceControlIntentActionV1::UpdateContribution {
                    clauses: relationship_clauses(Some("return after repair")),
                    source_asserted_at: Some(101),
                    evidence_digest: "d".repeat(64),
                },
            })
            .expect("commit reopened double-root successor");
        assert_eq!(successor.revision, 2);
        assert_active_double_root_snapshot(&platform, 2);
        let before_stale = platform
            .export_store_snapshot()
            .expect("snapshot before stale double-root request");
        let stale = runtime
            .control_relationship_source(RelationshipSourceControlIntentV1 {
                operation_id: "store-double-root-update-stale".to_string(),
                memory_space_id: runtime.memory_space_id().to_string(),
                relationship_id: relationship_id.to_string(),
                mounted_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
                counterparty_subject_ids: vec![human.clone()],
                expected_state: stale_expected,
                authority: RelationshipSourceControlAuthorityV1::HumanUser {
                    actor_subject_id: human,
                },
                action: RelationshipSourceControlIntentActionV1::UpdateContribution {
                    clauses: relationship_clauses(Some("must remain absent")),
                    source_asserted_at: Some(102),
                    evidence_digest: "e".repeat(64),
                },
            })
            .expect_err("stale double-root request must fail closed");
        assert_eq!(
            stale.key,
            RelationshipSourceControlErrorKeyV1::RevisionConflict
        );
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("snapshot after stale double-root rejection"),
            before_stale,
            "stale Soul head/manifest plus relationship source/manifest roots must leave all documents, receipts, audits, and events unchanged"
        );
    }

    let platform = support::open_store(config).expect("reopen double-root successor");
    let runtime = relationship_runtime_for_scope(&platform, relationship_id);
    let current = runtime
        .read_relationship_source(RelationshipSourceReadRequestV1 {
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector: RelationshipSourceReadSelectorV1::Current,
        })
        .expect("read double-root successor after second reopen");
    assert_eq!(current.current_revision, 2);
    assert_active_double_root_snapshot(&platform, 2);
}

fn assert_current_founding(runtime: &bm_sdk::MemoryRuntime) {
    let outcome = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("read persisted founding Soul");
    let SubjectSoulReadOutcomeV1::Verified { view } = outcome else {
        panic!("founding Soul must be a verified persisted root")
    };
    assert_eq!(view.state, SubjectSoulLifecycleStateV1::Active);
    assert_eq!(view.generation, 1);
    assert_eq!(view.revision, Some(1));
    assert!(view.material_digest.is_some());
}

fn current_verified_soul(runtime: &bm_sdk::MemoryRuntime) -> Box<VerifiedSubjectSoulReadViewV1> {
    let outcome = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("read current verified Soul");
    let SubjectSoulReadOutcomeV1::Verified { view } = outcome else {
        panic!("expected a persisted verified Soul root")
    };
    view
}

fn exact_expected_state(view: &VerifiedSubjectSoulReadViewV1) -> SubjectSoulExpectedStateV1 {
    SubjectSoulExpectedStateV1::Exact {
        generation: view.generation,
        revision: view.revision,
        lifecycle_state: view.state,
        head_digest: view.head_digest.clone(),
        manifest_digest: view.manifest_digest.clone(),
    }
}

fn exact_selector(view: &VerifiedSubjectSoulReadViewV1) -> SubjectSoulReadSelectorV1 {
    SubjectSoulReadSelectorV1::Exact {
        generation: view.generation,
        revision: view.revision.expect("active Soul revision"),
        material_digest: view
            .material_digest
            .clone()
            .expect("active Soul material digest"),
    }
}

fn founding_raw_addresses(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
) -> Vec<(String, String)> {
    founding_raw_documents(platform)
        .into_iter()
        .map(|(namespace, key, _)| (namespace, key))
        .collect()
}

fn founding_raw_documents(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
) -> Vec<(String, String, serde_json::Value)> {
    platform
        .export_store_snapshot()
        .expect("snapshot Soul raw closure")
        .json_docs
        .into_iter()
        .filter(|document| {
            matches!(
                document.namespace.as_str(),
                "subject_soul_revision_materials" | "self_authored_core" | "core_revision_ledger"
            )
        })
        .map(|document| (document.namespace, document.key, document.value))
        .collect()
}

fn assert_addresses_absent(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
    addresses: &[(String, String)],
) {
    let snapshot = platform
        .export_store_snapshot()
        .expect("snapshot after destructive lifecycle");
    for address in addresses {
        assert!(
            !snapshot.json_docs.iter().any(|document| {
                (&document.namespace, &document.key) == (&address.0, &address.1)
            }),
            "terminated generation raw address survived destructive lifecycle: {address:?}"
        );
    }
}

fn assert_reseed_replaced_or_removed_old_generation(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
    old_documents: &[(String, String, serde_json::Value)],
) {
    let snapshot = platform
        .export_store_snapshot()
        .expect("snapshot after reseed");
    for (namespace, key, old_value) in old_documents {
        let current = snapshot
            .json_docs
            .iter()
            .find(|document| document.namespace == *namespace && document.key == *key);
        if namespace == "subject_soul_revision_materials" {
            assert!(
                current.is_none(),
                "old generation material address survived reseed"
            );
        } else {
            let current = current.expect("scope-key current view must be atomically replaced");
            assert_ne!(
                &current.value, old_value,
                "old generation current-view bytes survived reseed"
            );
            let envelope: SubjectSoulOwnedDocumentV1 =
                serde_json::from_value(current.value.clone())
                    .expect("decode reseeded Core/ledger envelope");
            assert_eq!(envelope.generation, 2);
        }
    }
}

fn reset_founding_and_assert_raw_purge(
    platform: &bm_sdk::nonproduction_replay_harness::StorePlatform,
    runtime: &bm_sdk::MemoryRuntime,
    operation_id: &str,
) -> SubjectSoulReadSelectorV1 {
    let current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("read founding Soul before reset");
    let SubjectSoulReadOutcomeV1::Verified { view } = current else {
        panic!("reset fixture requires a verified founding Soul")
    };
    let revision = view.revision.expect("founding revision");
    let material_digest = view
        .material_digest
        .clone()
        .expect("founding material digest");
    let selector = SubjectSoulReadSelectorV1::Exact {
        generation: view.generation,
        revision,
        material_digest: material_digest.clone(),
    };
    let before = platform
        .export_store_snapshot()
        .expect("snapshot before destructive reset");
    let raw_addresses = before
        .json_docs
        .iter()
        .filter(|document| {
            matches!(
                document.namespace.as_str(),
                "subject_soul_revision_materials" | "self_authored_core" | "core_revision_ledger"
            )
        })
        .map(|document| (document.namespace.clone(), document.key.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        raw_addresses.len(),
        3,
        "founding fixture must contain real raw closure"
    );

    let target_subject_id = runtime.scoped_runtime().mounted_subject_id.clone();
    let report = runtime
        .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
            operation_id: operation_id.to_string(),
            target_subject_id: target_subject_id.clone(),
            expected_state: SubjectSoulExpectedStateV1::Exact {
                generation: view.generation,
                revision: view.revision,
                lifecycle_state: view.state,
                head_digest: view.head_digest.clone(),
                manifest_digest: view.manifest_digest.clone(),
            },
            authority: SubjectSoulLifecycleAuthorityV1::Destructive {
                system_actor_subject_id: system_governor_subject_id("test"),
                human_confirmation: HumanSoulLifecycleConfirmationV1 {
                    human_subject_id: primary_human_subject_id("test"),
                    target_subject_id,
                    expected_generation: view.generation,
                    action: SubjectSoulTerminalActionV1::Reset,
                    reason_code: "store_contract_reset".to_string(),
                    confirmed_at: 99,
                    evidence_digest: "e".repeat(64),
                },
            },
            action: SubjectSoulLifecycleActionV1::Reset {
                reason_code: "store_contract_reset".to_string(),
            },
        })
        .expect("reset founding Soul atomically");
    assert_eq!(report.outcome, SubjectSoulMutationOutcomeV1::Committed);
    assert_eq!(report.generation, view.generation + 1);
    assert_eq!(report.state_after, SubjectSoulLifecycleStateV1::Unseeded);

    let after = platform
        .export_store_snapshot()
        .expect("snapshot after destructive reset");
    for address in &raw_addresses {
        assert!(
            !after
                .json_docs
                .iter()
                .any(|document| (&document.namespace, &document.key) == (&address.0, &address.1)),
            "terminated generation raw address survived reset: {address:?}"
        );
    }
    assert_terminated_exact(
        runtime,
        selector.clone(),
        view.generation,
        revision,
        &material_digest,
        SubjectSoulTerminalActionV1::Reset,
    );
    let current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("read explicit unseeded generation after reset");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("reset must retain an explicit lifecycle root")
    };
    assert_eq!(current.state, SubjectSoulLifecycleStateV1::Unseeded);
    assert_eq!(current.revision, None);
    assert_eq!(current.material_digest, None);
    selector
}

fn assert_terminated_exact(
    runtime: &bm_sdk::MemoryRuntime,
    selector: SubjectSoulReadSelectorV1,
    generation: u64,
    revision: u64,
    material_digest: &str,
    terminal_action: SubjectSoulTerminalActionV1,
) {
    let exact = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("read terminated generation metadata");
    let SubjectSoulReadOutcomeV1::TerminatedGeneration { terminal, .. } = exact else {
        panic!("terminated exact selector must not recover raw Soul body")
    };
    assert_eq!(terminal.generation, generation);
    assert_eq!(terminal.terminal_revision, Some(revision));
    assert_eq!(
        terminal.terminal_material_digest.as_deref(),
        Some(material_digest)
    );
    assert_eq!(terminal.terminal_action, terminal_action);
}

fn destructive_authority(
    target_subject_id: &str,
    expected_generation: u64,
    action: SubjectSoulTerminalActionV1,
    reason_code: &str,
    confirmed_at: u64,
) -> SubjectSoulLifecycleAuthorityV1 {
    SubjectSoulLifecycleAuthorityV1::Destructive {
        system_actor_subject_id: system_governor_subject_id("test"),
        human_confirmation: HumanSoulLifecycleConfirmationV1 {
            human_subject_id: primary_human_subject_id("test"),
            target_subject_id: target_subject_id.to_string(),
            expected_generation,
            action,
            reason_code: reason_code.to_string(),
            confirmed_at,
            evidence_digest: "f".repeat(64),
        },
    }
}

fn run_archive_restore_reseed_delete_reopen(config: StoreBackendConfig) {
    {
        let platform = support::open_store(config.clone()).expect("open lifecycle store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        runtime
            .provision_subject_soul(founding_intent("store-lifecycle-founding"))
            .expect("commit lifecycle founding Soul");
        let current = current_verified_soul(&runtime);
        let archived = runtime
            .archive_subject_soul_self_governed(
                "store-lifecycle-archive",
                exact_expected_state(&current),
            )
            .expect("archive Soul before reopen");
        assert_eq!(archived.state_after, SubjectSoulLifecycleStateV1::Archived);
    }

    {
        let platform = support::open_store(config.clone()).expect("reopen archived Soul");
        let runtime = support::runtime_for_scope(&platform, "space:test", 101);
        let archived = current_verified_soul(&runtime);
        assert_eq!(archived.state, SubjectSoulLifecycleStateV1::Archived);
        let restored = runtime
            .restore_subject_soul_self_governed(
                "store-lifecycle-restore",
                exact_expected_state(&archived),
            )
            .expect("restore archived Soul before reopen");
        assert_eq!(restored.state_after, SubjectSoulLifecycleStateV1::Active);
    }

    let generation_one_selector;
    let generation_one_revision;
    let generation_one_material_digest;
    {
        let platform = support::open_store(config.clone()).expect("reopen restored Soul");
        let runtime = support::runtime_for_scope(&platform, "space:test", 102);
        let restored = current_verified_soul(&runtime);
        assert_eq!(restored.state, SubjectSoulLifecycleStateV1::Active);
        assert_eq!(restored.generation, 1);
        generation_one_selector = exact_selector(&restored);
        generation_one_revision = restored.revision.expect("generation one revision");
        generation_one_material_digest = restored
            .material_digest
            .clone()
            .expect("generation one material digest");
        let generation_one_raw = founding_raw_documents(&platform);
        assert_eq!(generation_one_raw.len(), 3);
        let target_subject_id = runtime.scoped_runtime().mounted_subject_id.clone();
        let reseeded = runtime
            .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
                operation_id: "store-lifecycle-reseed".to_string(),
                target_subject_id: target_subject_id.clone(),
                expected_state: exact_expected_state(&restored),
                authority: destructive_authority(
                    &target_subject_id,
                    restored.generation,
                    SubjectSoulTerminalActionV1::Reseed,
                    "store_contract_reseed",
                    101,
                ),
                action: SubjectSoulLifecycleActionV1::Reseed {
                    charter: Box::new(
                        SubjectSoulFoundingCharterSeedV1 {
                            identity_anchor: Some(
                                "A renewed but lineage-aware independent collaborator".to_string(),
                            ),
                            character_tendencies: vec![
                                "verify durable lineage before claiming".to_string()
                            ],
                            priority_constitution: vec!["truth before fluency".to_string()],
                            non_negotiables: vec!["never fabricate evidence".to_string()],
                            default_response_mode: Some("direct and evidence-led".to_string()),
                            default_initiative_posture: None,
                            default_relationship_posture: None,
                            boundary_doctrine: None,
                            truth_seeking_commitment: Some(
                                "state uncertainty explicitly".to_string(),
                            ),
                            self_preservation_doctrine: None,
                            repair_doctrine: None,
                            change_principle: None,
                        }
                        .canonicalize()
                        .expect("canonical reseed charter"),
                    ),
                    reason_code: "store_contract_reseed".to_string(),
                    source_asserted_at: Some(101),
                },
            })
            .expect("reseed must atomically purge generation one and create generation two");
        assert_eq!(reseeded.state_after, SubjectSoulLifecycleStateV1::Active);
        assert_eq!(reseeded.generation, 2);
        assert_eq!(reseeded.revision, Some(1));
        assert_reseed_replaced_or_removed_old_generation(&platform, &generation_one_raw);
        assert_terminated_exact(
            &runtime,
            generation_one_selector.clone(),
            1,
            generation_one_revision,
            &generation_one_material_digest,
            SubjectSoulTerminalActionV1::Reseed,
        );
    }

    let generation_two_selector;
    let generation_two_revision;
    let generation_two_material_digest;
    {
        let platform = support::open_store(config.clone()).expect("reopen reseeded Soul");
        let runtime = support::runtime_for_scope(&platform, "space:test", 103);
        let reseeded = current_verified_soul(&runtime);
        assert_eq!(reseeded.state, SubjectSoulLifecycleStateV1::Active);
        assert_eq!(reseeded.generation, 2);
        assert_eq!(reseeded.revision, Some(1));
        assert_terminated_exact(
            &runtime,
            generation_one_selector.clone(),
            1,
            generation_one_revision,
            &generation_one_material_digest,
            SubjectSoulTerminalActionV1::Reseed,
        );
        generation_two_selector = exact_selector(&reseeded);
        generation_two_revision = reseeded.revision.expect("generation two revision");
        generation_two_material_digest = reseeded
            .material_digest
            .clone()
            .expect("generation two material digest");
        let generation_two_raw = founding_raw_addresses(&platform);
        assert_eq!(generation_two_raw.len(), 3);
        let target_subject_id = runtime.scoped_runtime().mounted_subject_id.clone();
        let deleted = runtime
            .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
                operation_id: "store-lifecycle-delete".to_string(),
                target_subject_id: target_subject_id.clone(),
                expected_state: exact_expected_state(&reseeded),
                authority: destructive_authority(
                    &target_subject_id,
                    reseeded.generation,
                    SubjectSoulTerminalActionV1::Delete,
                    "store_contract_delete",
                    102,
                ),
                action: SubjectSoulLifecycleActionV1::Delete {
                    reason_code: "store_contract_delete".to_string(),
                },
            })
            .expect("delete must atomically purge generation two");
        assert_eq!(deleted.state_after, SubjectSoulLifecycleStateV1::Deleted);
        assert_eq!(deleted.generation, 2);
        assert_eq!(deleted.revision, None);
        assert_addresses_absent(&platform, &generation_two_raw);
        assert_terminated_exact(
            &runtime,
            generation_two_selector.clone(),
            2,
            generation_two_revision,
            &generation_two_material_digest,
            SubjectSoulTerminalActionV1::Delete,
        );
    }

    let platform = support::open_store(config).expect("reopen deleted Soul");
    let runtime = support::runtime_for_scope(&platform, "space:test", 104);
    let deleted = current_verified_soul(&runtime);
    assert_eq!(deleted.state, SubjectSoulLifecycleStateV1::Deleted);
    assert_eq!(deleted.generation, 2);
    assert_eq!(deleted.revision, None);
    assert_eq!(deleted.material_digest, None);
    assert_terminated_exact(
        &runtime,
        generation_one_selector,
        1,
        generation_one_revision,
        &generation_one_material_digest,
        SubjectSoulTerminalActionV1::Reseed,
    );
    assert_terminated_exact(
        &runtime,
        generation_two_selector,
        2,
        generation_two_revision,
        &generation_two_material_digest,
        SubjectSoulTerminalActionV1::Delete,
    );
    assert!(founding_raw_addresses(&platform).is_empty());
}

#[test]
fn store_schema_v12_is_the_only_current_schema() {
    assert_eq!(STORE_SCHEMA_ID, "beetle_memory_store_schema_v12");
    assert_eq!(STORE_SCHEMA_VERSION, 12);
}

#[test]
fn primitive_subject_soul_write_is_rejected_even_with_an_address_precondition() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("store config");
    let platform = support::open_store(config).expect("open store");
    let namespace = "self_model";
    let key = "space:test/subject:agent-a/soul:soul-a/generation:1/self-model";
    let batch = StoreMutationBatch {
        transaction_id: "txn-forged-primitive-subject-soul".to_string(),
        operation: "test.forged_primitive_subject_soul".to_string(),
        scope: StoreEventScope::system("test.forged_primitive_subject_soul"),
        mutations: vec![StoreMutation::PutJson {
            namespace: namespace.to_string(),
            key: key.to_string(),
            value: json!({
                "memory_space_id": "space:test",
                "subject_id": "subject:agent-a",
                "soul_id": "soul-a",
                "generation": 1,
                "body": "must never enter through a primitive writer"
            }),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: namespace.to_string(),
            record_key: key.to_string(),
        }],
    };
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            batch,
            &[StoreJsonPrecondition::Absent {
                namespace: namespace.to_string(),
                key: key.to_string(),
            }],
        )
        .expect_err("primitive subject Soul write must fail closed");
    assert!(!error.to_string().is_empty());

    let docs = platform
        .read_json_docs_by_keys(namespace, &[key.to_string()])
        .expect("read forged address");
    assert!(
        docs.is_empty(),
        "rejected write must leave exact-zero state"
    );
}

#[test]
fn in_memory_founding_is_one_typed_atomic_owner_closure() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("store config");
    let platform = support::open_store(config).expect("open store");
    let runtime = support::runtime_for_scope(&platform, "space:test", 100);
    let report = runtime
        .provision_subject_soul(founding_intent("store-in-memory-founding"))
        .expect("commit founding Soul");
    assert_eq!(report.outcome, SubjectSoulMutationOutcomeV1::Committed);
    assert_current_founding(&runtime);
}

#[test]
fn in_memory_reset_purges_raw_generation_and_keeps_only_terminated_metadata() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("store config");
    let platform = support::open_store(config).expect("open store");
    let runtime = support::runtime_for_scope(&platform, "space:test", 100);
    runtime
        .provision_subject_soul(founding_intent("store-in-memory-reset-founding"))
        .expect("commit founding Soul");
    reset_founding_and_assert_raw_purge(&platform, &runtime, "store-in-memory-reset");
}

#[test]
fn stale_exact_soul_roots_fail_with_zero_post_image_change() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("store config");
    let platform = support::open_store(config).expect("open store");
    let runtime = support::runtime_for_scope(&platform, "space:test", 100);
    runtime
        .provision_subject_soul(founding_intent("store-stale-cas-founding"))
        .expect("commit founding Soul");
    let active = current_verified_soul(&runtime);
    let stale_expected = exact_expected_state(&active);
    runtime
        .archive_subject_soul_self_governed("store-stale-cas-winner", stale_expected.clone())
        .expect("first exact roots archive wins");
    let before_rejected = platform
        .export_store_snapshot()
        .expect("snapshot before stale exact rejection");
    let error = runtime
        .archive_subject_soul_self_governed("store-stale-cas-loser", stale_expected)
        .expect_err("stale exact head/manifest owner must fail closed");
    assert_eq!(error.key, SubjectSoulLifecycleErrorKey::GenerationConflict);
    assert_eq!(
        platform
            .export_store_snapshot()
            .expect("snapshot after stale exact rejection"),
        before_rejected,
        "stale exact roots must not change head, manifest, result, receipt, audit, or event"
    );
}

#[test]
fn subject_soul_budget_overflow_rejects_the_entire_owner_closure() {
    let mut budget = StoreCapacityBudget::full();
    budget.event_log_max_items = 2;
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("store config")
    .try_with_nonproduction_store_budget_limit(budget.into_runtime_budget())
    .expect("valid constrained Store budget");
    let platform = support::open_store(config).expect("open constrained store");
    let runtime = support::runtime_for_scope(&platform, "space:test", 100);
    let before = platform
        .export_store_snapshot()
        .expect("snapshot before Soul budget rejection");

    let error = runtime
        .provision_subject_soul(founding_intent("store-budget-overflow"))
        .expect_err("Soul closure must exceed the constrained event budget");
    assert_eq!(error.key, SubjectSoulLifecycleErrorKey::CapacityExceeded);
    assert_eq!(
        platform
            .export_store_snapshot()
            .expect("snapshot after Soul budget rejection"),
        before,
        "budget rejection must leave head, manifest, material, Core, ledger, receipt, audit, and events unchanged"
    );
}

#[test]
fn file_reopen_preserves_founding_head_material_envelopes_and_receipt() {
    let root = temp_root("file", "founding-reopen");
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    {
        let platform = support::open_store(config.clone()).expect("open file store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        let report = runtime
            .provision_subject_soul(founding_intent("store-file-founding"))
            .expect("commit file founding Soul");
        assert_eq!(report.outcome, SubjectSoulMutationOutcomeV1::Committed);
        assert_current_founding(&runtime);
    }
    let reopened = support::open_store(config).expect("reopen file Store v10 closure");
    let runtime = support::runtime_for_scope(&reopened, "space:test", 101);
    assert_current_founding(&runtime);
    std::fs::remove_dir_all(root).expect("remove file fixture");
}

#[test]
fn file_reopen_preserves_reset_tombstone_and_no_terminated_raw_body() {
    let root = temp_root("file", "reset-reopen");
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    let selector;
    {
        let platform = support::open_store(config.clone()).expect("open file store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        runtime
            .provision_subject_soul(founding_intent("store-file-reset-founding"))
            .expect("commit file founding Soul");
        selector = reset_founding_and_assert_raw_purge(&platform, &runtime, "store-file-reset");
    }
    let reopened = support::open_store(config).expect("reopen reset file closure");
    let runtime = support::runtime_for_scope(&reopened, "space:test", 101);
    let SubjectSoulReadSelectorV1::Exact {
        generation,
        revision,
        ref material_digest,
    } = selector
    else {
        unreachable!("reset helper returns exact selector")
    };
    assert_terminated_exact(
        &runtime,
        selector.clone(),
        generation,
        revision,
        material_digest,
        SubjectSoulTerminalActionV1::Reset,
    );
    let reopened_snapshot = reopened
        .export_store_snapshot()
        .expect("snapshot reopened reset closure");
    assert!(reopened_snapshot.json_docs.iter().all(|document| !matches!(
        document.namespace.as_str(),
        "subject_soul_revision_materials" | "self_authored_core" | "core_revision_ledger"
    )));
    std::fs::remove_dir_all(root).expect("remove file fixture");
}

#[test]
fn file_reopen_closes_archive_restore_reseed_and_terminal_delete_lifecycle() {
    let root = temp_root("file", "full-lifecycle-reopen");
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    run_archive_restore_reseed_delete_reopen(config);
    std::fs::remove_dir_all(root).expect("remove file fixture");
}

#[test]
fn file_reopen_preserves_active_relationship_double_root_and_stale_zero_change() {
    let root = temp_root("file", "active-double-root-reopen");
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    run_active_double_root_reopen_and_stale_zero_change(config);
    std::fs::remove_dir_all(root).expect("remove file fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_founding_head_material_envelopes_and_receipt() {
    let path = temp_root("sqlite", "founding-reopen");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    {
        let platform = support::open_store(config.clone()).expect("open sqlite store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        let report = runtime
            .provision_subject_soul(founding_intent("store-sqlite-founding"))
            .expect("commit sqlite founding Soul");
        assert_eq!(report.outcome, SubjectSoulMutationOutcomeV1::Committed);
        assert_current_founding(&runtime);
    }
    let reopened = support::open_store(config).expect("reopen sqlite Store v10 closure");
    let runtime = support::runtime_for_scope(&reopened, "space:test", 101);
    assert_current_founding(&runtime);
    std::fs::remove_file(path).expect("remove sqlite fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_reset_tombstone_and_no_terminated_raw_body() {
    let path = temp_root("sqlite", "reset-reopen");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    let selector;
    {
        let platform = support::open_store(config.clone()).expect("open sqlite store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        runtime
            .provision_subject_soul(founding_intent("store-sqlite-reset-founding"))
            .expect("commit sqlite founding Soul");
        selector = reset_founding_and_assert_raw_purge(&platform, &runtime, "store-sqlite-reset");
    }
    let reopened = support::open_store(config).expect("reopen reset sqlite closure");
    let runtime = support::runtime_for_scope(&reopened, "space:test", 101);
    let SubjectSoulReadSelectorV1::Exact {
        generation,
        revision,
        ref material_digest,
    } = selector
    else {
        unreachable!("reset helper returns exact selector")
    };
    assert_terminated_exact(
        &runtime,
        selector.clone(),
        generation,
        revision,
        material_digest,
        SubjectSoulTerminalActionV1::Reset,
    );
    let reopened_snapshot = reopened
        .export_store_snapshot()
        .expect("snapshot reopened reset closure");
    assert!(reopened_snapshot.json_docs.iter().all(|document| !matches!(
        document.namespace.as_str(),
        "subject_soul_revision_materials" | "self_authored_core" | "core_revision_ledger"
    )));
    std::fs::remove_file(path).expect("remove sqlite fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_closes_archive_restore_reseed_and_terminal_delete_lifecycle() {
    let path = temp_root("sqlite", "full-lifecycle-reopen");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    run_archive_restore_reseed_delete_reopen(config);
    std::fs::remove_file(path).expect("remove sqlite fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_active_relationship_double_root_and_stale_zero_change() {
    let path = temp_root("sqlite", "active-double-root-reopen");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    run_active_double_root_reopen_and_stale_zero_change(config);
    std::fs::remove_file(path).expect("remove sqlite fixture");
}

#[test]
fn file_reopen_rejects_orphan_subject_soul_address_before_open_mutation() {
    let root = temp_root("file", "orphan-open");
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    drop(support::open_store(config.clone()).expect("initialize file store"));

    let (engine, _, _) = support::open_file_engine(&config).expect("open raw file engine");
    engine
        .put_json_value(
            "self_model",
            "space:test/subject:agent-a/soul:soul-a/generation:1/self-model",
            orphan_subject_soul_value(),
        )
        .expect("inject orphan through nonproduction raw engine");
    drop(engine);

    let error = match support::open_store(config) {
        Ok(_) => panic!("orphan subject Soul address must fail file reopen"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "file_store_open_preflight");
    assert!(!error.to_string().is_empty());
    std::fs::remove_dir_all(root).expect("remove file fixture");
}

#[test]
fn file_reopen_rejects_partial_subject_soul_roots_before_open_mutation() {
    let root = temp_root("file", "partial-roots-open");
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    {
        let platform = support::open_store(config.clone()).expect("open file store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        runtime
            .provision_subject_soul(founding_intent("store-file-partial-roots"))
            .expect("commit founding Soul before corruption");
    }

    let (engine, _, _) = support::open_file_engine(&config).expect("open raw file engine");
    let manifest_keys = engine
        .list_json_keys("subject_soul_scope_manifests")
        .expect("list manifest keys");
    assert_eq!(manifest_keys.len(), 1);
    assert!(
        engine
            .delete_json_value("subject_soul_scope_manifests", &manifest_keys[0])
            .expect("delete manifest root"),
        "fixture must remove the manifest half of the root pair"
    );
    drop(engine);

    let error = match support::open_store(config) {
        Ok(_) => panic!("partial Subject Soul root pair must fail file reopen"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "file_store_open_preflight");
    assert!(!error.to_string().is_empty());
    std::fs::remove_dir_all(root).expect("remove file fixture");
}

#[test]
fn file_reopen_rejects_subject_soul_digest_corruption_before_open_mutation() {
    let root = temp_root("file", "digest-corruption-open");
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    {
        let platform = support::open_store(config.clone()).expect("open file store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        runtime
            .provision_subject_soul(founding_intent("store-file-digest-corruption"))
            .expect("commit founding Soul before corruption");
    }

    let (engine, _, _) = support::open_file_engine(&config).expect("open raw file engine");
    let manifest_keys = engine
        .list_json_keys("subject_soul_scope_manifests")
        .expect("list manifest keys");
    assert_eq!(manifest_keys.len(), 1);
    let mut manifest = engine
        .get_json_value("subject_soul_scope_manifests", &manifest_keys[0])
        .expect("read manifest root")
        .expect("manifest root exists");
    manifest["closure_digest"] = json!("0".repeat(64));
    engine
        .put_json_value("subject_soul_scope_manifests", &manifest_keys[0], manifest)
        .expect("inject digest-corrupt manifest through raw engine");
    drop(engine);

    let error = match support::open_store(config) {
        Ok(_) => panic!("digest-corrupt Subject Soul closure must fail file reopen"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "file_store_open_preflight");
    assert!(!error.to_string().is_empty());
    std::fs::remove_dir_all(root).expect("remove file fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_rejects_orphan_subject_soul_address_before_open_mutation() {
    let root = temp_root("sqlite", "orphan-open");
    let config = StoreBackendConfig::sqlite(&root, support::native_persistent_profile())
        .expect("sqlite config");
    drop(support::open_store(config.clone()).expect("initialize sqlite store"));

    let (engine, _) = support::open_sqlite_engine(&config).expect("open raw sqlite engine");
    engine
        .put_json_value(
            "self_model",
            "space:test/subject:agent-a/soul:soul-a/generation:1/self-model",
            orphan_subject_soul_value(),
        )
        .expect("inject orphan through nonproduction raw engine");
    drop(engine);

    let error = match support::open_store(config) {
        Ok(_) => panic!("orphan subject Soul address must fail sqlite reopen"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert!(!error.to_string().is_empty());
    std::fs::remove_file(root).expect("remove sqlite fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_rejects_partial_subject_soul_roots_before_open_mutation() {
    let path = temp_root("sqlite", "partial-roots-open");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    {
        let platform = support::open_store(config.clone()).expect("open sqlite store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        runtime
            .provision_subject_soul(founding_intent("store-sqlite-partial-roots"))
            .expect("commit founding Soul before corruption");
    }

    let (engine, _) = support::open_sqlite_engine(&config).expect("open raw sqlite engine");
    let manifest_keys = engine
        .list_json_keys("subject_soul_scope_manifests")
        .expect("list manifest keys");
    assert_eq!(manifest_keys.len(), 1);
    assert!(
        engine
            .delete_json_value("subject_soul_scope_manifests", &manifest_keys[0])
            .expect("delete manifest root"),
        "fixture must remove the manifest half of the root pair"
    );
    drop(engine);

    let error = match support::open_store(config) {
        Ok(_) => panic!("partial Subject Soul root pair must fail sqlite reopen"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert!(!error.to_string().is_empty());
    std::fs::remove_file(path).expect("remove sqlite fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_rejects_subject_soul_digest_corruption_before_open_mutation() {
    let path = temp_root("sqlite", "digest-corruption-open");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    {
        let platform = support::open_store(config.clone()).expect("open sqlite store");
        let runtime = support::runtime_for_scope(&platform, "space:test", 100);
        runtime
            .provision_subject_soul(founding_intent("store-sqlite-digest-corruption"))
            .expect("commit founding Soul before corruption");
    }

    let (engine, _) = support::open_sqlite_engine(&config).expect("open raw sqlite engine");
    let manifest_keys = engine
        .list_json_keys("subject_soul_scope_manifests")
        .expect("list manifest keys");
    assert_eq!(manifest_keys.len(), 1);
    let mut manifest = engine
        .get_json_value("subject_soul_scope_manifests", &manifest_keys[0])
        .expect("read manifest root")
        .expect("manifest root exists");
    manifest["closure_digest"] = json!("0".repeat(64));
    engine
        .put_json_value("subject_soul_scope_manifests", &manifest_keys[0], manifest)
        .expect("inject digest-corrupt manifest through raw engine");
    drop(engine);

    let error = match support::open_store(config) {
        Ok(_) => panic!("digest-corrupt Subject Soul closure must fail sqlite reopen"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert!(!error.to_string().is_empty());
    std::fs::remove_file(path).expect("remove sqlite fixture");
}
