//! The existing learning worker's subject-wide, bounded reconciliation branch.
//! No fabricated turn, second queue or host-owned permission state is involved.
use super::*;
use crate::store_internal::procedural_feedback::{
    self as owner, ProceduralReconciliationCommit,
    ProceduralReconciliationPageAction as PageAction, ProceduralReconciliationPageInput,
};
use crate::store_internal::schema::{
    PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE, PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
    PROCEDURAL_PRODUCER_HEAD_NAMESPACE, PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
    RUNTIME_SKILL_RECORD_NAMESPACE, RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
};
use bm_core::memory::{GovernedOwnerRevisionRef, ProceduralFeedbackJobStatusV1};
use bm_core::memory::{
    ProceduralAppliedOwnerBindingV1 as Binding, ProceduralFeedbackApplicationLedgerV2,
    ProceduralLearningWorkV1, ProceduralProducerStateV1, ProceduralProducerToolV1,
    ProceduralReconciliationCheckpointV1, ProceduralReconciliationCursorV1 as Cursor,
    ProceduralReconciliationDependencyPinV1 as Dependency,
    ProceduralReconciliationManifestPinV1 as Pin, ProceduralReconciliationManifestPinsV1 as Pins,
    ProceduralReconciliationOwnerProofV1 as Proof,
    ProceduralReconciliationReadDispositionV1 as Disposition, ProceduralSubjectValidityRootV1,
    ToolExecutionContributionV1,
};
use bm_core::skills::{
    AgentToolExperienceBodyV1 as Body, AgentToolExperienceHeadStateV3, AgentToolExperienceStatus,
};

struct Source {
    ledger: ProceduralFeedbackApplicationLedgerV2,
    evidence: Option<PostTurnLearningEvidenceV2>,
    producer: Option<owner::VerifiedProceduralProducer>,
    permitted: bool,
}

fn repair() -> Error {
    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
}

/// Only immutable revisions of this exact asset can attest that a source was
/// actually applied. A declaration merely present in a ledger is not enough.
/// Counters here are deliberately not reused; the Core reducer rebuilds them
/// after current authority has selected the valid subset.
fn complete_applied_tool_body(
    current: &AgentToolExperienceRevisionMaterialV3,
    history: &[AgentToolExperienceRevisionMaterialV3],
    max_references: usize,
) -> Result<Body> {
    let mut body = current.body.clone();
    let merge_refs = |target: &mut Vec<bm_core::memory::ProceduralContributionRefV1>,
                      incoming: &[bm_core::memory::ProceduralContributionRefV1]|
     -> Result<()> {
        let mut refs = target
            .iter()
            .map(|reference| (reference.contribution_id.clone(), reference.clone()))
            .collect::<BTreeMap<_, _>>();
        for reference in incoming {
            if refs
                .insert(reference.contribution_id.clone(), reference.clone())
                .is_some_and(|before| before != *reference)
            {
                return Err(repair());
            }
        }
        *target = refs.into_values().collect();
        Ok(())
    };
    for material in history {
        if material.owner_ref != current.owner_ref
            || material.memory_space_id != current.memory_space_id
            || material.owning_scope != current.owning_scope
            || !material.validate_contract().accepted
        {
            return Err(repair());
        }
        match (&mut body, &material.body) {
            (
                Body::Execution { contributions, .. },
                Body::Execution {
                    contributions: old, ..
                },
            ) => merge_refs(contributions, old)?,
            (
                Body::Method {
                    task_signature,
                    procedure,
                    sources,
                    ..
                },
                Body::Method {
                    task_signature: old_task,
                    procedure: old_procedure,
                    sources: old_sources,
                    ..
                },
            ) => {
                if task_signature != old_task || procedure != old_procedure {
                    continue;
                }
                for source in old_sources {
                    if let Some(retained) = sources.iter_mut().find(|retained| {
                        retained.source_job_id == source.source_job_id
                            && retained.method_id == source.method_id
                    }) {
                        if retained.method_digest != source.method_digest {
                            return Err(repair());
                        }
                        merge_refs(&mut retained.execution_refs, &source.execution_refs)?;
                    } else {
                        sources.push(source.clone());
                    }
                }
                sources.sort_by(|a, b| {
                    (&a.source_job_id, &a.method_id).cmp(&(&b.source_job_id, &b.method_id))
                });
            }
            _ => return Err(repair()),
        }
        if body.contribution_count() > max_references {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::BudgetExceeded,
            ));
        }
    }
    Ok(body)
}

fn read<T: serde::de::DeserializeOwned>(
    store: &StorePlatform,
    namespace: &str,
    key: &str,
    conditions: &mut Vec<StoreJsonPrecondition>,
) -> Result<Option<T>> {
    let value = store
        .read_json_docs_by_keys(namespace, &[key.to_owned()])?
        .pop()
        .map(|document| document.value);
    conditions.push(match &value {
        Some(value) => StoreJsonPrecondition::Exact {
            namespace: namespace.into(),
            key: key.into(),
            value: value.clone(),
        },
        None => StoreJsonPrecondition::Absent {
            namespace: namespace.into(),
            key: key.into(),
        },
    });
    value
        .map(|value| serde_json::from_value(value).map_err(|_| repair()))
        .transpose()
}

impl MemoryRuntime {
    pub fn procedural_reconciliation_status(
        &self,
        request: crate::MemoryProceduralReconciliationStatusRequest,
    ) -> Result<crate::MemoryProceduralReconciliationStatusReport> {
        use bm_core::memory::{
            MemoryMutationAuditRecord, MemoryMutationReceipt,
            ProceduralLearningReadAvailabilityV1 as Availability, ProceduralReconciliationBlockV1,
            ProceduralSubjectValidityStateV1 as State, MEMORY_MUTATION_AUDIT_NAMESPACE,
            MEMORY_MUTATION_RECEIPT_NAMESPACE,
        };
        let operation = crate::ProceduralLearningSdkOperation::Status;
        (|| {
            if RuntimeBudgetLease::active_report(&self.config.runtime_budget_authority).is_none() {
                let lease = self.acquire_runtime_budget_lease()?;
                return self.execute_with_runtime_budget_lease(&lease, || {
                    self.procedural_reconciliation_status(request)
                });
            }
            // Revalidate the issuer as well as the supplied non-wire seal.
            self.learning_attachment_status_authority().map_err(|_| {
                procedural_error(
                    operation,
                    crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
                )
            })?;
            if !request
                .authority
                .authorizes(&self.governance_attachment_identity()?)
            {
                return Err(procedural_error(
                    operation,
                    crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
                ));
            }
            let store = self.config.store_platform.as_ref().ok_or_else(repair)?;
            let outcome =
                store.with_recall_immutable_read_session(&self.runtime_budget(), |context| {
                    let mut report = crate::MemoryProceduralReconciliationStatusReport {
                        read_availability: Availability::Ready,
                        block_reason: None,
                        recovery: None,
                    };
                    let Some(root) = context.read_subject_learning_root(
                        &self.config.memory_space_id,
                        &self.config.scoped_runtime.mounted_subject_id,
                    )?
                    else {
                        return Ok(report);
                    };
                    let reference = match &root.state {
                        State::Ready { completion } => {
                            completion.as_ref().map(|completion| &completion.work)
                        }
                        State::Reconciling { work } => {
                            report.read_availability = Availability::Reconciling;
                            Some(work)
                        }
                        State::Blocked { work, reason } => {
                            report.read_availability = Availability::Blocked;
                            report.block_reason = Some(*reason);
                            Some(work)
                        }
                    };
                    let Some(reference) = reference else {
                        return Ok(report);
                    };
                    let job: ProceduralFeedbackJobV2 = context
                        .read_json(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &reference.job_id)?
                        .ok_or_else(repair)?;
                    owner::validate_subject_reconciliation_job(&root, &job)?;
                    if let Some(authority) = &job.checkpoint_authority {
                        let key = authority.operation.storage_key();
                        let receipt: MemoryMutationReceipt = context
                            .read_json(MEMORY_MUTATION_RECEIPT_NAMESPACE, &key)?
                            .ok_or_else(repair)?;
                        let audit: MemoryMutationAuditRecord = context
                            .read_json(MEMORY_MUTATION_AUDIT_NAMESPACE, &key)?
                            .ok_or_else(repair)?;
                        owner::validate_learning_mutation_proof(
                            &receipt,
                            &audit,
                            "procedural_reconciliation_status",
                        )?;
                        owner::validate_reconciliation_checkpoint_commitment(&job, &receipt)?;
                    }
                    if let Some(completion) = &job.receipt {
                        let key = completion.mutation_receipt_key();
                        let receipt: MemoryMutationReceipt = context
                            .read_json(MEMORY_MUTATION_RECEIPT_NAMESPACE, key)?
                            .ok_or_else(repair)?;
                        let audit: MemoryMutationAuditRecord = context
                            .read_json(MEMORY_MUTATION_AUDIT_NAMESPACE, key)?
                            .ok_or_else(repair)?;
                        owner::validate_reconciliation_completion(&job, &receipt, &audit)?;
                    }
                    if report.block_reason == Some(ProceduralReconciliationBlockV1::Capacity) {
                        report.recovery =
                            Some(crate::MemoryProceduralReconciliationRecoveryTarget {
                                job_id: job.job_id,
                                expected_state_revision: job.state_revision,
                            });
                    }
                    Ok(report)
                })?;
            Ok(outcome.output)
        })()
        .map_err(|error| {
            crate::ProceduralLearningSdkError::from_owner_error(operation, &error).into_core_error()
        })
    }

    pub fn resume_procedural_reconciliation(
        &self,
        request: crate::MemoryProceduralReconciliationResumeRequest,
    ) -> Result<crate::MemoryProceduralReconciliationResumeReport> {
        let operation = crate::ProceduralLearningSdkOperation::ResumeReconciliation;
        (|| {
            self.ensure_visible(
                "write.procedural_reconciliation_resume",
                self.capabilities.write,
            )
            .map_err(|_| {
                procedural_error(
                    operation,
                    crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable,
                )
            })?;
            if !self.procedural_subject_active() {
                return Err(procedural_error(
                    operation,
                    crate::ProceduralLearningErrorKeyV1::ScopeMismatch,
                ));
            }
            let governor = self
                .exact_system_governor_actor("procedural_reconciliation_resume")
                .map_err(|_| {
                    procedural_error(
                        operation,
                        crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
                    )
                })?;
            let identity = bm_core::memory::MemoryMutationOperationIdentity::new(
                &request.operation_id,
                &self.config.memory_space_id,
                &self.config.scoped_runtime.mounted_subject_id,
                governor,
                bm_core::memory::MemoryMutationOperationKind::ProceduralLearning,
            )?;
            let store = self.config.store_platform.as_ref().ok_or_else(repair)?;
            owner::resume_reconciliation_capacity(
                store,
                self.memory_write_transaction_scope(),
                &self.runtime_budget(),
                identity,
                request,
                self.config.clock.now_secs(),
            )
        })()
        .map_err(|error| {
            crate::ProceduralLearningSdkError::from_owner_error(operation, &error).into_core_error()
        })
    }

    pub(crate) fn run_claimed_procedural_reconciliation_job(
        &self,
        job: &ProceduralFeedbackJobV2,
        lease_owner: &str,
    ) -> Result<ProceduralReconciliationCommit> {
        if !self.capabilities.procedural_learning.worker.visible
            || !self.procedural_subject_active()
            || job.work.subject_scope().memory_space_id != self.config.memory_space_id
            || job.work.subject_scope().mounted_subject_id
                != self.config.scoped_runtime.mounted_subject_id
        {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
            ));
        }
        let ProceduralLearningWorkV1::Reconcile { source: work } = &job.work else {
            return Err(repair());
        };
        let store = self.config.store_platform.as_ref().ok_or_else(repair)?;
        if let Some(replay) = owner::replay_reconciliation_page(
            store,
            &self.memory_write_transaction_scope(),
            job,
            lease_owner,
            &self.config.scoped_runtime.actor_subject_id,
        )? {
            return Ok(replay);
        }
        let budget = self.runtime_budget();
        let now = self.config.clock.now_secs();
        let mut conditions = Vec::new();
        let root: ProceduralSubjectValidityRootV1 = read(
            store,
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            &job.discovery_root_key,
            &mut conditions,
        )?
        .ok_or_else(repair)?;
        root.validate(bm_core::memory::MAX_PROCEDURAL_SUBJECT_SCOPES)?;
        let tool_scope = AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: work.scope.mounted_subject_id.clone(),
        };
        let skill_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: work.scope.mounted_subject_id.clone(),
        };
        let tool_key =
            agent_tool_experience_scope_manifest_key(&work.scope.memory_space_id, &tool_scope)?;
        let skill_key =
            runtime_skill_scope_manifest_key(&work.scope.memory_space_id, &skill_scope)?;
        let mut tools: Option<AgentToolExperienceScopeManifestV1> = read(
            store,
            AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
            &tool_key,
            &mut conditions,
        )?;
        let mut skills: Option<RuntimeSkillScopeManifest> = read(
            store,
            RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            &skill_key,
            &mut conditions,
        )?;
        if let Some(manifest) = &tools {
            manifest.validate_exact(
                manifest.bindings.clone(),
                budget
                    .governed_state_budget
                    .max_agent_tool_experience_owners_per_subject,
            )?;
        }
        if let Some(manifest) = &skills {
            manifest.validate_exact(
                &work.scope.memory_space_id,
                &skill_scope,
                manifest.owner_bindings.clone(),
                budget
                    .governed_state_budget
                    .max_retained_runtime_skill_owners_per_scope,
            )?;
        }
        let pins = |tools: &Option<AgentToolExperienceScopeManifestV1>,
                    skills: &Option<RuntimeSkillScopeManifest>| Pins {
            agent_tool: tools.as_ref().map_or(Pin::Absent, |manifest| Pin::Present {
                revision: manifest.revision,
                bindings_digest: manifest.bindings_digest.clone(),
            }),
            runtime_skill: skills
                .as_ref()
                .map_or(Pin::Absent, |manifest| Pin::Present {
                    revision: manifest.revision,
                    bindings_digest: manifest.bindings_digest.clone(),
                }),
        };
        let mut checkpoint = match &job.checkpoint {
            Some(checkpoint) => checkpoint.clone(),
            None => ProceduralReconciliationCheckpointV1::begin(
                work,
                &root,
                pins(&tools, &skills),
                now,
            )?,
        };
        checkpoint.validate_for(work)?;
        let snapshot_changed = checkpoint.root_revision != root.revision
            || checkpoint.root_digest != root.content_digest
            || checkpoint.current_manifests != pins(&tools, &skills);
        let dependencies_changed = if snapshot_changed {
            false
        } else {
            match self
                .refresh_reconciliation_dependencies(&checkpoint.dependencies, &mut conditions)
            {
                Ok(()) => false,
                Err(Error::Conflict { .. }) => true,
                Err(error) => return Err(error),
            }
        };
        if snapshot_changed || dependencies_changed {
            let checkpoint = ProceduralReconciliationCheckpointV1::begin(
                work,
                &root,
                pins(&tools, &skills),
                now,
            )?;
            let mut exact_conditions = Vec::new();
            merge_json_preconditions(&mut exact_conditions, conditions)?;
            return owner::commit_reconciliation_page(
                store,
                self.memory_write_transaction_scope(),
                &budget,
                ProceduralReconciliationPageInput {
                    claimed_job: job.clone(),
                    lease_owner: lease_owner.into(),
                    actor_subject_id: self.config.scoped_runtime.actor_subject_id.clone(),
                    checkpoint,
                    mutations: Vec::new(),
                    preconditions: exact_conditions,
                    action: PageAction::RestartSnapshot,
                    now,
                },
            );
        }
        let finish = matches!(checkpoint.cursor, Cursor::Complete);
        let mut mutations = Vec::new();
        let mut sources = BTreeMap::new();
        match checkpoint.cursor.clone() {
            Cursor::AgentTool { after } => {
                let next = tools
                    .as_ref()
                    .and_then(|manifest| {
                        manifest.bindings.iter().find(|binding| {
                            after
                                .as_ref()
                                .is_none_or(|previous| binding.owner_ref > previous.owner_ref)
                        })
                    })
                    .cloned();
                if let Some(binding) = next {
                    let head: AgentToolExperienceOwnerHeadV3 = read(
                        store,
                        AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
                        &binding.head_key,
                        &mut conditions,
                    )?
                    .ok_or_else(repair)?;
                    if AgentToolExperienceHeadBindingV1::from_head(&head)? != binding {
                        return Err(repair());
                    }
                    if head.retained_revisions.len()
                        > budget
                            .governed_state_budget
                            .max_agent_tool_experience_revisions_per_owner
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::BudgetExceeded,
                        ));
                    }
                    let mut current_material = None;
                    let mut applied_materials = Vec::new();
                    for retained in &head.retained_revisions {
                        let exact = Binding::AgentToolExperience {
                            owner_revision: GovernedOwnerRevisionRef::try_new(
                                head.owner_ref.clone(),
                                retained.owner_revision,
                            )?,
                            content_digest: retained.content_digest.clone(),
                        };
                        let disposition =
                            if head.state == AgentToolExperienceHeadStateV3::Tombstoned {
                                Disposition::Withdrawn
                            } else {
                                let material: AgentToolExperienceRevisionMaterialV3 = read(
                                    store,
                                    AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                                    &retained.material_key,
                                    &mut conditions,
                                )?
                                .ok_or_else(repair)?;
                                if !material.validate_contract().accepted
                                    || material.content_digest != retained.content_digest
                                    || material.owner_ref != head.owner_ref
                                    || material.owner_revision != retained.owner_revision
                                {
                                    return Err(repair());
                                }
                                let reduced = self.reconcile_tool_material(
                                    &material,
                                    &material.body,
                                    &mut sources,
                                    &mut checkpoint.dependencies,
                                    &mut conditions,
                                )?;
                                let permitted = reduced.as_ref().is_some_and(|(body, status)| {
                                    body == &material.body && status == &material.status
                                });
                                if material.owner_revision == head.current_revision {
                                    current_material = Some(material.clone());
                                }
                                applied_materials.push(material);
                                if permitted {
                                    Disposition::Visible
                                } else {
                                    Disposition::Withdrawn
                                }
                            };
                        checkpoint.verified_owners.push(Proof {
                            binding: exact,
                            disposition,
                        });
                    }
                    let current_reduction = if let Some(material) = current_material {
                        let complete = complete_applied_tool_body(
                            &material,
                            &applied_materials,
                            budget
                                .governed_state_budget
                                .max_agent_tool_experience_evidence_refs_per_owner,
                        )?;
                        let reduced = self.reconcile_tool_material(
                            &material,
                            &complete,
                            &mut sources,
                            &mut checkpoint.dependencies,
                            &mut conditions,
                        )?;
                        Some((material, reduced))
                    } else {
                        None
                    };
                    let mut last_revision = head.current_revision;
                    if let Some((material, Some((body, status)))) = current_reduction {
                        if body != material.body || status != material.status {
                            let revision =
                                head.current_revision.checked_add(1).ok_or_else(repair)?;
                            if revision as usize
                                > budget
                                    .governed_state_budget
                                    .max_agent_tool_experience_revisions_per_owner
                            {
                                return Err(feedback_error(
                                    crate::ProceduralLearningErrorKeyV1::BudgetExceeded,
                                ));
                            }
                            let next_material = AgentToolExperienceRevisionMaterialV3::build(
                                &material.memory_space_id,
                                material.owning_scope.clone(),
                                &material.registry_id,
                                material.registry_scope.clone(),
                                &material.tool_id,
                                &material.schema_fingerprint,
                                revision,
                                body,
                                status,
                                material.privacy_class,
                                material.created_at,
                                now,
                                Some(AgentToolExperienceRetainedRevisionDigestV3::from_material(
                                    &material,
                                )?),
                            )?;
                            let mut retained = head.retained_revisions.clone();
                            retained.push(
                                AgentToolExperienceRetainedRevisionDigestV3::from_material(
                                    &next_material,
                                )?,
                            );
                            let next_head = AgentToolExperienceOwnerHeadV3::build(
                                &material.memory_space_id,
                                material.owning_scope.clone(),
                                head.owner_ref.clone(),
                                revision,
                                retained,
                            )?;
                            let manifest = tools.as_ref().ok_or_else(repair)?;
                            let mut bindings = manifest.bindings.clone();
                            bindings.retain(|binding| binding.owner_ref != head.owner_ref);
                            bindings.push(
                                AgentToolExperienceHeadBindingV1::from_head_and_material(
                                    &next_head,
                                    &next_material,
                                )?,
                            );
                            let next_manifest = AgentToolExperienceScopeManifestV1::build(
                                manifest.revision.checked_add(1).ok_or_else(repair)?,
                                &manifest.memory_space_id,
                                manifest.owning_scope.clone(),
                                bindings,
                                budget
                                    .governed_state_budget
                                    .max_agent_tool_experience_owners_per_subject,
                            )?;
                            let proof = Binding::AgentToolExperience {
                                owner_revision: next_material.owner_revision_ref(),
                                content_digest: next_material.content_digest.clone(),
                            };
                            let plan = AgentToolExperienceStoreMutationPlanV1::try_new(
                                format!("reconcile-{}", job.job_id),
                                self.config.scoped_runtime.actor_subject_id.clone(),
                                self.memory_write_transaction_scope(),
                                Some(head.clone()),
                                tools.clone(),
                                next_material,
                                next_head,
                                next_manifest.clone(),
                                now,
                            )?;
                            let (planned, expected, _) = combine_agent_tool_experience_plans(
                                &[plan],
                                budget
                                    .governed_state_budget
                                    .max_agent_tool_experience_owners_per_subject,
                            )?;
                            mutations.extend(planned);
                            merge_json_preconditions(&mut conditions, expected)?;
                            tools = Some(next_manifest);
                            last_revision = revision;
                            checkpoint.verified_owners.push(Proof {
                                binding: proof,
                                disposition: Disposition::Visible,
                            });
                        }
                    }
                    checkpoint.cursor = Cursor::AgentTool {
                        after: Some(GovernedOwnerRevisionRef::try_new(
                            head.owner_ref,
                            last_revision,
                        )?),
                    };
                } else {
                    checkpoint.cursor = Cursor::RuntimeSkill { after: None };
                }
            }
            Cursor::RuntimeSkill { after } => {
                let next = skills
                    .as_ref()
                    .and_then(|manifest| {
                        manifest.owner_bindings.iter().find(|binding| {
                            after
                                .as_ref()
                                .is_none_or(|previous| binding.owner_ref > previous.owner_ref)
                        })
                    })
                    .cloned();
                if let Some(binding) = next {
                    let current: RuntimeSkillOwnerRecord = read(
                        store,
                        RUNTIME_SKILL_RECORD_NAMESPACE,
                        &binding.owner_physical_key,
                        &mut conditions,
                    )?
                    .ok_or_else(repair)?;
                    if RuntimeSkillOwnerBinding::from_record(&current)? != binding {
                        return Err(repair());
                    }
                    let mut allowed = true;
                    if let RuntimeSkillCreationRef::AgentToolExperiencePromotion {
                        experience_owner_ref,
                    } = &current.creation_ref
                    {
                        let tool_binding = tools
                            .as_ref()
                            .and_then(|manifest| {
                                manifest
                                    .bindings
                                    .iter()
                                    .find(|binding| &binding.owner_ref == experience_owner_ref)
                            })
                            .ok_or_else(repair)?;
                        let method_revision = GovernedOwnerRevisionRef::try_new(
                            experience_owner_ref.clone(),
                            tool_binding.current_revision,
                        )?;
                        let method_proof = checkpoint
                            .verified_owners
                            .iter()
                            .find(|proof| proof.binding.owner_revision_ref() == method_revision)
                            .ok_or_else(repair)?;
                        allowed = method_proof.disposition == Disposition::Visible;
                        if allowed {
                            let method: AgentToolExperienceRevisionMaterialV3 = read(
                                store,
                                AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                                &tool_binding.material_key,
                                &mut conditions,
                            )?
                            .ok_or_else(repair)?;
                            if method_proof.binding
                                != (Binding::AgentToolExperience {
                                    owner_revision: method.owner_revision_ref(),
                                    content_digest: method.content_digest.clone(),
                                })
                            {
                                return Err(repair());
                            }
                            // Creation must reproduce the accepted method. Later
                            // revisions are governed by the RuntimeSkill owner;
                            // source permission is not an eternal text override.
                            allowed = method.status == AgentToolExperienceStatus::Active
                                && matches!(&method.body,
                                Body::Method { procedure, .. } if current.owner_revision != 1
                                    || procedure == &current.procedural_content.procedure);
                        }
                        for evidence in &current.intrinsic_contract.evidence_bindings {
                            self.reconciliation_source(
                                &evidence.safe_ref,
                                &mut sources,
                                &mut checkpoint.dependencies,
                                &mut conditions,
                            )?;
                            allowed &= sources
                                .get(&evidence.safe_ref)
                                .is_some_and(|source| source.permitted);
                        }
                    }
                    let mut usage = Vec::new();
                    for reference in &current.lifecycle.usage_outcome.retained_contributions {
                        self.reconciliation_source(
                            &reference.source_job_id,
                            &mut sources,
                            &mut checkpoint.dependencies,
                            &mut conditions,
                        )?;
                        let source = sources.get(&reference.source_job_id).ok_or_else(repair)?;
                        let contribution = source
                            .ledger
                            .runtime_skill_contributions
                            .iter()
                            .find(|value| value.reference().ok().as_ref() == Some(reference))
                            .ok_or_else(repair)?;
                        if source.permitted
                            && source.producer.as_ref().is_some_and(|producer| {
                                producer.current.spec.claims.usage_feedback
                                    && producer.historical.spec.claims.usage_feedback
                            })
                        {
                            usage.push(contribution.clone());
                        }
                    }
                    // Terminal owners retain only previously accepted sources;
                    // Core may update their statistics without reactivating or
                    // reauthoring them. Retirement is not a privacy exemption.
                    let next = current.apply_usage_contributions(
                        &usage,
                        budget
                            .governed_state_budget
                            .max_agent_tool_experience_evidence_refs_per_owner,
                        now,
                    )?;
                    if next != current {
                        let plan = self.plan_runtime_skill_owner_records(
                            &current.owning_scope,
                            vec![next.clone()],
                        )?;
                        if !plan.blob_preconditions.is_empty() {
                            return Err(repair());
                        }
                        mutations.extend(plan.mutations);
                        merge_json_preconditions(&mut conditions, plan.preconditions)?;
                        let manifest = skills.as_ref().ok_or_else(repair)?;
                        let mut bindings = manifest.owner_bindings.clone();
                        bindings.retain(|binding| binding.owner_ref != current.owner_ref);
                        bindings.push(RuntimeSkillOwnerBinding::from_record(&next)?);
                        skills = Some(RuntimeSkillScopeManifest::build(
                            manifest.revision.checked_add(1).ok_or_else(repair)?,
                            &manifest.memory_space_id,
                            manifest.owning_scope.clone(),
                            bindings,
                            budget
                                .governed_state_budget
                                .max_retained_runtime_skill_owners_per_scope,
                        )?);
                    }
                    checkpoint.verified_owners.push(Proof {
                        binding: Binding::RuntimeSkill {
                            binding: RuntimeSkillOwnerBinding::from_record(&next)?,
                        },
                        disposition: if allowed {
                            Disposition::Visible
                        } else {
                            Disposition::Withdrawn
                        },
                    });
                    checkpoint.cursor = Cursor::RuntimeSkill {
                        after: Some(next.owner_revision_ref()),
                    };
                } else {
                    checkpoint.cursor = Cursor::Complete;
                }
            }
            Cursor::Complete => {}
        }
        if !finish {
            checkpoint.page_number = checkpoint.page_number.checked_add(1).ok_or_else(repair)?;
            checkpoint.updated_at = now;
            checkpoint.current_manifests = pins(&tools, &skills);
            checkpoint
                .verified_owners
                .sort_by_key(|proof| proof.binding.owner_revision_ref());
            let mut ordered = checkpoint
                .dependencies
                .into_iter()
                .map(|pin| {
                    serde_json::to_vec(&pin)
                        .map(|key| (key, pin))
                        .map_err(|_| repair())
                })
                .collect::<Result<Vec<_>>>()?;
            ordered.sort_by(|left, right| left.0.cmp(&right.0));
            ordered.dedup_by(|left, right| left.0 == right.0);
            checkpoint.dependencies = ordered.into_iter().map(|(_, pin)| pin).collect();
            checkpoint.content_digest = checkpoint.canonical_digest()?;
        }
        let mut exact_conditions = Vec::new();
        merge_json_preconditions(&mut exact_conditions, conditions)?;
        owner::commit_reconciliation_page(
            store,
            self.memory_write_transaction_scope(),
            &budget,
            ProceduralReconciliationPageInput {
                claimed_job: job.clone(),
                lease_owner: lease_owner.into(),
                actor_subject_id: self.config.scoped_runtime.actor_subject_id.clone(),
                checkpoint,
                mutations,
                preconditions: exact_conditions,
                action: if finish {
                    PageAction::Publish
                } else {
                    PageAction::Advance
                },
                now,
            },
        )
    }

    fn reconciliation_source(
        &self,
        job_id: &str,
        sources: &mut BTreeMap<String, Source>,
        dependencies: &mut Vec<Dependency>,
        conditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        if sources.contains_key(job_id) {
            return Ok(());
        }
        let store = self.config.store_platform.as_ref().ok_or_else(repair)?;
        let ledger: ProceduralFeedbackApplicationLedgerV2 = read(
            store,
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
            job_id,
            conditions,
        )?
        .ok_or_else(repair)?;
        ledger.validate()?;
        let job: ProceduralFeedbackJobV2 =
            read(store, PROCEDURAL_FEEDBACK_JOB_NAMESPACE, job_id, conditions)?
                .ok_or_else(repair)?;
        job.validate()?;
        if job.status != ProceduralFeedbackJobStatusV1::Succeeded
            || ledger.identity != job.feedback_source()?.identity
            || ledger.identity.memory_space_id != self.config.memory_space_id
            || ledger.identity.mounted_subject_id != self.config.scoped_runtime.mounted_subject_id
        {
            return Err(repair());
        }
        let key = ConversationKey::new(
            &ledger.identity.memory_space_id,
            &ledger.identity.channel_id,
            &ledger.identity.conversation_id,
        )?;
        let physical_key = crate::store_internal::transcript_turn_storage_key(
            &key,
            &ledger.identity.mounted_subject_id,
            &ledger.identity.turn_id,
        );
        let record: bm_core::memory::TranscriptTurnRecord =
            read(store, "conversation_transcript", &physical_key, conditions)?
                .ok_or_else(repair)?;
        dependencies.push(Dependency::Transcript {
            identity: ledger.identity.clone(),
            sequence: record.sequence,
            content_digest: bm_core::memory::procedural_transcript_state_digest(&record)?,
            lifecycle: record.lifecycle_state,
        });
        let mut source = Source {
            ledger,
            evidence: None,
            producer: None,
            permitted: false,
        };
        if record.permits_post_turn_learning() {
            let evidence = record.learning_evidence.ok_or_else(repair)?;
            if !evidence.validate_contract()
                || evidence.learning_evidence_digest != source.ledger.learning_evidence_digest
            {
                return Err(repair());
            }
            let reference = evidence.authority.producer_revision().ok_or_else(repair)?;
            let producer = owner::read_verified_procedural_producer(store, reference)?;
            producer
                .historical
                .authorize_evidence_with_current(
                    &producer.historical,
                    &source.ledger.identity.producer_scope(),
                    &evidence,
                )
                .map_err(|_| repair())?;
            conditions.push(StoreJsonPrecondition::Exact {
                namespace: PROCEDURAL_PRODUCER_HEAD_NAMESPACE.into(),
                key: producer.head.binding_key.clone(),
                value: serde_json::to_value(&producer.head).map_err(|_| repair())?,
            });
            source.permitted = self.reconciliation_producer_permitted(&producer, conditions)?;
            if let bm_core::memory::ProceduralProducerSourceAuthorityV1::GovernedSource {
                source_revision,
            } = &producer.historical.spec.source_authority
            {
                let (root, expected) = owner::source_dependents::read_verified(
                    store,
                    &self.config.memory_space_id,
                    &source_revision.owner_ref,
                    crate::store_internal::StoreCapacityBudget::from_runtime_budget(
                        self.runtime_budget().store_budget,
                    ),
                )?;
                let root = root.ok_or_else(repair)?;
                merge_json_preconditions(conditions, expected)?;
                dependencies.push(Dependency::Source {
                    owner_ref: root.owner_ref,
                    post_image: root.current,
                    owner_state_digest: root.content_digest,
                });
            }
            dependencies.push(Dependency::Producer {
                original_revision: reference.clone(),
                current_revision: producer.head.current.clone(),
                permitted: source.permitted,
            });
            source.evidence = Some(evidence);
            source.producer = Some(producer);
        }
        sources.insert(job_id.into(), source);
        Ok(())
    }

    fn reconciliation_producer_permitted(
        &self,
        producer: &owner::VerifiedProceduralProducer,
        conditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<bool> {
        let mut permitted = producer.current.state == ProceduralProducerStateV1::Active;
        let governance = self.validate_procedural_current_governance(
            &producer.historical,
            &producer.current,
            crate::ProceduralLearningSdkOperation::ApplyFeedback,
        );
        let source = self.resolve_procedural_source_authority(
            &producer.historical.spec.source_authority,
            crate::ProceduralLearningSdkOperation::ApplyFeedback,
        );
        for result in [governance.map(|_| Vec::new()), source] {
            match result {
                Ok(expected) => merge_json_preconditions(conditions, expected)?,
                Err(Error::Other { source, .. })
                    if source
                        .downcast_ref::<crate::ProceduralLearningSdkError>()
                        .is_some_and(|error| {
                            error.disposition
                                == crate::ProceduralLearningSdkErrorDisposition::AuthorityRejected
                        }) =>
                {
                    permitted = false
                }
                Err(error) => return Err(error),
            }
        }
        Ok(permitted)
    }

    /// Feedback and reconciliation select retained sources through the same
    /// current authority boundary. Historical material is not a read grant.
    pub(super) fn authorized_retained_tool_body(
        &self,
        material: &AgentToolExperienceRevisionMaterialV3,
        conditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<Option<Body>> {
        self.reconcile_tool_material(
            material,
            &material.body,
            &mut BTreeMap::new(),
            &mut Vec::new(),
            conditions,
        )
        .map(|reduced| reduced.map(|(body, _)| body))
    }

    fn reconcile_tool_material(
        &self,
        material: &AgentToolExperienceRevisionMaterialV3,
        applied_body: &Body,
        sources: &mut BTreeMap<String, Source>,
        dependencies: &mut Vec<Dependency>,
        conditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<Option<(Body, AgentToolExperienceStatus)>> {
        let mut body = applied_body.clone();
        let references = match &body {
            Body::Execution { contributions, .. } => contributions.clone(),
            Body::Method { sources, .. } => sources
                .iter()
                .flat_map(|source| source.execution_refs.clone())
                .collect(),
        };
        // Registry fingerprints belong to immutable producer evidence, not to
        // the experience's asset identity. Never synthesize one from the owner.
        let mut tool = None;
        if let Some(reference) = references.first() {
            self.reconciliation_source(
                &reference.source_job_id,
                sources,
                dependencies,
                conditions,
            )?;
            tool = sources
                .get(&reference.source_job_id)
                .and_then(|source| {
                    source
                        .ledger
                        .execution_contributions
                        .iter()
                        .find(|fact| fact.reference().ok().as_ref() == Some(reference))
                })
                .map(|fact| fact.tool.clone());
        } else if let Body::Method {
            sources: method_sources,
            ..
        } = &body
        {
            for method in method_sources {
                self.reconciliation_source(
                    &method.source_job_id,
                    sources,
                    dependencies,
                    conditions,
                )?;
                tool = sources
                    .get(&method.source_job_id)
                    .and_then(|source| source.evidence.as_ref())
                    .and_then(|evidence| {
                        evidence.agent_tool_feedback.iter().find(|feedback| {
                            feedback.registry_ref.registry_id == material.registry_id
                                && feedback.registry_ref.scope == material.registry_scope
                                && feedback.tool_id == material.tool_id
                                && feedback.schema_fingerprint == material.schema_fingerprint
                        })
                    })
                    .map(|feedback| ProceduralProducerToolV1 {
                        registry_ref: feedback.registry_ref.clone(),
                        tool_id: feedback.tool_id.clone(),
                        schema_fingerprint: feedback.schema_fingerprint.clone(),
                    });
                if tool.is_some() {
                    break;
                }
            }
        } else {
            return Ok(Some((body, material.status)));
        }
        let Some(tool) = tool else {
            return Ok(None);
        };
        if tool.registry_ref.registry_id != material.registry_id
            || tool.registry_ref.scope != material.registry_scope
            || tool.tool_id != material.tool_id
            || tool.schema_fingerprint != material.schema_fingerprint
        {
            return Err(repair());
        }
        let mut valid_facts = BTreeMap::<String, ToolExecutionContributionV1>::new();
        for reference in references {
            self.reconciliation_source(
                &reference.source_job_id,
                sources,
                dependencies,
                conditions,
            )?;
            let source = sources.get(&reference.source_job_id).ok_or_else(repair)?;
            let fact = source
                .ledger
                .execution_contributions
                .iter()
                .find(|fact| fact.reference().ok().as_ref() == Some(&reference))
                .ok_or_else(repair)?;
            if fact.tool != tool {
                return Err(repair());
            }
            if source.permitted
                && source.producer.as_ref().is_some_and(|producer| {
                    let feedback = bm_core::memory::AgentToolUsageFeedbackV3 {
                        registry_ref: tool.registry_ref.clone(),
                        tool_id: tool.tool_id.clone(),
                        schema_fingerprint: tool.schema_fingerprint.clone(),
                        execution_facts: vec![fact.fact.clone()],
                        method_evidence: Vec::new(),
                    };
                    [&producer.historical, &producer.current]
                        .into_iter()
                        .all(|binding| {
                            binding
                                .authorize_tool_feedback(
                                    &producer.historical.spec.principal,
                                    &source.ledger.identity.producer_scope(),
                                    &feedback,
                                )
                                .is_ok()
                        })
                })
            {
                valid_facts.insert(reference.contribution_id, fact.clone());
            }
        }
        match &mut body {
            Body::Execution { contributions, .. } => contributions
                .retain(|reference| valid_facts.contains_key(&reference.contribution_id)),
            Body::Method {
                sources: method_sources,
                ..
            } => {
                let mut retained = Vec::new();
                for mut method_source in method_sources.clone() {
                    self.reconciliation_source(
                        &method_source.source_job_id,
                        sources,
                        dependencies,
                        conditions,
                    )?;
                    let source = sources
                        .get(&method_source.source_job_id)
                        .ok_or_else(repair)?;
                    let declaration = source
                        .evidence
                        .as_ref()
                        .and_then(|evidence| {
                            evidence.agent_tool_feedback.iter().find(|feedback| {
                                feedback.registry_ref == tool.registry_ref
                                    && feedback.tool_id == tool.tool_id
                                    && feedback.schema_fingerprint == tool.schema_fingerprint
                            })
                        })
                        .and_then(|feedback| {
                            feedback
                                .method_evidence
                                .iter()
                                .find(|method| method.method_id == method_source.method_id)
                        });
                    let allowed = source.permitted
                        && declaration.is_some_and(|method| {
                            source.producer.as_ref().is_some_and(|producer| {
                                let feedback = bm_core::memory::AgentToolUsageFeedbackV3 {
                                    registry_ref: tool.registry_ref.clone(),
                                    tool_id: tool.tool_id.clone(),
                                    schema_fingerprint: tool.schema_fingerprint.clone(),
                                    execution_facts: Vec::new(),
                                    method_evidence: vec![method.clone()],
                                };
                                [&producer.historical, &producer.current].into_iter().all(
                                    |binding| {
                                        binding
                                            .authorize_tool_feedback(
                                                &producer.historical.spec.principal,
                                                &source.ledger.identity.producer_scope(),
                                                &feedback,
                                            )
                                            .is_ok()
                                    },
                                )
                            })
                        });
                    if allowed {
                        method_source.execution_refs.retain(|reference| {
                            valid_facts.contains_key(&reference.contribution_id)
                        });
                        retained.push(method_source);
                    }
                }
                *method_sources = retained;
                if method_sources.is_empty() {
                    return Ok(None);
                }
            }
        }
        let contributions = valid_facts.into_values().collect::<Vec<_>>();
        bm_core::memory::reduce_agent_tool_experience_contributions(
            &bm_core::memory::ProceduralSubjectScopeV1 {
                memory_space_id: material.memory_space_id.clone(),
                mounted_subject_id: material.owning_scope.mounted_subject_id().into(),
            },
            &tool,
            &body,
            &contributions,
            Some(material.status),
            self.runtime_budget()
                .governed_state_budget
                .max_agent_tool_experience_evidence_refs_per_owner,
        )
        .map(Some)
        .map_err(|_| repair())
    }

    fn refresh_reconciliation_dependencies(
        &self,
        dependencies: &[Dependency],
        conditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        let store = self.config.store_platform.as_ref().ok_or_else(repair)?;
        for dependency in dependencies {
            match dependency {
                Dependency::Producer {
                    original_revision,
                    current_revision,
                    permitted,
                } => {
                    let verified =
                        owner::read_verified_procedural_producer(store, original_revision)?;
                    if verified.head.current != *current_revision {
                        return Err(Error::conflict(
                            "procedural_reconciliation",
                            "producer pin changed",
                        ));
                    }
                    conditions.push(StoreJsonPrecondition::Exact {
                        namespace: PROCEDURAL_PRODUCER_HEAD_NAMESPACE.into(),
                        key: current_revision.binding_key.clone(),
                        value: serde_json::to_value(&verified.head).map_err(|_| repair())?,
                    });
                    if self.reconciliation_producer_permitted(&verified, conditions)? != *permitted
                    {
                        return Err(Error::conflict(
                            "procedural_reconciliation",
                            "original source authority changed",
                        ));
                    }
                }
                Dependency::Transcript {
                    identity,
                    sequence,
                    content_digest,
                    lifecycle,
                } => {
                    let key = ConversationKey::new(
                        &identity.memory_space_id,
                        &identity.channel_id,
                        &identity.conversation_id,
                    )?;
                    let physical = crate::store_internal::transcript_turn_storage_key(
                        &key,
                        &identity.mounted_subject_id,
                        &identity.turn_id,
                    );
                    let record: bm_core::memory::TranscriptTurnRecord =
                        read(store, "conversation_transcript", &physical, conditions)?
                            .ok_or_else(repair)?;
                    if record.sequence != *sequence
                        || record.lifecycle_state != *lifecycle
                        || bm_core::memory::procedural_transcript_state_digest(&record)?
                            != *content_digest
                    {
                        return Err(Error::conflict(
                            "procedural_reconciliation",
                            "transcript pin changed",
                        ));
                    }
                }
                Dependency::Source {
                    owner_ref,
                    post_image,
                    owner_state_digest,
                } => {
                    let (root, expected) = owner::source_dependents::read_verified(
                        store,
                        &self.config.memory_space_id,
                        owner_ref,
                        crate::store_internal::StoreCapacityBudget::from_runtime_budget(
                            self.runtime_budget().store_budget,
                        ),
                    )?;
                    let root = root.ok_or_else(repair)?;
                    merge_json_preconditions(conditions, expected)?;
                    if root.current != *post_image || root.content_digest != *owner_state_digest {
                        return Err(Error::conflict(
                            "procedural_reconciliation",
                            "source pin changed",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
