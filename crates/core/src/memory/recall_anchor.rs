use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRecallEvidenceGroup(String);

impl CanonicalRecallEvidenceGroup {
    pub fn from_canonical(value: impl Into<String>) -> Option<Self> {
        canonical_opaque_group(value, "recall-group").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRecallEvidenceFamilyGroup(String);

impl CanonicalRecallEvidenceFamilyGroup {
    pub fn from_structured_identity(value: &str) -> Option<Self> {
        let normalized = normalize_structured_evidence_key(value);
        let canonical = opaque_semantic_group("recall-family", &normalized);
        (!canonical.is_empty()).then_some(Self(canonical))
    }

    pub fn from_canonical(value: impl Into<String>) -> Option<Self> {
        canonical_opaque_group(value, "recall-family").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecallEvidenceFamilyInput {
    CanonicalFamily(CanonicalRecallEvidenceFamilyGroup),
    CanonicalEvidenceFallback(CanonicalRecallEvidenceGroup),
}

impl From<CanonicalRecallEvidenceGroup> for RecallEvidenceFamilyInput {
    fn from(group: CanonicalRecallEvidenceGroup) -> Self {
        Self::CanonicalEvidenceFallback(group)
    }
}

impl From<CanonicalRecallEvidenceFamilyGroup> for RecallEvidenceFamilyInput {
    fn from(group: CanonicalRecallEvidenceFamilyGroup) -> Self {
        Self::CanonicalFamily(group)
    }
}

pub(crate) fn recall_source_authority_score(source: &str) -> u32 {
    let normalized = source.trim().to_ascii_lowercase();
    if normalized.starts_with("transcript:")
        || normalized.starts_with("turn:")
        || normalized.starts_with("turn_ledger:")
        || normalized.starts_with("session_")
        || normalized.starts_with("archive:")
        || normalized.starts_with("daily_note:")
        || normalized.starts_with("turn_log:")
    {
        16
    } else if normalized.contains("scratchpad") || normalized.contains("debug") {
        1
    } else if normalized.is_empty() {
        0
    } else {
        6
    }
}

pub fn canonical_recall_evidence_group(evidence_ref: &str) -> String {
    let trimmed = evidence_ref.trim();
    if is_opaque_semantic_group(trimmed, "recall-group") {
        return trimmed.to_ascii_lowercase();
    }
    opaque_semantic_group(
        "recall-group",
        &canonical_recall_evidence_semantic_key(trimmed),
    )
}

fn canonical_recall_evidence_semantic_key(evidence_ref: &str) -> String {
    let trimmed = evidence_ref.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    normalize_structured_evidence_key(trimmed)
}

pub fn recall_evidence_family_group(input: RecallEvidenceFamilyInput) -> String {
    match input {
        RecallEvidenceFamilyInput::CanonicalFamily(group) => group.0,
        RecallEvidenceFamilyInput::CanonicalEvidenceFallback(group) => group.0,
    }
}

fn opaque_semantic_group(namespace: &str, semantic_key: &str) -> String {
    if semantic_key.is_empty() {
        return String::new();
    }
    let digest = Sha256::digest(semantic_key.as_bytes());
    format!("opaque:{namespace}:sha256:{digest:x}")
}

fn is_opaque_semantic_group(value: &str, namespace: &str) -> bool {
    let Some(digest) = value
        .to_ascii_lowercase()
        .strip_prefix(&format!("opaque:{namespace}:sha256:"))
        .map(str::to_string)
    else {
        return false;
    };
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn canonical_opaque_group(value: impl Into<String>, namespace: &str) -> Option<String> {
    let value = value.into();
    if value == value.to_ascii_lowercase() && is_opaque_semantic_group(&value, namespace) {
        Some(value)
    } else {
        None
    }
}

fn normalize_structured_evidence_key(input: &str) -> String {
    input
        .trim()
        .trim_matches(|ch: char| {
            !(ch.is_alphanumeric() || matches!(ch, '_' | ':' | '#' | '=' | '-' | '/' | '.'))
        })
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_evidence_groups_keep_archive_locators_distinct() {
        let first = canonical_recall_evidence_group("archive:release#turn=1");
        let second = canonical_recall_evidence_group("archive:release#turn=2");
        assert_ne!(first, second);
        assert!(first.starts_with("opaque:recall-group:sha256:"));
        assert!(!first.contains("archive"));
        assert!(!first.contains("release"));
    }

    #[test]
    fn production_recall_groups_do_not_parse_external_eval_locator_aliases() {
        assert_ne!(
            canonical_recall_evidence_group("external_eval:D1:12|session_1"),
            canonical_recall_evidence_group("external_eval:D1:12")
        );
        assert_ne!(
            canonical_recall_evidence_group("external_eval:D1:12"),
            canonical_recall_evidence_group("external_eval:D1:13")
        );
        assert!(!canonical_recall_evidence_group("external_eval:session_1").is_empty());
    }

    #[test]
    fn recall_evidence_groups_keep_distinct_urls_and_unknown_locators_exact() {
        assert_ne!(
            canonical_recall_evidence_group("https://example.test/a"),
            canonical_recall_evidence_group("https://example.test/b")
        );
        assert_ne!(
            canonical_recall_evidence_group("source:alpha"),
            canonical_recall_evidence_group("source:beta")
        );
        for group in [
            canonical_recall_evidence_group("https://example.test/a"),
            canonical_recall_evidence_group("source:alpha"),
        ] {
            assert!(group.starts_with("opaque:recall-group:sha256:"));
            assert!(!group.contains("example.test"));
            assert!(!group.contains("alpha"));
        }
    }

    #[test]
    fn external_eval_marker_does_not_receive_production_source_authority_bonus() {
        assert_eq!(recall_source_authority_score("external_eval:D1:12"), 6);
        assert_eq!(recall_source_authority_score("transcript:chat#turn=1"), 16);
    }

    #[test]
    fn opaque_recall_groups_are_idempotent_for_benchmark_mapping() {
        let canonical = canonical_recall_evidence_group("archive:release#turn=1");
        let family = recall_evidence_family_group(
            CanonicalRecallEvidenceGroup::from_canonical(canonical.clone())
                .expect("canonical evidence group")
                .into(),
        );

        assert_eq!(canonical_recall_evidence_group(&canonical), canonical);
        assert_eq!(family, canonical);
    }

    #[test]
    fn benchmark_locator_strings_do_not_determine_production_evidence_family() {
        let canonical = canonical_recall_evidence_group("external_eval:D1:12");
        assert!(
            CanonicalRecallEvidenceGroup::from_canonical("external_eval:D1:12|session_1").is_none()
        );
        let family = CanonicalRecallEvidenceFamilyGroup::from_structured_identity(
            "conversation:conversation_9",
        )
        .expect("structured family");
        assert_eq!(
            recall_evidence_family_group(family.clone().into()),
            family.as_str()
        );
        assert_ne!(family.as_str(), canonical);
    }

    #[test]
    fn governed_canonical_family_overrides_evidence_group_fallback() {
        let family = opaque_semantic_group("recall-family", "governed:conversation:9");

        assert_eq!(
            recall_evidence_family_group(
                CanonicalRecallEvidenceFamilyGroup::from_canonical(family.clone())
                    .expect("governed family group")
                    .into(),
            ),
            family
        );
    }
}
