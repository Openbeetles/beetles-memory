mod support;

use bm_sdk::{
    compile_subject_soul_founding_core, default_agent_subject_id, default_memory_space_id,
    primary_human_subject_id, render_subject_soul_constitutional_block, system_governor_subject_id,
    HumanSoulLifecycleConfirmationV1, MemoryIdentity, MemoryMutationCapabilityCatalog,
    MemoryMutationReliability, MemoryMutationSurface, MemoryProjectionRequest,
    MemoryRecallTemporalOperation, MemoryRuntime, MemoryScope, PressureLevel,
    RelationshipAccessConstraintV1, RelationshipConstraintLatticeV1,
    RelationshipDisclosureCeilingV1, RelationshipSourceClausesV1,
    RelationshipSourceControlAuthorityV1, RelationshipSourceControlErrorKeyV1,
    RelationshipSourceControlIntentActionV1, RelationshipSourceControlIntentV1,
    RelationshipSourceControlOutcomeV1, RelationshipSourceExpectedStateV1,
    RelationshipSourceReadRequestV1, RelationshipSourceReadSelectorV1, RelationshipSourceStateV1,
    RuntimeLifecycleModeInput, SoulGovernanceSdkErrorDisposition, StoreBackendConfig,
    SubjectDescriptor, SubjectRegistry, SubjectRelationshipGraph, SubjectRelationshipKind,
    SubjectScopedRuntime, SubjectSoulConstitutionalViewV1, SubjectSoulExpectedStateV1,
    SubjectSoulFoundingCharterSeedV1, SubjectSoulGovernedDisclosureDispositionV1,
    SubjectSoulGovernedDisclosureRequestV1, SubjectSoulLifecycleActionV1,
    SubjectSoulLifecycleAuthorityV1, SubjectSoulLifecycleErrorKey,
    SubjectSoulLifecycleMutationRequestV1, SubjectSoulLifecycleStateV1,
    SubjectSoulMutationOutcomeV1, SubjectSoulOperatorSafeExportV1, SubjectSoulProvisionIntentV1,
    SubjectSoulReadOutcomeV1, SubjectSoulReadRequestV1, SubjectSoulReadSelectorV1,
    SubjectSoulReadViewV1, SubjectSoulRevisionOriginV1, SubjectSoulRevisionProvenanceV1,
    SubjectSoulSourceAuthorityV1,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn public_mutation_inventory_lists_every_durable_subject_soul_operation_family() {
    let catalog = MemoryMutationCapabilityCatalog::current();
    let durable_surfaces = [
        MemoryMutationSurface::SubjectSoulEvidence,
        MemoryMutationSurface::SubjectSoulProvision,
        MemoryMutationSurface::SubjectSoulRevision,
        MemoryMutationSurface::SubjectSoulArchive,
        MemoryMutationSurface::SubjectSoulRestore,
        MemoryMutationSurface::SubjectSoulReset,
        MemoryMutationSurface::SubjectSoulReseed,
        MemoryMutationSurface::SubjectSoulDelete,
        MemoryMutationSurface::RelationshipSourceControl,
    ];

    for surface in durable_surfaces {
        assert!(
            catalog.operations.iter().any(|item| {
                item.surface == surface
                    && item.reliability == MemoryMutationReliability::DurableStoreReceipt
            }),
            "missing durable mutation capability for {surface:?}"
        );
    }
}

fn temp_root(backend: &str) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "beetle-memory-spv1-sdk-{backend}-{}-{sequence}",
        std::process::id()
    ))
}

fn single_agent_runtime(owner: &str, agent: &str) -> MemoryRuntime {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("store config"),
    )
    .expect("store");
    single_agent_runtime_with_store(owner, agent, store)
}

fn single_agent_runtime_with_store(
    owner: &str,
    agent: &str,
    store: bm_sdk::MemoryStoreHandle,
) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent, owner).expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(store)
        .build()
        .expect("runtime")
}

fn exact_soul_state(outcome: SubjectSoulReadOutcomeV1) -> SubjectSoulExpectedStateV1 {
    let SubjectSoulReadOutcomeV1::Verified { view } = outcome else {
        panic!("expected verified Soul root")
    };
    SubjectSoulExpectedStateV1::Exact {
        generation: view.generation,
        revision: view.revision,
        lifecycle_state: view.state,
        head_digest: view.head_digest,
        manifest_digest: view.manifest_digest,
    }
}

fn exact_soul_selector(outcome: &SubjectSoulReadOutcomeV1) -> SubjectSoulReadSelectorV1 {
    let SubjectSoulReadOutcomeV1::Verified { view } = outcome else {
        panic!("expected verified Soul root")
    };
    SubjectSoulReadSelectorV1::Exact {
        generation: view.generation,
        revision: view.revision.expect("verified material revision"),
        material_digest: view
            .material_digest
            .clone()
            .expect("verified material digest"),
    }
}

fn current_operator_soul(runtime: &MemoryRuntime) -> SubjectSoulReadOutcomeV1 {
    runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current operator-safe Soul")
}

fn project_soul(
    runtime: &MemoryRuntime,
    temporal_operation: MemoryRecallTemporalOperation,
) -> String {
    runtime
        .project(MemoryProjectionRequest {
            temporal_operation,
            user_query: "How should you approach this?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("runtime projection")
        .provider_payload()
        .system_memory_block()
        .to_string()
}

fn projection_request(
    temporal_operation: MemoryRecallTemporalOperation,
) -> MemoryProjectionRequest {
    MemoryProjectionRequest {
        temporal_operation,
        user_query: "How should you approach this?".to_string(),
        system_max_len: 4096,
        recent_messages_limit: 4,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    }
}

fn relationship_runtime(owner: &str, agent: &str, relationship_id: &str) -> MemoryRuntime {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("store config"),
    )
    .expect("store");
    relationship_runtime_with_store(owner, agent, relationship_id, store)
}

fn relationship_runtime_with_store(
    owner: &str,
    agent: &str,
    relationship_id: &str,
    store: bm_sdk::MemoryStoreHandle,
) -> MemoryRuntime {
    let registry = SubjectRegistry::single_agent_default(owner, agent).expect("registry");
    let mounted = default_agent_subject_id(agent);
    let human = primary_human_subject_id(owner);
    let mut graph = SubjectRelationshipGraph::single_agent_default(&registry).expect("graph");
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
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent, owner).expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(store)
        .subject_registry(registry)
        .subject_relationship_graph(graph)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id(owner),
            mounted_subject_id: mounted.clone(),
            actor_subject_id: mounted,
            agent_id: agent.to_string(),
            relationship_scope: Some(bm_core::memory::RelationshipScope {
                relationship_id: relationship_id.to_string(),
                channel: "local".to_string(),
                conversation_id: Some("chat-1".to_string()),
            }),
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .build()
        .expect("runtime")
}

fn create_relationship_root(
    runtime: &MemoryRuntime,
    owner: &str,
    agent: &str,
    relationship_id: &str,
) -> bm_sdk::RelationshipSourceControlReportV1 {
    let mounted = default_agent_subject_id(agent);
    let human = primary_human_subject_id(owner);
    runtime
        .control_relationship_source(RelationshipSourceControlIntentV1 {
            operation_id: format!("{relationship_id}:create"),
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: mounted,
            counterparty_subject_ids: vec![human.clone()],
            expected_state: runtime
                .relationship_source_pristine_expected_state(relationship_id)
                .expect("pinned pristine proof"),
            authority: RelationshipSourceControlAuthorityV1::HumanUser {
                actor_subject_id: human,
            },
            action: RelationshipSourceControlIntentActionV1::Create {
                clauses: relationship_clauses(None),
                source_asserted_at: Some(1_700_000_000),
                evidence_digest: "c".repeat(64),
            },
        })
        .expect("create relationship root")
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

fn founding_seed() -> SubjectSoulFoundingCharterSeedV1 {
    SubjectSoulFoundingCharterSeedV1 {
        identity_anchor: Some("A careful independent collaborator".to_string()),
        character_tendencies: vec!["curious before certain".to_string()],
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
}

#[test]
fn public_founding_projection_preserves_origin_and_character_tendencies() {
    let seed = founding_seed()
        .canonicalize()
        .expect("canonical founding seed");
    let core = compile_subject_soul_founding_core(&seed, 1_800_000_000)
        .expect("compile founding Core post-image");
    let block = render_subject_soul_constitutional_block(
        &SubjectSoulConstitutionalViewV1 {
            core,
            provenance: SubjectSoulRevisionProvenanceV1 {
                origin: SubjectSoulRevisionOriginV1::HumanFoundingCharter,
                source_authority: SubjectSoulSourceAuthorityV1::ActiveHumanUser,
                source_subject_id: "subject:human:owner".to_string(),
                source_asserted_at: None,
                recorded_at: 1_800_000_000,
                operation_ref: Some("operation:founding-1".to_string()),
                proposal_ref: None,
                source_refs: Vec::new(),
            },
            material_digest: "a".repeat(64),
        },
        4_096,
    )
    .expect("render verified constitutional view")
    .expect("non-empty founding projection");

    assert!(block.contains("## Human-Sourced Founding Constitution"));
    assert!(!block.contains("## Self-Authored Core"));
    let tendencies = block
        .find("Character tendencies: curious before certain")
        .unwrap_or_else(|| panic!("character tendencies must be rendered: {block}"));
    let priorities = block
        .find("Priority constitution: truth before fluency")
        .unwrap_or_else(|| panic!("priority constitution must be rendered: {block}"));
    assert!(tendencies < priorities, "{block}");
    assert!(!block.contains("Default task scope:"), "{block}");
}

#[test]
fn public_relationship_clamp_is_deny_biased() {
    let clamp = RelationshipConstraintLatticeV1 {
        mental_privacy: RelationshipDisclosureCeilingV1::GovernedSummary,
        relationship_source: RelationshipDisclosureCeilingV1::FullGovernedDisclosure,
        soul_self_boundary: RelationshipDisclosureCeilingV1::RefusalOnly,
    };
    assert_eq!(
        clamp.effective_disclosure_ceiling(),
        RelationshipDisclosureCeilingV1::RefusalOnly
    );
}

#[test]
fn operator_safe_export_contains_metadata_but_no_soul_body() {
    let report = SubjectSoulOperatorSafeExportV1 {
        subject_id: "subject:agent:main".to_string(),
        soul_id: "soul:subject.agent.main".to_string(),
        state: SubjectSoulLifecycleStateV1::Active,
        generation: 1,
        revision: Some(1),
        material_digest: Some("b".repeat(64)),
        origin: Some(SubjectSoulRevisionOriginV1::HumanFoundingCharter),
        terminated_generations: Vec::new(),
    };
    let encoded = serde_json::to_string(&report).expect("serialize operator-safe export");
    for forbidden in [
        "identity_anchor",
        "character_tendencies",
        "non_negotiables",
        "runtime_private_core",
        "private_garden",
        "raw",
    ] {
        assert!(!encoded.contains(forbidden), "{encoded}");
    }
}

#[test]
fn pristine_single_agent_soul_reads_as_implicit_unseeded_without_a_mutation() {
    let runtime = single_agent_runtime("owner-unseeded", "agent-main");
    let target_subject_id = runtime.scoped_runtime().mounted_subject_id.clone();
    let outcome = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: target_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("legal pristine Soul read");
    let SubjectSoulReadOutcomeV1::ImplicitUnseeded {
        memory_space_id,
        subject_id,
        soul_id,
        generation,
        closure_certificate_digest,
    } = outcome
    else {
        panic!("pristine subject must remain implicit unseeded")
    };
    assert_eq!(memory_space_id, runtime.memory_space_id());
    assert_eq!(subject_id, target_subject_id);
    assert_eq!(generation, 1);
    assert!(!soul_id.is_empty());
    assert_eq!(closure_certificate_digest.len(), 64);

    let exported = runtime
        .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
        .expect("operator-safe unseeded export");
    assert_eq!(exported.state, SubjectSoulLifecycleStateV1::Unseeded);
    assert_eq!(exported.revision, None);
    assert_eq!(exported.material_digest, None);
    assert_eq!(exported.origin, None);
}

#[test]
fn public_provision_keeps_none_unseeded_and_commits_founding_revision_one_atomically() {
    let runtime = single_agent_runtime("owner-provision", "agent-main");
    let no_seed = runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Unseeded)
        .expect("unseeded is a legal no-effect");
    assert_eq!(
        no_seed.outcome,
        SubjectSoulMutationOutcomeV1::UnseededNoEffect
    );
    assert!(no_seed.transaction_id.is_none());

    let founding_intent = SubjectSoulProvisionIntentV1::Founding {
        operation_id: "soul-provision-1".to_string(),
        human_actor_subject_id: primary_human_subject_id("owner-provision"),
        charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
        source_asserted_at: Some(1_700_000_000),
    };
    let committed = runtime
        .provision_subject_soul(founding_intent.clone())
        .expect("founding revision one commits");
    assert_eq!(committed.outcome, SubjectSoulMutationOutcomeV1::Committed);
    assert_eq!(
        committed.state_before,
        SubjectSoulLifecycleStateV1::Unseeded
    );
    assert_eq!(committed.state_after, SubjectSoulLifecycleStateV1::Active);
    assert_eq!(committed.generation, 1);
    assert_eq!(committed.revision, Some(1));
    assert!(committed.transaction_id.is_some());
    assert!(committed.durable_receipt_ref.is_some());
    assert!(committed.safe_event_ref.is_some());

    let replayed = runtime
        .provision_subject_soul(founding_intent)
        .expect("same typed operation replays before current-state admission");
    assert_eq!(replayed.outcome, SubjectSoulMutationOutcomeV1::Replayed);
    assert!(replayed.replayed);
    assert_eq!(replayed.generation, committed.generation);
    assert_eq!(replayed.revision, committed.revision);
    assert_eq!(replayed.head_digest, committed.head_digest);
    assert_eq!(replayed.transaction_id, committed.transaction_id);
    assert_eq!(replayed.durable_receipt_ref, committed.durable_receipt_ref);
    assert_eq!(replayed.safe_event_ref, committed.safe_event_ref);

    let current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("read committed Soul");
    let SubjectSoulReadOutcomeV1::Verified { view } = current else {
        panic!("founding must replace implicit unseeded with a verified root")
    };
    assert_eq!(view.state, SubjectSoulLifecycleStateV1::Active);
    assert_eq!(view.generation, 1);
    assert_eq!(view.revision, Some(1));
    assert_eq!(
        view.origin,
        Some(SubjectSoulRevisionOriginV1::HumanFoundingCharter)
    );
    assert!(view.runtime_private_core.is_none());
    assert!(view.governed_disclosure.is_none());
}

#[test]
fn public_lifecycle_archives_restores_and_resets_with_typed_authority_and_replay() {
    let owner = "owner-lifecycle";
    let runtime = single_agent_runtime(owner, "agent-main");
    let target = runtime.scoped_runtime().mounted_subject_id.clone();
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "lifecycle-provision".to_string(),
            human_actor_subject_id: primary_human_subject_id(owner),
            charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("provision active Soul");

    let archive_expected = exact_soul_state(current_operator_soul(&runtime));
    let archived = runtime
        .archive_subject_soul_self_governed("lifecycle-archive", archive_expected.clone())
        .expect("archive through SDK-owned self-governance authority");
    assert_eq!(archived.state_after, SubjectSoulLifecycleStateV1::Archived);
    let replayed = runtime
        .archive_subject_soul_self_governed("lifecycle-archive", archive_expected)
        .expect("archive replay precedes current-state validation");
    assert_eq!(replayed.outcome, SubjectSoulMutationOutcomeV1::Replayed);
    assert_eq!(replayed.transaction_id, archived.transaction_id);

    let restored = runtime
        .restore_subject_soul_self_governed(
            "lifecycle-restore",
            exact_soul_state(current_operator_soul(&runtime)),
        )
        .expect("restore through SDK-owned self-governance authority");
    assert_eq!(restored.state_after, SubjectSoulLifecycleStateV1::Active);

    let expected = exact_soul_state(current_operator_soul(&runtime));
    let SubjectSoulExpectedStateV1::Exact { generation, .. } = expected else {
        unreachable!("helper returns exact")
    };
    let reset = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "lifecycle-reset".to_string(),
        target_subject_id: target.clone(),
        expected_state: expected,
        authority: SubjectSoulLifecycleAuthorityV1::Destructive {
            system_actor_subject_id: system_governor_subject_id(owner),
            human_confirmation: HumanSoulLifecycleConfirmationV1 {
                human_subject_id: primary_human_subject_id(owner),
                target_subject_id: target,
                expected_generation: generation,
                action: bm_sdk::SubjectSoulTerminalActionV1::Reset,
                reason_code: "explicit_reset".to_string(),
                confirmed_at: 1_700_000_100,
                evidence_digest: "d".repeat(64),
            },
        },
        action: SubjectSoulLifecycleActionV1::Reset {
            reason_code: "explicit_reset".to_string(),
        },
    };
    let reset_report = runtime
        .mutate_subject_soul(reset.clone())
        .expect("system governor plus active HumanUser reset atomically");
    assert_eq!(
        reset_report.state_after,
        SubjectSoulLifecycleStateV1::Unseeded
    );
    assert_eq!(reset_report.generation, generation + 1);
    assert_eq!(reset_report.revision, None);
    assert_eq!(
        runtime
            .mutate_subject_soul(reset)
            .expect("destructive replay survives generation change")
            .outcome,
        SubjectSoulMutationOutcomeV1::Replayed
    );
    let SubjectSoulReadOutcomeV1::Verified { view } = current_operator_soul(&runtime) else {
        panic!("reset creates explicit unseeded lifecycle root")
    };
    assert_eq!(view.state, SubjectSoulLifecycleStateV1::Unseeded);
    assert_eq!(view.revision, None);
    assert_eq!(view.material_digest, None);
    assert_eq!(view.origin, None);
}

#[test]
fn destructive_reseed_and_delete_purge_raw_generations_and_leave_safe_terminated_exact_reads() {
    let owner = "owner-destructive";
    let runtime = single_agent_runtime(owner, "agent-main");
    let target = runtime.scoped_runtime().mounted_subject_id.clone();
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "destructive-provision".to_string(),
            human_actor_subject_id: primary_human_subject_id(owner),
            charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("provision founding Soul");
    let founding = current_operator_soul(&runtime);
    let founding_selector = exact_soul_selector(&founding);
    let expected = exact_soul_state(founding);
    let SubjectSoulExpectedStateV1::Exact { generation, .. } = expected else {
        unreachable!("exact expected")
    };
    runtime
        .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
            operation_id: "destructive-reset".to_string(),
            target_subject_id: target.clone(),
            expected_state: expected,
            authority: SubjectSoulLifecycleAuthorityV1::Destructive {
                system_actor_subject_id: system_governor_subject_id(owner),
                human_confirmation: HumanSoulLifecycleConfirmationV1 {
                    human_subject_id: primary_human_subject_id(owner),
                    target_subject_id: target.clone(),
                    expected_generation: generation,
                    action: bm_sdk::SubjectSoulTerminalActionV1::Reset,
                    reason_code: "replace_generation".to_string(),
                    confirmed_at: 1_700_000_100,
                    evidence_digest: "e".repeat(64),
                },
            },
            action: SubjectSoulLifecycleActionV1::Reset {
                reason_code: "replace_generation".to_string(),
            },
        })
        .expect("reset purges generation one raw material");
    let terminated = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: target.clone(),
            selector: founding_selector.clone(),
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("terminated exact selector returns safe metadata");
    assert!(matches!(
        terminated,
        SubjectSoulReadOutcomeV1::TerminatedGeneration { .. }
    ));
    let terminated_projection = runtime
        .project_with_subject_soul_selector(
            projection_request(MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_700_000_200,
            }),
            Some(founding_selector),
        )
        .expect("terminated exact projection is safe no-material")
        .provider_payload()
        .system_memory_block()
        .to_string();
    assert!(!terminated_projection.contains("curious before certain"));

    let reset_state = current_operator_soul(&runtime);
    let expected = exact_soul_state(reset_state);
    let SubjectSoulExpectedStateV1::Exact { generation, .. } = expected else {
        unreachable!("exact expected")
    };
    let mut reseed = founding_seed();
    reseed.character_tendencies = vec!["deliberate after reset".to_string()];
    runtime
        .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
            operation_id: "destructive-reseed".to_string(),
            target_subject_id: target.clone(),
            expected_state: expected,
            authority: SubjectSoulLifecycleAuthorityV1::Destructive {
                system_actor_subject_id: system_governor_subject_id(owner),
                human_confirmation: HumanSoulLifecycleConfirmationV1 {
                    human_subject_id: primary_human_subject_id(owner),
                    target_subject_id: target.clone(),
                    expected_generation: generation,
                    action: bm_sdk::SubjectSoulTerminalActionV1::Reseed,
                    reason_code: "confirmed_reseed".to_string(),
                    confirmed_at: 1_700_000_300,
                    evidence_digest: "f".repeat(64),
                },
            },
            action: SubjectSoulLifecycleActionV1::Reseed {
                charter: Box::new(reseed.canonicalize().expect("canonical reseed")),
                reason_code: "confirmed_reseed".to_string(),
                source_asserted_at: Some(1_700_000_250),
            },
        })
        .expect("reseed creates one new generation revision one");
    let current_projection = project_soul(&runtime, MemoryRecallTemporalOperation::Current);
    assert!(current_projection.contains("deliberate after reset"));
    assert!(!current_projection.contains("curious before certain"));

    let reseeded = current_operator_soul(&runtime);
    let reseeded_selector = exact_soul_selector(&reseeded);
    let expected = exact_soul_state(reseeded);
    let SubjectSoulExpectedStateV1::Exact { generation, .. } = expected else {
        unreachable!("exact expected")
    };
    runtime
        .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
            operation_id: "destructive-delete".to_string(),
            target_subject_id: target.clone(),
            expected_state: expected,
            authority: SubjectSoulLifecycleAuthorityV1::Destructive {
                system_actor_subject_id: system_governor_subject_id(owner),
                human_confirmation: HumanSoulLifecycleConfirmationV1 {
                    human_subject_id: primary_human_subject_id(owner),
                    target_subject_id: target.clone(),
                    expected_generation: generation,
                    action: bm_sdk::SubjectSoulTerminalActionV1::Delete,
                    reason_code: "confirmed_delete".to_string(),
                    confirmed_at: 1_700_000_400,
                    evidence_digest: "1".repeat(64),
                },
            },
            action: SubjectSoulLifecycleActionV1::Delete {
                reason_code: "confirmed_delete".to_string(),
            },
        })
        .expect("delete purges current generation raw material");
    assert!(
        !project_soul(&runtime, MemoryRecallTemporalOperation::Current)
            .contains("deliberate after reset")
    );
    assert!(matches!(
        runtime
            .read_subject_soul(SubjectSoulReadRequestV1 {
                target_subject_id: target,
                selector: reseeded_selector,
                view: SubjectSoulReadViewV1::OperatorSafe,
            })
            .expect("deleted exact safe metadata"),
        SubjectSoulReadOutcomeV1::TerminatedGeneration { .. }
    ));
}

#[test]
fn runtime_project_consumes_verified_current_soul_and_never_applies_it_to_historical_as_of() {
    let runtime = single_agent_runtime("owner-project", "agent-main");
    assert!(
        !project_soul(&runtime, MemoryRecallTemporalOperation::Current)
            .contains("curious before certain")
    );
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "project-provision".to_string(),
            human_actor_subject_id: primary_human_subject_id("owner-project"),
            charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("provision founding Soul");

    let current = project_soul(&runtime, MemoryRecallTemporalOperation::Current);
    assert!(
        current.contains("curious before certain"),
        "first governed projection must consume verified founding revision: {current}"
    );
    let SubjectSoulReadOutcomeV1::Verified { view } = current_operator_soul(&runtime) else {
        panic!("founding Soul must be verified")
    };
    let exact = runtime
        .project_with_subject_soul_selector(
            projection_request(MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_700_000_100,
            }),
            Some(SubjectSoulReadSelectorV1::Exact {
                generation: view.generation,
                revision: view.revision.expect("founding revision"),
                material_digest: view.material_digest.expect("founding digest"),
            }),
        )
        .expect("exact historical Soul projection")
        .provider_payload()
        .system_memory_block()
        .to_string();
    assert!(
        exact.contains("curious before certain"),
        "exact selector must positively project its verified revision: {exact}"
    );
    let historical = project_soul(
        &runtime,
        MemoryRecallTemporalOperation::HistoricalAsOf {
            as_of_time: 1_699_999_999,
        },
    );
    assert!(
        !historical.contains("curious before certain"),
        "historical projection must not apply the current Soul revision: {historical}"
    );
}

#[test]
fn active_soul_relationship_source_commits_and_projects_through_one_verified_double_root() {
    let owner = "owner-double-root";
    let agent = "agent-main";
    let relationship_id = "relationship:double-root";
    let runtime = relationship_runtime(owner, agent, relationship_id);
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "double-root-provision".to_string(),
            human_actor_subject_id: primary_human_subject_id(owner),
            charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("provision active Soul");

    let relationship = create_relationship_root(&runtime, owner, agent, relationship_id);
    assert_eq!(
        relationship.outcome,
        RelationshipSourceControlOutcomeV1::Committed
    );
    let projection = project_soul(&runtime, MemoryRecallTemporalOperation::Current);
    assert!(
        projection.contains("repair before escalation")
            && projection.matches("repair before escalation").count() == 1
            && projection.contains("- Relationship: maintain/aligned")
            && projection.contains("Boundary mode: closed")
            && projection.contains("relationship_constitution"),
        "verified relationship projection must be consumed by runtime: {projection}"
    );
    assert!(
        projection.contains("curious before certain"),
        "relationship projection must not replace the Soul root: {projection}"
    );
    let governed = runtime
        .disclose_subject_soul_governed(SubjectSoulGovernedDisclosureRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            relationship_id: relationship_id.to_string(),
            selector: SubjectSoulReadSelectorV1::Current,
        })
        .expect("deterministic governed disclosure");
    assert_eq!(
        governed.disposition,
        SubjectSoulGovernedDisclosureDispositionV1::GovernedSummary
    );
    let governed_text = governed.governed_text.expect("governed summary text");
    assert!(governed_text.contains("Governed Soul summary"));
    assert!(governed_text.contains("character-tendency commitments"));
    assert!(!governed_text.contains("repair before escalation"));
    assert!(!governed_text.contains("curious before certain"));
    assert!(!governed_text.contains("A careful independent collaborator"));

    let current_before_archive = current_operator_soul(&runtime);
    let founding_selector = exact_soul_selector(&current_before_archive);
    let archived = runtime
        .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
            operation_id: "double-root-system-archive".to_string(),
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            expected_state: exact_soul_state(current_before_archive),
            authority: SubjectSoulLifecycleAuthorityV1::Maintenance {
                system_actor_subject_id: system_governor_subject_id(owner),
            },
            action: SubjectSoulLifecycleActionV1::Archive,
        })
        .expect("governing SystemGovernor archives the active Soul");
    assert_eq!(archived.state_after, SubjectSoulLifecycleStateV1::Archived);
    let archived_projection = project_soul(&runtime, MemoryRecallTemporalOperation::Current);
    assert!(
        !archived_projection.contains("repair before escalation")
            && !archived_projection.contains("curious before certain")
            && !archived_projection.contains(&"a".repeat(64)),
        "archived Soul must project neither relationship source, Soul body, nor evidence: {archived_projection}"
    );
    let archived_exact_projection = runtime
        .project_with_subject_soul_selector(
            projection_request(MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_700_000_200,
            }),
            Some(founding_selector),
        )
        .expect("archived current root safely suppresses exact historical runtime projection")
        .provider_payload()
        .system_memory_block()
        .to_string();
    assert!(
        !archived_exact_projection.contains("repair before escalation")
            && !archived_exact_projection.contains("curious before certain")
            && !archived_exact_projection.contains(&"a".repeat(64)),
        "archived current lifecycle floor must suppress exact historical Soul projection: {archived_exact_projection}"
    );

    runtime
        .restore_subject_soul_self_governed(
            "double-root-self-restore",
            exact_soul_state(current_operator_soul(&runtime)),
        )
        .expect("restore the archived Soul through the sealed self-governance seam");
    let restored_projection = project_soul(&runtime, MemoryRecallTemporalOperation::Current);
    assert!(
        restored_projection.contains("repair before escalation")
            && restored_projection
                .matches("repair before escalation")
                .count()
                == 1
            && restored_projection.contains("curious before certain"),
        "restore must recompile the independent active relationship source without reviving a stored projection: {restored_projection}"
    );

    let refused = runtime
        .disclose_subject_soul_governed(SubjectSoulGovernedDisclosureRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            relationship_id: "relationship:not-mounted".to_string(),
            selector: SubjectSoulReadSelectorV1::Current,
        })
        .expect("missing governed root refuses without raw content");
    assert_eq!(
        refused.disposition,
        SubjectSoulGovernedDisclosureDispositionV1::Refused
    );
    assert!(refused.relationship_source_digest.is_none());
}

#[test]
fn public_soul_read_rejects_unknown_unmounted_and_raw_private_targets_with_typed_errors() {
    let runtime = single_agent_runtime("owner-errors", "agent-main");
    let unknown = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: "subject:missing".to_string(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect_err("unknown subject must fail closed");
    assert_eq!(unknown.key, SubjectSoulLifecycleErrorKey::SubjectNotFound);
    assert_eq!(
        unknown.disposition,
        SoulGovernanceSdkErrorDisposition::RegistryRejected
    );

    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("store config"),
    )
    .expect("store");
    let mut registry =
        SubjectRegistry::single_agent_default("owner-cross", "agent-a").expect("registry");
    let unmounted = default_agent_subject_id("agent-b");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(&unmounted, "Agent B"))
        .expect("second agent");
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-a", "owner-cross").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(store)
        .subject_registry(registry)
        .build()
        .expect("runtime");
    let cross = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: unmounted,
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect_err("unmounted subject must fail closed");
    assert_eq!(cross.key, SubjectSoulLifecycleErrorKey::TargetNotMounted);

    let private = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.scoped_runtime().mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::RuntimePrivate,
        })
        .expect_err("raw private Soul must not be a public read");
    assert_eq!(private.key, SubjectSoulLifecycleErrorKey::AuthorityDenied);
    assert_eq!(
        private.disposition,
        SoulGovernanceSdkErrorDisposition::AuthorityRejected
    );
}

#[test]
fn public_soul_authority_rejects_foreign_human_wrong_capability_and_unknown_governor_zero_change() {
    let owner = "owner-authority";
    let runtime = single_agent_runtime(owner, "agent-main");
    let target = runtime.scoped_runtime().mounted_subject_id.clone();
    let foreign = runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "foreign-human-provision".to_string(),
            human_actor_subject_id: "user:foreign".to_string(),
            charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
            source_asserted_at: None,
        })
        .expect_err("foreign HumanUser must fail before planning");
    assert_eq!(foreign.key, SubjectSoulLifecycleErrorKey::SubjectNotFound);
    assert!(matches!(
        current_operator_soul(&runtime),
        SubjectSoulReadOutcomeV1::ImplicitUnseeded { .. }
    ));

    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "authority-provision".to_string(),
            human_actor_subject_id: primary_human_subject_id(owner),
            charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
            source_asserted_at: None,
        })
        .expect("valid provision");
    let before = current_operator_soul(&runtime);
    let wrong_capability = runtime
        .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
            operation_id: "wrong-capability".to_string(),
            target_subject_id: target.clone(),
            expected_state: exact_soul_state(before.clone()),
            authority: SubjectSoulLifecycleAuthorityV1::SelfGovernance {
                capability_digest: "a".repeat(64),
            },
            action: SubjectSoulLifecycleActionV1::Archive,
        })
        .expect_err("caller-chosen capability must fail closed");
    assert_eq!(
        wrong_capability.key,
        SubjectSoulLifecycleErrorKey::AuthorityDenied
    );
    assert_eq!(current_operator_soul(&runtime), before);

    let SubjectSoulReadOutcomeV1::Verified { view } = before.clone() else {
        unreachable!("provisioned")
    };
    let unknown_governor = runtime
        .mutate_subject_soul(SubjectSoulLifecycleMutationRequestV1 {
            operation_id: "unknown-governor".to_string(),
            target_subject_id: target.clone(),
            expected_state: exact_soul_state(before.clone()),
            authority: SubjectSoulLifecycleAuthorityV1::Destructive {
                system_actor_subject_id: "system:foreign".to_string(),
                human_confirmation: HumanSoulLifecycleConfirmationV1 {
                    human_subject_id: primary_human_subject_id(owner),
                    target_subject_id: target,
                    expected_generation: view.generation,
                    action: bm_sdk::SubjectSoulTerminalActionV1::Delete,
                    reason_code: "attempted_delete".to_string(),
                    confirmed_at: 1_700_000_500,
                    evidence_digest: "2".repeat(64),
                },
            },
            action: SubjectSoulLifecycleActionV1::Delete {
                reason_code: "attempted_delete".to_string(),
            },
        })
        .expect_err("unknown SystemGovernor must fail closed");
    assert_eq!(
        unknown_governor.key,
        SubjectSoulLifecycleErrorKey::AuthorityDenied
    );
    assert_eq!(current_operator_soul(&runtime), before);
}

#[test]
fn relationship_source_create_replay_read_and_successor_are_one_typed_owner_chain() {
    let owner = "owner-relationship";
    let agent = "agent-main";
    let relationship_id = "relationship:primary";
    let runtime = relationship_runtime(owner, agent, relationship_id);
    let mounted = default_agent_subject_id(agent);
    let human = primary_human_subject_id(owner);
    let expected = runtime
        .relationship_source_pristine_expected_state(relationship_id)
        .expect("pinned pristine proof");
    let create = RelationshipSourceControlIntentV1 {
        operation_id: "relationship-create-1".to_string(),
        memory_space_id: runtime.memory_space_id().to_string(),
        relationship_id: relationship_id.to_string(),
        mounted_subject_id: mounted.clone(),
        counterparty_subject_ids: vec![human.clone()],
        expected_state: expected,
        authority: RelationshipSourceControlAuthorityV1::HumanUser {
            actor_subject_id: human.clone(),
        },
        action: RelationshipSourceControlIntentActionV1::Create {
            clauses: relationship_clauses(None),
            source_asserted_at: Some(1_700_000_000),
            evidence_digest: "a".repeat(64),
        },
    };
    let committed = runtime
        .control_relationship_source(create.clone())
        .expect("commit relationship rev1");
    assert_eq!(
        committed.outcome,
        RelationshipSourceControlOutcomeV1::Committed
    );
    assert_eq!(committed.revision, 1);
    assert_eq!(committed.state, RelationshipSourceStateV1::Active);

    let replayed = runtime
        .control_relationship_source(create)
        .expect("replay exact operation");
    assert_eq!(
        replayed.outcome,
        RelationshipSourceControlOutcomeV1::Replayed
    );
    assert!(replayed.replayed);
    assert_eq!(replayed.revision, committed.revision);
    assert_eq!(replayed.source_digest, committed.source_digest);
    assert_eq!(replayed.manifest_digest, committed.manifest_digest);
    assert_eq!(replayed.safe_event_ref, committed.safe_event_ref);

    let current = runtime
        .read_relationship_source(RelationshipSourceReadRequestV1 {
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: mounted.clone(),
            selector: RelationshipSourceReadSelectorV1::Current,
        })
        .expect("read current relationship root");
    assert_eq!(current.selected_revision, 1);
    assert_eq!(current.current_revision, 1);
    assert_eq!(current.selected_source_digest, committed.source_digest);
    assert_eq!(current.current_source_digest, committed.source_digest);
    assert_eq!(current.current_manifest_digest, committed.manifest_digest);

    let update = RelationshipSourceControlIntentV1 {
        operation_id: "relationship-update-2".to_string(),
        memory_space_id: runtime.memory_space_id().to_string(),
        relationship_id: relationship_id.to_string(),
        mounted_subject_id: mounted.clone(),
        counterparty_subject_ids: vec![human.clone()],
        expected_state: RelationshipSourceExpectedStateV1::Exact {
            revision: current.current_revision,
            state: current.current_state.expect("current state"),
            source_digest: current.current_source_digest.clone(),
            manifest_digest: current.current_manifest_digest.clone(),
        },
        authority: RelationshipSourceControlAuthorityV1::HumanUser {
            actor_subject_id: human,
        },
        action: RelationshipSourceControlIntentActionV1::UpdateContribution {
            clauses: relationship_clauses(Some("return after repair")),
            source_asserted_at: Some(1_700_000_010),
            evidence_digest: "b".repeat(64),
        },
    };
    let successor = runtime
        .control_relationship_source(update)
        .expect("commit relationship rev2 with retained rev1 root");
    assert_eq!(successor.revision, 2);

    let stale = runtime
        .control_relationship_source(RelationshipSourceControlIntentV1 {
            operation_id: "relationship-stale-update".to_string(),
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: mounted.clone(),
            counterparty_subject_ids: vec![primary_human_subject_id(owner)],
            expected_state: RelationshipSourceExpectedStateV1::Exact {
                revision: current.current_revision,
                state: current.current_state.expect("current state"),
                source_digest: current.current_source_digest.clone(),
                manifest_digest: current.current_manifest_digest.clone(),
            },
            authority: RelationshipSourceControlAuthorityV1::HumanUser {
                actor_subject_id: primary_human_subject_id(owner),
            },
            action: RelationshipSourceControlIntentActionV1::UpdateContribution {
                clauses: relationship_clauses(Some("must not commit stale change")),
                source_asserted_at: Some(1_700_000_020),
                evidence_digest: "d".repeat(64),
            },
        })
        .expect_err("stale relationship CAS must fail closed");
    assert_eq!(
        stale.key,
        RelationshipSourceControlErrorKeyV1::RevisionConflict
    );
    assert_eq!(
        stale.disposition,
        SoulGovernanceSdkErrorDisposition::ExpectedStateConflict
    );

    let after_stale = runtime
        .read_relationship_source(RelationshipSourceReadRequestV1 {
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: mounted.clone(),
            selector: RelationshipSourceReadSelectorV1::Current,
        })
        .expect("current root after stale conflict");
    assert_eq!(after_stale.current_revision, 2);
    assert_eq!(after_stale.current_source_digest, successor.source_digest);
    assert_eq!(
        after_stale.current_manifest_digest,
        successor.manifest_digest
    );

    let historical = runtime
        .read_relationship_source(RelationshipSourceReadRequestV1 {
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: mounted,
            selector: RelationshipSourceReadSelectorV1::Exact {
                revision: 1,
                source_digest: committed.source_digest.clone(),
            },
        })
        .expect("exact revision one remains readable");
    assert_eq!(historical.selected_revision, 1);
    assert_eq!(historical.current_revision, 2);
    assert_eq!(historical.selected_source_digest, committed.source_digest);
    assert_eq!(historical.current_source_digest, successor.source_digest);
}

#[test]
fn production_projection_recompiles_active_relationship_source_without_stored_soul_projection() {
    let owner = "owner-relationship-runtime";
    let agent = "agent-main";
    let relationship_id = "relationship:runtime-missing-projection";
    let runtime = relationship_runtime(owner, agent, relationship_id);
    let no_source_projection = project_soul(&runtime, MemoryRecallTemporalOperation::Current);
    assert!(
        !no_source_projection.contains("repair before escalation"),
        "negative control must not synthesize relationship commitments without a source root"
    );
    create_relationship_root(&runtime, owner, agent, relationship_id);

    let source_only_projection = project_soul(&runtime, MemoryRecallTemporalOperation::Current);
    assert!(
        source_only_projection.contains("repair before escalation")
            && source_only_projection
                .matches("repair before escalation")
                .count()
                == 1
            && source_only_projection.contains("- Relationship: maintain/aligned")
            && source_only_projection.contains("Boundary mode: closed")
            && source_only_projection.contains("relationship_constitution"),
        "active source must clamp the implicit-unseeded runtime even without a stored Soul projection: {source_only_projection}"
    );

    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "relationship-runtime-founding".to_string(),
            human_actor_subject_id: primary_human_subject_id(owner),
            charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
            source_asserted_at: Some(1_700_000_100),
        })
        .expect("found Soul after the independent relationship source");
    let recompiled_projection = project_soul(&runtime, MemoryRecallTemporalOperation::Current);
    assert!(
        recompiled_projection.contains("repair before escalation")
            && recompiled_projection
                .matches("repair before escalation")
                .count()
                == 1
            && recompiled_projection.contains("- Relationship: maintain/aligned")
            && recompiled_projection.contains("Boundary mode: closed")
            && recompiled_projection.contains("relationship_constitution"),
        "active source must be recompiled against current Soul material when projection is missing: {recompiled_projection}"
    );
    assert!(
        !recompiled_projection.contains(&"a".repeat(64)),
        "relationship evidence digest must never enter the model projection"
    );
}

#[test]
fn relationship_source_file_reopen_preserves_verified_current_root() {
    let owner = "owner-file-relationship";
    let agent = "agent-main";
    let relationship_id = "relationship:file-primary";
    let root = temp_root("file");
    let config =
        StoreBackendConfig::file(&root, support::host_test_profile()).expect("file store config");
    let runtime = relationship_runtime_with_store(
        owner,
        agent,
        relationship_id,
        support::open_memory_store(config.clone()).expect("open file store"),
    );
    let committed = create_relationship_root(&runtime, owner, agent, relationship_id);
    drop(runtime);

    let reopened = relationship_runtime_with_store(
        owner,
        agent,
        relationship_id,
        support::open_memory_store(config).expect("reopen file store"),
    );
    let read = reopened
        .read_relationship_source(RelationshipSourceReadRequestV1 {
            memory_space_id: reopened.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: default_agent_subject_id(agent),
            selector: RelationshipSourceReadSelectorV1::Current,
        })
        .expect("verified relationship after file reopen");
    assert_eq!(read.selected_revision, 1);
    assert_eq!(read.selected_source_digest, committed.source_digest);
    assert_eq!(read.current_manifest_digest, committed.manifest_digest);
    drop(reopened);
    std::fs::remove_dir_all(root).expect("remove file test fixture");
}

#[test]
fn subject_soul_file_reopen_preserves_projection_and_durable_provision_replay() {
    let owner = "owner-file-soul";
    let agent = "agent-main";
    let root = temp_root("file-soul");
    let config =
        StoreBackendConfig::file(&root, support::host_test_profile()).expect("file store config");
    let intent = SubjectSoulProvisionIntentV1::Founding {
        operation_id: "file-soul-provision".to_string(),
        human_actor_subject_id: primary_human_subject_id(owner),
        charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
        source_asserted_at: Some(1_700_000_000),
    };
    let runtime = single_agent_runtime_with_store(
        owner,
        agent,
        support::open_memory_store(config.clone()).expect("open file store"),
    );
    let committed = runtime
        .provision_subject_soul(intent.clone())
        .expect("commit file Soul");
    drop(runtime);

    let reopened = single_agent_runtime_with_store(
        owner,
        agent,
        support::open_memory_store(config).expect("reopen file store"),
    );
    let replayed = reopened
        .provision_subject_soul(intent)
        .expect("reopen provision replay");
    assert_eq!(replayed.outcome, SubjectSoulMutationOutcomeV1::Replayed);
    assert_eq!(replayed.transaction_id, committed.transaction_id);
    assert!(
        project_soul(&reopened, MemoryRecallTemporalOperation::Current)
            .contains("curious before certain")
    );
    drop(reopened);
    std::fs::remove_dir_all(root).expect("remove file Soul fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn relationship_source_sqlite_reopen_preserves_verified_current_root() {
    let owner = "owner-sqlite-relationship";
    let agent = "agent-main";
    let relationship_id = "relationship:sqlite-primary";
    let path = temp_root("sqlite");
    let config = StoreBackendConfig::sqlite(&path, support::host_test_profile())
        .expect("sqlite store config");
    let runtime = relationship_runtime_with_store(
        owner,
        agent,
        relationship_id,
        support::open_memory_store(config.clone()).expect("open sqlite store"),
    );
    let committed = create_relationship_root(&runtime, owner, agent, relationship_id);
    drop(runtime);

    let reopened = relationship_runtime_with_store(
        owner,
        agent,
        relationship_id,
        support::open_memory_store(config).expect("reopen sqlite store"),
    );
    let read = reopened
        .read_relationship_source(RelationshipSourceReadRequestV1 {
            memory_space_id: reopened.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: default_agent_subject_id(agent),
            selector: RelationshipSourceReadSelectorV1::Current,
        })
        .expect("verified relationship after sqlite reopen");
    assert_eq!(read.selected_revision, 1);
    assert_eq!(read.selected_source_digest, committed.source_digest);
    assert_eq!(read.current_manifest_digest, committed.manifest_digest);
    drop(reopened);
    std::fs::remove_file(path).expect("remove sqlite test fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn subject_soul_sqlite_reopen_preserves_projection_and_durable_provision_replay() {
    let owner = "owner-sqlite-soul";
    let agent = "agent-main";
    let path = temp_root("sqlite-soul");
    let config = StoreBackendConfig::sqlite(&path, support::host_test_profile())
        .expect("sqlite store config");
    let intent = SubjectSoulProvisionIntentV1::Founding {
        operation_id: "sqlite-soul-provision".to_string(),
        human_actor_subject_id: primary_human_subject_id(owner),
        charter: Box::new(founding_seed().canonicalize().expect("canonical seed")),
        source_asserted_at: Some(1_700_000_000),
    };
    let runtime = single_agent_runtime_with_store(
        owner,
        agent,
        support::open_memory_store(config.clone()).expect("open sqlite store"),
    );
    let committed = runtime
        .provision_subject_soul(intent.clone())
        .expect("commit sqlite Soul");
    drop(runtime);

    let reopened = single_agent_runtime_with_store(
        owner,
        agent,
        support::open_memory_store(config).expect("reopen sqlite store"),
    );
    let replayed = reopened
        .provision_subject_soul(intent)
        .expect("reopen provision replay");
    assert_eq!(replayed.outcome, SubjectSoulMutationOutcomeV1::Replayed);
    assert_eq!(replayed.transaction_id, committed.transaction_id);
    assert!(
        project_soul(&reopened, MemoryRecallTemporalOperation::Current)
            .contains("curious before certain")
    );
    drop(reopened);
    std::fs::remove_file(path).expect("remove sqlite Soul fixture");
}
