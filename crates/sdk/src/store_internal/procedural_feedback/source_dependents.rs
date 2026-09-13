//! Atomic reverse discovery and current-source fencing at the Store boundary.
//! No registry policy is owned here; source and producer owners remain typed.

use super::*;
use crate::store_internal::schema::{
    LONG_TERM_HEAD_MANIFEST_NAMESPACE as LT_HEAD,
    LONG_TERM_VERSION_MATERIAL_NAMESPACE as LT_MATERIAL,
    LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE as LT_SCOPE,
    PROCEDURAL_PRODUCER_BINDING_NAMESPACE as PRODUCER, PROCEDURAL_PRODUCER_HEAD_NAMESPACE as HEAD,
    PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE as ROOT,
};
use crate::store_internal::transaction::BackendTransactionState;
use crate::store_internal::{
    StoreCapacityBudget, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE as DOCUMENT,
};
use bm_core::memory::{
    long_term_version_head_key, long_term_version_material_key,
    long_term_version_scope_manifest_key, scoped_governed_evidence_document_key,
    validate_governed_evidence_document, GovernedEvidenceDocument, GovernedMemoryOwnerRef,
    GovernedOwnerRevisionRef, LongTermMemoryControlRevision, LongTermMemoryHeadManifest,
    LongTermMemoryVersionMaterial, LongTermMemoryVersionScopeManifest, ProceduralProducerBindingV1,
    ProceduralProducerSourceAuthorityV1, ProceduralSourceDependentsRootV1 as Root,
    ProceduralSourcePostImageV1 as Image, ProceduralSourceTransitionV1,
};

const CONTROL: &str = bm_core::memory::LONG_TERM_CONTROL_REVISION_NAMESPACE;
const STAGE: &str = "procedural_source_closure_repair_required";
type Address = (String, String);
type Json = BTreeMap<Address, serde_json::Value>;

fn repair() -> Error {
    Error::config(
        STAGE,
        "governed source and retained dependent closure is incomplete or inconsistent",
    )
}
fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| repair())
}

pub(crate) fn source_address(space: &str, owner: &GovernedMemoryOwnerRef) -> Result<Address> {
    match owner.owner_plane {
        GovernedMemoryOwnerPlane::LongTerm => Ok((
            LT_HEAD.into(),
            long_term_version_head_key(space, space, owner)?,
        )),
        GovernedMemoryOwnerPlane::EvidenceDocument => Ok((
            DOCUMENT.into(),
            scoped_governed_evidence_document_key(space, &owner.owner_id)?,
        )),
        _ => Err(repair()),
    }
}

/// One resolver for planning, locked post-image and Store-open. A terminal head
/// is not absence: its retained exact material and real control binding remain.
fn source_image(
    space: &str,
    owner: &GovernedMemoryOwnerRef,
    mut read: impl FnMut(&str, &str) -> Result<Option<serde_json::Value>>,
) -> Result<Option<Image>> {
    let (namespace, key) = source_address(space, owner)?;
    let Some(value) = read(&namespace, &key)? else {
        return Ok(None);
    };
    if namespace == DOCUMENT {
        let document: GovernedEvidenceDocument = parse(value)?;
        validate_governed_evidence_document(&document).map_err(|_| repair())?;
        if document.physical_key != key
            || document.memory_space_id != space
            || document.document_id != owner.owner_id
        {
            return Err(repair());
        }
        return Ok(Some(Image::Present {
            revision: GovernedOwnerRevisionRef::try_new(owner.clone(), document.owner_revision)?,
            content_digest: format!("sha256:{}", document.content_digest),
        }));
    }
    let head: LongTermMemoryHeadManifest = parse(value)?;
    if !head.validate_contract().accepted
        || head.memory_space_id != space
        || head.factual_owner_id != space
        || head.owner_ref != *owner
    {
        return Err(repair());
    }
    let material_key = long_term_version_material_key(space, space, owner, head.current_revision)?;
    let material: LongTermMemoryVersionMaterial =
        parse(read(LT_MATERIAL, &material_key)?.ok_or_else(repair)?)?;
    if !material.validate_contract().accepted
        || material.memory_space_id != space
        || material.factual_owner_id != space
        || material.owner_ref != *owner
        || material.owner_revision != head.current_revision
        || !head.retained_revision_digests.iter().any(|item| {
            item.owner_revision == material.owner_revision
                && item.content_digest == material.content_digest
        })
    {
        return Err(repair());
    }
    let revision = GovernedOwnerRevisionRef::try_new(owner.clone(), material.owner_revision)?;
    let content_digest = format!("sha256:{}", material.content_digest);
    if let Some(terminal) = &head.terminal_transition_ref {
        if *terminal != revision {
            return Err(repair());
        }
        let scope: LongTermMemoryVersionScopeManifest = parse(
            read(
                LT_SCOPE,
                &long_term_version_scope_manifest_key(space, space)?,
            )?
            .ok_or_else(repair)?,
        )?;
        let binding = scope
            .transition_bindings
            .iter()
            .find(|item| item.predecessor == revision)
            .ok_or_else(repair)?;
        let control: LongTermMemoryControlRevision =
            parse(read(CONTROL, &binding.control_revision_physical_key)?.ok_or_else(repair)?)?;
        if control.validate_contract().is_err()
            || control.memory_space_id != space
            || control.factual_owner_id != space
            || control.transition.predecessor != revision
            || control.content_digest != binding.control_revision_content_digest
            || control.predecessor_material_digest != material.content_digest
        {
            return Err(repair());
        }
        let image = Image::Terminated {
            revision,
            content_digest,
            termination: control.transition.termination,
            control: binding.clone(),
        };
        if !image.validates_owner(owner) {
            return Err(repair());
        }
        return Ok(Some(image));
    }
    Ok(Some(Image::Present {
        revision,
        content_digest,
    }))
}

fn owner_identity(
    namespace: &str,
    value: &serde_json::Value,
) -> Result<Option<(String, GovernedMemoryOwnerRef)>> {
    match namespace {
        LT_HEAD => {
            let head: LongTermMemoryHeadManifest = parse(value.clone())?;
            Ok(Some((head.memory_space_id, head.owner_ref)))
        }
        DOCUMENT => {
            let document: GovernedEvidenceDocument = parse(value.clone())?;
            Ok(Some((
                document.memory_space_id,
                GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    document.document_id,
                ),
            )))
        }
        _ => Ok(None),
    }
}

pub(super) struct ReadPlan<'a> {
    platform: &'a StorePlatform,
    batch: Option<&'a StoreMutationBatch>,
    expected: Option<&'a [StoreJsonPrecondition]>,
    pub(super) observed: BTreeMap<Address, Option<serde_json::Value>>,
    capacity: StoreCapacityBudget,
    bytes: usize,
}

impl ReadPlan<'_> {
    pub(super) fn before(
        &mut self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>> {
        let address = (namespace.to_owned(), key.to_owned());
        if let Some(value) = self.observed.get(&address) {
            return Ok(value.clone());
        }
        if self.observed.len() >= self.capacity.kv_max_entries {
            return Err(crate::store_internal::store_budget_error(
                "source dependency read count exceeded",
            ));
        }
        let value = self
            .platform
            .read_json_docs_by_keys(namespace, &[key.to_owned()])?
            .pop()
            .map(|doc| doc.value);
        for expected in self.expected.into_iter().flatten() {
            let matches = match expected {
                StoreJsonPrecondition::Exact {
                    namespace: ns,
                    key: k,
                    value: expected,
                } if ns == namespace && k == key => value.as_ref() == Some(expected),
                StoreJsonPrecondition::Absent {
                    namespace: ns,
                    key: k,
                } if ns == namespace && k == key => value.is_none(),
                _ => true,
            };
            if !matches {
                return Err(Error::conflict(
                    "memory_write_transaction_precondition_failed",
                    "source pre-image changed before closure planning",
                ));
            }
        }
        if let Some(value) = &value {
            self.bytes = self
                .bytes
                .checked_add(serde_json::to_vec(value).map_err(|_| repair())?.len())
                .ok_or_else(repair)?;
            if self.bytes > self.capacity.snapshot_max_bytes {
                return Err(crate::store_internal::store_budget_error(
                    "source dependency read bytes exceeded",
                ));
            }
        }
        self.observed.insert(address, value.clone());
        Ok(value)
    }

    fn after(&mut self, namespace: &str, key: &str) -> Result<Option<serde_json::Value>> {
        for mutation in self.batch.into_iter().flat_map(|batch| &batch.mutations) {
            match mutation {
                StoreMutation::PutJson {
                    namespace: ns,
                    key: k,
                    value,
                    ..
                } if ns == namespace && k == key => return Ok(Some(value.clone())),
                StoreMutation::DeleteJson {
                    namespace: ns,
                    key: k,
                    ..
                } if ns == namespace && k == key => return Ok(None),
                _ => {}
            }
        }
        self.before(namespace, key)
    }
}

impl<'a> ReadPlan<'a> {
    pub(super) fn for_conditions(
        platform: &'a StorePlatform,
        expected: &'a [StoreJsonPrecondition],
        capacity: StoreCapacityBudget,
    ) -> Self {
        Self {
            platform,
            batch: None,
            expected: Some(expected),
            observed: BTreeMap::new(),
            capacity,
            bytes: 0,
        }
    }
}

pub(crate) fn append(
    platform: &StorePlatform,
    batch: &mut StoreMutationBatch,
    preconditions: &mut Vec<StoreJsonPrecondition>,
    operation: Option<&MemoryMutationOperationIdentity>,
    now: u64,
    capacity: StoreCapacityBudget,
) -> Result<()> {
    if batch.mutations.iter().any(|mutation| matches!(mutation, StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } if namespace == ROOT)) {
        return Err(Error::invalid_input("procedural_source_root_authority", "source reverse roots are derived only by the Store source owner"));
    }
    if !batch.mutations.iter().any(|mutation| {
        matches!(mutation,
        StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. }
        if matches!(namespace.as_str(), LT_HEAD | DOCUMENT | PRODUCER))
    }) {
        return Ok(());
    }
    let caller_conditions = preconditions.clone();
    let mut plan = ReadPlan {
        platform,
        batch: Some(batch),
        expected: Some(&caller_conditions),
        observed: BTreeMap::new(),
        capacity,
        bytes: 0,
    };
    let mut sources = BTreeSet::new();
    let mut producers = Vec::new();
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } => {
                if let Some(identity) = owner_identity(namespace, value)? {
                    sources.insert(identity);
                }
                if namespace == PRODUCER {
                    let producer: ProceduralProducerBindingV1 = parse(value.clone())?;
                    if let ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } =
                        &producer.spec.source_authority
                    {
                        sources.insert((
                            producer.spec.scope.memory_space_id.clone(),
                            source_revision.owner_ref.clone(),
                        ));
                        producers.push(producer);
                    }
                }
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if matches!(namespace.as_str(), LT_HEAD | DOCUMENT) =>
            {
                if let Some(value) = plan.before(namespace, key)? {
                    if let Some(identity) = owner_identity(namespace, &value)? {
                        sources.insert(identity);
                    }
                }
            }
            _ => {}
        }
    }
    let mut mutations = Vec::new();
    let mut affected: BTreeMap<(String, String), Vec<ProceduralSourceTransitionV1>> =
        BTreeMap::new();
    for (space, owner) in sources {
        if space != batch.scope.memory_space_id {
            return Err(repair());
        }
        let key = Root::key(&space, &owner)?;
        let before_root: Option<Root> = plan.before(ROOT, &key)?.map(parse).transpose()?;
        let before = source_image(&space, &owner, |ns, key| plan.before(ns, key))?;
        let after = source_image(&space, &owner, |ns, key| plan.after(ns, key))?;
        let mut root = match before_root.as_ref() {
            Some(root) => {
                root.validate()?;
                if root.current != before.clone().unwrap_or(Image::Deleted) {
                    return Err(repair());
                }
                if matches!(root.current, Image::Deleted) && after.is_some() {
                    return Err(Error::Other { stage: "governed_source_deleted_identity",
                        source: Box::new(bm_core::memory::GovernedSourceLifecycleError::DeletedIdentityCannotBeRecreated) });
                }
                root.advance(
                    after.clone().unwrap_or(Image::Deleted),
                    batch.transaction_id.clone(),
                    operation.cloned(),
                )?
            }
            None => {
                if before.is_some() {
                    return Err(repair());
                }
                Root::create(
                    space.clone(),
                    owner.clone(),
                    after.clone().ok_or_else(repair)?,
                    batch.transaction_id.clone(),
                    operation.cloned(),
                )?
            }
        };
        for producer in &producers {
            if matches!(&producer.spec.source_authority, ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } if source_revision.owner_ref == owner)
                && producer.spec.scope.memory_space_id == space
            {
                if root.dependents.len() >= bm_core::memory::MAX_PROCEDURAL_SOURCE_DEPENDENTS
                    && !root.dependents.iter().any(|item| {
                        producer
                            .spec
                            .binding_key()
                            .is_ok_and(|key| key == item.binding_key)
                    })
                {
                    return Err(crate::store_internal::store_budget_error(
                        "source dependent retention capacity is exhausted",
                    ));
                }
                root = root.bind(producer)?;
            }
        }
        if before != after && before.is_some() {
            let change = ProceduralSourceTransitionV1 {
                owner_ref: owner,
                before: before.ok_or_else(repair)?,
                after: after.unwrap_or(Image::Deleted),
            };
            let subjects = root
                .dependents
                .iter()
                .map(|item| (space.clone(), item.scope.mounted_subject_id.clone()))
                .collect::<BTreeSet<_>>();
            for subject in subjects {
                affected.entry(subject).or_default().push(change.clone());
            }
        }
        if before_root.as_ref() != Some(&root) {
            mutations.push(put_json(ROOT, &key, encode(&root, STAGE)?));
        }
    }
    for ((space, subject), mut changes) in affected {
        changes.sort_by(|a, b| a.owner_ref.cmp(&b.owner_ref));
        let key = ProceduralSubjectScopeV1 {
            memory_space_id: space,
            mounted_subject_id: subject,
        }
        .root_key()?;
        let before: ProceduralSubjectValidityRootV1 = parse(
            plan.before(PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE, &key)?
                .ok_or_else(repair)?,
        )?;
        let (after, jobs, conditions) = super::plan_subject_reconciliation(
            platform,
            &before,
            bm_core::memory::ProceduralReconciliationTriggerV1::SourceChange {
                transaction_id: batch.transaction_id.clone(),
                operation: operation.cloned(),
                changes,
            },
            now,
        )?;
        mutations.push(put_json(
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            &key,
            encode(&after, STAGE)?,
        ));
        mutations.extend(jobs);
        merge_conditions(preconditions, conditions)?;
    }
    merge_conditions(
        preconditions,
        plan.observed
            .into_iter()
            .map(|((namespace, key), value)| match value {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace,
                    key,
                    value,
                },
                None => StoreJsonPrecondition::Absent { namespace, key },
            }),
    )?;
    batch.mutations.extend(mutations);
    Ok(())
}

pub(crate) fn merge_conditions(
    target: &mut Vec<StoreJsonPrecondition>,
    values: impl IntoIterator<Item = StoreJsonPrecondition>,
) -> Result<()> {
    for condition in values {
        let address = |condition: &StoreJsonPrecondition| match condition {
            StoreJsonPrecondition::Exact { namespace, key, .. }
            | StoreJsonPrecondition::Absent { namespace, key } => (namespace.clone(), key.clone()),
        };
        if let Some(existing) = target
            .iter()
            .find(|item| address(item) == address(&condition))
        {
            if *existing != condition {
                return Err(Error::conflict(STAGE, "source read changed while planning"));
            }
        } else {
            target.push(condition);
        }
    }
    Ok(())
}

pub(crate) fn read_verified(
    platform: &StorePlatform,
    space: &str,
    owner: &GovernedMemoryOwnerRef,
    capacity: StoreCapacityBudget,
) -> Result<(Option<Root>, Vec<StoreJsonPrecondition>)> {
    let mut plan = ReadPlan {
        platform,
        batch: None,
        expected: None,
        observed: BTreeMap::new(),
        capacity,
        bytes: 0,
    };
    let root = verified_root(&mut plan, space, owner)?;
    let conditions = plan
        .observed
        .into_iter()
        .map(|((namespace, key), value)| match value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            },
            None => StoreJsonPrecondition::Absent { namespace, key },
        })
        .collect();
    Ok((root, conditions))
}

fn verified_root(
    plan: &mut ReadPlan<'_>,
    space: &str,
    owner: &GovernedMemoryOwnerRef,
) -> Result<Option<Root>> {
    let root: Option<Root> = plan
        .before(ROOT, &Root::key(space, owner)?)?
        .map(parse)
        .transpose()?;
    let current = source_image(space, owner, |ns, key| plan.before(ns, key))?;
    match &root {
        None if current.is_some() => return Err(repair()),
        Some(root) => {
            root.validate()?;
            if root.memory_space_id != space
                || root.owner_ref != *owner
                || root.current != current.unwrap_or(Image::Deleted)
            {
                return Err(repair());
            }
            if let Some(operation) = &root.last_change.operation {
                let receipt: MemoryMutationReceipt = parse(
                    plan.before(
                        bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE,
                        &operation.storage_key(),
                    )?
                    .ok_or_else(repair)?,
                )?;
                let audit: MemoryMutationAuditRecord = parse(
                    plan.before(
                        bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE,
                        &operation.storage_key(),
                    )?
                    .ok_or_else(repair)?,
                )?;
                super::validate_learning_mutation_proof(&receipt, &audit, STAGE)?;
                if receipt.identity != *operation
                    || receipt.transaction_id != root.last_change.transaction_id
                {
                    return Err(repair());
                }
            }
        }
        None => {}
    }
    Ok(root)
}

/// Exact-address expansion; reverse fanout is only followed for a root mutation,
/// never a public projection or a source read in another subject's recall.
pub(crate) fn dependencies(
    namespace: &str,
    value: &serde_json::Value,
    expand_dependents: bool,
) -> Result<Vec<Address>> {
    let mut addresses = Vec::new();
    if let Some((space, owner)) = owner_identity(namespace, value)? {
        addresses.push((ROOT.into(), Root::key(&space, &owner)?));
        if namespace == LT_HEAD {
            let head: LongTermMemoryHeadManifest = parse(value.clone())?;
            addresses.push((
                LT_MATERIAL.into(),
                long_term_version_material_key(&space, &space, &owner, head.current_revision)?,
            ));
            if head.terminal_transition_ref.is_some() {
                addresses.push((
                    LT_SCOPE.into(),
                    long_term_version_scope_manifest_key(&space, &space)?,
                ));
            }
        }
    }
    match namespace {
        ROOT => {
            let root: Root = parse(value.clone())?;
            root.validate()?;
            addresses.push(source_address(&root.memory_space_id, &root.owner_ref)?);
            if let Image::Terminated { control, .. } = &root.current {
                addresses.push((
                    CONTROL.into(),
                    control.control_revision_physical_key.clone(),
                ));
            }
            if let Some(operation) = &root.last_change.operation {
                addresses.push((
                    bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.into(),
                    operation.storage_key(),
                ));
                addresses.push((
                    bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.into(),
                    operation.storage_key(),
                ));
            }
            if expand_dependents {
                addresses.extend(
                    root.dependents
                        .iter()
                        .map(|item| (HEAD.into(), item.binding_key.clone())),
                );
            }
        }
        PRODUCER => {
            let producer: ProceduralProducerBindingV1 = parse(value.clone())?;
            if let ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } =
                &producer.spec.source_authority
            {
                addresses.push((
                    ROOT.into(),
                    Root::key(
                        &producer.spec.scope.memory_space_id,
                        &source_revision.owner_ref,
                    )?,
                ));
            }
        }
        LT_SCOPE => {
            let scope: LongTermMemoryVersionScopeManifest = parse(value.clone())?;
            addresses.extend(
                scope
                    .transition_bindings
                    .into_iter()
                    .map(|binding| (CONTROL.into(), binding.control_revision_physical_key)),
            );
        }
        _ => {}
    }
    Ok(addresses)
}

pub(crate) fn relevant(namespace: &str) -> bool {
    matches!(
        namespace,
        ROOT | PRODUCER | LT_HEAD | LT_MATERIAL | LT_SCOPE | DOCUMENT | CONTROL
    )
}

/// Tracks exact typed edges while the bounded Store dependency graph is read.
/// Cross-subject reads must be reachable from an actual same-space source root;
/// a caller's arbitrary list of subject IDs is never an admission capability.
pub(crate) struct SourceDependencyReadProof {
    actual: BTreeMap<Address, Option<serde_json::Value>>,
    pending_jobs: Json,
}

impl SourceDependencyReadProof {
    pub(crate) fn prepare(
        platform: &StorePlatform,
        batch: &StoreMutationBatch,
        conditions: &[StoreJsonPrecondition],
        capacity: StoreCapacityBudget,
    ) -> Result<Self> {
        let mut plan = ReadPlan {
            platform,
            batch: None,
            expected: Some(conditions),
            observed: BTreeMap::new(),
            capacity,
            bytes: 0,
        };
        let mut requests = BTreeMap::new();
        for condition in conditions {
            if let StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            } = condition
            {
                if namespace == ROOT {
                    let root: Root = parse(value.clone())?;
                    root.validate()?;
                    if root.physical_key != *key
                        || root.memory_space_id != batch.scope.memory_space_id
                    {
                        return Err(repair());
                    }
                    requests.insert(key.clone(), (root.owner_ref, false));
                }
            }
        }
        for mutation in &batch.mutations {
            if let StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } = mutation
            {
                if namespace == ROOT {
                    let root: Root = parse(value.clone())?;
                    // The proposed root supplies only its address. Before any
                    // dependent read, independently reload the durable image.
                    if Root::key(&batch.scope.memory_space_id, &root.owner_ref)? != *key {
                        return Err(repair());
                    }
                    requests.insert(key.clone(), (root.owner_ref, true));
                }
            }
        }
        let mut expand = BTreeSet::new();
        let mut pending = Vec::new();
        for (key, (owner, fanout)) in requests {
            let Some(root) = verified_root(&mut plan, &batch.scope.memory_space_id, &owner)? else {
                continue;
            };
            if fanout {
                // Verify the actual head/history/operation chain before an
                // index/job/ledger or learned body becomes discoverable.
                for dependent in &root.dependents {
                    let head: bm_core::memory::ProceduralProducerHeadV1 = parse(
                        plan.before(HEAD, &dependent.binding_key)?
                            .ok_or_else(repair)?,
                    )?;
                    if !head.validate_contract()
                        || head.scope != dependent.scope
                        || head.binding_key != dependent.binding_key
                    {
                        return Err(repair());
                    }
                    for reference in &head.retained_revisions {
                        let producer: ProceduralProducerBindingV1 = parse(
                            plan.before(PRODUCER, &reference.material_key())?
                                .ok_or_else(repair)?,
                        )?;
                        if !matches!(&producer.spec.source_authority, ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } if source_revision.owner_ref == root.owner_ref)
                            || producer.spec.scope != dependent.scope
                            || producer.revision_ref()? != *reference
                        {
                            return Err(repair());
                        }
                        let operation_key = producer.operation_identity.storage_key();
                        let receipt: MemoryMutationReceipt = parse(
                            plan.before(
                                bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE,
                                &operation_key,
                            )?
                            .ok_or_else(repair)?,
                        )?;
                        let audit: MemoryMutationAuditRecord = parse(
                            plan.before(
                                bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE,
                                &operation_key,
                            )?
                            .ok_or_else(repair)?,
                        )?;
                        super::validate_producer_operation_proof(&producer, &receipt, &audit)?;
                    }
                }
                let known = plan
                    .observed
                    .iter()
                    .filter_map(|(key, value)| value.clone().map(|value| (key.clone(), value)))
                    .collect();
                validate_root_dependents(&root, &known)?;
                expand.insert(key.clone());
            }
            pending.push(((ROOT.to_owned(), key), None::<ProceduralSubjectScopeV1>));
        }
        let mut visited = BTreeSet::new();
        while let Some((address, expected_subject)) = pending.pop() {
            let Some(value) = plan.before(&address.0, &address.1)? else {
                continue;
            };
            let node = super::procedural_dependency_node(
                &address.0,
                &value,
                address.0 == ROOT && expand.contains(&address.1),
            )?;
            if node
                .subject_scope
                .as_ref()
                .is_some_and(|scope| scope.memory_space_id != batch.scope.memory_space_id)
            {
                return Err(repair());
            }
            if let (Some(expected), Some(actual)) = (&expected_subject, &node.subject_scope) {
                if expected != actual {
                    return Err(repair());
                }
            }
            if !visited.insert(address.clone()) {
                continue;
            }
            let source_root = if address.0 == ROOT {
                Some(parse::<Root>(value)?)
            } else {
                None
            };
            for next in node.addresses {
                let expected = if let Some(root) = &source_root {
                    if next.0 == HEAD {
                        let dependent = root
                            .dependents
                            .iter()
                            .find(|item| item.binding_key == next.1)
                            .ok_or_else(repair)?;
                        Some(ProceduralSubjectScopeV1 {
                            memory_space_id: dependent.scope.memory_space_id.clone(),
                            mounted_subject_id: dependent.scope.mounted_subject_id.clone(),
                        })
                    } else {
                        None
                    }
                } else if next.0 == ROOT {
                    None
                } else {
                    node.subject_scope
                        .clone()
                        .or_else(|| expected_subject.clone())
                };
                pending.push((next, expected));
            }
        }
        let mut pending_jobs = BTreeMap::new();
        for mutation in &batch.mutations {
            let StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } = mutation
            else {
                continue;
            };
            if namespace != PROCEDURAL_FEEDBACK_JOB_NAMESPACE {
                continue;
            }
            let job: ProceduralFeedbackJobV2 = parse(value.clone())?;
            let bm_core::memory::ProceduralLearningWorkV1::Reconcile { source } = &job.work else {
                continue;
            };
            if !matches!(&source.trigger, bm_core::memory::ProceduralReconciliationTriggerV1::SourceChange { transaction_id, .. } if transaction_id == &batch.transaction_id)
                || job.status != ProceduralFeedbackJobStatusV1::Pending
            {
                continue;
            }
            let Some(Some(root_value)) = plan.observed.get(&(
                PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.into(),
                job.discovery_root_key.clone(),
            )) else {
                continue;
            };
            let root: ProceduralSubjectValidityRootV1 = parse(root_value.clone())?;
            if root.scope != source.scope
                || root.validity_epoch.checked_add(1) != Some(source.target_epoch)
                || job
                    != ProceduralFeedbackJobV2::pending_work(job.work.clone(), 5, job.created_at)?
                || key != &job.job_id
                || plan.before(namespace, key)?.is_some()
            {
                return Err(repair());
            }
            pending_jobs.insert((namespace.clone(), key.clone()), value.clone());
        }
        Ok(Self {
            actual: plan.observed,
            pending_jobs,
        })
    }

    pub(crate) fn actual_reads(&self) -> impl Iterator<Item = &Address> {
        self.actual.keys()
    }

    pub(crate) fn admit_node(
        &self,
        namespace: &str,
        key: &str,
        value: &serde_json::Value,
        scope: Option<&ProceduralSubjectScopeV1>,
        batch: &StoreMutationBatch,
        dependencies: &[Address],
    ) -> Result<()> {
        let Some(scope) = scope else {
            return Ok(());
        };
        if scope.memory_space_id != batch.scope.memory_space_id {
            return Err(repair());
        }
        if scope.mounted_subject_id == batch.scope.subject_id {
            return Ok(());
        }
        let actual = self
            .actual
            .get(&(namespace.to_owned(), key.to_owned()))
            .and_then(|value| value.as_ref());
        let fresh_reconciliation = self
            .pending_jobs
            .get(&(namespace.to_owned(), key.to_owned()))
            == Some(value);
        if actual.is_none() && !fresh_reconciliation {
            return Err(repair());
        }
        // A changed image cannot add arbitrary foreign read edges. Newly
        // planned work only reads its already-proven discovery root.
        if actual != Some(value)
            && dependencies.iter().any(|address| {
                !self.actual.contains_key(address) && !self.pending_jobs.contains_key(address)
            })
        {
            return Err(repair());
        }
        Ok(())
    }
}

fn root_in(json: &Json, space: &str, owner: &GovernedMemoryOwnerRef) -> Result<Root> {
    let key = Root::key(space, owner)?;
    let root: Root = parse(
        json.get(&(ROOT.into(), key.clone()))
            .cloned()
            .ok_or_else(repair)?,
    )?;
    root.validate()?;
    if root.physical_key != key || root.memory_space_id != space || root.owner_ref != *owner {
        return Err(repair());
    }
    Ok(root)
}

pub(crate) enum SourceValidationScope<'a> {
    StoreOpen,
    Transaction(&'a BTreeSet<Address>),
}

impl SourceValidationScope<'_> {
    fn requires(&self, address: &Address) -> bool {
        match self {
            Self::StoreOpen => true,
            Self::Transaction(changed) => changed.contains(address),
        }
    }
}

fn validate_root_dependents(root: &Root, json: &Json) -> Result<()> {
    for dependent in &root.dependents {
        let head: bm_core::memory::ProceduralProducerHeadV1 = parse(
            json.get(&(HEAD.into(), dependent.binding_key.clone()))
                .cloned()
                .ok_or_else(repair)?,
        )?;
        if !head.validate_contract()
            || head.scope != dependent.scope
            || head.binding_key != dependent.binding_key
        {
            return Err(repair());
        }
        let mut materials = Vec::new();
        for reference in &head.retained_revisions {
            let producer: ProceduralProducerBindingV1 = parse(
                json.get(&(PRODUCER.into(), reference.material_key()))
                    .cloned()
                    .ok_or_else(repair)?,
            )?;
            if producer.revision_ref()? != *reference
                || !matches!(&producer.spec.source_authority,
                ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } if source_revision.owner_ref == root.owner_ref)
            {
                return Err(repair());
            }
            let operation_key = producer.operation_identity.storage_key();
            let receipt: MemoryMutationReceipt = parse(
                json.get(&(
                    bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.into(),
                    operation_key.clone(),
                ))
                .cloned()
                .ok_or_else(repair)?,
            )?;
            let audit: MemoryMutationAuditRecord = parse(
                json.get(&(
                    bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.into(),
                    operation_key,
                ))
                .cloned()
                .ok_or_else(repair)?,
            )?;
            super::validate_producer_operation_proof(&producer, &receipt, &audit)?;
            materials.push(producer);
        }
        if !head.validates_materials(&materials) {
            return Err(repair());
        }
    }
    Ok(())
}

pub(crate) fn validate_image(json: &Json, scope: SourceValidationScope<'_>) -> Result<()> {
    for ((namespace, key), value) in json {
        if scope.requires(&(namespace.clone(), key.clone())) {
            if let Some((space, owner)) = owner_identity(namespace, value)? {
                root_in(json, &space, &owner)?;
            }
        }
        match namespace.as_str() {
            ROOT => {
                let root: Root = parse(value.clone())?;
                root.validate()?;
                if root.physical_key != *key
                    || root.current
                        != source_image(&root.memory_space_id, &root.owner_ref, |ns, key| {
                            Ok(json.get(&(ns.into(), key.into())).cloned())
                        })?
                        .unwrap_or(Image::Deleted)
                {
                    return Err(repair());
                }
                if let Some(operation) = &root.last_change.operation {
                    let receipt: MemoryMutationReceipt = parse(
                        json.get(&(
                            bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.into(),
                            operation.storage_key(),
                        ))
                        .cloned()
                        .ok_or_else(repair)?,
                    )?;
                    let audit: MemoryMutationAuditRecord = parse(
                        json.get(&(
                            bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.into(),
                            operation.storage_key(),
                        ))
                        .cloned()
                        .ok_or_else(repair)?,
                    )?;
                    super::validate_learning_mutation_proof(&receipt, &audit, STAGE)?;
                    if receipt.identity != *operation
                        || receipt.transaction_id != root.last_change.transaction_id
                    {
                        return Err(repair());
                    }
                }
                if scope.requires(&(namespace.clone(), key.clone())) {
                    validate_root_dependents(&root, json)?;
                }
            }
            PRODUCER => {
                let producer: ProceduralProducerBindingV1 = parse(value.clone())?;
                if let ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } =
                    &producer.spec.source_authority
                {
                    let root = root_in(
                        json,
                        &producer.spec.scope.memory_space_id,
                        &source_revision.owner_ref,
                    )?;
                    let key = producer.spec.binding_key()?;
                    if !root
                        .dependents
                        .iter()
                        .any(|item| item.binding_key == key && item.scope == producer.spec.scope)
                    {
                        return Err(repair());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Called under the backend transaction lock, even for primitive requests. It
/// admits only exact derived root changes, and returns the precisely proven
/// subject fences so the generic protected-owner gate need not trust a caller.
pub(crate) fn validate_transition(
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    transaction_id: &str,
    conditions: &[StoreJsonPrecondition],
) -> Result<BTreeSet<String>> {
    let changed = before
        .json
        .keys()
        .chain(after.json.keys())
        .filter(|key| before.json.get(*key) != after.json.get(*key))
        .cloned()
        .collect::<BTreeSet<_>>();
    if !changed.iter().any(|(ns, _)| relevant(ns)) {
        return Ok(BTreeSet::new());
    }
    let receipt = after
        .json
        .iter()
        .filter(|((ns, key), value)| {
            ns == bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE
                && before.json.get(&(ns.clone(), key.clone())) != Some(*value)
        })
        .map(|(_, value)| parse::<MemoryMutationReceipt>(value.clone()))
        .collect::<Result<Vec<_>>>()?;
    let operations = receipt
        .iter()
        .filter(|receipt| receipt.transaction_id == transaction_id)
        .collect::<Vec<_>>();
    if operations.len() > 1 {
        return Err(repair());
    }
    let operation = operations.first().map(|receipt| receipt.identity.clone());
    let mut affected: BTreeMap<(String, String), Vec<ProceduralSourceTransitionV1>> =
        BTreeMap::new();
    for (namespace, key) in &changed {
        for json in [&before.json, &after.json] {
            if let Some(value) = json.get(&(namespace.clone(), key.clone())) {
                if let Some((space, owner)) = owner_identity(namespace, value)? {
                    let root_key = Root::key(&space, &owner)?;
                    if !after.json.contains_key(&(ROOT.into(), root_key)) {
                        return Err(repair());
                    }
                }
            }
        }
        if namespace != ROOT {
            continue;
        }
        let next: Root = parse(
            after
                .json
                .get(&(namespace.clone(), key.clone()))
                .cloned()
                .ok_or_else(repair)?,
        )?;
        let previous: Option<Root> = before
            .json
            .get(&(namespace.clone(), key.clone()))
            .cloned()
            .map(parse)
            .transpose()?;
        let actual_before = source_image(&next.memory_space_id, &next.owner_ref, |ns, key| {
            Ok(before.json.get(&(ns.into(), key.into())).cloned())
        })?;
        let actual_after = source_image(&next.memory_space_id, &next.owner_ref, |ns, key| {
            Ok(after.json.get(&(ns.into(), key.into())).cloned())
        })?;
        let mut expected = match &previous {
            None => {
                if actual_before.is_some() {
                    return Err(repair());
                }
                Root::create(
                    next.memory_space_id.clone(),
                    next.owner_ref.clone(),
                    actual_after.clone().ok_or_else(repair)?,
                    transaction_id.into(),
                    operation.clone(),
                )?
            }
            Some(root) => {
                root.validate()?;
                if root.current != actual_before.clone().unwrap_or(Image::Deleted) {
                    return Err(repair());
                }
                root.advance(
                    actual_after.clone().unwrap_or(Image::Deleted),
                    transaction_id.into(),
                    operation.clone(),
                )?
            }
        };
        for (producer_ns, producer_key) in &changed {
            if producer_ns != PRODUCER {
                continue;
            }
            let Some(value) = after.json.get(&(producer_ns.clone(), producer_key.clone())) else {
                continue;
            };
            let producer: ProceduralProducerBindingV1 = parse(value.clone())?;
            if producer.spec.scope.memory_space_id == next.memory_space_id
                && matches!(&producer.spec.source_authority,
                ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision } if source_revision.owner_ref == next.owner_ref)
            {
                expected = expected.bind(&producer)?;
            }
        }
        if expected != next
            || !conditions.iter().any(|condition| {
                match (
                    condition,
                    before.json.get(&(namespace.clone(), key.clone())),
                ) {
                    (
                        StoreJsonPrecondition::Absent {
                            namespace: ns,
                            key: k,
                        },
                        None,
                    ) => ns == namespace && k == key,
                    (
                        StoreJsonPrecondition::Exact {
                            namespace: ns,
                            key: k,
                            value,
                        },
                        Some(old),
                    ) => ns == namespace && k == key && value == old,
                    _ => false,
                }
            })
        {
            return Err(repair());
        }
        if actual_before != actual_after && actual_before.is_some() {
            let change = ProceduralSourceTransitionV1 {
                owner_ref: next.owner_ref.clone(),
                before: actual_before.ok_or_else(repair)?,
                after: actual_after.unwrap_or(Image::Deleted),
            };
            for subject in next
                .dependents
                .iter()
                .map(|item| item.scope.mounted_subject_id.clone())
                .collect::<BTreeSet<_>>()
            {
                affected
                    .entry((next.memory_space_id.clone(), subject))
                    .or_default()
                    .push(change.clone());
            }
        }
    }
    validate_image(&after.json, SourceValidationScope::Transaction(&changed))?;
    let mut fences = BTreeSet::new();
    for ((space, subject), mut changes) in affected {
        changes.sort_by(|a, b| a.owner_ref.cmp(&b.owner_ref));
        let scope = ProceduralSubjectScopeV1 {
            memory_space_id: space,
            mounted_subject_id: subject,
        };
        let key = scope.root_key()?;
        let address = (
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.to_owned(),
            key.clone(),
        );
        let old: ProceduralSubjectValidityRootV1 =
            parse(before.json.get(&address).cloned().ok_or_else(repair)?)?;
        let next: ProceduralSubjectValidityRootV1 =
            parse(after.json.get(&address).cloned().ok_or_else(repair)?)?;
        let work = bm_core::memory::ProceduralReconciliationWorkV1 {
            scope,
            target_epoch: old.validity_epoch.checked_add(1).ok_or_else(repair)?,
            trigger: bm_core::memory::ProceduralReconciliationTriggerV1::SourceChange {
                transaction_id: transaction_id.into(),
                operation: operation.clone(),
                changes,
            },
        };
        if next != old.begin(&work, MAX_PROCEDURAL_SUBJECT_SCOPES)? {
            return Err(repair());
        }
        let job: ProceduralFeedbackJobV2 = parse(
            after
                .json
                .get(&(
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(),
                    work.reference()?.job_id,
                ))
                .cloned()
                .ok_or_else(repair)?,
        )?;
        if before
            .json
            .contains_key(&(PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(), job.job_id.clone()))
            || job
                != ProceduralFeedbackJobV2::pending_work(
                    bm_core::memory::ProceduralLearningWorkV1::Reconcile { source: work },
                    5,
                    job.created_at,
                )?
            || !conditions.iter().any(|condition| {
                matches!(condition, StoreJsonPrecondition::Exact { namespace, key: k, value }
                if namespace == &address.0 && k == &key && Some(value) == before.json.get(&address))
            })
        {
            return Err(repair());
        }
        fences.insert(key);
    }
    Ok(fences)
}
