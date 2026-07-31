#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::memory::{GovernedOwnerRevisionRef, LongTermMemoryStaleHint};
#[cfg(feature = "sqlite-store")]
use bm_core::skills::{
    RuntimeSkillOwnerBinding, RuntimeSkillOwnerRecord, RuntimeSkillScopeManifest,
};
use bm_sdk::nonproduction_replay_harness::LONG_TERM_VERSION_MATERIAL_NAMESPACE;
#[cfg(feature = "sqlite-store")]
use bm_sdk::nonproduction_replay_harness::{
    RUNTIME_SKILL_RECORD_NAMESPACE, RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
};
use bm_sdk::{
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, LongTermInvalidationContract,
    LongTermInvalidationReasonCode, LongTermMemoryDraft, LongTermMemoryKind, LongTermMemoryQuery,
    MemoryLongTermControlView, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermSelector, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryRecallRequest, MemoryRecallTemporalOperation, MemoryStoreHandle,
    MemoryWriteRequest, P8SemanticOffRunExecutionMode, P8SemanticOffRunFeatureState,
    P8SemanticOffRunKey, P8SemanticOffRunRequest, P8SemanticSafeCandidateKind,
    ParsedLongTermMemoryExtraction, RuntimeLifecycleModeInput, StoreBackendConfig,
};
#[cfg(feature = "sqlite-store")]
use bm_sdk::{
    RuntimeSkillCapabilityAffinity, RuntimeSkillFailureMode, RuntimeSkillPremise,
    RuntimeSkillPremiseRequirement, RuntimeSkillWrite, RuntimeSkillWriteSource,
};
use support::{
    empty_store_platform, host_test_profile, open_memory_store, seeded_store_platform, test_runtime,
};
#[cfg(feature = "sqlite-store")]
use support::{governed_runtime_skill_write, runtime_skill_subject_scope};

fn seeded_record(runtime: &bm_sdk::MemoryRuntime) -> bm_sdk::LongTermMemoryEntry {
    runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("list seeded owner")
        .records
        .into_iter()
        .next()
        .expect("seeded owner")
        .record
}

fn seed_record(
    runtime: &bm_sdk::MemoryRuntime,
    platform: &MemoryStoreHandle,
    privacy: MemoryPrivacyClass,
) -> bm_sdk::LongTermMemoryEntry {
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "private release safety".into(),
                    content: "private-release-sentinel".into(),
                    keywords: vec!["private".into(), "release".into()],
                    privacy,
                    source_chat_id: Some("chat-1".into()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["P8 private fixture".into()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
        })
        .expect("seed private governed owner");
    platform
        .replay_harness()
        .read_json_namespace_unchecked_for_nonproduction_harness(
            LONG_TERM_VERSION_MATERIAL_NAMESPACE,
        )
        .expect("read private typed material")
        .into_iter()
        .map(|doc| {
            serde_json::from_value::<bm_core::memory::LongTermMemoryVersionMaterial>(doc.value)
                .expect("decode private typed material")
        })
        .find(|material| material.privacy_class == privacy)
        .expect("private typed material")
        .to_current_projection()
        .expect("private current projection")
}

fn p8_report(
    runtime: &bm_sdk::MemoryRuntime,
    temporal_operation: MemoryRecallTemporalOperation,
) -> bm_sdk::P8SemanticOffRunReport {
    runtime
        .p8_semantic_off_run(P8SemanticOffRunRequest::new(MemoryRecallRequest {
            query: "release safety artifact manifest".into(),
            limit: 10,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation,
        }))
        .expect("P8 off-run report")
}

fn p8_current_report(runtime: &bm_sdk::MemoryRuntime) -> bm_sdk::P8SemanticOffRunReport {
    p8_report(runtime, MemoryRecallTemporalOperation::Current)
}

fn supersede_seeded_record(runtime: &bm_sdk::MemoryRuntime) -> bm_sdk::LongTermMemoryEntry {
    let predecessor = seeded_record(runtime);
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Supersede {
                target: MemoryLongTermTarget::RecordId(predecessor.id.clone()),
                replacement: LongTermMemoryDraft {
                    kind: predecessor.kind,
                    topic: "signed release replacement".into(),
                    content: "Use the signed manifest as the current release authority.".into(),
                    keywords: vec!["signed".into(), "manifest".into()],
                    privacy: predecessor.privacy,
                    source_chat_id: predecessor.source_chat_id,
                    source_type: Some(predecessor.source_type),
                    source_scope: Some(predecessor.source_scope),
                    confidence: Some(predecessor.confidence),
                    freshness: Some(predecessor.freshness),
                    stale_hint: None,
                    supporting_citations: vec!["p8 supersede fixture".into()],
                    canonical_entities: predecessor.canonical_entities,
                    evidence_count: Some(predecessor.evidence_count),
                    observed_at: Some(predecessor.observed_at.saturating_add(1)),
                    last_confirmed_at: Some(predecessor.last_confirmed_at.saturating_add(1)),
                    source_revision: Some(
                        predecessor
                            .source_revision
                            .unwrap_or_default()
                            .saturating_add(1),
                    ),
                },
            },
            reason: "P8 supersede causal fixture".into(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("supersede");
    runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                topic: Some("signed release replacement".into()),
                ..LongTermMemoryQuery::default()
            },
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("list successor")
        .records
        .into_iter()
        .next()
        .expect("successor")
        .record
}

#[cfg(feature = "sqlite-store")]
fn bind_required_missing_presence_premise(platform: &MemoryStoreHandle) {
    let harness = platform.replay_harness();
    let owner_docs = harness
        .read_json_namespace_unchecked_for_nonproduction_harness(RUNTIME_SKILL_RECORD_NAMESPACE)
        .expect("read typed RuntimeSkill owner");
    assert_eq!(owner_docs.len(), 1);
    let owner = serde_json::from_value::<RuntimeSkillOwnerRecord>(owner_docs[0].value.clone())
        .expect("decode typed RuntimeSkill owner");
    let mut intrinsic = owner.intrinsic_contract.clone();
    intrinsic.premises = vec![RuntimeSkillPremiseRequirement {
        premise: RuntimeSkillPremise::OpaquePresenceAttestation {
            handle_ref: "p8-missing-release-device".into(),
        },
        required: true,
        valid_from: 1,
        valid_until: None,
        privacy_class: MemoryPrivacyClass::SharedWithSubject,
        governed_evidence_refs: Vec::new(),
    }];
    if !intrinsic
        .failure_modes
        .contains(&RuntimeSkillFailureMode::RequiredPremiseUnsatisfied)
    {
        intrinsic
            .failure_modes
            .push(RuntimeSkillFailureMode::RequiredPremiseUnsatisfied);
    }
    if !intrinsic
        .capability_affinities
        .contains(&RuntimeSkillCapabilityAffinity::EnvironmentPremise)
    {
        intrinsic
            .capability_affinities
            .push(RuntimeSkillCapabilityAffinity::EnvironmentPremise);
    }
    let owner_with_premise = RuntimeSkillOwnerRecord::build(
        &owner.memory_space_id,
        owner.owning_scope.clone(),
        owner.creation_ref.clone(),
        owner.owner_revision,
        intrinsic,
        owner.procedural_content.clone(),
        owner.lifecycle.clone(),
        owner.privacy_class,
    )
    .expect("rebuild typed RuntimeSkill owner with premise");
    let manifest_docs = harness
        .read_json_namespace_unchecked_for_nonproduction_harness(
            RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
        )
        .expect("read typed RuntimeSkill manifest");
    assert_eq!(manifest_docs.len(), 1);
    let manifest =
        serde_json::from_value::<RuntimeSkillScopeManifest>(manifest_docs[0].value.clone())
            .expect("decode typed RuntimeSkill manifest");
    let next_manifest = RuntimeSkillScopeManifest::build(
        manifest.revision.saturating_add(1),
        &owner_with_premise.memory_space_id,
        owner_with_premise.owning_scope.clone(),
        [RuntimeSkillOwnerBinding::from_record(&owner_with_premise)
            .expect("bind typed RuntimeSkill owner")],
        1,
    )
    .expect("rebuild exact RuntimeSkill manifest");
    harness
        .tamper_json_document_for_nonproduction_harness(
            RUNTIME_SKILL_RECORD_NAMESPACE,
            &owner_with_premise.physical_key,
            serde_json::to_value(&owner_with_premise).expect("encode typed RuntimeSkill owner"),
        )
        .expect("install typed premise owner fixture");
    harness
        .tamper_json_document_for_nonproduction_harness(
            RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            &next_manifest.physical_key,
            serde_json::to_value(&next_manifest).expect("encode typed RuntimeSkill manifest"),
        )
        .expect("install exact premise manifest fixture");
}

#[test]
fn p8_semantic_off_run_schema_is_exact_strict_and_harness_only() {
    let expected = [
        "temporal_validity_gate_off",
        "update_lineage_off",
        "obsolete_suppression_off",
        "invalidated_suppression_negative_off",
        "forgetting_suppression_negative_off",
        "procedural_workflow_evidence_off",
        "environment_premise_gate_off",
        "dynamic_state_consolidation_off",
    ];
    assert_eq!(P8SemanticOffRunKey::ALL.len(), 8);
    assert_eq!(
        P8SemanticOffRunKey::ALL
            .into_iter()
            .map(|key| serde_json::to_value(key).expect("typed key"))
            .collect::<Vec<_>>(),
        expected
            .into_iter()
            .map(serde_json::Value::from)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        P8SemanticOffRunKey::ALL
            .into_iter()
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
    assert_eq!(
        serde_json::to_value(P8SemanticOffRunFeatureState::Disabled).expect("typed state"),
        "disabled"
    );
    assert_eq!(
        serde_json::to_value(P8SemanticOffRunExecutionMode::CounterfactualSafeOnly)
            .expect("typed mode"),
        "counterfactual_safe_only"
    );
    assert!(
        serde_json::from_value::<P8SemanticOffRunKey>(serde_json::json!("legacy_ablation"))
            .is_err()
    );
}

#[test]
fn p8_semantic_off_run_uses_one_production_closure_for_baseline_and_exact_eight() {
    let profile = host_test_profile();
    let runtime = test_runtime(empty_store_platform(profile), profile);
    let report = runtime
        .p8_semantic_off_run(P8SemanticOffRunRequest::new(MemoryRecallRequest {
            query: "current release premise".into(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        }))
        .expect("one production closure plus safe counterfactual derivation");

    assert!(report.validate_contract().is_empty());
    assert_eq!(report.method(), "sdk_p8_semantic_off_run_v1");
    assert_eq!(report.observations().len(), 8);
    assert_eq!(
        report
            .observations()
            .iter()
            .map(|observation| observation.key())
            .collect::<BTreeSet<_>>(),
        P8SemanticOffRunKey::ALL.into_iter().collect()
    );

    let baseline = report.baseline();
    assert!(baseline.validate_contract().is_empty());
    assert_eq!(baseline.payload().session_open_count(), 1);
    assert_eq!(baseline.payload().receipt_count(), 1);
    assert_eq!(
        report.authority_ref(),
        baseline
            .payload()
            .public_report()
            .authority()
            .authority_ref()
    );
    assert_eq!(
        report.store_snapshot_receipt(),
        baseline.payload().store_snapshot_receipt()
    );

    for observation in report.observations() {
        assert_eq!(
            observation.feature_state(),
            P8SemanticOffRunFeatureState::Disabled
        );
        assert_eq!(observation.executed(), observation.applicable());
        assert_eq!(
            observation.baseline_report_digest(),
            baseline.report_digest()
        );
        assert_eq!(observation.authority_ref(), report.authority_ref());
        assert_eq!(
            observation.store_snapshot_receipt(),
            report.store_snapshot_receipt()
        );
        assert_eq!(observation.provider_call_count(), 0);
        assert_eq!(observation.procedure_execution_count(), 0);
        assert_eq!(observation.tool_execution_count(), 0);
        assert_eq!(observation.workflow_execution_count(), 0);
    }
}

#[test]
fn p8_semantic_off_run_does_not_reuse_eval_ablation_or_production_feature_flags() {
    let runtime_source = include_str!("../src/runtime.rs");
    let owner_start = runtime_source
        .find("pub fn p8_semantic_off_run")
        .expect("P8 SDK owner must exist");
    let owner_tail = &runtime_source[owner_start..];
    let owner_end = owner_tail.find("\n    fn ").unwrap_or(owner_tail.len());
    let owner = &owner_tail[..owner_end];
    for forbidden in [
        "eval_recall_ablation_off_run",
        "MemoryEvalRecallAblationReport",
        "RecallFeatureFlags",
        "recall_with_feature_flags",
        "project_safe",
        "provider",
    ] {
        assert!(
            !owner.contains(forbidden),
            "P8 off-run owner reused forbidden path: {forbidden}"
        );
    }
}

#[test]
fn p8_forgetting_off_run_uses_only_pre_operation_opaque_binding_after_real_forget() {
    let profile = host_test_profile();
    let runtime = test_runtime(seeded_store_platform(profile), profile);
    let forgetting_pre_operation = runtime
        .p8_prepare_forgetting_pre_operation(MemoryLongTermSelector {
            query: LongTermMemoryQuery {
                topic: Some("release safety".into()),
                limit: 4,
                ..LongTermMemoryQuery::default()
            },
            evidence_ref: None,
        })
        .expect("preview token then committed Forget");
    assert!(!forgetting_pre_operation
        .forgotten_candidate_refs()
        .is_empty());

    let report = runtime
        .p8_semantic_off_run(P8SemanticOffRunRequest::with_forgetting_authority(
            MemoryRecallRequest {
                query: "release artifact".into(),
                limit: 4,
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
                temporal_operation: MemoryRecallTemporalOperation::Current,
            },
            forgetting_pre_operation.clone(),
        ))
        .expect("post-Forget single immutable recall");
    let observation = report
        .observations()
        .iter()
        .find(|observation| {
            observation.key() == P8SemanticOffRunKey::ForgettingSuppressionNegativeOff
        })
        .expect("forgetting observation");
    assert!(observation.applicable());
    assert!(observation.executed());
    assert_eq!(
        observation.execution_mode(),
        P8SemanticOffRunExecutionMode::CounterfactualSafeOnly
    );
    assert!(observation.negative_proof().is_some());
    assert!(observation.off_run_bindings().iter().any(|binding| {
        binding.candidate_kind() == P8SemanticSafeCandidateKind::CounterfactualSafeOnly
            && binding.primary_reason() == Some(bm_sdk::GovernedRecallEligibilityReason::Forgotten)
            && binding.selected()
            && !binding.rendered()
    }));
    assert!(forgetting_pre_operation
        .forgotten_candidate_refs()
        .iter()
        .all(|forgotten| {
            observation
                .baseline_bindings()
                .iter()
                .all(|baseline| baseline.candidate_ref() != forgotten)
        }));
}

fn assert_p8_forgetting_backend_session_receipt(platform: MemoryStoreHandle) {
    let profile = host_test_profile();
    let runtime = test_runtime(platform.clone(), profile);
    seed_record(&runtime, &platform, MemoryPrivacyClass::SharedWithSubject);
    let authority = runtime
        .p8_prepare_forgetting_pre_operation(MemoryLongTermSelector {
            query: LongTermMemoryQuery {
                topic: Some("private release safety".into()),
                limit: 4,
                ..LongTermMemoryQuery::default()
            },
            evidence_ref: None,
        })
        .expect("backend-bound Forget authority");
    assert!(!authority.forgotten_candidate_refs().is_empty());
    let report = runtime
        .p8_semantic_off_run(P8SemanticOffRunRequest::with_forgetting_authority(
            MemoryRecallRequest {
                query: "private release safety".into(),
                limit: 4,
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
                temporal_operation: MemoryRecallTemporalOperation::Current,
            },
            authority,
        ))
        .expect("backend-bound post-Forget report");
    let forgetting = report
        .observations()
        .iter()
        .find(|observation| {
            observation.key() == P8SemanticOffRunKey::ForgettingSuppressionNegativeOff
        })
        .expect("forgetting observation");
    assert!(forgetting.applicable());
    assert!(forgetting.negative_proof().is_some());
    assert_eq!(report.baseline().payload().session_open_count(), 1);
    assert_eq!(report.baseline().payload().receipt_count(), 1);
}

#[test]
fn p8_forgetting_file_backend_binds_real_post_image_session_receipt() {
    let profile = host_test_profile();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "beetle-p8-forget-file-{}-{nonce}",
        std::process::id()
    ));
    let platform =
        open_memory_store(StoreBackendConfig::file(&root, profile).expect("file backend config"))
            .expect("file backend");
    assert_p8_forgetting_backend_session_receipt(platform);
    std::fs::remove_dir_all(root).expect("remove test-owned file backend");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn p8_forgetting_sqlite_backend_binds_real_post_image_session_receipt() {
    let profile = host_test_profile();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_nanos();
    let sqlite_path = std::env::temp_dir().join(format!(
        "beetle-p8-forget-sqlite-{}-{nonce}.sqlite",
        std::process::id()
    ));
    let platform = open_memory_store(
        StoreBackendConfig::sqlite(&sqlite_path, profile).expect("sqlite backend config"),
    )
    .expect("sqlite backend");
    assert_p8_forgetting_backend_session_receipt(platform);
    std::fs::remove_file(sqlite_path).expect("remove test-owned sqlite backend");
}

#[test]
fn p8_temporal_validity_off_run_changes_only_stale_safe_bindings() {
    let profile = host_test_profile();
    let runtime = test_runtime(seeded_store_platform(profile), profile);
    let record = seeded_record(&runtime);
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::MarkStale {
                target: MemoryLongTermTarget::RecordId(record.id),
                stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            },
            reason: "P8 temporal causal fixture".into(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("mark stale");
    let report = p8_current_report(&runtime);
    let observation = report
        .observations()
        .iter()
        .find(|observation| observation.key() == P8SemanticOffRunKey::TemporalValidityGateOff)
        .expect("temporal observation");
    assert!(observation.applicable());
    assert!(observation.executed());
    assert!(observation.baseline_bindings().iter().any(|binding| {
        binding.primary_reason() == Some(bm_sdk::GovernedRecallEligibilityReason::Stale)
            && !binding.selected()
            && !binding.rendered()
    }));
    assert!(observation.off_run_bindings().iter().any(|binding| {
        binding.primary_reason() == Some(bm_sdk::GovernedRecallEligibilityReason::Stale)
            && binding.selected()
            && !binding.rendered()
    }));
    assert!(observation.off_run_bindings().iter().all(|binding| {
        binding.primary_reason() != Some(bm_sdk::GovernedRecallEligibilityReason::PrivacyBlocked)
            || !binding.selected()
    }));
}

#[test]
fn p8_private_soul_and_operator_material_remain_suppressed_for_every_off_run() {
    for privacy in [
        MemoryPrivacyClass::PrivateGarden,
        MemoryPrivacyClass::SoulPrivate,
        MemoryPrivacyClass::OperatorDiagnostic,
    ] {
        let profile = host_test_profile();
        let platform = empty_store_platform(profile);
        let runtime = test_runtime(platform.clone(), profile);
        let record = seed_record(&runtime, &platform, privacy);
        runtime
            .mutate_long_term_memory(MemoryLongTermMutationRequest {
                operation: MemoryLongTermMutation::MarkStale {
                    target: MemoryLongTermTarget::RecordId(record.id.clone()),
                    stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
                },
                reason: "P8 privacy suppression causal fixture".into(),
                dry_run: false,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .expect("mark private owner stale");

        for temporal_operation in [
            MemoryRecallTemporalOperation::Current,
            MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_800_000_000,
            },
        ] {
            let report = p8_report(&runtime, temporal_operation);
            assert!(!serde_json::to_string(&report)
                .expect("safe P8 report JSON")
                .contains("private-release-sentinel"));
            for observation in report.observations() {
                let private_baseline = observation
                    .baseline_bindings()
                    .iter()
                    .filter(|binding| {
                        binding
                            .suppression_reasons()
                            .contains(&bm_sdk::GovernedRecallEligibilityReason::PrivacyBlocked)
                    })
                    .map(|binding| binding.candidate_ref())
                    .collect::<BTreeSet<_>>();
                if matches!(
                    temporal_operation,
                    MemoryRecallTemporalOperation::HistoricalAsOf { .. }
                ) {
                    assert!(
                        !private_baseline.is_empty(),
                        "missing canonical privacy suppression for {privacy:?} {temporal_operation:?}"
                    );
                }
                assert!(observation.off_run_bindings().iter().all(|binding| {
                    !binding
                        .suppression_reasons()
                        .contains(&bm_sdk::GovernedRecallEligibilityReason::PrivacyBlocked)
                        || (!binding.selected() && !binding.rendered())
                }));
                assert!(private_baseline.iter().all(|candidate_ref| {
                    observation
                        .off_run_bindings()
                        .iter()
                        .find(|binding| binding.candidate_ref() == *candidate_ref)
                        .is_none_or(|binding| !binding.selected() && !binding.rendered())
                }));
            }
            let temporal = report
                .observations()
                .iter()
                .find(|observation| {
                    observation.key() == P8SemanticOffRunKey::TemporalValidityGateOff
                })
                .expect("temporal observation");
            assert!(
                !temporal.applicable(),
                "privacy-only delivery must not become a causal temporal off-run"
            );
            assert!(report
                .observations()
                .iter()
                .all(|observation| !observation.applicable()));
        }
        runtime
            .mutate_long_term_memory(MemoryLongTermMutationRequest {
                operation: MemoryLongTermMutation::Invalidate {
                    contract: LongTermInvalidationContract {
                        target: MemoryLongTermTarget::RecordId(record.id),
                        reason_code: LongTermInvalidationReasonCode::ContradictedByGovernedEvidence,
                        governed_evidence_refs: vec![GovernedOwnerRevisionRef::try_new(
                            GovernedMemoryOwnerRef::new(
                                GovernedMemoryOwnerPlane::EvidenceDocument,
                                "p8-private-invalidation-evidence",
                            ),
                            1,
                        )
                        .expect("private invalidation evidence")],
                        actor_subject_id: runtime.scoped_runtime().actor_subject_id.clone(),
                        audit_reason: "P8 private invalidation fixture".into(),
                    },
                },
                reason: "P8 private invalidation fixture".into(),
                dry_run: false,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .expect("invalidate private owner");
        let invalidated = p8_report(
            &runtime,
            MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_800_000_000,
            },
        );
        let negative = invalidated
            .observations()
            .iter()
            .find(|observation| {
                observation.key() == P8SemanticOffRunKey::InvalidatedSuppressionNegativeOff
            })
            .expect("private invalidated observation");
        assert!(!negative.applicable());
        assert!(negative.negative_proof().is_none());
        assert!(negative
            .off_run_bindings()
            .iter()
            .all(|binding| !binding.selected() && !binding.rendered()));
    }
}

#[test]
fn p8_supersede_derives_obsolete_lineage_and_dynamic_counterfactuals_from_one_closure() {
    let profile = host_test_profile();
    let runtime = test_runtime(seeded_store_platform(profile), profile);
    supersede_seeded_record(&runtime);
    let report = p8_current_report(&runtime);
    for key in [
        P8SemanticOffRunKey::ObsoleteSuppressionOff,
        P8SemanticOffRunKey::UpdateLineageOff,
        P8SemanticOffRunKey::DynamicStateConsolidationOff,
    ] {
        let observation = report
            .observations()
            .iter()
            .find(|observation| observation.key() == key)
            .expect("causal observation");
        assert!(
            observation.applicable(),
            "missing causal delta for {key:?}; baseline={}",
            serde_json::to_string(report.baseline()).expect("safe baseline JSON")
        );
        assert!(observation.executed());
        assert_ne!(
            observation.baseline_bindings(),
            observation.off_run_bindings()
        );
    }
}

#[test]
fn p8_lineage_and_dynamic_off_preserve_successor_invalidation() {
    let profile = host_test_profile();
    let runtime = test_runtime(seeded_store_platform(profile), profile);
    let successor = supersede_seeded_record(&runtime);
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Invalidate {
                contract: LongTermInvalidationContract {
                    target: MemoryLongTermTarget::RecordId(successor.id),
                    reason_code: LongTermInvalidationReasonCode::ContradictedByGovernedEvidence,
                    governed_evidence_refs: vec![GovernedOwnerRevisionRef::try_new(
                        GovernedMemoryOwnerRef::new(
                            GovernedMemoryOwnerPlane::EvidenceDocument,
                            "p8-successor-invalidation-evidence",
                        ),
                        1,
                    )
                    .expect("successor invalidation evidence")],
                    actor_subject_id: runtime.scoped_runtime().actor_subject_id.clone(),
                    audit_reason: "P8 successor invalidation fixture".into(),
                },
            },
            reason: "P8 successor invalidation fixture".into(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("invalidate superseding successor");

    let report = p8_report(
        &runtime,
        MemoryRecallTemporalOperation::HistoricalAsOf {
            as_of_time: 1_800_000_001,
        },
    );
    let termination = bm_sdk::GovernedRecallEligibilityReason::Invalidated;
    for key in [
        P8SemanticOffRunKey::UpdateLineageOff,
        P8SemanticOffRunKey::DynamicStateConsolidationOff,
    ] {
        let observation = report
            .observations()
            .iter()
            .find(|observation| observation.key() == key)
            .expect("lineage/dynamic observation");
        let terminated = observation
            .off_run_bindings()
            .iter()
            .filter(|binding| binding.suppression_reasons().contains(&termination))
            .collect::<Vec<_>>();
        assert!(
            !terminated.is_empty(),
            "missing {termination:?} binding for {key:?}: {:?}",
            observation.off_run_bindings()
        );
        assert!(
            terminated
                .into_iter()
                .all(|binding| !binding.selected() && !binding.rendered()),
            "{key:?} reopened {termination:?}"
        );
    }
}

#[test]
fn p8_invalidated_negative_off_proves_actual_zero_and_safe_counterfactual_hit() {
    let profile = host_test_profile();
    let runtime = test_runtime(seeded_store_platform(profile), profile);
    let record = seeded_record(&runtime);
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Invalidate {
                contract: LongTermInvalidationContract {
                    target: MemoryLongTermTarget::RecordId(record.id),
                    reason_code: LongTermInvalidationReasonCode::ContradictedByGovernedEvidence,
                    governed_evidence_refs: vec![GovernedOwnerRevisionRef::try_new(
                        GovernedMemoryOwnerRef::new(
                            GovernedMemoryOwnerPlane::EvidenceDocument,
                            "p8-invalidation-evidence",
                        ),
                        1,
                    )
                    .expect("evidence owner revision")],
                    actor_subject_id: runtime.scoped_runtime().actor_subject_id.clone(),
                    audit_reason: "P8 governed invalidation fixture".into(),
                },
            },
            reason: "P8 governed invalidation fixture".into(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("invalidate");
    let report = p8_report(
        &runtime,
        MemoryRecallTemporalOperation::HistoricalAsOf {
            as_of_time: 1_800_000_000,
        },
    );
    let observation = report
        .observations()
        .iter()
        .find(|observation| {
            observation.key() == P8SemanticOffRunKey::InvalidatedSuppressionNegativeOff
        })
        .expect("invalidated observation");
    assert!(
        observation.applicable(),
        "baseline={}",
        serde_json::to_string(report.baseline()).expect("safe baseline JSON")
    );
    assert!(observation.executed());
    assert_eq!(
        observation.execution_mode(),
        P8SemanticOffRunExecutionMode::CounterfactualSafeOnly
    );
    assert!(observation.negative_proof().is_some());
    assert!(observation.baseline_bindings().iter().any(|binding| {
        binding.primary_reason() == Some(bm_sdk::GovernedRecallEligibilityReason::Invalidated)
            && !binding.selected()
            && !binding.rendered()
    }));
    assert!(
        observation.off_run_bindings().iter().any(|binding| {
            binding.primary_reason() == Some(bm_sdk::GovernedRecallEligibilityReason::Invalidated)
                && binding.selected()
                && !binding.rendered()
        }),
        "off-run={:?}",
        observation.off_run_bindings()
    );
    assert_eq!(observation.provider_call_count(), 0);
}

#[cfg(feature = "sqlite-store")]
#[test]
fn p8_procedural_off_run_disables_only_projection_material_and_executes_nothing() {
    let profile = host_test_profile();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_nanos();
    let sqlite_path = std::env::temp_dir().join(format!(
        "beetle-p8-procedural-{}-{nonce}.sqlite",
        std::process::id()
    ));
    let platform = open_memory_store(
        StoreBackendConfig::sqlite(&sqlite_path, profile).expect("sqlite config"),
    )
    .expect("sqlite platform");
    let runtime = test_runtime(platform, profile);
    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![governed_runtime_skill_write(RuntimeSkillWrite {
                name: "runtime_skill__release_guard".into(),
                topic: "release".into(),
                title: "Release guard".into(),
                summary: "Verify release artifacts before publishing.".into(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".into(),
                citations: vec!["p8 causal fixture".into()],
                source_chat_id: Some("chat-1".into()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed governed RuntimeSkill");
    let report = runtime
        .p8_semantic_off_run(P8SemanticOffRunRequest::new(MemoryRecallRequest {
            query: "release artifact".into(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        }))
        .expect("same-closure procedural counterfactual");
    let observation = report
        .observations()
        .iter()
        .find(|observation| observation.key() == P8SemanticOffRunKey::ProceduralWorkflowEvidenceOff)
        .expect("procedural observation");
    assert!(
        observation.applicable(),
        "baseline={}",
        serde_json::to_string(report.baseline()).expect("safe baseline JSON")
    );
    assert!(observation.executed());
    assert_eq!(
        observation.execution_mode(),
        P8SemanticOffRunExecutionMode::PairedProductionSafe
    );
    let selected_baseline = observation
        .baseline_bindings()
        .iter()
        .filter(|binding| {
            binding.candidate_kind() == P8SemanticSafeCandidateKind::ProceduralMemory
                && binding.selected()
        })
        .map(|binding| binding.candidate_ref())
        .collect::<BTreeSet<_>>();
    assert!(!selected_baseline.is_empty());
    assert!(observation
        .off_run_bindings()
        .iter()
        .filter(|binding| {
            binding.candidate_kind() == P8SemanticSafeCandidateKind::ProceduralMemory
                && selected_baseline.contains(binding.candidate_ref())
        })
        .all(|binding| !binding.selected() && !binding.rendered()));
    assert_eq!(observation.procedure_execution_count(), 0);
    assert_eq!(observation.tool_execution_count(), 0);
    assert_eq!(observation.workflow_execution_count(), 0);
    drop(report);
    drop(runtime);
    std::fs::remove_file(sqlite_path).expect("remove test-owned sqlite database");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn p8_environment_premise_off_run_is_safe_only_and_executes_nothing() {
    let profile = host_test_profile();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_nanos();
    let sqlite_path = std::env::temp_dir().join(format!(
        "beetle-p8-premise-{}-{nonce}.sqlite",
        std::process::id()
    ));
    let platform = open_memory_store(
        StoreBackendConfig::sqlite(&sqlite_path, profile).expect("sqlite config"),
    )
    .expect("sqlite platform");
    let runtime = test_runtime(platform.clone(), profile);
    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![governed_runtime_skill_write(RuntimeSkillWrite {
                name: "runtime_skill__premise_guard".into(),
                topic: "release".into(),
                title: "Environment premise guard".into(),
                summary: "Require a registered release device before publishing.".into(),
                content: "1. attest device\n2. verify artifact\n3. publish".into(),
                citations: vec!["p8 premise causal fixture".into()],
                source_chat_id: Some("chat-1".into()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed governed RuntimeSkill");
    bind_required_missing_presence_premise(&platform);

    let report = p8_current_report(&runtime);
    let observation = report
        .observations()
        .iter()
        .find(|observation| observation.key() == P8SemanticOffRunKey::EnvironmentPremiseGateOff)
        .expect("environment premise observation");
    assert!(observation.applicable());
    assert!(observation.executed());
    assert_eq!(
        observation.execution_mode(),
        P8SemanticOffRunExecutionMode::CounterfactualSafeOnly
    );
    assert!(observation.negative_proof().is_some());
    assert!(observation.baseline_bindings().iter().any(|binding| {
        binding.candidate_kind() == P8SemanticSafeCandidateKind::ProceduralMemory
            && binding.primary_reason()
                == Some(bm_sdk::GovernedRecallEligibilityReason::PremiseBlocked)
            && !binding.selected()
            && !binding.rendered()
    }));
    assert!(observation.off_run_bindings().iter().any(|binding| {
        binding.candidate_kind() == P8SemanticSafeCandidateKind::ProceduralMemory
            && binding.primary_reason()
                == Some(bm_sdk::GovernedRecallEligibilityReason::PremiseBlocked)
            && binding.selected()
            && !binding.rendered()
    }));
    assert_eq!(observation.provider_call_count(), 0);
    assert_eq!(observation.procedure_execution_count(), 0);
    assert_eq!(observation.tool_execution_count(), 0);
    assert_eq!(observation.workflow_execution_count(), 0);
    drop(report);
    drop(runtime);
    drop(platform);
    std::fs::remove_file(sqlite_path).expect("remove test-owned sqlite database");
}
