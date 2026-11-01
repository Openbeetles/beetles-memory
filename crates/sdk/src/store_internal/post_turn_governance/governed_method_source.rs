//! Current source authority for immutable method provenance. Callers provide one
//! bounded immutable snapshot or retain every observation as a transaction CAS.
use super::super::procedural_feedback::source_dependents;
use super::super::schema::*;
use super::super::{GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE, GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE};
use bm_core::memory::*;
use bm_core::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};

fn invalid() -> Error {
    Error::config(
        "semantic_method_source_repair_required",
        "governed method source closure is invalid",
    )
}

struct SourceRead<'a, F> {
    read: &'a mut F,
    values: BTreeMap<(String, String), serde_json::Value>,
    absent: BTreeSet<(String, String)>,
}
impl<F: FnMut(&str, &str) -> Result<serde_json::Value>> SourceRead<'_, F> {
    fn optional<T: serde::de::DeserializeOwned>(
        &mut self,
        ns: &str,
        key: &str,
    ) -> Result<Option<T>> {
        let address = (ns.to_owned(), key.to_owned());
        if self.absent.contains(&address) {
            return Ok(None);
        }
        let value = match self.values.get(&address) {
            Some(value) => value.clone(),
            None => {
                let value = (self.read)(ns, key)?;
                if value.is_null() {
                    self.absent.insert(address);
                    return Ok(None);
                }
                self.values.insert(address, value.clone());
                value
            }
        };
        serde_json::from_value(value)
            .map(Some)
            .map_err(|_| invalid())
    }
    fn required<T: serde::de::DeserializeOwned>(&mut self, ns: &str, key: &str) -> Result<T> {
        self.optional(ns, key)?.ok_or_else(invalid)
    }
}

pub(crate) fn verify_governed_method_material_sources(
    space: &str,
    subject: &str,
    materials: &[MethodLearningMaterialRefV1],
    read: impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<bool> {
    if materials
        .iter()
        .any(|material| !material.validate_contract())
    {
        return Err(invalid());
    }
    let revisions = materials
        .iter()
        .filter_map(|material| match &material.source {
            MethodLearningMaterialSourceV1::GovernedRevision {
                revision,
                content_digest,
            } => Some((revision, Some(content_digest.as_str()))),
            _ => None,
        })
        .collect::<Vec<_>>();
    verify_governed_revisions(space, subject, &revisions, read)
}

pub(crate) fn verify_governed_producer_sources(
    space: &str,
    subject: &str,
    historical: &ProceduralProducerSourceAuthorityV1,
    current: &ProceduralProducerSourceAuthorityV1,
    read: impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<bool> {
    let revisions = [historical, current]
        .into_iter()
        .filter_map(|authority| match authority {
            ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } => {
                Some((source_revision, None))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    verify_governed_revisions(space, subject, &revisions, read)
}

fn verify_governed_revisions(
    space: &str,
    subject: &str,
    revisions: &[(&GovernedOwnerRevisionRef, Option<&str>)],
    mut read: impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<bool> {
    let mut snapshot = SourceRead {
        read: &mut read,
        values: BTreeMap::new(),
        absent: BTreeSet::new(),
    };
    let mut permitted = true;
    for &(revision, content_digest) in revisions {
        let root: ProceduralSourceDependentsRootV1 = snapshot.required(
            PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE,
            &ProceduralSourceDependentsRootV1::key(space, &revision.owner_ref)?,
        )?;
        root.validate()?;
        if root.memory_space_id != space || root.owner_ref != revision.owner_ref {
            return Err(invalid());
        }
        if let Some(operation) = &root.last_change.operation {
            let _: serde_json::Value =
                snapshot.required(MEMORY_MUTATION_RECEIPT_NAMESPACE, &operation.storage_key())?;
            let _: serde_json::Value =
                snapshot.required(MEMORY_MUTATION_AUDIT_NAMESPACE, &operation.storage_key())?;
        }
        permitted &= match revision.owner_ref.owner_plane {
            GovernedMemoryOwnerPlane::LongTerm => {
                long_term_source(&mut snapshot, space, subject, revision, content_digest)?
            }
            GovernedMemoryOwnerPlane::EvidenceDocument => {
                evidence_source(&mut snapshot, space, subject, revision, content_digest)?
            }
            _ => return Err(invalid()),
        };
        permitted &= matches!(root.current, ProceduralSourcePostImageV1::Present { .. });
    }
    // Reuse the source owner's exact current-image, terminal control and receipt
    // proof. Reverse dependent fanout is not part of reading a source.
    source_dependents::validate_image(
        &snapshot.values,
        source_dependents::SourceValidationScope::Transaction(&BTreeSet::new()),
    )?;
    Ok(permitted)
}

fn long_term_source<F: FnMut(&str, &str) -> Result<serde_json::Value>>(
    read: &mut SourceRead<'_, F>,
    space: &str,
    subject: &str,
    reference: &GovernedOwnerRevisionRef,
    digest: Option<&str>,
) -> Result<bool> {
    let owner = &reference.owner_ref;
    let Some(head) = read.optional::<LongTermMemoryHeadManifest>(
        LONG_TERM_HEAD_MANIFEST_NAMESPACE,
        &long_term_version_head_key(space, space, owner)?,
    )?
    else {
        return Ok(false);
    };
    if !head.validate_contract().accepted
        || head.memory_space_id != space
        || head.factual_owner_id != space
        || head.owner_ref != *owner
    {
        return Err(invalid());
    }
    let scope: LongTermMemoryVersionScopeManifest = read.required(
        LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
        &long_term_version_scope_manifest_key(space, space)?,
    )?;
    let mut materials = Vec::with_capacity(head.retained_revision_digests.len());
    for retained in &head.retained_revision_digests {
        let material: LongTermMemoryVersionMaterial = read.required(
            LONG_TERM_VERSION_MATERIAL_NAMESPACE,
            &long_term_version_material_key(space, space, owner, retained.owner_revision)?,
        )?;
        materials.push(material);
    }
    let expected = materials
        .iter()
        .filter(|value| {
            value.owner_revision != head.current_revision
                || head.terminal_transition_ref.as_ref() == Some(&value.owner_revision_ref())
        })
        .map(LongTermMemoryVersionMaterial::owner_revision_ref)
        .collect::<BTreeSet<_>>();
    let bindings = scope
        .transition_bindings
        .iter()
        .filter(|binding| expected.contains(&binding.predecessor))
        .collect::<Vec<_>>();
    if bindings.len() != expected.len() {
        return Err(invalid());
    }
    let mut controls = Vec::with_capacity(bindings.len());
    let mut dependency_heads = BTreeMap::new();
    let mut dependency_materials = BTreeMap::new();
    for binding in bindings {
        let control: LongTermMemoryControlRevision = read.required(
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            &binding.control_revision_physical_key,
        )?;
        control.validate_contract()?;
        if control.memory_space_id != space
            || control.factual_owner_id != space
            || control.transition.predecessor != binding.predecessor
            || control.content_digest != binding.control_revision_content_digest
            || scoped_long_term_control_storage_key(
                space,
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                &control.revision_id,
            )? != binding.control_revision_physical_key
        {
            return Err(invalid());
        }
        if let Some(successor) = &control.transition.successor {
            if !materials
                .iter()
                .any(|value| value.owner_revision_ref() == *successor)
            {
                let successor_head: LongTermMemoryHeadManifest = read.required(
                    LONG_TERM_HEAD_MANIFEST_NAMESPACE,
                    &long_term_version_head_key(space, space, &successor.owner_ref)?,
                )?;
                let successor_material: LongTermMemoryVersionMaterial = read.required(
                    LONG_TERM_VERSION_MATERIAL_NAMESPACE,
                    &long_term_version_material_key(
                        space,
                        space,
                        &successor.owner_ref,
                        successor.owner_revision,
                    )?,
                )?;
                dependency_heads.insert(successor.owner_ref.clone(), successor_head);
                dependency_materials.insert(successor.clone(), successor_material);
            }
        }
        controls.push(control);
    }
    let dependency_heads = dependency_heads.into_values().collect::<Vec<_>>();
    let dependency_materials = dependency_materials.into_values().collect::<Vec<_>>();
    // The caller's Store snapshot/read budget bounds the loaded closure. The
    // Core owner validates its retained revision and cross-owner commitments.
    let retention = materials.len().max(1);
    let current = build_long_term_current_recall_authority(
        &scope,
        &head,
        &materials,
        &dependency_heads,
        &dependency_materials,
        &controls,
        retention,
    )?;
    let Some(exact) = materials
        .iter()
        .find(|value| value.owner_revision_ref() == *reference)
    else {
        return Ok(false);
    };
    if digest.is_some_and(|digest| exact.content_digest != digest) {
        return Err(invalid());
    }
    let query_time = current
        .projection()
        .validity
        .valid_from
        .max(exact.origin.valid_from)
        .max(1);
    let gates = GovernedRecallAuthorityGates {
        disclosure: GovernedRecallDisclosure::Allowed,
        required_premise: GovernedRequiredPremiseGate::NotApplicable,
        profile_budget_drop: GovernedProfileBudgetDrop::None,
    };
    let facts = project_current_long_term_recall_lifecycle_facts(&current)?;
    if decide_governed_recall_eligibility(
        &facts,
        GovernedRecallTemporalQuery::Current { query_time },
        gates.clone(),
    )?
    .eligibility
        == GovernedRecallEligibility::Excluded
    {
        return Ok(false);
    }
    if exact.owner_revision != current.projection().material.owner_revision {
        let mut heads = vec![head];
        heads.extend(dependency_heads);
        let mut all_materials = materials.clone();
        all_materials.extend(dependency_materials);
        let Some(historical) = build_long_term_historical_recall_authority(
            &scope,
            &heads,
            &all_materials,
            &controls,
            owner,
            exact.origin.valid_from,
            retention,
            controls.len().saturating_add(1),
        )?
        else {
            return Ok(false);
        };
        if historical.projection().material.owner_revision_ref() != *reference {
            return Ok(false);
        }
        if decide_governed_recall_eligibility(
            &project_historical_long_term_recall_lifecycle_facts(&historical)?,
            GovernedRecallTemporalQuery::HistoricalAsOf {
                query_time,
                as_of_time: exact.origin.valid_from,
            },
            gates,
        )?
        .eligibility
            == GovernedRecallEligibility::Excluded
        {
            return Ok(false);
        }
    }
    for material in [exact, &current.projection().material] {
        if !ProceduralProducerSourceAuthorityV1::permits_governed_method_source(
            material.governed_content.provenance.source_authority,
            material.privacy_class,
            false,
        ) || material.subject_visibility.decision_for_subject(subject)?
            != MemorySubjectVisibilityDecision::Allowed
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn evidence_source<F: FnMut(&str, &str) -> Result<serde_json::Value>>(
    read: &mut SourceRead<'_, F>,
    space: &str,
    subject: &str,
    reference: &GovernedOwnerRevisionRef,
    digest: Option<&str>,
) -> Result<bool> {
    let key = scoped_governed_evidence_document_key(space, &reference.owner_ref.owner_id)?;
    let Some(document) =
        read.optional::<GovernedEvidenceDocument>(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE, &key)?
    else {
        return Ok(false);
    };
    validate_governed_evidence_document(&document).map_err(|_| invalid())?;
    if document.physical_key != key
        || document.memory_space_id != space
        || document.document_id != reference.owner_ref.owner_id
    {
        return Err(invalid());
    }
    let expected_claim = governed_evidence_source_ref_from_document(&document)?;
    let claim: GovernedEvidenceSourceRef = read.required(
        GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE,
        &expected_claim.physical_key,
    )?;
    validate_governed_evidence_source_ref(&document, &claim).map_err(|_| invalid())?;
    let manifest: GovernedEvidenceSourceClaimManifest = read.required(
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
        &governed_evidence_source_claim_manifest_key(space, &document.mounted_subject_id)?,
    )?;
    manifest.validate_exact(
        space,
        &document.mounted_subject_id,
        manifest.owner_claim_bindings.clone(),
        manifest.owner_claim_bindings.len().max(1),
    )?;
    if manifest.binding_for_owner(&key)
        != Some(&GovernedEvidenceOwnerClaimBinding::from_document_claim(
            &document, &claim,
        )?)
    {
        return Err(invalid());
    }
    if document.owner_revision != reference.owner_revision {
        return Ok(false);
    }
    if digest.is_some_and(|digest| document.content_digest != digest) {
        return Err(invalid());
    }
    Ok((document.privacy == MemoryPrivacyClass::PublicRuntime
        || document.mounted_subject_id == subject)
        && ProceduralProducerSourceAuthorityV1::permits_governed_method_source(
            document.authority,
            document.privacy,
            document.source_kind == GovernedEvidenceDocumentSourceKind::ExternalContent,
        ))
}
