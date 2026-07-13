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
    pub fn from_canonical(value: impl Into<String>) -> Option<Self> {
        canonical_opaque_group(value, "recall-family").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
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
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("external_eval:") {
        return recall_external_eval_evidence_group_key(trimmed);
    }
    if lower.starts_with("session_")
        || lower.starts_with("transcript:")
        || lower.starts_with("turn:")
        || lower.starts_with("turn_ledger:")
        || lower.starts_with("archive:")
        || lower.starts_with("daily_note:")
        || lower.starts_with("turn_log:")
    {
        return normalize_structured_evidence_key(trimmed);
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

fn recall_external_eval_evidence_group_key(evidence_ref: &str) -> String {
    let normalized = normalize_structured_evidence_key(evidence_ref);
    let Some(source) = normalized.strip_prefix("external_eval:") else {
        return normalized;
    };
    let canonical_source = source.split('|').next().unwrap_or(source).trim();
    if canonical_source.is_empty() {
        normalized
    } else if canonical_source.starts_with("session_") {
        String::new()
    } else {
        format!("external_eval:{canonical_source}")
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
    fn recall_evidence_groups_collapse_external_eval_composite_aliases() {
        assert_eq!(
            canonical_recall_evidence_group("external_eval:D1:12|session_1"),
            canonical_recall_evidence_group("external_eval:D1:12")
        );
        assert_ne!(
            canonical_recall_evidence_group("external_eval:D1:12"),
            canonical_recall_evidence_group("external_eval:D1:13")
        );
        assert!(canonical_recall_evidence_group("external_eval:session_1").is_empty());
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
        let session_group = canonical_recall_evidence_group("external_eval:D1:12|session_1");
        let conversation_group =
            canonical_recall_evidence_group("external_eval:D1:12|conversation_9");

        assert_eq!(session_group, canonical);
        assert_eq!(conversation_group, canonical);
        assert_eq!(
            recall_evidence_family_group(
                CanonicalRecallEvidenceGroup::from_canonical(session_group)
                    .expect("governed session evidence group")
                    .into(),
            ),
            canonical
        );
        assert_eq!(
            recall_evidence_family_group(
                CanonicalRecallEvidenceGroup::from_canonical(conversation_group)
                    .expect("governed conversation evidence group")
                    .into(),
            ),
            canonical
        );
        assert!(
            CanonicalRecallEvidenceGroup::from_canonical("external_eval:D1:12|session_1").is_none()
        );
        assert!(CanonicalRecallEvidenceFamilyGroup::from_canonical("conversation_9").is_none());
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
