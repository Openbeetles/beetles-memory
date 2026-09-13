//! Scoped archive replacement consumes the existing source-dependency owner.
//! The closure is ephemeral Store planning data, never portable authority.
use super::*;
use crate::store_internal::procedural_feedback::source_dependents;
use crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE;
use crate::store_internal::transaction::{
    read_scoped_projection_from_parts, BackendTransactionState,
};
use crate::store_internal::{StoreCapacityBudget, StoreScopedProjection};
use crate::StorePhysicalOwningScope;

type Address = (String, String);
const STAGE: &str = "store_scoped_source_closure";

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ScopedSourceClosure {
    expected_scope_digest: String,
    replacement_digest: String,
    batch: StoreMutationBatch,
    preconditions: Vec<StoreJsonPrecondition>,
    derived_documents: Vec<StoreSnapshotJsonDoc>,
    derived_events: Vec<MemoryStoreEvent>,
}

impl ScopedSourceClosure {
    pub(crate) fn derived_documents(&self) -> &[StoreSnapshotJsonDoc] {
        &self.derived_documents
    }

    pub(crate) fn derived_events(&self) -> &[MemoryStoreEvent] {
        &self.derived_events
    }
}

pub(crate) struct PreparedSourceClosure {
    pub(crate) transaction_id: String,
    pub(crate) documents: Vec<StoreSnapshotJsonDoc>,
    pub(crate) before: BTreeMap<Address, Option<serde_json::Value>>,
    pub(crate) events: Vec<MemoryStoreEvent>,
}

impl PreparedSourceClosure {
    pub(crate) fn added_entries(&self) -> usize {
        self.before.values().filter(|value| value.is_none()).count()
    }
}

pub(crate) fn transaction_id(scope: &StoreScopedProjectionScope) -> String {
    let owner = match &scope.physical_owning_scope {
        StorePhysicalOwningScope::Subject { mounted_subject_id } => {
            format!("subject_{mounted_subject_id}")
        }
        StorePhysicalOwningScope::SharedProgram => "shared_program".into(),
    };
    format!("scoped_projection_{}_{}", scope.memory_space_id, owner)
}

fn replacement_digest(request: &StoreScopedProjectionReplaceRequest) -> Result<String> {
    stable_hash_json(&serde_json::json!([
        request.scope,
        request.json_namespaces,
        request.json_docs,
        request.events,
        request.preserve_protected_owner_state
    ]))
}

fn source_image_namespace(namespace: &str) -> bool {
    source_dependents::relevant(namespace)
        || matches!(
            namespace,
            MEMORY_MUTATION_RECEIPT_NAMESPACE
                | MEMORY_MUTATION_AUDIT_NAMESPACE
                | "conversation_transcript"
        )
}

fn conflict() -> Error {
    Error::conflict(
        STAGE,
        "scoped replacement pre-image changed during planning",
    )
}

impl StorePlatform {
    pub(super) fn bind_scoped_source_closure(
        &self,
        request: &mut StoreScopedProjectionReplaceRequest,
        capacity: StoreCapacityBudget,
        now: u64,
    ) -> Result<()> {
        let observed = self.engine.read_scoped_projection(
            &StoreScopedProjectionRequest {
                scope: request.scope.clone(),
                json_namespaces: request.json_namespaces.clone(),
                include_events: true,
            },
            capacity,
        )?;
        preserve_local_transcript_sources(request, &observed, &self.schema_manifest)?;
        let mut before = BTreeMap::new();
        for document in &observed.json_docs {
            if source_image_namespace(&document.namespace)
                && !crate::store_internal::engine::json_document_is_protected_owner(
                    &document.namespace,
                    &document.value,
                )?
            {
                before.insert(
                    (document.namespace.clone(), document.key.clone()),
                    document.value.clone(),
                );
            }
        }
        let incoming = request
            .json_docs
            .iter()
            .filter(|document| source_image_namespace(&document.namespace))
            .map(|document| {
                (
                    (document.namespace.clone(), document.key.clone()),
                    document.value.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut scope = self.config.event_scope.clone();
        scope.memory_space_id = request.scope.memory_space_id.clone();
        scope.physical_owning_scope = request.scope.physical_owning_scope.clone();
        if let StorePhysicalOwningScope::Subject { mounted_subject_id } =
            &scope.physical_owning_scope
        {
            scope.subject_id = mounted_subject_id.clone();
        }
        let mut batch = StoreMutationBatch {
            transaction_id: transaction_id(&request.scope),
            operation: "post_turn.procedural.source_archive_replace".into(),
            scope,
            mutations: Vec::new(),
        };
        let mut preconditions = Vec::new();
        // The actual mounted subject root discovers all retained local learning
        // dependencies, including RuntimeObservation producers without a fact
        // source. Incoming archive rows cannot mint this read authority.
        for document in &observed.json_docs {
            if document.namespace
                == crate::store_internal::schema::PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE
            {
                preconditions.push(StoreJsonPrecondition::Exact {
                    namespace: document.namespace.clone(),
                    key: document.key.clone(),
                    value: document.value.clone(),
                });
            }
        }
        for (namespace, key) in before
            .keys()
            .chain(incoming.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
        {
            let previous = before.get(&(namespace.clone(), key.clone()));
            let next = incoming.get(&(namespace.clone(), key.clone()));
            preconditions.push(match previous {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: value.clone(),
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: namespace.clone(),
                    key: key.clone(),
                },
            });
            // Include unchanged source heads so the source owner validates their
            // retained reverse root, while its reducer emits no epoch change.
            batch.mutations.push(match next {
                Some(value) => StoreMutation::PutJson {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: value.clone(),
                    event_kind: MemoryStoreEventKind::MemoryMaintenance,
                    plane: namespace,
                    record_key: key,
                },
                None => StoreMutation::DeleteJson {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    event_kind: MemoryStoreEventKind::MemoryMaintenance,
                    plane: namespace,
                    record_key: key,
                },
            });
        }
        let original_len = batch.mutations.len();
        source_dependents::append(self, &mut batch, &mut preconditions, None, now, capacity)?;
        let reads =
            self.procedural_transaction_dependency_json_reads(&batch, &preconditions, capacity)?;
        let keys = reads
            .into_iter()
            .chain(preconditions.iter().map(|condition| match condition {
                StoreJsonPrecondition::Exact { namespace, key, .. }
                | StoreJsonPrecondition::Absent { namespace, key } => {
                    (namespace.clone(), key.clone())
                }
            }))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let exact = self
            .engine
            .read_consistent_known_keys(&keys, &[], false, capacity)?;
        source_dependents::merge_conditions(
            &mut preconditions,
            exact.json.into_iter().map(|read| match read.value {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: read.namespace,
                    key: read.key,
                    value,
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: read.namespace,
                    key: read.key,
                },
            }),
        )?;
        let mut derived_documents = Vec::new();
        let mut derived_events = Vec::new();
        for mutation in batch.mutations.iter().skip(original_len) {
            let StoreMutation::PutJson {
                namespace,
                key,
                value,
                event_kind,
                plane,
                record_key,
            } = mutation
            else {
                return Err(Error::config(
                    STAGE,
                    "source owner produced an unsupported archive effect",
                ));
            };
            derived_documents.push(StoreSnapshotJsonDoc {
                namespace: namespace.clone(),
                key: key.clone(),
                value: value.clone(),
            });
            derived_events.push(self.build_batch_event(
                &batch,
                now,
                event_kind.clone(),
                plane,
                record_key,
                stable_hash_json(value)?,
            ));
        }
        request.source_closure = Some(Box::new(ScopedSourceClosure {
            expected_scope_digest: observed.receipt.state_digest,
            replacement_digest: replacement_digest(request)?,
            batch,
            preconditions,
            derived_documents,
            derived_events,
        }));
        Ok(())
    }
}

/// Called under each backend's original replacement lock/transaction, before
/// writes. Reads only exact Store-proven dependencies and checks the same source
/// successor contract as ordinary mutation batches.
pub(crate) fn prepare(
    request: &StoreScopedProjectionReplaceRequest,
    actual: &StoreScopedProjection,
    capacity: StoreCapacityBudget,
    mut read: impl FnMut(&str, &str) -> Result<Option<serde_json::Value>>,
) -> Result<PreparedSourceClosure> {
    let Some(closure) = &request.source_closure else {
        if actual
            .json_docs
            .iter()
            .chain(&request.json_docs)
            .any(|doc| {
                matches!(
                    doc.namespace.as_str(),
                    LONG_TERM_HEAD_MANIFEST_NAMESPACE
                        | GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
                        | "conversation_transcript"
                )
            })
        {
            return Err(Error::config(
                STAGE,
                "governed source replacement requires its Store-planned closure",
            ));
        }
        return Ok(PreparedSourceClosure {
            transaction_id: transaction_id(&request.scope),
            documents: Vec::new(),
            before: BTreeMap::new(),
            events: Vec::new(),
        });
    };
    if actual.scope != request.scope
        || actual.receipt.state_digest != closure.expected_scope_digest
        || replacement_digest(request)? != closure.replacement_digest
        || closure.batch.transaction_id != transaction_id(&request.scope)
    {
        return Err(conflict());
    }
    if closure.preconditions.len() > capacity.kv_max_entries {
        return Err(store_budget_error(
            "scoped source read-set exceeds entry budget",
        ));
    }
    let mut before = BackendTransactionState::default();
    let mut bytes = 0usize;
    let mut seen = BTreeSet::new();
    for condition in &closure.preconditions {
        let (namespace, key, expected) = match condition {
            StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            } => (namespace, key, Some(value)),
            StoreJsonPrecondition::Absent { namespace, key } => (namespace, key, None),
        };
        if !seen.insert((namespace.clone(), key.clone())) {
            return Err(conflict());
        }
        let observed = read(namespace, key)?;
        if observed.as_ref() != expected {
            return Err(conflict());
        }
        if let Some(value) = observed {
            admit_store_json_document(namespace, key, &value, STAGE)?;
            bytes = bytes
                .checked_add(
                    serde_json::to_vec(&value)
                        .map_err(|error| Error::config(STAGE, error.to_string()))?
                        .len(),
                )
                .ok_or_else(|| store_budget_error("scoped source read size overflow"))?;
            if bytes > capacity.snapshot_max_bytes {
                return Err(store_budget_error(
                    "scoped source read-set exceeds byte budget",
                ));
            }
            before.json.insert((namespace.clone(), key.clone()), value);
        }
    }
    let mut after = before.clone();
    for mutation in &closure.batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } => {
                after
                    .json
                    .insert((namespace.clone(), key.clone()), value.clone());
            }
            StoreMutation::DeleteJson { namespace, key, .. } => {
                after.json.remove(&(namespace.clone(), key.clone()));
            }
            _ => {
                return Err(Error::config(
                    STAGE,
                    "archive source closure contains a non-JSON mutation",
                ))
            }
        }
    }
    source_dependents::validate_transition(
        &before,
        &after,
        &closure.batch.transaction_id,
        &closure.preconditions,
    )?;
    let changed = before
        .json
        .keys()
        .chain(after.json.keys())
        .filter(|key| before.json.get(*key) != after.json.get(*key))
        .cloned()
        .collect();
    source_dependents::validate_image(
        &after.json,
        source_dependents::SourceValidationScope::Transaction(&changed),
    )?;
    crate::store_internal::procedural_feedback::validate_procedural_feedback_store_image(
        &after, STAGE,
    )?;
    let derived_before = closure
        .derived_documents
        .iter()
        .map(|doc| {
            let key = (doc.namespace.clone(), doc.key.clone());
            let previous = before.json.get(&key).cloned();
            (key, previous)
        })
        .collect();
    Ok(PreparedSourceClosure {
        transaction_id: closure.batch.transaction_id.clone(),
        documents: closure.derived_documents.clone(),
        before: derived_before,
        events: closure.derived_events.clone(),
    })
}

fn preserve_local_transcript_sources(
    request: &mut StoreScopedProjectionReplaceRequest,
    observed: &StoreScopedProjection,
    schema: &crate::store_internal::StoreSchemaManifest,
) -> Result<()> {
    let mut retained = BTreeSet::new();
    for document in &observed.json_docs {
        if document.namespace != PROCEDURAL_FEEDBACK_JOB_NAMESPACE {
            continue;
        }
        let job: ProceduralFeedbackJobV2 = serde_json::from_value(document.value.clone())
            .map_err(|_| Error::config(STAGE, "invalid retained procedural job"))?;
        job.validate()?;
        if let ProceduralLearningWorkV1::Feedback { source } = &job.work {
            let identity = &source.identity;
            let key = ConversationKey::new(
                &identity.memory_space_id,
                &identity.channel_id,
                &identity.conversation_id,
            )?;
            retained.insert(transcript_turn_storage_key(
                &key,
                &identity.mounted_subject_id,
                &identity.turn_id,
            ));
        }
    }
    let mut changed = false;
    for document in &observed.json_docs {
        if document.namespace != "conversation_transcript" {
            continue;
        }
        let actual: TranscriptTurnRecord = serde_json::from_value(document.value.clone())
            .map_err(|_| Error::config(STAGE, "invalid actual transcript source"))?;
        actual.validate_canonical_intake()?;
        let incoming = request.json_docs.iter_mut().find(|candidate| {
            candidate.namespace == document.namespace && candidate.key == document.key
        });
        if let Some(incoming) = incoming {
            let projected: TranscriptTurnRecord = serde_json::from_value(incoming.value.clone())
                .map_err(|_| Error::config(STAGE, "invalid incoming transcript projection"))?;
            if projected != crate::public_procedural_transcript_projection(&actual) {
                return Err(Error::Other {
                    stage: "memory_space_import",
                    source: Box::new(crate::MemorySpaceImportConflict::ExistingTranscriptDiffers),
                });
            }
            if incoming.value != document.value {
                incoming.value = document.value.clone();
                changed = true;
            }
        } else if actual.learning_evidence.is_some()
            || actual.lifecycle_state == TranscriptLifecycleState::RawDeleted
            || retained.contains(&document.key)
        {
            return Err(Error::Other {
                stage: "memory_space_import",
                source: Box::new(
                    crate::MemorySpaceImportConflict::ProtectedTranscriptSourceMissing,
                ),
            });
        }
    }
    if changed {
        let mut snapshot = StoreSnapshot::new(
            schema.clone(),
            request.json_docs.clone(),
            Vec::new(),
            request.events.clone(),
        );
        crate::rebuild_disclosed_recall_manifest_closure(&mut snapshot)?;
        request.json_docs = snapshot.json_docs;
    }
    Ok(())
}

pub(crate) fn actual_projection(
    request: &StoreScopedProjectionReplaceRequest,
    capacity: StoreCapacityBudget,
    json: &BTreeMap<Address, serde_json::Value>,
    events: &[MemoryStoreEvent],
) -> Result<StoreScopedProjection> {
    read_scoped_projection_from_parts(
        &StoreScopedProjectionRequest {
            scope: request.scope.clone(),
            json_namespaces: request.json_namespaces.clone(),
            include_events: true,
        },
        capacity,
        json,
        events,
    )
}

#[cfg(all(test, feature = "nonproduction-replay-harness"))]
mod tests;
