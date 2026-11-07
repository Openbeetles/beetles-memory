//! Beetle-selected visual observations enter the existing factual owner with
//! an exact perception episode source. No assistant reply is extracted here.

use super::*;
use crate::store_internal::perception::{
    perception_episode_storage_key, PerceptionEpisodeEvidenceV1, PerceptionTurnEpisodeIntakeV1,
    PERCEPTION_EPISODE_NAMESPACE, PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE,
};
use bm_core::memory::{
    plan_governed_shared_memory_in_space, plan_visual_long_term_draft, LongTermMemoryStore,
    PerceptionEpisodeSourceRef, SharedFactWriteGovernanceContext, SharedMemoryWriteSource,
};
use sha2::{Digest, Sha256};

impl MemoryRuntime {
    pub(crate) fn consolidate_visual_long_term_after_turn(
        &self,
        episodes: &[PerceptionTurnEpisodeIntakeV1],
    ) -> Result<()> {
        for intake in episodes {
            if intake.episode.observation.is_some() {
                self.consolidate_visual_long_term_episode(&intake.episode.source_ref()?)?;
            }
        }
        Ok(())
    }

    /// Retries Beetle's own selected observation after an interrupted
    /// consolidation. The caller supplies only the immutable episode address;
    /// the draft is rederived and revalidated from Store evidence.
    pub fn consolidate_visual_long_term_episode(
        &self,
        source: &PerceptionEpisodeSourceRef,
    ) -> Result<bool> {
        self.ensure_visible("visual_memory.consolidate", self.capabilities.write)?;
        source.validate_contract()?;
        if source.memory_space_id != self.config.memory_space_id
            || source.subject_id != self.config.scoped_runtime.mounted_subject_id
            || source.relationship_id.as_deref()
                != self
                    .config
                    .scoped_runtime
                    .relationship_scope
                    .as_ref()
                    .map(|scope| scope.relationship_id.as_str())
        {
            return Err(Error::config(
                "visual_long_term_source",
                "visual episode escapes mounted subject or relationship",
            ));
        }
        let platform = self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config("visual_long_term_source", "transactional Store is required")
        })?;
        let key = perception_episode_storage_key(
            &source.memory_space_id,
            &source.subject_id,
            &source.source,
            &source.episode_id,
        )?;
        if !platform
            .read_known_json_docs(
                PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .is_empty()
        {
            return Ok(false);
        }
        let document = platform
            .read_known_json_docs(PERCEPTION_EPISODE_NAMESPACE, std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| {
                Error::config(
                    "visual_long_term_source",
                    "exact visual episode is unavailable",
                )
            })?;
        let episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(document.value)
            .map_err(|error| Error::config("visual_long_term_source", error.to_string()))?;
        if episode.source_ref()? != *source {
            return Err(Error::config(
                "visual_long_term_source",
                "visual episode digest differs",
            ));
        }
        let Some(observation) = episode.observation.as_ref() else {
            return Ok(false);
        };
        let Some(visual) = plan_visual_long_term_draft(observation, source, episode.source_kind)?
        else {
            return Ok(false);
        };
        let sources = LongTermSourceContext {
            platform: Some(platform),
            memory_space_id: &self.config.memory_space_id,
            mounted_subject_id: &self.config.scoped_runtime.mounted_subject_id,
            mounted_relationship_id: self
                .config
                .scoped_runtime
                .relationship_scope
                .as_ref()
                .map(|scope| scope.relationship_id.as_str()),
            admitted_turn: None,
        };
        let mut source_preconditions = Vec::new();
        let admitted_source = sources.admit_visual_draft(&visual, &mut source_preconditions)?;
        let policy_precondition =
            platform.perception_policy_manifest_precondition(&self.config.memory_space_id)?;
        let draft_id = visual.draft.stable_id().ok_or_else(|| {
            Error::config(
                "visual_long_term_draft",
                "visual fact has no stable owner id",
            )
        })?;
        let now_secs = self.config.clock.now_secs();
        if self
            .long_term_draft_admission_policy(now_secs)?
            .as_ref()
            .is_some_and(|policy| !policy.accepts_long_term_draft(&visual.draft))
        {
            return Ok(false);
        }
        let planning = PlanningLongTermMemoryStore::new(
            self.config.long_term_memory_read_store.as_ref(),
            &self.config.scoped_runtime.actor_subject_id,
            sources,
        )?;
        if let Some(existing) = LongTermMemoryStore::get(&planning, &draft_id)? {
            if existing.perception_sources.contains(&admitted_source) {
                return Ok(false);
            }
            return Err(Error::config(
                "visual_long_term_draft",
                "visual owner id is occupied by another source",
            ));
        }
        let mut governance = SharedFactWriteGovernanceContext::new(
            self.config.memory_space_id.clone(),
            self.config.scoped_runtime.mounted_subject_id.clone(),
            self.config.scoped_runtime.actor_subject_id.clone(),
            SharedMemoryWriteSource::VisualObservation,
        );
        governance.relationship_id = self
            .config
            .scoped_runtime
            .relationship_scope
            .as_ref()
            .map(|scope| scope.relationship_id.clone());
        let governed = plan_governed_shared_memory_in_space(
            &planning,
            std::slice::from_ref(&visual.draft),
            now_secs,
            governance,
        )?;
        self.reject_reported_subject_visibility_transition(&governed.outcome)?;
        if governed.accepted_entries.len() != 1 {
            return Ok(false);
        }
        let mut entry = governed.accepted_entries[0].clone();
        if entry.id != draft_id {
            return Err(Error::config(
                "visual_long_term_draft",
                "governed owner id differs",
            ));
        }
        entry.perception_sources.push(admitted_source.clone());
        entry.perception_sources.sort();
        entry.perception_sources.dedup();
        planning.stage_entries(std::slice::from_ref(&entry))?;
        let mut plan = planning.into_plan(
            &self.config.memory_space_id,
            &self.config.memory_space_id,
            platform,
            self.runtime_budget()
                .governed_state_budget
                .max_retained_long_term_revisions_per_owner,
        )?;
        let facet =
            self.plan_long_term_facet_index_mutations_for_store_mutations(&plan.mutations)?;
        plan.merge(facet)?;
        merge_json_preconditions(&mut plan.preconditions, source_preconditions)?;
        merge_json_preconditions(&mut plan.preconditions, vec![policy_precondition])?;
        let event_scope = crate::store_internal::StoreEventScope::new(
            &self.config.identity.agent_id,
            &self.config.identity.owner_id,
            &self.config.scope.channel,
            &self.config.scope.chat_id,
        )
        .with_memory_space(&self.config.memory_space_id)
        .with_subject(&self.config.scoped_runtime.mounted_subject_id)
        .with_conversation(self.config.scope.conversation_id_or_chat_id());
        let transaction_id =
            format!(
                "visual-long-term:sha256:{:x}",
                Sha256::digest(serde_json::to_vec(&(&admitted_source, &draft_id)).map_err(
                    |error| { Error::config("visual_long_term_draft", error.to_string()) }
                )?)
            );
        let now_secs = self.config.clock.now_secs();
        platform.commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget_at(
            StoreMutationBatch {
                transaction_id,
                operation: "visual_long_term_consolidation".into(),
                scope: event_scope,
                mutations: plan.mutations,
            },
            &plan.preconditions,
            &plan.blob_preconditions,
            &self.runtime_budget(),
            now_secs,
        )?;
        Ok(true)
    }
}
