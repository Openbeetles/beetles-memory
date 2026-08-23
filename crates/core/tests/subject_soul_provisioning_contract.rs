use bm_core::memory::{
    canonical_relationship_source_revision_ref_v1, compile_subject_soul_founding_core,
    compile_subject_soul_relationship_runtime_view_v1,
    compute_self_authored_core_expected_prior_v1, mental_privacy_safety_baseline,
    plan_relationship_source_control, plan_subject_soul_autonomous_cycle_v1,
    plan_subject_soul_generation_layer_delta_v1, plan_subject_soul_lifecycle_v1,
    plan_subject_soul_provision_v1, plan_subject_soul_relationship_projection_v1,
    plan_subject_soul_self_authored_revision_v1, relationship_mental_privacy_ceiling_v1,
    relationship_soul_self_boundary_ceiling_v1, render_subject_soul_constitutional_block,
    subject_soul_autonomous_cycle_intent_digest_v1, subject_soul_generation_layer_intent_digest_v1,
    subject_soul_lifecycle_intent_digest_v1, subject_soul_provision_intent_digest_v1,
    subject_soul_self_authored_revision_intent_digest_v1, CoreRevisionLedger, CoreRevisionOutcome,
    CoreRevisionRecord, HumanSoulLifecycleConfirmationV1, MemoryMutationOperationKind,
    RelationshipConstraintLatticeV1, RelationshipDisclosureCeilingV1,
    RelationshipSourceAuthorityKindV1, RelationshipSourceClausesV1,
    RelationshipSourceConstitutionV1, RelationshipSourceContributionV1,
    RelationshipSourceControlActionV1, RelationshipSourceControlAuthorityV1,
    RelationshipSourceControlIntentActionV1, RelationshipSourceControlIntentV1,
    RelationshipSourceExpectedStateV1, RelationshipSourceProvenanceV1,
    RelationshipSourceScopeManifestV1, RelationshipSourceStateV1, SelfAuthoredCore,
    SelfAuthoredCoreRefreshPlanV1, SubjectDescriptor, SubjectKind, SubjectRegistry,
    SubjectSoulAutonomousCycleIntentV1, SubjectSoulAutonomousCyclePlanV1,
    SubjectSoulAutonomousRevisionDeltaV1, SubjectSoulBinding, SubjectSoulConstitutionalViewV1,
    SubjectSoulExpectedStateV1, SubjectSoulFoundingCharterSeedV1,
    SubjectSoulGenerationLayerAuthorityV1, SubjectSoulGenerationLayerBasisV1,
    SubjectSoulGenerationLayerDeltaPlanV1, SubjectSoulGenerationLayerIntentV1,
    SubjectSoulGenerationLayerKindV1, SubjectSoulGenerationLayerMutationV1,
    SubjectSoulLifecycleActionV1, SubjectSoulLifecycleAuthorityV1,
    SubjectSoulLifecycleMutationRequestV1, SubjectSoulLifecycleStateV1,
    SubjectSoulManifestAddressV1, SubjectSoulMutationOutcomeV1, SubjectSoulMutationReportV1,
    SubjectSoulOperatorSafeExportV1, SubjectSoulOwnedDocumentV1, SubjectSoulOwnerV1,
    SubjectSoulProvisionIntentV1, SubjectSoulProvisionPlanV1, SubjectSoulReadOutcomeV1,
    SubjectSoulRelationshipProjectionPlanV1, SubjectSoulRelationshipProjectionV1,
    SubjectSoulRelationshipRuntimeInputV1, SubjectSoulRelationshipRuntimeProjectionDispositionV1,
    SubjectSoulRevisionAddressBindingsV1, SubjectSoulRevisionOriginV1,
    SubjectSoulRevisionProvenanceV1, SubjectSoulSelfAuthoredCommitPlanV1,
    SubjectSoulSelfAuthoredPostImageAddressesV1, SubjectSoulSelfAuthoredRevisionBasisV1,
    SubjectSoulSourceAuthorityV1, SubjectSoulTerminalActionV1, SubjectSoulTerminatedGenerationV1,
    SubjectSoulVerifiedSnapshotV1, SubjectVisibility,
};

#[test]
fn founding_character_tendencies_must_not_be_silently_discarded() {
    let core: SelfAuthoredCore = serde_json::from_value(serde_json::json!({
        "revision": 1,
        "character_tendencies": ["curious", "careful"]
    }))
    .expect("the v0.4 core material must admit typed character tendencies");

    let encoded = serde_json::to_value(core).expect("core remains serializable");
    assert_eq!(
        encoded.get("character_tendencies"),
        Some(&serde_json::json!(["curious", "careful"])),
        "typed founding material must survive the Core round trip"
    );
}

#[test]
fn founding_planner_closes_revision_one_core_ledger_material_manifest_and_head() {
    let owner = SubjectSoulOwnerV1 {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
    };
    let charter = SubjectSoulFoundingCharterSeedV1 {
        identity_anchor: Some("a curious autonomous subject".to_string()),
        character_tendencies: vec!["curious before certain".to_string()],
        ..SubjectSoulFoundingCharterSeedV1::default()
    }
    .canonicalize()
    .expect("canonical charter");
    let intent = SubjectSoulProvisionIntentV1::Founding {
        operation_id: "operation:founding".to_string(),
        human_actor_subject_id: "human:a".to_string(),
        charter: Box::new(charter),
        source_asserted_at: Some(7),
    };
    let expected = SubjectSoulExpectedStateV1::PristineAbsent {
        closure_certificate_digest:
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
    };
    let bindings = SubjectSoulRevisionAddressBindingsV1 {
        material: SubjectSoulManifestAddressV1 {
            namespace: "soul_material".to_string(),
            physical_key: "material:1".to_string(),
        },
        core: SubjectSoulManifestAddressV1 {
            namespace: "soul_core".to_string(),
            physical_key: "core:1".to_string(),
        },
        revision_ledger: SubjectSoulManifestAddressV1 {
            namespace: "soul_ledger".to_string(),
            physical_key: "ledger:1".to_string(),
        },
    };

    let plan = plan_subject_soul_provision_v1(&owner, &intent, &expected, Some(&bindings), 11)
        .expect("founding plan");
    let SubjectSoulProvisionPlanV1::Commit {
        head,
        manifest,
        material,
        core,
        revision_ledger,
        revision_ledger_digest,
        ..
    } = plan
    else {
        panic!("founding charter must create one complete commit plan");
    };
    assert_eq!(head.generation, 1);
    assert_eq!(head.current_revision, Some(1));
    assert_eq!(material.core, *core);
    assert_eq!(revision_ledger.entries.len(), 1);
    assert_eq!(head.current_ledger_digest, Some(revision_ledger_digest));
    assert_eq!(manifest.entries.len(), 3);
}

#[test]
fn owned_document_body_tamper_cannot_preserve_envelope_digest() {
    let owner = SubjectSoulOwnerV1 {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
    };
    let address = SubjectSoulManifestAddressV1 {
        namespace: "soul_core".to_string(),
        physical_key: "core:1".to_string(),
    };
    let mut document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        Some(1),
        &address,
        &serde_json::json!({"identity_anchor": "original"}),
    )
    .expect("owned document");
    document.body = serde_json::json!({"identity_anchor": "tampered"});
    assert!(document.validate_contract().is_err());
}

#[test]
fn provision_intent_digest_is_stable_without_ephemeral_closure_certificate() {
    let owner = SubjectSoulOwnerV1 {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
    };
    let intent = SubjectSoulProvisionIntentV1::Founding {
        operation_id: "operation:stable-replay".to_string(),
        human_actor_subject_id: "human:a".to_string(),
        charter: Box::new(
            SubjectSoulFoundingCharterSeedV1 {
                identity_anchor: Some("stable".to_string()),
                ..SubjectSoulFoundingCharterSeedV1::default()
            }
            .canonicalize()
            .expect("canonical charter"),
        ),
        source_asserted_at: Some(7),
    };
    let before =
        subject_soul_provision_intent_digest_v1(&owner, &intent).expect("preflight digest");
    let after_reopen =
        subject_soul_provision_intent_digest_v1(&owner, &intent).expect("reopen digest");
    assert_eq!(before, after_reopen);
}

#[test]
fn lifecycle_intent_digest_is_available_before_current_state_admission() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "operation:archive:stable-replay".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_state: SubjectSoulExpectedStateV1::Exact {
            generation: snapshot.head.generation,
            revision: snapshot.head.current_revision,
            lifecycle_state: snapshot.head.state,
            head_digest: snapshot.head.head_digest.clone(),
            manifest_digest: snapshot.manifest.closure_digest.clone(),
        },
        authority: SubjectSoulLifecycleAuthorityV1::SelfGovernance {
            capability_digest: "c".repeat(64),
        },
        action: SubjectSoulLifecycleActionV1::Archive,
    };

    let before = subject_soul_lifecycle_intent_digest_v1(&owner, &request)
        .expect("preflight lifecycle digest");
    let after_reopen =
        subject_soul_lifecycle_intent_digest_v1(&owner, &request).expect("reopen lifecycle digest");
    assert_eq!(before, after_reopen);
}

#[test]
fn terminated_generation_read_is_safe_metadata_without_soul_body() {
    let outcome = SubjectSoulReadOutcomeV1::TerminatedGeneration {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
        terminal: Box::new(SubjectSoulTerminatedGenerationV1 {
            generation: 1,
            terminal_revision: Some(1),
            terminal_material_digest: Some("a".repeat(64)),
            terminal_action: SubjectSoulTerminalActionV1::Reset,
            tombstone_digest: "b".repeat(64),
            terminated_at: 12,
            current_generation: 2,
            current_state: SubjectSoulLifecycleStateV1::Unseeded,
        }),
    };
    outcome.validate_contract().expect("safe terminal metadata");
    let encoded = serde_json::to_value(outcome).expect("serialize read outcome");
    assert!(encoded.get("runtime_private_core").is_none());
    assert!(encoded.get("governed_disclosure").is_none());
    assert!(encoded.get("body").is_none());
}

fn active_founding_snapshot() -> (
    SubjectSoulOwnerV1,
    SubjectSoulRevisionAddressBindingsV1,
    SubjectSoulVerifiedSnapshotV1,
) {
    let owner = SubjectSoulOwnerV1 {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
    };
    let bindings = SubjectSoulRevisionAddressBindingsV1 {
        material: SubjectSoulManifestAddressV1 {
            namespace: "soul_material".to_string(),
            physical_key: "material:1".to_string(),
        },
        core: SubjectSoulManifestAddressV1 {
            namespace: "soul_core".to_string(),
            physical_key: "core:1".to_string(),
        },
        revision_ledger: SubjectSoulManifestAddressV1 {
            namespace: "soul_ledger".to_string(),
            physical_key: "ledger:1".to_string(),
        },
    };
    let charter = SubjectSoulFoundingCharterSeedV1 {
        identity_anchor: Some("stable subject".to_string()),
        boundary_doctrine: Some("private raw stays private".to_string()),
        ..SubjectSoulFoundingCharterSeedV1::default()
    }
    .canonicalize()
    .expect("canonical charter");
    let plan = plan_subject_soul_provision_v1(
        &owner,
        &SubjectSoulProvisionIntentV1::Founding {
            operation_id: "operation:founding-snapshot".to_string(),
            human_actor_subject_id: "human:a".to_string(),
            charter: Box::new(charter),
            source_asserted_at: Some(7),
        },
        &SubjectSoulExpectedStateV1::PristineAbsent {
            closure_certificate_digest: "a".repeat(64),
        },
        Some(&bindings),
        11,
    )
    .expect("founding snapshot plan");
    let SubjectSoulProvisionPlanV1::Commit {
        head,
        manifest,
        material,
        core,
        core_document,
        revision_ledger,
        revision_ledger_document,
        ..
    } = plan
    else {
        panic!("founding commit");
    };
    (
        owner,
        bindings,
        SubjectSoulVerifiedSnapshotV1 {
            head: *head,
            manifest: *manifest,
            current_material: Some(*material),
            current_core: Some(*core),
            current_core_document: Some(*core_document),
            current_revision_ledger: Some(*revision_ledger),
            current_revision_ledger_document: Some(*revision_ledger_document),
        },
    )
}

fn autonomous_revision_bindings(revision: u64) -> SubjectSoulRevisionAddressBindingsV1 {
    SubjectSoulRevisionAddressBindingsV1 {
        material: SubjectSoulManifestAddressV1 {
            namespace: "soul_material".to_string(),
            physical_key: format!("material:{revision}"),
        },
        core: SubjectSoulManifestAddressV1 {
            namespace: "soul_core".to_string(),
            physical_key: format!("core:{revision}"),
        },
        revision_ledger: SubjectSoulManifestAddressV1 {
            namespace: "soul_ledger".to_string(),
            physical_key: format!("ledger:{revision}"),
        },
    }
}

fn autonomous_adoption_refresh_plan(
    existing_core: Option<&SelfAuthoredCore>,
    existing_ledger: &CoreRevisionLedger,
    next_core: SelfAuthoredCore,
    origin: SubjectSoulRevisionOriginV1,
    recorded_at: u64,
) -> SelfAuthoredCoreRefreshPlanV1 {
    let mut next_ledger = existing_ledger.clone();
    next_ledger.entries.push(CoreRevisionRecord {
        based_on_revision: existing_core.map(|core| core.revision).unwrap_or(0),
        resulting_revision: next_core.revision,
        source_layers: vec!["self_model".to_string()],
        outcome: CoreRevisionOutcome::Adopted,
        evidence_summary: vec!["stable multi-turn identity evidence".to_string()],
        adjudication_reason: "stable_identity_evidence".to_string(),
        rationale: "The subject adopted a stable self-governed revision.".to_string(),
        reviewed_at: recorded_at,
        ..CoreRevisionRecord::default()
    });
    next_ledger.updated_at = recorded_at;
    SelfAuthoredCoreRefreshPlanV1::Adopt {
        expected_prior: compute_self_authored_core_expected_prior_v1(
            existing_core,
            existing_ledger,
        )
        .expect("expected prior"),
        next_core: Box::new(next_core),
        next_ledger,
        origin,
        proposal_ref: format!("self-authored-proposal:{recorded_at}"),
        source_refs: vec!["self_model".to_string()],
    }
}

#[test]
fn autonomous_bootstrap_builds_revision_one_from_implicit_unseeded_in_one_post_image() {
    let owner = SubjectSoulOwnerV1 {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
    };
    let ledger = CoreRevisionLedger::default();
    let core = SelfAuthoredCore {
        revision: 1,
        supersedes_revision: None,
        identity_anchor: "autonomously grounded subject".to_string(),
        last_reviewed_at: 20,
        updated_at: 20,
        ..SelfAuthoredCore::default()
    };
    let refresh = autonomous_adoption_refresh_plan(
        None,
        &ledger,
        core,
        SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap,
        20,
    );
    let addresses = SubjectSoulSelfAuthoredPostImageAddressesV1::Adopt {
        revision: Box::new(autonomous_revision_bindings(1)),
    };
    let expected_a = SubjectSoulExpectedStateV1::PristineAbsent {
        closure_certificate_digest: "a".repeat(64),
    };
    let expected_b = SubjectSoulExpectedStateV1::PristineAbsent {
        closure_certificate_digest: "b".repeat(64),
    };
    assert_eq!(
        subject_soul_self_authored_revision_intent_digest_v1(&owner, &expected_a, &refresh)
            .expect("autonomous bootstrap intent digest"),
        subject_soul_self_authored_revision_intent_digest_v1(&owner, &expected_b, &refresh)
            .expect("reopened autonomous bootstrap intent digest"),
        "ephemeral pristine certificates cannot destabilize autonomous replay"
    );
    let plan = plan_subject_soul_self_authored_revision_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded {
            closure_certificate_digest: "a".repeat(64),
        },
        &refresh,
        Some(&addresses),
        20,
    )
    .expect("autonomous bootstrap post-image");
    let SubjectSoulSelfAuthoredCommitPlanV1::Adopt {
        post_head,
        post_manifest,
        material,
        revision_ledger_document,
        ..
    } = plan
    else {
        panic!("bootstrap must atomically adopt revision one");
    };
    assert_eq!(post_head.generation, 1);
    assert_eq!(post_head.current_revision, Some(1));
    assert_eq!(post_manifest.entries.len(), 3);
    assert_eq!(
        material.provenance.origin,
        SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap
    );
    assert_eq!(
        material.provenance.source_authority,
        SubjectSoulSourceAuthorityV1::SoulSelfGovernance
    );
    assert_eq!(material.provenance.source_subject_id, owner.subject_id);
    assert_eq!(
        post_head.current_ledger_digest.as_deref(),
        Some(revision_ledger_document.content_digest.as_str())
    );
}

#[test]
fn autonomous_bootstrap_uses_revision_one_inside_an_explicit_reset_generation() {
    let (owner, _, active) = active_founding_snapshot();
    let reset_request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "operation:reset-before-autonomous-bootstrap".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_state: SubjectSoulExpectedStateV1::Exact {
            generation: 1,
            revision: Some(1),
            lifecycle_state: SubjectSoulLifecycleStateV1::Active,
            head_digest: active.head.head_digest.clone(),
            manifest_digest: active.manifest.closure_digest.clone(),
        },
        authority: SubjectSoulLifecycleAuthorityV1::Destructive {
            system_actor_subject_id: "system:governor".to_string(),
            human_confirmation: HumanSoulLifecycleConfirmationV1 {
                human_subject_id: "human:a".to_string(),
                target_subject_id: owner.subject_id.clone(),
                expected_generation: 1,
                action: SubjectSoulTerminalActionV1::Reset,
                reason_code: "user_requested_reset".to_string(),
                confirmed_at: 12,
                evidence_digest: "b".repeat(64),
            },
        },
        action: SubjectSoulLifecycleActionV1::Reset {
            reason_code: "user_requested_reset".to_string(),
        },
    };
    let reset = plan_subject_soul_lifecycle_v1(
        &owner,
        &reset_request,
        &active,
        None,
        Some("tombstone:reset:1"),
        13,
    )
    .expect("reset plan");
    let explicit_unseeded = SubjectSoulVerifiedSnapshotV1 {
        head: *reset.post_head,
        manifest: *reset.post_manifest,
        current_material: None,
        current_core: None,
        current_core_document: None,
        current_revision_ledger: None,
        current_revision_ledger_document: None,
    };
    explicit_unseeded
        .validate_contract()
        .expect("explicit unseeded roots");
    let empty_ledger = CoreRevisionLedger::default();
    let refresh = autonomous_adoption_refresh_plan(
        None,
        &empty_ledger,
        SelfAuthoredCore {
            revision: 1,
            identity_anchor: "new autonomous generation".to_string(),
            last_reviewed_at: 20,
            updated_at: 20,
            ..SelfAuthoredCore::default()
        },
        SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap,
        20,
    );
    let plan = plan_subject_soul_self_authored_revision_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(explicit_unseeded),
        },
        &refresh,
        Some(&SubjectSoulSelfAuthoredPostImageAddressesV1::Adopt {
            revision: Box::new(SubjectSoulRevisionAddressBindingsV1 {
                material: SubjectSoulManifestAddressV1 {
                    namespace: "soul_material".to_string(),
                    physical_key: "material:g2:r1".to_string(),
                },
                core: SubjectSoulManifestAddressV1 {
                    namespace: "soul_core".to_string(),
                    physical_key: "core:g2:r1".to_string(),
                },
                revision_ledger: SubjectSoulManifestAddressV1 {
                    namespace: "soul_ledger".to_string(),
                    physical_key: "ledger:g2:r1".to_string(),
                },
            }),
        }),
        20,
    )
    .expect("explicit-unseeded autonomous bootstrap");
    let SubjectSoulSelfAuthoredCommitPlanV1::Adopt {
        post_head,
        material,
        ..
    } = plan
    else {
        panic!("explicit unseeded must bootstrap revision one");
    };
    assert_eq!(post_head.generation, 2);
    assert_eq!(post_head.current_revision, Some(1));
    assert_eq!(material.generation, 2);
    assert_eq!(material.revision, 1);
    assert_eq!(
        material.provenance.origin,
        SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap
    );
    assert_eq!(post_head.retained_tombstone_refs, vec!["tombstone:reset:1"]);
}

#[test]
fn founding_revision_advances_to_self_governed_revision_and_retains_exact_as_of_material() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let existing_core = snapshot.current_core.as_ref().expect("founding core");
    let existing_ledger = snapshot
        .current_revision_ledger
        .as_ref()
        .expect("founding ledger");
    let mut next_core = existing_core.clone();
    next_core.revision = 2;
    next_core.supersedes_revision = Some(1);
    next_core.identity_anchor = "self-governed continuing subject".to_string();
    next_core.last_reviewed_at = 20;
    next_core.updated_at = 20;
    let refresh = autonomous_adoption_refresh_plan(
        Some(existing_core),
        existing_ledger,
        next_core,
        SubjectSoulRevisionOriginV1::SelfGovernedRevision,
        20,
    );
    let plan = plan_subject_soul_self_authored_revision_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot),
        },
        &refresh,
        Some(&SubjectSoulSelfAuthoredPostImageAddressesV1::Adopt {
            revision: Box::new(autonomous_revision_bindings(2)),
        }),
        20,
    )
    .expect("self-governed successor");
    let SubjectSoulSelfAuthoredCommitPlanV1::Adopt {
        post_head,
        post_manifest,
        material,
        purge_manifest_addresses,
        ..
    } = plan
    else {
        panic!("stable evidence must adopt revision two");
    };
    assert_eq!(post_head.current_revision, Some(2));
    assert_eq!(post_head.retained_revision_refs, vec!["material:1"]);
    assert!(!post_manifest
        .entries
        .iter()
        .any(|entry| entry.physical_key == "material:1"));
    assert_eq!(material.supersedes_revision, Some(1));
    assert_eq!(
        material.provenance.origin,
        SubjectSoulRevisionOriginV1::SelfGovernedRevision
    );
    assert_eq!(
        material.provenance.proposal_ref.as_deref(),
        Some("self-authored-proposal:20")
    );
    assert_eq!(material.provenance.source_refs, vec!["self_model"]);
    assert!(purge_manifest_addresses
        .iter()
        .any(|address| address.physical_key == "core:1"));
    assert!(purge_manifest_addresses
        .iter()
        .any(|address| address.physical_key == "ledger:1"));
}

#[test]
fn reviewed_rejection_updates_only_current_ledger_closure_without_a_soul_revision() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let existing_core = snapshot.current_core.as_ref().expect("current core");
    let existing_ledger = snapshot.current_revision_ledger.as_ref().expect("ledger");
    let mut next_ledger = existing_ledger.clone();
    next_ledger.entries.push(CoreRevisionRecord {
        based_on_revision: 1,
        resulting_revision: 1,
        outcome: CoreRevisionOutcome::Deferred,
        adjudication_reason: "operational_only_evidence".to_string(),
        rationale: "Task and tool habits are not constitutional evidence.".to_string(),
        reviewed_at: 20,
        ..CoreRevisionRecord::default()
    });
    next_ledger.updated_at = 20;
    let refresh = SelfAuthoredCoreRefreshPlanV1::ReviewedRejected {
        expected_prior: compute_self_authored_core_expected_prior_v1(
            Some(existing_core),
            existing_ledger,
        )
        .expect("expected prior"),
        next_ledger,
        origin: SubjectSoulRevisionOriginV1::SelfGovernedRevision,
        proposal_ref: "self-authored-proposal:review:20".to_string(),
        source_refs: Vec::new(),
    };
    let prior_material_digest = snapshot.head.current_material_digest.clone();
    let plan = plan_subject_soul_self_authored_revision_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot),
        },
        &refresh,
        Some(
            &SubjectSoulSelfAuthoredPostImageAddressesV1::ReviewedRejected {
                revision_ledger: SubjectSoulManifestAddressV1 {
                    namespace: "soul_ledger".to_string(),
                    physical_key: "ledger:review:1".to_string(),
                },
            },
        ),
        20,
    )
    .expect("review-only ledger closure");
    let SubjectSoulSelfAuthoredCommitPlanV1::ReviewedRejected {
        post_head,
        post_manifest,
        revision_ledger,
        ..
    } = plan
    else {
        panic!("rejected evidence cannot create a Soul revision");
    };
    assert_eq!(post_head.current_revision, Some(1));
    assert_eq!(post_head.current_material_digest, prior_material_digest);
    assert_eq!(
        revision_ledger.entries.last().unwrap().outcome,
        CoreRevisionOutcome::Deferred
    );
    assert_eq!(
        post_manifest
            .entries
            .iter()
            .filter(|entry| entry.physical_key.starts_with("material:"))
            .count(),
        1
    );
}

#[test]
fn autonomous_cycle_combines_review_rejection_and_layer_evidence_without_a_fake_revision() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let existing_core = snapshot.current_core.as_ref().expect("current core");
    let existing_ledger = snapshot.current_revision_ledger.as_ref().expect("ledger");
    let mut next_ledger = existing_ledger.clone();
    next_ledger.entries.push(CoreRevisionRecord {
        based_on_revision: 1,
        resulting_revision: 1,
        outcome: CoreRevisionOutcome::Deferred,
        adjudication_reason: "task_habit_is_not_identity".to_string(),
        rationale: "Operational evidence remains below the constitutional boundary.".to_string(),
        reviewed_at: 20,
        ..CoreRevisionRecord::default()
    });
    next_ledger.updated_at = 20;
    let refresh = SelfAuthoredCoreRefreshPlanV1::ReviewedRejected {
        expected_prior: compute_self_authored_core_expected_prior_v1(
            Some(existing_core),
            existing_ledger,
        )
        .expect("expected prior"),
        next_ledger,
        origin: SubjectSoulRevisionOriginV1::SelfGovernedRevision,
        proposal_ref: "self-authored-proposal:cycle-rejected".to_string(),
        source_refs: Vec::new(),
    };
    let address = SubjectSoulManifestAddressV1 {
        namespace: "outer_voice".to_string(),
        physical_key: "outer-voice:cycle-rejected".to_string(),
    };
    let intent = autonomous_cycle_intent(
        "operation:cycle-reviewed-rejected",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::OuterVoice,
            expected_previous_digest: None,
            document: Box::new(
                SubjectSoulOwnedDocumentV1::new(
                    &owner,
                    1,
                    Some(1),
                    &address,
                    &serde_json::json!({"style": "calm but not constitutional"}),
                )
                .expect("outer voice evidence"),
            ),
        }],
    );
    let plan = plan_subject_soul_autonomous_cycle_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &intent,
        &refresh,
        Some(
            &SubjectSoulSelfAuthoredPostImageAddressesV1::ReviewedRejected {
                revision_ledger: SubjectSoulManifestAddressV1 {
                    namespace: "soul_ledger".to_string(),
                    physical_key: "ledger:cycle-rejected".to_string(),
                },
            },
        ),
        20,
    )
    .expect("review rejection and layer evidence cycle");
    let SubjectSoulAutonomousCyclePlanV1::Commit {
        post_image,
        revision_delta,
        layer_upserts,
        ..
    } = plan
    else {
        panic!("review decision must commit its ledger closure");
    };
    assert_eq!(post_image.head.current_revision, Some(1));
    assert_eq!(
        post_image.head.current_material_digest,
        snapshot.head.current_material_digest
    );
    assert_eq!(layer_upserts.len(), 1);
    assert_eq!(layer_upserts[0].revision, Some(1));
    assert_eq!(
        post_image
            .current_revision_ledger
            .as_ref()
            .expect("review ledger")
            .entries
            .last()
            .expect("review decision")
            .outcome,
        CoreRevisionOutcome::Deferred
    );
    assert!(matches!(
        *revision_delta,
        SubjectSoulAutonomousRevisionDeltaV1::ReviewedRejected { .. }
    ));

    let refresh_only_intent = autonomous_cycle_intent(
        "operation:cycle-reviewed-rejected-without-layers",
        Vec::new(),
    );
    let refresh_only = plan_subject_soul_autonomous_cycle_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &refresh_only_intent,
        &refresh,
        Some(
            &SubjectSoulSelfAuthoredPostImageAddressesV1::ReviewedRejected {
                revision_ledger: SubjectSoulManifestAddressV1 {
                    namespace: "soul_ledger".to_string(),
                    physical_key: "ledger:cycle-rejected-no-layers".to_string(),
                },
            },
        ),
        20,
    )
    .expect("refresh-only autonomous cycle");
    let SubjectSoulAutonomousCyclePlanV1::Commit {
        intent_digest,
        layer_upserts,
        layer_deletes,
        ..
    } = refresh_only
    else {
        panic!("reviewed rejection without layers must still commit its ledger");
    };
    assert_eq!(intent_digest.len(), 64);
    assert!(layer_upserts.is_empty());
    assert!(layer_deletes.is_empty());

    let mut changed_refresh = refresh.clone();
    let SelfAuthoredCoreRefreshPlanV1::ReviewedRejected { proposal_ref, .. } = &mut changed_refresh
    else {
        unreachable!("fixture is a reviewed rejection")
    };
    *proposal_ref = "self-authored-proposal:different-decision".to_string();
    let expected = SubjectSoulExpectedStateV1::Exact {
        generation: snapshot.head.generation,
        revision: snapshot.head.current_revision,
        lifecycle_state: snapshot.head.state,
        head_digest: snapshot.head.head_digest.clone(),
        manifest_digest: snapshot.manifest.closure_digest.clone(),
    };
    assert_ne!(
        intent_digest,
        subject_soul_autonomous_cycle_intent_digest_v1(
            &owner,
            &expected,
            &refresh_only_intent,
            &changed_refresh,
        )
        .expect("changed refresh digest"),
        "same durable job id with a different refresh decision must conflict"
    );
}

#[test]
fn self_authored_planner_rejects_a_forged_non_successor_ledger() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let existing_core = snapshot.current_core.as_ref().expect("current core");
    let existing_ledger = snapshot.current_revision_ledger.as_ref().expect("ledger");
    let forged_ledger = CoreRevisionLedger {
        entries: vec![CoreRevisionRecord {
            based_on_revision: 1,
            resulting_revision: 1,
            outcome: CoreRevisionOutcome::Deferred,
            adjudication_reason: "forged_history_replacement".to_string(),
            rationale: "A caller cannot replace the founding ledger history.".to_string(),
            reviewed_at: 20,
            ..CoreRevisionRecord::default()
        }],
        updated_at: 20,
    };
    let refresh = SelfAuthoredCoreRefreshPlanV1::ReviewedRejected {
        expected_prior: compute_self_authored_core_expected_prior_v1(
            Some(existing_core),
            existing_ledger,
        )
        .expect("expected prior"),
        next_ledger: forged_ledger,
        origin: SubjectSoulRevisionOriginV1::SelfGovernedRevision,
        proposal_ref: "self-authored-proposal:forged-ledger".to_string(),
        source_refs: Vec::new(),
    };

    let error = plan_subject_soul_self_authored_revision_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot),
        },
        &refresh,
        Some(
            &SubjectSoulSelfAuthoredPostImageAddressesV1::ReviewedRejected {
                revision_ledger: SubjectSoulManifestAddressV1 {
                    namespace: "soul_ledger".to_string(),
                    physical_key: "ledger:forged".to_string(),
                },
            },
        ),
        20,
    )
    .expect_err("the next ledger must append to the exact observed history");
    assert_eq!(
        error.key,
        bm_core::memory::SubjectSoulLifecycleErrorKey::RepairRequired
    );
    assert!(error.reason.contains("canonical successor"));
}

#[test]
fn autonomous_revision_overflow_fails_closed_before_allocating_a_post_image() {
    let (owner, _, mut snapshot) = active_founding_snapshot();
    let max_revision = u64::MAX;
    let mut core = snapshot.current_core.take().expect("core");
    core.revision = max_revision;
    core.supersedes_revision = Some(max_revision - 1);
    let mut material = snapshot.current_material.take().expect("material");
    material.revision = max_revision;
    material.supersedes_revision = Some(max_revision - 1);
    material.core = core.clone();
    material.refresh_digest().expect("max material");
    let old_core_document = snapshot
        .current_core_document
        .take()
        .expect("core document");
    let core_document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        Some(max_revision),
        &SubjectSoulManifestAddressV1 {
            namespace: old_core_document.namespace,
            physical_key: old_core_document.physical_key,
        },
        &core,
    )
    .expect("max core document");
    let ledger = snapshot.current_revision_ledger.clone().expect("ledger");
    let old_ledger_document = snapshot
        .current_revision_ledger_document
        .take()
        .expect("ledger document");
    let ledger_document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        Some(max_revision),
        &SubjectSoulManifestAddressV1 {
            namespace: old_ledger_document.namespace,
            physical_key: old_ledger_document.physical_key,
        },
        &ledger,
    )
    .expect("max ledger document");
    for entry in &mut snapshot.manifest.entries {
        entry.revision = Some(max_revision);
        if entry.physical_key == "material:1" {
            entry.content_digest = material.content_digest.clone();
        } else if entry.physical_key == "core:1" {
            entry.content_digest = core_document.content_digest.clone();
        } else if entry.physical_key == "ledger:1" {
            entry.content_digest = ledger_document.content_digest.clone();
        }
    }
    snapshot.manifest.refresh_digest().expect("max manifest");
    snapshot.head.current_revision = Some(max_revision);
    snapshot.head.current_material_digest = Some(material.content_digest.clone());
    snapshot.head.current_ledger_digest = Some(ledger_document.content_digest.clone());
    snapshot.head.scope_manifest_digest = snapshot.manifest.closure_digest.clone();
    snapshot.head.refresh_digest().expect("max head");
    snapshot.current_material = Some(material);
    snapshot.current_core = Some(core.clone());
    snapshot.current_core_document = Some(core_document);
    snapshot.current_revision_ledger_document = Some(ledger_document);
    snapshot
        .validate_contract()
        .expect("valid max revision snapshot");

    let mut impossible_next = core;
    impossible_next.updated_at = 20;
    impossible_next.last_reviewed_at = 20;
    let refresh = autonomous_adoption_refresh_plan(
        snapshot.current_core.as_ref(),
        &ledger,
        impossible_next,
        SubjectSoulRevisionOriginV1::SelfGovernedRevision,
        20,
    );
    let error = plan_subject_soul_autonomous_cycle_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot),
        },
        &autonomous_cycle_intent("operation:overflow-cycle", Vec::new()),
        &refresh,
        Some(&SubjectSoulSelfAuthoredPostImageAddressesV1::Adopt {
            revision: Box::new(autonomous_revision_bindings(max_revision)),
        }),
        20,
    )
    .expect_err("revision overflow must fail closed");
    assert_eq!(
        error.key,
        bm_core::memory::SubjectSoulLifecycleErrorKey::GenerationConflict
    );
    assert!(error.reason.contains("overflow"));
}

#[test]
fn autonomous_cycle_rejects_archived_and_deleted_soul_roots() {
    let (owner, _, active) = active_founding_snapshot();
    let mut archived = active.clone();
    archived.head.state = SubjectSoulLifecycleStateV1::Archived;
    archived.head.updated_at = 20;
    archived.head.refresh_digest().expect("archived head");
    archived
        .validate_contract()
        .expect("valid archived snapshot");
    let archived_error = plan_subject_soul_autonomous_cycle_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(archived),
        },
        &autonomous_cycle_intent("operation:archived-cycle", Vec::new()),
        &SelfAuthoredCoreRefreshPlanV1::Skipped,
        None,
        21,
    )
    .expect_err("archived Soul must fail closed before no-effect admission");
    assert_eq!(
        archived_error.key,
        bm_core::memory::SubjectSoulLifecycleErrorKey::Archived
    );

    let delete_request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "operation:delete-before-cycle".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_state: SubjectSoulExpectedStateV1::Exact {
            generation: active.head.generation,
            revision: active.head.current_revision,
            lifecycle_state: active.head.state,
            head_digest: active.head.head_digest.clone(),
            manifest_digest: active.manifest.closure_digest.clone(),
        },
        authority: SubjectSoulLifecycleAuthorityV1::Destructive {
            system_actor_subject_id: "system:governor".to_string(),
            human_confirmation: HumanSoulLifecycleConfirmationV1 {
                human_subject_id: "human:a".to_string(),
                target_subject_id: owner.subject_id.clone(),
                expected_generation: active.head.generation,
                action: SubjectSoulTerminalActionV1::Delete,
                reason_code: "terminal_delete".to_string(),
                confirmed_at: 20,
                evidence_digest: "d".repeat(64),
            },
        },
        action: SubjectSoulLifecycleActionV1::Delete {
            reason_code: "terminal_delete".to_string(),
        },
    };
    let deleted_plan = plan_subject_soul_lifecycle_v1(
        &owner,
        &delete_request,
        &active,
        None,
        Some("tombstone:deleted-before-cycle"),
        21,
    )
    .expect("deleted lifecycle post-image");
    let deleted = SubjectSoulVerifiedSnapshotV1 {
        head: *deleted_plan.post_head,
        manifest: *deleted_plan.post_manifest,
        current_material: None,
        current_core: None,
        current_core_document: None,
        current_revision_ledger: None,
        current_revision_ledger_document: None,
    };
    deleted.validate_contract().expect("valid deleted snapshot");
    let deleted_error = plan_subject_soul_autonomous_cycle_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(deleted),
        },
        &autonomous_cycle_intent("operation:deleted-cycle", Vec::new()),
        &SelfAuthoredCoreRefreshPlanV1::Skipped,
        None,
        22,
    )
    .expect_err("deleted Soul must fail closed before no-effect admission");
    assert_eq!(
        deleted_error.key,
        bm_core::memory::SubjectSoulLifecycleErrorKey::Deleted
    );
}

fn generation_layer_intent(
    operation_id: &str,
    mutations: Vec<SubjectSoulGenerationLayerMutationV1>,
) -> SubjectSoulGenerationLayerIntentV1 {
    SubjectSoulGenerationLayerIntentV1 {
        operation_id: operation_id.to_string(),
        authority: SubjectSoulGenerationLayerAuthorityV1::MountedAgentPersona {
            actor_subject_id: "agent:a".to_string(),
        },
        mutations,
    }
}

fn autonomous_cycle_intent(
    operation_id: &str,
    mutations: Vec<SubjectSoulGenerationLayerMutationV1>,
) -> SubjectSoulAutonomousCycleIntentV1 {
    SubjectSoulAutonomousCycleIntentV1 {
        operation_id: operation_id.to_string(),
        actor_subject_id: "agent:a".to_string(),
        layer_mutations: mutations,
    }
}

#[test]
fn first_governed_layer_write_creates_explicit_unseeded_without_a_default_core() {
    let owner = SubjectSoulOwnerV1 {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
    };
    let address = SubjectSoulManifestAddressV1 {
        namespace: "self_model".to_string(),
        physical_key: "self-model:first-evidence".to_string(),
    };
    let document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        None,
        &address,
        &serde_json::json!({"continuity_anchor": "first stable evidence"}),
    )
    .expect("unseeded evidence envelope");
    let intent = generation_layer_intent(
        "operation:first-soul-evidence",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfModel,
            expected_previous_digest: None,
            document: Box::new(document.clone()),
        }],
    );
    let expected_a = SubjectSoulExpectedStateV1::PristineAbsent {
        closure_certificate_digest: "a".repeat(64),
    };
    let expected_b = SubjectSoulExpectedStateV1::PristineAbsent {
        closure_certificate_digest: "b".repeat(64),
    };
    let digest = subject_soul_generation_layer_intent_digest_v1(&owner, &expected_a, &intent)
        .expect("first evidence digest");
    assert_eq!(
        digest,
        subject_soul_generation_layer_intent_digest_v1(&owner, &expected_b, &intent)
            .expect("reopen first evidence digest"),
        "ephemeral open certificates cannot destabilize durable replay"
    );
    let plan = plan_subject_soul_generation_layer_delta_v1(
        &owner,
        &SubjectSoulGenerationLayerBasisV1::ImplicitUnseeded {
            closure_certificate_digest: "a".repeat(64),
        },
        &intent,
        10,
    )
    .expect("first governed Soul evidence write");
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        expected_state,
        intent_digest,
        post_head,
        post_manifest,
        upsert_documents,
        ..
    } = plan
    else {
        panic!("first evidence cannot be a no-effect plan");
    };
    assert_eq!(expected_state, expected_a);
    assert_eq!(intent_digest, digest);
    assert_eq!(post_head.state, SubjectSoulLifecycleStateV1::Unseeded);
    assert_eq!(post_head.generation, 1);
    assert_eq!(post_head.current_revision, None);
    assert_eq!(post_head.current_material_digest, None);
    assert_eq!(post_head.current_ledger_digest, None);
    assert_eq!(post_manifest.entries.len(), 1);
    assert_eq!(post_manifest.entries[0].namespace, "self_model");
    assert_eq!(upsert_documents, vec![document]);
    assert!(!post_manifest
        .entries
        .iter()
        .any(|entry| entry.namespace == "self_authored_core"));

    let founding = plan_subject_soul_provision_v1(
        &owner,
        &SubjectSoulProvisionIntentV1::Founding {
            operation_id: "operation:concurrent-founding".to_string(),
            human_actor_subject_id: "human:a".to_string(),
            charter: Box::new(
                SubjectSoulFoundingCharterSeedV1 {
                    identity_anchor: Some("human founding".to_string()),
                    ..SubjectSoulFoundingCharterSeedV1::default()
                }
                .canonicalize()
                .expect("founding charter"),
            ),
            source_asserted_at: Some(9),
        },
        &expected_a,
        Some(&autonomous_revision_bindings(1)),
        10,
    )
    .expect("concurrent founding plan");
    let SubjectSoulProvisionPlanV1::Commit {
        expected_state: founding_expected,
        ..
    } = founding
    else {
        panic!("founding must allocate");
    };
    assert_eq!(
        founding_expected, expected_a,
        "both writers must race on one absent CAS"
    );

    let changed_document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        None,
        &address,
        &serde_json::json!({"continuity_anchor": "different evidence"}),
    )
    .expect("changed evidence envelope");
    let changed_intent = generation_layer_intent(
        "operation:first-soul-evidence",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfModel,
            expected_previous_digest: None,
            document: Box::new(changed_document),
        }],
    );
    assert_ne!(
        digest,
        subject_soul_generation_layer_intent_digest_v1(&owner, &expected_b, &changed_intent)
            .expect("changed intent digest"),
        "same operation with a different private body must conflict by digest"
    );
    let other_layer_address = SubjectSoulManifestAddressV1 {
        namespace: "self_continuity".to_string(),
        physical_key: "self-continuity:first-evidence".to_string(),
    };
    let other_layer_intent = generation_layer_intent(
        "operation:first-soul-evidence",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfContinuity,
            expected_previous_digest: None,
            document: Box::new(
                SubjectSoulOwnedDocumentV1::new(
                    &owner,
                    1,
                    None,
                    &other_layer_address,
                    &serde_json::json!({"continuity_anchor": "first stable evidence"}),
                )
                .expect("other first evidence layer"),
            ),
        }],
    );
    assert_ne!(
        digest,
        subject_soul_generation_layer_intent_digest_v1(&owner, &expected_b, &other_layer_intent)
            .expect("changed layer intent digest"),
        "same operation with a different owned layer must conflict by digest"
    );
}

#[test]
fn autonomous_cycle_requires_one_manifest_revision_for_first_evidence_and_bootstrap() {
    let owner = SubjectSoulOwnerV1 {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
    };
    let basis = SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded {
        closure_certificate_digest: "a".repeat(64),
    };
    let document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        None,
        &SubjectSoulManifestAddressV1 {
            namespace: "self_model".to_string(),
            physical_key: "self-model:atomic-bootstrap".to_string(),
        },
        &serde_json::json!({"identity_anchor": "stable autonomous evidence"}),
    )
    .expect("first evidence envelope");
    let intent = SubjectSoulAutonomousCycleIntentV1 {
        operation_id: "operation:atomic-autonomous-cycle".to_string(),
        actor_subject_id: owner.subject_id.clone(),
        layer_mutations: vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfModel,
            expected_previous_digest: None,
            document: Box::new(document),
        }],
    };
    let refresh = autonomous_adoption_refresh_plan(
        None,
        &CoreRevisionLedger::default(),
        SelfAuthoredCore {
            revision: 1,
            identity_anchor: "autonomously grounded subject".to_string(),
            last_reviewed_at: 20,
            updated_at: 20,
            ..SelfAuthoredCore::default()
        },
        SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap,
        20,
    );
    let expected_a = SubjectSoulExpectedStateV1::PristineAbsent {
        closure_certificate_digest: "a".repeat(64),
    };
    let expected_b = SubjectSoulExpectedStateV1::PristineAbsent {
        closure_certificate_digest: "b".repeat(64),
    };
    assert_eq!(
        subject_soul_autonomous_cycle_intent_digest_v1(&owner, &expected_a, &intent, &refresh,)
            .expect("autonomous cycle digest"),
        subject_soul_autonomous_cycle_intent_digest_v1(&owner, &expected_b, &intent, &refresh,)
            .expect("reopened autonomous cycle digest"),
        "Store-open certificate changes cannot destabilize autonomous MOR replay"
    );
    let plan = plan_subject_soul_autonomous_cycle_v1(
        &owner,
        &basis,
        &intent,
        &refresh,
        Some(&SubjectSoulSelfAuthoredPostImageAddressesV1::Adopt {
            revision: Box::new(autonomous_revision_bindings(1)),
        }),
        20,
    )
    .expect("atomic autonomous cycle");
    let SubjectSoulAutonomousCyclePlanV1::Commit {
        post_image,
        revision_delta,
        layer_upserts,
        ..
    } = plan
    else {
        panic!("bootstrap cycle must commit");
    };
    assert_eq!(
        post_image.manifest.manifest_revision, 1,
        "one autonomous Store transaction must expose one manifest revision"
    );
    assert_eq!(post_image.head.current_revision, Some(1));
    assert_eq!(post_image.head.updated_at, 20);
    assert_eq!(post_image.manifest.entries.len(), 4);
    assert_eq!(layer_upserts.len(), 1);
    assert_eq!(layer_upserts[0].revision, Some(1));
    assert!(matches!(
        *revision_delta,
        SubjectSoulAutonomousRevisionDeltaV1::Adopt { .. }
    ));
}

#[test]
fn autonomous_cycle_persists_all_generation_layers_without_inventing_an_unseeded_core() {
    let owner = SubjectSoulOwnerV1 {
        memory_space_id: "space:1".to_string(),
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
    };
    let layers = [
        (SubjectSoulGenerationLayerKindV1::SelfModel, "self_model"),
        (
            SubjectSoulGenerationLayerKindV1::SelfContinuity,
            "self_continuity",
        ),
        (
            SubjectSoulGenerationLayerKindV1::RelationshipPortfolio,
            "relationship_portfolio",
        ),
        (
            SubjectSoulGenerationLayerKindV1::RelationshipTopology,
            "relationship_topology",
        ),
        (
            SubjectSoulGenerationLayerKindV1::AutonomyStrategy,
            "autonomy_strategy",
        ),
        (SubjectSoulGenerationLayerKindV1::InnerLife, "inner_life"),
        (
            SubjectSoulGenerationLayerKindV1::FeltSignificance,
            "felt_significance",
        ),
        (
            SubjectSoulGenerationLayerKindV1::TemperamentContinuity,
            "temperament_continuity",
        ),
        (
            SubjectSoulGenerationLayerKindV1::InnerConflict,
            "inner_conflict",
        ),
        (
            SubjectSoulGenerationLayerKindV1::MentalPrivacy,
            "mental_privacy",
        ),
        (
            SubjectSoulGenerationLayerKindV1::PrivateDocument,
            "private_doc",
        ),
        (
            SubjectSoulGenerationLayerKindV1::PrivateGarden,
            "private_garden",
        ),
        (SubjectSoulGenerationLayerKindV1::OuterVoice, "outer_voice"),
    ];
    let mutations = layers
        .iter()
        .enumerate()
        .map(|(index, (layer, namespace))| {
            let address = SubjectSoulManifestAddressV1 {
                namespace: (*namespace).to_string(),
                physical_key: format!("{namespace}:cycle:{index}"),
            };
            SubjectSoulGenerationLayerMutationV1::Upsert {
                layer: *layer,
                expected_previous_digest: None,
                document: Box::new(
                    SubjectSoulOwnedDocumentV1::new(
                        &owner,
                        1,
                        None,
                        &address,
                        &serde_json::json!({"synthetic_layer_index": index}),
                    )
                    .expect("typed generation layer envelope"),
                ),
            }
        })
        .collect::<Vec<_>>();
    let intent = autonomous_cycle_intent("operation:all-generation-layers", mutations);
    let expected = SubjectSoulExpectedStateV1::PristineAbsent {
        closure_certificate_digest: "a".repeat(64),
    };
    let original_digest = subject_soul_autonomous_cycle_intent_digest_v1(
        &owner,
        &expected,
        &intent,
        &SelfAuthoredCoreRefreshPlanV1::Skipped,
    )
    .expect("all-layer cycle digest");
    let plan = plan_subject_soul_autonomous_cycle_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded {
            closure_certificate_digest: "a".repeat(64),
        },
        &intent,
        &SelfAuthoredCoreRefreshPlanV1::Skipped,
        None,
        20,
    )
    .expect("all generation layers in one unseeded cycle");
    let SubjectSoulAutonomousCyclePlanV1::Commit {
        post_image,
        revision_delta,
        layer_upserts,
        ..
    } = plan
    else {
        panic!("first governed layer cycle must commit");
    };
    assert_eq!(post_image.head.state, SubjectSoulLifecycleStateV1::Unseeded);
    assert_eq!(post_image.head.current_revision, None);
    assert_eq!(post_image.manifest.manifest_revision, 1);
    assert_eq!(post_image.manifest.entries.len(), 13);
    assert_eq!(layer_upserts.len(), 13);
    assert!(layer_upserts
        .iter()
        .all(|document| document.revision.is_none()));
    assert!(post_image.current_material.is_none());
    assert!(post_image.current_core.is_none());
    assert!(post_image.current_revision_ledger.is_none());
    assert!(matches!(
        *revision_delta,
        SubjectSoulAutonomousRevisionDeltaV1::None
    ));

    let mut changed = intent.clone();
    let SubjectSoulGenerationLayerMutationV1::Upsert { document, .. } =
        &mut changed.layer_mutations[0]
    else {
        unreachable!("fixture contains only upserts")
    };
    **document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        None,
        &SubjectSoulManifestAddressV1 {
            namespace: "self_model".to_string(),
            physical_key: "self_model:cycle:0".to_string(),
        },
        &serde_json::json!({"synthetic_layer_index": "changed"}),
    )
    .expect("changed layer body");
    assert_ne!(
        original_digest,
        subject_soul_autonomous_cycle_intent_digest_v1(
            &owner,
            &expected,
            &changed,
            &SelfAuthoredCoreRefreshPlanV1::Skipped,
        )
        .expect("changed all-layer cycle digest"),
        "same operation id with a different layer body must conflict"
    );
}

#[test]
fn autonomous_cycle_advances_founding_to_revision_two_with_one_final_root() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let previous_manifest_revision = snapshot.manifest.manifest_revision;
    let existing_core = snapshot.current_core.as_ref().expect("founding core");
    let existing_ledger = snapshot
        .current_revision_ledger
        .as_ref()
        .expect("founding ledger");
    let mut next_core = existing_core.clone();
    next_core.revision = 2;
    next_core.supersedes_revision = Some(1);
    next_core.identity_anchor = "self-governed revision two".to_string();
    next_core.last_reviewed_at = 20;
    next_core.updated_at = 20;
    let refresh = autonomous_adoption_refresh_plan(
        Some(existing_core),
        existing_ledger,
        next_core,
        SubjectSoulRevisionOriginV1::SelfGovernedRevision,
        20,
    );
    let layer_address = SubjectSoulManifestAddressV1 {
        namespace: "self_continuity".to_string(),
        physical_key: "self-continuity:cycle:rev2".to_string(),
    };
    let intent = autonomous_cycle_intent(
        "operation:founding-to-rev2-cycle",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfContinuity,
            expected_previous_digest: None,
            document: Box::new(
                SubjectSoulOwnedDocumentV1::new(
                    &owner,
                    1,
                    Some(1),
                    &layer_address,
                    &serde_json::json!({"continuity": "stable across revisions"}),
                )
                .expect("revision-one layer input"),
            ),
        }],
    );
    let plan = plan_subject_soul_autonomous_cycle_v1(
        &owner,
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &intent,
        &refresh,
        Some(&SubjectSoulSelfAuthoredPostImageAddressesV1::Adopt {
            revision: Box::new(autonomous_revision_bindings(2)),
        }),
        20,
    )
    .expect("founding to self-governed revision cycle");
    let SubjectSoulAutonomousCyclePlanV1::Commit {
        expected_state,
        post_image,
        revision_delta,
        layer_upserts,
        ..
    } = plan
    else {
        panic!("self-governed revision must commit");
    };
    assert!(matches!(
        expected_state,
        SubjectSoulExpectedStateV1::Exact { .. }
    ));
    assert_eq!(post_image.head.current_revision, Some(2));
    assert_eq!(
        post_image.manifest.manifest_revision,
        previous_manifest_revision + 1
    );
    assert_eq!(post_image.head.retained_revision_refs, vec!["material:1"]);
    assert_eq!(layer_upserts.len(), 1);
    assert_eq!(layer_upserts[0].revision, Some(2));
    assert_eq!(
        post_image
            .current_material
            .as_ref()
            .expect("revision two material")
            .provenance
            .origin,
        SubjectSoulRevisionOriginV1::SelfGovernedRevision
    );
    assert!(matches!(
        *revision_delta,
        SubjectSoulAutonomousRevisionDeltaV1::Adopt { .. }
    ));
}

#[test]
fn reset_unseeded_generation_accepts_evidence_without_creating_core_material() {
    let (owner, _, active) = active_founding_snapshot();
    let reset_request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "operation:reset-before-evidence".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_state: SubjectSoulExpectedStateV1::Exact {
            generation: 1,
            revision: Some(1),
            lifecycle_state: SubjectSoulLifecycleStateV1::Active,
            head_digest: active.head.head_digest.clone(),
            manifest_digest: active.manifest.closure_digest.clone(),
        },
        authority: SubjectSoulLifecycleAuthorityV1::Destructive {
            system_actor_subject_id: "system:governor".to_string(),
            human_confirmation: HumanSoulLifecycleConfirmationV1 {
                human_subject_id: "human:a".to_string(),
                target_subject_id: owner.subject_id.clone(),
                expected_generation: 1,
                action: SubjectSoulTerminalActionV1::Reset,
                reason_code: "reset_for_evidence".to_string(),
                confirmed_at: 12,
                evidence_digest: "c".repeat(64),
            },
        },
        action: SubjectSoulLifecycleActionV1::Reset {
            reason_code: "reset_for_evidence".to_string(),
        },
    };
    let reset = plan_subject_soul_lifecycle_v1(
        &owner,
        &reset_request,
        &active,
        None,
        Some("tombstone:reset:evidence"),
        13,
    )
    .expect("reset plan");
    let explicit_unseeded = SubjectSoulVerifiedSnapshotV1 {
        head: *reset.post_head,
        manifest: *reset.post_manifest,
        current_material: None,
        current_core: None,
        current_core_document: None,
        current_revision_ledger: None,
        current_revision_ledger_document: None,
    };
    let address = SubjectSoulManifestAddressV1 {
        namespace: "self_continuity".to_string(),
        physical_key: "continuity:g2".to_string(),
    };
    let document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        2,
        None,
        &address,
        &serde_json::json!({"wake_anchor": "new generation only"}),
    )
    .expect("reset generation evidence");
    let plan = plan_subject_soul_generation_layer_delta_v1(
        &owner,
        &SubjectSoulGenerationLayerBasisV1::Verified {
            snapshot: Box::new(explicit_unseeded),
        },
        &generation_layer_intent(
            "operation:reset-generation-evidence",
            vec![SubjectSoulGenerationLayerMutationV1::Upsert {
                layer: SubjectSoulGenerationLayerKindV1::SelfContinuity,
                expected_previous_digest: None,
                document: Box::new(document),
            }],
        ),
        14,
    )
    .expect("reset generation evidence plan");
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_head,
        post_manifest,
        ..
    } = plan
    else {
        panic!("new evidence must commit");
    };
    assert_eq!(post_head.generation, 2);
    assert_eq!(post_head.state, SubjectSoulLifecycleStateV1::Unseeded);
    assert!(post_head.current_revision.is_none());
    assert!(post_head.current_material_digest.is_none());
    assert!(post_head.current_ledger_digest.is_none());
    assert_eq!(post_manifest.entries.len(), 1);
    assert_eq!(
        post_head.retained_tombstone_refs,
        vec!["tombstone:reset:evidence"]
    );
}

#[test]
fn generation_layer_delta_upsert_and_delete_preserve_revision_roots() {
    let (owner, _, mut snapshot) = active_founding_snapshot();
    let address = SubjectSoulManifestAddressV1 {
        namespace: "self_model".to_string(),
        physical_key: "self-model:1".to_string(),
    };
    let document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        snapshot.head.generation,
        Some(1),
        &address,
        &serde_json::json!({"continuity_anchor": "same autonomous subject"}),
    )
    .expect("generation layer envelope");
    let upsert_intent = generation_layer_intent(
        "operation:layer-upsert",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfModel,
            expected_previous_digest: None,
            document: Box::new(document.clone()),
        }],
    );
    let upsert = plan_subject_soul_generation_layer_delta_v1(
        &owner,
        &SubjectSoulGenerationLayerBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &upsert_intent,
        20,
    )
    .expect("layer upsert");
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_head,
        post_manifest,
        upsert_documents,
        ..
    } = upsert
    else {
        panic!("new layer must produce a commit plan");
    };
    assert_eq!(post_head.current_revision, Some(1));
    assert_eq!(upsert_documents, vec![document.clone()]);
    assert!(post_manifest.entries.iter().any(|entry| {
        entry.namespace == "self_model"
            && entry.owner_role == bm_core::memory::SubjectSoulManifestOwnerRoleV1::Private
    }));
    snapshot.head = *post_head;
    snapshot.manifest = *post_manifest;
    let no_effect_intent = generation_layer_intent(
        "operation:layer-no-effect",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfModel,
            expected_previous_digest: Some(document.content_digest.clone()),
            document: Box::new(document.clone()),
        }],
    );
    let no_effect = plan_subject_soul_generation_layer_delta_v1(
        &owner,
        &SubjectSoulGenerationLayerBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &no_effect_intent,
        21,
    )
    .expect("exact identical layer post-image");
    assert!(matches!(
        no_effect,
        SubjectSoulGenerationLayerDeltaPlanV1::NoEffect { .. }
    ));
    let delete_intent = generation_layer_intent(
        "operation:layer-delete",
        vec![SubjectSoulGenerationLayerMutationV1::Delete {
            layer: SubjectSoulGenerationLayerKindV1::SelfModel,
            address,
            expected_content_digest: document.content_digest,
        }],
    );
    let delete = plan_subject_soul_generation_layer_delta_v1(
        &owner,
        &SubjectSoulGenerationLayerBasisV1::Verified {
            snapshot: Box::new(snapshot),
        },
        &delete_intent,
        21,
    )
    .expect("layer delete");
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit { post_manifest, .. } = delete else {
        panic!("exact layer delete must commit");
    };
    assert!(!post_manifest
        .entries
        .iter()
        .any(|entry| entry.namespace == "self_model"));
}

#[test]
fn generation_layer_delta_rejects_duplicate_cross_generation_and_root_namespace_targets() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let valid_address = SubjectSoulManifestAddressV1 {
        namespace: "self_model".to_string(),
        physical_key: "self-model:1".to_string(),
    };
    let valid_document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        Some(1),
        &valid_address,
        &serde_json::json!({"anchor": "stable"}),
    )
    .expect("valid layer document");
    let duplicate = SubjectSoulGenerationLayerMutationV1::Upsert {
        layer: SubjectSoulGenerationLayerKindV1::SelfModel,
        expected_previous_digest: None,
        document: Box::new(valid_document.clone()),
    };
    let duplicate_intent = generation_layer_intent(
        "operation:duplicate-layer",
        vec![duplicate.clone(), duplicate],
    );
    assert!(plan_subject_soul_generation_layer_delta_v1(
        &owner,
        &SubjectSoulGenerationLayerBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &duplicate_intent,
        20,
    )
    .is_err());

    let cross_generation = SubjectSoulOwnedDocumentV1::new(
        &owner,
        2,
        Some(1),
        &valid_address,
        &serde_json::json!({"anchor": "wrong generation"}),
    )
    .expect("well-formed but wrong-generation document");
    let cross_generation_intent = generation_layer_intent(
        "operation:cross-generation-layer",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfModel,
            expected_previous_digest: None,
            document: Box::new(cross_generation),
        }],
    );
    assert!(plan_subject_soul_generation_layer_delta_v1(
        &owner,
        &SubjectSoulGenerationLayerBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &cross_generation_intent,
        20,
    )
    .is_err());

    let root_address = SubjectSoulManifestAddressV1 {
        namespace: "self_authored_core".to_string(),
        physical_key: "core:forged".to_string(),
    };
    let root_document = SubjectSoulOwnedDocumentV1::new(
        &owner,
        1,
        Some(1),
        &root_address,
        &serde_json::json!({"forged": true}),
    )
    .expect("well-formed envelope cannot grant namespace authority");
    let root_intent = generation_layer_intent(
        "operation:root-layer-forgery",
        vec![SubjectSoulGenerationLayerMutationV1::Upsert {
            layer: SubjectSoulGenerationLayerKindV1::SelfModel,
            expected_previous_digest: None,
            document: Box::new(root_document),
        }],
    );
    assert!(plan_subject_soul_generation_layer_delta_v1(
        &owner,
        &SubjectSoulGenerationLayerBasisV1::Verified {
            snapshot: Box::new(snapshot),
        },
        &root_intent,
        20,
    )
    .is_err());
}

#[test]
fn reset_and_delete_purge_all_terminated_generation_raw_artifacts() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let confirmation = HumanSoulLifecycleConfirmationV1 {
        human_subject_id: "human:a".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_generation: 1,
        action: SubjectSoulTerminalActionV1::Reset,
        reason_code: "user_requested_reset".to_string(),
        confirmed_at: 12,
        evidence_digest: "b".repeat(64),
    };
    let request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "operation:reset".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_state: SubjectSoulExpectedStateV1::Exact {
            generation: 1,
            revision: Some(1),
            lifecycle_state: SubjectSoulLifecycleStateV1::Active,
            head_digest: snapshot.head.head_digest.clone(),
            manifest_digest: snapshot.manifest.closure_digest.clone(),
        },
        authority: SubjectSoulLifecycleAuthorityV1::Destructive {
            system_actor_subject_id: "system:governor".to_string(),
            human_confirmation: confirmation,
        },
        action: SubjectSoulLifecycleActionV1::Reset {
            reason_code: "user_requested_reset".to_string(),
        },
    };
    let reset =
        plan_subject_soul_lifecycle_v1(&owner, &request, &snapshot, None, Some("tombstone:1"), 13)
            .expect("reset plan");
    assert!(reset.post_head.retained_revision_refs.is_empty());
    assert!(reset.post_manifest.entries.is_empty());
    assert_eq!(reset.purge_manifest_addresses.len(), 3);
    assert!(reset
        .purge_manifest_addresses
        .iter()
        .any(|address| address.physical_key == "material:1"));
    assert!(reset
        .purge_manifest_addresses
        .iter()
        .any(|address| address.physical_key == "core:1"));
    assert!(reset
        .purge_manifest_addresses
        .iter()
        .any(|address| address.physical_key == "ledger:1"));

    let mut delete_request = request;
    let SubjectSoulLifecycleAuthorityV1::Destructive {
        human_confirmation, ..
    } = &mut delete_request.authority
    else {
        unreachable!();
    };
    human_confirmation.action = SubjectSoulTerminalActionV1::Delete;
    delete_request.action = SubjectSoulLifecycleActionV1::Delete {
        reason_code: "user_requested_reset".to_string(),
    };
    let deleted = plan_subject_soul_lifecycle_v1(
        &owner,
        &delete_request,
        &snapshot,
        None,
        Some("tombstone:delete"),
        13,
    )
    .expect("delete plan");
    assert!(deleted.post_head.retained_revision_refs.is_empty());
    assert_eq!(deleted.purge_manifest_addresses.len(), 3);
    assert_eq!(
        deleted.post_head.state,
        SubjectSoulLifecycleStateV1::Deleted
    );
}

#[test]
fn reseed_purges_terminated_generation_and_only_binds_new_generation_artifacts() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "operation:reseed".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_state: SubjectSoulExpectedStateV1::Exact {
            generation: 1,
            revision: Some(1),
            lifecycle_state: SubjectSoulLifecycleStateV1::Active,
            head_digest: snapshot.head.head_digest.clone(),
            manifest_digest: snapshot.manifest.closure_digest.clone(),
        },
        authority: SubjectSoulLifecycleAuthorityV1::Destructive {
            system_actor_subject_id: "system:governor".to_string(),
            human_confirmation: HumanSoulLifecycleConfirmationV1 {
                human_subject_id: "human:a".to_string(),
                target_subject_id: owner.subject_id.clone(),
                expected_generation: 1,
                action: SubjectSoulTerminalActionV1::Reseed,
                reason_code: "user_requested_reseed".to_string(),
                confirmed_at: 12,
                evidence_digest: "b".repeat(64),
            },
        },
        action: SubjectSoulLifecycleActionV1::Reseed {
            charter: Box::new(
                SubjectSoulFoundingCharterSeedV1 {
                    identity_anchor: Some("new generation".to_string()),
                    ..SubjectSoulFoundingCharterSeedV1::default()
                }
                .canonicalize()
                .expect("canonical reseed charter"),
            ),
            reason_code: "user_requested_reseed".to_string(),
            source_asserted_at: Some(12),
        },
    };
    let new_bindings = SubjectSoulRevisionAddressBindingsV1 {
        material: SubjectSoulManifestAddressV1 {
            namespace: "soul_material".to_string(),
            physical_key: "material:2".to_string(),
        },
        core: SubjectSoulManifestAddressV1 {
            namespace: "soul_core".to_string(),
            physical_key: "core:2".to_string(),
        },
        revision_ledger: SubjectSoulManifestAddressV1 {
            namespace: "soul_ledger".to_string(),
            physical_key: "ledger:2".to_string(),
        },
    };

    let reseeded = plan_subject_soul_lifecycle_v1(
        &owner,
        &request,
        &snapshot,
        Some(&new_bindings),
        Some("tombstone:reseed:1"),
        13,
    )
    .expect("reseed plan");

    assert_eq!(reseeded.post_head.generation, 2);
    assert_eq!(reseeded.post_head.current_revision, Some(1));
    assert_eq!(
        reseeded.post_head.state,
        SubjectSoulLifecycleStateV1::Active
    );
    assert!(reseeded.post_head.retained_revision_refs.is_empty());
    assert_eq!(reseeded.purge_manifest_addresses.len(), 3);
    assert_eq!(reseeded.post_manifest.entries.len(), 3);
    assert!(reseeded
        .post_manifest
        .entries
        .iter()
        .all(|entry| entry.physical_key.ends_with(":2")));
    assert!(reseeded
        .purge_manifest_addresses
        .iter()
        .all(|address| address.physical_key.ends_with(":1")));
}

#[test]
fn operator_safe_export_requires_one_terminal_record_per_generation() {
    let terminal = SubjectSoulTerminatedGenerationV1 {
        generation: 1,
        terminal_revision: Some(1),
        terminal_material_digest: Some("a".repeat(64)),
        terminal_action: SubjectSoulTerminalActionV1::Reset,
        tombstone_digest: "b".repeat(64),
        terminated_at: 12,
        current_generation: 2,
        current_state: SubjectSoulLifecycleStateV1::Unseeded,
    };
    let mut export = SubjectSoulOperatorSafeExportV1 {
        subject_id: "agent:a".to_string(),
        soul_id: "soul:agent.a".to_string(),
        state: SubjectSoulLifecycleStateV1::Unseeded,
        generation: 2,
        revision: None,
        material_digest: None,
        origin: None,
        terminated_generations: vec![terminal.clone()],
    };
    export.validate_contract().expect("safe export");

    let mut conflicting = terminal;
    conflicting.terminal_action = SubjectSoulTerminalActionV1::Reseed;
    export.terminated_generations.push(conflicting);
    assert!(export.validate_contract().is_err());
}

#[test]
fn relationship_projection_planner_uses_typed_lattice_and_updates_soul_double_root() {
    let (_, _, snapshot) = active_founding_snapshot();
    let source = relationship_source_with_provenance(
        RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment,
        "human:a",
    );
    let source_manifest = relationship_manifest(&source, 1);
    let plan = plan_subject_soul_relationship_projection_v1(
        &snapshot,
        &source,
        &source_manifest,
        None,
        &SubjectSoulManifestAddressV1 {
            namespace: "soul_relationship_projection".to_string(),
            physical_key: "projection:relationship:1".to_string(),
        },
        RelationshipDisclosureCeilingV1::RefusalOnly,
        RelationshipDisclosureCeilingV1::GovernedSummary,
        15,
    )
    .expect("projection plan");
    let SubjectSoulRelationshipProjectionPlanV1::Upsert {
        projection,
        post_head,
        post_manifest,
    } = plan
    else {
        panic!("active exact roots must plan a projection upsert");
    };
    assert_eq!(
        projection.effective_disclosure_ceiling,
        RelationshipDisclosureCeilingV1::RefusalOnly
    );
    assert_eq!(
        post_head.scope_manifest_digest,
        post_manifest.closure_digest
    );
    assert!(post_manifest.entries.iter().any(|entry| {
        entry.owner_role == bm_core::memory::SubjectSoulManifestOwnerRoleV1::RelationshipProjection
            && entry.content_digest == projection.content_digest
    }));
}

#[test]
fn archive_purges_all_relationship_projections_while_restore_preserves_soul_material_only() {
    let (owner, _, snapshot) = active_founding_snapshot();
    let source = relationship_source_with_provenance(
        RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment,
        "human:a",
    );
    let projection_address = SubjectSoulManifestAddressV1 {
        namespace: "soul_relationship_projection".to_string(),
        physical_key: "projection:relationship:1".to_string(),
    };
    let SubjectSoulRelationshipProjectionPlanV1::Upsert {
        post_head,
        post_manifest,
        ..
    } = plan_subject_soul_relationship_projection_v1(
        &snapshot,
        &source,
        &relationship_manifest(&source, 1),
        None,
        &projection_address,
        RelationshipDisclosureCeilingV1::GovernedSummary,
        RelationshipDisclosureCeilingV1::GovernedSummary,
        15,
    )
    .expect("projection plan")
    else {
        panic!("projection upsert");
    };
    let mut projected = snapshot.clone();
    projected.head = *post_head;
    projected.manifest = *post_manifest;
    projected
        .manifest
        .entries
        .push(bm_core::memory::SubjectSoulScopeManifestEntryV1 {
            namespace: "soul_relationship_projection".to_string(),
            physical_key: "projection:relationship:2".to_string(),
            owner_role: bm_core::memory::SubjectSoulManifestOwnerRoleV1::RelationshipProjection,
            generation: projected.head.generation,
            revision: projected.head.current_revision,
            content_digest: "d".repeat(64),
        });
    projected.manifest.entries.sort();
    projected
        .manifest
        .refresh_digest()
        .expect("two-projection manifest");
    projected.head.scope_manifest_digest = projected.manifest.closure_digest.clone();
    projected.head.refresh_digest().expect("projected head");
    projected.validate_contract().expect("projected snapshot");
    let retained_soul_entries = projected
        .manifest
        .entries
        .iter()
        .filter(|entry| {
            entry.owner_role
                != bm_core::memory::SubjectSoulManifestOwnerRoleV1::RelationshipProjection
        })
        .cloned()
        .collect::<Vec<_>>();

    let archive_request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "operation:archive:projection-purge".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_state: SubjectSoulExpectedStateV1::Exact {
            generation: projected.head.generation,
            revision: projected.head.current_revision,
            lifecycle_state: projected.head.state,
            head_digest: projected.head.head_digest.clone(),
            manifest_digest: projected.manifest.closure_digest.clone(),
        },
        authority: SubjectSoulLifecycleAuthorityV1::SelfGovernance {
            capability_digest: "c".repeat(64),
        },
        action: SubjectSoulLifecycleActionV1::Archive,
    };
    let archive =
        plan_subject_soul_lifecycle_v1(&owner, &archive_request, &projected, None, None, 20)
            .expect("archive plan");
    assert_eq!(
        archive.post_head.state,
        SubjectSoulLifecycleStateV1::Archived
    );
    assert_eq!(archive.purge_manifest_addresses.len(), 2);
    assert_eq!(archive.post_manifest.entries, retained_soul_entries);
    assert_eq!(
        archive.post_head.current_revision,
        projected.head.current_revision
    );
    assert_eq!(
        archive.post_head.current_material_digest,
        projected.head.current_material_digest
    );

    let mut archived = projected;
    archived.head = (*archive.post_head).clone();
    archived.manifest = (*archive.post_manifest).clone();
    archived.validate_contract().expect("archived snapshot");
    let restore_request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "operation:restore:no-fake-projection".to_string(),
        target_subject_id: owner.subject_id.clone(),
        expected_state: SubjectSoulExpectedStateV1::Exact {
            generation: archived.head.generation,
            revision: archived.head.current_revision,
            lifecycle_state: archived.head.state,
            head_digest: archived.head.head_digest.clone(),
            manifest_digest: archived.manifest.closure_digest.clone(),
        },
        authority: SubjectSoulLifecycleAuthorityV1::SelfGovernance {
            capability_digest: "c".repeat(64),
        },
        action: SubjectSoulLifecycleActionV1::Restore,
    };
    let restore =
        plan_subject_soul_lifecycle_v1(&owner, &restore_request, &archived, None, None, 21)
            .expect("restore plan");
    assert_eq!(restore.post_head.state, SubjectSoulLifecycleStateV1::Active);
    assert!(restore.purge_manifest_addresses.is_empty());
    assert_eq!(restore.post_manifest.entries, retained_soul_entries);
    assert!(restore.post_manifest.entries.iter().all(|entry| {
        entry.owner_role != bm_core::memory::SubjectSoulManifestOwnerRoleV1::RelationshipProjection
    }));
}

#[test]
fn subject_binding_must_reject_removed_revision_refs_truth() {
    let legacy = serde_json::json!({
        "soul_id": "soul:agent",
        "owner_subject_id": "agent",
        "surfaces": [],
        "revision_refs": ["revision:1"]
    });

    assert!(
        serde_json::from_value::<SubjectSoulBinding>(legacy).is_err(),
        "revision truth belongs to the lifecycle head/material owner"
    );
}

#[test]
fn soul_lifecycle_operation_kinds_are_typed_core_contracts() {
    let provision = serde_json::json!({"plane": "soul_provision"});
    assert!(
        serde_json::from_value::<MemoryMutationOperationKind>(provision).is_ok(),
        "Soul provisioning must use the authoritative MOR operation identity"
    );
}

fn seed_with_tendencies(values: Vec<&str>) -> SubjectSoulFoundingCharterSeedV1 {
    SubjectSoulFoundingCharterSeedV1 {
        identity_anchor: None,
        character_tendencies: values.into_iter().map(str::to_string).collect(),
        priority_constitution: Vec::new(),
        non_negotiables: Vec::new(),
        default_response_mode: None,
        default_initiative_posture: None,
        default_relationship_posture: None,
        boundary_doctrine: None,
        truth_seeking_commitment: None,
        self_preservation_doctrine: None,
        repair_doctrine: None,
        change_principle: None,
    }
}

#[test]
fn founding_duplicate_clause_is_not_silently_rewritten() {
    assert!(
        seed_with_tendencies(vec!["curious", "curious"])
            .canonicalize()
            .is_err(),
        "duplicate founding clauses are an invalid caller intent"
    );
}

#[test]
fn pristine_expected_state_requires_open_closure_certificate() {
    assert!(
        serde_json::from_value::<SubjectSoulExpectedStateV1>(serde_json::json!({
            "state": "absent"
        }))
        .is_err(),
        "a naked absent precondition cannot prove pristine unseeded state"
    );
}

#[test]
fn committed_report_cannot_omit_verified_head() {
    let report = SubjectSoulMutationReportV1 {
        outcome: SubjectSoulMutationOutcomeV1::Committed,
        state_before: SubjectSoulLifecycleStateV1::Unseeded,
        state_after: SubjectSoulLifecycleStateV1::Active,
        generation: 1,
        revision: Some(1),
        head_digest: None,
        transaction_id: Some("tx:1".to_string()),
        durable_receipt_ref: Some("receipt:1".to_string()),
        replayed: false,
        safe_event_ref: Some("event:1".to_string()),
    };
    assert!(report.validate_contract().is_err());
}

#[test]
fn relationship_access_constraint_is_not_an_open_string_policy() {
    let payload = serde_json::json!({
        "schema_version": 1,
        "memory_space_id": "space:1",
        "relationship_id": "relationship:1",
        "mounted_subject_id": "agent:a",
        "counterparty_subject_ids": ["human:a"],
        "revision": 1,
        "supersedes_revision": null,
        "state": "active",
        "clauses": {
            "disclosure_ceiling": "governed_summary",
            "access_constraints": ["invented_host_role"],
            "truth_commitments": [],
            "mutual_boundary_commitments": [],
            "repair_commitments": []
        },
        "provenance": [{
            "source": "human_relationship_commitment",
            "source_subject_id": "human:a",
            "source_asserted_at": null,
            "recorded_at": 1,
            "evidence_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }],
        "content_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    });
    assert!(
        serde_json::from_value::<RelationshipSourceConstitutionV1>(payload).is_err(),
        "relationship permissions must use a closed typed lattice"
    );
}

#[test]
fn founding_seed_maps_one_to_one_without_task_or_authority_invention() {
    let mut seed = seed_with_tendencies(vec![" curious ", "careful"]);
    seed.identity_anchor = Some("  keeps faith with evidence  ".to_string());
    seed.non_negotiables = vec!["never fabricate".to_string()];
    seed.truth_seeking_commitment = Some("state uncertainty".to_string());
    let seed = seed.canonicalize().expect("canonical typed seed");
    let core = compile_subject_soul_founding_core(&seed, 42).expect("founding compiler");

    assert_eq!(core.revision, 1);
    assert_eq!(core.identity_anchor, "keeps faith with evidence");
    assert_eq!(core.character_tendencies, vec!["curious", "careful"]);
    assert_eq!(core.non_negotiables, vec!["never fabricate"]);
    assert_eq!(core.truth_doctrine, "state uncertainty");
    assert!(core.default_task_scope.is_empty());
    assert_eq!(core.stability_score, 0);
}

#[test]
fn revision_origin_cannot_impersonate_self_governance() {
    let provenance = SubjectSoulRevisionProvenanceV1 {
        origin: SubjectSoulRevisionOriginV1::HumanFoundingCharter,
        source_authority: SubjectSoulSourceAuthorityV1::SoulSelfGovernance,
        source_subject_id: "human:a".to_string(),
        source_asserted_at: Some(1),
        recorded_at: 2,
        operation_ref: Some("operation:1".to_string()),
        proposal_ref: None,
        source_refs: Vec::new(),
    };
    assert!(provenance.validate_contract().is_err());
}

#[test]
fn relationship_disclosure_lattice_is_deny_biased() {
    let lattice = RelationshipConstraintLatticeV1 {
        mental_privacy: RelationshipDisclosureCeilingV1::GovernedSummary,
        relationship_source: RelationshipDisclosureCeilingV1::FullGovernedDisclosure,
        soul_self_boundary: RelationshipDisclosureCeilingV1::RefusalOnly,
    };
    assert_eq!(
        lattice.effective_disclosure_ceiling(),
        RelationshipDisclosureCeilingV1::RefusalOnly
    );
}

#[test]
fn agent_soul_binding_requires_all_eight_surfaces_in_canonical_order() {
    let mut registry = SubjectRegistry::empty("space:1");
    let mut agent = SubjectDescriptor::new(
        "agent:a",
        SubjectKind::AgentPersona,
        "Agent A",
        SubjectVisibility::Visible,
    );
    let mut binding = SubjectSoulBinding::agent_persona("agent:a");
    binding.surfaces.reverse();
    agent.soul_binding = Some(binding);
    registry.upsert_subject(agent).expect("synthetic subject");

    let validation = registry.validate_contract();
    assert!(!validation.accepted);
    assert_eq!(validation.reason, "soul_surfaces_not_exact");
}

#[test]
fn constitutional_renderer_preserves_human_origin_and_tendencies() {
    let core = compile_subject_soul_founding_core(
        &seed_with_tendencies(vec!["curious"])
            .canonicalize()
            .expect("canonical seed"),
        2,
    )
    .expect("founding core");
    let view = SubjectSoulConstitutionalViewV1 {
        core,
        provenance: SubjectSoulRevisionProvenanceV1 {
            origin: SubjectSoulRevisionOriginV1::HumanFoundingCharter,
            source_authority: SubjectSoulSourceAuthorityV1::ActiveHumanUser,
            source_subject_id: "human:a".to_string(),
            source_asserted_at: Some(1),
            recorded_at: 2,
            operation_ref: Some("operation:1".to_string()),
            proposal_ref: None,
            source_refs: Vec::new(),
        },
        material_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_string(),
    };

    let rendered = render_subject_soul_constitutional_block(&view, 1_024)
        .expect("valid view")
        .expect("rendered founding core");
    assert!(rendered.starts_with("## Human-Sourced Founding Constitution"));
    assert!(rendered.contains("Character tendencies: curious"));
    assert!(!rendered.starts_with("## Self-Authored Core"));
}

fn relationship_source_with_provenance(
    source: RelationshipSourceAuthorityKindV1,
    source_subject_id: &str,
) -> RelationshipSourceConstitutionV1 {
    let clauses = RelationshipSourceClausesV1 {
        disclosure_ceiling: RelationshipDisclosureCeilingV1::GovernedSummary,
        access_constraints: Vec::new(),
        truth_commitments: vec!["be truthful".to_string()],
        mutual_boundary_commitments: Vec::new(),
        repair_commitments: Vec::new(),
    };
    let mut contribution = RelationshipSourceContributionV1 {
        contributor_subject_id: source_subject_id.to_string(),
        clauses: clauses.clone(),
        provenance: RelationshipSourceProvenanceV1 {
            source,
            source_subject_id: source_subject_id.to_string(),
            source_asserted_at: None,
            recorded_at: 1,
            evidence_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
        },
        contribution_digest: String::new(),
    };
    contribution.refresh_digest().expect("contribution digest");
    let mut root = RelationshipSourceConstitutionV1 {
        schema_version: 1,
        memory_space_id: "space:1".to_string(),
        relationship_id: "relationship:1".to_string(),
        mounted_subject_id: "agent:a".to_string(),
        counterparty_subject_ids: vec!["human:a".to_string()],
        revision: 1,
        supersedes_revision: None,
        state: RelationshipSourceStateV1::Active,
        clauses,
        contributions: vec![contribution],
        content_digest: String::new(),
    };
    root.refresh_digest().expect("canonical digest");
    root
}

#[test]
fn relationship_system_floor_cannot_be_authored_by_a_relationship_member() {
    let forged = relationship_source_with_provenance(
        RelationshipSourceAuthorityKindV1::SystemPolicyFloor,
        "agent:a",
    );
    assert!(
        forged.validate_contract().is_err(),
        "system floor authority cannot be inferred from an actor string"
    );
}

fn relationship_manifest(
    source: &RelationshipSourceConstitutionV1,
    manifest_revision: u64,
) -> RelationshipSourceScopeManifestV1 {
    let mut manifest = RelationshipSourceScopeManifestV1 {
        schema_version: 1,
        memory_space_id: source.memory_space_id.clone(),
        relationship_id: source.relationship_id.clone(),
        current_revision: source.revision,
        current_digest: source.content_digest.clone(),
        retained_revision_refs: Vec::new(),
        closure_digest: String::new(),
    };
    assert!(manifest_revision > 0);
    manifest.refresh_digest().expect("relationship manifest");
    manifest
}

#[test]
fn relationship_create_binds_member_authority_and_double_root() {
    let source = relationship_source_with_provenance(
        RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment,
        "human:a",
    );
    let intent = RelationshipSourceControlIntentV1 {
        operation_id: "operation:create-relationship".to_string(),
        memory_space_id: source.memory_space_id.clone(),
        relationship_id: source.relationship_id.clone(),
        mounted_subject_id: source.mounted_subject_id.clone(),
        counterparty_subject_ids: source.counterparty_subject_ids.clone(),
        expected_state: RelationshipSourceExpectedStateV1::PristineAbsent {
            closure_certificate_digest:
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        },
        authority: RelationshipSourceControlAuthorityV1::HumanUser {
            actor_subject_id: "human:a".to_string(),
        },
        action: RelationshipSourceControlIntentActionV1::Create {
            clauses: source.clauses,
            source_asserted_at: Some(7),
            evidence_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
        },
    };
    let first_digest = intent.intent_digest().expect("create intent digest");
    let mut reopened_intent = intent.clone();
    reopened_intent.expected_state = RelationshipSourceExpectedStateV1::PristineAbsent {
        closure_certificate_digest: "c".repeat(64),
    };
    assert_eq!(
        first_digest,
        reopened_intent
            .intent_digest()
            .expect("reopen create intent digest"),
        "ephemeral pristine certificates cannot destabilize durable replay identity"
    );
    let plan = plan_relationship_source_control(&intent, None, None, 11)
        .expect("Core owns the relationship post-image");
    assert!(plan.validate_contract().is_ok());
    assert_eq!(plan.action, RelationshipSourceControlActionV1::Create);
    assert_eq!(plan.post_source.revision, 1);
    assert_eq!(plan.post_source.contributions.len(), 1);
    assert_eq!(
        plan.post_manifest.current_digest,
        plan.post_source.content_digest
    );
}

#[test]
fn relationship_successor_manifest_retains_the_exact_previous_revision() {
    let previous = relationship_source_with_provenance(
        RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment,
        "human:a",
    );
    let previous_manifest = relationship_manifest(&previous, 1);
    let intent = RelationshipSourceControlIntentV1 {
        operation_id: "operation:update-relationship".to_string(),
        memory_space_id: previous.memory_space_id.clone(),
        relationship_id: previous.relationship_id.clone(),
        mounted_subject_id: previous.mounted_subject_id.clone(),
        counterparty_subject_ids: previous.counterparty_subject_ids.clone(),
        expected_state: RelationshipSourceExpectedStateV1::Exact {
            revision: previous.revision,
            state: previous.state,
            source_digest: previous.content_digest.clone(),
            manifest_digest: previous_manifest.closure_digest.clone(),
        },
        authority: RelationshipSourceControlAuthorityV1::HumanUser {
            actor_subject_id: "human:a".to_string(),
        },
        action: RelationshipSourceControlIntentActionV1::UpdateContribution {
            clauses: previous.clauses.clone(),
            source_asserted_at: Some(7),
            evidence_digest: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                .to_string(),
        },
    };

    let plan =
        plan_relationship_source_control(&intent, Some(&previous), Some(&previous_manifest), 11)
            .expect("revision two relationship plan");
    let previous_ref = canonical_relationship_source_revision_ref_v1(
        &previous.memory_space_id,
        &previous.relationship_id,
        previous.revision,
    )
    .expect("canonical previous ref");
    assert_eq!(plan.post_source.revision, 2);
    assert_eq!(
        plan.post_manifest.retained_revision_refs,
        vec![previous_ref]
    );
}

#[test]
fn relationship_human_cannot_write_another_members_contribution() {
    let source = relationship_source_with_provenance(
        RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment,
        "human:a",
    );
    let intent = RelationshipSourceControlIntentV1 {
        operation_id: "operation:forged-relationship".to_string(),
        memory_space_id: source.memory_space_id.clone(),
        relationship_id: source.relationship_id.clone(),
        mounted_subject_id: source.mounted_subject_id.clone(),
        counterparty_subject_ids: source.counterparty_subject_ids.clone(),
        expected_state: RelationshipSourceExpectedStateV1::PristineAbsent {
            closure_certificate_digest:
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        },
        authority: RelationshipSourceControlAuthorityV1::HumanUser {
            actor_subject_id: "human:b".to_string(),
        },
        action: RelationshipSourceControlIntentActionV1::Create {
            clauses: source.clauses,
            source_asserted_at: None,
            evidence_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
        },
    };
    assert!(plan_relationship_source_control(&intent, None, None, 11).is_err());
}

#[test]
fn mounted_agent_cannot_loosen_its_existing_self_boundary() {
    let previous = relationship_source_with_provenance(
        RelationshipSourceAuthorityKindV1::SubjectSelfBoundary,
        "agent:a",
    );
    let previous_manifest = relationship_manifest(&previous, 1);
    let mut looser_clauses = previous.clauses.clone();
    looser_clauses.disclosure_ceiling = RelationshipDisclosureCeilingV1::FullGovernedDisclosure;
    let intent = RelationshipSourceControlIntentV1 {
        operation_id: "operation:loosen-agent-boundary".to_string(),
        memory_space_id: previous.memory_space_id.clone(),
        relationship_id: previous.relationship_id.clone(),
        mounted_subject_id: previous.mounted_subject_id.clone(),
        counterparty_subject_ids: previous.counterparty_subject_ids.clone(),
        expected_state: RelationshipSourceExpectedStateV1::Exact {
            revision: previous.revision,
            state: previous.state,
            source_digest: previous.content_digest.clone(),
            manifest_digest: previous_manifest.closure_digest.clone(),
        },
        authority: RelationshipSourceControlAuthorityV1::MountedAgentPersona {
            actor_subject_id: "agent:a".to_string(),
            self_governance_capability_digest:
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string(),
        },
        action: RelationshipSourceControlIntentActionV1::UpdateContribution {
            clauses: looser_clauses,
            source_asserted_at: None,
            evidence_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
        },
    };
    assert!(plan_relationship_source_control(
        &intent,
        Some(&previous),
        Some(&previous_manifest),
        11,
    )
    .is_err());
}

#[test]
fn legal_implicit_unseeded_needs_a_typed_read_outcome_without_forged_roots() {
    let payload = serde_json::json!({
        "outcome": "implicit_unseeded",
        "memory_space_id": "space:1",
        "subject_id": "agent:a",
        "soul_id": "soul:agent.a",
        "generation": 1,
        "closure_certificate_digest": "a".repeat(64)
    });
    assert!(
        serde_json::from_value::<bm_core::memory::SubjectSoulReadOutcomeV1>(payload).is_ok(),
        "implicit unseeded must not forge head or manifest digests"
    );
}

#[test]
fn relationship_runtime_view_uses_source_truth_and_recompiles_missing_or_stale_projection() {
    let (_, _, snapshot) = active_founding_snapshot();
    let material = snapshot
        .current_material
        .as_ref()
        .expect("active material")
        .clone();
    let source = relationship_source_with_provenance(
        RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment,
        "human:a",
    );
    let source_only = compile_subject_soul_relationship_runtime_view_v1(
        "agent:a",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source: source.clone(),
            current_material: None,
            stored_projection: None,
        },
        None,
    )
    .expect("active source remains enforceable for an unseeded Soul");
    assert_eq!(
        source_only.projection_disposition,
        SubjectSoulRelationshipRuntimeProjectionDispositionV1::SourceOnlyUnseeded
    );
    assert_eq!(
        source_only.effective_disclosure_ceiling,
        RelationshipDisclosureCeilingV1::RefusalOnly,
        "absent MentalPrivacy and Soul-boundary state must fail closed"
    );
    assert_eq!(source_only.truth_commitments, vec!["be truthful"]);

    let safety_baseline = mental_privacy_safety_baseline(10);
    let baseline_clamped = compile_subject_soul_relationship_runtime_view_v1(
        "agent:a",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source: source.clone(),
            current_material: None,
            stored_projection: None,
        },
        Some(&safety_baseline),
    )
    .expect("privacy safety baseline compiles against active source");
    assert_eq!(
        baseline_clamped.effective_disclosure_ceiling,
        RelationshipDisclosureCeilingV1::RefusalOnly,
        "a safety baseline cannot be widened by a more permissive Relationship Source"
    );
    let mut restrictive_source = source.clone();
    restrictive_source.clauses.disclosure_ceiling = RelationshipDisclosureCeilingV1::None;
    for contribution in &mut restrictive_source.contributions {
        contribution.clauses.disclosure_ceiling = RelationshipDisclosureCeilingV1::None;
        contribution
            .refresh_digest()
            .expect("restrictive contribution digest");
    }
    restrictive_source
        .refresh_digest()
        .expect("restrictive source digest");
    let source_clamped = compile_subject_soul_relationship_runtime_view_v1(
        "agent:a",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source: restrictive_source,
            current_material: None,
            stored_projection: None,
        },
        Some(&safety_baseline),
    )
    .expect("restrictive source clamps safety baseline");
    assert_eq!(
        source_clamped.effective_disclosure_ceiling,
        RelationshipDisclosureCeilingV1::None,
        "Relationship Source may tighten but never be loosened by the safety baseline"
    );

    let missing = compile_subject_soul_relationship_runtime_view_v1(
        "agent:a",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source: source.clone(),
            current_material: Some(material.clone()),
            stored_projection: None,
        },
        None,
    )
    .expect("missing projection recompiles from roots");
    assert_eq!(
        missing.projection_disposition,
        SubjectSoulRelationshipRuntimeProjectionDispositionV1::RecompiledMissingProjection
    );

    let mental_privacy_ceiling = relationship_mental_privacy_ceiling_v1(None);
    let soul_self_boundary_ceiling = relationship_soul_self_boundary_ceiling_v1(None);
    let mut projection = SubjectSoulRelationshipProjectionV1 {
        schema_version: 1,
        memory_space_id: material.memory_space_id.clone(),
        subject_id: material.subject_id.clone(),
        soul_id: material.soul_id.clone(),
        relationship_id: source.relationship_id.clone(),
        generation: material.generation,
        soul_revision: material.revision,
        soul_material_digest: material.content_digest.clone(),
        relationship_source_revision: source.revision,
        relationship_source_digest: source.content_digest.clone(),
        mental_privacy_ceiling,
        soul_self_boundary_ceiling,
        effective_disclosure_ceiling: mental_privacy_ceiling
            .most_restrictive(source.clauses.disclosure_ceiling)
            .most_restrictive(soul_self_boundary_ceiling),
        inherited_postures: source.clauses.mutual_boundary_commitments.clone(),
        response_commitments: source
            .clauses
            .truth_commitments
            .iter()
            .chain(source.clauses.repair_commitments.iter())
            .cloned()
            .collect(),
        content_digest: String::new(),
    };
    projection.refresh_digest().expect("projection digest");
    let current = compile_subject_soul_relationship_runtime_view_v1(
        "agent:a",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source: source.clone(),
            current_material: Some(material.clone()),
            stored_projection: Some(projection.clone()),
        },
        None,
    )
    .expect("current projection");
    assert_eq!(
        current.projection_disposition,
        SubjectSoulRelationshipRuntimeProjectionDispositionV1::CurrentProjection
    );

    let current_projection = projection.clone();
    projection.relationship_source_revision += 1;
    projection
        .refresh_digest()
        .expect("stale projection digest");
    let stale = compile_subject_soul_relationship_runtime_view_v1(
        "agent:a",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source: source.clone(),
            current_material: Some(material),
            stored_projection: Some(projection),
        },
        None,
    )
    .expect("stale projection is ignored in favor of exact roots");
    assert_eq!(
        stale.projection_disposition,
        SubjectSoulRelationshipRuntimeProjectionDispositionV1::RecompiledStaleProjection
    );
    assert_eq!(missing.truth_commitments, stale.truth_commitments);
    assert_eq!(missing.response_commitments, stale.response_commitments);

    let mut inactive_source = source.clone();
    inactive_source.state = RelationshipSourceStateV1::Archived;
    inactive_source
        .refresh_digest()
        .expect("archived source digest");
    let inactive = compile_subject_soul_relationship_runtime_view_v1(
        "agent:a",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source: inactive_source,
            current_material: None,
            stored_projection: None,
        },
        None,
    )
    .expect("inactive source has a typed closed view");
    assert_eq!(
        inactive.projection_disposition,
        SubjectSoulRelationshipRuntimeProjectionDispositionV1::InactiveSource
    );
    assert_eq!(
        inactive.effective_disclosure_ceiling,
        RelationshipDisclosureCeilingV1::None
    );
    assert!(inactive.access_constraints.is_empty());
    assert!(inactive.truth_commitments.is_empty());
    assert!(inactive.mutual_boundary_commitments.is_empty());
    assert!(inactive.repair_commitments.is_empty());
    assert!(inactive.inherited_postures.is_empty());
    assert!(inactive.response_commitments.is_empty());

    let mut wrong_owner_projection = current_projection;
    wrong_owner_projection.subject_id = "agent:b".to_string();
    wrong_owner_projection
        .refresh_digest()
        .expect("wrong owner projection remains self-contained");
    assert!(compile_subject_soul_relationship_runtime_view_v1(
        "agent:a",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source: source.clone(),
            current_material: snapshot.current_material.clone(),
            stored_projection: Some(wrong_owner_projection),
        },
        None,
    )
    .is_err());
    assert!(compile_subject_soul_relationship_runtime_view_v1(
        "agent:b",
        &SubjectSoulRelationshipRuntimeInputV1 {
            source,
            current_material: None,
            stored_projection: None,
        },
        None,
    )
    .is_err());
}
