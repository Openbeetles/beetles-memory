use crate::{
    canonicalize_long_term_draft, Confidence, EvidenceState, Freshness, LongTermMemoryDraft,
    LongTermMemoryKind, LongTermSlot, SourceKind, SourceRef,
};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LongTermExtractionState {
    pub pending_turn_id: Option<String>,
    pub last_processed_turn_id: Option<String>,
    pub cooldown_remaining: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParsedLongTermMemoryAction {
    Upsert(LongTermMemoryDraft),
    Delete(LongTermSlot),
    Ignore,
    RouteToProcedural { content: String, source: SourceRef },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArchiveReconcileReport {
    pub support_count: usize,
    pub conflict_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedLongTermExtraction {
    pub upserts: Vec<LongTermMemoryDraft>,
    pub deletes: Vec<LongTermSlot>,
    pub routed_to_procedural: Vec<String>,
    pub dropped_duplicates: usize,
    pub archive_reconcile: ArchiveReconcileReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongTermExtractionApplyReport {
    pub reports: Vec<crate::WriteReport>,
    pub deleted: usize,
    pub routed_to_procedural: usize,
    pub dropped_duplicates: usize,
}

pub fn parse_long_term_extraction_response(
    raw: &str,
    identity: &str,
    scope: &str,
    source: SourceRef,
) -> Vec<ParsedLongTermMemoryAction> {
    let Ok(items) = serde_json::from_str::<Vec<RawExtractionItem>>(raw) else {
        return vec![ParsedLongTermMemoryAction::Ignore];
    };
    items
        .into_iter()
        .map(|item| parse_item(item, identity, scope, &source))
        .collect()
}

pub fn prepare_long_term_extraction(
    actions: Vec<ParsedLongTermMemoryAction>,
) -> PreparedLongTermExtraction {
    let mut upserts = Vec::<LongTermMemoryDraft>::new();
    let mut deletes = Vec::<LongTermSlot>::new();
    let mut routed_to_procedural = Vec::new();
    let mut dropped_duplicates = 0;

    for action in actions {
        match action {
            ParsedLongTermMemoryAction::Upsert(draft) => {
                let draft = canonicalize_long_term_draft(draft);
                let slot = draft.slot().stable_id();
                if let Some(index) = upserts
                    .iter()
                    .position(|existing| existing.slot().stable_id() == slot)
                {
                    upserts[index] = draft;
                    dropped_duplicates += 1;
                } else {
                    upserts.push(draft);
                }
                deletes.retain(|delete| delete.stable_id() != slot);
            }
            ParsedLongTermMemoryAction::Delete(slot) => {
                if upserts
                    .iter()
                    .any(|draft| draft.slot().stable_id() == slot.stable_id())
                {
                    dropped_duplicates += 1;
                    continue;
                }
                if deletes
                    .iter()
                    .any(|existing| existing.stable_id() == slot.stable_id())
                {
                    dropped_duplicates += 1;
                } else {
                    deletes.push(slot);
                }
            }
            ParsedLongTermMemoryAction::RouteToProcedural { content, .. } => {
                if !content.trim().is_empty() {
                    routed_to_procedural.push(content);
                }
            }
            ParsedLongTermMemoryAction::Ignore => {}
        }
    }

    PreparedLongTermExtraction {
        upserts,
        deletes,
        routed_to_procedural,
        dropped_duplicates,
        archive_reconcile: ArchiveReconcileReport::default(),
    }
}

#[derive(Deserialize)]
struct RawExtractionItem {
    plane: Option<String>,
    op: Option<String>,
    kind: Option<String>,
    topic: Option<String>,
    content: Option<String>,
    keywords: Option<Vec<String>>,
}

fn parse_item(
    item: RawExtractionItem,
    identity: &str,
    scope: &str,
    source: &SourceRef,
) -> ParsedLongTermMemoryAction {
    match item.plane.as_deref().unwrap_or("ignore") {
        "factual" => parse_factual_item(item, identity, scope, source),
        "skill" => ParsedLongTermMemoryAction::RouteToProcedural {
            content: item.content.unwrap_or_default(),
            source: source.clone(),
        },
        _ => ParsedLongTermMemoryAction::Ignore,
    }
}

fn parse_factual_item(
    item: RawExtractionItem,
    identity: &str,
    scope: &str,
    source: &SourceRef,
) -> ParsedLongTermMemoryAction {
    let Some(kind) = item.kind.as_deref().and_then(parse_kind) else {
        return ParsedLongTermMemoryAction::Ignore;
    };
    let topic = item.topic.unwrap_or_default();
    if topic.trim().is_empty() {
        return ParsedLongTermMemoryAction::Ignore;
    }
    if item.op.as_deref() == Some("delete") {
        return ParsedLongTermMemoryAction::Delete(LongTermSlot::new(
            kind,
            identity.to_owned(),
            scope.to_owned(),
            topic,
        ));
    }
    let content = item.content.unwrap_or_default();
    if content.trim().is_empty() {
        return ParsedLongTermMemoryAction::Ignore;
    }
    ParsedLongTermMemoryAction::Upsert(LongTermMemoryDraft {
        kind,
        identity: identity.to_owned(),
        scope: scope.to_owned(),
        topic,
        content,
        keywords: item.keywords.unwrap_or_default(),
        source: SourceRef::new(SourceKind::LongTermExtraction, source.id.clone()),
        evidence: EvidenceState::Supported,
        confidence: Confidence::Medium,
        freshness: Freshness::Unknown,
        observed_at: None,
        canonical: true,
        archive_links: Vec::new(),
    })
}

fn parse_kind(value: &str) -> Option<LongTermMemoryKind> {
    match value {
        "preference" => Some(LongTermMemoryKind::Preference),
        "profile" => Some(LongTermMemoryKind::Profile),
        "relationship" => Some(LongTermMemoryKind::Relationship),
        "project" => Some(LongTermMemoryKind::Project),
        "task" => Some(LongTermMemoryKind::Task),
        "constraint" => Some(LongTermMemoryKind::Constraint),
        "fact" => Some(LongTermMemoryKind::Fact),
        _ => None,
    }
}
