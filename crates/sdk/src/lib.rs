//! SDK runtime entrypoint for Beetle Memory.

use bm_core::{
    archive_record_from_memory, build_archive_evidence_block, canonicalize_long_term_draft,
    inspect_long_term_merge, merge_long_term_record_meta, parse_long_term_extraction_response,
    parse_procedural_skill_import_envelope, prepare_long_term_extraction,
    procedural_skill_meta_from_draft, procedural_skill_slot_id, score_procedural_skill_record,
    search_archive_records, select_archive_hits_for_prompt, ArchiveEvidenceBlock,
    ArchiveSearchBackendKind, ArchiveSearchQuery, ArchiveSearchResult, Confidence,
    CrossPlanePlaneSignal, CrossPlaneRerankCandidate, CrossPlaneRerankReport, DisclosureSurface,
    EvidenceState, EvolutionInput, EvolutionProposal, EvolutionProposalBatch, Freshness,
    GovernanceReport, LongTermExtractionApplyReport, LongTermMemoryDraft, LongTermMemoryKind,
    LongTermWriteAction, MemoryPlane, MemoryRecord, MemoryRecordMeta, MentalPrivacyLayer,
    MentalPrivacyQuotePolicy, NewMemoryRecord, PrivacyDisclosureDecision, ProceduralEvidenceRef,
    ProceduralSkillDraft, ProceduralSkillImportReport, ProceduralSkillOrigin,
    ProceduralSkillRecallCandidate, ProceduralSkillRecallQuery, ProceduralSkillRecallReport,
    ProceduralSkillReuseOutcome, ProceduralSkillSkippedCandidate, ProceduralSkillState,
    ProceduralSkillStrategyDiff, ProceduralSkillWriteAction, ProceduralSkillWriteReason,
    ProceduralSkillWriteReport, ProjectionBlock, ProjectionReport, ProjectionSanitizerReport,
    ProjectionSurface, PromptAssemblyBudgetReport, PromptAssemblyBudgetSlice, PromptAssemblyGroups,
    PromptAssemblyReport, PromptContextBlock, PromptContextGroup, PromptRecallIntent,
    PromptRecallRouterDecision, PromptRecallRouterSignal, RecallAssemblyRequest,
    RecallPlaneExecutionReport, RecallPlaneReport, RecallQuery, RecallScoreBreakdown,
    RecallSelection, RecallSelectionReport, RecallSkipReason, RecallWarning, RuntimeProfile,
    SkippedRecallCandidate, SoulGovernanceReason, SourceKind, SourceRef, SubjectAssemblyReport,
    SubjectAssemblySource, SubjectAssemblySourceRef, WriteCandidate, WriteDecision,
    WriteRejectReason, WriteReport,
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

        if matches!(plane, MemoryPlane::SharedFactual) && looks_like_procedural_memory(&content) {
            return rejected_report(
                WriteRejectReason::RoutedToProcedural,
                self.profile,
                Some(MemoryPlane::Procedural),
                Some(source_ref),
            );
        }

        if matches!(plane, MemoryPlane::SharedFactual) {
            return self.write_long_term(candidate, source_ref, content);
        }

        if matches!(plane, MemoryPlane::Procedural) {
            let draft = procedural_draft_from_candidate(&candidate, &source_ref, &content);
            return self.write_procedural_skill(draft);
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
                    procedural: None,
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

    pub fn write_procedural_skill(&mut self, draft: ProceduralSkillDraft) -> WriteReport {
        let imported = draft.provenance.imported;
        let state = if imported {
            ProceduralSkillState::Quarantined
        } else {
            match draft.origin {
                bm_core::ProceduralSkillOrigin::UserProvided => ProceduralSkillState::Active,
                bm_core::ProceduralSkillOrigin::RuntimeLearned => ProceduralSkillState::Candidate,
            }
        };
        self.write_procedural_skill_with_state(draft, state)
    }

    fn write_procedural_skill_with_state(
        &mut self,
        draft: ProceduralSkillDraft,
        state: ProceduralSkillState,
    ) -> WriteReport {
        let source_ref = procedural_source_ref(&draft);
        let slot_id = procedural_skill_slot_id(&draft);
        let inspection = bm_core::inspect_procedural_skill_draft(&draft);
        if !inspection.accepted {
            return procedural_rejected_report(
                self.profile,
                source_ref,
                slot_id,
                inspection.reason,
                inspection.detail,
            );
        }
        if !self.profile.allows_plane(MemoryPlane::Procedural) {
            return procedural_rejected_report(
                self.profile,
                source_ref,
                slot_id,
                ProceduralSkillWriteReason::ProfileRejected,
                "profile rejected procedural plane",
            );
        }

        let now = draft.observed_at.unwrap_or(1);
        let mut meta = MemoryRecordMeta::default_for_plane(MemoryPlane::Procedural);
        meta.topic = Some(draft.trigger.clone());
        meta.keywords = draft
            .trigger
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        meta.canonical = true;
        meta.slot_id = Some(slot_id.clone());
        meta.observed_at = draft.observed_at;
        meta.updated_at = now;
        let procedural_meta = procedural_skill_meta_from_draft(&draft, state, now);
        meta.procedural = Some(procedural_meta);

        let existing = match self.store.records() {
            Ok(records) => records.into_iter().find(|record| {
                record.plane == MemoryPlane::Procedural
                    && record.meta.slot_id.as_deref() == Some(slot_id.as_str())
            }),
            Err(err) => {
                return store_deferred_report(
                    self.profile,
                    MemoryPlane::Procedural,
                    source_ref,
                    err,
                )
            }
        };

        let mut action = match state {
            ProceduralSkillState::Quarantined => ProceduralSkillWriteAction::Quarantined,
            _ => ProceduralSkillWriteAction::Inserted,
        };
        let record = match existing {
            Some(mut existing) => {
                let existing_meta = existing.meta.procedural.clone();
                let incoming_meta = meta.procedural.clone();
                let existing_quality = existing_meta
                    .as_ref()
                    .map(|meta| meta.quality_score)
                    .unwrap_or_default();
                let incoming_quality = incoming_meta
                    .as_ref()
                    .map(|meta| meta.quality_score)
                    .unwrap_or_default();
                let existing_digest = existing_meta
                    .as_ref()
                    .and_then(|meta| meta.lineage.last())
                    .map(|node| node.strategy_digest.clone());
                let incoming_digest = incoming_meta
                    .as_ref()
                    .and_then(|meta| meta.lineage.last())
                    .map(|node| node.strategy_digest.clone());
                let existing_state = existing_meta
                    .as_ref()
                    .map(|meta| meta.state)
                    .unwrap_or(ProceduralSkillState::Candidate);
                if existing_state == ProceduralSkillState::Active
                    && state == ProceduralSkillState::Quarantined
                {
                    return procedural_rejected_report(
                        self.profile,
                        source_ref,
                        slot_id,
                        ProceduralSkillWriteReason::LowerQualityRejected,
                        "trusted local procedural skill preserved over quarantined import",
                    );
                }
                if incoming_quality < existing_quality {
                    return procedural_rejected_report(
                        self.profile,
                        source_ref,
                        slot_id,
                        ProceduralSkillWriteReason::LowerQualityRejected,
                        "lower quality procedural skill rejected",
                    );
                }
                if incoming_digest == existing_digest {
                    action = ProceduralSkillWriteAction::Refreshed;
                } else if incoming_quality >= existing_quality {
                    action = ProceduralSkillWriteAction::Superseded;
                } else {
                    action = ProceduralSkillWriteAction::Merged;
                }
                if let (Some(mut next_meta), Some(previous_meta)) =
                    (meta.procedural.clone(), existing_meta)
                {
                    if incoming_digest != existing_digest {
                        let from_node_id = previous_meta
                            .lineage
                            .last()
                            .map(|node| node.node_id.clone())
                            .unwrap_or_else(|| existing.id.clone());
                        let to_node_id = next_meta
                            .lineage
                            .last()
                            .map(|node| node.node_id.clone())
                            .unwrap_or_else(|| draft.trigger.clone());
                        next_meta.strategy_diffs = previous_meta.strategy_diffs;
                        next_meta.strategy_diffs.push(ProceduralSkillStrategyDiff {
                            recorded_at: now,
                            from_node_id,
                            to_node_id,
                            summary: "procedure strategy changed under the same trigger".to_owned(),
                        });
                        next_meta.supersedes = previous_meta.supersedes.clone();
                        if !next_meta.supersedes.contains(&existing.id) {
                            next_meta.supersedes.push(existing.id.clone());
                        }
                    }
                    let incoming_lineage = next_meta.lineage.clone();
                    next_meta.lineage = previous_meta.lineage.clone();
                    for node in incoming_lineage {
                        if !next_meta
                            .lineage
                            .iter()
                            .any(|existing| existing.node_id == node.node_id)
                        {
                            next_meta.lineage.push(node);
                        }
                    }
                    next_meta.use_count = previous_meta.use_count;
                    next_meta.validated_success_count = previous_meta.validated_success_count;
                    next_meta.mismatch_count = previous_meta.mismatch_count;
                    next_meta.revision_count = previous_meta.revision_count;
                    next_meta.revision_pending = previous_meta.revision_pending;
                    next_meta.last_used_at = previous_meta.last_used_at;
                    next_meta.last_outcome_at = previous_meta.last_outcome_at;
                    next_meta.last_outcome_note = previous_meta.last_outcome_note;
                    next_meta.quality_score = bm_core::compute_procedural_skill_quality(&next_meta);
                    meta.procedural = Some(next_meta);
                }
                existing.content = draft.procedure.clone();
                existing.source = source_ref.id.clone();
                existing.meta = meta;
                match self.store.replace(existing) {
                    Ok(record) => record,
                    Err(err) => {
                        return store_deferred_report(
                            self.profile,
                            MemoryPlane::Procedural,
                            source_ref,
                            err,
                        )
                    }
                }
            }
            None => match self.store.insert(NewMemoryRecord {
                identity: draft.identity.clone(),
                scope: draft.scope.clone(),
                content: draft.procedure.clone(),
                source: source_ref.id.clone(),
                domain: MemoryPlane::Procedural.domain(),
                plane: MemoryPlane::Procedural,
                meta,
            }) {
                Ok(record) => record,
                Err(err) => {
                    return store_deferred_report(
                        self.profile,
                        MemoryPlane::Procedural,
                        source_ref,
                        err,
                    )
                }
            },
        };

        let procedural = procedural_write_report(
            action,
            state,
            draft.origin,
            slot_id,
            record
                .meta
                .procedural
                .as_ref()
                .map(|meta| meta.quality_score)
                .unwrap_or_default(),
        );
        WriteReport {
            decision: if action == ProceduralSkillWriteAction::Rejected {
                WriteDecision::Rejected
            } else if matches!(
                action,
                ProceduralSkillWriteAction::Merged
                    | ProceduralSkillWriteAction::Refreshed
                    | ProceduralSkillWriteAction::Superseded
            ) {
                WriteDecision::Merged
            } else {
                WriteDecision::Accepted
            },
            domain: Some(MemoryPlane::Procedural.domain()),
            plane: Some(MemoryPlane::Procedural),
            record_id: Some(record.id),
            governance: GovernanceReport::new(procedural.reason.as_str())
                .with_detail(report_detail(&source_ref, self.profile)),
            source: Some(source_ref),
            profile: Some(self.profile),
            long_term: None,
            procedural: Some(procedural),
        }
    }

    pub fn import_procedural_skill(
        &mut self,
        envelope_json: &str,
        adjudicated: bool,
    ) -> ProceduralSkillImportReport {
        let envelope = match parse_procedural_skill_import_envelope(envelope_json) {
            Ok(envelope) => envelope,
            Err(error) => {
                return ProceduralSkillImportReport {
                    rejected: 1,
                    reports: vec![ProceduralSkillWriteReport {
                        action: ProceduralSkillWriteAction::Rejected,
                        reason: ProceduralSkillWriteReason::EmptyOrInvalid,
                        state: ProceduralSkillState::Quarantined,
                        slot_id: String::new(),
                        quality_score: 0,
                        detail: error,
                    }],
                    ..ProceduralSkillImportReport::default()
                }
            }
        };
        let state = if adjudicated {
            ProceduralSkillState::Active
        } else {
            ProceduralSkillState::Quarantined
        };
        let write = self.write_procedural_skill_with_state(envelope.draft, state);
        let mut report = ProceduralSkillImportReport::default();
        if let Some(item) = write.procedural {
            match item.action {
                ProceduralSkillWriteAction::Rejected => report.rejected += 1,
                ProceduralSkillWriteAction::Quarantined => {
                    report.imported += 1;
                    report.quarantined += 1;
                }
                _ if state == ProceduralSkillState::Active => {
                    report.imported += 1;
                    report.adopted += 1;
                }
                _ => report.imported += 1,
            }
            report.reports.push(item);
        }
        report
    }

    pub fn record_procedural_skill_outcome(
        &mut self,
        record_ids: &[String],
        outcome: ProceduralSkillReuseOutcome,
        now_secs: u64,
        note: &str,
    ) -> bm_core::ProceduralSkillOutcomeReport {
        let mut report = bm_core::ProceduralSkillOutcomeReport {
            submitted: record_ids.len(),
            ..bm_core::ProceduralSkillOutcomeReport::default()
        };
        let records = match self.store.records() {
            Ok(records) => records,
            Err(_) => {
                report.missing = record_ids.len();
                return report;
            }
        };
        for record_id in record_ids {
            let Some(mut record) = records
                .iter()
                .find(|record| &record.id == record_id)
                .cloned()
            else {
                report.missing += 1;
                continue;
            };
            let Some(mut procedural) = record.meta.procedural.clone() else {
                report.missing += 1;
                continue;
            };
            procedural.use_count = procedural.use_count.saturating_add(1);
            match outcome {
                ProceduralSkillReuseOutcome::Neutral => {}
                ProceduralSkillReuseOutcome::Succeeded => {
                    procedural.validated_success_count =
                        procedural.validated_success_count.saturating_add(1);
                    procedural.state = ProceduralSkillState::Active;
                    procedural.revision_pending = false;
                }
                ProceduralSkillReuseOutcome::Mismatch => {
                    procedural.mismatch_count = procedural.mismatch_count.saturating_add(1);
                    procedural.revision_count = procedural.revision_count.saturating_add(1);
                    procedural.revision_pending = true;
                }
            }
            procedural.last_used_at = Some(now_secs);
            procedural.last_outcome_at = Some(now_secs);
            procedural.last_outcome_note = trim_to_budget(note.trim().to_owned(), 160);
            procedural.quality_score = bm_core::compute_procedural_skill_quality(&procedural);
            record.meta.procedural = Some(procedural);
            record.meta.updated_at = now_secs.max(record.meta.updated_at);
            if self.store.replace(record).is_ok() {
                report.updated += 1;
            } else {
                report.missing += 1;
            }
        }
        report
    }

    pub fn recall_procedural_skills(
        &self,
        query: ProceduralSkillRecallQuery,
    ) -> ProceduralSkillRecallReport {
        let records = match self.store.records() {
            Ok(records) => records,
            Err(err) => {
                return ProceduralSkillRecallReport {
                    query: Some(query),
                    backend: "store_scan".to_owned(),
                    warnings: vec![store_error_detail(&err)],
                    ..ProceduralSkillRecallReport::default()
                }
            }
        };
        let mut candidates = Vec::new();
        let mut skipped = Vec::new();
        let mut warnings = Vec::new();
        for record in records
            .into_iter()
            .filter(|record| record.scope == query.scope && record.plane == MemoryPlane::Procedural)
        {
            let Some(meta) = record.meta.procedural.as_ref() else {
                skipped.push(ProceduralSkillSkippedCandidate {
                    record_id: record.id,
                    state: ProceduralSkillState::Deprecated,
                    reason: "missing_procedural_meta".to_owned(),
                });
                continue;
            };
            if !matches!(meta.state, ProceduralSkillState::Active) {
                warnings.push(format!("{:?} procedural skill skipped", meta.state));
                skipped.push(ProceduralSkillSkippedCandidate {
                    record_id: record.id,
                    state: meta.state,
                    reason: format!("{:?}", meta.state),
                });
                continue;
            }
            let score = score_procedural_skill_record(&record, &query.query, &query.scope);
            candidates.push(ProceduralSkillRecallCandidate {
                record_id: record.id,
                title: record
                    .meta
                    .topic
                    .clone()
                    .unwrap_or_else(|| meta.trigger.clone()),
                trigger: meta.trigger.clone(),
                procedure: record.content,
                state: meta.state,
                origin: meta.origin,
                quality_score: meta.quality_score,
                validated_success_count: meta.validated_success_count,
                score,
            });
        }
        let candidate_count = candidates.len() + skipped.len();
        candidates.sort_by(|left, right| {
            right
                .score
                .total_score
                .cmp(&left.score.total_score)
                .then_with(|| right.quality_score.cmp(&left.quality_score))
                .then_with(|| left.record_id.cmp(&right.record_id))
        });
        candidates.truncate(query.limit);
        let selected_ids = candidates
            .iter()
            .map(|candidate| candidate.record_id.clone())
            .collect::<Vec<_>>();
        ProceduralSkillRecallReport {
            query: Some(query),
            backend: "store_scan".to_owned(),
            candidate_count,
            selected_count: candidates.len(),
            selected_ids,
            miss_reason: candidates
                .is_empty()
                .then(|| "no_active_procedural_skill_candidates".to_owned()),
            selection_note: (!candidates.is_empty())
                .then(|| "procedural_skill_selected".to_owned()),
            selected: candidates,
            skipped,
            warnings,
        }
    }

    pub fn project_procedural_skills(
        &self,
        report: &ProceduralSkillRecallReport,
        surface: ProjectionSurface,
    ) -> ProjectionReport {
        let blocks = report
            .selected
            .iter()
            .map(|candidate| ProjectionBlock {
                record_id: candidate.record_id.clone(),
                domain: MemoryPlane::Procedural.domain(),
                plane: MemoryPlane::Procedural,
                content: trim_to_budget(
                    format!(
                        "Procedural skill hint, not execution authority: [{}] when {}; do {}; quality={}; state={:?}",
                        candidate.title,
                        candidate.trigger,
                        candidate.procedure,
                        candidate.quality_score,
                        candidate.state
                    ),
                    self.profile.projection_budget_bytes(),
                ),
                source: SourceRef::new(SourceKind::AdapterEvent, "procedural-skill".to_owned()),
                privacy_filtered: false,
            })
            .collect();
        ProjectionReport {
            surface,
            blocks,
            privacy_filtered_count: 0,
            subject_assembly: None,
            warnings: report.warnings.clone(),
        }
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
                    procedural: None,
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
                procedural: None,
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
            procedural: None,
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

            if let Some(identity) = query.identity.as_deref() {
                if identity != record.identity {
                    skipped.push(SkippedRecallCandidate {
                        record_id: record.id,
                        plane: record.plane,
                        reason: RecallSkipReason::ScopeMismatch,
                        reason_fragments: vec![format!("identity_mismatch:expected={identity}")],
                    });
                    continue;
                }
            }

            if !self.profile.allows_plane(record.plane) {
                skipped.push(SkippedRecallCandidate {
                    record_id: record.id,
                    plane: record.plane,
                    reason: RecallSkipReason::ProfileBudget,
                    reason_fragments: vec![format!(
                        "profile_rejected:profile={};plane={}",
                        self.profile.as_str(),
                        record.plane.as_str()
                    )],
                });
                continue;
            }

            if record.plane == MemoryPlane::Procedural
                && !record
                    .meta
                    .procedural
                    .as_ref()
                    .is_some_and(|meta| meta.state == ProceduralSkillState::Active)
            {
                skipped.push(SkippedRecallCandidate {
                    record_id: record.id,
                    plane: record.plane,
                    reason: RecallSkipReason::PrivacyPolicy,
                    reason_fragments: vec!["procedural_skill_not_active".to_owned()],
                });
                continue;
            }

            if query.domain.is_some_and(|domain| domain != record.domain) {
                skipped.push(SkippedRecallCandidate {
                    record_id: record.id,
                    plane: record.plane,
                    reason: RecallSkipReason::DomainFiltered,
                    reason_fragments: vec!["domain_filter_mismatch".to_owned()],
                });
                continue;
            }

            if query.plane.is_some_and(|plane| plane != record.plane) {
                skipped.push(SkippedRecallCandidate {
                    record_id: record.id,
                    plane: record.plane,
                    reason: RecallSkipReason::PlaneFiltered,
                    reason_fragments: vec!["plane_filter_mismatch".to_owned()],
                });
                continue;
            }

            let mut selection = RecallSelection::from(record);
            selection.score = score_selection(query.intent, selection.plane);
            selection
                .reason_fragments
                .push(format!("intent={:?}", query.intent));
            selection
                .reason_fragments
                .push(format!("plane={}", selection.plane.as_str()));
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
                let mut reason_fragments = selection.reason_fragments;
                reason_fragments.push(format!("limit_reached:limit={}", query.limit));
                skipped.push(SkippedRecallCandidate {
                    record_id: selection.record_id,
                    plane: selection.plane,
                    reason: RecallSkipReason::LimitReached,
                    reason_fragments,
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
                let top_selection = selected
                    .iter()
                    .filter(|selection| selection.plane == plane)
                    .max_by_key(|selection| selection.score.total);
                RecallPlaneReport {
                    plane,
                    available,
                    selected: selected_count,
                    skipped: skipped_count,
                    top_score: top_selection.map(|selection| selection.score.total),
                    top_reason: top_selection.map(|selection| selection.reason_fragments.join(";")),
                }
            })
            .collect::<Vec<_>>();

        let warnings = recall_warnings(self.profile, &selected);
        let rerank = CrossPlaneRerankReport {
            intent: query.intent,
            top_planes: plane_reports
                .iter()
                .filter(|report| report.selected > 0)
                .map(|report| CrossPlanePlaneSignal {
                    plane: report.plane,
                    score: report.top_score.unwrap_or_default(),
                    candidate_count: report.available,
                    selected_count: report.selected,
                    top_reason: report.top_reason.clone(),
                })
                .collect(),
            top_candidates: selected
                .iter()
                .map(|selection| CrossPlaneRerankCandidate {
                    record_id: selection.record_id.clone(),
                    plane: selection.plane,
                    selected: true,
                    original_score: selection.score.total,
                    rerank_score: selection.score.total,
                    score: selection.score.total,
                    source: selection.source.clone(),
                    reason_fragments: {
                        let mut reasons = selection.reason_fragments.clone();
                        reasons.push(format!("rerank:intent={:?}", query.intent));
                        reasons
                    },
                })
                .collect(),
            skipped_candidates: skipped.clone(),
            warnings: recall_warning_messages(&warnings),
        };

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

    pub fn assemble_recall(&self, mut request: RecallAssemblyRequest) -> PromptAssemblyReport {
        request.profile = self.profile;
        request.limits.max_bytes = request.limits.max_bytes.min(
            self.profile
                .projection_budget_profile(request.surface)
                .total_bytes,
        );
        let intent = decide_assembly_intent(&request);
        let mut plane_reports = Vec::new();
        for plane in MemoryPlaneSet::all() {
            plane_reports.push(self.recall_plane_for_assembly(&request, intent, plane));
        }
        let router = build_router_decision(&request, intent, &plane_reports);
        let selected = selected_for_assembly(&plane_reports, intent, request.limits.max_blocks);
        let rerank = rerank_for_assembly(intent, &selected, &plane_reports);
        let budget_profile = self.profile.projection_budget_profile(request.surface);
        let mut sanitizer = ProjectionSanitizerReport::new(request.surface);
        let mut context_blocks = Vec::new();
        let mut blocks = Vec::new();
        let mut groups = PromptAssemblyGroups::default();
        let active_seed = active_context_text(&request);
        push_group_text(
            &mut groups.active_task_context,
            sanitize_projection_text(&active_seed, &request, &mut sanitizer),
        );

        for selection in selected {
            let group = group_for_selection(&selection);
            let projected = project_content_for_context(&selection, request.surface);
            let sanitized = sanitize_projection_text(&projected, &request, &mut sanitizer);
            if sanitized.trim().is_empty() {
                continue;
            }
            let content = trim_to_budget(sanitized, budget_profile.block_bytes);
            let block = ProjectionBlock {
                record_id: selection.record_id.clone(),
                domain: selection.domain,
                plane: selection.plane,
                content,
                source: selection.source.clone(),
                privacy_filtered: privacy_filtered_for_projection(&selection, request.surface)
                    || surface_requires_report_first(selection.plane, request.surface),
            };
            push_group_text(group_text_mut(&mut groups, group), block.content.clone());
            context_blocks.push(PromptContextBlock {
                group,
                projection: block.clone(),
            });
            blocks.push(block);
        }

        let budget = normalize_assembly_groups(&mut groups, &budget_profile);
        let privacy_filtered_count = blocks.iter().filter(|block| block.privacy_filtered).count();
        let mut warnings = assembly_warnings(
            &plane_reports,
            &budget,
            &sanitizer,
            request.surface,
            privacy_filtered_count,
        );
        if blocks.is_empty() {
            warnings.push("prompt_assembly:no_projection_blocks".to_owned());
        }

        PromptAssemblyReport {
            surface: request.surface,
            profile: self.profile,
            request,
            router,
            rerank,
            plane_reports,
            groups,
            context_blocks,
            blocks,
            budget,
            sanitizer,
            privacy_filtered_count,
            warnings,
        }
    }

    pub fn inspect_recall(
        &self,
        request: RecallAssemblyRequest,
    ) -> bm_core::WorkingRecallInspectionReport {
        let assembly = self.assemble_recall(request);
        let selected = assembly
            .plane_reports
            .iter()
            .map(|report| report.selected_count)
            .sum();
        let skipped = assembly
            .plane_reports
            .iter()
            .map(|report| report.skipped_count)
            .sum();
        let warnings = assembly.warnings.clone();
        bm_core::WorkingRecallInspectionReport {
            assembly,
            selected,
            skipped,
            warnings,
        }
    }

    pub fn project_context(&self, request: RecallAssemblyRequest) -> ProjectionReport {
        let assembly = self.assemble_recall(request);
        let mut warnings = assembly.warnings.clone();
        warnings.push(format!(
            "prompt_assembly:intent={:?};planes={};sanitized={}",
            assembly.router.intent,
            assembly.plane_reports.len(),
            assembly.sanitizer.redacted_fragments
        ));
        ProjectionReport {
            surface: assembly.surface,
            blocks: assembly.blocks,
            privacy_filtered_count: assembly.privacy_filtered_count,
            subject_assembly: None,
            warnings,
        }
    }

    pub fn propose_evolution(&self, mut input: EvolutionInput) -> EvolutionProposalBatch {
        input.profile = self.profile;
        bm_evolve::deterministic_evolve(input).batch
    }

    fn recall_plane_for_assembly(
        &self,
        request: &RecallAssemblyRequest,
        intent: PromptRecallIntent,
        plane: MemoryPlane,
    ) -> RecallPlaneExecutionReport {
        let query = request.recall_query(intent, plane);
        if !self.profile.allows_plane(plane) {
            return RecallPlaneExecutionReport {
                plane,
                query: query.clone(),
                backend: "profile_gate".to_owned(),
                candidate_count: 0,
                selected_count: 0,
                selected_ids: Vec::new(),
                skipped_count: 0,
                miss_reason: Some("profile_rejected".to_owned()),
                selection_note: None,
                warnings: vec![format!("profile_rejected:plane={}", plane.as_str())],
                recall: RecallSelectionReport {
                    query,
                    profile: self.profile,
                    selected: Vec::new(),
                    skipped: Vec::new(),
                    plane_reports: Vec::new(),
                    rerank: CrossPlaneRerankReport::empty(intent),
                    warnings: Vec::new(),
                },
            };
        }
        let recall = self.recall(query.clone());
        let plane_summary = recall
            .plane_reports
            .iter()
            .find(|report| report.plane == plane);
        let candidate_count = plane_summary.map(|report| report.available).unwrap_or(0);
        let selected_count = recall.selected.len();
        let skipped_count = recall.skipped.len();
        let selected_ids = recall
            .selected
            .iter()
            .map(|selection| selection.record_id.clone())
            .collect::<Vec<_>>();
        let miss_reason = (selected_count == 0).then(|| {
            if candidate_count == 0 {
                "no_candidate_for_plane".to_owned()
            } else {
                "no_selected_candidate_for_plane".to_owned()
            }
        });
        let selection_note =
            (selected_count > 0).then(|| "selected_for_prompt_assembly".to_owned());
        let warnings = projection_warnings(&recall);
        RecallPlaneExecutionReport {
            plane,
            query,
            backend: "store_scan".to_owned(),
            candidate_count,
            selected_count,
            selected_ids,
            skipped_count,
            miss_reason,
            selection_note,
            warnings,
            recall,
        }
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
        procedural: None,
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
        procedural: None,
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
        procedural: None,
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

fn procedural_source_ref(draft: &ProceduralSkillDraft) -> SourceRef {
    let source = draft
        .provenance
        .source_ref
        .clone()
        .unwrap_or_else(|| match draft.origin {
            ProceduralSkillOrigin::UserProvided => "procedural:user-provided".to_owned(),
            ProceduralSkillOrigin::RuntimeLearned => "procedural:runtime-learned".to_owned(),
        });
    source_ref_for(&source)
}

fn procedural_draft_from_candidate(
    candidate: &WriteCandidate,
    source: &SourceRef,
    content: &str,
) -> ProceduralSkillDraft {
    let title = candidate
        .topic
        .clone()
        .unwrap_or_else(|| first_words(content, 6));
    let origin = if matches!(
        source.kind,
        SourceKind::TaskLearning | SourceKind::ReplayFixture
    ) {
        ProceduralSkillOrigin::RuntimeLearned
    } else {
        ProceduralSkillOrigin::UserProvided
    };
    let mut draft = ProceduralSkillDraft::new(
        candidate.identity.clone(),
        candidate.scope.clone(),
        origin,
        title.clone(),
        title,
        content.to_owned(),
    );
    draft.provenance.source_ref = Some(source.id.clone());
    if matches!(
        source.kind,
        SourceKind::TaskLearning | SourceKind::ReplayFixture
    ) {
        draft.evidence = vec![ProceduralEvidenceRef::new(
            source.id.clone(),
            "procedural write candidate source",
        )];
    }
    draft
}

fn procedural_rejected_report(
    profile: RuntimeProfile,
    source: SourceRef,
    slot_id: String,
    reason: ProceduralSkillWriteReason,
    detail: impl Into<String>,
) -> WriteReport {
    let procedural = ProceduralSkillWriteReport {
        action: ProceduralSkillWriteAction::Rejected,
        reason,
        state: ProceduralSkillState::Candidate,
        slot_id,
        quality_score: 0,
        detail: detail.into(),
    };
    WriteReport {
        decision: WriteDecision::Rejected,
        domain: Some(MemoryPlane::Procedural.domain()),
        plane: Some(MemoryPlane::Procedural),
        record_id: None,
        governance: GovernanceReport::new(reason.as_str())
            .with_detail(report_detail(&source, profile)),
        source: Some(source),
        profile: Some(profile),
        long_term: None,
        procedural: Some(procedural),
    }
}

fn procedural_write_report(
    action: ProceduralSkillWriteAction,
    state: ProceduralSkillState,
    origin: ProceduralSkillOrigin,
    slot_id: String,
    quality_score: u8,
) -> ProceduralSkillWriteReport {
    let reason = match action {
        ProceduralSkillWriteAction::Quarantined => {
            ProceduralSkillWriteReason::ImportedRequiresAdjudication
        }
        ProceduralSkillWriteAction::Rejected => ProceduralSkillWriteReason::WeakProcedure,
        _ => match origin {
            ProceduralSkillOrigin::UserProvided => ProceduralSkillWriteReason::UserProvidedAccepted,
            ProceduralSkillOrigin::RuntimeLearned => {
                ProceduralSkillWriteReason::RuntimeEvidenceAccepted
            }
        },
    };
    ProceduralSkillWriteReport {
        action,
        reason,
        state,
        slot_id,
        quality_score,
        detail: format!("state={state:?};quality={quality_score}"),
    }
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

fn recall_warning_messages(warnings: &[RecallWarning]) -> Vec<String> {
    warnings
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
        (MemoryPlane::Procedural, _) => {
            let trigger = selection
                .meta
                .procedural
                .as_ref()
                .map(|meta| meta.trigger.as_str())
                .unwrap_or("procedural memory");
            let quality = selection
                .meta
                .procedural
                .as_ref()
                .map(|meta| meta.quality_score)
                .unwrap_or_default();
            format!(
                "Procedural skill hint, not execution authority: when {}; do {}; quality={}",
                trigger, selection.content, quality
            )
        }
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

fn decide_assembly_intent(request: &RecallAssemblyRequest) -> PromptRecallIntent {
    if let Some(intent) = request.intent_hint {
        return intent;
    }
    if request.exact_lookup {
        return PromptRecallIntent::Factual;
    }
    let query = request.normalized_query.to_ascii_lowercase();
    if contains_any(
        &query,
        &["原文", "日志", "证据", "archive", "evidence", "trace"],
    ) {
        return PromptRecallIntent::Evidence;
    }
    if contains_any(
        &query,
        &[
            "流程",
            "步骤",
            "怎么",
            "如何",
            "how to",
            "procedure",
            "runbook",
        ],
    ) {
        return PromptRecallIntent::Procedural;
    }
    if request.active_context.active_task.is_some() || structurally_weak_query(&query) {
        return PromptRecallIntent::Continuity;
    }
    PromptRecallIntent::Mixed
}

fn build_router_decision(
    request: &RecallAssemblyRequest,
    intent: PromptRecallIntent,
    reports: &[RecallPlaneExecutionReport],
) -> PromptRecallRouterDecision {
    let signals = reports
        .iter()
        .map(|report| {
            let selected_score = report
                .recall
                .selected
                .iter()
                .map(|selection| selection.score.total)
                .max()
                .unwrap_or_default();
            let context_bonus = match (intent, report.plane) {
                (PromptRecallIntent::Continuity, MemoryPlane::ContinuityCapsule)
                    if request.active_context.active_task.is_some() =>
                {
                    24
                }
                (PromptRecallIntent::Continuity, MemoryPlane::TaskRecall) => 12,
                (PromptRecallIntent::Procedural, MemoryPlane::Procedural) => 16,
                (PromptRecallIntent::Evidence, MemoryPlane::ArchiveEvidence) => 16,
                _ => 0,
            };
            let score = selected_score.saturating_add(context_bonus);
            let reason = if report.selected_count > 0 {
                format!(
                    "selected={};backend={};intent={:?}",
                    report.selected_count, report.backend, intent
                )
            } else {
                report
                    .miss_reason
                    .clone()
                    .unwrap_or_else(|| "no_signal".to_owned())
            };
            PromptRecallRouterSignal {
                plane: report.plane,
                score,
                reason,
            }
        })
        .collect::<Vec<_>>();
    let mut decision = PromptRecallRouterDecision::new(intent, router_reason(request, intent));
    decision.signals = signals;
    decision
}

fn router_reason(request: &RecallAssemblyRequest, intent: PromptRecallIntent) -> String {
    match intent {
        PromptRecallIntent::Factual => "exact_or_factual_query".to_owned(),
        PromptRecallIntent::Procedural => "procedural_query_signal".to_owned(),
        PromptRecallIntent::Continuity => {
            if request.active_context.active_task.is_some() {
                "active_context_or_weak_query".to_owned()
            } else {
                "weak_continuation_query".to_owned()
            }
        }
        PromptRecallIntent::Evidence => "evidence_query_signal".to_owned(),
        PromptRecallIntent::Mixed => "mixed_query_signal".to_owned(),
    }
}

fn selected_for_assembly(
    reports: &[RecallPlaneExecutionReport],
    intent: PromptRecallIntent,
    max_blocks: usize,
) -> Vec<RecallSelection> {
    let mut selected = reports
        .iter()
        .flat_map(|report| report.recall.selected.iter().cloned())
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        assembly_candidate_score(right, intent)
            .cmp(&assembly_candidate_score(left, intent))
            .then_with(|| plane_rank(intent, left.plane).cmp(&plane_rank(intent, right.plane)))
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    selected.truncate(max_blocks);
    selected
}

fn rerank_for_assembly(
    intent: PromptRecallIntent,
    selected: &[RecallSelection],
    reports: &[RecallPlaneExecutionReport],
) -> CrossPlaneRerankReport {
    let top_planes = reports
        .iter()
        .filter(|report| report.selected_count > 0)
        .map(|report| CrossPlanePlaneSignal {
            plane: report.plane,
            score: report
                .recall
                .selected
                .iter()
                .map(|selection| assembly_candidate_score(selection, intent))
                .max()
                .unwrap_or_default(),
            candidate_count: report.candidate_count,
            selected_count: report.selected_count,
            top_reason: report
                .recall
                .selected
                .iter()
                .max_by_key(|selection| assembly_candidate_score(selection, intent))
                .map(|selection| selection.reason_fragments.join(";")),
        })
        .collect::<Vec<_>>();
    let top_candidates = selected
        .iter()
        .map(|selection| CrossPlaneRerankCandidate {
            record_id: selection.record_id.clone(),
            plane: selection.plane,
            selected: true,
            original_score: selection.score.total,
            rerank_score: assembly_candidate_score(selection, intent),
            score: assembly_candidate_score(selection, intent),
            source: selection.source.clone(),
            reason_fragments: {
                let mut reasons = selection.reason_fragments.clone();
                reasons.push(format!("assembly_rerank:intent={intent:?}"));
                reasons
            },
        })
        .collect();
    CrossPlaneRerankReport {
        intent,
        top_planes,
        top_candidates,
        skipped_candidates: reports
            .iter()
            .flat_map(|report| report.recall.skipped.iter().cloned())
            .collect(),
        warnings: reports
            .iter()
            .flat_map(|report| report.warnings.iter().cloned())
            .collect(),
    }
}

fn assembly_candidate_score(selection: &RecallSelection, intent: PromptRecallIntent) -> u32 {
    selection
        .score
        .total
        .saturating_add(plane_intent_score(intent, selection.plane))
}

fn active_context_text(request: &RecallAssemblyRequest) -> String {
    let mut lines = Vec::new();
    if let Some(active_task) = request.active_context.active_task.as_deref() {
        lines.push(format!("active_task: {active_task}"));
    }
    if let Some(summary) = request.active_context.summary.as_deref() {
        lines.push(format!("summary: {summary}"));
    }
    if let Some(recent_grounding) = request.active_context.recent_grounding.as_deref() {
        lines.push(format!("recent_grounding: {recent_grounding}"));
    }
    lines.join("\n")
}

fn group_for_selection(selection: &RecallSelection) -> PromptContextGroup {
    match selection.plane {
        MemoryPlane::SoulGovernance => PromptContextGroup::ConstitutionalStack,
        MemoryPlane::ContinuityCapsule
        | MemoryPlane::TaskRecall
        | MemoryPlane::SubjectProjection => PromptContextGroup::ActiveTaskContext,
        MemoryPlane::SharedFactual | MemoryPlane::ArchiveEvidence | MemoryPlane::Procedural => {
            PromptContextGroup::GovernedMemoryEvidence
        }
    }
}

fn group_text_mut(
    groups: &mut PromptAssemblyGroups,
    group: PromptContextGroup,
) -> &mut Option<String> {
    match group {
        PromptContextGroup::ConstitutionalStack => &mut groups.constitutional_stack,
        PromptContextGroup::ActiveTaskContext => &mut groups.active_task_context,
        PromptContextGroup::GovernedMemoryEvidence => &mut groups.governed_memory_evidence,
        PromptContextGroup::BackgroundGovernance => &mut groups.background_governance,
    }
}

fn push_group_text(target: &mut Option<String>, text: String) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    match target {
        Some(existing) if !existing.trim().is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(text);
        }
        Some(existing) => *existing = text.to_owned(),
        None => *target = Some(text.to_owned()),
    }
}

fn project_content_for_context(selection: &RecallSelection, surface: ProjectionSurface) -> String {
    match (selection.plane, surface) {
        (MemoryPlane::SubjectProjection, ProjectionSurface::Adapter)
        | (MemoryPlane::SubjectProjection, ProjectionSurface::OperatorInspection)
        | (MemoryPlane::SubjectProjection, ProjectionSurface::Replay) => {
            "subject projection presence: current-turn frame available; raw private material withheld"
                .to_owned()
        }
        (MemoryPlane::SoulGovernance, ProjectionSurface::Adapter)
        | (MemoryPlane::SoulGovernance, ProjectionSurface::OperatorInspection)
        | (MemoryPlane::SoulGovernance, ProjectionSurface::Replay) => {
            "soul governance presence: policy summary available; raw private material withheld"
                .to_owned()
        }
        _ => project_content(selection, surface),
    }
}

fn surface_requires_report_first(plane: MemoryPlane, surface: ProjectionSurface) -> bool {
    matches!(
        (plane, surface),
        (MemoryPlane::SoulGovernance, ProjectionSurface::Adapter)
            | (
                MemoryPlane::SoulGovernance,
                ProjectionSurface::OperatorInspection
            )
            | (MemoryPlane::SubjectProjection, ProjectionSurface::Adapter)
            | (
                MemoryPlane::SubjectProjection,
                ProjectionSurface::OperatorInspection
            )
    )
}

fn sanitize_projection_text(
    input: &str,
    request: &RecallAssemblyRequest,
    report: &mut ProjectionSanitizerReport,
) -> String {
    if input.trim().is_empty() {
        return String::new();
    }
    let mut output = redact_credentials(input, report);
    let fragments = request
        .redaction
        .private_fragments
        .iter()
        .chain(request.redaction.identifier_fragments.iter())
        .collect::<Vec<_>>();
    report.checked_fragments = report.checked_fragments.saturating_add(fragments.len());
    for fragment in fragments {
        let trimmed = fragment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if output.contains(trimmed) {
            output = output.replace(trimmed, "[redacted:private_echo]");
            report.redacted_fragments = report.redacted_fragments.saturating_add(1);
            report.private_echo_redacted = report.private_echo_redacted.saturating_add(1);
        }
    }
    output
}

fn redact_credentials(input: &str, report: &mut ProjectionSanitizerReport) -> String {
    let mut redacted = Vec::new();
    for token in input.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        let marker = ["token=", "secret=", "api_key=", "apikey=", "password="]
            .iter()
            .find(|marker| lower.contains(**marker));
        if let Some(marker) = marker {
            if let Some(start) = lower.find(marker) {
                let prefix_end = start + marker.len();
                let mut value = token.to_owned();
                value.replace_range(prefix_end.., "[redacted:credential]");
                redacted.push(value);
                report.redacted_fragments = report.redacted_fragments.saturating_add(1);
                report.credentials_redacted = report.credentials_redacted.saturating_add(1);
                continue;
            }
        }
        redacted.push(token.to_owned());
    }
    redacted.join(" ")
}

fn normalize_assembly_groups(
    groups: &mut PromptAssemblyGroups,
    budget: &bm_core::ProjectionBudgetProfile,
) -> PromptAssemblyBudgetReport {
    let constitutional_stack = cap_group(
        &mut groups.constitutional_stack,
        budget.constitutional_bytes,
    );
    let active_task = cap_group(&mut groups.active_task_context, budget.active_task_bytes);
    let governed_memory = cap_group(
        &mut groups.governed_memory_evidence,
        budget.governed_memory_bytes,
    );
    let background_governance = cap_group(
        &mut groups.background_governance,
        budget.background_governance_bytes,
    );
    let before_bytes = constitutional_stack
        .before_bytes
        .saturating_add(active_task.before_bytes)
        .saturating_add(governed_memory.before_bytes)
        .saturating_add(background_governance.before_bytes);
    let mut after_bytes = constitutional_stack
        .after_bytes
        .saturating_add(active_task.after_bytes)
        .saturating_add(governed_memory.after_bytes)
        .saturating_add(background_governance.after_bytes);
    let mut total = PromptAssemblyBudgetSlice {
        before_bytes,
        after_bytes,
        max_bytes: budget.total_bytes,
        trimmed: before_bytes > budget.total_bytes,
    };
    if after_bytes > budget.total_bytes {
        total.trimmed = true;
        after_bytes = budget.total_bytes;
        total.after_bytes = after_bytes;
    }
    PromptAssemblyBudgetReport {
        total,
        constitutional_stack,
        active_task,
        governed_memory,
        background_governance,
    }
}

fn cap_group(target: &mut Option<String>, max_bytes: usize) -> PromptAssemblyBudgetSlice {
    let before_bytes = target.as_ref().map(|text| text.len()).unwrap_or_default();
    if let Some(text) = target.as_mut() {
        let capped = trim_to_budget(text.trim().to_owned(), max_bytes);
        if capped.is_empty() {
            *target = None;
        } else {
            *text = capped;
        }
    }
    let after_bytes = target.as_ref().map(|text| text.len()).unwrap_or_default();
    PromptAssemblyBudgetSlice {
        before_bytes,
        after_bytes,
        max_bytes,
        trimmed: before_bytes > after_bytes,
    }
}

fn assembly_warnings(
    plane_reports: &[RecallPlaneExecutionReport],
    budget: &PromptAssemblyBudgetReport,
    sanitizer: &ProjectionSanitizerReport,
    surface: ProjectionSurface,
    privacy_filtered_count: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if budget.total.trimmed
        || budget.constitutional_stack.trimmed
        || budget.active_task.trimmed
        || budget.governed_memory.trimmed
        || budget.background_governance.trimmed
    {
        warnings.push(format!(
            "prompt_assembly:budget_trimmed:surface={:?};before={};after={}",
            surface, budget.total.before_bytes, budget.total.after_bytes
        ));
    }
    if sanitizer.redacted_fragments > 0 {
        warnings.push(format!(
            "prompt_assembly:sanitized:redacted={}",
            sanitizer.redacted_fragments
        ));
    }
    if privacy_filtered_count > 0 {
        warnings.push(format!(
            "prompt_assembly:privacy_filtered:blocks={privacy_filtered_count}"
        ));
    }
    for report in plane_reports {
        if let Some(reason) = report.miss_reason.as_deref() {
            warnings.push(format!(
                "prompt_assembly:plane_miss:{}:{reason}",
                report.plane.as_str()
            ));
        }
        for warning in &report.warnings {
            warnings.push(format!(
                "prompt_assembly:plane_warning:{}:{warning}",
                report.plane.as_str()
            ));
        }
    }
    warnings.sort();
    warnings.dedup();
    warnings
}

fn structurally_weak_query(query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.is_empty() || trimmed.chars().count() <= 12 || matches!(trimmed, "继续" | "continue")
}

fn contains_any(query: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| query.contains(needle))
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
