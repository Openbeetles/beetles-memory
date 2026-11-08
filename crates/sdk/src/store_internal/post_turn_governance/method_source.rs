//! Exact semantic application proof shared by locked Store validation and
//! immutable reads. Readers supply one bounded snapshot, never a live fallback.
use super::*;
use bm_core::memory::{
    GovernanceMethodExtractionSourceV1, PostTurnGovernanceMethodApplicationV1,
    ProceduralAppliedOwnerBindingV1, ProceduralProducerScopeV1, TranscriptTurnRecord,
};
use bm_core::skills::{agent_tool_experience_material_key, AgentToolExperienceRevisionMaterialV4};

fn application_binding(
    job: &PostTurnGovernanceJob,
    read: &mut impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<PostTurnGovernanceBindingSnapshotV1> {
    let invalid = || {
        Error::config(
            "semantic_method_source_repair_required",
            "semantic application binding differs",
        )
    };
    let PostTurnGovernanceExecutionBindingV1::Bound {
        binding_id,
        binding_revision,
    } = &job.execution_binding
    else {
        return Err(invalid());
    };
    let binding: PostTurnGovernanceBindingSnapshotV1 = serde_json::from_value(read(
        POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE,
        &format!("{binding_id}:{binding_revision}"),
    )?)
    .map_err(|_| invalid())?;
    binding.validate().map_err(|_| invalid())?;
    if binding.binding_id != *binding_id || binding.binding_revision != *binding_revision {
        return Err(invalid());
    }
    Ok(binding)
}

/// Historical application authority, independent of today's read permission.
/// The exact immutable receipt must resolve every producing revision, or its
/// exact retained deletion tombstone. This never grants model access to raw.
pub(crate) fn verify_semantic_method_application(
    job: &PostTurnGovernanceJob,
    mut read: impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<Vec<ProceduralAppliedOwnerBindingV1>> {
    let invalid = || {
        Error::config(
            "semantic_method_source_repair_required",
            "semantic application lacks exact producing closure",
        )
    };
    job.validate().map_err(|_| invalid())?;
    if job.status != PostTurnGovernanceJobStatusV2::Succeeded {
        return Err(invalid());
    }
    let receipt = job.receipt.as_ref().ok_or_else(invalid)?;
    let PostTurnGovernanceMethodApplicationV1::Applied {
        owner_bindings,
        source_commitment_digest,
        ..
    } = &receipt.method_application
    else {
        return Err(invalid());
    };
    let binding = application_binding(job, &mut read)?;
    if job
        .method_source_commitment_digest(&binding)
        .map_err(|_| invalid())?
        != *source_commitment_digest
    {
        return Err(invalid());
    }
    let scope = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: job.identity.mounted_subject_id.clone(),
    };
    for owner_binding in owner_bindings {
        let ProceduralAppliedOwnerBindingV1::AgentToolExperience {
            owner_revision,
            content_digest,
        } = owner_binding
        else {
            return Err(invalid());
        };
        let key = agent_tool_experience_material_key(
            &job.identity.memory_space_id,
            &scope,
            &owner_revision.owner_ref,
            owner_revision.owner_revision,
        )?;
        let head_key = bm_core::skills::agent_tool_experience_head_key(
            &job.identity.memory_space_id,
            &scope,
            &owner_revision.owner_ref,
        )?;
        let head: bm_core::skills::AgentToolExperienceOwnerHeadV3 = serde_json::from_value(read(
            super::super::schema::AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
            &head_key,
        )?)
        .map_err(|_| invalid())?;
        if !head.validate_contract().accepted
            || head.physical_key != head_key
            || head.memory_space_id != job.identity.memory_space_id
            || head.owning_scope != scope
            || head.owner_ref != owner_revision.owner_ref
            || !head.retained_revisions.iter().any(|retained| {
                retained.material_key == key
                    && retained.owner_ref == owner_revision.owner_ref
                    && retained.owner_revision == owner_revision.owner_revision
                    && retained.content_digest == *content_digest
            })
        {
            return Err(invalid());
        }
        let raw = read(
            super::super::schema::AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
            &key,
        )?;
        if raw.is_null() {
            if head.state != bm_core::skills::AgentToolExperienceHeadStateV3::Tombstoned {
                return Err(invalid());
            }
            continue;
        }
        let material: AgentToolExperienceRevisionMaterialV4 =
            serde_json::from_value(raw).map_err(|_| invalid())?;
        if !material.validate_contract().accepted
            || material.physical_key != key
            || material.owner_revision_ref() != *owner_revision
            || material.content_digest != *content_digest
            || material.memory_space_id != job.identity.memory_space_id
            || material.owning_scope != scope
            || head.state == bm_core::skills::AgentToolExperienceHeadStateV3::Tombstoned
        {
            return Err(invalid());
        }
        let sources = material
            .body
            .method_sources()
            .iter()
            .filter_map(|source| source.governance_extraction())
            .filter(|source| source.source_job_id == job.job_id)
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return Err(invalid());
        }
        for source in sources {
            if !source.validate_contract()
                || source.identity != job.identity
                || source.source_transcript_sequence != job.transcript_sequence
                || source.source_transcript_digest != job.transcript_digest
                || source.runtime_binding_digest != job.runtime_binding_digest
                || source.governance_contract_version != job.governance_contract_version
                || source.execution_binding != job.execution_binding
                || source.execution_binding_digest != binding.canonical_digest
                || source.tool.registry_ref.registry_id != material.registry_id
                || source.tool.registry_ref.scope != material.registry_scope
                || source.tool.tool_id != material.tool_id
                || source.tool.schema_fingerprint != material.schema_fingerprint
                || material.body.method_content_digest()?.as_deref()
                    != Some(source.method_digest.as_str())
            {
                return Err(invalid());
            }
        }
    }
    Ok(owner_bindings.clone())
}

pub(crate) fn verify_semantic_method_source(
    source: &GovernanceMethodExtractionSourceV1,
    consumer: &AgentToolExperienceRevisionMaterialV4,
    mut read: impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<bool> {
    let stage = "semantic_method_source_repair_required";
    let invalid = || Error::config(stage, "semantic method source has no exact durable closure");
    if !source.validate_contract()
        || !consumer.validate_contract().accepted
        || consumer.memory_space_id != source.identity.memory_space_id
        || consumer.owning_scope.mounted_subject_id() != source.identity.mounted_subject_id
        || consumer.registry_id != source.tool.registry_ref.registry_id
        || consumer.registry_scope != source.tool.registry_ref.scope
        || consumer.tool_id != source.tool.tool_id
        || consumer.schema_fingerprint != source.tool.schema_fingerprint
        || consumer.body.method_content_digest()?.as_deref() != Some(source.method_digest.as_str())
    {
        return Err(invalid());
    }
    let job: PostTurnGovernanceJob = serde_json::from_value(read(
        POST_TURN_GOVERNANCE_JOB_NAMESPACE,
        &source.source_job_id,
    )?)
    .map_err(|_| invalid())?;
    job.validate().map_err(|_| invalid())?;
    // Durable producing commitments remain auditable after lawful raw-source
    // withdrawal. Check them before the current read-permission short circuit;
    // a denied source must not hide corruption in the retained provenance.
    if job.job_id != source.source_job_id
        || job.identity != source.identity
        || job.status != PostTurnGovernanceJobStatusV2::Succeeded
        || source.source_transcript_sequence != job.transcript_sequence
        || source.source_transcript_digest != job.transcript_digest
        || source.runtime_binding_digest != job.runtime_binding_digest
        || source.governance_contract_version != job.governance_contract_version
    {
        return Err(invalid());
    }
    let owner_bindings = verify_semantic_method_application(&job, &mut read)?;
    let mut matched = false;
    for binding in owner_bindings {
        let ProceduralAppliedOwnerBindingV1::AgentToolExperience { owner_revision, .. } = binding
        else {
            return Err(invalid());
        };
        if owner_revision.owner_ref != consumer.owner_ref {
            continue;
        }
        let key = agent_tool_experience_material_key(
            &consumer.memory_space_id,
            &consumer.owning_scope,
            &owner_revision.owner_ref,
            owner_revision.owner_revision,
        )?;
        let raw = read(
            super::super::schema::AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
            &key,
        )?;
        if raw.is_null() {
            continue;
        }
        let material: AgentToolExperienceRevisionMaterialV4 =
            serde_json::from_value(raw).map_err(|_| invalid())?;
        matched |= material
            .body
            .method_sources()
            .iter()
            .filter_map(|value| value.governance_extraction())
            .any(|value| value == source);
    }
    if !matched {
        return Err(invalid());
    }
    let binding = application_binding(&job, &mut read)?;
    if source.execution_binding_digest != binding.canonical_digest
        || source.execution_binding != job.execution_binding
    {
        return Err(invalid());
    }
    let key = bm_core::memory::ConversationKey::new(
        &job.identity.memory_space_id,
        &job.identity.channel_id,
        &job.identity.conversation_id,
    )?;
    let transcript: TranscriptTurnRecord = serde_json::from_value(read(
        "conversation_transcript",
        &super::super::transcript_turn_storage_key(
            &key,
            &job.identity.mounted_subject_id,
            &job.identity.turn_id,
        ),
    )?)
    .map_err(|_| invalid())?;
    if transcript.key != key
        || transcript.subject != job.identity.mounted_subject_id
        || transcript.turn_id != job.identity.turn_id
        || transcript.sequence != source.source_transcript_sequence
    {
        return Err(invalid());
    }
    // A legitimate lifecycle denial is not corruption and never reopens raw
    // content. Missing roots and forged commitments above remain repair errors.
    if !transcript.permits_post_turn_learning() {
        return Ok(false);
    }
    source
        .validate_against(&job, &binding, &transcript)
        .map_err(|_| invalid())?;
    let evidence = transcript.learning_evidence.as_ref().ok_or_else(invalid)?;
    let reference = evidence.authority.producer_revision().ok_or_else(invalid)?;
    let producer = super::super::procedural_feedback::read_verified_procedural_producer_with(
        reference, &mut read,
    )?;
    let scope = ProceduralProducerScopeV1 {
        memory_space_id: job.identity.memory_space_id,
        mounted_subject_id: job.identity.mounted_subject_id,
        channel_id: job.identity.channel_id,
        chat_id: job.identity.chat_id,
    };
    let sources_permitted = super::verify_governed_method_material_sources(
        &scope.memory_space_id,
        &scope.mounted_subject_id,
        &source.materials,
        &mut read,
    )?;
    let producer_permitted = super::verify_governed_producer_sources(
        &scope.memory_space_id,
        &scope.mounted_subject_id,
        &producer.historical.spec.source_authority,
        &producer.current.spec.source_authority,
        &mut read,
    )?;
    Ok(sources_permitted
        && producer_permitted
        && producer
            .historical
            .authorize_evidence_with_current(&producer.current, &scope, evidence)
            .is_ok())
}

/// Exact known-key edges consumed by the existing bounded transaction closure.
pub(crate) fn dependencies(
    namespace: &str,
    value: &serde_json::Value,
) -> Result<Vec<(String, String)>> {
    use super::super::schema::*;
    let invalid = || {
        Error::config(
            "semantic_method_source_repair_required",
            "invalid semantic dependency document",
        )
    };
    let mut addresses = Vec::new();
    match namespace {
        AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
            let material: AgentToolExperienceRevisionMaterialV4 =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            for source in material
                .body
                .method_sources()
                .iter()
                .filter_map(|source| source.governance_extraction())
            {
                addresses.push((
                    POST_TURN_GOVERNANCE_JOB_NAMESPACE.into(),
                    source.source_job_id.clone(),
                ));
                for material_ref in &source.materials {
                    if let bm_core::memory::MethodLearningMaterialSourceV1::GovernedRevision {
                        revision,
                        ..
                    } = &material_ref.source
                    {
                        addresses.push((
                            PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE.into(),
                            bm_core::memory::ProceduralSourceDependentsRootV1::key(
                                &source.identity.memory_space_id,
                                &revision.owner_ref,
                            )?,
                        ));
                    }
                }
            }
        }
        POST_TURN_GOVERNANCE_JOB_NAMESPACE => {
            let job: PostTurnGovernanceJob =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            job.validate()?;
            let key = bm_core::memory::ConversationKey::new(
                &job.identity.memory_space_id,
                &job.identity.channel_id,
                &job.identity.conversation_id,
            )?;
            addresses.push((
                "conversation_transcript".into(),
                super::super::transcript_turn_storage_key(
                    &key,
                    &job.identity.mounted_subject_id,
                    &job.identity.turn_id,
                ),
            ));
            if let PostTurnGovernanceExecutionBindingV1::Bound {
                binding_id,
                binding_revision,
            } = &job.execution_binding
            {
                addresses.push((
                    POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE.into(),
                    format!("{binding_id}:{binding_revision}"),
                ));
            }
            if let Some(PostTurnGovernanceMethodApplicationV1::Applied { owner_bindings, .. }) =
                job.receipt.as_ref().map(|value| &value.method_application)
            {
                let scope = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
                    mounted_subject_id: job.identity.mounted_subject_id.clone(),
                };
                for binding in owner_bindings {
                    let ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                        owner_revision, ..
                    } = binding
                    else {
                        return Err(invalid());
                    };
                    addresses.push((
                        AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.into(),
                        agent_tool_experience_material_key(
                            &job.identity.memory_space_id,
                            &scope,
                            &owner_revision.owner_ref,
                            owner_revision.owner_revision,
                        )?,
                    ));
                    addresses.push((
                        AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.into(),
                        bm_core::skills::agent_tool_experience_head_key(
                            &job.identity.memory_space_id,
                            &scope,
                            &owner_revision.owner_ref,
                        )?,
                    ));
                }
            }
        }
        "conversation_transcript" => {
            let turn: TranscriptTurnRecord =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            if let Some(producer) = turn
                .learning_evidence
                .as_ref()
                .and_then(|value| value.authority.producer_revision())
            {
                addresses.push((
                    PROCEDURAL_PRODUCER_HEAD_NAMESPACE.into(),
                    producer.binding_key.clone(),
                ));
            }
            if let Some(evidence) = &turn.learning_evidence {
                for adoption in &evidence.method_adoptions {
                    let reference = &adoption.method;
                    let scope = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
                        mounted_subject_id: reference.mounted_subject_id.clone(),
                    };
                    addresses.push((
                        AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.into(),
                        agent_tool_experience_material_key(
                            &reference.memory_space_id,
                            &scope,
                            &reference.owner_revision.owner_ref,
                            reference.owner_revision.owner_revision,
                        )?,
                    ));
                }
            }
        }
        LONG_TERM_HEAD_MANIFEST_NAMESPACE => {
            let head: bm_core::memory::LongTermMemoryHeadManifest =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            if !head.validate_contract().accepted {
                return Err(invalid());
            }
            addresses.push((
                LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE.into(),
                bm_core::memory::long_term_version_scope_manifest_key(
                    &head.memory_space_id,
                    &head.factual_owner_id,
                )?,
            ));
            for retained in &head.retained_revision_digests {
                addresses.push((
                    LONG_TERM_VERSION_MATERIAL_NAMESPACE.into(),
                    bm_core::memory::long_term_version_material_key(
                        &head.memory_space_id,
                        &head.factual_owner_id,
                        &head.owner_ref,
                        retained.owner_revision,
                    )?,
                ));
            }
        }
        bm_core::memory::LONG_TERM_CONTROL_REVISION_NAMESPACE => {
            let control: bm_core::memory::LongTermMemoryControlRevision =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            control.validate_contract()?;
            if let Some(successor) = &control.transition.successor {
                addresses.push((
                    LONG_TERM_HEAD_MANIFEST_NAMESPACE.into(),
                    bm_core::memory::long_term_version_head_key(
                        &control.memory_space_id,
                        &control.factual_owner_id,
                        &successor.owner_ref,
                    )?,
                ));
                addresses.push((
                    LONG_TERM_VERSION_MATERIAL_NAMESPACE.into(),
                    bm_core::memory::long_term_version_material_key(
                        &control.memory_space_id,
                        &control.factual_owner_id,
                        &successor.owner_ref,
                        successor.owner_revision,
                    )?,
                ));
            }
        }
        super::super::GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
            let document: bm_core::memory::GovernedEvidenceDocument =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            bm_core::memory::validate_governed_evidence_document(&document)
                .map_err(|_| invalid())?;
            let claim = bm_core::memory::governed_evidence_source_ref_from_document(&document)?;
            addresses.push((
                super::super::GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.into(),
                claim.physical_key,
            ));
            addresses.push((
                GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.into(),
                governed_evidence_source_claim_manifest_key(
                    &document.memory_space_id,
                    &document.mounted_subject_id,
                )?,
            ));
        }
        _ => {}
    }
    addresses.sort();
    addresses.dedup();
    Ok(addresses)
}
