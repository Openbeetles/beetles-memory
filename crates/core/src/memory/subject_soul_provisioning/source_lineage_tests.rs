use super::*;

fn owner() -> SubjectSoulOwnerV1 {
    SubjectSoulOwnerV1 {
        memory_space_id: "space:a".into(),
        subject_id: "agent:a".into(),
        soul_id: "soul:a".into(),
    }
}

fn source(index: usize) -> SubjectSoulSourceRefV1 {
    SubjectSoulSourceRefV1 {
        memory_space_id: "space:a".into(),
        subject_id: "agent:a".into(),
        channel_id: "chat".into(),
        conversation_id: "conversation:a".into(),
        turn_id: format!("turn:{index}"),
        sequence: index as u64 + 1,
        canonical_turn_digest: format!("sha256:{}", "1".repeat(64)),
        transcript_content_digest: format!("sha256:{}", "2".repeat(64)),
    }
}

#[test]
fn lineage_union_is_exact_bounded_and_subject_scoped() {
    let a = source(0);
    let b = source(1);
    let union = merge_subject_soul_source_lineage_v1(
        &owner(),
        [&[b.clone(), a.clone()][..], &[a.clone()][..]],
    )
    .unwrap();
    assert_eq!(union, vec![a, b]);
    let too_many = (0..=SUBJECT_SOUL_MAX_SOURCE_LINEAGE)
        .map(source)
        .collect::<Vec<_>>();
    assert_eq!(
        merge_subject_soul_source_lineage_v1(&owner(), [&too_many[..]])
            .unwrap_err()
            .key,
        SubjectSoulLifecycleErrorKey::CapacityExceeded
    );
    let mut foreign = source(2);
    foreign.subject_id = "agent:b".into();
    assert!(merge_subject_soul_source_lineage_v1(&owner(), [&[foreign][..]]).is_err());
}

#[test]
fn owned_document_lineage_is_digest_bound_and_cannot_be_dropped_on_update() {
    let address = SubjectSoulManifestAddressV1 {
        namespace: "inner_life".into(),
        physical_key: "inner:a".into(),
    };
    let plain = SubjectSoulOwnedDocumentV1::new(
        &owner(),
        1,
        None,
        &address,
        &serde_json::json!({"private":"synthetic"}),
    )
    .unwrap();
    let bound = plain.clone().with_source_lineage(vec![source(0)]).unwrap();
    assert_ne!(plain.content_digest, bound.content_digest);
    let mut forged = bound.clone();
    forged.source_lineage.clear();
    assert!(forged.validate_contract().is_err());
    assert!(bound
        .source_lineage
        .iter()
        .all(|item| item.validate_for_owner(&owner()).is_ok()));
}

fn active_snapshot() -> SubjectSoulVerifiedSnapshotV1 {
    let address = |namespace: &str| SubjectSoulManifestAddressV1 {
        namespace: namespace.into(),
        physical_key: if namespace == "subject_soul_revision_material" {
            "material:a".into()
        } else {
            "scope:subject".into()
        },
    };
    let plan = plan_subject_soul_provision_v1(
        &owner(),
        &SubjectSoulProvisionIntentV1::Founding {
            operation_id: "founding:a".into(),
            human_actor_subject_id: "human:a".into(),
            charter: Box::new(
                SubjectSoulFoundingCharterSeedV1 {
                    identity_anchor: Some("curious and careful".into()),
                    non_negotiables: vec!["Respect explicit consent.".into()],
                    boundary_doctrine: Some("Do not widen another subject's boundaries.".into()),
                    ..Default::default()
                }
                .canonicalize()
                .unwrap(),
            ),
            source_asserted_at: Some(1),
        },
        &SubjectSoulExpectedStateV1::PristineAbsent {
            closure_certificate_digest: "a".repeat(64),
        },
        Some(&SubjectSoulRevisionAddressBindingsV1 {
            material: address("subject_soul_revision_material"),
            core: address("self_authored_core"),
            revision_ledger: address("core_revision_ledger"),
        }),
        2,
    )
    .unwrap();
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
        panic!("expected founding");
    };
    SubjectSoulVerifiedSnapshotV1 {
        head: *head,
        manifest: *manifest,
        current_material: Some(*material),
        current_core: Some(*core),
        current_core_document: Some(*core_document),
        current_revision_ledger: Some(*revision_ledger),
        current_revision_ledger_document: Some(*revision_ledger_document),
    }
}

fn add_source_layers(mut snapshot: SubjectSoulVerifiedSnapshotV1) -> SubjectSoulVerifiedSnapshotV1 {
    let mutations = [
        SubjectSoulGenerationLayerKindV1::TurnPersonaEvidence,
        SubjectSoulGenerationLayerKindV1::PrivateGarden,
        SubjectSoulGenerationLayerKindV1::MentalPrivacy,
    ]
    .into_iter()
    .map(|layer| {
        let doc = SubjectSoulOwnedDocumentV1::new(
            &owner(),
            snapshot.head.generation,
            snapshot.head.current_revision,
            &SubjectSoulManifestAddressV1 {
                namespace: layer.canonical_namespace().into(),
                physical_key: format!("{}:a", layer.canonical_namespace()),
            },
            &serde_json::json!({"private":"synthetic source quote"}),
        )
        .unwrap()
        .with_source_lineage(vec![source(0)])
        .unwrap();
        SubjectSoulGenerationLayerMutationV1::Upsert {
            layer,
            expected_previous_digest: None,
            document: Box::new(doc),
        }
    })
    .collect();
    let plan = plan_subject_soul_generation_layer_delta_v1(
        &owner(),
        &SubjectSoulGenerationLayerBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &SubjectSoulGenerationLayerIntentV1 {
            operation_id: "layer:a".into(),
            authority: SubjectSoulGenerationLayerAuthorityV1::MountedAgentPersona {
                actor_subject_id: owner().subject_id,
            },
            mutations,
        },
        3,
    )
    .unwrap();
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_head,
        post_manifest,
        ..
    } = plan
    else {
        panic!("expected layers");
    };
    snapshot.head = *post_head;
    snapshot.manifest = *post_manifest;
    snapshot
}

#[test]
fn immutable_soul_revision_requires_typed_source_lineage_on_wire() {
    let snapshot = active_snapshot();
    let value = serde_json::to_value(snapshot.current_material.unwrap()).unwrap();
    assert_eq!(
        value.get("source_lineage"),
        Some(&serde_json::json!([])),
        "founding material must explicitly distinguish empty lineage from a missing contract"
    );
}

#[test]
fn revision_only_source_withdrawal_is_not_mistaken_for_no_effect() {
    let founding = active_snapshot();
    let snapshot = adopt_snapshot(&founding, vec![source(0)], None, 3);
    let retained = retained_material(&founding);
    let delta = plan_subject_soul_source_withdrawal_v1(
        &owner(),
        &snapshot,
        &[retained],
        &SubjectSoulSourceWithdrawalIntentV1 {
            operation_id: "withdraw:revision-only".into(),
            actor_subject_id: "human:a".into(),
            withdrawn_sources: vec![source(0)],
        },
        4,
    )
    .unwrap();
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_manifest,
        delete_addresses,
        ..
    } = delta
    else {
        panic!("retained Soul revision dependencies need durable denial even without a private layer to delete");
    };
    assert!(delete_addresses.is_empty());
    assert!(post_manifest.source_review.requires_review());
    assert!(post_manifest
        .source_review
        .denies_revision(snapshot.current_material.as_ref().unwrap()));
}

fn retained_material(snapshot: &SubjectSoulVerifiedSnapshotV1) -> SubjectSoulRetainedMaterialV1 {
    let material = snapshot.current_material.as_ref().unwrap();
    let entry = snapshot
        .manifest
        .entries
        .iter()
        .find(|entry| entry.content_digest == material.content_digest)
        .unwrap();
    SubjectSoulRetainedMaterialV1 {
        physical_key: entry.physical_key.clone(),
        material: material.clone(),
    }
}

fn correction_plan(
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    time: u64,
    correction: bool,
) -> SelfAuthoredCoreRefreshPlanV1 {
    let current = snapshot.current_core.as_ref().unwrap();
    let ledger = snapshot.current_revision_ledger.as_ref().unwrap();
    let mut next = current.clone();
    next.revision += 1;
    next.supersedes_revision = Some(current.revision);
    next.identity_anchor = "Stable lawful observations support careful curiosity.".into();
    next.updated_at = time;
    next.last_reviewed_at = time;
    let decision = CoreRevisionRecord {
        based_on_revision: current.revision,
        resulting_revision: next.revision,
        source_layers: vec!["turn_persona_evidence".into()],
        outcome: CoreRevisionOutcome::Adopted,
        corrects_revision: correction.then_some(current.revision),
        correction_kind: correction
            .then_some(crate::memory::CoreRevisionCorrectionKind::Correction),
        evidence_summary: vec!["lawful stable observations".into()],
        reviewed_at: time,
        ..CoreRevisionRecord::default()
    };
    SelfAuthoredCoreRefreshPlanV1::Adopt {
        expected_prior: compute_self_authored_core_expected_prior_v1(Some(current), ledger)
            .unwrap(),
        next_core: Box::new(next),
        next_ledger: append_core_revision_record(ledger.clone(), decision),
        origin: SubjectSoulRevisionOriginV1::SelfGovernedRevision,
        proposal_ref: format!("proposal:{time}"),
        source_refs: vec!["turn_persona_evidence".into()],
    }
}

fn adopt_snapshot(
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    lineage: Vec<SubjectSoulSourceRefV1>,
    review: Option<SubjectSoulSourceReviewBasisV1>,
    time: u64,
) -> SubjectSoulVerifiedSnapshotV1 {
    let refresh = correction_plan(snapshot, time, review.is_some());
    let next_revision = snapshot.head.current_revision.unwrap() + 1;
    let address = |namespace: &str| SubjectSoulManifestAddressV1 {
        namespace: namespace.into(),
        physical_key: if namespace == "subject_soul_revision_material" {
            format!("material:r{next_revision}")
        } else {
            format!("scope:r{next_revision}")
        },
    };
    let plan = plan_subject_soul_self_authored_revision_v1(
        &owner(),
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &refresh,
        &SubjectSoulRevisionEvidenceV1 {
            lawful_founding: review
                .as_ref()
                .map(|_| Box::new(active_snapshot().current_material.unwrap())),
            source_lineage: lineage,
            source_review: review,
        },
        Some(&SubjectSoulSelfAuthoredPostImageAddressesV1::Adopt {
            revision: Box::new(SubjectSoulRevisionAddressBindingsV1 {
                material: address("subject_soul_revision_material"),
                core: address("self_authored_core"),
                revision_ledger: address("core_revision_ledger"),
            }),
        }),
        time,
    )
    .unwrap();
    let SubjectSoulSelfAuthoredCommitPlanV1::Adopt {
        post_head,
        post_manifest,
        material,
        core,
        core_document,
        revision_ledger,
        revision_ledger_document,
        ..
    } = plan
    else {
        panic!("adoption");
    };
    SubjectSoulVerifiedSnapshotV1 {
        head: *post_head,
        manifest: *post_manifest,
        current_material: Some(*material),
        current_core: Some(*core),
        current_core_document: Some(*core_document),
        current_revision_ledger: Some(*revision_ledger),
        current_revision_ledger_document: Some(*revision_ledger_document),
    }
}

#[test]
fn withdrawal_erases_raw_dependencies_on_active_and_archived_soul_without_rewriting_history() {
    for archived in [false, true] {
        let mut snapshot = add_source_layers(active_snapshot());
        if archived {
            snapshot.head.state = SubjectSoulLifecycleStateV1::Archived;
            snapshot.head.refresh_digest().unwrap();
        }
        snapshot.validate_contract().unwrap();
        let intent = SubjectSoulSourceWithdrawalIntentV1 {
            operation_id: "erase:a".into(),
            actor_subject_id: "human:a".into(),
            withdrawn_sources: vec![source(0)],
        };
        let delta =
            plan_subject_soul_source_withdrawal_v1(&owner(), &snapshot, &[], &intent, 4).unwrap();
        let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
            post_head,
            post_manifest,
            delete_addresses,
            ..
        } = delta
        else {
            panic!("expected erasure");
        };
        assert!(delete_addresses
            .iter()
            .any(|address| address.namespace == "private_garden"));
        assert!(delete_addresses
            .iter()
            .any(|address| address.namespace == "mental_privacy"));
        assert!(delete_addresses
            .iter()
            .any(|address| address.namespace == "soul_turn_persona_evidence"));
        assert!(post_manifest.source_review.requires_review());
        assert_eq!(post_head.current_revision, snapshot.head.current_revision);
        assert_eq!(
            post_head.current_material_digest,
            snapshot.head.current_material_digest
        );
        assert_eq!(
            post_head.current_ledger_digest,
            snapshot.head.current_ledger_digest
        );
        assert_eq!(post_head.state, snapshot.head.state);
        let mut held = snapshot.clone();
        held.head = *post_head;
        held.manifest = *post_manifest;
        held.validate_contract().unwrap();
        let no_change =
            plan_subject_soul_source_withdrawal_v1(&owner(), &held, &[], &intent, 5).unwrap();
        assert!(matches!(
            no_change,
            SubjectSoulGenerationLayerDeltaPlanV1::NoEffect { .. }
        ));
        let denied = plan_subject_soul_generation_layer_delta_v1(
            &owner(),
            &SubjectSoulGenerationLayerBasisV1::Verified {
                snapshot: Box::new(held),
            },
            &SubjectSoulGenerationLayerIntentV1 {
                operation_id: "unrelated:a".into(),
                authority: SubjectSoulGenerationLayerAuthorityV1::MountedAgentPersona {
                    actor_subject_id: owner().subject_id,
                },
                mutations: vec![SubjectSoulGenerationLayerMutationV1::Delete {
                    layer: SubjectSoulGenerationLayerKindV1::PrivateGarden,
                    address: SubjectSoulManifestAddressV1 {
                        namespace: "private_garden".into(),
                        physical_key: "private_garden:a".into(),
                    },
                    expected_content_digest: "a".repeat(64),
                }],
            },
            5,
        )
        .unwrap_err();
        assert_eq!(
            denied.key,
            SubjectSoulLifecycleErrorKey::SourceReviewRequired
        );
    }
}

#[test]
fn reset_clears_review_hold_only_with_new_generation_and_destructive_authority() {
    let mut snapshot = add_source_layers(active_snapshot());
    let delta = plan_subject_soul_source_withdrawal_v1(
        &owner(),
        &snapshot,
        &[],
        &SubjectSoulSourceWithdrawalIntentV1 {
            operation_id: "erase:reset-basis".into(),
            actor_subject_id: "human:a".into(),
            withdrawn_sources: vec![source(0)],
        },
        4,
    )
    .unwrap();
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_head,
        post_manifest,
        ..
    } = delta
    else {
        panic!("withdrawal");
    };
    snapshot.head = *post_head;
    snapshot.manifest = *post_manifest;
    let expected = SubjectSoulExpectedStateV1::Exact {
        generation: snapshot.head.generation,
        revision: snapshot.head.current_revision,
        lifecycle_state: snapshot.head.state,
        head_digest: snapshot.head.head_digest.clone(),
        manifest_digest: snapshot.manifest.closure_digest.clone(),
    };
    let request = SubjectSoulLifecycleMutationRequestV1 {
        operation_id: "reset:a".into(),
        target_subject_id: owner().subject_id.clone(),
        expected_state: expected,
        authority: SubjectSoulLifecycleAuthorityV1::Destructive {
            system_actor_subject_id: "system:a".into(),
            human_confirmation: HumanSoulLifecycleConfirmationV1 {
                human_subject_id: "human:a".into(),
                target_subject_id: owner().subject_id,
                expected_generation: 1,
                action: SubjectSoulTerminalActionV1::Reset,
                reason_code: "source_review_reset".into(),
                confirmed_at: 4,
                evidence_digest: "a".repeat(64),
            },
        },
        action: SubjectSoulLifecycleActionV1::Reset {
            reason_code: "source_review_reset".into(),
        },
    };
    let plan =
        plan_subject_soul_lifecycle_v1(&owner(), &request, &snapshot, None, Some("tombstone:a"), 5)
            .unwrap();
    assert_eq!(plan.post_head.generation, 2);
    assert!(!plan.post_manifest.source_review.requires_review());
    assert!(plan.post_manifest.entries.is_empty());
    assert!(plan.tombstone.is_some());
    assert_eq!(snapshot.head.current_revision, Some(1));
}

#[test]
fn explicitly_unseeded_source_erasure_stays_unseeded_and_does_not_create_personality() {
    let document = SubjectSoulOwnedDocumentV1::new(
        &owner(),
        1,
        None,
        &SubjectSoulManifestAddressV1 {
            namespace: "private_garden".into(),
            physical_key: "garden:unseeded".into(),
        },
        &serde_json::json!({"private":"synthetic unseeded experience"}),
    )
    .unwrap()
    .with_source_lineage(vec![source(0)])
    .unwrap();
    let delta = plan_subject_soul_generation_layer_delta_v1(
        &owner(),
        &SubjectSoulGenerationLayerBasisV1::ImplicitUnseeded {
            closure_certificate_digest: "a".repeat(64),
        },
        &SubjectSoulGenerationLayerIntentV1 {
            operation_id: "unseeded:source".into(),
            authority: SubjectSoulGenerationLayerAuthorityV1::MountedAgentPersona {
                actor_subject_id: owner().subject_id,
            },
            mutations: vec![SubjectSoulGenerationLayerMutationV1::Upsert {
                layer: SubjectSoulGenerationLayerKindV1::PrivateGarden,
                expected_previous_digest: None,
                document: Box::new(document),
            }],
        },
        2,
    )
    .unwrap();
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_head,
        post_manifest,
        ..
    } = delta
    else {
        panic!("source layer");
    };
    let snapshot = SubjectSoulVerifiedSnapshotV1 {
        head: *post_head,
        manifest: *post_manifest,
        current_material: None,
        current_core: None,
        current_core_document: None,
        current_revision_ledger: None,
        current_revision_ledger_document: None,
    };
    let delta = plan_subject_soul_source_withdrawal_v1(
        &owner(),
        &snapshot,
        &[],
        &SubjectSoulSourceWithdrawalIntentV1 {
            operation_id: "unseeded:erase".into(),
            actor_subject_id: "human:a".into(),
            withdrawn_sources: vec![source(0)],
        },
        3,
    )
    .unwrap();
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_head,
        post_manifest,
        delete_addresses,
        ..
    } = delta
    else {
        panic!("source erasure");
    };
    assert_eq!(post_head.state, SubjectSoulLifecycleStateV1::Unseeded);
    assert_eq!(post_head.current_revision, None);
    assert!(post_manifest.entries.is_empty());
    assert!(post_manifest.source_review.requires_review());
    assert!(delete_addresses
        .iter()
        .any(|address| address.physical_key == "garden:unseeded"));
}

#[test]
fn transcript_source_identity_survives_archive_but_not_erasure_or_body_correction() {
    use crate::memory::{
        CanonicalTurnDelta, ConversationKey, ConversationScope, IngressKind,
        MemoryTurnDeliveryStatus, MemoryTurnProtocol, MemoryTurnSource, TranscriptInputMessage,
        TranscriptLifecycleTransition, TranscriptTurnRecord,
    };
    let delta = CanonicalTurnDelta {
        turn_id: "turn:source".into(),
        conversation: ConversationScope {
            channel: "chat".into(),
            chat_id: "chat:a".into(),
            conversation_id: Some("conversation:a".into()),
        },
        subject: owner().subject_id,
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: IngressKind::User,
            channel: "chat".into(),
            provider: None,
            protocol: MemoryTurnProtocol::Native,
            endpoint: None,
            model_alias: None,
            model_resolved: None,
            request_id: None,
            client_conversation_hint: None,
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user("synthetic private source")],
        assistant_messages: vec![crate::memory::TranscriptAssistantMessage::complete(
            TranscriptInputMessage::assistant("synthetic reply"),
        )],
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    };
    let record = TranscriptTurnRecord::from_delta(
        &ConversationKey::from_delta(&owner().memory_space_id, &delta).unwrap(),
        1,
        &delta,
        Vec::new(),
        2,
    )
    .unwrap();
    let source = SubjectSoulSourceRefV1::from_transcript(&record).unwrap();
    assert!(source.matches_transcript(&record));
    let mut archived = record.clone();
    archived.apply_lifecycle_transition(TranscriptLifecycleTransition::Archive, 3);
    assert!(source.matches_transcript(&archived));
    archived.apply_lifecycle_transition(TranscriptLifecycleTransition::Restore, 4);
    assert!(source.matches_transcript(&archived));
    let mut corrected = record.clone();
    corrected.canonical_turn_digest = format!("sha256:{}", "f".repeat(64));
    assert!(!source.matches_transcript(&corrected));
    for transition in [
        TranscriptLifecycleTransition::Mask,
        TranscriptLifecycleTransition::DeleteRaw,
    ] {
        let mut withdrawn = record.clone();
        withdrawn.apply_lifecycle_transition(transition, 5);
        assert!(!source.matches_transcript(&withdrawn));
    }
}

fn held_revision() -> (
    SubjectSoulVerifiedSnapshotV1,
    SubjectSoulVerifiedSnapshotV1,
    SubjectSoulRetainedMaterialV1,
) {
    let founding = active_snapshot();
    let grown = adopt_snapshot(&founding, vec![source(0), source(1)], None, 3);
    let retained = retained_material(&founding);
    let delta = plan_subject_soul_source_withdrawal_v1(
        &owner(),
        &grown,
        std::slice::from_ref(&retained),
        &SubjectSoulSourceWithdrawalIntentV1 {
            operation_id: "withdraw:review".into(),
            actor_subject_id: "human:a".into(),
            withdrawn_sources: vec![source(0)],
        },
        4,
    )
    .unwrap();
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_head,
        post_manifest,
        ..
    } = delta
    else {
        panic!("withdrawal");
    };
    let mut held = grown.clone();
    held.head = *post_head;
    held.manifest = *post_manifest;
    held.validate_contract().unwrap();
    (held, grown, retained)
}

#[test]
fn correction_recovers_same_generation_without_reauthorizing_history() {
    let (held, grown, founding) = held_revision();
    let basis = held.manifest.source_review.pending_basis().unwrap();
    let evidence = SubjectSoulRevisionEvidenceV1 {
        lawful_founding: Some(Box::new(founding.material.clone())),
        source_lineage: vec![source(2)],
        source_review: Some(basis.clone()),
    };
    let skipped = plan_subject_soul_self_authored_revision_v1(
        &owner(),
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(held.clone()),
        },
        &SelfAuthoredCoreRefreshPlanV1::Skipped,
        &evidence,
        None,
        5,
    )
    .unwrap();
    assert!(matches!(
        skipped,
        SubjectSoulSelfAuthoredCommitPlanV1::NoEffect
    ));
    assert!(held.manifest.source_review.requires_review());
    let restored = adopt_snapshot(&held, vec![source(2)], Some(basis), 6);
    assert_eq!(restored.head.generation, held.head.generation);
    assert_eq!(restored.head.current_revision, Some(3));
    assert!(!restored.manifest.source_review.requires_review());
    assert_eq!(
        restored.manifest.source_review.denied_revisions,
        held.manifest.source_review.denied_revisions
    );
    assert_eq!(
        restored.current_material.as_ref().unwrap().source_lineage,
        vec![source(2)]
    );
    assert!(restored
        .current_material
        .as_ref()
        .unwrap()
        .source_review_basis
        .is_some());
    assert_eq!(
        restored
            .manifest
            .source_review
            .validate_material_read(grown.current_material.as_ref().unwrap())
            .unwrap_err()
            .key,
        SubjectSoulLifecycleErrorKey::RevisionSourceWithdrawn
    );
    restored
        .manifest
        .source_review
        .validate_material_read(restored.current_material.as_ref().unwrap())
        .unwrap();
    let mut retained = vec![founding, retained_material(&grown)];
    retained.sort_by(|a, b| a.physical_key.cmp(&b.physical_key));
    let delta = plan_subject_soul_source_withdrawal_v1(
        &owner(),
        &restored,
        &retained,
        &SubjectSoulSourceWithdrawalIntentV1 {
            operation_id: "withdraw:history-only".into(),
            actor_subject_id: "human:a".into(),
            withdrawn_sources: vec![source(1)],
        },
        7,
    )
    .unwrap();
    let SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        post_manifest,
        delete_addresses,
        ..
    } = delta
    else {
        panic!("historical dependency still needs denial");
    };
    assert!(delete_addresses.is_empty());
    assert_eq!(post_manifest.source_review.epoch, 2);
    assert!(post_manifest
        .source_review
        .withdrawn_sources
        .contains(&source(1)));
}

#[test]
fn stale_review_and_withdrawn_or_empty_lineage_cannot_release_pending() {
    let (held, _, founding) = held_revision();
    for mutation in 0..4 {
        let mut evidence = SubjectSoulRevisionEvidenceV1 {
            lawful_founding: Some(Box::new(founding.material.clone())),
            source_lineage: vec![source(2)],
            source_review: held.manifest.source_review.pending_basis(),
        };
        match mutation {
            0 => evidence.source_review.as_mut().unwrap().epoch += 1,
            1 => evidence.source_lineage = vec![source(0)],
            2 => evidence.source_lineage.clear(),
            _ => evidence.lawful_founding = None,
        }
        let refresh = correction_plan(&held, 5, true);
        assert!(plan_subject_soul_self_authored_revision_v1(
            &owner(),
            &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
                snapshot: Box::new(held.clone())
            },
            &refresh,
            &evidence,
            None,
            5
        )
        .is_err());
    }
}

#[test]
fn review_rebase_keeps_foundation_without_reading_the_withdrawn_core_into_model() {
    let (held, _, founding) = held_revision();
    let evidence = SubjectSoulRevisionEvidenceV1 {
        lawful_founding: Some(Box::new(founding.material.clone())),
        source_lineage: vec![source(2)],
        source_review: held.manifest.source_review.pending_basis(),
    };
    let ledger = CoreRevisionLedger::default();
    let proposal = SelfAuthoredCoreRefreshPlanV1::Adopt {
        expected_prior: compute_self_authored_core_expected_prior_v1(None, &ledger).unwrap(),
        next_core: Box::new(SelfAuthoredCore {
            revision: 1,
            identity_anchor: "Lawful stable observations.".into(),
            updated_at: 6,
            last_reviewed_at: 6,
            ..SelfAuthoredCore::default()
        }),
        next_ledger: append_core_revision_record(
            ledger,
            CoreRevisionRecord {
                based_on_revision: 0,
                resulting_revision: 1,
                outcome: CoreRevisionOutcome::Adopted,
                reviewed_at: 6,
                ..CoreRevisionRecord::default()
            },
        ),
        origin: SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap,
        proposal_ref: "review:clean-bootstrap".into(),
        source_refs: vec!["persona_evidence".into()],
    };
    let rebound =
        rebase_subject_soul_source_review_plan_v1(&owner(), &held, &evidence, &proposal, 6)
            .unwrap();
    let SelfAuthoredCoreRefreshPlanV1::Adopt {
        next_core,
        next_ledger,
        expected_prior,
        ..
    } = rebound
    else {
        panic!("rebase");
    };
    assert_eq!(next_core.revision, 3);
    assert_eq!(next_core.supersedes_revision, Some(2));
    assert_eq!(expected_prior.core_revision, Some(2));
    assert_eq!(
        next_core.non_negotiables,
        founding.material.core.non_negotiables
    );
    assert_eq!(
        next_core.boundary_doctrine,
        founding.material.core.boundary_doctrine
    );
    assert_eq!(
        next_ledger.entries.last().unwrap().corrects_revision,
        Some(2)
    );
    assert!(rebase_subject_soul_source_review_plan_v1(
        &owner(),
        &held,
        &evidence,
        &correction_plan(&held, 6, true),
        6
    )
    .is_err());
}

#[test]
fn source_review_read_view_is_metadata_only_and_keeps_true_lifecycle() {
    let (held, _, _) = held_revision();
    let material = held.current_material.as_ref().unwrap();
    let mut view = VerifiedSubjectSoulReadViewV1 {
        source_read_availability: SubjectSoulSourceReadAvailabilityV1::SourceReviewPending {
            basis: held.manifest.source_review.pending_basis().unwrap(),
        },
        memory_space_id: owner().memory_space_id,
        subject_id: owner().subject_id,
        soul_id: owner().soul_id,
        state: held.head.state,
        generation: held.head.generation,
        revision: held.head.current_revision,
        material_digest: Some(material.content_digest.clone()),
        origin: Some(material.provenance.origin),
        requested_view: SubjectSoulReadViewV1::RuntimePrivate,
        runtime_private_core: None,
        governed_disclosure: None,
        head_digest: held.head.head_digest.clone(),
        manifest_digest: held.manifest.closure_digest.clone(),
    };
    view.validate_contract().unwrap();
    assert_eq!(view.state, SubjectSoulLifecycleStateV1::Active);
    view.runtime_private_core = Some(material.core.clone());
    assert!(view.validate_contract().is_err());
}

#[test]
fn recovered_generation_uses_metadata_only_ledger_without_destroying_audit_history() {
    let (held, _, _) = held_revision();
    let restored = adopt_snapshot(
        &held,
        vec![source(2)],
        held.manifest.source_review.pending_basis(),
        6,
    );
    let original = restored.current_revision_ledger.as_ref().unwrap();
    let mut poisoned_text = original.clone();
    for record in &mut poisoned_text.entries {
        record.rationale = "WITHDRAWN_PRIVATE_LEDGER_SENTINEL".into();
        record.evidence_summary = vec!["WITHDRAWN_PRIVATE_LEDGER_SENTINEL".into()];
        record.counterevidence = vec!["WITHDRAWN_PRIVATE_LEDGER_SENTINEL".into()];
        record.relationship_scope_id = "WITHDRAWN_PRIVATE_LEDGER_SENTINEL".into();
        record.source_layers = vec!["WITHDRAWN_PRIVATE_LEDGER_SENTINEL".into()];
        record.adjudication_reason = "WITHDRAWN_PRIVATE_LEDGER_SENTINEL".into();
    }
    let safe = source_governance_ledger_view_v1(&poisoned_text, &restored.manifest.source_review);
    assert!(!serde_json::to_string(&safe)
        .unwrap()
        .contains("WITHDRAWN_PRIVATE_LEDGER_SENTINEL"));
    assert_eq!(safe.entries.len(), original.entries.len());
    assert_eq!(safe.updated_at, original.updated_at);
    assert_eq!(safe.entries.last().unwrap().resulting_revision, 3);
    assert!(poisoned_text.entries[0]
        .rationale
        .contains("WITHDRAWN_PRIVATE_LEDGER_SENTINEL"));
    let safe = source_governance_ledger_view_v1(original, &restored.manifest.source_review);
    let mut refresh = correction_plan(&restored, 8, false);
    let SelfAuthoredCoreRefreshPlanV1::Adopt {
        expected_prior,
        next_ledger,
        ..
    } = &mut refresh
    else {
        panic!("adopt");
    };
    let decision = next_ledger.entries.last().unwrap().clone();
    *expected_prior =
        compute_self_authored_core_expected_prior_v1(restored.current_core.as_ref(), &safe)
            .unwrap();
    *next_ledger = append_core_revision_record(safe, decision);
    let rebound =
        rebase_subject_soul_safe_history_plan_v1(&owner(), &restored, &refresh, 8).unwrap();
    let SelfAuthoredCoreRefreshPlanV1::Adopt {
        expected_prior,
        next_ledger,
        ..
    } = rebound
    else {
        panic!("rebound");
    };
    assert_eq!(
        expected_prior,
        compute_self_authored_core_expected_prior_v1(restored.current_core.as_ref(), original)
            .unwrap()
    );
    assert_eq!(
        &next_ledger.entries[..original.entries.len()],
        &original.entries[..]
    );
    assert!(rebase_subject_soul_safe_history_plan_v1(&owner(), &held, &refresh, 8).is_err());
}

#[test]
fn reviewed_rejection_keeps_same_key_core_and_ledger_lineage_separate() {
    let snapshot = active_snapshot();
    let core = snapshot.current_core.as_ref().unwrap();
    let ledger = snapshot.current_revision_ledger.as_ref().unwrap();
    let next_ledger = append_core_revision_record(
        ledger.clone(),
        CoreRevisionRecord {
            based_on_revision: core.revision,
            resulting_revision: core.revision,
            outcome: CoreRevisionOutcome::Deferred,
            reviewed_at: 5,
            ..Default::default()
        },
    );
    let refresh = SelfAuthoredCoreRefreshPlanV1::ReviewedRejected {
        expected_prior: compute_self_authored_core_expected_prior_v1(Some(core), ledger).unwrap(),
        next_ledger,
        origin: SubjectSoulRevisionOriginV1::SelfGovernedRevision,
        proposal_ref: "deferred-same-key".into(),
        source_refs: vec![],
    };
    let address = SubjectSoulManifestAddressV1 {
        namespace: "core_revision_ledger".into(),
        physical_key: snapshot
            .current_core_document
            .as_ref()
            .unwrap()
            .physical_key
            .clone(),
    };
    let plan = plan_subject_soul_self_authored_revision_v1(
        &owner(),
        &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
            snapshot: Box::new(snapshot.clone()),
        },
        &refresh,
        &SubjectSoulRevisionEvidenceV1 {
            lawful_founding: None,
            source_lineage: vec![source(3)],
            source_review: None,
        },
        Some(
            &SubjectSoulSelfAuthoredPostImageAddressesV1::ReviewedRejected {
                revision_ledger: address,
            },
        ),
        5,
    )
    .unwrap();
    let SubjectSoulSelfAuthoredCommitPlanV1::ReviewedRejected {
        post_head,
        post_manifest,
        revision_ledger,
        revision_ledger_document,
        ..
    } = plan
    else {
        panic!("review rejected");
    };
    let after = SubjectSoulVerifiedSnapshotV1 {
        head: *post_head,
        manifest: *post_manifest,
        current_revision_ledger: Some(*revision_ledger),
        current_revision_ledger_document: Some(*revision_ledger_document),
        ..snapshot
    };
    after.validate_contract().unwrap();
    let core_entry = after
        .manifest
        .entries
        .iter()
        .find(|entry| entry.namespace == "self_authored_core")
        .unwrap();
    assert!(core_entry.source_lineage.is_empty());
    let ledger_entry = after
        .manifest
        .entries
        .iter()
        .find(|entry| entry.namespace == "core_revision_ledger")
        .unwrap();
    assert_eq!(ledger_entry.source_lineage, vec![source(3)]);
}
