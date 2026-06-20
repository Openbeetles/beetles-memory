pub(crate) fn recall_source_authority_score(source: &str) -> u32 {
    let normalized = source.trim().to_ascii_lowercase();
    if normalized.starts_with("external_eval:")
        || normalized.starts_with("transcript:")
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

pub(crate) fn recall_evidence_group_key(evidence_ref: &str) -> String {
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
    lower
        .split([':', '#', '/'])
        .next()
        .unwrap_or(trimmed)
        .trim()
        .to_string()
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
        assert_ne!(
            recall_evidence_group_key("archive:release#turn=1"),
            recall_evidence_group_key("archive:release#turn=2")
        );
    }

    #[test]
    fn recall_evidence_groups_collapse_external_eval_composite_aliases() {
        assert_eq!(
            recall_evidence_group_key("external_eval:D1:12|session_1"),
            recall_evidence_group_key("external_eval:D1:12")
        );
        assert_ne!(
            recall_evidence_group_key("external_eval:D1:12"),
            recall_evidence_group_key("external_eval:D1:13")
        );
        assert!(recall_evidence_group_key("external_eval:session_1").is_empty());
    }
}
