use crate::{MemoryPlane, RuntimeProfile, SourceKind, SourceRef};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ArchiveRecordSource {
    Transcript,
    DailyNote,
    TurnLog,
    Import,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ArchiveSearchBackendKind {
    InMemoryHybrid,
    StoreScan,
    SqliteHybrid,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ArchiveRecordLocator {
    pub source: ArchiveRecordSource,
    pub scope: String,
    pub record_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArchiveEvidenceLink {
    pub locator: ArchiveRecordLocator,
    pub supports: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArchiveRecord {
    pub id: String,
    pub locator: ArchiveRecordLocator,
    pub title: String,
    pub content: String,
    pub cues: Vec<String>,
    pub observed_at: Option<u64>,
    pub source_ref: SourceRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveSearchQuery {
    pub scope: String,
    pub text: String,
    pub sources: Vec<ArchiveRecordSource>,
    pub limit: usize,
    pub profile: RuntimeProfile,
}

impl ArchiveSearchQuery {
    pub fn new(scope: impl Into<String>, text: impl Into<String>, profile: RuntimeProfile) -> Self {
        Self {
            scope: scope.into(),
            text: text.into(),
            sources: Vec::new(),
            limit: 8,
            profile,
        }
    }

    pub fn sources(mut self, sources: Vec<ArchiveRecordSource>) -> Self {
        self.sources = sources;
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveSearchScoreBreakdown {
    pub lexical: u32,
    pub cue: u32,
    pub recency: u32,
    pub source: u32,
    pub total: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveRetrievalTrace {
    pub backend: ArchiveSearchBackendKind,
    pub score: ArchiveSearchScoreBreakdown,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveSearchHit {
    pub record_id: String,
    pub locator: ArchiveRecordLocator,
    pub title: String,
    pub excerpt: String,
    pub score: ArchiveSearchScoreBreakdown,
    pub trace: ArchiveRetrievalTrace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveSearchSourceStats {
    pub source: ArchiveRecordSource,
    pub candidates: usize,
    pub hits: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveSearchQueryReport {
    pub query_text: String,
    pub requested_sources: Vec<ArchiveRecordSource>,
    pub backend: ArchiveSearchBackendKind,
    pub candidates: usize,
    pub hits: usize,
    pub source_stats: Vec<ArchiveSearchSourceStats>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveSearchResult {
    pub hits: Vec<ArchiveSearchHit>,
    pub report: ArchiveSearchQueryReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivePromptSelectionReport {
    pub selected: usize,
    pub skipped_by_similarity: usize,
    pub deferred_by_quota: usize,
    pub relaxed_quota_selected: usize,
    pub selection_note: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivePromptSelection {
    pub hits: Vec<ArchiveSearchHit>,
    pub report: ArchivePromptSelectionReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveEvidenceBlock {
    pub lines: Vec<String>,
    pub query_report: ArchiveSearchQueryReport,
    pub selection_report: ArchivePromptSelectionReport,
}

pub fn archive_source_from_ref(source: &SourceRef) -> ArchiveRecordSource {
    if source.id.starts_with("archive:transcript") {
        ArchiveRecordSource::Transcript
    } else if source.id.starts_with("archive:daily") {
        ArchiveRecordSource::DailyNote
    } else if source.id.starts_with("archive:turn") {
        ArchiveRecordSource::TurnLog
    } else if matches!(source.kind, SourceKind::ArchiveImport) {
        ArchiveRecordSource::Import
    } else {
        ArchiveRecordSource::External
    }
}

pub fn archive_record_from_memory(record: &crate::MemoryRecord) -> Option<ArchiveRecord> {
    if record.plane != MemoryPlane::ArchiveEvidence {
        return None;
    }
    let source_ref = SourceRef::new(SourceKind::ArchiveEvidence, record.source.clone());
    let source = archive_source_from_ref(&source_ref);
    Some(ArchiveRecord {
        id: record.id.clone(),
        locator: ArchiveRecordLocator {
            source,
            scope: record.scope.clone(),
            record_id: record.id.clone(),
        },
        title: record
            .meta
            .topic
            .clone()
            .unwrap_or_else(|| first_words(&record.content, 8)),
        content: record.content.clone(),
        cues: record.meta.keywords.clone(),
        observed_at: record.meta.observed_at,
        source_ref,
    })
}

pub fn search_archive_records(
    query: &ArchiveSearchQuery,
    records: &[ArchiveRecord],
    backend: ArchiveSearchBackendKind,
) -> ArchiveSearchResult {
    let terms = tokenize(&query.text);
    let mut candidate_by_source = HashMap::<ArchiveRecordSource, usize>::new();
    let mut hit_by_source = HashMap::<ArchiveRecordSource, usize>::new();
    let mut hits = Vec::new();

    for record in records {
        if record.locator.scope != query.scope {
            continue;
        }
        if !query.sources.is_empty() && !query.sources.contains(&record.locator.source) {
            continue;
        }
        *candidate_by_source
            .entry(record.locator.source)
            .or_default() += 1;
        let (score, reasons) = score_archive_record(record, &terms);
        if score.total == 0 {
            continue;
        }
        *hit_by_source.entry(record.locator.source).or_default() += 1;
        hits.push(ArchiveSearchHit {
            record_id: record.id.clone(),
            locator: record.locator.clone(),
            title: record.title.clone(),
            excerpt: first_words(&record.content, 24),
            score: score.clone(),
            trace: ArchiveRetrievalTrace {
                backend,
                score,
                reasons,
            },
        });
    }

    hits.sort_by(|left, right| right.score.total.cmp(&left.score.total));
    if hits.len() > query.limit {
        hits.truncate(query.limit);
    }

    let source_stats = source_stats(&query.sources, &candidate_by_source, &hit_by_source);
    ArchiveSearchResult {
        report: ArchiveSearchQueryReport {
            query_text: query.text.clone(),
            requested_sources: query.sources.clone(),
            backend,
            candidates: candidate_by_source.values().sum(),
            hits: hits.len(),
            source_stats,
            warnings: Vec::new(),
        },
        hits,
    }
}

pub fn select_archive_hits_for_prompt(
    hits: Vec<ArchiveSearchHit>,
    profile: RuntimeProfile,
) -> ArchivePromptSelection {
    let max_items = match profile {
        RuntimeProfile::EspCompact | RuntimeProfile::SdkEmbedded => 2,
        RuntimeProfile::LinuxDevice => 4,
        RuntimeProfile::DesktopMacos
        | RuntimeProfile::DesktopWindows
        | RuntimeProfile::ServerLinux => 6,
        RuntimeProfile::SdkFull | RuntimeProfile::MemoryGateway | RuntimeProfile::DevFull => 8,
    };
    let budget = profile.projection_budget_bytes();
    let mut selected = Vec::new();
    let mut seen_similarity = HashSet::<String>::new();
    let mut per_source = HashMap::<ArchiveRecordSource, usize>::new();
    let mut skipped_by_similarity = 0;
    let mut deferred_by_quota = 0;
    let mut used_bytes = 0;

    for hit in hits {
        if selected.len() >= max_items {
            deferred_by_quota += 1;
            continue;
        }
        let key = similarity_key(&hit.excerpt);
        if !seen_similarity.insert(key) {
            skipped_by_similarity += 1;
            continue;
        }
        let used = per_source.entry(hit.locator.source).or_default();
        if *used >= source_quota(profile, hit.locator.source) {
            deferred_by_quota += 1;
            continue;
        }
        let projected_len = hit.excerpt.len() + hit.title.len() + 8;
        if used_bytes + projected_len > budget {
            deferred_by_quota += 1;
            continue;
        }
        *used += 1;
        used_bytes += projected_len;
        selected.push(hit);
    }

    ArchivePromptSelection {
        report: ArchivePromptSelectionReport {
            selected: selected.len(),
            skipped_by_similarity,
            deferred_by_quota,
            relaxed_quota_selected: 0,
            selection_note: if selected.is_empty() {
                Some("no_archive_evidence_selected".to_owned())
            } else {
                Some("archive_selector_primary_pass".to_owned())
            },
        },
        hits: selected,
    }
}

pub fn build_archive_evidence_block(
    search_report: ArchiveSearchQueryReport,
    selection: ArchivePromptSelection,
) -> ArchiveEvidenceBlock {
    let lines = selection
        .hits
        .iter()
        .map(|hit| {
            format!(
                "- {:?}: {} [{}]",
                hit.locator.source, hit.excerpt, hit.locator.record_id
            )
        })
        .collect();
    ArchiveEvidenceBlock {
        lines,
        query_report: search_report,
        selection_report: selection.report,
    }
}

fn source_quota(profile: RuntimeProfile, source: ArchiveRecordSource) -> usize {
    match (profile, source) {
        (RuntimeProfile::EspCompact | RuntimeProfile::SdkEmbedded, _) => 1,
        (_, ArchiveRecordSource::Transcript) => 3,
        (_, ArchiveRecordSource::DailyNote) => 2,
        (_, ArchiveRecordSource::TurnLog) => 2,
        (_, ArchiveRecordSource::Import | ArchiveRecordSource::External) => 2,
    }
}

fn score_archive_record(
    record: &ArchiveRecord,
    terms: &[String],
) -> (ArchiveSearchScoreBreakdown, Vec<String>) {
    if terms.is_empty() {
        return (
            ArchiveSearchScoreBreakdown {
                lexical: 0,
                cue: 0,
                recency: 0,
                source: 0,
                total: 0,
            },
            Vec::new(),
        );
    }
    let haystack = format!(
        "{} {} {}",
        record.title,
        record.content,
        record.cues.join(" ")
    )
    .to_ascii_lowercase();
    let lexical_matches = terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count() as u32;
    let cue_matches = terms
        .iter()
        .filter(|term| {
            record
                .cues
                .iter()
                .any(|cue| cue.to_ascii_lowercase().contains(term.as_str()))
        })
        .count() as u32;
    let lexical = lexical_matches * 10;
    let cue = cue_matches * 8;
    let recency = record.observed_at.map(|_| 3).unwrap_or_default();
    let source = match record.locator.source {
        ArchiveRecordSource::Transcript => 4,
        ArchiveRecordSource::DailyNote => 3,
        ArchiveRecordSource::TurnLog => 2,
        ArchiveRecordSource::Import | ArchiveRecordSource::External => 1,
    };
    let total = lexical + cue + recency + source;
    let mut reasons = Vec::new();
    if lexical > 0 {
        reasons.push("lexical_match".to_owned());
    }
    if cue > 0 {
        reasons.push("cue_match".to_owned());
    }
    (
        ArchiveSearchScoreBreakdown {
            lexical,
            cue,
            recency,
            source,
            total,
        },
        reasons,
    )
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn source_stats(
    requested: &[ArchiveRecordSource],
    candidates: &HashMap<ArchiveRecordSource, usize>,
    hits: &HashMap<ArchiveRecordSource, usize>,
) -> Vec<ArchiveSearchSourceStats> {
    let mut sources = requested.to_vec();
    if sources.is_empty() {
        sources.extend(candidates.keys().copied());
    }
    sources.sort_by_key(|source| *source as u8);
    sources.dedup();
    sources
        .into_iter()
        .map(|source| ArchiveSearchSourceStats {
            source,
            candidates: candidates.get(&source).copied().unwrap_or_default(),
            hits: hits.get(&source).copied().unwrap_or_default(),
        })
        .collect()
}

fn first_words(text: &str, max_words: usize) -> String {
    text.split_whitespace()
        .take(max_words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn similarity_key(text: &str) -> String {
    tokenize(text)
        .into_iter()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
}
