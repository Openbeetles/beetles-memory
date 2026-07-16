use bm_core::memory::{
    canonical_recall_evidence_group, recall_evidence_family_group,
    CanonicalRecallEvidenceFamilyGroup, CanonicalRecallEvidenceGroup,
};

#[test]
fn benchmark_locator_strings_do_not_determine_production_evidence_family() {
    let canonical = canonical_recall_evidence_group("external_eval:D1:12");
    let session_group = canonical_recall_evidence_group("external_eval:D1:12|session_1");
    let conversation_group = canonical_recall_evidence_group("external_eval:D1:12|conversation_9");

    assert_ne!(session_group, canonical);
    assert_ne!(conversation_group, canonical);
    assert_ne!(session_group, conversation_group);
    for group in [session_group, conversation_group] {
        let governed = CanonicalRecallEvidenceGroup::from_canonical(group.clone())
            .expect("canonical evidence group must cross the production boundary");
        assert_eq!(recall_evidence_family_group(governed.into()), group);
    }
    assert!(
        CanonicalRecallEvidenceGroup::from_canonical("external_eval:D1:12|session_1").is_none()
    );
    assert!(CanonicalRecallEvidenceGroup::from_canonical("conversation_9").is_none());
}

#[test]
fn governed_canonical_family_is_used_without_locator_parsing() {
    let governed =
        CanonicalRecallEvidenceFamilyGroup::from_structured_identity("conversation:conversation_9")
            .expect("structured family identity must close at the owner boundary");
    let canonical_family = governed.as_str().to_string();

    assert_eq!(
        recall_evidence_family_group(governed.into()),
        canonical_family
    );
    assert!(CanonicalRecallEvidenceFamilyGroup::from_canonical("session_1").is_none());
    assert!(CanonicalRecallEvidenceFamilyGroup::from_canonical("conversation_9").is_none());
}
