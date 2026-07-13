use bm_core::memory::{
    canonical_recall_evidence_group, recall_evidence_family_group,
    CanonicalRecallEvidenceFamilyGroup, CanonicalRecallEvidenceGroup,
};

#[test]
fn benchmark_locator_strings_do_not_determine_production_evidence_family() {
    let canonical = canonical_recall_evidence_group("external_eval:D1:12");
    let session_group = canonical_recall_evidence_group("external_eval:D1:12|session_1");
    let conversation_group = canonical_recall_evidence_group("external_eval:D1:12|conversation_9");

    assert_eq!(session_group, canonical);
    assert_eq!(conversation_group, canonical);
    for group in [session_group, conversation_group] {
        let governed = CanonicalRecallEvidenceGroup::from_canonical(group)
            .expect("canonical evidence group must cross the production boundary");
        assert_eq!(recall_evidence_family_group(governed.into()), canonical);
    }
    assert!(
        CanonicalRecallEvidenceGroup::from_canonical("external_eval:D1:12|session_1").is_none()
    );
    assert!(CanonicalRecallEvidenceGroup::from_canonical("conversation_9").is_none());
}

#[test]
fn governed_canonical_family_is_used_without_locator_parsing() {
    let canonical_family = format!("opaque:recall-family:sha256:{}", "a".repeat(64));
    let governed = CanonicalRecallEvidenceFamilyGroup::from_canonical(canonical_family.clone())
        .expect("canonical family group must cross the production boundary");

    assert_eq!(
        recall_evidence_family_group(governed.into()),
        canonical_family
    );
    assert!(CanonicalRecallEvidenceFamilyGroup::from_canonical("session_1").is_none());
    assert!(CanonicalRecallEvidenceFamilyGroup::from_canonical("conversation_9").is_none());
}
