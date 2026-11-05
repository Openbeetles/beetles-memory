use super::*;

mod extraction;
mod reconciliation;

/// Constructed only after validating a sealed caller capability against the
/// current Store-owned producer head. Never deserialized from a request body.
pub(super) struct ProceduralSubmissionContext {
    pub(super) binding: bm_core::memory::ProceduralProducerBindingV1,
    pub(super) current: bm_core::memory::ProceduralProducerBindingV1,
    pub(super) head: bm_core::memory::ProceduralProducerHeadV1,
    pub(super) principal: bm_core::memory::ProceduralProducerPrincipalV1,
    pub(super) source_preconditions: Vec<StoreJsonPrecondition>,
}

/// One staged revision per experience owner, even when several input groups
/// contribute to it. This is an in-transaction plan, never another state store.
struct ProceduralExperienceUpdate {
    tool: bm_core::memory::ProceduralProducerToolV1,
    body: bm_core::skills::AgentToolExperienceBodyV1,
    method_inputs: Vec<(u32, u32)>,
}

fn feedback_error(key: crate::ProceduralLearningErrorKeyV1) -> Error {
    procedural_error(crate::ProceduralLearningSdkOperation::FinalizeTurn, key)
}

fn procedural_error(
    operation: crate::ProceduralLearningSdkOperation,
    key: crate::ProceduralLearningErrorKeyV1,
) -> Error {
    use crate::ProceduralLearningErrorKeyV1 as Key;
    use crate::ProceduralLearningSdkErrorDisposition as Disposition;
    let disposition = match key {
        Key::RepairRequired => Disposition::RepairRequired,
        Key::ProducerAuthorityDenied
        | Key::ProducerAuthorityRequired
        | Key::ConfirmationAuthorityInvalid => Disposition::AuthorityRejected,
        Key::EvidenceConflict => Disposition::ExpectedStateConflict,
        Key::BudgetExceeded => Disposition::CapacityRejected,
        Key::StoreUnavailable => Disposition::StoreCommitRejected,
        _ => Disposition::ContractRejected,
    };
    Error::Other {
        stage: "post_turn_learning_evidence",
        source: Box::new(crate::ProceduralLearningSdkError {
            operation,
            key,
            disposition,
        }),
    }
}

impl MemoryRuntime {
    /// Reused by the sealed intake proof: all mutable roots checked while
    /// resolving a bounded source remain exact transaction preconditions.
    pub(super) fn resolve_production_learning_input_sources(
        &self,
        request: &MemoryTurnFinalizeRequest,
    ) -> Result<Vec<StoreJsonPrecondition>> {
        self.resolve_production_learning_inputs(request)
            .map(|(_, conditions)| conditions)
    }

    fn resolve_production_learning_inputs(
        &self,
        request: &MemoryTurnFinalizeRequest,
    ) -> Result<(
        Vec<bm_core::memory::AdmittedMethodLearningInputV1>,
        Vec<StoreJsonPrecondition>,
    )> {
        use crate::ProceduralLearningErrorKeyV1 as Key;
        use bm_core::memory::{
            MethodLearningMaterialSourceV1 as Source,
            ProceduralProducerSourceAuthorityV1 as Authority, ProceduralSourceSensitivity,
        };
        if !request.learning.validate_production_contract() {
            return Err(feedback_error(Key::EvidenceConflict));
        }
        let mut admitted = Vec::new();
        let mut conditions = Vec::new();
        for group in &request.learning.method_learning_inputs {
            self.validate_production_tool(
                &group.registry_ref,
                &group.tool_id,
                &group.schema_fingerprint,
            )?;
            let mut resolved = group.clone();
            for material in &mut resolved.materials {
                material.external_content |= request.turn.external_content_used;
                // Rejected content never causes a private/raw source fetch.
                if material.sensitivity != ProceduralSourceSensitivity::NonPrivate
                    || material.external_content
                {
                    continue;
                }
                match &material.source {
                    Source::CanonicalInputMessage { input_index } => {
                        let source = request
                            .turn
                            .input_messages
                            .get(*input_index as usize)
                            .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
                        if !source.content.contains(&material.text) {
                            return Err(feedback_error(Key::EvidenceConflict));
                        }
                        Self::tighten_method_material_authority(material, source.authority);
                    }
                    Source::CanonicalAssistantMessage { assistant_index } => {
                        if request.turn.delivery_status
                            != bm_core::memory::MemoryTurnDeliveryStatus::Delivered
                        {
                            return Err(feedback_error(Key::EvidenceConflict));
                        }
                        let source = request
                            .turn
                            .assistant_messages
                            .get(*assistant_index as usize)
                            .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
                        if source.settlement
                            != bm_core::memory::AssistantMessageSettlement::Complete
                            || !source.message.content.contains(&material.text)
                        {
                            return Err(feedback_error(Key::EvidenceConflict));
                        }
                        Self::tighten_method_material_authority(material, source.message.authority);
                    }
                    Source::ToolObservation { observation_id } => {
                        let observation = request
                            .turn
                            .tool_observations
                            .iter()
                            .find(|observation| &observation.observation_id == observation_id)
                            .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
                        if observation.tool_name != group.tool_id
                            || !observation.summary.contains(&material.text)
                        {
                            return Err(feedback_error(Key::EvidenceConflict));
                        }
                        material.external_content |= observation.external_content;
                    }
                    Source::GovernedRevision {
                        revision,
                        content_digest,
                    } => {
                        let source_conditions = self.resolve_procedural_source_authority(
                            &Authority::GovernedSource {
                                source_revision: revision.clone(),
                            },
                            crate::ProceduralLearningSdkOperation::FinalizeTurn,
                        )?;
                        let mut exact_fragment = false;
                        for condition in &source_conditions {
                            let StoreJsonPrecondition::Exact {
                                namespace, value, ..
                            } = condition
                            else {
                                continue;
                            };
                            if namespace
                                == crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE
                            {
                                let source: bm_core::memory::LongTermMemoryVersionMaterial =
                                    serde_json::from_value(value.clone())
                                        .map_err(|_| feedback_error(Key::RepairRequired))?;
                                if source.owner_revision_ref() == *revision {
                                    exact_fragment = source.content_digest == *content_digest
                                        && source.governed_content.content.contains(&material.text);
                                }
                            } else if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
                                let source: GovernedEvidenceDocument =
                                    serde_json::from_value(value.clone())
                                        .map_err(|_| feedback_error(Key::RepairRequired))?;
                                if source.owner_revision == revision.owner_revision {
                                    exact_fragment = source.content_digest == *content_digest
                                        && source.body.contains(&material.text);
                                }
                            }
                        }
                        if !exact_fragment {
                            return Err(feedback_error(Key::EvidenceConflict));
                        }
                        merge_json_preconditions(&mut conditions, source_conditions)?;
                    }
                }
            }
            let mut result = bm_core::memory::AdmittedMethodLearningInputV1::admit(&resolved)?;
            result.submitted_classifications = group
                .materials
                .iter()
                .map(|item| item.sensitivity)
                .collect();
            result.submitted_classifications.sort();
            result.submitted_classifications.dedup();
            admitted.push(result);
        }
        for adoption in &request.learning.method_adoptions {
            self.resolve_production_method_reference(&adoption.method, &mut conditions)?;
            for reference in &adoption.observation_refs {
                if !request.turn.tool_observations.iter().any(|observation| {
                    &observation.observation_id == reference
                        && observation.tool_name == adoption.method.tool_id
                        && !observation.external_content
                }) {
                    return Err(feedback_error(Key::EvidenceConflict));
                }
            }
        }
        for evaluation in &request.learning.method_task_evaluations {
            self.resolve_production_method_reference(&evaluation.method, &mut conditions)?;
            if evaluation.observation_refs.iter().any(|reference| {
                !request.turn.tool_observations.iter().any(|observation| {
                    &observation.observation_id == reference && !observation.external_content
                })
            }) {
                return Err(feedback_error(Key::EvidenceConflict));
            }
        }
        Ok((admitted, conditions))
    }

    fn tighten_method_material_authority(
        material: &mut bm_core::memory::MethodLearningMaterialV1,
        authority: bm_core::memory::MemoryEvidenceAuthority,
    ) {
        use bm_core::memory::{
            MemoryEvidenceAuthority as Authority, ProceduralSourceSensitivity as Sensitivity,
        };
        match authority {
            Authority::PrivateGardenInternal => material.sensitivity = Sensitivity::Private,
            Authority::ExternalContent => material.external_content = true,
            Authority::SubjectProjection
            | Authority::SoulGovernance
            | Authority::OperatorDiagnostic
            | Authority::LegacyTranscript
            | Authority::AssistantSelfClaim => {
                material.sensitivity = material.sensitivity.ceiling(Sensitivity::Unknown)
            }
            _ => {}
        }
    }

    fn validate_production_tool(
        &self,
        registry: &bm_core::skills::AgentToolRegistryRef,
        tool_id: &str,
        schema: &str,
    ) -> Result<()> {
        if !self
            .procedural_applicability_for_conversation(
                self.config.scope.conversation_id_or_chat_id(),
            )?
            .permits_registry_scope(&registry.scope)
            || !self.agent_tool_registries().iter().any(|candidate| {
                candidate.registry_ref() == *registry
                    && candidate.tools.iter().any(|tool| {
                        !tool.disabled
                            && tool.tool_id == tool_id
                            && tool.schema_fingerprint == schema
                    })
            })
        {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
            ));
        }
        Ok(())
    }

    fn resolve_production_method_reference(
        &self,
        reference: &bm_core::memory::MethodContentRefV1,
        conditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        use crate::ProceduralLearningErrorKeyV1 as Key;
        if !reference.validate_contract()
            || reference.memory_space_id != self.config.memory_space_id
            || reference.mounted_subject_id != self.config.scoped_runtime.mounted_subject_id
        {
            return Err(feedback_error(Key::ScopeMismatch));
        }
        self.validate_production_tool(
            &reference.registry_ref,
            &reference.tool_id,
            &reference.schema_fingerprint,
        )?;
        let store = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        let scope = AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: reference.mounted_subject_id.clone(),
        };
        let key = agent_tool_experience_head_key(
            &reference.memory_space_id,
            &scope,
            &reference.owner_revision.owner_ref,
        )?;
        let head_doc = store
            .read_json_docs_by_keys(
                AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .pop()
            .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
        let head: AgentToolExperienceOwnerHeadV3 =
            serde_json::from_value(head_doc.value.clone())
                .map_err(|_| feedback_error(Key::RepairRequired))?;
        if !head.validate_contract().accepted
            || head.state == bm_core::skills::AgentToolExperienceHeadStateV3::Tombstoned
            || head.memory_space_id != reference.memory_space_id
            || head.owning_scope != scope
            || head.owner_ref != reference.owner_revision.owner_ref
        {
            return Err(feedback_error(Key::EvidenceConflict));
        }
        let exact = head
            .retained_revisions
            .iter()
            .find(|item| {
                item.owner_revision == reference.owner_revision.owner_revision
                    && item.content_digest == reference.content_digest
            })
            .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
        let material_doc = store
            .read_json_docs_by_keys(
                AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                std::slice::from_ref(&exact.material_key),
            )?
            .pop()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        let material: AgentToolExperienceRevisionMaterialV4 =
            serde_json::from_value(material_doc.value.clone())
                .map_err(|_| feedback_error(Key::RepairRequired))?;
        if !material.validate_contract().accepted
            || material.memory_space_id != reference.memory_space_id
            || material.owning_scope != scope
            || material.owner_ref != reference.owner_revision.owner_ref
            || material.owner_revision != reference.owner_revision.owner_revision
            || material.content_digest != reference.content_digest
            || material.registry_id != reference.registry_ref.registry_id
            || material.registry_scope != reference.registry_ref.scope
            || material.tool_id != reference.tool_id
            || material.schema_fingerprint != reference.schema_fingerprint
            || !material.privacy_class.projection_content_allowed()
            || !matches!(
                &material.body,
                bm_core::skills::AgentToolExperienceBodyV1::Method { .. }
            )
        {
            return Err(feedback_error(Key::EvidenceConflict));
        }
        let current_ref = head
            .retained_revisions
            .last()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        let current_doc = store
            .read_json_docs_by_keys(
                AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                std::slice::from_ref(&current_ref.material_key),
            )?
            .pop()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        let current: AgentToolExperienceRevisionMaterialV4 =
            serde_json::from_value(current_doc.value.clone())
                .map_err(|_| feedback_error(Key::RepairRequired))?;
        AgentToolExperienceHeadBindingV1::from_head_and_material(&head, &current)?;
        if !current.privacy_class.projection_content_allowed()
            || matches!(
                current.status,
                bm_core::skills::AgentToolExperienceStatus::Stale
                    | bm_core::skills::AgentToolExperienceStatus::Rejected
            )
            || matches!(
                material.status,
                bm_core::skills::AgentToolExperienceStatus::Stale
                    | bm_core::skills::AgentToolExperienceStatus::Rejected
            )
            || self
                .authorized_retained_tool_body(&current, conditions)?
                .is_none()
        {
            return Err(feedback_error(Key::ProducerAuthorityDenied));
        }
        // Current source revocation applies even to an exact retained revision.
        if self
            .authorized_retained_tool_body(&material, conditions)?
            .is_none()
        {
            return Err(feedback_error(Key::ProducerAuthorityDenied));
        }
        merge_json_preconditions(
            conditions,
            vec![
                StoreJsonPrecondition::Exact {
                    namespace: AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.into(),
                    key,
                    value: head_doc.value,
                },
                StoreJsonPrecondition::Exact {
                    namespace: AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.into(),
                    key: exact.material_key.clone(),
                    value: material_doc.value,
                },
                StoreJsonPrecondition::Exact {
                    namespace: AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.into(),
                    key: current_ref.material_key.clone(),
                    value: current_doc.value,
                },
            ],
        )?;
        Ok(())
    }

    fn validate_procedural_current_governance(
        &self,
        historical: &bm_core::memory::ProceduralProducerBindingV1,
        current: &bm_core::memory::ProceduralProducerBindingV1,
        operation: crate::ProceduralLearningSdkOperation,
    ) -> Result<()> {
        let reject = || {
            procedural_error(
                operation,
                crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
            )
        };
        let governor = self
            .active_governing_system_subject("procedural_current_authority")
            .map_err(|_| reject())?;
        if historical.operation_identity.actor_subject_id() != governor
            || current.operation_identity.actor_subject_id() != governor
        {
            return Err(reject());
        }
        Ok(())
    }

    /// Single source-authority resolver used by producer control, intake and
    /// application. A locator identifies the existing owner; it grants nothing.
    fn resolve_procedural_source_authority(
        &self,
        source: &bm_core::memory::ProceduralProducerSourceAuthorityV1,
        operation: crate::ProceduralLearningSdkOperation,
    ) -> Result<Vec<StoreJsonPrecondition>> {
        (|| {
        use bm_core::memory::ProceduralProducerSourceAuthorityV1 as Source;
        use crate::ProceduralLearningErrorKeyV1 as Key;
        if !source.validate_contract() { return Err(feedback_error(Key::ProducerAuthorityDenied)); }
        let source_revision = match source {
            Source::RuntimeObservation => return Ok(Vec::new()),
            Source::HumanUser { subject_id } => {
                if !self.config.subject_registry.subject(subject_id).is_some_and(|subject|
                    subject.kind == bm_core::memory::SubjectKind::HumanUser && subject.lifecycle_state == SubjectLifecycleState::Active) {
                    return Err(feedback_error(Key::ProducerAuthorityDenied));
                }
                return Ok(Vec::new());
            },
            Source::ModelInferred { subject_id } => {
                if subject_id != &self.config.scoped_runtime.mounted_subject_id
                    || !self.config.subject_registry.subject(subject_id).is_some_and(|subject|
                        subject.kind == bm_core::memory::SubjectKind::AgentPersona && subject.lifecycle_state == SubjectLifecycleState::Active) {
                    return Err(feedback_error(Key::ProducerAuthorityDenied));
                }
                return Ok(Vec::new());
            },
            Source::GovernedSource { source_revision } => source_revision,
        };
        if RuntimeBudgetLease::active_report(&self.config.runtime_budget_authority).is_none() {
            let lease = self.acquire_runtime_budget_lease()?;
            return self.execute_with_runtime_budget_lease(&lease, || self.resolve_procedural_source_authority(source, operation));
        }
        let store = self.config.store_platform.as_ref().ok_or_else(|| feedback_error(Key::RepairRequired))?;
        let budget = self.runtime_budget();
        let (source_root, source_conditions) = crate::store_internal::procedural_feedback::source_dependents::read_verified(
            store, &self.config.memory_space_id, &source_revision.owner_ref,
            crate::store_internal::StoreCapacityBudget::from_runtime_budget(budget.store_budget))?;
        if !source_root.is_some_and(|root| matches!(root.current, bm_core::memory::ProceduralSourcePostImageV1::Present { .. })) {
            return Err(feedback_error(Key::ProducerAuthorityDenied));
        }
        let mut conditions = match source_revision.owner_ref.owner_plane {
            GovernedMemoryOwnerPlane::LongTerm => {
                let retention = budget.governed_state_budget.max_retained_long_term_revisions_per_owner;
                let result = store.with_recall_immutable_read_session(&budget, |context| {
                    context.materialize_long_term_owner_closure(&self.config.memory_space_id, &self.config.memory_space_id,
                        &source_revision.owner_ref, retention, budget.governed_state_budget.max_validity_joins)?
                        .ok_or_else(|| feedback_error(Key::ProducerAuthorityDenied))?;
                    let view = context.take_materialized_view();
                    let authorities = view.current_long_term_projections(retention)?;
                    let current = authorities.get(&source_revision.owner_ref)
                        .ok_or_else(|| feedback_error(Key::RepairRequired))?.projection();
                    let retained = view.json_docs::<bm_core::memory::LongTermMemoryVersionMaterial>(
                        crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE)?;
                    let exact = retained.iter().find(|material| material.owner_revision_ref() == *source_revision)
                        .ok_or_else(|| feedback_error(Key::ProducerAuthorityDenied))?;
                    let query_time = self.config.clock.now_secs().max(current.validity.valid_from);
                    let gates = bm_core::memory::GovernedRecallAuthorityGates {
                        disclosure: bm_core::memory::GovernedRecallDisclosure::Allowed,
                        required_premise: bm_core::memory::GovernedRequiredPremiseGate::NotApplicable,
                        profile_budget_drop: bm_core::memory::GovernedProfileBudgetDrop::None,
                    };
                    let current_facts = bm_core::memory::project_current_long_term_recall_lifecycle_facts(
                        authorities.get(&source_revision.owner_ref).ok_or_else(|| feedback_error(Key::RepairRequired))?)?;
                    let current_decision = bm_core::memory::decide_governed_recall_eligibility(&current_facts,
                        bm_core::memory::GovernedRecallTemporalQuery::Current { query_time }, gates.clone())?;
                    if current_decision.eligibility == bm_core::memory::GovernedRecallEligibility::Excluded {
                        return Err(feedback_error(Key::ProducerAuthorityDenied));
                    }
                    if exact.owner_revision != current.material.owner_revision {
                        let roots = view.json_docs::<bm_core::memory::LongTermMemoryVersionScopeManifest>(
                            crate::store_internal::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE)?;
                        let root = roots.iter().find(|root| root.memory_space_id == self.config.memory_space_id
                            && root.factual_owner_id == self.config.memory_space_id).ok_or_else(|| feedback_error(Key::RepairRequired))?;
                        let heads = view.json_docs::<bm_core::memory::LongTermMemoryHeadManifest>(crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE)?;
                        let controls = view.json_docs::<bm_core::memory::LongTermMemoryControlRevision>(bm_core::memory::LONG_TERM_CONTROL_REVISION_NAMESPACE)?;
                        let historical = bm_core::memory::build_long_term_historical_recall_authority(root, &heads, &retained, &controls,
                            &source_revision.owner_ref, exact.origin.valid_from, retention, budget.governed_state_budget.max_lineage_depth)?
                            .ok_or_else(|| feedback_error(Key::ProducerAuthorityDenied))?;
                        if historical.projection().material.owner_revision_ref() != *source_revision {
                            return Err(feedback_error(Key::ProducerAuthorityDenied));
                        }
                        let facts = bm_core::memory::project_historical_long_term_recall_lifecycle_facts(&historical)?;
                        let decision = bm_core::memory::decide_governed_recall_eligibility(&facts,
                            bm_core::memory::GovernedRecallTemporalQuery::HistoricalAsOf { query_time, as_of_time: exact.origin.valid_from }, gates)?;
                        if decision.eligibility == bm_core::memory::GovernedRecallEligibility::Excluded {
                            return Err(feedback_error(Key::ProducerAuthorityDenied));
                        }
                    }
                    for material in [exact, &current.material] {
                        if !Source::permits_governed_method_source(material.governed_content.provenance.source_authority, material.privacy_class, false)
                            || self.subject_visibility_decision_for_subject(&material.subject_visibility,
                                &self.config.scoped_runtime.mounted_subject_id)? != MemorySubjectVisibilityDecision::Allowed {
                            return Err(feedback_error(Key::ProducerAuthorityDenied));
                        }
                    }
                    Ok(view.json_preconditions())
                })?;
                Ok(result.output)
            },
            GovernedMemoryOwnerPlane::EvidenceDocument => {
                // The first known-key lookup only locates the real source
                // subject. The exact owner/claim/manifest snapshot below is
                // authoritative and every returned root is included in CAS.
                let located = self.read_evidence_document_from_platform(store, &source_revision.owner_ref.owner_id)?
                    .ok_or_else(|| feedback_error(Key::ProducerAuthorityDenied))?;
                let exact = store.read_governed_evidence_exact(&budget, StoreGovernedEvidenceExactReadRequest {
                    memory_space_id: self.config.memory_space_id.clone(), mounted_subject_id: located.mounted_subject_id.clone(),
                    owner_keys: vec![located.physical_key.clone()], include_all_manifest_bindings: false,
                    allow_missing_manifest_for_empty_scope: false,
                })?;
                let manifest = exact.manifest.ok_or_else(|| feedback_error(Key::RepairRequired))?;
                let read = exact.reads.into_iter().find(|read| read.owner_key == located.physical_key)
                    .ok_or_else(|| feedback_error(Key::RepairRequired))?;
                let owner_value = read.owner.ok_or_else(|| feedback_error(Key::ProducerAuthorityDenied))?;
                let claim_value = read.claim.ok_or_else(|| feedback_error(Key::RepairRequired))?;
                let document: GovernedEvidenceDocument = serde_json::from_value(owner_value.clone())
                    .map_err(|_| feedback_error(Key::RepairRequired))?;
                let claim: GovernedEvidenceSourceRef = serde_json::from_value(claim_value.clone())
                    .map_err(|_| feedback_error(Key::RepairRequired))?;
                validate_governed_evidence_source_ref(&document, &claim).map_err(|_| feedback_error(Key::RepairRequired))?;
                if document.owner_revision != source_revision.owner_revision || claim.owner_ref != source_revision.owner_ref
                    || manifest.mounted_subject_id != document.mounted_subject_id
                    || !self.config.subject_registry.subject(&document.mounted_subject_id)
                        .is_some_and(|subject| subject.lifecycle_state == SubjectLifecycleState::Active)
                    || !self.evidence_document_visible_to_runtime(&document)
                    || !Source::permits_governed_method_source(document.authority, document.privacy,
                        document.source_kind == bm_core::memory::GovernedEvidenceDocumentSourceKind::ExternalContent) {
                    return Err(feedback_error(Key::ProducerAuthorityDenied));
                }
                Ok(vec![
                    StoreJsonPrecondition::Exact { namespace: crate::store_internal::GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.into(),
                        key: manifest.physical_key.clone(), value: serde_json::to_value(manifest).map_err(|_| feedback_error(Key::RepairRequired))? },
                    StoreJsonPrecondition::Exact { namespace: GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.into(), key: document.physical_key, value: owner_value },
                    StoreJsonPrecondition::Exact { namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.into(), key: claim.physical_key, value: claim_value },
                ])
            },
            _ => Err(feedback_error(Key::ProducerAuthorityDenied)),
        }?;
        merge_json_preconditions(&mut conditions, source_conditions)?;
        Ok(conditions)
        })().map_err(|error| crate::ProceduralLearningSdkError::from_owner_error(operation, &error).into_core_error())
    }

    /// Trusted composition-root handoff. The immutable grant does not itself
    /// authorize reporting; only the governing runtime can mint this capability.
    pub fn procedural_submission_capability(
        &self,
        reference: &bm_core::memory::ProceduralProducerRevisionRefV1,
    ) -> Result<crate::MemoryProceduralSubmissionCapability> {
        (|| {
            let reject = |key| {
                procedural_error(
                    crate::ProceduralLearningSdkOperation::IssueSubmissionCapability,
                    key,
                )
            };
            self.ensure_visible("write.procedural_submission", self.capabilities.write)
                .map_err(|_| reject(crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable))?;
            self.exact_system_governor_actor("procedural_submission_capability")
                .map_err(|_| {
                    reject(crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied)
                })?;
            if !reference.validate_contract() {
                return Err(reject(
                    crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
                ));
            }
            let store = self
                .config
                .store_platform
                .as_ref()
                .ok_or_else(|| reject(crate::ProceduralLearningErrorKeyV1::RepairRequired))?;
            let verified =
                crate::store_internal::procedural_feedback::read_verified_procedural_producer(
                    store, reference,
                )?;
            let capability = crate::MemoryProceduralSubmissionCapability {
                identity: self.governance_attachment_identity()?,
                binding: verified.historical,
                store_incarnation: store
                    .procedural_store_incarnation(&self.config.memory_space_id)?,
            };
            if verified.current.state != bm_core::memory::ProceduralProducerStateV1::Active
                || capability.binding.spec.scope != self.procedural_producer_scope()
                || !self.procedural_subject_active()
            {
                return Err(reject(
                    crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
                ));
            }
            Ok(capability)
        })()
        .map_err(|error| {
            crate::ProceduralLearningSdkError::from_owner_error(
                crate::ProceduralLearningSdkOperation::IssueSubmissionCapability,
                &error,
            )
            .into_core_error()
        })
    }

    pub fn finalize_turn_with_procedural_evidence(
        &self,
        capability: &crate::MemoryProceduralSubmissionCapability,
        request: MemoryTurnFinalizeRequest,
    ) -> Result<MemoryTurnFinalizeReport> {
        (|| {
            self.ensure_visible("write.turn", self.capabilities.write)
                .map_err(|_| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable)
                })?;
            validate_turn_scope(
                &self.config.scope,
                &self.config.scoped_runtime,
                &request.turn,
            )
            .map_err(|_| feedback_error(crate::ProceduralLearningErrorKeyV1::ScopeMismatch))?;
            let verified = self.verified_procedural_submission_capability(capability)?;
            if let Some(report) = self.exact_procedural_finalization_replay(&request, &verified)? {
                return Ok(report);
            }
            let context = self.procedural_submission_context_from_verified(&verified)?;
            self.finalize_turn_internal(None, None, request, Some(&context))
        })()
        .map_err(|error| {
            crate::ProceduralLearningSdkError::from_owner_error(
                crate::ProceduralLearningSdkOperation::FinalizeTurn,
                &error,
            )
            .into_core_error()
        })
    }

    pub(super) fn procedural_producer_scope(&self) -> bm_core::memory::ProceduralProducerScopeV1 {
        bm_core::memory::ProceduralProducerScopeV1 {
            memory_space_id: self.config.memory_space_id.clone(),
            mounted_subject_id: self.config.scoped_runtime.mounted_subject_id.clone(),
            channel_id: self.config.scope.channel.clone(),
            chat_id: self.config.scope.chat_id.clone(),
        }
    }

    #[cfg(all(test, feature = "sqlite-store"))]
    fn procedural_submission_context(
        &self,
        capability: &crate::MemoryProceduralSubmissionCapability,
    ) -> Result<ProceduralSubmissionContext> {
        let verified = self.verified_procedural_submission_capability(capability)?;
        self.procedural_submission_context_from_verified(&verified)
    }

    fn verified_procedural_submission_capability(
        &self,
        capability: &crate::MemoryProceduralSubmissionCapability,
    ) -> Result<crate::store_internal::procedural_feedback::VerifiedProceduralProducer> {
        use crate::ProceduralLearningErrorKeyV1 as Key;
        if capability.identity != self.governance_attachment_identity()?
            || !self.procedural_subject_active()
        {
            return Err(feedback_error(Key::ScopeMismatch));
        }
        let store = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        if capability.store_incarnation
            != store.procedural_store_incarnation(&self.config.memory_space_id)?
        {
            return Err(feedback_error(Key::ProducerAuthorityDenied));
        }
        let verified =
            crate::store_internal::procedural_feedback::read_verified_procedural_producer(
                store,
                &capability.binding.revision_ref()?,
            )?;
        self.validate_procedural_current_governance(
            &verified.historical,
            &verified.current,
            crate::ProceduralLearningSdkOperation::FinalizeTurn,
        )?;
        if verified.historical != capability.binding
            || verified.head.scope != self.procedural_producer_scope()
        {
            return Err(feedback_error(Key::ProducerAuthorityDenied));
        }
        let registry = &self.config.subject_registry;
        let actor_id = &self.config.scoped_runtime.actor_subject_id;
        if !registry
            .subject(actor_id)
            .is_some_and(|subject| subject.lifecycle_state == SubjectLifecycleState::Active)
        {
            return Err(feedback_error(Key::ProducerAuthorityDenied));
        }
        use bm_core::memory::ProceduralProducerSourceAuthorityV1 as Source;
        if let Source::HumanUser { subject_id } = &verified.historical.spec.source_authority {
            if subject_id != actor_id {
                return Err(feedback_error(Key::ProducerAuthorityDenied));
            }
        }
        Ok(verified)
    }

    fn procedural_submission_context_from_verified(
        &self,
        verified: &crate::store_internal::procedural_feedback::VerifiedProceduralProducer,
    ) -> Result<ProceduralSubmissionContext> {
        if verified.current.state != bm_core::memory::ProceduralProducerStateV1::Active {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
            ));
        }
        let source_preconditions = self.resolve_procedural_source_authority(
            &verified.historical.spec.source_authority,
            crate::ProceduralLearningSdkOperation::FinalizeTurn,
        )?;
        Ok(ProceduralSubmissionContext {
            principal: verified.historical.spec.principal.clone(),
            binding: verified.historical.clone(),
            current: verified.current.clone(),
            head: verified.head.clone(),
            source_preconditions,
        })
    }

    /// A committed turn acknowledges historical intake. Its receipt must not
    /// reacquire a method, producer, or source that changed after that intake.
    fn exact_procedural_finalization_replay(
        &self,
        request: &MemoryTurnFinalizeRequest,
        producer: &crate::store_internal::procedural_feedback::VerifiedProceduralProducer,
    ) -> Result<Option<MemoryTurnFinalizeReport>> {
        use crate::ProceduralLearningErrorKeyV1 as Key;
        if !request.learning.has_authorized_content() {
            return Ok(None);
        }
        let transcript_store = self.config.platform.conversation_transcript_store();
        let key = ConversationKey::from_delta(&self.config.memory_space_id, &request.turn)?;
        let Some(record) =
            transcript_store.get_turn(&key, &self.config.subject_id, &request.turn.turn_id)?
        else {
            return Ok(None);
        };
        if !record.permits_post_turn_learning() {
            return Err(feedback_error(Key::EvidenceConflict));
        }
        record.validate_canonical_intake()?;
        let evidence = record
            .learning_evidence
            .as_ref()
            .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
        let submitted = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?
            .post_turn_learning_input_commitment(
                &self.config.memory_space_id,
                &request.turn,
                &request.learning,
            )?;
        if record.canonical_turn_digest != canonical_turn_learning_digest(&request.turn)?
            || !record.host_refs.is_empty()
            || evidence.submitted_input_commitment.as_deref() != Some(submitted.as_str())
            || evidence.authority.producer_revision() != Some(&producer.historical.revision_ref()?)
        {
            return Err(feedback_error(Key::EvidenceConflict));
        }

        let semantic_identity = PostTurnGovernanceIdentityV2::new(
            &record.key.memory_space_id,
            &record.subject,
            &record.key.channel_id,
            &request.turn.conversation.chat_id,
            &record.key.conversation_id,
            &record.turn_id,
        )?;
        let store = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        let mut semantic_job = read_governance_job(store, &semantic_identity.job_id())?;
        let transcript_digest = post_turn_governance_transcript_digest(&record)?;
        if semantic_job.as_ref().is_some_and(|job| {
            job.identity != semantic_identity
                || job.transcript_sequence != record.sequence
                || job.transcript_digest != transcript_digest
        }) {
            return Err(feedback_error(Key::RepairRequired));
        }
        let procedural_identity = ProceduralFeedbackIdentityV1::new(
            &record.key.memory_space_id,
            &record.subject,
            &record.key.channel_id,
            &request.turn.conversation.chat_id,
            &record.key.conversation_id,
            &record.turn_id,
        )?;
        let procedural_id = evidence
            .has_procedural_work()
            .then(|| procedural_identity.job_id(&evidence.learning_evidence_digest))
            .transpose()?;
        let mut procedural_job = procedural_id
            .as_ref()
            .map(|id| crate::store_internal::procedural_feedback::read_job(store, id))
            .transpose()?
            .flatten();
        if let Some(job) = procedural_job.as_ref() {
            let source = job.feedback_source()?;
            if source.identity != procedural_identity
                || source.transcript_sequence != record.sequence
                || source.transcript_digest != transcript_digest
                || source.learning_evidence_digest != evidence.learning_evidence_digest
            {
                return Err(feedback_error(Key::RepairRequired));
            }
        }
        let plan = bm_core::memory::plan_canonical_turn_delta_with_transcript(
            self.config.platform.session_store().as_ref(),
            transcript_store.as_ref(),
            &self.config.memory_space_id,
            &request.turn,
            CanonicalTurnTranscriptCommitOptions {
                host_refs: Vec::new(),
                learning_evidence: Some(evidence.clone()),
                conversation_alias: self.conversation_alias_from_delta(&request.turn)?,
                now_secs: self.config.clock.now_secs(),
            },
        )?;
        let committed = plan.commit_with(|_| Err(feedback_error(Key::RepairRequired)))?;
        if committed.session_commit.committed
            || committed
                .transcript_commit
                .as_ref()
                .is_none_or(|report| report.committed)
        {
            return Err(feedback_error(Key::RepairRequired));
        }
        let mut semantic_created = false;
        let mut procedural_created = false;
        let recovering_semantic = semantic_job.is_none();
        let recovering_procedural = procedural_id.is_some() && procedural_job.is_none();
        if recovering_semantic || recovering_procedural {
            let mut fences = Vec::new();
            let current = self.procedural_submission_context_from_verified(producer)?;
            merge_json_preconditions(&mut fences, current.source_preconditions)?;
            merge_json_preconditions(
                &mut fences,
                self.resolve_production_learning_input_sources(request)?,
            )?;
            merge_json_preconditions(
                &mut fences,
                vec![StoreJsonPrecondition::Exact {
                    namespace: crate::store_internal::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE
                        .into(),
                    key: current.head.binding_key.clone(),
                    value: serde_json::to_value(&current.head)
                        .map_err(|_| feedback_error(Key::RepairRequired))?,
                }],
            )?;
            let now = self.config.clock.now_secs();
            let expected_semantic = self.build_governance_job_from_transcript(
                &record,
                &request.turn.conversation.chat_id,
                request.learning.tool_call_count,
                "conversation_transcript_turn_already_committed",
                now,
            )?;
            let expected_procedural = if recovering_procedural {
                Some(
                    self.build_procedural_job_from_transcript(
                        &record,
                        &request.turn.conversation.chat_id,
                        now,
                    )?
                    .ok_or_else(|| feedback_error(Key::RepairRequired))?,
                )
            } else {
                procedural_job.clone()
            };
            let scope = self
                .memory_write_transaction_scope()
                .with_conversation(key.conversation_id.clone());
            let (semantic_outcome, procedural_outcome) =
                crate::store_internal::procedural_feedback::ensure_post_turn_learning_intents(
                    store,
                    scope,
                    &self.runtime_budget(),
                    &expected_semantic,
                    expected_procedural.as_ref(),
                    now,
                    &fences,
                )?;
            semantic_created = semantic_outcome == GovernanceIntentEnsureOutcome::Created;
            procedural_created = procedural_outcome == Some(ProceduralIntentEnsureOutcome::Created);
            semantic_job = read_governance_job(store, &semantic_identity.job_id())?;
            procedural_job = procedural_id
                .as_ref()
                .map(|id| crate::store_internal::procedural_feedback::read_job(store, id))
                .transpose()?
                .flatten();
            if semantic_created || procedural_created {
                self.notify_learning_wake_sinks();
            }
        }
        let semantic_job = semantic_job.ok_or_else(|| feedback_error(Key::RepairRequired))?;
        if procedural_id.is_some() && procedural_job.is_none() {
            return Err(feedback_error(Key::RepairRequired));
        }
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::PostReply,
            self.mode_input_for_request(request.mode_input, request.pressure),
        );
        let lifecycle_report = lifecycle.finish_success(
            self.config.clock.now_secs(),
            false,
            "conversation_transcript_turn_already_committed",
        );
        Ok(Some(MemoryTurnFinalizeReport {
            session_commit: committed.session_commit,
            transcript_commit: committed.transcript_commit,
            maintenance: None,
            private_garden_self_work: PostTurnPrivateGardenReport::skipped("turn_not_committed"),
            semantic_governance: PostTurnSemanticGovernanceReport::skipped("turn_not_committed"),
            memory_consolidation: MemoryConsolidationReport {
                state: MemoryConsolidationState::Queued,
                job_id: Some(semantic_job.job_id),
                reason: if semantic_created {
                    "governance_intent_created"
                } else {
                    "governance_intent_already_present"
                }
                .into(),
            },
            procedural_learning: if let Some(job) = procedural_job {
                MemoryProceduralLearningIntentReport {
                    state: MemoryConsolidationState::Queued,
                    job_id: Some(job.job_id),
                    reason: if procedural_created {
                        "procedural_intent_created"
                    } else {
                        "procedural_intent_already_present"
                    }
                    .into(),
                }
            } else {
                MemoryProceduralLearningIntentReport {
                    state: MemoryConsolidationState::NotScheduled,
                    job_id: None,
                    reason: "no_actionable_procedural_feedback".into(),
                }
            },
            lifecycle_report,
        }))
    }

    pub fn control_procedural_producer(
        &self,
        request: crate::MemoryProceduralProducerControlRequest,
    ) -> Result<crate::MemoryProceduralProducerControlReport> {
        (|| {
            let reject =
                |key| procedural_error(crate::ProceduralLearningSdkOperation::ProducerControl, key);
            self.ensure_visible("write.procedural_producer", self.capabilities.write)
                .map_err(|_| reject(crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable))?;
            let governor = self
                .exact_system_governor_actor("procedural_producer_control")
                .map_err(|_| {
                    reject(crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied)
                })?;
            let scope = bm_core::memory::ProceduralProducerScopeV1 {
                memory_space_id: self.config.memory_space_id.clone(),
                mounted_subject_id: self.config.scoped_runtime.mounted_subject_id.clone(),
                channel_id: self.config.scope.channel.clone(),
                chat_id: self.config.scope.chat_id.clone(),
            };
            let withdrawing = request.state == bm_core::memory::ProceduralProducerStateV1::Revoked;
            if !request.spec.validate_contract()
                || request.spec.scope != scope
                || (!withdrawing && !self.procedural_subject_active())
            {
                return Err(reject(crate::ProceduralLearningErrorKeyV1::ScopeMismatch));
            }
            let source_preconditions = if withdrawing {
                Vec::new()
            } else {
                self.resolve_procedural_source_authority(
                    &request.spec.source_authority,
                    crate::ProceduralLearningSdkOperation::ProducerControl,
                )?
            };
            if withdrawing {
                let reference = request
                    .expected_revision
                    .as_ref()
                    .ok_or_else(|| reject(crate::ProceduralLearningErrorKeyV1::EvidenceConflict))?;
                let store =
                    self.config.store_platform.as_ref().ok_or_else(|| {
                        reject(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                let existing =
                    crate::store_internal::procedural_feedback::read_verified_procedural_producer(
                        store, reference,
                    )?;
                if existing.historical.spec != request.spec {
                    return Err(reject(
                        crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                    ));
                }
            } else {
                let registries = self.agent_tool_registries();
                for tool in &request.spec.tools {
                    if !registries.iter().any(|registry| {
                        registry.registry_ref() == tool.registry_ref
                            && registry.tools.iter().any(|descriptor| {
                                descriptor.tool_id == tool.tool_id
                                    && descriptor.schema_fingerprint == tool.schema_fingerprint
                                    && !descriptor.disabled
                            })
                    }) {
                        return Err(reject(
                            crate::ProceduralLearningErrorKeyV1::ProducerAuthorityDenied,
                        ));
                    }
                }
            }
            let identity = bm_core::memory::MemoryMutationOperationIdentity::new(
                request.operation_id,
                &scope.memory_space_id,
                &scope.mounted_subject_id,
                governor,
                bm_core::memory::MemoryMutationOperationKind::ProceduralProducerControl,
            )?;
            let store = self
                .config
                .store_platform
                .as_ref()
                .ok_or_else(|| reject(crate::ProceduralLearningErrorKeyV1::RepairRequired))?;
            let (binding, receipt) =
                crate::store_internal::procedural_feedback::control_procedural_producer(
                    store,
                    self.memory_write_transaction_scope(),
                    &self.runtime_budget(),
                    identity,
                    request.spec,
                    request.state,
                    request.expected_revision,
                    self.config.clock.now_secs(),
                    &source_preconditions,
                )?;
            Ok(crate::MemoryProceduralProducerControlReport { binding, receipt })
        })()
        .map_err(|error| {
            crate::ProceduralLearningSdkError::from_owner_error(
                crate::ProceduralLearningSdkOperation::ProducerControl,
                &error,
            )
            .into_core_error()
        })
    }
}

#[cfg(all(test, feature = "sqlite-store"))]
#[path = "procedural/tests/pfi2_intake_authority_tests.rs"]
mod pfi2_intake_authority_tests;

#[cfg(all(test, feature = "nonproduction-replay-harness"))]
#[path = "procedural/tests/promotion_atomic_tests.rs"]
mod promotion_atomic_tests;

impl MemoryRuntime {
    pub(super) fn build_post_turn_learning_evidence(
        &self,
        request: &MemoryTurnFinalizeRequest,
        producer: Option<&ProceduralSubmissionContext>,
    ) -> Result<PostTurnLearningEvidenceV3> {
        let learning = &request.learning;
        let has_feedback = learning.has_signed_turn_context() || learning.has_authorized_content();
        if (has_feedback
            && !self
                .capabilities
                .procedural_learning
                .finalize_feedback
                .visible)
            || (!learning.agent_tool_feedback.is_empty()
                && !self
                    .capabilities
                    .procedural_learning
                    .agent_tool_learning
                    .visible)
        {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable,
            ));
        }
        let conversation_id = request
            .turn
            .conversation
            .conversation_id
            .as_deref()
            .unwrap_or(request.turn.conversation.chat_id.as_str())
            .to_string();
        use crate::ProceduralLearningErrorKeyV1 as Key;
        let input = &request.learning;
        if !input.validate_production_contract() {
            return Err(feedback_error(Key::EvidenceConflict));
        }
        if request.turn.subject != self.config.scoped_runtime.mounted_subject_id
            || request.turn.conversation.channel != self.config.scope.channel
            || request.turn.conversation.chat_id != self.config.scope.chat_id
            || self
                .config
                .scope
                .conversation_id
                .as_ref()
                .is_some_and(|pinned| &conversation_id != pinned)
            || !self.procedural_subject_active()
        {
            return Err(feedback_error(Key::ScopeMismatch));
        }
        let mut agent_tool_feedback = Vec::new();
        let authority = if input.has_authorized_content() {
            let producer =
                producer.ok_or_else(|| feedback_error(Key::ProducerAuthorityRequired))?;
            let binding = &producer.binding;
            let scope = bm_core::memory::ProceduralProducerScopeV1 {
                memory_space_id: self.config.memory_space_id.clone(),
                mounted_subject_id: self.config.scoped_runtime.mounted_subject_id.clone(),
                channel_id: self.config.scope.channel.clone(),
                chat_id: self.config.scope.chat_id.clone(),
            };
            if !binding.validate_contract()
                || binding.state != bm_core::memory::ProceduralProducerStateV1::Active
                || binding.spec.scope != scope
                || binding.spec.principal != producer.principal
                || ((!input.runtime_skill_feedback.is_empty()
                    || !input.agent_skill_feedback.is_empty()
                    || !input.task_learning_feedback.is_empty())
                    && !binding.spec.claims.usage_feedback)
            {
                return Err(feedback_error(Key::ProducerAuthorityDenied));
            }
            binding
                .authorize_production_input(input)
                .map_err(|_| feedback_error(Key::ProducerAuthorityDenied))?;
            producer
                .current
                .authorize_production_input(input)
                .map_err(|_| feedback_error(Key::ProducerAuthorityDenied))?;
            for feedback in &input.agent_tool_feedback {
                binding
                    .authorize_tool_feedback(&producer.principal, &scope, feedback)
                    .map_err(|_| feedback_error(Key::ProducerAuthorityDenied))?;
                producer
                    .current
                    .authorize_tool_feedback(&producer.principal, &scope, feedback)
                    .map_err(|_| feedback_error(Key::ProducerAuthorityDenied))?;
            }
            agent_tool_feedback = bm_core::memory::admit_agent_tool_feedback_for_turn(
                &input.agent_tool_feedback,
                &request.turn.tool_observations,
                request.turn.external_content_used,
                self.config.clock.now_secs(),
            )
            .map_err(|_| feedback_error(Key::EvidenceConflict))?;
            let confirmation = if let Some(operation_id) = &input.human_confirmation_operation_id {
                let actor_id = &self.config.scoped_runtime.actor_subject_id;
                let actor = self
                    .config
                    .subject_registry
                    .subject(actor_id)
                    .ok_or_else(|| feedback_error(Key::ConfirmationAuthorityInvalid))?;
                if actor.kind != bm_core::memory::SubjectKind::HumanUser
                    || actor.lifecycle_state != bm_core::memory::SubjectLifecycleState::Active
                    || operation_id.is_empty()
                    || operation_id.trim() != operation_id
                    || binding.spec.source_authority
                        != (bm_core::memory::ProceduralProducerSourceAuthorityV1::HumanUser {
                            subject_id: actor_id.clone(),
                        })
                {
                    return Err(feedback_error(Key::ConfirmationAuthorityInvalid));
                }
                Some(bm_core::memory::ProceduralHumanConfirmationV1 {
                    actor_subject_id: actor_id.clone(),
                    operation_id: operation_id.clone(),
                    evidence_digest: String::new(),
                })
            } else {
                None
            };
            ProceduralFeedbackAuthorityV2::Producer {
                producer_revision: binding.revision_ref()?,
                source_authority: binding.spec.source_authority.clone(),
                confirmation,
            }
        } else {
            if input.human_confirmation_operation_id.is_some() {
                return Err(feedback_error(Key::ConfirmationAuthorityInvalid));
            }
            ProceduralFeedbackAuthorityV2::Empty
        };
        let (method_learning_inputs, _) = self.resolve_production_learning_inputs(request)?;
        let submitted_input_commitment = if input.has_authorized_content() {
            Some(
                self.config
                    .store_platform
                    .as_ref()
                    .ok_or_else(|| feedback_error(Key::RepairRequired))?
                    .post_turn_learning_input_commitment(
                        &self.config.memory_space_id,
                        &request.turn,
                        input,
                    )?,
            )
        } else {
            None
        };
        let mut evidence = PostTurnLearningEvidenceV3 {
            schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
            memory_space_id: self.config.memory_space_id.clone(),
            mounted_subject_id: self.config.scoped_runtime.mounted_subject_id.clone(),
            conversation_id,
            turn_id: request.turn.turn_id.clone(),
            canonical_turn_digest: canonical_turn_learning_digest(&request.turn)?,
            tool_call_count: input.tool_call_count,
            selection_receipt: input.selection_receipt.clone(),
            runtime_skill_feedback: input.runtime_skill_feedback.clone(),
            agent_skill_feedback: input.agent_skill_feedback.clone(),
            task_learning_feedback: input.task_learning_feedback.clone(),
            agent_tool_feedback,
            method_learning_inputs,
            method_adoptions: input.method_adoptions.clone(),
            method_task_evaluations: input.method_task_evaluations.clone(),
            submitted_input_commitment,
            authority,
            learning_evidence_digest: String::new(),
        };
        let confirmation_digest = evidence.canonical_confirmation_payload_digest()?;
        if let ProceduralFeedbackAuthorityV2::Producer {
            confirmation: Some(confirmation),
            ..
        } = &mut evidence.authority
        {
            confirmation.evidence_digest = confirmation_digest;
        }
        evidence.learning_evidence_digest = evidence.canonical_digest()?;
        if !evidence.validate_contract() {
            return Err(feedback_error(Key::ReceiptInvalid));
        }
        if let Some(producer) = producer.filter(|_| input.has_authorized_content()) {
            producer
                .binding
                .authorize_evidence_with_current(
                    &producer.current,
                    &self.procedural_producer_scope(),
                    &evidence,
                )
                .map_err(|_| feedback_error(Key::ProducerAuthorityDenied))?;
        }
        let store = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        if let Some(receipt) = &evidence.selection_receipt {
            store
                .verify_procedural_selection_receipt(receipt)
                .map_err(|_| feedback_error(Key::ReceiptInvalid))?;
            if receipt.identity.memory_space_id != evidence.memory_space_id
                || receipt.identity.mounted_subject_id != evidence.mounted_subject_id
                || receipt.identity.channel != request.turn.conversation.channel
                || receipt.identity.chat_id != request.turn.conversation.chat_id
                || receipt.identity.conversation_id != evidence.conversation_id
                || receipt.identity.turn_id != evidence.turn_id
                || receipt.applicability
                    != self.procedural_applicability_for_conversation(&evidence.conversation_id)?
            {
                return Err(feedback_error(Key::ScopeMismatch));
            }
        }
        let key = ConversationKey::from_delta(&evidence.memory_space_id, &request.turn)?;
        if let Some(existing) = self
            .config
            .platform
            .conversation_transcript_store()
            .get_turn(&key, &evidence.mounted_subject_id, &evidence.turn_id)?
        {
            if existing.learning_evidence.as_ref() != Some(&evidence) {
                return Err(feedback_error(Key::EvidenceConflict));
            }
            if !existing.permits_post_turn_learning() && evidence.selection_receipt.is_some() {
                return Err(feedback_error(Key::EvidenceConflict));
            }
            return Ok(evidence);
        }
        if let Some(receipt) = &evidence.selection_receipt {
            let now = self.config.clock.now_secs();
            if receipt.issued_at > now
                || now >= receipt.expires_at
                || receipt.expires_at.saturating_sub(receipt.issued_at) != 86_400
            {
                return Err(feedback_error(Key::ReceiptInvalid));
            }
        }
        self.validate_feedback_sources(&evidence)?;
        let mut observation_ids = BTreeSet::new();
        for feedback in &evidence.agent_tool_feedback {
            self.normalize_agent_tool_learning_feedback(feedback, &evidence.conversation_id)
                .map_err(|_| feedback_error(Key::EvidenceConflict))?;
            for observation in &feedback.execution_facts {
                if !observation_ids.insert(observation.observation_id.clone())
                    || !observation.validates_canonical_call(
                        &feedback.tool_id,
                        &request.turn.tool_observations,
                        self.config.clock.now_secs(),
                    )
                {
                    return Err(feedback_error(Key::EvidenceConflict));
                }
            }
        }
        if input.tool_call_count as usize != request.turn.tool_observations.len()
            || observation_ids.len() > input.tool_call_count as usize
        {
            return Err(feedback_error(Key::EvidenceConflict));
        }
        Ok(evidence)
    }

    fn normalize_agent_tool_learning_feedback(
        &self,
        feedback: &bm_core::memory::AdmittedAgentToolEvidenceV1,
        conversation_id: &str,
    ) -> Result<()> {
        if !feedback.validate_contract() {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
            ));
        }
        let registry = self
            .agent_tool_registries()
            .into_iter()
            .find(|registry| registry.registry_ref() == feedback.registry_ref)
            .ok_or_else(|| feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict))?;
        if !self
            .procedural_applicability_for_conversation(conversation_id)?
            .permits_registry_scope(&registry.scope)
            || !registry.tools.iter().any(|tool| {
                !tool.disabled
                    && tool.tool_id == feedback.tool_id
                    && tool.schema_fingerprint == feedback.schema_fingerprint
            })
        {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
            ));
        }
        Ok(())
    }

    fn validate_feedback_sources(&self, evidence: &PostTurnLearningEvidenceV3) -> Result<()> {
        use crate::ProceduralLearningErrorKeyV1 as Key;
        for feedback in &evidence.agent_tool_feedback {
            self.normalize_agent_tool_learning_feedback(feedback, &evidence.conversation_id)
                .map_err(|_| feedback_error(Key::EvidenceConflict))?;
        }
        let applicability =
            self.procedural_applicability_for_conversation(&evidence.conversation_id)?;
        let expected_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: evidence.mounted_subject_id.clone(),
        };
        for feedback in &evidence.runtime_skill_feedback {
            if feedback.locator.owning_scope() != &expected_scope
                && feedback.locator.owning_scope() != &RuntimeSkillOwningScope::SharedProgram
            {
                return Err(feedback_error(Key::ScopeMismatch));
            }
            let snapshot = self.read_runtime_skill_locator_snapshot(
                &feedback.locator,
                "procedural_runtime_feedback",
            )?;
            let record = runtime_skill_record_for_locator(
                &snapshot,
                &feedback.locator,
                "procedural_runtime_feedback",
            )
            .map_err(|_| feedback_error(Key::EvidenceConflict))?;
            if record.content_digest != feedback.selected_content_digest
                || record.lifecycle.availability
                    != bm_core::skills::RuntimeSkillAvailability::Enabled
                || matches!(
                    record.lifecycle.state,
                    bm_core::skills::RuntimeSkillLifecycleState::Retired
                        | bm_core::skills::RuntimeSkillLifecycleState::Superseded
                )
            {
                return Err(feedback_error(Key::EvidenceConflict));
            }
        }
        for feedback in &evidence.agent_skill_feedback {
            let matches = self
                .config
                .agent_skill_registry
                .packages
                .iter()
                .filter(|package| {
                    package.status == bm_core::skills::AgentSkillPackageStatus::Active
                        && package.id == feedback.package_id
                        && package.fingerprint == feedback.package_fingerprint
                        && package.package_binding() == feedback.package_binding
                        && applicability.permits_agent_skill_scope(&package.scope)
                })
                .count();
            if matches != 1 {
                return Err(feedback_error(Key::EvidenceConflict));
            }
        }
        for feedback in &evidence.task_learning_feedback {
            let record = self
                .config
                .platform
                .task_learning_store()
                .get(&feedback.learning_id)?
                .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
            if !record
                .permits_usage_feedback(&self.config.scope.channel, &self.config.scope.chat_id)
                || record.canonical_content_digest()? != feedback.learning_digest
            {
                return Err(feedback_error(Key::EvidenceConflict));
            }
        }
        let store = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| feedback_error(Key::RepairRequired))?;
        if let Some(receipt) = &evidence.selection_receipt {
            let scope = AgentToolExperienceOwningScopeV1::Subject {
                mounted_subject_id: evidence.mounted_subject_id.clone(),
            };
            for selected in &receipt.agent_tool_experiences {
                if !evidence.agent_tool_feedback.iter().any(|feedback| {
                    feedback.registry_ref == selected.registry_ref
                        && feedback.tool_id == selected.tool_id
                        && feedback.schema_fingerprint == selected.schema_fingerprint
                }) {
                    continue;
                }
                let owner = GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::AgentToolExperience,
                    &selected.experience_owner_id,
                );
                let key =
                    agent_tool_experience_head_key(&evidence.memory_space_id, &scope, &owner)?;
                let head = store
                    .read_json_docs_by_keys(AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, &[key])?
                    .pop()
                    .ok_or_else(|| feedback_error(Key::EvidenceConflict))?;
                let head: AgentToolExperienceOwnerHeadV3 = serde_json::from_value(head.value)
                    .map_err(|_| feedback_error(Key::RepairRequired))?;
                if head.state == bm_core::skills::AgentToolExperienceHeadStateV3::Tombstoned
                    || head.current_revision != selected.experience_revision
                    || head.retained_revisions.last().is_none_or(|revision| {
                        revision.content_digest != selected.experience_content_digest
                    })
                {
                    return Err(feedback_error(Key::EvidenceConflict));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn run_claimed_procedural_feedback_job(
        &self,
        job: &ProceduralFeedbackJobV2,
        lease_owner: &str,
    ) -> Result<ProceduralFeedbackCompletionOutcome> {
        if !self.capabilities.procedural_learning.worker.visible {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable,
            ));
        }
        if !self.procedural_subject_active()
            || job.feedback_source()?.identity.memory_space_id != self.config.memory_space_id
            || job.feedback_source()?.identity.mounted_subject_id
                != self.config.scoped_runtime.mounted_subject_id
        {
            return Err(Error::conflict(
                "procedural_feedback_evidence",
                "learning authority does not match active subject",
            ));
        }
        let now_secs = self.config.clock.now_secs();
        let key = ConversationKey::new(
            &job.feedback_source()?.identity.memory_space_id,
            &job.feedback_source()?.identity.channel_id,
            &job.feedback_source()?.identity.conversation_id,
        )?;
        let record = self
            .config
            .platform
            .conversation_transcript_store()
            .get_turn(
                &key,
                &job.feedback_source()?.identity.mounted_subject_id,
                &job.feedback_source()?.identity.turn_id,
            )?
            .ok_or_else(|| {
                Error::config(
                    "procedural_feedback_evidence",
                    "canonical transcript root is unavailable",
                )
            })?;
        if !record.permits_post_turn_learning() {
            return Err(Error::conflict(
                "procedural_feedback_evidence",
                "canonical source no longer permits learning",
            ));
        }
        let evidence = record.learning_evidence.as_ref().ok_or_else(|| {
            Error::config(
                "procedural_feedback_evidence",
                "canonical transcript learning evidence is unavailable",
            )
        })?;
        if !evidence.agent_tool_feedback.is_empty()
            && !self
                .capabilities
                .procedural_learning
                .agent_tool_learning
                .visible
        {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::CapabilityUnavailable,
            ));
        }
        if record.sequence != job.feedback_source()?.transcript_sequence
            || post_turn_governance_transcript_digest(&record)?
                != job.feedback_source()?.transcript_digest
            || evidence.learning_evidence_digest != job.feedback_source()?.learning_evidence_digest
            || evidence.memory_space_id != job.feedback_source()?.identity.memory_space_id
            || evidence.mounted_subject_id != job.feedback_source()?.identity.mounted_subject_id
            || evidence.conversation_id != job.feedback_source()?.identity.conversation_id
            || evidence.turn_id != job.feedback_source()?.identity.turn_id
        {
            return Err(Error::config(
                "procedural_feedback_evidence",
                "procedural job and canonical transcript evidence diverged",
            ));
        }
        if !evidence.validate_contract() {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
            ));
        }
        let producer =
            crate::store_internal::procedural_feedback::read_authorized_procedural_evidence(
                self.config.store_platform.as_ref().ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?,
                evidence,
                &bm_core::memory::ProceduralProducerScopeV1 {
                    memory_space_id: job.feedback_source()?.identity.memory_space_id.clone(),
                    mounted_subject_id: job.feedback_source()?.identity.mounted_subject_id.clone(),
                    channel_id: job.feedback_source()?.identity.channel_id.clone(),
                    chat_id: job.feedback_source()?.identity.chat_id.clone(),
                },
            )
            .map_err(|error| {
                crate::ProceduralLearningSdkError::from_owner_error(
                    crate::ProceduralLearningSdkOperation::ApplyFeedback,
                    &error,
                )
                .into_core_error()
            })?;
        self.validate_procedural_current_governance(
            &producer.historical,
            &producer.current,
            crate::ProceduralLearningSdkOperation::ApplyFeedback,
        )?;
        let source_preconditions = self.resolve_procedural_source_authority(
            &producer.historical.spec.source_authority,
            crate::ProceduralLearningSdkOperation::ApplyFeedback,
        )?;
        if let Some(receipt) = &evidence.selection_receipt {
            self.config
                .store_platform
                .as_ref()
                .ok_or_else(|| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired))?
                .verify_procedural_selection_receipt(receipt)
                .map_err(|_| feedback_error(crate::ProceduralLearningErrorKeyV1::ReceiptInvalid))?;
        }
        let mut planned =
            match self.plan_agent_tool_feedback_application(job, &record, evidence, now_secs) {
                Ok(plan) => plan,
                Err(error) => {
                    let rejection = match &error {
                        Error::Other { source, .. } => source
                            .downcast_ref::<crate::ProceduralLearningSdkError>()
                            .filter(|typed| {
                                matches!(
                                    typed.key,
                                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict
                                        | crate::ProceduralLearningErrorKeyV1::ScopeMismatch
                                )
                            })
                            .map(|typed| typed.key),
                        _ => None,
                    };
                    let Some(key) = rejection else {
                        return Err(error);
                    };
                    ProceduralFeedbackApplicationPlan {
                        mutations: Vec::new(),
                        preconditions: Vec::new(),
                        owner_bindings: Vec::new(),
                        accepted_count: 0,
                        partially_accepted_count: 0,
                        method_dispositions: Vec::new(),
                        deferred_count: 0,
                        rejected_count: job.feedback_source()?.submitted_count,
                        changed_count: 0,
                        rejection_cause: Some(key),
                    }
                }
            };
        merge_json_preconditions(&mut planned.preconditions, source_preconditions)?;
        merge_json_preconditions(
            &mut planned.preconditions,
            vec![StoreJsonPrecondition::Exact {
                namespace: crate::store_internal::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE.into(),
                key: producer.head.binding_key.clone(),
                value: serde_json::to_value(&producer.head).map_err(|_| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?,
            }],
        )?;
        merge_json_preconditions(
            &mut planned.preconditions,
            vec![StoreJsonPrecondition::Exact {
                namespace: "conversation_transcript".to_string(),
                key: crate::store_internal::transcript_turn_storage_key(
                    &key,
                    &job.feedback_source()?.identity.mounted_subject_id,
                    &job.feedback_source()?.identity.turn_id,
                ),
                value: serde_json::to_value(&record).map_err(|_| {
                    Error::config(
                        "procedural_feedback_evidence",
                        "canonical transcript cannot be encoded",
                    )
                })?,
            }],
        )?;
        let store = self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config(
                "procedural_feedback_complete",
                "durable procedural completion requires StorePlatform",
            )
        })?;
        let method_preconditions =
            crate::store_internal::procedural_feedback::method_application_preconditions(
                store,
                job,
                &planned.mutations,
                &planned.owner_bindings,
                &planned.preconditions,
                &self.runtime_budget(),
                now_secs,
            )?;
        merge_json_preconditions(&mut planned.preconditions, method_preconditions)?;
        complete_procedural_feedback_job(
            store,
            self.memory_write_transaction_scope(),
            &self.runtime_budget(),
            ProceduralFeedbackCompletionInput {
                job_id: job.job_id.clone(),
                lease_owner: lease_owner.to_string(),
                lease_epoch: job.lease_epoch,
                operation_id: format!("procedural-feedback-{}", job.job_id),
                actor_subject_id: self.config.scoped_runtime.actor_subject_id.clone(),
                experience_mutations: planned.mutations,
                experience_preconditions: planned.preconditions,
                applied_owner_bindings: planned.owner_bindings,
                accepted_count: planned.accepted_count,
                partially_accepted_count: planned.partially_accepted_count,
                method_dispositions: planned.method_dispositions,
                deferred_count: planned.deferred_count,
                rejected_count: planned.rejected_count,
                changed_count: planned.changed_count,
                reason_digest: sha256_field_digest(&[
                    b"procedural_feedback_governance_v1",
                    &planned.accepted_count.to_be_bytes(),
                    &planned.partially_accepted_count.to_be_bytes(),
                    &planned.deferred_count.to_be_bytes(),
                    &planned.rejected_count.to_be_bytes(),
                    &serde_json::to_vec(&planned.rejection_cause).map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?,
                ]),
                completed_at: now_secs,
            },
        )
    }

    fn plan_agent_tool_feedback_application(
        &self,
        job: &ProceduralFeedbackJobV2,
        source_record: &bm_core::memory::TranscriptTurnRecord,
        evidence: &PostTurnLearningEvidenceV3,
        now_secs: u64,
    ) -> Result<ProceduralFeedbackApplicationPlan> {
        let runtime_budget = self.runtime_budget();
        let limits = &runtime_budget.governed_state_budget;
        let owning_scope = AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: job.feedback_source()?.identity.mounted_subject_id.clone(),
        };
        let store = self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config(
                "procedural_feedback_planning",
                "procedural feedback planning requires StorePlatform",
            )
        })?;
        let manifest_key = agent_tool_experience_scope_manifest_key(
            &job.feedback_source()?.identity.memory_space_id,
            &owning_scope,
        )?;
        let mut manifest = store
            .read_json_docs_by_keys(
                AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
                &[manifest_key],
            )?
            .pop()
            .map(|doc| {
                serde_json::from_value::<AgentToolExperienceScopeManifestV1>(doc.value).map_err(
                    |error| Error::config("procedural_feedback_planning", error.to_string()),
                )
            })
            .transpose()?;
        use bm_core::memory::{
            ProceduralEvidenceRejection, ProceduralFeedbackMethodDispositionV1,
            ProceduralMethodDispositionPhaseV1, ToolExecutionContributionV1, ToolExecutionCountsV1,
        };
        use bm_core::skills::{AgentToolExperienceBodyV1 as Body, AgentToolMethodSourceRefV2};
        let registries = self.agent_tool_registries();
        self.validate_feedback_sources(evidence)?;
        let current_contributions = evidence.execution_contributions_for_job(job)?;
        let mut contributions = current_contributions
            .iter()
            .map(|value| (value.contribution_id.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut source_preconditions = Vec::new();
        let mut adopted_materials = BTreeMap::new();
        let current_tasks =
            evidence.method_task_contributions_for_job(job, source_record, |reference| {
                self.validate_production_tool(
                    &reference.registry_ref,
                    &reference.tool_id,
                    &reference.schema_fingerprint,
                )?;
                let key = bm_core::skills::agent_tool_experience_material_key(
                    &reference.memory_space_id,
                    &owning_scope,
                    &reference.owner_revision.owner_ref,
                    reference.owner_revision.owner_revision,
                )?;
                let document = store
                    .read_json_docs_by_keys(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE, &[key])?
                    .pop()
                    .ok_or_else(|| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                let material: AgentToolExperienceRevisionMaterialV4 =
                    serde_json::from_value(document.value.clone()).map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                source_preconditions.push(StoreJsonPrecondition::Exact {
                    namespace: document.namespace,
                    key: document.key,
                    value: document.value,
                });
                adopted_materials.insert(material.owner_ref.owner_id.clone(), material.clone());
                Ok(material)
            })?;
        let mut task_contributions = current_tasks
            .iter()
            .map(|task| (task.contribution_id.clone(), task.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut updates = BTreeMap::<String, ProceduralExperienceUpdate>::new();
        let mut method_dispositions = Vec::new();
        let mut group_has_accepted_item = vec![false; evidence.agent_tool_feedback.len()];
        for (group_index, feedback) in evidence.agent_tool_feedback.iter().enumerate() {
            let registry = registries
                .iter()
                .find(|registry| registry.registry_ref() == feedback.registry_ref)
                .ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict)
                })?;
            if !self
                .procedural_applicability_for_conversation(
                    &job.feedback_source()?.identity.conversation_id,
                )?
                .permits_registry_scope(&registry.scope)
            {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::ScopeMismatch,
                ));
            }
            for rejected in &feedback.rejected_methods {
                method_dispositions.push(ProceduralFeedbackMethodDispositionV1 {
                    feedback_group_ordinal: group_index as u32,
                    method_ordinal: rejected.input_index,
                    phase: ProceduralMethodDispositionPhaseV1::Intake,
                    rejection: rejected.rejection,
                });
            }
            let facts = current_contributions
                .iter()
                .filter(|value| {
                    value.tool.registry_ref == feedback.registry_ref
                        && value.tool.tool_id == feedback.tool_id
                        && value.tool.schema_fingerprint == feedback.schema_fingerprint
                        && feedback.execution_facts.contains(&value.fact)
                })
                .collect::<Vec<_>>();
            let mut bodies = Vec::new();
            if !facts.is_empty() {
                let mut refs = facts
                    .iter()
                    .map(|value| {
                        value.reference().map_err(|_| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict)
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                refs.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
                bodies.push((
                    Body::Execution {
                        counts: ToolExecutionCountsV1::default(),
                        contributions: refs,
                    },
                    None,
                ));
                group_has_accepted_item[group_index] = true;
            }
            let admitted_ordinals = (0..feedback.submitted_method_count).filter(|index| {
                !feedback
                    .rejected_methods
                    .iter()
                    .any(|item| item.input_index == *index)
            });
            for (ordinal, method) in admitted_ordinals.zip(&feedback.method_evidence) {
                let mut refs = facts
                    .iter()
                    .filter(|value| method.execution_refs.contains(&value.fact.observation_id))
                    .map(|value| {
                        value.reference().map_err(|_| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict)
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                refs.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
                let mut body = Body::Method {
                    usage_contributions: Vec::new(),
                    task_signature: method.task_signature.clone(),
                    procedure: method.body.clone(),
                    constraints: Vec::new(),
                    sources: Vec::new(),
                };
                let method_digest = body.method_content_digest()?.ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?;
                if let Body::Method { sources, .. } = &mut body {
                    sources.push(AgentToolMethodSourceRefV2::ProducerDeclaration {
                        source_job_id: job.job_id.clone(),
                        method_id: method.method_id.clone(),
                        method_digest,
                        execution_refs: refs,
                    });
                }
                bodies.push((body, Some(ordinal)));
            }
            for (body, ordinal) in bodies {
                let owner_id = canonical_agent_tool_experience_owner_id(
                    &job.feedback_source()?.identity.memory_space_id,
                    &owning_scope,
                    &registry.registry_id,
                    &registry.scope,
                    &feedback.tool_id,
                    &feedback.schema_fingerprint,
                    &body.focus(),
                )?;
                let update =
                    updates
                        .entry(owner_id)
                        .or_insert_with(|| ProceduralExperienceUpdate {
                            tool: bm_core::memory::ProceduralProducerToolV1 {
                                registry_ref: feedback.registry_ref.clone(),
                                tool_id: feedback.tool_id.clone(),
                                schema_fingerprint: feedback.schema_fingerprint.clone(),
                            },
                            body: body.clone(),
                            method_inputs: Vec::new(),
                        });
                if let Some(ordinal) = ordinal {
                    update.method_inputs.push((group_index as u32, ordinal));
                }
                match (&mut update.body, body) {
                    (
                        Body::Execution {
                            contributions: existing,
                            ..
                        },
                        Body::Execution {
                            contributions: added,
                            ..
                        },
                    ) => {
                        existing.extend(added);
                        existing.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
                        existing.dedup();
                    }
                    (
                        Body::Method {
                            procedure, sources, ..
                        },
                        Body::Method {
                            procedure: added_procedure,
                            sources: added,
                            ..
                        },
                    ) => {
                        // Contradictory declarations inside a single input cannot
                        // choose a winning method by array order.
                        if *procedure != added_procedure {
                            return Err(feedback_error(
                                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                            ));
                        }
                        sources.extend(added);
                        AgentToolMethodSourceRefV2::canonicalize(sources)?;
                    }
                    _ => {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ))
                    }
                }
            }
        }
        for task in &current_tasks {
            let reference = &task.adoption.method;
            let material = adopted_materials
                .get(&reference.owner_revision.owner_ref.owner_id)
                .ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?;
            let update = updates
                .entry(material.owner_ref.owner_id.clone())
                .or_insert_with(|| {
                    let mut body = material.body.clone();
                    if let Body::Method {
                        usage_contributions,
                        ..
                    } = &mut body
                    {
                        usage_contributions.clear();
                    }
                    ProceduralExperienceUpdate {
                        tool: bm_core::memory::ProceduralProducerToolV1 {
                            registry_ref: reference.registry_ref.clone(),
                            tool_id: reference.tool_id.clone(),
                            schema_fingerprint: reference.schema_fingerprint.clone(),
                        },
                        body,
                        method_inputs: Vec::new(),
                    }
                });
            if update.body.method_content_digest()? != Some(task.method_content_digest.clone()) {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                ));
            }
            let Body::Method {
                usage_contributions,
                ..
            } = &mut update.body
            else {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                ));
            };
            usage_contributions.push(task.reference()?);
        }
        let mut plans = Vec::new();
        for (owner_id, mut update) in updates {
            let tool = &update.tool;
            let registry = registries
                .iter()
                .find(|registry| registry.registry_ref() == tool.registry_ref)
                .ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict)
                })?;
            let owner_ref = GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::AgentToolExperience,
                owner_id,
            );
            let head_key = agent_tool_experience_head_key(
                &job.feedback_source()?.identity.memory_space_id,
                &owning_scope,
                &owner_ref,
            )?;
            let previous_head = store
                .read_json_docs_by_keys(AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, &[head_key])?
                .pop()
                .map(|doc| {
                    serde_json::from_value::<AgentToolExperienceOwnerHeadV3>(doc.value).map_err(
                        |_| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired),
                    )
                })
                .transpose()?;
            if previous_head.as_ref().is_some_and(|head| {
                head.state == bm_core::skills::AgentToolExperienceHeadStateV3::Tombstoned
            }) {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                ));
            }
            let previous_material = previous_head
                .as_ref()
                .map(|head| {
                    let revision = head.retained_revisions.last().ok_or_else(|| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                    let doc = store
                        .read_json_docs_by_keys(
                            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                            std::slice::from_ref(&revision.material_key),
                        )?
                        .pop()
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    let material: AgentToolExperienceRevisionMaterialV4 =
                        serde_json::from_value(doc.value).map_err(|_| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    AgentToolExperienceHeadBindingV1::from_head_and_material(head, &material)?;
                    Ok::<_, Error>(material)
                })
                .transpose()?;
            if let Some(previous) = &previous_material {
                let authorized =
                    self.authorized_retained_tool_material(previous, &mut source_preconditions)?;
                let authorized_body = authorized.body.as_ref().map(|(body, _)| body);
                let method_content_conflict = update.body.method_content_digest()?
                    != previous.body.method_content_digest()?;
                match (&mut update.body, &previous.body) {
                    (Body::Execution { contributions, .. }, Body::Execution { .. }) => {
                        if let Some(Body::Execution {
                            contributions: retained,
                            ..
                        }) = authorized_body
                        {
                            contributions.extend(retained.clone());
                        }
                        contributions.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
                        contributions.dedup();
                    }
                    (
                        Body::Method {
                            sources,
                            usage_contributions,
                            ..
                        },
                        Body::Method { .. },
                    ) => {
                        if method_content_conflict {
                            for (group, ordinal) in &update.method_inputs {
                                method_dispositions.push(ProceduralFeedbackMethodDispositionV1 {
                                    feedback_group_ordinal: *group,
                                    method_ordinal: *ordinal,
                                    phase: ProceduralMethodDispositionPhaseV1::Application,
                                    rejection:
                                        ProceduralEvidenceRejection::MethodOwnerContentConflict,
                                });
                            }
                            let head = previous_head.as_ref().ok_or_else(|| {
                                feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                            })?;
                            source_preconditions.push(StoreJsonPrecondition::Exact {
                                namespace: AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.to_owned(),
                                key: head.physical_key.clone(),
                                value: serde_json::to_value(head).map_err(|_| {
                                    feedback_error(
                                        crate::ProceduralLearningErrorKeyV1::RepairRequired,
                                    )
                                })?,
                            });
                            continue;
                        }
                        if let Some(Body::Method {
                            sources: retained, ..
                        }) = authorized_body
                        {
                            sources.extend(retained.clone());
                        }
                        usage_contributions.extend(authorized.retained_tasks);
                        AgentToolMethodSourceRefV2::canonicalize(sources)?;
                        usage_contributions
                            .sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
                        usage_contributions.dedup();
                    }
                    _ => {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ))
                    }
                }
            }
            if update.body.contribution_count()
                > limits.max_agent_tool_experience_evidence_refs_per_owner
            {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::BudgetExceeded,
                ));
            }
            let mut task_execution_refs = Vec::new();
            let mut exact_tasks = Vec::new();
            for reference in update.body.method_usage_contributions() {
                if !task_contributions.contains_key(&reference.contribution_id) {
                    let document = store.read_json_docs_by_keys(
                        crate::store_internal::schema::PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                        std::slice::from_ref(&reference.source_job_id),
                    )?.pop().ok_or_else(|| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired))?;
                    let ledger: bm_core::memory::ProceduralFeedbackApplicationLedgerV3 =
                        serde_json::from_value(document.value.clone()).map_err(|_| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    ledger.validate()?;
                    if ledger.job_id != reference.source_job_id
                        || ledger.identity.memory_space_id
                            != job.feedback_source()?.identity.memory_space_id
                        || ledger.identity.mounted_subject_id
                            != job.feedback_source()?.identity.mounted_subject_id
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    for task in ledger.method_task_contributions {
                        if task_contributions
                            .insert(task.contribution_id.clone(), task.clone())
                            .is_some_and(|old| old != task)
                        {
                            return Err(feedback_error(
                                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                            ));
                        }
                    }
                    for fact in ledger.execution_contributions {
                        if contributions
                            .insert(fact.contribution_id.clone(), fact.clone())
                            .is_some_and(|old| old != fact)
                        {
                            return Err(feedback_error(
                                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                            ));
                        }
                    }
                    source_preconditions.push(StoreJsonPrecondition::Exact {
                        namespace: document.namespace,
                        key: document.key,
                        value: document.value,
                    });
                }
                let task = task_contributions
                    .get(&reference.contribution_id)
                    .ok_or_else(|| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                if task.reference()? != *reference {
                    return Err(feedback_error(
                        crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                    ));
                }
                task_execution_refs.extend(task.execution_refs.clone());
                exact_tasks.push(task.clone());
            }
            let mut refs = match &update.body {
                Body::Execution { contributions, .. } => contributions.iter().collect::<Vec<_>>(),
                Body::Method { sources, .. } => sources
                    .iter()
                    .flat_map(|source| source.execution_refs())
                    .collect(),
            };
            refs.extend(task_execution_refs.iter());
            // Follow only this owner's retained contribution roots. Unrelated
            // subjects/tools must not consume this operation's read budget.
            for reference in refs {
                if contributions.contains_key(&reference.contribution_id) {
                    continue;
                }
                let document = store.read_json_docs_by_keys(
                    crate::store_internal::schema::PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                    std::slice::from_ref(&reference.source_job_id))?.pop().ok_or_else(||
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired))?;
                let ledger: bm_core::memory::ProceduralFeedbackApplicationLedgerV3 =
                    serde_json::from_value(document.value.clone()).map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                ledger.validate()?;
                if ledger.job_id != reference.source_job_id
                    || ledger.identity.memory_space_id
                        != job.feedback_source()?.identity.memory_space_id
                    || ledger.identity.mounted_subject_id
                        != job.feedback_source()?.identity.mounted_subject_id
                {
                    return Err(feedback_error(
                        crate::ProceduralLearningErrorKeyV1::RepairRequired,
                    ));
                }
                for contribution in ledger.execution_contributions {
                    if let Some(previous) = contributions
                        .insert(contribution.contribution_id.clone(), contribution.clone())
                    {
                        if previous != contribution {
                            return Err(feedback_error(
                                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                            ));
                        }
                    }
                }
                source_preconditions.push(StoreJsonPrecondition::Exact {
                    namespace: document.namespace,
                    key: document.key,
                    value: document.value,
                });
            }
            let exact_fact = |reference: &bm_core::memory::ProceduralContributionRefV1| -> Result<&ToolExecutionContributionV1> {
                let fact = contributions.get(&reference.contribution_id).ok_or_else(||
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired))?;
                if fact.reference().ok().as_ref() != Some(reference)
                    || fact.source.memory_space_id != job.feedback_source()?.identity.memory_space_id
                    || fact.source.mounted_subject_id != job.feedback_source()?.identity.mounted_subject_id
                    || fact.tool.registry_ref != tool.registry_ref || fact.tool.tool_id != tool.tool_id
                    || fact.tool.schema_fingerprint != tool.schema_fingerprint {
                    return Err(feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired));
                }
                Ok(fact)
            };
            let mut references = match &update.body {
                Body::Execution { contributions, .. } => contributions.iter().collect::<Vec<_>>(),
                Body::Method { sources, .. } => sources
                    .iter()
                    .flat_map(|source| source.execution_refs())
                    .collect(),
            };
            references.extend(task_execution_refs.iter());
            let exact_contributions = references
                .into_iter()
                .map(|reference| exact_fact(reference).cloned())
                .collect::<Result<Vec<_>>>()?;
            let (body, status) = bm_core::memory::reduce_agent_tool_experience_contributions(
                &job.work.subject_scope(),
                &bm_core::memory::ProceduralProducerToolV1 {
                    registry_ref: tool.registry_ref.clone(),
                    tool_id: tool.tool_id.clone(),
                    schema_fingerprint: tool.schema_fingerprint.clone(),
                },
                &update.body,
                &exact_contributions,
                &exact_tasks,
                previous_material.as_ref().map(|material| material.status),
                limits.max_agent_tool_experience_evidence_refs_per_owner,
            )
            .map_err(|_| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired))?;
            update.body = body;
            for (group, _) in &update.method_inputs {
                group_has_accepted_item[*group as usize] = true;
            }
            let revision = previous_head
                .as_ref()
                .map(|head| head.current_revision.checked_add(1))
                .unwrap_or(Some(1))
                .ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?;
            if usize::try_from(revision).unwrap_or(usize::MAX)
                > limits.max_agent_tool_experience_revisions_per_owner
            {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::BudgetExceeded,
                ));
            }
            let predecessor = previous_material
                .as_ref()
                .map(AgentToolExperienceRetainedRevisionDigestV3::from_material)
                .transpose()?;
            let material = AgentToolExperienceRevisionMaterialV4::build(
                &job.feedback_source()?.identity.memory_space_id,
                owning_scope.clone(),
                &registry.registry_id,
                registry.scope.clone(),
                &tool.tool_id,
                &tool.schema_fingerprint,
                revision,
                update.body,
                status,
                MemoryPrivacyClass::SharedWithSubject,
                previous_material
                    .as_ref()
                    .map_or(now_secs, |material| material.created_at),
                now_secs,
                predecessor,
            )?;
            let mut retained = previous_head
                .as_ref()
                .map(|head| head.retained_revisions.clone())
                .unwrap_or_default();
            retained.push(AgentToolExperienceRetainedRevisionDigestV3::from_material(
                &material,
            )?);
            let head = AgentToolExperienceOwnerHeadV3::build(
                &job.feedback_source()?.identity.memory_space_id,
                owning_scope.clone(),
                material.owner_ref.clone(),
                revision,
                retained,
            )?;
            let binding =
                AgentToolExperienceHeadBindingV1::from_head_and_material(&head, &material)?;
            let mut bindings = manifest
                .as_ref()
                .map(|manifest| manifest.bindings.clone())
                .unwrap_or_default();
            bindings.retain(|candidate| candidate.owner_ref != head.owner_ref);
            bindings.push(binding);
            let next_manifest = AgentToolExperienceScopeManifestV1::build(
                manifest
                    .as_ref()
                    .map_or(1, |manifest| manifest.revision.saturating_add(1)),
                &job.feedback_source()?.identity.memory_space_id,
                owning_scope.clone(),
                bindings,
                limits.max_agent_tool_experience_owners_per_subject,
            )?;
            plans.push(AgentToolExperienceStoreMutationPlanV1::try_new(
                format!("procedural-feedback-{}", job.job_id),
                self.config.scoped_runtime.actor_subject_id.clone(),
                self.memory_write_transaction_scope(),
                previous_head,
                manifest.clone(),
                material,
                head,
                next_manifest.clone(),
                now_secs,
            )?);
            manifest = Some(next_manifest);
        }
        let (mut mutations, mut preconditions, owner_revisions) =
            combine_agent_tool_experience_plans(
                &plans,
                limits.max_agent_tool_experience_owners_per_subject,
            )?;
        preconditions.extend(source_preconditions);
        method_dispositions.sort_by_key(|item| (item.feedback_group_ordinal, item.method_ordinal));
        let mut accepted_count = u32::try_from(
            evidence
                .runtime_skill_feedback
                .len()
                .saturating_add(evidence.agent_skill_feedback.len())
                .saturating_add(evidence.task_learning_feedback.len())
                .saturating_add(evidence.method_adoptions.len())
                .saturating_add(evidence.method_task_evaluations.len()),
        )
        .map_err(|_| feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict))?;
        let mut partially_accepted_count = 0_u32;
        let deferred_count = 0_u32;
        let mut rejected_count = 0_u32;
        for (group, accepted) in group_has_accepted_item.iter().enumerate() {
            let rejected = method_dispositions
                .iter()
                .any(|item| item.feedback_group_ordinal == group as u32);
            match (*accepted, rejected) {
                (true, true) => partially_accepted_count += 1,
                (true, false) => accepted_count += 1,
                (false, _) => rejected_count += 1,
            }
        }
        if accepted_count
            .checked_add(partially_accepted_count)
            .and_then(|value| value.checked_add(rejected_count))
            != Some(job.feedback_source()?.submitted_count)
        {
            return Err(feedback_error(
                crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
            ));
        }
        let mut owner_bindings = owner_revisions
            .into_iter()
            .map(|revision| {
                let content_digest = mutations
                    .iter()
                    .find_map(|mutation| match mutation {
                        StoreMutation::PutJson {
                            namespace, value, ..
                        } if namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
                            serde_json::from_value::<AgentToolExperienceRevisionMaterialV4>(
                                value.clone(),
                            )
                            .ok()
                            .filter(|material| {
                                material.owner_ref == revision.owner_ref
                                    && material.owner_revision == revision.owner_revision
                            })
                            .map(|material| material.content_digest)
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                Ok(
                    bm_core::memory::ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                        owner_revision: revision,
                        content_digest,
                    },
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut runtime_records = self.plan_runtime_promotions_from_accepted_experiences(
            job,
            source_record,
            &mutations,
            &mut preconditions,
            now_secs,
        )?;
        for record in &runtime_records {
            owner_bindings.push(
                bm_core::memory::ProceduralAppliedOwnerBindingV1::RuntimeSkill {
                    binding: bm_core::skills::RuntimeSkillOwnerBinding::from_record(record)?,
                },
            );
        }
        let mut runtime_feedback = BTreeMap::<String, Vec<_>>::new();
        let new_usage =
            evidence.runtime_skill_contributions_for_job(job, source_record.created_at)?;
        for feedback in &evidence.runtime_skill_feedback {
            runtime_feedback
                .entry(feedback.locator.owner_id().to_owned())
                .or_default()
                .push(feedback);
        }
        for feedbacks in runtime_feedback.into_values() {
            let first = feedbacks[0];
            let snapshot = self.read_runtime_skill_locator_snapshot(
                &first.locator,
                "procedural_runtime_feedback",
            )?;
            let current = runtime_skill_record_for_locator(
                &snapshot,
                &first.locator,
                "procedural_runtime_feedback",
            )?;
            if feedbacks
                .iter()
                .any(|feedback| feedback.selected_content_digest != current.content_digest)
            {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                ));
            }
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_owned(),
                key: current.physical_key.clone(),
                value: serde_json::to_value(current).map_err(|_| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?,
            });
            if first.locator.owning_scope() == &RuntimeSkillOwningScope::SharedProgram {
                continue;
            }
            let mut usage = new_usage
                .iter()
                .filter(|contribution| {
                    contribution.locator.owner_id() == current.owner_ref.owner_id
                })
                .cloned()
                .collect::<Vec<_>>();
            for reference in &current.lifecycle.usage_outcome.contributions {
                let document = store.read_json_docs_by_keys(crate::store_internal::schema::PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                    std::slice::from_ref(&reference.source_job_id))?.pop()
                    .ok_or_else(|| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired))?;
                let ledger: bm_core::memory::ProceduralFeedbackApplicationLedgerV3 =
                    serde_json::from_value(document.value.clone()).map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                ledger.validate()?;
                let contribution = ledger
                    .runtime_skill_contributions
                    .iter()
                    .find(|contribution| contribution.reference().ok().as_ref() == Some(reference))
                    .ok_or_else(|| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                if contribution.locator.owner_id() != current.owner_ref.owner_id {
                    return Err(feedback_error(
                        crate::ProceduralLearningErrorKeyV1::RepairRequired,
                    ));
                }
                usage.push(contribution.clone());
                preconditions.push(StoreJsonPrecondition::Exact {
                    namespace: document.namespace,
                    key: document.key,
                    value: document.value,
                });
            }
            let next = current.apply_usage_contributions(
                &usage,
                limits.max_agent_tool_experience_evidence_refs_per_owner,
                now_secs,
            )?;
            if next != *current {
                owner_bindings.push(
                    bm_core::memory::ProceduralAppliedOwnerBindingV1::RuntimeSkill {
                        binding: bm_core::skills::RuntimeSkillOwnerBinding::from_record(&next)?,
                    },
                );
                runtime_records.push(next);
            }
        }
        if !runtime_records.is_empty() {
            let scope = RuntimeSkillOwningScope::Subject {
                mounted_subject_id: evidence.mounted_subject_id.clone(),
            };
            let plan = self.plan_runtime_skill_owner_records(&scope, runtime_records)?;
            if !plan.blob_preconditions.is_empty() {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::RepairRequired,
                ));
            }
            mutations.extend(plan.mutations);
            preconditions.extend(plan.preconditions);
        }
        for feedback in &evidence.task_learning_feedback {
            let current = self
                .config
                .platform
                .task_learning_store()
                .get(&feedback.learning_id)?
                .ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict)
                })?;
            if !current
                .permits_usage_feedback(&self.config.scope.channel, &self.config.scope.chat_id)
                || current.canonical_content_digest()? != feedback.learning_digest
            {
                return Err(feedback_error(
                    crate::ProceduralLearningErrorKeyV1::EvidenceConflict,
                ));
            }
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: "task_learning".to_owned(),
                key: current.learning_id.clone(),
                value: serde_json::to_value(&current).map_err(|_| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                })?,
            });
        }
        let changed_count = u32::try_from(owner_bindings.len()).map_err(|_| {
            Error::config(
                "procedural_feedback_planning",
                "changed owner count overflow",
            )
        })?;
        // Selection freshness and the typed owner planner can depend on the
        // same source. Preserve one exact CAS per address; divergent snapshots
        // are evidence conflicts, never last-writer-wins conditions.
        let mut normalized_preconditions = Vec::new();
        merge_json_preconditions(&mut normalized_preconditions, preconditions)
            .map_err(|_| feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict))?;
        Ok(ProceduralFeedbackApplicationPlan {
            mutations,
            preconditions: normalized_preconditions,
            owner_bindings,
            accepted_count,
            partially_accepted_count,
            method_dispositions,
            deferred_count,
            rejected_count,
            changed_count,
            rejection_cause: None,
        })
    }

    fn plan_runtime_promotions_from_accepted_experiences(
        &self,
        job: &ProceduralFeedbackJobV2,
        source_record: &bm_core::memory::TranscriptTurnRecord,
        mutations: &[StoreMutation],
        preconditions: &mut Vec<StoreJsonPrecondition>,
        observed_at: u64,
    ) -> Result<Vec<RuntimeSkillOwnerRecord>> {
        use crate::store_internal::schema::{
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE, PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
        };
        use bm_core::memory::{
            ProceduralAppliedOwnerBindingV1, ProceduralFeedbackApplicationLedgerV3,
        };
        use bm_core::skills::{
            plan_runtime_skill_promotion_from_tool_evidence, RuntimeSkillToolPromotionDecision,
            RuntimeSkillToolPromotionInput, RuntimeSkillToolPromotionSource,
        };
        let materials = mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreMutation::PutJson {
                    namespace, value, ..
                } if namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => Some(value),
                _ => None,
            })
            .map(|value| {
                serde_json::from_value::<AgentToolExperienceRevisionMaterialV4>(value.clone())
                    .map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        if materials.is_empty() {
            return Ok(Vec::new());
        }
        let store =
            self.config.store_platform.as_ref().ok_or_else(|| {
                feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
            })?;
        let runtime_budget = self.runtime_budget();
        let limits = &runtime_budget.governed_state_budget;
        let scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: job.feedback_source()?.identity.mounted_subject_id.clone(),
        };
        let runtime_snapshot = read_runtime_skill_scope_snapshot(
            store,
            &job.feedback_source()?.identity.memory_space_id,
            &scope,
            limits.max_retained_runtime_skill_owners_per_scope,
        )?;
        preconditions.push(json_precondition(
            crate::store_internal::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            &runtime_snapshot.manifest_key,
            runtime_snapshot.manifest_value.clone(),
        ));
        // Historical evidence is discovered only through the method's retained
        // execution references, never through a global application-ledger scan.
        let registries = self.agent_tool_registries();
        let mut promoted = Vec::new();
        for material in materials {
            if !matches!(
                material.body,
                bm_core::skills::AgentToolExperienceBodyV1::Method { .. }
            ) {
                continue;
            }
            let creation_ref = RuntimeSkillCreationRef::AgentToolExperiencePromotion {
                experience_owner_ref: material.owner_ref.clone(),
            };
            let existing = runtime_snapshot
                .records
                .iter()
                .find(|record| record.creation_ref == creation_ref);
            if let Some(existing) = existing {
                preconditions.push(StoreJsonPrecondition::Exact {
                    namespace: crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.into(),
                    key: existing.physical_key.clone(),
                    value: serde_json::to_value(existing).map_err(|_| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?,
                });
            }
            let registry = registries
                .iter()
                .find(|registry| {
                    registry.registry_id == material.registry_id
                        && registry.scope == material.registry_scope
                })
                .ok_or_else(|| {
                    feedback_error(crate::ProceduralLearningErrorKeyV1::EvidenceConflict)
                })?;
            let mut sources = vec![(job.clone(), source_record.clone())];
            let mut source_preconditions = Vec::new();
            // An existing promotion, including a retired one, is never recreated.
            // Core owns that decision; historical raw evidence need not be reread.
            if existing.is_none() {
                let bm_core::skills::AgentToolExperienceBodyV1::Method {
                    usage_contributions,
                    ..
                } = &material.body
                else {
                    unreachable!()
                };
                let source_keys = usage_contributions
                    .iter()
                    .filter(|source| source.source_job_id != job.job_id)
                    .map(|source| source.source_job_id.clone())
                    .collect::<BTreeSet<_>>();
                let head = mutations
                    .iter()
                    .find_map(|mutation| match mutation {
                        StoreMutation::PutJson {
                            namespace, value, ..
                        } if namespace == AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE => {
                            serde_json::from_value::<AgentToolExperienceOwnerHeadV3>(value.clone())
                                .ok()
                                .filter(|head| head.owner_ref == material.owner_ref)
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                    })?;
                let mut source_ids = BTreeSet::from([job.job_id.clone()]);
                for source_key in source_keys {
                    let document = store.read_json_docs_by_keys(
                        crate::store_internal::schema::PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                        std::slice::from_ref(&source_key))?.pop().ok_or_else(|| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired))?;
                    let ledger: ProceduralFeedbackApplicationLedgerV3 =
                        serde_json::from_value(document.value.clone()).map_err(|_| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    if ledger.identity.memory_space_id
                        != job.feedback_source()?.identity.memory_space_id
                        || ledger.identity.mounted_subject_id
                            != job.feedback_source()?.identity.mounted_subject_id
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    let applied =
                        ledger
                            .applied_owner_bindings
                            .iter()
                            .find_map(|binding| match binding {
                                ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                                    owner_revision,
                                    content_digest,
                                } if owner_revision.owner_ref == material.owner_ref => {
                                    Some((owner_revision, content_digest))
                                }
                                _ => None,
                            });
                    let Some((revision, digest)) = applied else {
                        continue;
                    };
                    ledger.validate()?;
                    let retained = head
                        .retained_revisions
                        .iter()
                        .find(|retained| {
                            retained.owner_revision == revision.owner_revision
                                && retained.owner_ref == revision.owner_ref
                                && retained.content_digest == *digest
                        })
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    if sources.len() >= limits.max_agent_tool_experience_evidence_refs_per_owner {
                        return Err(crate::store_internal::store_budget_error(
                            "promotion source evidence capacity is exhausted",
                        ));
                    }
                    let source_material_doc = store
                        .read_json_docs_by_keys(
                            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                            std::slice::from_ref(&retained.material_key),
                        )?
                        .pop()
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    let historical: AgentToolExperienceRevisionMaterialV4 =
                        serde_json::from_value(source_material_doc.value.clone()).map_err(
                            |_| feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired),
                        )?;
                    if AgentToolExperienceRetainedRevisionDigestV3::from_material(&historical)?
                        != *retained
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    let source_job_doc = store
                        .read_json_docs_by_keys(
                            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                            std::slice::from_ref(&ledger.job_id),
                        )?
                        .pop()
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    let source_job: ProceduralFeedbackJobV2 =
                        serde_json::from_value(source_job_doc.value.clone()).map_err(|_| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    source_job.validate()?;
                    if source_job.status
                        != bm_core::memory::ProceduralFeedbackJobStatusV1::Succeeded
                        || source_job.feedback_source()?.identity != ledger.identity
                        || source_job.feedback_source()?.learning_evidence_digest
                            != ledger.learning_evidence_digest
                        || source_job.job_id != ledger.job_id
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    let key = ConversationKey::new(
                        &ledger.identity.memory_space_id,
                        &ledger.identity.channel_id,
                        &ledger.identity.conversation_id,
                    )?;
                    let transcript = self
                        .config
                        .platform
                        .conversation_transcript_store()
                        .get_turn(
                            &key,
                            &ledger.identity.mounted_subject_id,
                            &ledger.identity.turn_id,
                        )?
                        .ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    if !transcript.permits_post_turn_learning() {
                        continue;
                    }
                    let source_evidence =
                        transcript.learning_evidence.as_ref().ok_or_else(|| {
                            feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                        })?;
                    if !source_evidence.validate_contract()
                        || source_evidence.learning_evidence_digest
                            != ledger.learning_evidence_digest
                    {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    let current = crate::store_internal::procedural_feedback::read_authorized_procedural_evidence(
                        store, source_evidence, &bm_core::memory::ProceduralProducerScopeV1 {
                            memory_space_id: ledger.identity.memory_space_id.clone(), mounted_subject_id: ledger.identity.mounted_subject_id.clone(),
                            channel_id: ledger.identity.channel_id.clone(), chat_id: ledger.identity.chat_id.clone(),
                        })?;
                    self.validate_procedural_current_governance(
                        &current.historical,
                        &current.current,
                        crate::ProceduralLearningSdkOperation::ApplyFeedback,
                    )?;
                    merge_json_preconditions(
                        &mut source_preconditions,
                        self.resolve_procedural_source_authority(
                            &current.historical.spec.source_authority,
                            crate::ProceduralLearningSdkOperation::ApplyFeedback,
                        )?,
                    )?;
                    merge_json_preconditions(
                        &mut source_preconditions,
                        vec![StoreJsonPrecondition::Exact {
                            namespace:
                                crate::store_internal::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE
                                    .into(),
                            key: current.head.binding_key.clone(),
                            value: serde_json::to_value(&current.head).map_err(|_| {
                                feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                            })?,
                        }],
                    )?;
                    if source_evidence.authority.is_model_inferred() {
                        continue;
                    }
                    if !source_ids.insert(source_job.job_id.clone()) {
                        return Err(feedback_error(
                            crate::ProceduralLearningErrorKeyV1::RepairRequired,
                        ));
                    }
                    for (namespace, key, value) in [
                        (
                            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                            document.key.clone(),
                            document.value.clone(),
                        ),
                        (
                            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                            source_material_doc.key,
                            source_material_doc.value,
                        ),
                        (
                            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                            source_job_doc.key,
                            source_job_doc.value,
                        ),
                        (
                            "conversation_transcript",
                            crate::store_internal::transcript_turn_storage_key(
                                &key,
                                &ledger.identity.mounted_subject_id,
                                &ledger.identity.turn_id,
                            ),
                            serde_json::to_value(&transcript).map_err(|_| {
                                feedback_error(crate::ProceduralLearningErrorKeyV1::RepairRequired)
                            })?,
                        ),
                    ] {
                        source_preconditions.push(StoreJsonPrecondition::Exact {
                            namespace: namespace.into(),
                            key,
                            value,
                        });
                    }
                    sources.push((source_job, transcript));
                }
            }
            let adopted = sources
                .iter()
                .map(|(_, transcript)| {
                    crate::store_internal::procedural_feedback::read_method_adopted_materials(
                        transcript,
                        |namespace, key| {
                            let document = store
                                .read_json_docs_by_keys(namespace, &[key.into()])?
                                .pop()
                                .ok_or_else(|| {
                                    feedback_error(
                                        crate::ProceduralLearningErrorKeyV1::RepairRequired,
                                    )
                                })?;
                            merge_json_preconditions(
                                &mut source_preconditions,
                                [StoreJsonPrecondition::Exact {
                                    namespace: document.namespace,
                                    key: document.key,
                                    value: document.value.clone(),
                                }],
                            )?;
                            Ok(document.value)
                        },
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let sources = sources
                .iter()
                .zip(&adopted)
                .map(
                    |((job, transcript), adopted_materials)| RuntimeSkillToolPromotionSource {
                        job,
                        transcript,
                        adopted_materials,
                    },
                )
                .collect::<Vec<_>>();
            match plan_runtime_skill_promotion_from_tool_evidence(
                &RuntimeSkillToolPromotionInput {
                    experience: &material,
                    registry,
                    sources: &sources,
                    existing_owner: existing,
                    observed_at,
                },
            )? {
                RuntimeSkillToolPromotionDecision::Promote { record, .. } => {
                    merge_json_preconditions(preconditions, source_preconditions)?;
                    promoted.push(*record);
                }
                RuntimeSkillToolPromotionDecision::Observe { .. }
                | RuntimeSkillToolPromotionDecision::Reject { .. }
                | RuntimeSkillToolPromotionDecision::AlreadyPromoted { .. } => {}
            }
        }
        Ok(promoted)
    }
}
