//! SDK runtime entrypoint for Beetle Memory.

use bm_core::{
    archive_record_from_memory, build_archive_evidence_block, canonicalize_long_term_draft,
    inspect_long_term_merge, merge_long_term_record_meta, parse_long_term_extraction_response,
    prepare_long_term_extraction, search_archive_records, select_archive_hits_for_prompt,
    ArchiveEvidenceBlock, ArchiveSearchBackendKind, ArchiveSearchQuery, ArchiveSearchResult,
    Confidence, CrossPlanePlaneSignal, CrossPlaneRerankCandidate, CrossPlaneRerankReport,
    DisclosureSurface, EvidenceState, EvolutionInput, EvolutionProposal, EvolutionProposalBatch,
    Freshness, GovernanceReport, LongTermExtractionApplyReport, LongTermMemoryDraft,
    LongTermMemoryKind, LongTermWriteAction, MemoryPlane, MemoryRecord, MemoryRecordMeta,
    MentalPrivacyLayer, MentalPrivacyQuotePolicy, NewMemoryRecord, PrivacyDisclosureDecision,
    ProjectionBlock, ProjectionReport, ProjectionSurface, PromptRecallIntent, RecallPlaneReport,
    RecallQuery, RecallScoreBreakdown, RecallSelection, RecallSelectionReport, RecallSkipReason,
    RecallWarning, RuntimeProfile, SkippedRecallCandidate, SoulGovernanceReason, SourceKind,
    SourceRef, SubjectAssemblyReport, SubjectAssemblySource, SubjectAssemblySourceRef,
    WriteCandidate, WriteDecision, WriteRejectReason, WriteReport,
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
        let content = candidate.content.trim().to_owned();
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
        if looks_like_raw_payload_or_log(&content) {
            return rejected_report(
                WriteRejectReason::RawPayloadOrLog,
                self.profile,
                None,
                Some(source_ref),
            );
        }

        let plane = candidate.plane_hint.unwrap_or_else(|| {
            if looks_like_procedural_memory(&content) {
                MemoryPlane::Procedural
            } else {
                MemoryPlane::SharedFactual
            }
        });

        if matches!(
            candidate.privacy_layer,
            MentalPrivacyLayer::Private | MentalPrivacyLayer::Sealed
        ) {
            return rejected_report(
                WriteRejectReason::RawPrivateRejected,
                self.profile,
                Some(plane),
                Some(source_ref),
            );
        }

        if (source.starts_with("archive:")
            || matches!(candidate.evidence, EvidenceState::ArchiveOnly))
            && matches!(
                plane,
                MemoryPlane::SharedFactual | MemoryPlane::SoulGovernance
            )
        {
            return rejected_report(
                WriteRejectReason::NeedsDistillation,
                self.profile,
                Some(plane),
                Some(source_ref),
            );
        }

        if matches!(plane, MemoryPlane::SoulGovernance)
            && (source.starts_with("task-learning")
                || source.starts_with("task:")
                || looks_like_procedural_memory(&content))
        {
            return rejected_report(
                WriteRejectReason::NeedsDistillation,
                self.profile,
                Some(plane),
                Some(source_ref),
            );
        }

        if candidate.canonical
            && !matches!(
                candidate.evidence,
                EvidenceState::Supported | EvidenceState::Canonical
            )
        {
            return rejected_report(
                WriteRejectReason::WeakCanonicalStatement,
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

        if matches!(plane, MemoryPlane::SharedFactual) {
            return self.write_long_term(candidate, source_ref, content);
        }

        let meta = memory_meta_for_candidate(&candidate, plane, &content);
        let record = match self.store.insert(NewMemoryRecord {
            identity: candidate.identity,
            scope: candidate.scope,
            content,
            source: source.to_owned(),
            domain: plane.domain(),
            plane,
            meta,
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
                    long_term: None,
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

    fn write_long_term(
        &mut self,
        candidate: WriteCandidate,
        source_ref: SourceRef,
        content: String,
    ) -> WriteReport {
        let draft = canonicalize_long_term_draft(LongTermMemoryDraft {
            kind: candidate.long_term_kind.unwrap_or(LongTermMemoryKind::Fact),
            identity: candidate.identity.clone(),
            scope: candidate.scope.clone(),
            topic: candidate
                .topic
                .clone()
                .unwrap_or_else(|| first_words(&content, 8)),
            content: content.clone(),
            keywords: candidate.keywords.clone(),
            source: source_ref.clone(),
            evidence: candidate.evidence,
            confidence: candidate.confidence.unwrap_or(Confidence::Medium),
            freshness: candidate.freshness.unwrap_or(Freshness::Unknown),
            observed_at: candidate.observed_at,
            canonical: candidate.canonical
                || !matches!(candidate.evidence, EvidenceState::ArchiveOnly),
            archive_links: candidate.archive_links.clone(),
        });
        let slot_id = draft.slot().stable_id();
        let existing = match self.store.records() {
            Ok(records) => records
                .into_iter()
                .find(|record| record.meta.slot_id.as_deref() == Some(slot_id.as_str())),
            Err(err) => {
                return WriteReport {
                    decision: WriteDecision::Deferred,
                    domain: Some(MemoryPlane::SharedFactual.domain()),
                    plane: Some(MemoryPlane::SharedFactual),
                    record_id: None,
                    governance: GovernanceReport::new("store_unavailable")
                        .with_detail(store_error_detail(&err)),
                    source: Some(source_ref),
                    profile: Some(self.profile),
                    long_term: None,
                };
            }
        };
        let merge = inspect_long_term_merge(existing.as_ref(), &draft);
        if matches!(merge.action, LongTermWriteAction::Rejected) {
            return WriteReport {
                decision: WriteDecision::Rejected,
                domain: Some(MemoryPlane::SharedFactual.domain()),
                plane: Some(MemoryPlane::SharedFactual),
                record_id: existing.as_ref().map(|record| record.id.clone()),
                governance: GovernanceReport::new(merge.reason.as_str())
                    .with_detail(report_detail(&source_ref, self.profile)),
                source: Some(source_ref),
                profile: Some(self.profile),
                long_term: Some(merge),
            };
        }

        let updated_at = next_record_timestamp(existing.as_ref());
        let stored = match existing {
            Some(mut record) => {
                if !matches!(merge.action, LongTermWriteAction::Refreshed) {
                    record.content = content;
                }
                record.source = candidate.source.unwrap_or_else(|| source_ref.id.clone());
                merge_long_term_record_meta(&mut record, &draft);
                record.meta.updated_at = updated_at;
                match self.store.replace(record) {
                    Ok(record) => record,
                    Err(err) => {
                        return store_deferred_report(
                            self.profile,
                            MemoryPlane::SharedFactual,
                            source_ref,
                            err,
                        )
                    }
                }
            }
            None => {
                let meta = draft.clone().into_meta(updated_at);
                match self.store.insert(NewMemoryRecord {
                    identity: candidate.identity,
                    scope: candidate.scope,
                    content,
                    source: candidate.source.unwrap_or_else(|| source_ref.id.clone()),
                    domain: MemoryPlane::SharedFactual.domain(),
                    plane: MemoryPlane::SharedFactual,
                    meta,
                }) {
                    Ok(record) => record,
                    Err(err) => {
                        return store_deferred_report(
                            self.profile,
                            MemoryPlane::SharedFactual,
                            source_ref,
                            err,
                        )
                    }
                }
            }
        };
        let mut merge = merge;
        merge.new_record_id = Some(stored.id.clone());
        let decision = match merge.action {
            LongTermWriteAction::Inserted => WriteDecision::Accepted,
            LongTermWriteAction::Replaced
            | LongTermWriteAction::Merged
            | LongTermWriteAction::Refreshed => WriteDecision::Merged,
            LongTermWriteAction::Deleted => WriteDecision::Superseded,
            LongTermWriteAction::Rejected => WriteDecision::Rejected,
        };
        WriteReport {
            decision,
            domain: Some(stored.domain),
            plane: Some(stored.plane),
            record_id: Some(stored.id),
            governance: GovernanceReport::new(merge.reason.as_str())
                .with_detail(report_detail(&source_ref, self.profile)),
            source: Some(source_ref),
            profile: Some(self.profile),
            long_term: Some(merge),
        }
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
        let mut privacy_filtered_count = 0;
        let blocks = report
            .selected
            .iter()
            .map(|selection| ProjectionBlock {
                record_id: selection.record_id.clone(),
                domain: selection.domain,
                plane: selection.plane,
                content: trim_to_budget(
                    project_content(selection, surface),
                    report.profile.projection_budget_bytes(),
                ),
                source: selection.source.clone(),
                privacy_filtered: privacy_filtered_for_projection(selection, surface),
            })
            .inspect(|block| {
                if block.privacy_filtered {
                    privacy_filtered_count += 1;
                }
            })
            .collect();

        ProjectionReport {
            surface,
            subject_assembly: build_subject_assembly(report, surface),
            privacy_filtered_count,
            warnings: projection_warnings(report),
            blocks,
        }
    }

    pub fn propose_evolution(&self, mut input: EvolutionInput) -> EvolutionProposalBatch {
        input.profile = self.profile;
        bm_evolve::deterministic_evolve(input).batch
    }

    pub fn submit_evolution_proposal(&mut self, proposal: &EvolutionProposal) -> WriteReport {
        let Some(candidate) = proposal.candidate_write.clone() else {
            return WriteReport::rejected_with_reason(
                WriteRejectReason::NeedsDistillation,
                self.profile,
            );
        };

        self.write(candidate)
    }

    pub fn search_archive(&self, query: ArchiveSearchQuery) -> ArchiveSearchResult {
        let archive_records = self
            .store
            .records()
            .unwrap_or_default()
            .iter()
            .filter_map(archive_record_from_memory)
            .collect::<Vec<_>>();
        search_archive_records(
            &query,
            &archive_records,
            ArchiveSearchBackendKind::StoreScan,
        )
    }

    pub fn select_archive_for_prompt(&self, query: ArchiveSearchQuery) -> ArchiveEvidenceBlock {
        let result = self.search_archive(query);
        let selection = select_archive_hits_for_prompt(result.hits, self.profile);
        build_archive_evidence_block(result.report, selection)
    }

    pub fn prepare_long_term_extraction(
        &self,
        raw_json: &str,
        identity: &str,
        scope: &str,
        source: SourceRef,
    ) -> bm_core::PreparedLongTermExtraction {
        prepare_long_term_extraction(parse_long_term_extraction_response(
            raw_json, identity, scope, source,
        ))
    }

    pub fn apply_long_term_extraction(
        &mut self,
        prepared: bm_core::PreparedLongTermExtraction,
    ) -> LongTermExtractionApplyReport {
        let mut reports = Vec::new();
        let mut deleted = 0;
        for draft in prepared.upserts {
            let mut candidate = WriteCandidate::new(
                draft.identity.clone(),
                draft.scope.clone(),
                draft.content.clone(),
            )
            .source(draft.source.id.clone())
            .plane_hint(MemoryPlane::SharedFactual)
            .long_term_kind(draft.kind)
            .topic(draft.topic.clone())
            .keywords(draft.keywords.clone())
            .confidence(draft.confidence)
            .freshness(draft.freshness)
            .canonical(draft.canonical)
            .archive_links(draft.archive_links.clone());
            if let Some(observed_at) = draft.observed_at {
                candidate = candidate.observed_at(observed_at);
            }
            reports.push(self.write(candidate));
        }
        for slot in prepared.deletes {
            if let Ok(records) = self.store.records() {
                for record in records {
                    if record.meta.slot_id.as_deref() == Some(slot.stable_id().as_str())
                        && self.store.delete(&record.id).unwrap_or(false)
                    {
                        deleted += 1;
                    }
                }
            }
        }
        LongTermExtractionApplyReport {
            reports,
            deleted,
            routed_to_procedural: prepared.routed_to_procedural.len(),
            dropped_duplicates: prepared.dropped_duplicates,
        }
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
        long_term: None,
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
        long_term: None,
    }
}

fn store_deferred_report(
    profile: RuntimeProfile,
    plane: MemoryPlane,
    source: SourceRef,
    err: StoreError,
) -> WriteReport {
    WriteReport {
        decision: WriteDecision::Deferred,
        domain: Some(plane.domain()),
        plane: Some(plane),
        record_id: None,
        governance: GovernanceReport::new("store_unavailable")
            .with_detail(store_error_detail(&err)),
        source: Some(source),
        profile: Some(profile),
        long_term: None,
    }
}

fn memory_meta_for_candidate(
    candidate: &WriteCandidate,
    plane: MemoryPlane,
    content: &str,
) -> MemoryRecordMeta {
    let mut meta = MemoryRecordMeta::default_for_plane(plane);
    meta.evidence = candidate.evidence;
    meta.confidence = candidate.confidence.unwrap_or(Confidence::Medium);
    meta.freshness = candidate.freshness.unwrap_or(Freshness::Unknown);
    meta.canonical = candidate.canonical || !matches!(plane, MemoryPlane::ArchiveEvidence);
    meta.topic = candidate
        .topic
        .clone()
        .or_else(|| Some(first_words(content, 8)));
    meta.keywords = candidate.keywords.clone();
    meta.observed_at = candidate.observed_at;
    meta.archive_links = candidate.archive_links.clone();
    meta
}

fn next_record_timestamp(existing: Option<&MemoryRecord>) -> u64 {
    existing
        .map(|record| record.meta.updated_at.saturating_add(1))
        .unwrap_or(1)
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
    } else if source.starts_with("archive-import:") {
        SourceKind::ArchiveImport
    } else if source.starts_with("long-term-extraction:") {
        SourceKind::LongTermExtraction
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
            warnings.push(RecallWarning::ArchiveEvidenceNotCanonical {
                record_id: selection.record_id.clone(),
            });
        }
        if matches!(selection.meta.evidence, EvidenceState::Conflict) {
            warnings.push(RecallWarning::ArchiveConflict {
                record_id: selection.record_id.clone(),
            });
        }
        if matches!(selection.meta.freshness, Freshness::Stale) {
            warnings.push(RecallWarning::StaleLongTermMemory {
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

fn first_words(text: &str, max_words: usize) -> String {
    text.split_whitespace()
        .take(max_words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn project_content(selection: &RecallSelection, surface: ProjectionSurface) -> String {
    match (selection.plane, surface) {
        (MemoryPlane::SoulGovernance, ProjectionSurface::Prompt) => {
            "soul governance presence: governed summary available for subject assembly".to_owned()
        }
        (MemoryPlane::SoulGovernance, ProjectionSurface::ToolContext) => {
            "soul governance presence: use subject assembly report, not raw text".to_owned()
        }
        (MemoryPlane::SoulGovernance, ProjectionSurface::OperatorInspection) => {
            "soul governance presence: raw private material is not exposed by projection".to_owned()
        }
        (MemoryPlane::SubjectProjection, ProjectionSurface::OperatorInspection) => {
            "subject projection presence: current-turn frame available".to_owned()
        }
        (MemoryPlane::Procedural, _) => format!(
            "Procedural memory reference, not execution authority: {}",
            selection.content
        ),
        (MemoryPlane::ArchiveEvidence, _) => {
            format!(
                "Archive evidence reference, not canonical fact: {}",
                selection.content
            )
        }
        _ => selection.content.clone(),
    }
}

fn privacy_filtered_for_projection(
    selection: &RecallSelection,
    surface: ProjectionSurface,
) -> bool {
    selection.privacy_filtered
        || matches!(selection.plane, MemoryPlane::SoulGovernance)
        || matches!(
            (selection.plane, surface),
            (
                MemoryPlane::SubjectProjection,
                ProjectionSurface::OperatorInspection
            )
        )
}

fn build_subject_assembly(
    report: &RecallSelectionReport,
    surface: ProjectionSurface,
) -> Option<SubjectAssemblyReport> {
    let sources_used = report
        .selected
        .iter()
        .filter_map(subject_source_for_selection)
        .collect::<Vec<_>>();
    if sources_used.is_empty() {
        return None;
    }

    let privacy_decisions = report
        .selected
        .iter()
        .map(|selection| PrivacyDisclosureDecision {
            surface: disclosure_surface_for(surface),
            layer: privacy_layer_for_selection(selection),
            allowed: !privacy_filtered_for_projection(selection, surface),
            quote_policy: quote_policy_for_selection(selection, surface),
            reason: privacy_decision_reason(selection, surface),
        })
        .collect();

    Some(SubjectAssemblyReport {
        mounted: true,
        sources_missing: missing_subject_sources(&sources_used),
        sources_used,
        privacy_decisions,
        profile: report.profile,
        budget_bytes: report.profile.projection_budget_bytes(),
    })
}

fn subject_source_for_selection(selection: &RecallSelection) -> Option<SubjectAssemblySourceRef> {
    let source = match selection.plane {
        MemoryPlane::SoulGovernance => SubjectAssemblySource::SelfCore,
        MemoryPlane::SubjectProjection => SubjectAssemblySource::SelfContinuity,
        MemoryPlane::TaskRecall => SubjectAssemblySource::Task,
        MemoryPlane::Procedural
        | MemoryPlane::SharedFactual
        | MemoryPlane::ContinuityCapsule
        | MemoryPlane::ArchiveEvidence => SubjectAssemblySource::ProgramMemory,
    };
    Some(SubjectAssemblySourceRef {
        source,
        record_id: selection.record_id.clone(),
        plane: selection.plane,
        privacy_layer: privacy_layer_for_selection(selection),
    })
}

fn missing_subject_sources(
    sources_used: &[SubjectAssemblySourceRef],
) -> Vec<SubjectAssemblySource> {
    let mut missing = Vec::new();
    if !sources_used
        .iter()
        .any(|source| source.source == SubjectAssemblySource::SelfCore)
    {
        missing.push(SubjectAssemblySource::SelfCore);
    }
    if !sources_used
        .iter()
        .any(|source| source.source == SubjectAssemblySource::SelfContinuity)
    {
        missing.push(SubjectAssemblySource::SelfContinuity);
    }
    missing
}

fn privacy_decision_reason(
    selection: &RecallSelection,
    surface: ProjectionSurface,
) -> SoulGovernanceReason {
    if matches!(selection.plane, MemoryPlane::SoulGovernance) {
        return SoulGovernanceReason::PrivacyFiltered;
    }
    if matches!(
        (selection.plane, surface),
        (
            MemoryPlane::SubjectProjection,
            ProjectionSurface::OperatorInspection
        )
    ) {
        return SoulGovernanceReason::PrivacyFiltered;
    }
    if selection.privacy_filtered {
        return SoulGovernanceReason::PrivacyFiltered;
    }
    SoulGovernanceReason::StableIdentity
}

fn disclosure_surface_for(surface: ProjectionSurface) -> DisclosureSurface {
    match surface {
        ProjectionSurface::Prompt => DisclosureSurface::Prompt,
        ProjectionSurface::ToolContext => DisclosureSurface::ToolContext,
        ProjectionSurface::OperatorInspection => DisclosureSurface::OperatorInspection,
        ProjectionSurface::Adapter => DisclosureSurface::Adapter,
        ProjectionSurface::Replay => DisclosureSurface::Replay,
    }
}

fn privacy_layer_for_selection(selection: &RecallSelection) -> MentalPrivacyLayer {
    match selection.plane {
        MemoryPlane::SoulGovernance => MentalPrivacyLayer::Private,
        MemoryPlane::SubjectProjection => MentalPrivacyLayer::Relational,
        _ => MentalPrivacyLayer::Shared,
    }
}

fn quote_policy_for_selection(
    selection: &RecallSelection,
    surface: ProjectionSurface,
) -> MentalPrivacyQuotePolicy {
    if privacy_filtered_for_projection(selection, surface) {
        MentalPrivacyQuotePolicy::SummaryOnly
    } else {
        MentalPrivacyQuotePolicy::Raw
    }
}

fn projection_warnings(report: &RecallSelectionReport) -> Vec<String> {
    report
        .warnings
        .iter()
        .map(|warning| match warning {
            RecallWarning::ProfileBudgetTrimmed {
                profile,
                before,
                after,
            } => format!(
                "profile_budget_trimmed:profile={};before={};after={}",
                profile.as_str(),
                before,
                after
            ),
            RecallWarning::PrivacyFiltered { plane } => {
                format!("privacy_filtered:plane={}", plane.as_str())
            }
            RecallWarning::EvidenceNotCanonical { record_id } => {
                format!("evidence_not_canonical:record_id={record_id}")
            }
            RecallWarning::ArchiveEvidenceNotCanonical { record_id } => {
                format!("archive_evidence_not_canonical:record_id={record_id}")
            }
            RecallWarning::ArchiveConflict { record_id } => {
                format!("archive_conflict:record_id={record_id}")
            }
            RecallWarning::StaleLongTermMemory { record_id } => {
                format!("stale_long_term_memory:record_id={record_id}")
            }
            RecallWarning::StoreUnavailable { operation, message } => {
                format!("store_unavailable:operation={operation};message={message}")
            }
        })
        .collect()
}

fn trim_to_budget(content: String, budget: usize) -> String {
    if content.len() <= budget {
        return content;
    }

    let mut end = budget;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_owned()
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
