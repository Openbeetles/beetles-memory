use bm_core::{
    MemoryPlane, MentalPrivacyLayer, MentalPrivacyOwnerAccessMode, MentalPrivacyPolicy,
    MentalPrivacyQuotePolicy, MentalPrivacyVisibility, RuntimeProfile, SoulFeedbackReport,
    SoulGovernanceRef, SoulSourceKind, SubjectProjectionReport, SubjectShellReport,
};

#[test]
fn soul_governance_reference_is_policy_not_raw_private_text() {
    let policy = MentalPrivacyPolicy {
        layer: MentalPrivacyLayer::Private,
        visibility: MentalPrivacyVisibility::SummaryOnly,
        owner_access_mode: MentalPrivacyOwnerAccessMode::RequestOnly,
        quote_policy: MentalPrivacyQuotePolicy::NeverQuote,
    };

    let source = SoulGovernanceRef {
        source_id: "self-core:42".to_owned(),
        source_kind: SoulSourceKind::SelfAuthoredCore,
        layer: MentalPrivacyLayer::Private,
        revision: Some("42".to_owned()),
        policy: policy.clone(),
    };

    assert!(!policy.allows_raw_default_surface());
    assert_eq!(source.source_kind, SoulSourceKind::SelfAuthoredCore);
    assert_eq!(
        source.policy.quote_policy,
        MentalPrivacyQuotePolicy::NeverQuote
    );
}

#[test]
fn esp_compact_rejects_raw_soul_governance_but_allows_subject_projection() {
    assert!(!RuntimeProfile::EspCompact.allows_plane(MemoryPlane::SoulGovernance));
    assert!(RuntimeProfile::EspCompact.allows_plane(MemoryPlane::SubjectProjection));
}

#[test]
fn subject_projection_is_current_turn_projection_not_write_api() {
    let shell = SubjectShellReport {
        grounded: true,
        sources_used: vec![
            "self_authored_core".to_owned(),
            "continuity_capsule".to_owned(),
        ],
        sources_missing: vec![],
        profile: RuntimeProfile::EspCompact,
    };
    let projection = SubjectProjectionReport {
        mounted: true,
        summary: "当前回合使用 compact 主体挂载帧。".to_owned(),
        privacy_filtered: true,
        budget_bytes: 512,
        shell,
    };
    let feedback = SoulFeedbackReport {
        reply: Some("先按受治理事实回答。".to_owned()),
        initiative: None,
        strategy: Some("保持边界，不输出私域原文。".to_owned()),
        privacy_filtered: true,
    };

    assert!(projection.mounted);
    assert!(projection.privacy_filtered);
    assert!(projection.budget_bytes <= RuntimeProfile::EspCompact.projection_budget_bytes());
    assert!(feedback.privacy_filtered);
}
