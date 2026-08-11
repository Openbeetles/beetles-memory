#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::Arc;

use bm_core::memory::{
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef, InnerLife,
    PrivateDocEntry, PrivateDocWorkspace, SelfContinuity, SelfModel,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    default_agent_subject_id, IngressKind, MemoryAuditSink, MemoryClock, MemoryIdentity,
    MemoryInspectionRequest, MemoryMaintenanceRequest, MemoryPrivacyClass, MemoryPrivacyPolicy,
    MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope, MemoryWriteRequest,
    NoopMemoryAuditSink, PressureLevel, ProfileId, RuntimeLifecycleModeInput,
    RuntimeSkillCreationRef, RuntimeSkillOwningScope, RuntimeSkillPremiseObservation,
    RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteSource,
};

use support::{
    empty_store_platform, test_runtime, test_runtime_with_scope, StaticHttpClient, StaticLlmClient,
};

struct RuntimeSkillPremiseTestClock;

fn runtime_skill_test_profile() -> ProfileId {
    ProfileId::EspStandaloneMemory
}

impl MemoryClock for RuntimeSkillPremiseTestClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

#[test]
fn runtime_write_recall_project_uses_sdk_entry_only() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);

    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");

    assert!(write.accepted);
    assert_eq!(write.changed, 1);
    let evolution = write
        .procedural_evolution
        .as_ref()
        .expect("procedural evolution report");
    assert!(evolution
        .added
        .iter()
        .any(|name| name == "runtime_skill__release_guard"));
    assert!(evolution
        .reasons
        .iter()
        .any(|reason| reason.contains("procedural_memory")));

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release artifact".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert!(recall
        .procedural_delivery_reports
        .iter()
        .any(|report| report.selected && !report.rendered));
    assert!(!recall.graph_gate.high_confidence_projection_allowed);
    assert!(recall
        .graph_gate
        .failures
        .iter()
        .any(|failure| failure == "runtime_recall_graph_preview_not_persistent"));
    assert!(recall.procedural_delivery_reports.iter().all(|report| {
        !recall
            .graph_rerank
            .candidate_ids
            .contains(&report.candidate_ref)
            && !recall
                .graph_rerank
                .reranked_candidate_ids
                .contains(&report.candidate_ref)
    }));

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "How should I publish?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert_eq!(
        projection.report().temporal_operation(),
        bm_sdk::MemoryRecallTemporalOperation::Current
    );
    assert!(projection.provider_payload().system_memory_block().len() <= 4096);
    assert_eq!(
        projection
            .report()
            .procedural_delivery_reports()
            .iter()
            .filter(|report| report.rendered)
            .count(),
        1,
        "the same governed RuntimeSkill selected by recall must produce one provider-visible procedural block"
    );
    assert!(
        projection
            .provider_payload()
            .system_memory_block()
            .contains("1. inspect artifacts\n2. verify manifest\n3. publish"),
        "the selected governed procedure must reach the provider prompt"
    );
    assert!(
        !projection
            .report()
            .ui_api_projection()
            .contains("1. inspect artifacts\n2. verify manifest\n3. publish"),
        "the governed procedure must remain absent from the public UI/API surface"
    );
    assert!(!projection
        .report()
        .operator_projection()
        .contains("1. inspect artifacts\n2. verify manifest\n3. publish"));
    assert!(!projection
        .report()
        .gateway_audit()
        .block
        .contains("1. inspect artifacts\n2. verify manifest\n3. publish"));
    assert!(!projection
        .report()
        .shared_fact_projection()
        .contains("1. inspect artifacts\n2. verify manifest\n3. publish"));
    assert!(projection.report().audit().delivery_digest_verified);
    assert_eq!(
        projection.report().audit().delivery_digest_candidate_count,
        1
    );
}

#[test]
fn runtime_projection_drops_an_oversized_procedure_as_one_exact_item() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    let procedure = vec!["PROCEDURAL_BUDGET_SENTINEL_STEP"; 96].join("\n");

    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "budget_guard".to_string(),
                topic: "budget".to_string(),
                title: "Budget guard".to_string(),
                summary: "Keep the workflow atomic under projection pressure.".to_string(),
                content: procedure.clone(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write oversized governed procedure");
    assert!(write.accepted, "{write:#?}");
    assert_eq!(write.changed, 1, "{write:#?}");

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "budget workflow".to_string(),
            system_max_len: 1024,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    let reports = projection.report().procedural_delivery_reports();
    let report = reports
        .iter()
        .find(|report| report.selected)
        .unwrap_or_else(|| panic!("selected governed procedure: {reports:#?}"));
    assert!(!report.rendered);
    assert!(report
        .drop_reasons
        .contains(&bm_sdk::RuntimeSkillDeliveryDropReason::RenderBudgetExceeded));
    let forbidden = "PROCEDURAL_BUDGET_SENTINEL_STEP";
    assert!(!projection
        .provider_payload()
        .system_memory_block()
        .contains(forbidden));
    assert!(!projection.report().ui_api_projection().contains(forbidden));
    assert!(!projection
        .report()
        .operator_projection()
        .contains(forbidden));
    assert!(!projection
        .report()
        .gateway_audit()
        .block
        .contains(forbidden));
    assert!(!projection
        .report()
        .shared_fact_projection()
        .contains(forbidden));
    assert!(projection.report().audit().delivery_digest_verified);
    assert_eq!(
        projection.report().audit().delivery_digest_candidate_count,
        0
    );
}

#[test]
fn runtime_projection_accepts_the_exact_procedural_ceiling_and_rejects_ceiling_minus_one() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    let sentinel = "PROCEDURAL_EXACT_CEILING_SENTINEL";
    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "exact_ceiling_guard".to_string(),
                topic: "exact procedural ceiling".to_string(),
                title: "Exact procedural ceiling".to_string(),
                summary: "Bind complete procedural rendering to the minimum accepted ceiling."
                    .to_string(),
                content: format!("1. {sentinel}\n2. Verify the exact render receipt."),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write exact-ceiling governed procedure");
    assert!(write.accepted, "{write:#?}");

    let project = |system_max_len| {
        runtime
            .project(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: "exact procedural ceiling".to_string(),
                system_max_len,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
            })
            .expect("project exact-ceiling governed procedure")
    };
    let rendered_count = |projection: &bm_sdk::MemoryProjectionOutput| {
        projection
            .report()
            .procedural_delivery_reports()
            .iter()
            .filter(|report| report.rendered)
            .count()
    };

    let mut rejected = 1_usize;
    let mut accepted = 4096_usize;
    assert_eq!(rendered_count(&project(accepted)), 1);
    while rejected + 1 < accepted {
        let candidate = rejected + (accepted - rejected) / 2;
        if rendered_count(&project(candidate)) == 1 {
            accepted = candidate;
        } else {
            rejected = candidate;
        }
    }
    assert_eq!(accepted, rejected + 1);

    let exact = project(accepted);
    assert_eq!(rendered_count(&exact), 1);
    assert!(exact
        .provider_payload()
        .system_memory_block()
        .contains(sentinel));
    assert!(
        exact
            .provider_payload()
            .system_memory_block()
            .chars()
            .count()
            <= accepted
    );
    assert!(exact.report().audit().delivery_digest_verified);
    assert_eq!(exact.report().audit().delivery_digest_candidate_count, 1);

    let below = project(rejected);
    let reports = below.report().procedural_delivery_reports();
    assert_eq!(
        reports.iter().filter(|report| report.selected).count(),
        1,
        "{reports:#?}"
    );
    assert_eq!(rendered_count(&below), 0, "{reports:#?}");
    assert!(reports[0]
        .drop_reasons
        .contains(&bm_sdk::RuntimeSkillDeliveryDropReason::RenderBudgetExceeded));
    assert!(!below
        .provider_payload()
        .system_memory_block()
        .contains(sentinel));
    assert!(below.report().audit().delivery_digest_verified);
    assert_eq!(below.report().audit().delivery_digest_candidate_count, 0);
}

#[test]
fn runtime_projection_keeps_the_fitting_item_and_drops_only_the_n_plus_one_item() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    let make_write = |name: &str, content: String, candidate_ref: &str, digest_digit: char| {
        let mut input = support::governed_runtime_skill_write(RuntimeSkillWrite {
            name: name.to_string(),
            topic: "n plus one budget".to_string(),
            title: "N plus one budget".to_string(),
            summary: "Keep each governed procedure atomic at the projection ceiling.".to_string(),
            content,
            citations: vec!["operator accepted".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        });
        input.creation_ref = RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: candidate_ref.to_string(),
            verification_receipt_digest: format!("sha256:{}", digest_digit.to_string().repeat(64)),
        };
        input
    };
    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![
                make_write(
                    "n_plus_one_small",
                    "1. N_PLUS_ONE_FITTING_SENTINEL\n2. Verify the accepted item.".to_string(),
                    "test:n-plus-one-small",
                    '8',
                ),
                make_write(
                    "n_plus_one_large",
                    vec!["N_PLUS_ONE_DROPPED_SENTINEL"; 96].join("\n"),
                    "test:n-plus-one-large",
                    '9',
                ),
            ],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write N+1 governed procedures");
    assert!(write.accepted, "{write:#?}");
    assert_eq!(write.changed, 2, "{write:#?}");

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "n plus one budget".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project N+1 governed procedures");
    let reports = projection.report().procedural_delivery_reports();
    assert_eq!(
        reports.iter().filter(|report| report.selected).count(),
        2,
        "{reports:#?}"
    );
    assert_eq!(
        reports.iter().filter(|report| report.rendered).count(),
        1,
        "{reports:#?}"
    );
    assert_eq!(
        reports
            .iter()
            .filter(|report| {
                report
                    .drop_reasons
                    .contains(&bm_sdk::RuntimeSkillDeliveryDropReason::RenderBudgetExceeded)
            })
            .count(),
        1,
        "{reports:#?}"
    );
    let provider = projection.provider_payload().system_memory_block();
    assert!(
        provider.contains("N_PLUS_ONE_FITTING_SENTINEL"),
        "{provider}"
    );
    assert!(
        !provider.contains("N_PLUS_ONE_DROPPED_SENTINEL"),
        "{provider}"
    );
    assert!(projection.report().audit().delivery_digest_verified);
    assert_eq!(
        projection.report().audit().delivery_digest_candidate_count,
        1
    );
}

#[test]
fn runtime_projection_attributes_identical_procedures_to_distinct_opaque_candidates() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    let procedure = "1. DUPLICATE_CONTENT_PROCEDURE_SENTINEL\n2. Verify exact attribution.";
    let make_write = |name: &str, candidate_ref: &str, digest_digit: char| {
        let mut input = support::governed_runtime_skill_write(RuntimeSkillWrite {
            name: name.to_string(),
            topic: "duplicate procedure".to_string(),
            title: "Duplicate content procedure".to_string(),
            summary: "Prove candidate identity does not collapse equal content.".to_string(),
            content: procedure.to_string(),
            citations: vec!["operator accepted".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        });
        input.creation_ref = RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: candidate_ref.to_string(),
            verification_receipt_digest: format!("sha256:{}", digest_digit.to_string().repeat(64)),
        };
        input
    };
    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![
                make_write("duplicate_a", "test:duplicate-procedure-a", 'a'),
                make_write("duplicate_b", "test:duplicate-procedure-b", 'b'),
            ],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write equal-content governed procedures");
    assert!(write.accepted, "{write:#?}");
    assert_eq!(write.changed, 2, "{write:#?}");

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "duplicate procedure".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project equal-content governed procedures");
    let reports = projection.report().procedural_delivery_reports();
    assert_eq!(reports.len(), 2, "{reports:#?}");
    assert_ne!(reports[0].candidate_ref, reports[1].candidate_ref);
    let rendered = reports
        .iter()
        .filter(|report| report.rendered)
        .collect::<Vec<_>>();
    assert_eq!(rendered.len(), 1, "{reports:#?}");
    assert_eq!(
        projection
            .provider_payload()
            .system_memory_block()
            .matches("DUPLICATE_CONTENT_PROCEDURE_SENTINEL")
            .count(),
        1
    );
    assert!(projection.report().audit().delivery_digest_verified);
    assert_eq!(
        projection.report().audit().delivery_digest_candidate_count,
        1
    );
    for safe_surface in [
        projection.report().ui_api_projection(),
        projection.report().operator_projection(),
        projection.report().gateway_audit().block.as_str(),
        projection.report().shared_fact_projection(),
    ] {
        assert!(!safe_surface.contains("DUPLICATE_CONTENT_PROCEDURE_SENTINEL"));
    }
}

#[test]
fn runtime_projection_keeps_private_runtime_skills_out_of_every_surface_and_receipt() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    let cases = [
        (
            MemoryPrivacyClass::PrivateGarden,
            "private_garden_projection_guard",
            "PRIVATE_GARDEN_PROCEDURE_SENTINEL",
            'c',
        ),
        (
            MemoryPrivacyClass::SoulPrivate,
            "soul_private_projection_guard",
            "SOUL_PRIVATE_PROCEDURE_SENTINEL",
            'd',
        ),
        (
            MemoryPrivacyClass::OperatorDiagnostic,
            "operator_projection_guard",
            "OPERATOR_DIAGNOSTIC_PROCEDURE_SENTINEL",
            'e',
        ),
    ];
    for (privacy_class, name, sentinel, digest_digit) in cases {
        let mut input = support::governed_runtime_skill_write(RuntimeSkillWrite {
            name: name.to_string(),
            topic: name.replace('_', " "),
            title: name.replace('_', " "),
            summary: "Private procedure must remain outside provider and safe surfaces."
                .to_string(),
            content: format!("1. {sentinel}\n2. Keep the material private."),
            citations: vec!["private governed source".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        });
        input.creation_ref = RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: format!("test:{name}"),
            verification_receipt_digest: format!("sha256:{}", digest_digit.to_string().repeat(64)),
        };
        input.privacy_class = privacy_class;
        let write = runtime
            .write(MemoryWriteRequest::Procedural {
                writes: vec![input],
                owning_scope: support::runtime_skill_subject_scope(),
                source: RuntimeSkillWriteSource::Manual,
            })
            .expect("write private governed procedure");
        assert!(write.accepted, "{write:#?}");

        let projection = runtime
            .project(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: name.replace('_', " "),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
            })
            .expect("project private governed procedure");
        assert!(projection
            .report()
            .procedural_delivery_reports()
            .iter()
            .all(|report| !report.selected && !report.rendered));
        assert!(!projection
            .provider_payload()
            .system_memory_block()
            .contains(sentinel));
        for safe_surface in [
            projection.report().ui_api_projection(),
            projection.report().operator_projection(),
            projection.report().gateway_audit().block.as_str(),
            projection.report().shared_fact_projection(),
        ] {
            assert!(!safe_surface.contains(sentinel));
        }
        assert!(projection.report().audit().delivery_digest_verified);
        assert_eq!(
            projection.report().audit().delivery_digest_candidate_count,
            0
        );
    }
}

#[test]
fn runtime_projection_applies_the_shared_program_privacy_matrix_to_every_surface() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    let make_write = |name: &str,
                      topic: &str,
                      sentinel: &str,
                      privacy_class: MemoryPrivacyClass,
                      digest_digit: char| {
        let mut input = support::governed_runtime_skill_write(RuntimeSkillWrite {
            name: name.to_string(),
            topic: topic.to_string(),
            title: topic.to_string(),
            summary: "Prove the SharedProgram privacy matrix on the production projection path."
                .to_string(),
            content: format!("1. {sentinel}\n2. Verify the exact SharedProgram policy."),
            citations: vec!["shared program governed source".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        });
        input.creation_ref = RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: format!("test:{name}"),
            verification_receipt_digest: format!("sha256:{}", digest_digit.to_string().repeat(64)),
        };
        input.privacy_class = privacy_class;
        input
    };
    let public_sentinel = "SHARED_PROGRAM_PUBLIC_RUNTIME_SENTINEL";
    let private_sentinel = "SHARED_PROGRAM_SHARED_WITH_SUBJECT_SENTINEL";
    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![
                make_write(
                    "shared_program_public_runtime",
                    "orbit launch checklist",
                    public_sentinel,
                    MemoryPrivacyClass::PublicRuntime,
                    '4',
                ),
                make_write(
                    "shared_program_shared_with_subject",
                    "orchid irrigation routine",
                    private_sentinel,
                    MemoryPrivacyClass::SharedWithSubject,
                    '5',
                ),
            ],
            owning_scope: RuntimeSkillOwningScope::SharedProgram,
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write SharedProgram privacy matrix");
    assert!(write.accepted, "{write:#?}");
    assert_eq!(write.changed, 2, "{write:#?}");

    let project = |query: &str| {
        runtime
            .project(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: query.to_string(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
            })
            .expect("project SharedProgram privacy matrix")
    };
    let public_projection = project("orbit launch checklist");
    let public_reports = public_projection.report().procedural_delivery_reports();
    assert_eq!(public_reports.len(), 1, "{public_reports:#?}");
    assert!(public_reports[0].selected);
    assert!(public_reports[0].rendered);
    assert!(public_projection
        .provider_payload()
        .system_memory_block()
        .contains(public_sentinel));
    assert!(!public_projection
        .provider_payload()
        .system_memory_block()
        .contains(private_sentinel));
    for safe_surface in [
        public_projection.report().ui_api_projection(),
        public_projection.report().operator_projection(),
        public_projection.report().gateway_audit().block.as_str(),
        public_projection.report().shared_fact_projection(),
    ] {
        assert!(!safe_surface.contains(public_sentinel));
        assert!(!safe_surface.contains(private_sentinel));
    }
    assert_eq!(
        public_projection
            .report()
            .audit()
            .delivery_digest_candidate_count,
        1
    );

    let private_projection = project("orchid irrigation routine");
    let private_reports = private_projection.report().procedural_delivery_reports();
    assert_eq!(private_reports.len(), 1, "{private_reports:#?}");
    assert!(!private_reports[0].matched);
    assert!(!private_reports[0].selected);
    assert!(!private_reports[0].rendered);
    for surface in [
        private_projection.provider_payload().system_memory_block(),
        private_projection.report().ui_api_projection(),
        private_projection.report().operator_projection(),
        private_projection.report().gateway_audit().block.as_str(),
        private_projection.report().shared_fact_projection(),
    ] {
        assert!(!surface.contains(private_sentinel));
    }
    assert!(private_projection.report().audit().delivery_digest_verified);
    assert_eq!(
        private_projection
            .report()
            .audit()
            .delivery_digest_candidate_count,
        0
    );
}

#[test]
fn runtime_projection_does_not_read_cross_subject_or_cross_space_runtime_skills() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let main = support::test_runtime_with_identity_scope(
        platform.clone(),
        profile,
        "agent-main",
        "owner-default",
        "local",
        "chat-1",
    );
    let foreign_subject = support::test_runtime_with_identity_scope(
        platform.clone(),
        profile,
        "agent-foreign",
        "owner-default",
        "local",
        "chat-foreign-subject",
    );
    let foreign_space = support::test_runtime_with_identity_scope(
        platform,
        profile,
        "agent-main",
        "owner-foreign",
        "local",
        "chat-foreign-space",
    );
    let seed = |runtime: &MemoryRuntime, name: &str, sentinel: &str| {
        let write = runtime
            .write(MemoryWriteRequest::Procedural {
                writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                    name: name.to_string(),
                    topic: name.replace('_', " "),
                    title: name.replace('_', " "),
                    summary: "Foreign procedure must never enter the mounted projection."
                        .to_string(),
                    content: format!("1. {sentinel}\n2. Preserve scope isolation."),
                    citations: vec!["foreign governed source".to_string()],
                    source_chat_id: Some("foreign-chat".to_string()),
                    observed_at: 1_800_000_000,
                })],
                owning_scope: bm_sdk::RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: runtime.subject_id().to_string(),
                },
                source: RuntimeSkillWriteSource::Manual,
            })
            .expect("seed foreign governed procedure");
        assert!(write.accepted, "{write:#?}");
    };
    seed(
        &foreign_subject,
        "cross_subject_projection_guard",
        "CROSS_SUBJECT_PROCEDURE_SENTINEL",
    );
    seed(
        &foreign_space,
        "cross_space_projection_guard",
        "CROSS_SPACE_PROCEDURE_SENTINEL",
    );

    for (query, sentinel) in [
        (
            "cross subject projection guard",
            "CROSS_SUBJECT_PROCEDURE_SENTINEL",
        ),
        (
            "cross space projection guard",
            "CROSS_SPACE_PROCEDURE_SENTINEL",
        ),
    ] {
        let projection = main
            .project(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: query.to_string(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
            })
            .expect("project current mounted scope");
        assert!(projection.report().procedural_delivery_reports().is_empty());
        assert!(!projection
            .provider_payload()
            .system_memory_block()
            .contains(sentinel));
        for safe_surface in [
            projection.report().ui_api_projection(),
            projection.report().operator_projection(),
            projection.report().gateway_audit().block.as_str(),
            projection.report().shared_fact_projection(),
        ] {
            assert!(!safe_surface.contains(sentinel));
        }
        assert!(projection.report().audit().delivery_digest_verified);
        assert_eq!(
            projection.report().audit().delivery_digest_candidate_count,
            0
        );
    }
}

#[test]
fn runtime_projection_reports_same_scope_query_miss_without_exposing_owner_identity() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release guard".to_string(),
                summary: "Verify governed artifacts before release.".to_string(),
                content: "1. QUERY_MISS_PROCEDURE_SENTINEL\n2. Verify the manifest.".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write governed procedure");

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "unrelated gardening question".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project query miss");
    let reports = projection.report().procedural_delivery_reports();
    assert_eq!(reports.len(), 1, "{reports:#?}");
    assert!(!reports[0].candidate_ref.is_empty());
    assert!(!reports[0].matched);
    assert!(!reports[0].selected);
    assert!(!reports[0].rendered);
    assert!(reports[0]
        .drop_reasons
        .contains(&bm_sdk::RuntimeSkillDeliveryDropReason::QueryUnmatched));
    assert!(!projection
        .provider_payload()
        .system_memory_block()
        .contains("QUERY_MISS_PROCEDURE_SENTINEL"));
    assert_eq!(
        projection.report().audit().delivery_digest_candidate_count,
        0
    );
}

#[test]
fn runtime_rejects_host_asserted_governed_premise_presence() {
    let profile = runtime_skill_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(platform)
        .clock(Arc::new(RuntimeSkillPremiseTestClock))
        .runtime_skill_premise_observations(vec![
            RuntimeSkillPremiseObservation::GovernedEnvironmentEvidence {
                evidence_revision_ref: GovernedOwnerRevisionRef {
                    owner_ref: GovernedMemoryOwnerRef::new(
                        GovernedMemoryOwnerPlane::EvidenceDocument,
                        "environment-1",
                    ),
                    owner_revision: 1,
                },
                present: true,
            },
        ])
        .build()
        .expect("runtime");
    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed typed RuntimeSkill");
    assert!(write.accepted, "{write:#?}");
    assert_eq!(write.changed, 1);
    let error = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release artifact".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect_err("the host cannot assert typed store evidence");
    assert_eq!(error.stage(), "runtime_skill_delivery");
}

#[test]
fn runtime_projection_isolates_session_context_by_chat_scope_under_same_store_platform() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    platform
        .replay_harness()
        .session_store()
        .append("chat-a", "user", "chat-a-only-user")
        .expect("seed chat-a user");
    platform
        .replay_harness()
        .session_store()
        .append("chat-a", "assistant", "chat-a-only-assistant")
        .expect("seed chat-a assistant");
    platform
        .replay_harness()
        .session_store()
        .append("chat-b", "user", "chat-b-only-user")
        .expect("seed chat-b user");
    platform
        .replay_harness()
        .session_store()
        .append("chat-b", "assistant", "chat-b-only-assistant")
        .expect("seed chat-b assistant");
    platform
        .replay_harness()
        .session_summary_store()
        .set_with_count("chat-a", "chat-a-only-summary", 2)
        .expect("seed chat-a summary");
    platform
        .replay_harness()
        .session_summary_store()
        .set_with_count("chat-b", "chat-b-only-summary", 2)
        .expect("seed chat-b summary");

    let runtime_a = test_runtime_with_scope(platform.clone(), profile, "local", "chat-a");
    let runtime_b = test_runtime_with_scope(platform, profile, "local", "chat-b");

    let project = |runtime: &MemoryRuntime, query: &str| {
        runtime
            .project(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: query.to_string(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
            })
            .expect("projection")
    };

    let projection_a = project(&runtime_a, "what happened in chat a?");
    let provider_a = projection_a.provider_payload().system_memory_block();
    assert!(provider_a.contains("chat-a-only-summary"), "{provider_a}");
    assert!(!provider_a.contains("chat-b-only"), "{provider_a}");

    let projection_b = project(&runtime_b, "what happened in chat b?");
    let provider_b = projection_b.provider_payload().system_memory_block();
    assert!(provider_b.contains("chat-b-only-summary"), "{provider_b}");
    assert!(!provider_b.contains("chat-a-only"), "{provider_b}");
}

#[test]
fn runtime_maintain_and_inspect_return_structured_reports() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform, support::host_test_profile());
    let llm = StaticLlmClient::summary_response("Summary: release safety");
    let mut http = StaticHttpClient;

    let maintenance = runtime
        .maintain(
            &mut http,
            &llm,
            MemoryMaintenanceRequest {
                ingress: IngressKind::User,
                user_content: "remember the release process".to_string(),
                reply_content: "I will verify artifacts first.".to_string(),
                tool_calls: 0,
                external_content_used: false,
                runtime_skill_selected_ids: Vec::new(),
                task_learning_selected_ids: Vec::new(),
                reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
                reuse_outcome_note: String::new(),
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .expect("maintenance");

    let maintenance_report = maintenance.report.expect("maintenance report");
    assert!(maintenance_report.after_count <= maintenance_report.after_count.saturating_add(1));

    let inspection = runtime
        .inspect(MemoryInspectionRequest {
            query: "release".to_string(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("inspection");

    assert_eq!(inspection.working.query, "release");
    assert_eq!(
        inspection.capabilities.profile,
        support::host_test_profile()
    );
}

#[test]
fn runtime_projection_includes_private_planes_when_policy_allows_it() {
    let platform = empty_store_platform(support::host_test_profile());
    let mounted_subject_id = default_agent_subject_id("agent-main");
    platform
        .replay_harness()
        .private_doc_store()
        .set(
            &mounted_subject_id,
            &PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "private workspace release note".to_string(),
                    updated_at: 1_800_000_000,
                    revision: 1,
                }),
                ..PrivateDocWorkspace::default()
            },
        )
        .expect("private workspace seed");
    platform
        .replay_harness()
        .private_garden_store()
        .write(
            &mounted_subject_id,
            "diary/release.md",
            "private garden release note",
            1_800_000_000,
        )
        .expect("private garden seed");
    platform
        .replay_harness()
        .self_model_store()
        .set(
            &mounted_subject_id,
            &SelfModel {
                continuity_anchor: "private self model release anchor".to_string(),
                attachment_style: "steady".to_string(),
                privacy_need: "high".to_string(),
                directness: "direct".to_string(),
                ..SelfModel::default()
            },
        )
        .expect("self model seed");

    let mut privacy = MemoryPrivacyPolicy::standard_private_boundary();
    privacy.private_plane_projection_allowed = true;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(platform)
        .clock(Arc::new(TestClock))
        .capability_policy(bm_sdk::MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(privacy)
        .audit_sink(Arc::new(NoopMemoryAuditSink) as Arc<dyn MemoryAuditSink>)
        .build()
        .expect("runtime");

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "release".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    let provider_prompt = projection.provider_payload().system_memory_block();
    assert!(provider_prompt.contains("private workspace release note"));
    assert!(provider_prompt.contains("private garden release note"));
    assert!(provider_prompt.contains("Continuity tendencies"));
    let lower = provider_prompt.to_ascii_lowercase();
    for forbidden in [
        "roleplay",
        "personality",
        "model identity",
        "memory helper",
        "assistant self-description",
        "relationship theater",
        "training provenance",
        "user-facing identity",
        "personality axes",
    ] {
        assert!(
            !lower.contains(forbidden),
            "{forbidden} leaked into private runtime projection:\n{}",
            provider_prompt
        );
    }
    assert!(
        provider_prompt.contains("## Soul Private Runtime Context"),
        "{}",
        provider_prompt
    );
    assert!(
        provider_prompt.contains("Runtime private context: allowed"),
        "{}",
        provider_prompt
    );
    assert!(projection.report().audit().runtime_private_context_allowed);
    assert!(!projection.report().audit().foreground_disclosure_allowed);
    for forbidden_heading in [
        "## Private Garden",
        "## Inner Workspace",
        "## Inner Life",
        "## Self State",
        "## Outer Voice",
    ] {
        assert!(
            !provider_prompt.contains(forbidden_heading),
            "{}",
            provider_prompt
        );
    }
    for (surface, block) in [
        ("ui_api", projection.report().ui_api_projection()),
        (
            "gateway_raw_audit",
            projection.report().gateway_audit().block.as_str(),
        ),
    ] {
        for private_raw in [
            "private workspace release note",
            "private garden release note",
            "private self model release anchor",
        ] {
            assert!(
                !block.contains(private_raw),
                "{surface} leaked exact protected content: {private_raw}"
            );
        }
    }
    assert!(projection.report().audit().disclosure_integrity_passed);
    assert_eq!(projection.report().audit().raw_private_violation_count, 0);
}

#[test]
fn runtime_projection_excludes_private_planes_when_policy_denies_it() {
    let platform = empty_store_platform(support::host_test_profile());
    let mounted_subject_id = default_agent_subject_id("agent-main");
    platform
        .replay_harness()
        .self_model_store()
        .set(
            &mounted_subject_id,
            &SelfModel {
                continuity_anchor: "denied private self model anchor".to_string(),
                private_notes: "denied private self model note".to_string(),
                ..SelfModel::default()
            },
        )
        .expect("self model seed");
    platform
        .replay_harness()
        .self_continuity_store()
        .set(
            &mounted_subject_id,
            &SelfContinuity {
                wake_anchor: "denied private self continuity anchor".to_string(),
                current_self_state: "denied private self continuity state".to_string(),
                ..SelfContinuity::default()
            },
        )
        .expect("self continuity seed");
    platform
        .replay_harness()
        .inner_life_store()
        .set(
            &mounted_subject_id,
            &InnerLife {
                internal_monologue: "denied private inner monologue".to_string(),
                private_journal: "denied private inner journal".to_string(),
                ..InnerLife::default()
            },
        )
        .expect("inner life seed");
    platform
        .replay_harness()
        .private_doc_store()
        .set(
            &mounted_subject_id,
            &PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "denied private workspace note".to_string(),
                    updated_at: 1_800_000_000,
                    revision: 1,
                }),
                ..PrivateDocWorkspace::default()
            },
        )
        .expect("private workspace seed");
    platform
        .replay_harness()
        .private_garden_store()
        .write(
            &mounted_subject_id,
            "diary/denied.md",
            "denied private garden note",
            1_800_000_000,
        )
        .expect("private garden seed");
    let runtime =
        test_runtime_with_scope(platform, support::host_test_profile(), "local", "chat-1");

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "release".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(!projection.report().audit().runtime_private_context_allowed);
    assert!(!projection.report().audit().foreground_disclosure_allowed);
    let provider_prompt = projection.provider_payload().system_memory_block();
    for private_text in [
        "denied private self model anchor",
        "denied private self model note",
        "denied private self continuity anchor",
        "denied private self continuity state",
        "denied private inner monologue",
        "denied private inner journal",
        "denied private workspace note",
        "denied private garden note",
    ] {
        assert!(
            !provider_prompt.contains(private_text),
            "{}",
            provider_prompt
        );
    }
    assert_eq!(projection.report().audit().raw_private_violation_count, 0);
}

struct TestClock;

impl MemoryClock for TestClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}
