use std::collections::{BTreeMap, BTreeSet};

#[cfg(feature = "nonproduction-replay-harness")]
use bm_core::memory::{MemoryMutationEffect, MemoryMutationOperationIdentity};
use bm_core::memory::{
    MemoryMutationOperationKind, MemoryMutationReceipt, MEMORY_MUTATION_RECEIPT_NAMESPACE,
};
use bm_core::skills::{
    validate_agent_tool_experience_owner_history, validate_agent_tool_experience_scope_closure,
    AgentToolExperienceHeadStateV2, AgentToolExperienceOwnerHeadV2,
    AgentToolExperienceRevisionMaterialV2, AgentToolExperienceScopeManifestV1,
};
use bm_core::skills::{AgentToolExperienceHeadBindingV1, AgentToolExperienceOwningScopeV1};
use bm_core::{Error, Result};
use serde::Serialize;
#[cfg(feature = "nonproduction-replay-harness")]
use sha2::{Digest, Sha256};

#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::platform::{StoreMutationOperationOutcome, StoreMutationOperationPlan};
use crate::store_internal::schema::{
    AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
    AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
};
use crate::store_internal::transaction::BackendTransactionState;
#[cfg(feature = "nonproduction-replay-harness")]
use crate::StoreMutationBatchReport;
use crate::{MemoryStoreEventKind, StoreEventScope, StoreJsonPrecondition, StorePlatform};
use crate::{StoreMutation, StoreMutationBatch, StorePhysicalOwningScope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AgentToolExperienceStoreBudget {
    pub(crate) max_owners_per_subject: usize,
    pub(crate) max_revisions_per_owner: usize,
    pub(crate) max_evidence_refs_per_owner: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(feature = "nonproduction-replay-harness")]
pub struct AgentToolExperienceStoreLimitsV1 {
    pub max_owners_per_subject: usize,
    pub max_revisions_per_owner: usize,
    pub max_evidence_refs_per_owner: usize,
}

#[cfg(feature = "nonproduction-replay-harness")]
impl From<AgentToolExperienceStoreBudget> for AgentToolExperienceStoreLimitsV1 {
    fn from(value: AgentToolExperienceStoreBudget) -> Self {
        Self {
            max_owners_per_subject: value.max_owners_per_subject,
            max_revisions_per_owner: value.max_revisions_per_owner,
            max_evidence_refs_per_owner: value.max_evidence_refs_per_owner,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentToolExperienceStoreMutationPlanV1 {
    operation_id: String,
    actor_subject_id: String,
    scope: StoreEventScope,
    previous_head: Option<AgentToolExperienceOwnerHeadV2>,
    previous_manifest: Option<AgentToolExperienceScopeManifestV1>,
    material: AgentToolExperienceRevisionMaterialV2,
    head: AgentToolExperienceOwnerHeadV2,
    manifest: AgentToolExperienceScopeManifestV1,
    committed_at_unix_secs: u64,
}

impl AgentToolExperienceStoreMutationPlanV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        operation_id: impl Into<String>,
        actor_subject_id: impl Into<String>,
        scope: StoreEventScope,
        previous_head: Option<AgentToolExperienceOwnerHeadV2>,
        previous_manifest: Option<AgentToolExperienceScopeManifestV1>,
        material: AgentToolExperienceRevisionMaterialV2,
        head: AgentToolExperienceOwnerHeadV2,
        manifest: AgentToolExperienceScopeManifestV1,
        committed_at_unix_secs: u64,
    ) -> Result<Self> {
        let plan = Self {
            operation_id: operation_id.into(),
            actor_subject_id: actor_subject_id.into(),
            scope,
            previous_head,
            previous_manifest,
            material,
            head,
            manifest,
            committed_at_unix_secs,
        };
        plan.validate_contract(usize::MAX)?;
        Ok(plan)
    }

    fn validate_contract(&self, max_owners: usize) -> Result<()> {
        let owning_scope = AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: self.scope.subject_id.clone(),
        };
        if self.operation_id.trim().is_empty()
            || self.operation_id.trim() != self.operation_id
            || self.actor_subject_id.trim().is_empty()
            || self.actor_subject_id.trim() != self.actor_subject_id
            || self.committed_at_unix_secs == 0
            || self.scope.memory_space_id != self.material.memory_space_id
            || self.scope.subject_id != self.material.owning_scope.mounted_subject_id()
            || self.scope.physical_owning_scope
                != (StorePhysicalOwningScope::Subject {
                    mounted_subject_id: self.scope.subject_id.clone(),
                })
            || self.material.owning_scope != owning_scope
            || self.head.memory_space_id != self.material.memory_space_id
            || self.head.owning_scope != self.material.owning_scope
            || self.head.owner_ref != self.material.owner_ref
            || self.head.current_revision != self.material.owner_revision
            || self.manifest.memory_space_id != self.material.memory_space_id
            || self.manifest.owning_scope != self.material.owning_scope
            || !self.material.validate_contract().accepted
            || !self.head.validate_contract().accepted
            || self.manifest.bindings.is_empty()
        {
            return Err(Error::invalid_input(
                "agent_tool_experience_store_plan",
                "operation, scope, material, head, and manifest must form one canonical subject owner",
            ));
        }

        let current_binding =
            AgentToolExperienceHeadBindingV1::from_head_and_material(&self.head, &self.material)?;
        let expected_manifest_revision = self
            .previous_manifest
            .as_ref()
            .map_or(1, |manifest| manifest.revision.saturating_add(1));
        let mut expected_bindings = self
            .previous_manifest
            .as_ref()
            .map(|manifest| manifest.bindings.clone())
            .unwrap_or_default();
        expected_bindings.retain(|binding| binding.owner_ref != self.head.owner_ref);
        expected_bindings.push(current_binding);
        let expected_manifest = AgentToolExperienceScopeManifestV1::build(
            expected_manifest_revision,
            &self.material.memory_space_id,
            self.material.owning_scope.clone(),
            expected_bindings,
            max_owners,
        )?;
        if expected_manifest != self.manifest {
            return Err(Error::invalid_input(
                "agent_tool_experience_store_plan",
                "scope manifest must exactly replace or add the committed owner head",
            ));
        }

        if self
            .previous_head
            .as_ref()
            .is_some_and(|head| head.state == AgentToolExperienceHeadStateV2::Tombstoned)
        {
            return Err(Error::invalid_input(
                "agent_tool_experience_store_plan",
                "tombstoned experience cannot be recreated",
            ));
        }
        match (&self.previous_head, &self.previous_manifest) {
            (None, None) => {
                if self.material.owner_revision != 1 || self.head.current_revision != 1 {
                    return Err(Error::invalid_input(
                        "agent_tool_experience_store_plan",
                        "a new owner must begin at revision one",
                    ));
                }
            }
            (None, Some(previous_manifest)) => {
                if previous_manifest.memory_space_id != self.material.memory_space_id
                    || previous_manifest.owning_scope != self.material.owning_scope
                    || previous_manifest
                        .bindings
                        .iter()
                        .any(|binding| binding.owner_ref == self.material.owner_ref)
                    || self.material.owner_revision != 1
                {
                    return Err(Error::invalid_input(
                        "agent_tool_experience_store_plan",
                        "new owner insertion conflicts with the previous scope closure",
                    ));
                }
            }
            (Some(_), None) => {
                return Err(Error::invalid_input(
                    "agent_tool_experience_store_plan",
                    "an existing owner requires its previous scope manifest",
                ));
            }
            (Some(previous_head), Some(previous_manifest)) => {
                if !previous_head.validate_contract().accepted
                    || previous_head.memory_space_id != self.material.memory_space_id
                    || previous_head.owning_scope != self.material.owning_scope
                    || previous_head.owner_ref != self.material.owner_ref
                    || previous_manifest.memory_space_id != self.material.memory_space_id
                    || previous_manifest.owning_scope != self.material.owning_scope
                    || self.material.owner_revision
                        != previous_head.current_revision.saturating_add(1)
                    || self.head.retained_revisions.len()
                        != previous_head.retained_revisions.len().saturating_add(1)
                    || !self
                        .head
                        .retained_revisions
                        .starts_with(&previous_head.retained_revisions)
                {
                    return Err(Error::invalid_input(
                        "agent_tool_experience_store_plan",
                        "owner update must append exactly one revision to the previous closure",
                    ));
                }
                let previous_binding = previous_manifest
                    .bindings
                    .iter()
                    .find(|binding| binding.owner_ref == previous_head.owner_ref)
                    .ok_or_else(|| {
                        Error::invalid_input(
                            "agent_tool_experience_store_plan",
                            "previous manifest does not bind the expected owner head",
                        )
                    })?;
                if previous_binding.head_key != previous_head.physical_key
                    || previous_binding.head_digest != previous_head.content_digest
                    || previous_binding.current_revision != previous_head.current_revision
                {
                    return Err(Error::invalid_input(
                        "agent_tool_experience_store_plan",
                        "previous manifest binding differs from the expected owner head",
                    ));
                }
            }
        }
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn intent_digest(&self) -> Result<String> {
        #[derive(Serialize)]
        struct Intent<'a> {
            actor_subject_id: &'a str,
            scope: &'a StoreEventScope,
            previous_head: &'a Option<AgentToolExperienceOwnerHeadV2>,
            previous_manifest: &'a Option<AgentToolExperienceScopeManifestV1>,
            material: &'a AgentToolExperienceRevisionMaterialV2,
            head: &'a AgentToolExperienceOwnerHeadV2,
            manifest: &'a AgentToolExperienceScopeManifestV1,
        }
        let bytes = serde_json::to_vec(&Intent {
            actor_subject_id: &self.actor_subject_id,
            scope: &self.scope,
            previous_head: &self.previous_head,
            previous_manifest: &self.previous_manifest,
            material: &self.material,
            head: &self.head,
            manifest: &self.manifest,
        })
        .map_err(|error| Error::config("agent_tool_experience_intent", error.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(b"agent_tool_experience_store_intent_v1");
        hasher.update(bytes.len().to_be_bytes());
        hasher.update(bytes);
        Ok(format!("sha256:{:x}", hasher.finalize()))
    }

    pub(crate) fn store_parts(
        &self,
        max_owners: usize,
    ) -> Result<(
        Vec<StoreMutation>,
        Vec<StoreJsonPrecondition>,
        bm_core::memory::GovernedOwnerRevisionRef,
    )> {
        self.validate_contract(max_owners)?;
        let material_value = encode(&self.material, "agent_tool_experience_material")?;
        let head_value = encode(&self.head, "agent_tool_experience_head")?;
        let manifest_value = encode(&self.manifest, "agent_tool_experience_manifest")?;
        let preconditions = vec![
            StoreJsonPrecondition::Absent {
                namespace: AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.to_string(),
                key: self.material.physical_key.clone(),
            },
            exact_or_absent(
                AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
                &self.head.physical_key,
                self.previous_head.as_ref(),
            )?,
            exact_or_absent(
                AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
                &self.manifest.physical_key,
                self.previous_manifest.as_ref(),
            )?,
        ];
        let mutations = vec![
            put_json(
                AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                &self.material.physical_key,
                material_value,
            ),
            put_json(
                AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
                &self.head.physical_key,
                head_value,
            ),
            put_json(
                AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
                &self.manifest.physical_key,
                manifest_value,
            ),
        ];
        Ok((mutations, preconditions, self.material.owner_revision_ref()))
    }
}

pub(crate) fn combine_agent_tool_experience_plans(
    plans: &[AgentToolExperienceStoreMutationPlanV1],
    max_owners: usize,
) -> Result<(
    Vec<StoreMutation>,
    Vec<StoreJsonPrecondition>,
    Vec<bm_core::memory::GovernedOwnerRevisionRef>,
)> {
    let Some(first) = plans.first() else {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    };
    let final_manifest = plans
        .last()
        .expect("non-empty procedural experience plans")
        .manifest
        .clone();
    let mut mutations = Vec::new();
    let mut preconditions = Vec::new();
    let mut owner_revisions = Vec::new();
    for (index, plan) in plans.iter().enumerate() {
        if plan.scope != first.scope
            || plan.operation_id != first.operation_id
            || plan.actor_subject_id != first.actor_subject_id
            || plan.committed_at_unix_secs != first.committed_at_unix_secs
            || (index > 0 && plan.previous_manifest.as_ref() != Some(&plans[index - 1].manifest))
        {
            return Err(Error::invalid_input(
                "agent_tool_experience_store_batch_plan",
                "batched experience plans must form one exact sequential scope manifest",
            ));
        }
        let (plan_mutations, plan_preconditions, owner_revision) = plan.store_parts(max_owners)?;
        mutations.extend(plan_mutations.into_iter().filter(|mutation| {
            !matches!(mutation, StoreMutation::PutJson { namespace, .. }
                if namespace == AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE)
        }));
        preconditions.extend(plan_preconditions.into_iter().filter(|precondition| {
            !matches!(precondition,
                StoreJsonPrecondition::Absent { namespace, .. }
                | StoreJsonPrecondition::Exact { namespace, .. }
                if namespace == AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE)
        }));
        owner_revisions.push(owner_revision);
    }
    preconditions.push(exact_or_absent(
        AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
        &first.manifest.physical_key,
        first.previous_manifest.as_ref(),
    )?);
    mutations.push(put_json(
        AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
        &final_manifest.physical_key,
        encode(&final_manifest, "agent_tool_experience_manifest")?,
    ));
    Ok((mutations, preconditions, owner_revisions))
}

#[derive(Clone, Debug)]
#[cfg(feature = "nonproduction-replay-harness")]
pub enum AgentToolExperienceStoreMutationOutcomeV1 {
    Committed {
        receipt: MemoryMutationReceipt,
        report: StoreMutationBatchReport,
    },
    Replayed {
        receipt: MemoryMutationReceipt,
    },
}

impl StorePlatform {
    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn commit_agent_tool_experience_mutation_with_runtime_budget(
        &self,
        plan: AgentToolExperienceStoreMutationPlanV1,
        runtime_budget: &bm_core::budget::RuntimeBudgetReport,
    ) -> Result<AgentToolExperienceStoreMutationOutcomeV1> {
        let budget = AgentToolExperienceStoreBudget {
            max_owners_per_subject: runtime_budget
                .governed_state_budget
                .max_agent_tool_experience_owners_per_subject,
            max_revisions_per_owner: runtime_budget
                .governed_state_budget
                .max_agent_tool_experience_revisions_per_owner,
            max_evidence_refs_per_owner: runtime_budget
                .governed_state_budget
                .max_agent_tool_experience_evidence_refs_per_owner,
        };
        plan.validate_contract(budget.max_owners_per_subject)?;
        let identity = MemoryMutationOperationIdentity::new(
            &plan.operation_id,
            &plan.scope.memory_space_id,
            &plan.scope.subject_id,
            &plan.actor_subject_id,
            MemoryMutationOperationKind::ProceduralLearning,
        )?;
        let operation = StoreMutationOperationPlan::new(
            identity,
            plan.intent_digest()?,
            MemoryMutationEffect::Changed,
            3,
            &plan.actor_subject_id,
            plan.committed_at_unix_secs,
        )?;
        let (mutations, preconditions, _) = plan.store_parts(budget.max_owners_per_subject)?;
        let batch = StoreMutationBatch {
            transaction_id: operation.transaction_id().to_string(),
            operation: "agent_tool_experience.commit".to_string(),
            scope: plan.scope,
            mutations,
        };
        match self.commit_memory_mutation_operation_with_runtime_budget(
            batch,
            &preconditions,
            operation,
            runtime_budget,
        )? {
            StoreMutationOperationOutcome::Committed { receipt, report } => {
                Ok(AgentToolExperienceStoreMutationOutcomeV1::Committed { receipt, report })
            }
            StoreMutationOperationOutcome::Replayed { receipt } => {
                Ok(AgentToolExperienceStoreMutationOutcomeV1::Replayed { receipt })
            }
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn commit_agent_tool_experience_for_nonproduction_harness(
        &self,
        plan: AgentToolExperienceStoreMutationPlanV1,
    ) -> Result<AgentToolExperienceStoreMutationOutcomeV1> {
        let runtime_budget = self.current_runtime_budget(super::platform::current_unix_secs());
        self.commit_agent_tool_experience_mutation_with_runtime_budget(plan, &runtime_budget)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn agent_tool_experience_limits_for_nonproduction_harness(
        &self,
    ) -> AgentToolExperienceStoreLimitsV1 {
        let report = self.current_runtime_budget(super::platform::current_unix_secs());
        AgentToolExperienceStoreBudget {
            max_owners_per_subject: report
                .governed_state_budget
                .max_agent_tool_experience_owners_per_subject,
            max_revisions_per_owner: report
                .governed_state_budget
                .max_agent_tool_experience_revisions_per_owner,
            max_evidence_refs_per_owner: report
                .governed_state_budget
                .max_agent_tool_experience_evidence_refs_per_owner,
        }
        .into()
    }
}

pub(crate) fn validate_agent_tool_experience_transition_preconditions(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    let stage = "agent_tool_experience_transition";
    for mutation in &batch.mutations {
        if let StoreMutation::DeleteJson { namespace, key, .. } = mutation {
            if namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE {
                let before = preconditions
                    .iter()
                    .find_map(|condition| match condition {
                        StoreJsonPrecondition::Exact {
                            namespace: expected_namespace,
                            key: expected_key,
                            value,
                        } if expected_namespace == namespace && expected_key == key => Some(value),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        Error::config(stage, "material deletion requires exact prior material CAS")
                    })?;
                let before: AgentToolExperienceRevisionMaterialV2 = decode(before, stage)?;
                if before.physical_key != *key
                    || before.memory_space_id != batch.scope.memory_space_id
                    || before.owning_scope.mounted_subject_id() != batch.scope.subject_id
                {
                    return Err(Error::config(
                        stage,
                        "material deletion crosses exact operation scope",
                    ));
                }
            }
            continue;
        }
        let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        else {
            continue;
        };
        if namespace != AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE {
            continue;
        }
        let after: AgentToolExperienceOwnerHeadV2 = decode(value, stage)?;
        let before = preconditions.iter().find(|condition| match condition {
            StoreJsonPrecondition::Exact {
                namespace: expected_namespace,
                key: expected_key,
                ..
            }
            | StoreJsonPrecondition::Absent {
                namespace: expected_namespace,
                key: expected_key,
            } => expected_namespace == namespace && expected_key == key,
        });
        match before {
            Some(StoreJsonPrecondition::Absent { .. })
                if after.current_revision == 1
                    && after.state == AgentToolExperienceHeadStateV2::Active => {}
            Some(StoreJsonPrecondition::Exact { value, .. }) => {
                let before: AgentToolExperienceOwnerHeadV2 = decode(value, stage)?;
                if before.state == AgentToolExperienceHeadStateV2::Tombstoned {
                    return Err(Error::config(
                        stage,
                        "tombstoned owner commitment cannot be rewritten or resurrected",
                    ));
                }
                if after.state == AgentToolExperienceHeadStateV2::Tombstoned {
                    if after != before.tombstone()? {
                        return Err(Error::config(
                            stage,
                            "terminal head must retain exact historical commitments",
                        ));
                    }
                } else if before.current_revision.checked_add(1) != Some(after.current_revision)
                    || !after
                        .retained_revisions
                        .starts_with(&before.retained_revisions)
                    || before.owner_ref != after.owner_ref
                    || before.memory_space_id != after.memory_space_id
                    || before.owning_scope != after.owning_scope
                {
                    return Err(Error::config(
                        stage,
                        "owner advance must preserve the exact prior head history",
                    ));
                }
            }
            _ => {
                return Err(Error::config(
                    stage,
                    "owner transition requires exact prior head CAS",
                ))
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_agent_tool_experience_transaction_post_image(
    batch: &StoreMutationBatch,
    after: &BackendTransactionState,
    budget: Option<AgentToolExperienceStoreBudget>,
) -> Result<()> {
    let dedicated = [
        AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
        AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
        AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
    ];
    if !batch.mutations.iter().any(|mutation| {
        matches!(mutation,
            StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. }
                if dedicated.contains(&namespace.as_str()))
    }) {
        return Ok(());
    }
    let budget = budget.ok_or_else(|| {
        Error::config(
            "agent_tool_experience_store_post_image",
            "Agent Tool experience mutation requires pinned governed limits",
        )
    })?;
    let lifecycle = batch.mutations.iter().any(|mutation| matches!(mutation,
        StoreMutation::PutJson { namespace, value, .. }
            if namespace == MEMORY_MUTATION_RECEIPT_NAMESPACE
                && serde_json::from_value::<MemoryMutationReceipt>(value.clone()).is_ok_and(|receipt|
                    receipt.identity.operation_kind() == MemoryMutationOperationKind::ProceduralLifecycle)));
    let mut counts = BTreeMap::<&str, usize>::new();
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if dedicated.contains(&namespace.as_str()) => {
                *counts.entry(namespace.as_str()).or_default() += 1;
                let exact_scope = match namespace.as_str() {
                    AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
                        let material: AgentToolExperienceRevisionMaterialV2 =
                            decode(value, "agent_tool_experience_store_post_image")?;
                        material.memory_space_id == batch.scope.memory_space_id
                            && material.owning_scope.mounted_subject_id() == batch.scope.subject_id
                    }
                    AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE => {
                        let head: AgentToolExperienceOwnerHeadV2 =
                            decode(value, "agent_tool_experience_store_post_image")?;
                        head.memory_space_id == batch.scope.memory_space_id
                            && head.owning_scope.mounted_subject_id() == batch.scope.subject_id
                            && head.state
                                == if lifecycle {
                                    AgentToolExperienceHeadStateV2::Tombstoned
                                } else {
                                    AgentToolExperienceHeadStateV2::Active
                                }
                    }
                    AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE => {
                        let manifest: AgentToolExperienceScopeManifestV1 =
                            decode(value, "agent_tool_experience_store_post_image")?;
                        manifest.memory_space_id == batch.scope.memory_space_id
                            && manifest.owning_scope.mounted_subject_id() == batch.scope.subject_id
                    }
                    _ => unreachable!("dedicated namespace already matched"),
                };
                if !exact_scope {
                    return Err(Error::config(
                        "agent_tool_experience_store_post_image",
                        "Agent Tool experience mutation crosses the operation subject scope",
                    ));
                }
            }
            StoreMutation::DeleteJson { namespace, .. }
                if dedicated.contains(&namespace.as_str()) =>
            {
                if !lifecycle || namespace != AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE {
                    return Err(Error::config(
                    "agent_tool_experience_store_post_image",
                    "only typed lifecycle may delete revision materials; owner commitments must remain",
                    ));
                }
            }
            _ => {}
        }
    }
    let material_count = counts
        .get(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE)
        .copied()
        .unwrap_or(0);
    let head_count = counts
        .get(AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE)
        .copied()
        .unwrap_or(0);
    let manifest_count = counts
        .get(AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE)
        .copied()
        .unwrap_or(0);
    if (!lifecycle && (material_count == 0 || material_count != head_count))
        || (lifecycle && (material_count != 0 || head_count == 0))
        || manifest_count != 1
    {
        return Err(Error::config(
            "agent_tool_experience_store_post_image",
            "one procedural learning commit requires equal non-zero material/head writes and one final manifest",
        ));
    }
    let receipt = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == MEMORY_MUTATION_RECEIPT_NAMESPACE => Some(value),
            _ => None,
        })
        .map(|value| serde_json::from_value::<MemoryMutationReceipt>(value.clone()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| {
            Error::config("agent_tool_experience_store_post_image", error.to_string())
        })?;
    if receipt.len() != 1
        || receipt[0].identity.operation_kind()
            != if lifecycle {
                MemoryMutationOperationKind::ProceduralLifecycle
            } else {
                MemoryMutationOperationKind::ProceduralLearning
            }
        || receipt[0].identity.memory_space_id() != batch.scope.memory_space_id
        || receipt[0].identity.mounted_subject_id() != batch.scope.subject_id
        || batch.scope.physical_owning_scope
            != (StorePhysicalOwningScope::Subject {
                mounted_subject_id: batch.scope.subject_id.clone(),
            })
    {
        return Err(Error::config(
            "agent_tool_experience_store_post_image",
            "Agent Tool experience requires exact subject-scoped procedural-learning operation authority",
        ));
    }
    validate_agent_tool_experience_store_image(
        after,
        budget,
        "agent_tool_experience_store_post_image",
    )
}

pub(crate) fn validate_agent_tool_experience_store_image(
    state: &BackendTransactionState,
    budget: AgentToolExperienceStoreBudget,
    stage: &'static str,
) -> Result<()> {
    if budget.max_owners_per_subject == 0
        || budget.max_revisions_per_owner == 0
        || budget.max_evidence_refs_per_owner == 0
    {
        return Err(Error::config(
            stage,
            "Agent Tool experience limits must be positive",
        ));
    }
    let mut scopes = BTreeSet::new();
    let mut materials = Vec::new();
    let mut heads = Vec::new();
    let mut manifests = Vec::new();
    for ((namespace, key), value) in &state.json {
        match namespace.as_str() {
            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
                let material: AgentToolExperienceRevisionMaterialV2 = decode(value, stage)?;
                if material.physical_key != *key
                    || !material.validate_contract().accepted
                    || material.evidence_refs.len() > budget.max_evidence_refs_per_owner
                {
                    return Err(Error::config(
                        stage,
                        "invalid Agent Tool experience material",
                    ));
                }
                scopes.insert((
                    material.memory_space_id.clone(),
                    material.owning_scope.clone(),
                ));
                materials.push(material);
            }
            AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE => {
                let head: AgentToolExperienceOwnerHeadV2 = decode(value, stage)?;
                if head.physical_key != *key
                    || !head.validate_contract().accepted
                    || head.retained_revisions.len() > budget.max_revisions_per_owner
                {
                    return Err(Error::config(
                        stage,
                        "invalid Agent Tool experience owner head",
                    ));
                }
                scopes.insert((head.memory_space_id.clone(), head.owning_scope.clone()));
                heads.push(head);
            }
            AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE => {
                let manifest: AgentToolExperienceScopeManifestV1 = decode(value, stage)?;
                if manifest.physical_key != *key || manifest.bindings.is_empty() {
                    return Err(Error::config(
                        stage,
                        "invalid Agent Tool experience scope manifest",
                    ));
                }
                scopes.insert((
                    manifest.memory_space_id.clone(),
                    manifest.owning_scope.clone(),
                ));
                manifests.push(manifest);
            }
            _ => {}
        }
    }
    for (memory_space_id, owning_scope) in scopes {
        let scope_materials = materials
            .iter()
            .filter(|material| {
                material.memory_space_id == memory_space_id && material.owning_scope == owning_scope
            })
            .cloned()
            .collect::<Vec<_>>();
        let scope_heads = heads
            .iter()
            .filter(|head| {
                head.memory_space_id == memory_space_id && head.owning_scope == owning_scope
            })
            .cloned()
            .collect::<Vec<_>>();
        let scope_manifests = manifests
            .iter()
            .filter(|manifest| {
                manifest.memory_space_id == memory_space_id && manifest.owning_scope == owning_scope
            })
            .collect::<Vec<_>>();
        if scope_heads.is_empty()
            || scope_heads.len() > budget.max_owners_per_subject
            || scope_manifests.len() != 1
        {
            return Err(Error::config(
                stage,
                "Agent Tool experience scope has an orphan, missing manifest, or capacity overflow",
            ));
        }
        for head in &scope_heads {
            let mut history = scope_materials
                .iter()
                .filter(|material| material.owner_ref == head.owner_ref)
                .cloned()
                .collect::<Vec<_>>();
            history.sort_by_key(|material| material.owner_revision);
            if head.state == AgentToolExperienceHeadStateV2::Tombstoned {
                if !history.is_empty() {
                    return Err(Error::config(
                        stage,
                        "tombstoned experience retains raw material",
                    ));
                }
                continue;
            }
            if history.len() != head.retained_revisions.len() {
                return Err(Error::config(
                    stage,
                    "Agent Tool experience head does not own the exact retained history",
                ));
            }
            validate_agent_tool_experience_owner_history(&history)?;
        }
        validate_agent_tool_experience_scope_closure(
            scope_manifests[0],
            &scope_heads,
            &scope_materials,
            budget.max_owners_per_subject,
        )?;
    }
    Ok(())
}

fn encode<T: Serialize>(value: &T, stage: &'static str) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|error| Error::config(stage, error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    stage: &'static str,
) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|error| Error::config(stage, error.to_string()))
}

fn exact_or_absent<T: Serialize>(
    namespace: &str,
    key: &str,
    before: Option<&T>,
) -> Result<StoreJsonPrecondition> {
    match before {
        Some(value) => Ok(StoreJsonPrecondition::Exact {
            namespace: namespace.to_string(),
            key: key.to_string(),
            value: encode(value, "agent_tool_experience_precondition")?,
        }),
        None => Ok(StoreJsonPrecondition::Absent {
            namespace: namespace.to_string(),
            key: key.to_string(),
        }),
    }
}

fn put_json(namespace: &str, key: &str, value: serde_json::Value) -> StoreMutation {
    StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value,
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: "agent_tool_experience".to_string(),
        record_key: key.to_string(),
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use bm_core::memory::MemoryPrivacyClass;
    use bm_core::skills::{
        AgentToolExperienceConfidence, AgentToolExperienceRetainedRevisionDigestV2,
        AgentToolExperienceStatus, AgentToolOutcome, AgentToolRegistryScope,
    };

    fn material(subject: &str) -> AgentToolExperienceRevisionMaterialV2 {
        AgentToolExperienceRevisionMaterialV2::build(
            "space",
            AgentToolExperienceOwningScopeV1::Subject {
                mounted_subject_id: subject.to_string(),
            },
            "tools",
            AgentToolRegistryScope::Global,
            "read",
            "schema",
            "read-document",
            1,
            "private source",
            "governed guidance",
            vec![],
            2,
            2,
            0,
            AgentToolOutcome::Succeeded,
            AgentToolExperienceConfidence::High,
            AgentToolExperienceStatus::Active,
            vec!["source-a".to_string()],
            MemoryPrivacyClass::SharedWithSubject,
            100,
            101,
            None,
        )
        .expect("material")
    }

    fn scope() -> StoreEventScope {
        StoreEventScope::new("agent-a", "human", "test", "chat")
            .with_memory_space("space")
            .with_subject("agent-a")
    }

    #[test]
    fn raw_material_delete_requires_exact_same_subject_before_image() {
        let own = material("agent-a");
        let other = material("agent-b");
        for (source, allowed) in [(own, true), (other, false)] {
            let batch = StoreMutationBatch {
                transaction_id: "lifecycle".to_string(),
                operation: "lifecycle".to_string(),
                scope: scope(),
                mutations: vec![StoreMutation::DeleteJson {
                    namespace: AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.to_string(),
                    key: source.physical_key.clone(),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "agent_tool_experience".to_string(),
                    record_key: source.physical_key.clone(),
                }],
            };
            let before = StoreJsonPrecondition::Exact {
                namespace: AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.to_string(),
                key: source.physical_key.clone(),
                value: serde_json::to_value(&source).expect("value"),
            };
            assert_eq!(
                validate_agent_tool_experience_transition_preconditions(&batch, &[before]).is_ok(),
                allowed
            );
            assert!(validate_agent_tool_experience_transition_preconditions(&batch, &[]).is_err());
        }
    }

    #[test]
    fn terminal_commitment_cannot_be_resurrected_or_rewritten() {
        let material = material("agent-a");
        let active = AgentToolExperienceOwnerHeadV2::build(
            &material.memory_space_id,
            material.owning_scope.clone(),
            material.owner_ref.clone(),
            1,
            vec![
                AgentToolExperienceRetainedRevisionDigestV2::from_material(&material)
                    .expect("retained"),
            ],
        )
        .expect("head");
        let terminal = active.tombstone().expect("terminal");
        let batch_for = |head: &AgentToolExperienceOwnerHeadV2| StoreMutationBatch {
            transaction_id: "lifecycle".to_string(),
            operation: "lifecycle".to_string(),
            scope: scope(),
            mutations: vec![put_json(
                AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
                &head.physical_key,
                serde_json::to_value(head).expect("head value"),
            )],
        };
        let before_for = |head: &AgentToolExperienceOwnerHeadV2| StoreJsonPrecondition::Exact {
            namespace: AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.to_string(),
            key: head.physical_key.clone(),
            value: serde_json::to_value(head).expect("head value"),
        };
        validate_agent_tool_experience_transition_preconditions(
            &batch_for(&terminal),
            &[before_for(&active)],
        )
        .expect("terminal positive");
        assert!(validate_agent_tool_experience_transition_preconditions(
            &batch_for(&active),
            &[before_for(&terminal)]
        )
        .is_err());
        assert!(validate_agent_tool_experience_transition_preconditions(
            &batch_for(&terminal),
            &[before_for(&terminal)]
        )
        .is_err());
    }
}
