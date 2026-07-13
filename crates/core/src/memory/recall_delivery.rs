use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallDeliveryCandidate {
    pub candidate_id: String,
    pub canonical_evidence_groups: Vec<String>,
    pub evidence_family_groups: Vec<String>,
    pub owner_available: bool,
    pub citation_eligible: bool,
    pub privacy_eligible: bool,
    pub temporal_eligible: bool,
    pub source_rank: Option<usize>,
    pub expanded_rank: Option<usize>,
    pub reranked_rank: usize,
    pub relevance_score: u32,
    pub authority_score: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallDeliveryText<'a> {
    pub candidate_id: &'a str,
    pub text: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallDeliveryLexicalScore {
    pub candidate_id: String,
    pub score: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecallDeliverySelectionDropReason {
    CitationMissing,
    DuplicateEvidenceGroup,
    OwnerRecordUnavailable,
    ProfileBudgetExhausted,
    PrivacyScopeBlocked,
    TemporalSuperseded,
    LowerRank,
}

impl RecallDeliverySelectionDropReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CitationMissing => "citation_missing",
            Self::DuplicateEvidenceGroup => "duplicate_evidence_group",
            Self::OwnerRecordUnavailable => "owner_record_unavailable",
            Self::ProfileBudgetExhausted => "profile_budget_exhausted",
            Self::PrivacyScopeBlocked => "privacy_scope_blocked",
            Self::TemporalSuperseded => "temporal_superseded",
            Self::LowerRank => "lower_rank",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecallDeliveryOrderingPolicy {
    RelevanceRank,
    EvidenceFamilyRotationWithinEqualUtility,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallDeliverySelectionDecision {
    pub candidate_id: String,
    pub canonical_evidence_groups: Vec<String>,
    pub evidence_family_groups: Vec<String>,
    pub selected: bool,
    pub drop_reason: Option<RecallDeliverySelectionDropReason>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallDeliverySelectionReport {
    pub selected_candidate_ids: Vec<String>,
    pub decisions: Vec<RecallDeliverySelectionDecision>,
    pub covered_evidence_groups: Vec<String>,
    pub covered_evidence_family_groups: Vec<String>,
    pub ordering_policy: RecallDeliveryOrderingPolicy,
}

pub fn allocate_recall_delivery_candidates(
    candidates: &[RecallDeliveryCandidate],
    limit: usize,
    ordering_policy: RecallDeliveryOrderingPolicy,
) -> RecallDeliverySelectionReport {
    let mut selected_candidate_ids = Vec::new();
    let mut covered_groups = BTreeSet::new();
    let mut covered_families = BTreeSet::new();
    let eligible = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            candidate.owner_available
                && candidate_has_governed_citation(candidate)
                && candidate.privacy_eligible
                && candidate.temporal_eligible
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut ordered = match ordering_policy {
        RecallDeliveryOrderingPolicy::EvidenceFamilyRotationWithinEqualUtility => {
            relevance_preserving_family_order(candidates, &eligible)
        }
        RecallDeliveryOrderingPolicy::RelevanceRank => {
            let mut ranked = eligible.clone();
            ranked.sort_by(|left, right| {
                compare_delivery_utility(&candidates[*left], &candidates[*right])
            });
            ranked
        }
    };
    for index in ordered.drain(..) {
        if selected_candidate_ids.len() >= limit {
            break;
        }
        let candidate = &candidates[index];
        if candidate_overlaps_covered_evidence_group(candidate, &covered_groups) {
            continue;
        }
        select_delivery_candidate(
            candidate,
            &mut selected_candidate_ids,
            &mut covered_groups,
            &mut covered_families,
        );
    }
    let decisions = candidates
        .iter()
        .map(|candidate| {
            let selected = selected_candidate_ids
                .iter()
                .any(|selected| selected == &candidate.candidate_id);
            let duplicate_group =
                candidate_overlaps_covered_evidence_group(candidate, &covered_groups);
            RecallDeliverySelectionDecision {
                candidate_id: candidate.candidate_id.clone(),
                canonical_evidence_groups: candidate.canonical_evidence_groups.clone(),
                evidence_family_groups: candidate.evidence_family_groups.clone(),
                selected,
                drop_reason: if selected {
                    None
                } else if !candidate.owner_available {
                    Some(RecallDeliverySelectionDropReason::OwnerRecordUnavailable)
                } else if !candidate.privacy_eligible {
                    Some(RecallDeliverySelectionDropReason::PrivacyScopeBlocked)
                } else if !candidate.temporal_eligible {
                    Some(RecallDeliverySelectionDropReason::TemporalSuperseded)
                } else if !candidate_has_governed_citation(candidate) {
                    Some(RecallDeliverySelectionDropReason::CitationMissing)
                } else if duplicate_group {
                    Some(RecallDeliverySelectionDropReason::DuplicateEvidenceGroup)
                } else {
                    Some(RecallDeliverySelectionDropReason::ProfileBudgetExhausted)
                },
            }
        })
        .collect();

    RecallDeliverySelectionReport {
        selected_candidate_ids,
        decisions,
        covered_evidence_groups: covered_groups.into_iter().collect(),
        covered_evidence_family_groups: covered_families.into_iter().collect(),
        ordering_policy,
    }
}

fn candidate_has_governed_citation(candidate: &RecallDeliveryCandidate) -> bool {
    candidate.citation_eligible && !candidate.canonical_evidence_groups.is_empty()
}

fn candidate_overlaps_covered_evidence_group(
    candidate: &RecallDeliveryCandidate,
    covered_groups: &BTreeSet<String>,
) -> bool {
    candidate
        .canonical_evidence_groups
        .iter()
        .any(|group| covered_groups.contains(group))
}

pub fn score_recall_delivery_texts(
    query: &str,
    documents: &[RecallDeliveryText<'_>],
) -> Vec<RecallDeliveryLexicalScore> {
    let query_terms = delivery_lexical_features(query);
    if query_terms.is_empty() || documents.is_empty() {
        return documents
            .iter()
            .map(|document| RecallDeliveryLexicalScore {
                candidate_id: document.candidate_id.to_string(),
                score: 0,
            })
            .collect();
    }
    let document_terms = documents
        .iter()
        .map(|document| delivery_lexical_features(document.text))
        .collect::<Vec<_>>();
    let average_document_len = document_terms.iter().map(Vec::len).sum::<usize>().max(1) as f64
        / document_terms.len() as f64;
    let query_term_set = query_terms.iter().cloned().collect::<BTreeSet<_>>();
    let mut document_frequency = BTreeMap::<String, usize>::new();
    for terms in &document_terms {
        let unique = terms.iter().cloned().collect::<BTreeSet<_>>();
        for term in unique.intersection(&query_term_set) {
            *document_frequency.entry(term.clone()).or_default() += 1;
        }
    }
    let document_count = documents.len() as f64;
    documents
        .iter()
        .zip(document_terms.iter())
        .map(|(document, terms)| {
            let mut term_frequency = BTreeMap::<&str, usize>::new();
            for term in terms {
                *term_frequency.entry(term.as_str()).or_default() += 1;
            }
            let document_len = terms.len().max(1) as f64;
            let mut score = 0.0_f64;
            for query_term in &query_term_set {
                let frequency = term_frequency
                    .get(query_term.as_str())
                    .copied()
                    .unwrap_or(0) as f64;
                if frequency == 0.0 {
                    continue;
                }
                let frequency_in_documents =
                    document_frequency.get(query_term).copied().unwrap_or(0) as f64;
                let inverse_document_frequency = ((document_count - frequency_in_documents + 0.5)
                    / (frequency_in_documents + 0.5)
                    + 1.0)
                    .ln();
                let length_normalization =
                    1.2 * (1.0 - 0.75 + 0.75 * document_len / average_document_len);
                score += inverse_document_frequency * (frequency * (1.2 + 1.0))
                    / (frequency + length_normalization);
            }
            RecallDeliveryLexicalScore {
                candidate_id: document.candidate_id.to_string(),
                score: (score * 1_000.0).round().clamp(0.0, u32::MAX as f64) as u32,
            }
        })
        .collect()
}

fn delivery_lexical_features(input: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "a", "an", "and", "are", "at", "be", "did", "do", "does", "for", "from", "has", "have",
        "in", "is", "it", "of", "on", "or", "the", "to", "was", "were", "what", "when", "where",
        "which", "who", "why", "with", "would",
    ];
    let normalized = input.to_lowercase();
    let mut features = Vec::new();
    let mut token = String::new();
    let flush_token = |token: &mut String, features: &mut Vec<String>| {
        if token.is_empty() {
            return;
        }
        let value = std::mem::take(token);
        if STOP_WORDS.contains(&value.as_str()) {
            return;
        }
        features.push(format!("w:{value}"));
        let chars = value.chars().collect::<Vec<_>>();
        if chars.len() >= 4 {
            for window in chars.windows(3) {
                features.push(format!("g:{}{}{}", window[0], window[1], window[2]));
            }
        }
    };
    let mut cjk_run = Vec::new();
    for ch in normalized.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if !cjk_run.is_empty() {
                append_cjk_features(&mut features, &cjk_run);
                cjk_run.clear();
            }
            token.push(ch);
        } else if is_cjk(ch) {
            flush_token(&mut token, &mut features);
            cjk_run.push(ch);
        } else {
            flush_token(&mut token, &mut features);
            if !cjk_run.is_empty() {
                append_cjk_features(&mut features, &cjk_run);
                cjk_run.clear();
            }
        }
    }
    features
}

fn append_cjk_features(features: &mut Vec<String>, chars: &[char]) {
    for ch in chars {
        features.push(format!("c:{ch}"));
    }
    for window in chars.windows(2) {
        features.push(format!("b:{}{}", window[0], window[1]));
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff)
}

fn relevance_preserving_family_order(
    candidates: &[RecallDeliveryCandidate],
    eligible: &[usize],
) -> Vec<usize> {
    let mut relevance_order = eligible.to_vec();
    relevance_order
        .sort_by(|left, right| compare_delivery_utility(&candidates[*left], &candidates[*right]));
    let mut ordered = Vec::with_capacity(relevance_order.len());
    let mut start = 0;
    while start < relevance_order.len() {
        let utility = delivery_utility(&candidates[relevance_order[start]]);
        let mut end = start + 1;
        while end < relevance_order.len()
            && delivery_utility(&candidates[relevance_order[end]]) == utility
        {
            end += 1;
        }
        ordered.extend(hierarchical_family_order(
            candidates,
            &relevance_order[start..end],
        ));
        start = end;
    }
    ordered
}

fn hierarchical_family_order(
    candidates: &[RecallDeliveryCandidate],
    eligible: &[usize],
) -> Vec<usize> {
    let mut families = BTreeMap::<String, Vec<usize>>::new();
    for index in eligible {
        let candidate = &candidates[*index];
        let family = delivery_candidate_family(candidate);
        families.entry(family).or_default().push(*index);
    }
    for members in families.values_mut() {
        members.sort_by(|left, right| {
            compare_delivery_utility(&candidates[*left], &candidates[*right])
        });
    }
    let mut family_order = families.keys().cloned().collect::<Vec<_>>();
    family_order.sort_by(|left, right| {
        let left_candidate = &candidates[families[left][0]];
        let right_candidate = &candidates[families[right][0]];
        compare_delivery_utility(left_candidate, right_candidate).then_with(|| left.cmp(right))
    });
    let mut ordered = Vec::with_capacity(eligible.len());
    let mut round = 0;
    loop {
        let mut added = false;
        for family in &family_order {
            if let Some(index) = families[family].get(round) {
                ordered.push(*index);
                added = true;
            }
        }
        if !added {
            break;
        }
        round += 1;
    }
    ordered
}

fn delivery_candidate_family(candidate: &RecallDeliveryCandidate) -> String {
    candidate
        .evidence_family_groups
        .first()
        .or_else(|| candidate.canonical_evidence_groups.first())
        .cloned()
        .unwrap_or_else(|| format!("candidate:{}", candidate.candidate_id))
}

fn compare_delivery_utility(
    left: &RecallDeliveryCandidate,
    right: &RecallDeliveryCandidate,
) -> std::cmp::Ordering {
    delivery_utility(right)
        .cmp(&delivery_utility(left))
        .then_with(|| left.reranked_rank.cmp(&right.reranked_rank))
        .then_with(|| left.candidate_id.cmp(&right.candidate_id))
}

fn delivery_utility(candidate: &RecallDeliveryCandidate) -> u64 {
    rank_utility(candidate.source_rank, 4)
        .saturating_add(rank_utility(candidate.expanded_rank, 3))
        .saturating_add(rank_utility(Some(candidate.reranked_rank), 2))
        .saturating_add(u64::from(candidate.relevance_score).saturating_mul(100))
        .saturating_add(u64::from(candidate.authority_score).saturating_mul(50))
}

fn rank_utility(rank: Option<usize>, weight: u64) -> u64 {
    rank.map(|rank| {
        weight
            .saturating_mul(1_000_000)
            .checked_div(60_u64.saturating_add(rank as u64))
            .unwrap_or(0)
    })
    .unwrap_or(0)
}

fn select_delivery_candidate(
    candidate: &RecallDeliveryCandidate,
    selected_candidate_ids: &mut Vec<String>,
    covered_groups: &mut BTreeSet<String>,
    covered_families: &mut BTreeSet<String>,
) {
    selected_candidate_ids.push(candidate.candidate_id.clone());
    covered_groups.extend(candidate.canonical_evidence_groups.iter().cloned());
    covered_families.extend(candidate.evidence_family_groups.iter().cloned());
}
