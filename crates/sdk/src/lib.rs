//! SDK runtime entrypoint for Beetle Memory.

use bm_core::{
    CrossPlanePlaneSignal, CrossPlaneRerankCandidate, CrossPlaneRerankReport, GovernanceReport,
    MemoryPlane, NewMemoryRecord, ProjectionBlock, ProjectionReport, ProjectionSurface,
    PromptRecallIntent, RecallPlaneReport, RecallQuery, RecallScoreBreakdown, RecallSelection,
    RecallSelectionReport, RecallSkipReason, RecallWarning, RuntimeProfile, SkippedRecallCandidate,
    SourceKind, SourceRef, WriteCandidate, WriteDecision, WriteRejectReason, WriteReport,
};
use bm_store::{MemoryStore, StoreError};

pub struct MemoryRuntimeBuilder {
    profile: RuntimeProfile,
}

impl MemoryRuntimeBuilder {
    pub fn new(profile: RuntimeProfile) -> Self {
        Self { profile }
    }

    pub fn store<S>(self, store: S) -> MemoryRuntimeBuilderWithStore<S>
    where
        S: MemoryStore,
    {
        MemoryRuntimeBuilderWithStore {
            profile: self.profile,
            store,
        }
    }
}

pub struct MemoryRuntimeBuilderWithStore<S> {
    profile: RuntimeProfile,
    store: S,
}

impl<S> MemoryRuntimeBuilderWithStore<S>
where
    S: MemoryStore,
{
    pub fn build(self) -> MemoryRuntime<S> {
        MemoryRuntime {
            profile: self.profile,
            store: self.store,
        }
    }
}

pub struct MemoryRuntime<S> {
    profile: RuntimeProfile,
    store: S,
}

impl<S> MemoryRuntime<S>
where
    S: MemoryStore,
{
    pub fn write(&mut self, candidate: WriteCandidate) -> WriteReport {
        let content = candidate.content.trim();
        if content.is_empty() {
            return WriteReport::rejected_with_reason(
                WriteRejectReason::EmptyContent,
                self.profile,
            );
        }

        let Some(source) = candidate.source.as_deref().map(str::trim) else {
            return WriteReport::rejected_with_reason(
                WriteRejectReason::MissingSource,
                self.profile,
            );
        };
        if source.is_empty() {
            return WriteReport::rejected_with_reason(
                WriteRejectReason::MissingSource,
                self.profile,
            );
        }

        let source_ref = source_ref_for(source);
        if looks_like_raw_payload_or_log(content) {
            return rejected_report(
                WriteRejectReason::RawPayloadOrLog,
                self.profile,
                None,
                Some(source_ref),
            );
        }

        let plane = candidate.plane_hint.unwrap_or_else(|| {
            if looks_like_procedural_memory(content) {
                MemoryPlane::Procedural
            } else {
                MemoryPlane::SharedFactual
            }
        });

        if source.starts_with("archive:") && matches!(plane, MemoryPlane::SharedFactual) {
            return rejected_report(
                WriteRejectReason::NeedsDistillation,
                self.profile,
                Some(plane),
                Some(source_ref),
            );
        }

        if !self.profile.allows_plane(plane) {
            return rejected_report(
                WriteRejectReason::ProfileRejected,
                self.profile,
                Some(plane),
                Some(source_ref),
            );
        }

        let record = match self.store.insert(NewMemoryRecord {
            identity: candidate.identity,
            scope: candidate.scope,
            content: content.to_owned(),
            source: source.to_owned(),
            domain: plane.domain(),
            plane,
        }) {
            Ok(record) => record,
            Err(err) => {
                return WriteReport {
                    decision: WriteDecision::Deferred,
                    domain: Some(plane.domain()),
                    plane: Some(plane),
                    record_id: None,
                    governance: GovernanceReport::new("store_unavailable")
                        .with_detail(store_error_detail(&err)),
                    source: Some(source_ref),
                    profile: Some(self.profile),
                };
            }
        };

        accepted_report(
            &record,
            self.profile,
            source_ref,
            governance_reason_for(plane, candidate.plane_hint),
        )
    }

    pub fn recall(&self, query: RecallQuery) -> RecallSelectionReport {
        let mut allowed = Vec::new();
        let mut skipped = Vec::new();
        let mut available_by_plane: Vec<(MemoryPlane, usize)> = MemoryPlaneSet::all()
            .into_iter()
            .map(|plane| (plane, 0))
            .collect();

        let records = match self.store.records() {
            Ok(records) => records,
            Err(err) => return recall_store_unavailable_report(self.profile, query, err),
        };

        for record in records {
            if record.scope != query.scope {
                continue;
            }
            increment_plane_count(&mut available_by_plane, record.plane);

            if query
                .identity
                .as_deref()
                .is_some_and(|identity| identity != record.identity)
            {
                skipped.push(SkippedRecallCandidate {
                    record_id: record.id,
                    plane: record.plane,
                    reason: RecallSkipReason::ScopeMismatch,
                });
                continue;
            }

            if !self.profile.allows_plane(record.plane) {
                skipped.push(SkippedRecallCandidate {
                    record_id: record.id,
                    plane: record.plane,
                    reason: RecallSkipReason::ProfileBudget,
                });
                continue;
            }

            if query.domain.is_some_and(|domain| domain != record.domain) {
                skipped.push(SkippedRecallCandidate {
                    record_id: record.id,
                    plane: record.plane,
                    reason: RecallSkipReason::DomainFiltered,
                });
                continue;
            }

            if query.plane.is_some_and(|plane| plane != record.plane) {
                skipped.push(SkippedRecallCandidate {
                    record_id: record.id,
                    plane: record.plane,
                    reason: RecallSkipReason::PlaneFiltered,
                });
                continue;
            }

            let mut selection = RecallSelection::from(record);
            selection.score = score_selection(query.intent, selection.plane);
            allowed.push(selection);
        }

        allowed.sort_by(|left, right| {
            right.score.total.cmp(&left.score.total).then_with(|| {
                plane_rank(query.intent, left.plane).cmp(&plane_rank(query.intent, right.plane))
            })
        });

        let mut selected = Vec::new();
        for selection in allowed {
            if selected.len() < query.limit {
                selected.push(selection);
            } else {
                skipped.push(SkippedRecallCandidate {
                    record_id: selection.record_id,
                    plane: selection.plane,
                    reason: RecallSkipReason::LimitReached,
                });
            }
        }

        let plane_reports = available_by_plane
            .into_iter()
            .filter(|(_, available)| *available > 0)
            .map(|(plane, available)| {
                let selected_count = selected
                    .iter()
                    .filter(|selection| selection.plane == plane)
                    .count();
                let skipped_count = skipped
                    .iter()
                    .filter(|candidate| candidate.plane == plane)
                    .count();
                RecallPlaneReport {
                    plane,
                    available,
                    selected: selected_count,
                    skipped: skipped_count,
                }
            })
            .collect::<Vec<_>>();

        let rerank = CrossPlaneRerankReport {
            intent: query.intent,
            top_planes: plane_reports
                .iter()
                .filter(|report| report.selected > 0)
                .map(|report| CrossPlanePlaneSignal {
                    plane: report.plane,
                    score: selected
                        .iter()
                        .filter(|selection| selection.plane == report.plane)
                        .map(|selection| selection.score.total)
                        .max()
                        .unwrap_or_default(),
                })
                .collect(),
            top_candidates: selected
                .iter()
                .map(|selection| CrossPlaneRerankCandidate {
                    record_id: selection.record_id.clone(),
                    plane: selection.plane,
                    score: selection.score.total,
                    source: selection.source.clone(),
                })
                .collect(),
        };

        let warnings = recall_warnings(self.profile, &selected);

        RecallSelectionReport {
            selected,
            skipped,
            profile: self.profile,
            query,
            plane_reports,
            rerank,
            warnings,
        }
    }

    pub fn project(
        &self,
        report: &RecallSelectionReport,
        surface: ProjectionSurface,
    ) -> ProjectionReport {
        let blocks = report
            .selected
            .iter()
            .map(|selection| ProjectionBlock {
                record_id: selection.record_id.clone(),
                domain: selection.domain,
                plane: selection.plane,
                content: project_content(selection),
                source: selection.source.clone(),
                privacy_filtered: selection.privacy_filtered,
            })
            .collect();

        ProjectionReport { surface, blocks }
    }
}

fn accepted_report(
    record: &bm_core::MemoryRecord,
    profile: RuntimeProfile,
    source: SourceRef,
    reason: &'static str,
) -> WriteReport {
    WriteReport {
        decision: WriteDecision::Accepted,
        domain: Some(record.domain),
        plane: Some(record.plane),
        record_id: Some(record.id.clone()),
        governance: GovernanceReport::new(reason).with_detail(report_detail(&source, profile)),
        source: Some(source),
        profile: Some(profile),
    }
}

fn rejected_report(
    reason: WriteRejectReason,
    profile: RuntimeProfile,
    plane: Option<MemoryPlane>,
    source: Option<SourceRef>,
) -> WriteReport {
    WriteReport {
        decision: WriteDecision::Rejected,
        domain: plane.map(MemoryPlane::domain),
        plane,
        record_id: None,
        governance: GovernanceReport::rejected(reason).with_detail(match &source {
            Some(source) => report_detail(source, profile),
            None => format!("profile={}", profile.as_str()),
        }),
        source,
        profile: Some(profile),
    }
}

fn report_detail(source: &SourceRef, profile: RuntimeProfile) -> String {
    format!("source={};profile={}", source.id, profile.as_str())
}

fn store_error_detail(err: &StoreError) -> String {
    match err.path.as_deref() {
        Some(path) => format!(
            "operation={};path={};recoverable={};message={}",
            err.operation.as_str(),
            path,
            err.recoverable,
            err.message
        ),
        None => format!(
            "operation={};recoverable={};message={}",
            err.operation.as_str(),
            err.recoverable,
            err.message
        ),
    }
}

fn recall_store_unavailable_report(
    profile: RuntimeProfile,
    query: RecallQuery,
    err: StoreError,
) -> RecallSelectionReport {
    let intent = query.intent;
    RecallSelectionReport {
        query,
        profile,
        selected: Vec::new(),
        skipped: Vec::new(),
        plane_reports: Vec::new(),
        rerank: CrossPlaneRerankReport::empty(intent),
        warnings: vec![RecallWarning::StoreUnavailable {
            operation: err.operation.as_str().to_owned(),
            message: store_error_detail(&err),
        }],
    }
}

fn source_ref_for(source: &str) -> SourceRef {
    let kind = if source.starts_with("archive:") {
        SourceKind::ArchiveEvidence
    } else if source.starts_with("task-learning") || source.starts_with("task:") {
        SourceKind::TaskLearning
    } else if source.starts_with("replay:") {
        SourceKind::ReplayFixture
    } else {
        SourceKind::AdapterEvent
    };
    SourceRef::new(kind, source.to_owned())
}

fn governance_reason_for(plane: MemoryPlane, plane_hint: Option<MemoryPlane>) -> &'static str {
    match (plane, plane_hint) {
        (MemoryPlane::Procedural, None) => "routed_to_procedural_memory",
        _ => "accepted",
    }
}

fn looks_like_raw_payload_or_log(content: &str) -> bool {
    let trimmed = content.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || trimmed.contains("stack backtrace:")
        || trimmed.contains("thread '")
        || trimmed.contains("Traceback (most recent call last)")
        || trimmed.contains("\nERROR")
        || trimmed.contains("\nWARN")
}

fn looks_like_procedural_memory(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    content.contains("下次")
        || content.contains("步骤")
        || (content.contains("先") && content.contains("再"))
        || lower.contains("next time")
        || (lower.contains("when ")
            && (lower.contains("first ") || lower.contains("then ") || lower.contains("run ")))
        || (lower.contains("first ") && lower.contains("then "))
}

fn score_selection(intent: PromptRecallIntent, plane: MemoryPlane) -> RecallScoreBreakdown {
    let intent_score = plane_intent_score(intent, plane);
    let total = 40 + intent_score;
    RecallScoreBreakdown {
        lexical: 10,
        semantic: 12,
        recency: 8,
        provenance: 10,
        intent: intent_score,
        total,
    }
}

fn plane_intent_score(intent: PromptRecallIntent, plane: MemoryPlane) -> u32 {
    match intent {
        PromptRecallIntent::Factual => match plane {
            MemoryPlane::SharedFactual => 50,
            MemoryPlane::ContinuityCapsule | MemoryPlane::ArchiveEvidence => 25,
            _ => 5,
        },
        PromptRecallIntent::Procedural => match plane {
            MemoryPlane::Procedural => 50,
            MemoryPlane::TaskRecall => 30,
            MemoryPlane::SharedFactual => 10,
            _ => 5,
        },
        PromptRecallIntent::Continuity => match plane {
            MemoryPlane::ContinuityCapsule => 50,
            MemoryPlane::SubjectProjection => 30,
            MemoryPlane::SharedFactual => 10,
            _ => 5,
        },
        PromptRecallIntent::Evidence => match plane {
            MemoryPlane::ArchiveEvidence => 50,
            MemoryPlane::SharedFactual | MemoryPlane::TaskRecall => 20,
            _ => 5,
        },
        PromptRecallIntent::Mixed => 20,
    }
}

fn plane_rank(intent: PromptRecallIntent, plane: MemoryPlane) -> u8 {
    match intent {
        PromptRecallIntent::Factual => match plane {
            MemoryPlane::SharedFactual => 0,
            MemoryPlane::ContinuityCapsule => 1,
            MemoryPlane::ArchiveEvidence => 2,
            _ => 9,
        },
        PromptRecallIntent::Procedural => match plane {
            MemoryPlane::Procedural => 0,
            MemoryPlane::TaskRecall => 1,
            MemoryPlane::SharedFactual => 2,
            _ => 9,
        },
        PromptRecallIntent::Continuity => match plane {
            MemoryPlane::ContinuityCapsule => 0,
            MemoryPlane::SubjectProjection => 1,
            MemoryPlane::SharedFactual => 2,
            _ => 9,
        },
        PromptRecallIntent::Evidence => match plane {
            MemoryPlane::ArchiveEvidence => 0,
            MemoryPlane::SharedFactual => 1,
            MemoryPlane::TaskRecall => 2,
            _ => 9,
        },
        PromptRecallIntent::Mixed => 0,
    }
}

fn increment_plane_count(counts: &mut [(MemoryPlane, usize)], plane: MemoryPlane) {
    if let Some((_, count)) = counts.iter_mut().find(|(candidate, _)| *candidate == plane) {
        *count += 1;
    }
}

fn recall_warnings(profile: RuntimeProfile, selected: &[RecallSelection]) -> Vec<RecallWarning> {
    let mut warnings = Vec::new();
    let projected_bytes: usize = selected
        .iter()
        .map(|selection| selection.content.len())
        .sum();
    let budget = profile.projection_budget_bytes();
    if projected_bytes > budget {
        warnings.push(RecallWarning::ProfileBudgetTrimmed {
            profile,
            before: projected_bytes,
            after: budget,
        });
    }
    for selection in selected {
        if matches!(selection.plane, MemoryPlane::ArchiveEvidence) {
            warnings.push(RecallWarning::EvidenceNotCanonical {
                record_id: selection.record_id.clone(),
            });
        }
        if selection.privacy_filtered {
            warnings.push(RecallWarning::PrivacyFiltered {
                plane: selection.plane,
            });
        }
    }
    warnings
}

fn project_content(selection: &RecallSelection) -> String {
    match selection.plane {
        MemoryPlane::Procedural => format!(
            "Procedural memory reference, not execution authority: {}",
            selection.content
        ),
        MemoryPlane::ArchiveEvidence => {
            format!(
                "Archive evidence reference, not canonical fact: {}",
                selection.content
            )
        }
        _ => selection.content.clone(),
    }
}

struct MemoryPlaneSet;

impl MemoryPlaneSet {
    fn all() -> [MemoryPlane; 7] {
        [
            MemoryPlane::SharedFactual,
            MemoryPlane::Procedural,
            MemoryPlane::ContinuityCapsule,
            MemoryPlane::ArchiveEvidence,
            MemoryPlane::TaskRecall,
            MemoryPlane::SubjectProjection,
            MemoryPlane::SoulGovernance,
        ]
    }
}
