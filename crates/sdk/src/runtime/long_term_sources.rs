use super::*;
use crate::store_internal::perception::{
    perception_episode_storage_key, PerceptionEpisodeEvidenceV1, PERCEPTION_EPISODE_NAMESPACE,
    PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE,
};
use crate::store_internal::transcript_turn_storage_key;
use bm_core::memory::{
    plan_visual_long_term_draft, PerceptionEpisodeSourceRef, PerceptionRetentionCeiling,
    VisualLongTermDraft,
};

/// SDK-resolved source authority; callers and model output cannot supply a clock.
#[derive(Clone, Copy)]
pub(super) struct LongTermSourceContext<'a> {
    pub platform: Option<&'a StorePlatform>,
    pub memory_space_id: &'a str,
    pub mounted_subject_id: &'a str,
    pub mounted_relationship_id: Option<&'a str>,
    pub admitted_turn: Option<&'a TranscriptTurnRecord>,
}

impl LongTermSourceContext<'_> {
    fn load_perception_episode(
        &self,
        source: &PerceptionEpisodeSourceRef,
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<PerceptionEpisodeEvidenceV1> {
        source.validate_contract()?;
        if source.memory_space_id != self.memory_space_id
            || source.subject_id != self.mounted_subject_id
            || source.relationship_id.as_deref() != self.mounted_relationship_id
        {
            return Err(Error::config(
                "long_term_perception_source",
                "perception source belongs to another mounted subject",
            ));
        }
        let platform = self.platform.ok_or_else(|| {
            Error::config(
                "long_term_perception_source",
                "episode Store owner is required",
            )
        })?;
        let key = perception_episode_storage_key(
            &source.memory_space_id,
            &source.subject_id,
            &source.source,
            &source.episode_id,
        )?;
        let value = platform
            .read_known_json_docs(PERCEPTION_EPISODE_NAMESPACE, std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .map(|document| document.value)
            .ok_or_else(|| {
                Error::config(
                    "long_term_perception_source",
                    "episode owner is unavailable",
                )
            })?;
        let episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(value.clone())
            .map_err(|error| Error::config("long_term_perception_source", error.to_string()))?;
        if episode.source_ref()? != *source
            || episode.retention_ceiling < PerceptionRetentionCeiling::DurableSemantic
        {
            return Err(Error::config(
                "long_term_perception_source",
                "episode digest or retention differs",
            ));
        }
        if !platform
            .read_known_json_docs(
                PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .is_empty()
        {
            return Err(Error::config(
                "long_term_perception_source",
                "episode was withdrawn",
            ));
        }
        merge_json_preconditions(
            preconditions,
            vec![
                json_precondition(PERCEPTION_EPISODE_NAMESPACE, &key, Some(value)),
                json_precondition(PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE, &key, None),
            ],
        )?;
        Ok(episode)
    }

    pub(super) fn admit_visual_draft(
        &self,
        visual: &VisualLongTermDraft,
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<PerceptionEpisodeSourceRef> {
        let episode = self.load_perception_episode(&visual.source, preconditions)?;
        let observation = episode.observation.as_ref().ok_or_else(|| {
            Error::config(
                "long_term_perception_source",
                "episode has no Beetle observation",
            )
        })?;
        if observation.input_id != visual.observation_input_id {
            return Err(Error::config(
                "long_term_perception_source",
                "observation input differs from visual draft",
            ));
        }
        let expected =
            plan_visual_long_term_draft(observation, &visual.source, episode.source_kind)?
                .ok_or_else(|| {
                    Error::config(
                        "long_term_perception_source",
                        "visual fact policy rejected source",
                    )
                })?;
        if expected != *visual {
            return Err(Error::config(
                "long_term_perception_source",
                "visual draft differs from governed episode",
            ));
        }
        Ok(visual.source.clone())
    }

    fn validate_direct_visual_entry_source(
        &self,
        entry: &LongTermMemoryEntry,
        source: &PerceptionEpisodeSourceRef,
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        let episode = self.load_perception_episode(source, preconditions)?;
        let observation = episode.observation.as_ref().ok_or_else(|| {
            Error::config(
                "long_term_perception_source",
                "episode has no Beetle observation",
            )
        })?;
        let planned = plan_visual_long_term_draft(observation, source, episode.source_kind)?
            .ok_or_else(|| {
                Error::config(
                    "long_term_perception_source",
                    "visual fact policy rejected source",
                )
            })?;
        let draft = &planned.draft;
        if entry.kind != draft.kind
            || entry.topic != draft.topic
            || entry.content != draft.content
            || entry.keywords != draft.keywords
            || entry.privacy != draft.privacy
            || entry.source_chat_id != draft.source_chat_id
            || Some(entry.source_type) != draft.source_type
            || Some(entry.source_scope) != draft.source_scope
            || entry.subject_visibility != draft.subject_visibility
            || entry.provenance != draft.provenance
            || Some(entry.confidence) != draft.confidence
            || Some(entry.freshness) != draft.freshness
            || Some(entry.stale_hint) != draft.stale_hint
            || Some(entry.observed_at) != draft.observed_at
            || entry.source_revision != draft.source_revision
        {
            return Err(Error::config(
                "long_term_perception_source",
                "long-term owner differs from governed visual draft",
            ));
        }
        Ok(())
    }
    fn resolve(
        &self,
        citation: &str,
    ) -> Result<Option<(TranscriptEvidenceRef, TranscriptTurnRecord)>> {
        if !citation.starts_with("transcript:") {
            return Ok(None);
        }
        let mut source = TranscriptEvidenceRef::parse_display_citation(citation)
            .filter(|source| source.display_citation() == citation)
            .ok_or_else(|| {
                Error::config(
                    "long_term_source_authority",
                    "noncanonical transcript citation",
                )
            })?;
        if source.memory_space_id != self.memory_space_id {
            return Err(Error::config(
                "long_term_source_authority",
                "source belongs to another memory space",
            ));
        }
        let platform = self.platform.ok_or_else(|| {
            Error::config(
                "long_term_source_authority",
                "transcript source requires its Store owner",
            )
        })?;
        let key = ConversationKey::new(
            &source.memory_space_id,
            &source.channel_id,
            &source.conversation_id,
        )?;
        let record = platform
            .get_turn(&key, self.mounted_subject_id, &source.turn_id)?
            .ok_or_else(|| {
                Error::config(
                    "long_term_source_authority",
                    "exact mounted source is unavailable",
                )
            })?;
        if record.redaction_state != bm_core::memory::TranscriptRedactionState::RawAvailable
            || record.lifecycle_state == bm_core::memory::TranscriptLifecycleState::RawDeleted
        {
            return Err(Error::config(
                "long_term_source_authority",
                "source has been withdrawn",
            ));
        }
        source.subject_id = Some(record.subject.clone());
        if let Some(message_id) = &source.message_id {
            let message = record
                .input_messages
                .iter()
                .chain(record.assistant_messages.iter().map(|entry| &entry.message))
                .find(|message| &message.message_id == message_id)
                .ok_or_else(|| {
                    Error::config(
                        "long_term_source_authority",
                        "exact source message is unavailable",
                    )
                })?;
            source.authority = Some(message.authority);
        }
        Ok(Some((source, record)))
    }

    pub fn authority(
        &self,
        citations: &[String],
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<(Vec<TranscriptEvidenceRef>, Option<u64>)> {
        // A durable job has one immutable factual input. Historic grounding does
        // not advance the authority of that input, nor veto a genuinely new turn.
        if let Some(turn) = self.admitted_turn {
            if !turn.perception_source_uses.is_empty() {
                return Err(Error::config(
                    "long_term_perception_source",
                    "visual Transcript cannot enter generic long-term learning",
                ));
            }
            append_source_precondition(turn, preconditions)?;
            return Ok((
                vec![turn_source(turn)],
                Some(turn.factual_admission_revision),
            ));
        }
        let mut sources = Vec::new();
        let mut revision: Option<u64> = None;
        for citation in citations {
            if let Some((source, record)) = self.resolve(citation)? {
                if !record.perception_source_uses.is_empty() {
                    return Err(Error::config(
                        "long_term_perception_source",
                        "visual Transcript cannot enter generic long-term learning",
                    ));
                }
                revision = Some(revision.map_or(record.factual_admission_revision, |prior| {
                    prior.min(record.factual_admission_revision)
                }));
                sources.push(source);
                append_source_precondition(&record, preconditions)?;
            }
        }
        sources.sort_by_key(TranscriptEvidenceRef::display_citation);
        sources.dedup();
        Ok((sources, revision))
    }

    pub fn bind(
        &self,
        entry: &mut LongTermMemoryEntry,
        prior: Option<&LongTermMemoryEntry>,
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        let mut sources = Vec::new();
        let direct_sources = entry.perception_sources.clone();
        let mut perception_sources = prior
            .map(|entry| entry.perception_sources.clone())
            .unwrap_or_default();
        for source in direct_sources {
            if prior.is_some_and(|old| old.perception_sources.contains(&source)) {
                continue;
            }
            self.validate_direct_visual_entry_source(entry, &source, preconditions)?;
            perception_sources.push(source);
        }
        for citation in entry.supporting_citations.iter().map(String::as_str).chain(
            entry.canonical_entities.iter().flat_map(|entity| {
                entity
                    .evidence_refs
                    .iter()
                    .map(|evidence| evidence.source_ref.as_str())
            }),
        ) {
            // Retain immutable provenance after withdrawal; only new dependencies need admission.
            if let Some(inherited) = prior.and_then(|prior| {
                prior
                    .transcript_sources
                    .iter()
                    .find(|source| source.display_citation() == citation)
            }) {
                sources.push(inherited.clone());
            } else if let Some((source, record)) = self.resolve(citation)? {
                if !record.perception_source_uses.is_empty() {
                    return Err(Error::config(
                        "long_term_perception_source",
                        "visual Transcript cannot enter generic long-term learning",
                    ));
                }
                append_source_precondition(&record, preconditions)?;
                sources.push(source);
            }
        }
        if let Some(turn) = self.admitted_turn {
            if !turn.perception_source_uses.is_empty() {
                return Err(Error::config(
                    "long_term_perception_source",
                    "visual Transcript cannot enter generic long-term learning",
                ));
            }
            let source = turn_source(turn);
            let citation = source.display_citation();
            if !entry.supporting_citations.contains(&citation) {
                entry.supporting_citations.push(citation);
            }
            sources.push(source);
            append_source_precondition(turn, preconditions)?;
        }
        sources.sort_by_key(TranscriptEvidenceRef::display_citation);
        sources.dedup();
        entry.transcript_sources = sources;
        perception_sources.sort();
        perception_sources.dedup();
        entry.perception_sources = perception_sources;
        Ok(())
    }
}

fn turn_source(turn: &TranscriptTurnRecord) -> TranscriptEvidenceRef {
    TranscriptEvidenceRef {
        memory_space_id: turn.key.memory_space_id.clone(),
        channel_id: turn.key.channel_id.clone(),
        conversation_id: turn.key.conversation_id.clone(),
        turn_id: turn.turn_id.clone(),
        message_id: None,
        subject_id: Some(turn.subject.clone()),
        authority: None,
    }
}

fn append_source_precondition(
    record: &TranscriptTurnRecord,
    preconditions: &mut Vec<StoreJsonPrecondition>,
) -> Result<()> {
    merge_json_preconditions(
        preconditions,
        vec![json_precondition(
            "conversation_transcript",
            &transcript_turn_storage_key(&record.key, &record.subject, &record.turn_id),
            Some(
                serde_json::to_value(record).map_err(|error| {
                    Error::config("long_term_source_authority", error.to_string())
                })?,
            ),
        )],
    )
}

/// A durable job reads its original canonical turn, not a moving live session.
pub(super) struct GovernanceTurnSession {
    chat_id: String,
    records: Vec<SessionMessageRecord>,
    source_count: usize,
}
impl GovernanceTurnSession {
    pub fn new(
        chat_id: &str,
        turn: &TranscriptTurnRecord,
        records: &[TranscriptTurnRecord],
    ) -> Self {
        let slice = bm_core::memory::RedactedTranscriptSlice::from_records(
            turn.key.clone(),
            TranscriptReplayView::RawOwnerOnly,
            std::slice::from_ref(turn),
        );
        let source_count = records
            .iter()
            .map(|record| record.input_messages.len() + record.assistant_messages.len())
            .sum();
        Self {
            chat_id: chat_id.to_string(),
            records: redacted_transcript_slice_to_session_records(&slice, &turn.key),
            source_count,
        }
    }
    fn reject_write<T>() -> Result<T> {
        Err(Error::config(
            "governance_source_snapshot",
            "a canonical job input is read-only",
        ))
    }
}
impl SessionStore for GovernanceTurnSession {
    fn append(&self, _: &str, _: &str, _: &str) -> Result<()> {
        Self::reject_write()
    }
    fn load_recent(&self, chat: &str, n: usize) -> Result<Vec<SessionMessage>> {
        Ok(self
            .load_recent_records(chat, n)?
            .into_iter()
            .map(SessionMessageRecord::into_message)
            .collect())
    }
    fn load_recent_records(&self, chat: &str, n: usize) -> Result<Vec<SessionMessageRecord>> {
        if chat != self.chat_id {
            return Err(Error::config(
                "governance_source_snapshot",
                "job input scope mismatch",
            ));
        }
        Ok(self
            .records
            .iter()
            .skip(self.records.len().saturating_sub(n))
            .cloned()
            .collect())
    }
    fn message_count(&self, chat: &str) -> Result<usize> {
        self.load_recent_records(chat, 0)?;
        Ok(self.source_count)
    }
    fn clear(&self, _: &str) -> Result<()> {
        Self::reject_write()
    }
    fn list_chat_ids(&self) -> Result<Vec<String>> {
        Ok(vec![self.chat_id.clone()])
    }
    fn delete(&self, _: &str) -> Result<()> {
        Self::reject_write()
    }
}

/// Inspection owns a pinned, already visibility/evidence-governed snapshot.
/// Mutation planning deliberately never consumes this lossy projection.
pub(super) struct LongTermInspectionSnapshot {
    pub entries: Vec<LongTermMemoryEntry>,
    pub operation_time: u64,
}
impl LongTermMemoryReadStore for LongTermInspectionSnapshot {
    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        Ok(self.entries.iter().find(|entry| entry.id == id).cloned())
    }
    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        Ok(self.entries.iter().take(limit).cloned().collect())
    }
    fn count(&self) -> Result<usize> {
        Ok(self.entries.len())
    }
    fn query_all(&self, query: &LongTermMemoryQuery) -> Result<Vec<LongTermMemoryEntry>> {
        Ok(query.filter_sort_all_entries(self.entries.clone(), self.operation_time))
    }
    fn query(&self, query: &LongTermMemoryQuery) -> Result<Vec<LongTermMemoryEntry>> {
        Ok(query.filter_sort_entries(self.entries.clone(), self.operation_time))
    }
    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = self
            .entries
            .iter()
            .filter(|entry| {
                query.is_empty() || entry.content.contains(query) || entry.topic.contains(query)
            })
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| {
            (
                std::cmp::Reverse(entry.source_chat_id.as_deref() == source_chat_id),
                std::cmp::Reverse(entry.updated_at),
            )
        });
        entries.truncate(limit);
        Ok(entries)
    }
}

/// Not serializable and not constructible by hosts or automatic learning plans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LongTermControlAuthorization {
    memory_space_id: String,
    actor_subject_id: String,
    revision_digests: BTreeSet<String>,
}
impl LongTermControlAuthorization {
    pub(super) fn issue(space: &str, actor: &str, mutations: &[StoreMutation]) -> Result<Self> {
        let mut revision_digests = BTreeSet::new();
        for mutation in mutations {
            if let StoreMutation::PutJson {
                namespace, value, ..
            } = mutation
            {
                if namespace == LONG_TERM_CONTROL_REVISION_NAMESPACE {
                    let revision: bm_core::memory::LongTermMemoryControlRevision =
                        serde_json::from_value(value.clone()).map_err(|error| {
                            Error::config("long_term_control_authority", error.to_string())
                        })?;
                    revision.validate_contract()?;
                    if revision.memory_space_id != space
                        || revision.actor_subject_id.as_deref() != Some(actor)
                        || !matches!(
                            revision.basis,
                            bm_core::memory::LongTermTransitionBasis::ExplicitControl
                        )
                    {
                        return Err(Error::config(
                            "long_term_control_authority",
                            "control plan differs from authorized actor or operation",
                        ));
                    }
                    revision_digests.insert(revision.content_digest);
                }
            }
        }
        Ok(Self {
            memory_space_id: space.into(),
            actor_subject_id: actor.into(),
            revision_digests,
        })
    }
    pub(crate) fn authorizes(
        &self,
        revision: &bm_core::memory::LongTermMemoryControlRevision,
    ) -> bool {
        revision.memory_space_id == self.memory_space_id
            && revision.actor_subject_id.as_deref() == Some(self.actor_subject_id.as_str())
            && self.revision_digests.contains(&revision.content_digest)
    }
}
