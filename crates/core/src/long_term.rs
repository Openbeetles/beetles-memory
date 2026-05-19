use crate::{
    ArchiveEvidenceLink, Confidence, EvidenceState, Freshness, MemoryPlane, MemoryRecord,
    MemoryRecordMeta, SourceRef,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum LongTermMemoryKind {
    Preference,
    Profile,
    Relationship,
    Project,
    Task,
    Constraint,
    Fact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LongTermWriteAction {
    Inserted,
    Replaced,
    Merged,
    Refreshed,
    Rejected,
    Deleted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LongTermWriteReason {
    DurableFact,
    SameSlotMerge,
    ArchiveSupported,
    ArchiveConflict,
    OlderThanExisting,
    LowerConfidenceThanExisting,
    WeakCanonicalStatement,
    NeedsDistillation,
    EmptyOrInvalid,
    RawPayloadOrLog,
    StructuredMaterial,
}

impl LongTermWriteReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DurableFact => "durable_fact",
            Self::SameSlotMerge => "same_slot_merge",
            Self::ArchiveSupported => "archive_supported",
            Self::ArchiveConflict => "archive_conflict",
            Self::OlderThanExisting => "older_than_existing",
            Self::LowerConfidenceThanExisting => "lower_confidence_than_existing",
            Self::WeakCanonicalStatement => "weak_canonical_statement",
            Self::NeedsDistillation => "needs_distillation",
            Self::EmptyOrInvalid => "empty_or_invalid",
            Self::RawPayloadOrLog => "raw_payload_or_log",
            Self::StructuredMaterial => "structured_material",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LongTermSlot {
    pub kind: LongTermMemoryKind,
    pub identity: String,
    pub scope: String,
    pub topic: String,
}

impl LongTermSlot {
    pub fn new(
        kind: LongTermMemoryKind,
        identity: impl Into<String>,
        scope: impl Into<String>,
        topic: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            identity: identity.into(),
            scope: scope.into(),
            topic: normalize_long_term_topic(&topic.into()),
        }
    }

    pub fn stable_id(&self) -> String {
        format!(
            "{:?}:{}:{}:{}",
            self.kind,
            normalize_slot_part(&self.identity),
            normalize_slot_part(&self.scope),
            normalize_long_term_topic(&self.topic)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongTermMemoryDraft {
    pub kind: LongTermMemoryKind,
    pub identity: String,
    pub scope: String,
    pub topic: String,
    pub content: String,
    pub keywords: Vec<String>,
    pub source: SourceRef,
    pub evidence: EvidenceState,
    pub confidence: Confidence,
    pub freshness: Freshness,
    pub observed_at: Option<u64>,
    pub canonical: bool,
    pub archive_links: Vec<ArchiveEvidenceLink>,
}

impl LongTermMemoryDraft {
    pub fn slot(&self) -> LongTermSlot {
        LongTermSlot::new(
            self.kind,
            self.identity.clone(),
            self.scope.clone(),
            self.topic.clone(),
        )
    }

    pub fn into_meta(self, updated_at: u64) -> MemoryRecordMeta {
        let slot = self.slot();
        let slot_id = slot.stable_id();
        MemoryRecordMeta {
            long_term_kind: Some(self.kind),
            topic: Some(slot.topic),
            keywords: self.keywords,
            evidence: self.evidence,
            confidence: self.confidence,
            freshness: self.freshness,
            canonical: self.canonical,
            slot_id: Some(slot_id),
            observed_at: self.observed_at,
            updated_at,
            archive_links: self.archive_links,
            procedural: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongTermMergeReport {
    pub action: LongTermWriteAction,
    pub reason: LongTermWriteReason,
    pub existing_record_id: Option<String>,
    pub new_record_id: Option<String>,
    pub slot: LongTermSlot,
    pub archive_support_count: usize,
    pub archive_conflict_count: usize,
}

pub fn normalize_long_term_topic(topic: &str) -> String {
    topic
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

pub fn canonicalize_long_term_draft(mut draft: LongTermMemoryDraft) -> LongTermMemoryDraft {
    draft.topic = normalize_long_term_topic(&draft.topic);
    draft.keywords = normalize_keywords(draft.keywords);
    if matches!(draft.evidence, EvidenceState::Canonical) {
        draft.canonical = true;
    }
    draft
}

pub fn inspect_long_term_merge(
    existing: Option<&MemoryRecord>,
    draft: &LongTermMemoryDraft,
) -> LongTermMergeReport {
    let slot = draft.slot();
    if matches!(draft.evidence, EvidenceState::ArchiveOnly) && draft.canonical {
        return merge_report(
            LongTermWriteAction::Rejected,
            LongTermWriteReason::NeedsDistillation,
            existing,
            &slot,
            draft,
        );
    }
    if draft.content.trim().is_empty() {
        return merge_report(
            LongTermWriteAction::Rejected,
            LongTermWriteReason::EmptyOrInvalid,
            existing,
            &slot,
            draft,
        );
    }
    let Some(existing) = existing else {
        return merge_report(
            LongTermWriteAction::Inserted,
            LongTermWriteReason::DurableFact,
            None,
            &slot,
            draft,
        );
    };
    if draft.confidence.rank() < existing.meta.confidence.rank() {
        return merge_report(
            LongTermWriteAction::Rejected,
            LongTermWriteReason::LowerConfidenceThanExisting,
            Some(existing),
            &slot,
            draft,
        );
    }
    if let (Some(incoming), Some(current)) = (draft.observed_at, existing.meta.observed_at) {
        if incoming < current {
            return merge_report(
                LongTermWriteAction::Rejected,
                LongTermWriteReason::OlderThanExisting,
                Some(existing),
                &slot,
                draft,
            );
        }
    }
    if existing.content.trim() == draft.content.trim() {
        let reason = if draft.archive_links.iter().any(|link| link.supports) {
            LongTermWriteReason::ArchiveSupported
        } else {
            LongTermWriteReason::SameSlotMerge
        };
        return merge_report(
            LongTermWriteAction::Refreshed,
            reason,
            Some(existing),
            &slot,
            draft,
        );
    }
    let reason = if draft.archive_links.iter().any(|link| !link.supports) {
        LongTermWriteReason::ArchiveConflict
    } else {
        LongTermWriteReason::SameSlotMerge
    };
    merge_report(
        LongTermWriteAction::Replaced,
        reason,
        Some(existing),
        &slot,
        draft,
    )
}

pub fn merge_long_term_record_meta(existing: &mut MemoryRecord, draft: &LongTermMemoryDraft) {
    let mut keywords = existing.meta.keywords.clone();
    keywords.extend(draft.keywords.iter().cloned());
    existing.meta.keywords = normalize_keywords(keywords);
    existing.meta.evidence = draft.evidence;
    existing.meta.confidence = draft.confidence;
    existing.meta.freshness = draft.freshness;
    existing.meta.canonical = draft.canonical;
    existing.meta.observed_at = draft.observed_at.or(existing.meta.observed_at);
    existing
        .meta
        .archive_links
        .extend(draft.archive_links.clone());
    existing.meta.long_term_kind = Some(draft.kind);
    existing.meta.topic = Some(normalize_long_term_topic(&draft.topic));
    existing.meta.slot_id = Some(draft.slot().stable_id());
}

pub fn slot_id_for_record(record: &MemoryRecord) -> Option<&str> {
    record.meta.slot_id.as_deref()
}

pub fn default_long_term_kind_for_plane(plane: MemoryPlane) -> Option<LongTermMemoryKind> {
    match plane {
        MemoryPlane::SharedFactual => Some(LongTermMemoryKind::Fact),
        _ => None,
    }
}

fn merge_report(
    action: LongTermWriteAction,
    reason: LongTermWriteReason,
    existing: Option<&MemoryRecord>,
    slot: &LongTermSlot,
    draft: &LongTermMemoryDraft,
) -> LongTermMergeReport {
    LongTermMergeReport {
        action,
        reason,
        existing_record_id: existing.map(|record| record.id.clone()),
        new_record_id: None,
        slot: slot.clone(),
        archive_support_count: draft
            .archive_links
            .iter()
            .filter(|link| link.supports)
            .count(),
        archive_conflict_count: draft
            .archive_links
            .iter()
            .filter(|link| !link.supports)
            .count(),
    }
}

fn normalize_keywords(keywords: Vec<String>) -> Vec<String> {
    let mut out = keywords
        .into_iter()
        .map(|keyword| normalize_long_term_topic(&keyword))
        .filter(|keyword| !keyword.is_empty())
        .collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

fn normalize_slot_part(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}
