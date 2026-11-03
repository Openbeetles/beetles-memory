use super::*;

pub(crate) fn plan_governed_owner_facet_index_changes(
    store_platform: &dyn RuntimeKnownJsonRead,
    memory_space_id: &str,
    mut changes: BTreeMap<GovernedMemoryOwnerRef, Option<MemoryFacetIndexDoc>>,
    facet_scope_id: &str,
) -> Result<MemoryStoreMutationPlan> {
    if changes.is_empty() {
        return Ok(MemoryStoreMutationPlan::default());
    }
    let mut mutations = Vec::new();
    let mut preconditions = Vec::new();
    let facet_keys = changes
        .keys()
        .map(|owner_ref| {
            scoped_memory_facet_owner_storage_key(memory_space_id, facet_scope_id, owner_ref)
                .map_err(|error| Error::config("memory_facet_index_plan", error.to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    let previous_raw_docs =
        store_platform.read_known_json_docs(MEMORY_FACET_INDEX_NAMESPACE, &facet_keys)?;
    let previous_raw_by_key = previous_raw_docs
        .iter()
        .map(|doc| (doc.key.clone(), doc.value.clone()))
        .collect::<BTreeMap<_, _>>();
    for key in &facet_keys {
        preconditions.push(json_precondition(
            MEMORY_FACET_INDEX_NAMESPACE,
            key,
            previous_raw_by_key.get(key).cloned(),
        ));
    }
    for owner_ref in changes
        .keys()
        .filter(|owner_ref| owner_ref.owner_plane == GovernedMemoryOwnerPlane::LongTerm)
    {
        let head_key = long_term_version_head_key(memory_space_id, memory_space_id, owner_ref)?;
        let head_value = store_platform
            .read_known_json_docs(
                crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
                std::slice::from_ref(&head_key),
            )?
            .into_iter()
            .next()
            .map(|doc| doc.value);
        preconditions.push(json_precondition(
            crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
            &head_key,
            head_value.clone(),
        ));
        let Some(head_value) = head_value else {
            continue;
        };
        let head = serde_json::from_value::<LongTermMemoryHeadManifest>(head_value)
            .map_err(|error| Error::config("memory_facet_index_plan", error.to_string()))?;
        let material_key = long_term_version_material_key(
            memory_space_id,
            memory_space_id,
            owner_ref,
            head.current_revision,
        )?;
        let material_value = store_platform
            .read_known_json_docs(
                crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
                std::slice::from_ref(&material_key),
            )?
            .into_iter()
            .next()
            .map(|doc| doc.value);
        preconditions.push(json_precondition(
            crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
            &material_key,
            material_value,
        ));
    }
    let previous_docs = previous_raw_docs
        .into_iter()
        .map(|doc| {
            serde_json::from_value::<MemoryFacetIndexDoc>(doc.value)
                .map(|facet| (facet.owner_ref.clone(), facet))
                .map_err(|error| Error::config("memory_facet_posting_plan", error.to_string()))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    for (owner_ref, next) in &mut changes {
        if let Some(next) = next {
            next.facet_index_revision = previous_docs
                .get(owner_ref)
                .map(|previous| previous.facet_index_revision.saturating_add(1).max(1))
                .unwrap_or(1);
        }
    }
    mutations.clear();
    for (owner_ref, doc) in &changes {
        let key = scoped_memory_facet_owner_storage_key(memory_space_id, facet_scope_id, owner_ref)
            .map_err(|error| Error::config("memory_facet_index_plan", error.to_string()))?;
        match doc {
            Some(doc) => mutations.push(StoreMutation::PutJson {
                namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                key: key.clone(),
                value: serde_json::to_value(doc)
                    .map_err(|error| Error::config("memory_facet_index_plan", error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                record_key: key,
            }),
            None => mutations.push(StoreMutation::DeleteJson {
                namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                key: key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                record_key: key,
            }),
        }
    }
    let subject_id = facet_scope_id.trim();
    let manifest_key = memory_facet_manifest_key(memory_space_id, subject_id)
        .map_err(|error| Error::config("memory_facet_manifest_plan", error.to_string()))?;
    let manifest_raw = store_platform
        .read_known_json_docs(
            MEMORY_FACET_POSTING_NAMESPACE,
            std::slice::from_ref(&manifest_key),
        )?
        .into_iter()
        .next();
    preconditions.push(json_precondition(
        MEMORY_FACET_POSTING_NAMESPACE,
        &manifest_key,
        manifest_raw.as_ref().map(|doc| doc.value.clone()),
    ));
    let mut manifest = manifest_raw
        .map(|doc| {
            serde_json::from_value::<MemoryFacetIndexManifest>(doc.value)
                .map_err(|error| Error::config("memory_facet_manifest_plan", error.to_string()))
        })
        .transpose()?
        .unwrap_or(MemoryFacetIndexManifest {
            schema_version: bm_core::memory::MEMORY_FACET_SCHEMA_VERSION,
            memory_space_id: memory_space_id.to_string(),
            subject_id: subject_id.to_string(),
            owner_doc_count: 0,
            posting_doc_count: 0,
            revision: 0,
            owner_versions: Vec::new(),
            posting_revisions: Vec::new(),
        });
    if manifest.schema_version != bm_core::memory::MEMORY_FACET_SCHEMA_VERSION
        || manifest.memory_space_id != memory_space_id
        || manifest.subject_id != subject_id
    {
        return Err(Error::config(
            "memory_facet_manifest_plan",
            "memory_facet_manifest_contract_mismatch",
        ));
    }
    let previous_manifest_revision = manifest.revision;
    let posting_keys_for = |doc: &MemoryFacetIndexDoc| -> Result<Vec<String>> {
        if !doc.privacy.projection_content_allowed()
            || doc.status != MemoryFacetStatus::Active
            || !doc.subject_ids.iter().any(|subject| subject == subject_id)
        {
            return Ok(Vec::new());
        }
        doc.posting_keys_for_subject(subject_id)
            .map_err(|error| Error::config("memory_facet_posting_plan", error.to_string()))
    };
    let mut affected_posting_keys = BTreeSet::new();
    for (owner_ref, next) in &changes {
        if let Some(previous) = previous_docs.get(owner_ref) {
            affected_posting_keys.extend(posting_keys_for(previous)?);
        }
        if let Some(next) = next {
            affected_posting_keys.extend(posting_keys_for(next)?);
        }
    }
    let posting_keys = affected_posting_keys.into_iter().collect::<Vec<_>>();
    let posting_raw_docs =
        store_platform.read_known_json_docs(MEMORY_FACET_POSTING_NAMESPACE, &posting_keys)?;
    let posting_raw_by_key = posting_raw_docs
        .iter()
        .map(|doc| (doc.key.clone(), doc.value.clone()))
        .collect::<BTreeMap<_, _>>();
    for key in &posting_keys {
        preconditions.push(json_precondition(
            MEMORY_FACET_POSTING_NAMESPACE,
            key,
            posting_raw_by_key.get(key).cloned(),
        ));
    }
    struct PlanningFacetPosting {
        document: MemoryFacetPostingDoc,
        owner_versions: BTreeMap<GovernedMemoryOwnerRef, MemoryFacetOwnerVersion>,
    }
    let mut postings = posting_raw_docs
        .into_iter()
        .map(|doc| {
            serde_json::from_value::<MemoryFacetPostingDoc>(doc.value)
                .map_err(|error| Error::config("memory_facet_posting_plan", error.to_string()))
                .and_then(|posting| {
                    let owner_versions = MemoryRuntime::memory_facet_owner_version_map(
                        &posting.owner_versions,
                        "memory_facet_posting_plan",
                    )?;
                    Ok((
                        posting.posting_key.clone(),
                        PlanningFacetPosting {
                            document: posting,
                            owner_versions,
                        },
                    ))
                })
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    for posting_key in &posting_keys {
        postings
            .entry(posting_key.clone())
            .or_insert_with(|| PlanningFacetPosting {
                document: MemoryFacetPostingDoc {
                    schema_version: bm_core::memory::MEMORY_FACET_SCHEMA_VERSION,
                    memory_space_id: memory_space_id.to_string(),
                    subject_id: subject_id.to_string(),
                    posting_key: posting_key.clone(),
                    revision: 0,
                    owner_versions: Vec::new(),
                },
                owner_versions: BTreeMap::new(),
            });
    }
    for (owner_ref, next) in &changes {
        if let Some(previous) = previous_docs.get(owner_ref) {
            for posting_key in posting_keys_for(previous)? {
                if let Some(posting) = postings.get_mut(&posting_key) {
                    posting.owner_versions.remove(owner_ref);
                }
            }
        }
        if let Some(next) = next {
            for posting_key in posting_keys_for(next)? {
                if let Some(posting) = postings.get_mut(&posting_key) {
                    posting.owner_versions.insert(
                        owner_ref.clone(),
                        MemoryFacetOwnerVersion {
                            owner_ref: owner_ref.clone(),
                            owner_revision: next.owner_revision,
                            facet_index_revision: next.facet_index_revision,
                        },
                    );
                }
            }
        }
    }
    let mut manifest_owner_versions = MemoryRuntime::memory_facet_owner_version_map(
        &manifest.owner_versions,
        "memory_facet_manifest_plan",
    )?;
    for (owner_ref, next) in &changes {
        manifest_owner_versions.remove(owner_ref);
        if let Some(next) = next.as_ref().filter(|doc| {
            doc.privacy.projection_content_allowed()
                && doc.status == MemoryFacetStatus::Active
                && doc.subject_ids.iter().any(|subject| subject == subject_id)
        }) {
            manifest_owner_versions.insert(
                owner_ref.clone(),
                MemoryFacetOwnerVersion {
                    owner_ref: owner_ref.clone(),
                    owner_revision: next.owner_revision,
                    facet_index_revision: next.facet_index_revision,
                },
            );
        }
    }
    manifest.owner_versions = manifest_owner_versions.into_values().collect();
    manifest.revision = previous_manifest_revision.saturating_add(1).max(1);
    let mut manifest_posting_revisions =
        MemoryRuntime::memory_facet_posting_revision_map(&manifest.posting_revisions)?;
    for planning in postings.into_values() {
        let mut posting = planning.document;
        posting.owner_versions = planning.owner_versions.into_values().collect();
        let key = posting.posting_key.clone();
        manifest_posting_revisions.remove(&key);
        if posting.owner_versions.is_empty() {
            mutations.push(StoreMutation::DeleteJson {
                namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                key: key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                record_key: key,
            });
        } else {
            posting.revision = posting.revision.saturating_add(1).max(1);
            manifest_posting_revisions.insert(
                key.clone(),
                MemoryFacetPostingRevision {
                    posting_key: key.clone(),
                    revision: posting.revision,
                },
            );
            mutations.push(StoreMutation::PutJson {
                namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                key: key.clone(),
                value: serde_json::to_value(&posting).map_err(|error| {
                    Error::config("memory_facet_posting_plan", error.to_string())
                })?,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                record_key: key,
            });
        }
    }
    manifest.posting_revisions = manifest_posting_revisions.into_values().collect();
    manifest.owner_doc_count = manifest.owner_versions.len();
    manifest.posting_doc_count = manifest.posting_revisions.len();
    if manifest.owner_versions.is_empty() || manifest.posting_revisions.is_empty() {
        mutations.push(StoreMutation::DeleteJson {
            namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            key: manifest_key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            record_key: manifest_key,
        });
    } else {
        validate_memory_facet_manifest(memory_space_id, subject_id, &manifest)
            .map_err(|error| Error::config("memory_facet_manifest_plan", error.to_string()))?;
        mutations.push(StoreMutation::PutJson {
            namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            key: manifest_key.clone(),
            value: serde_json::to_value(&manifest)
                .map_err(|error| Error::config("memory_facet_manifest_plan", error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
            record_key: manifest_key,
        });
    }
    for owner_ref in manifest
        .owner_versions
        .iter()
        .map(|owner| &owner.owner_ref)
        .filter(|owner_ref| !changes.contains_key(*owner_ref))
    {
        let facet_key =
            scoped_memory_facet_owner_storage_key(memory_space_id, facet_scope_id, owner_ref)
                .map_err(|error| Error::config("memory_facet_index_plan", error.to_string()))?;
        let facet_value = store_platform
            .read_known_json_docs(
                MEMORY_FACET_INDEX_NAMESPACE,
                std::slice::from_ref(&facet_key),
            )?
            .into_iter()
            .next()
            .map(|doc| doc.value);
        preconditions.push(json_precondition(
            MEMORY_FACET_INDEX_NAMESPACE,
            &facet_key,
            facet_value,
        ));
        if owner_ref.owner_plane != GovernedMemoryOwnerPlane::LongTerm {
            continue;
        }
        let head_key = long_term_version_head_key(memory_space_id, memory_space_id, owner_ref)?;
        let head_value = store_platform
            .read_known_json_docs(
                crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
                std::slice::from_ref(&head_key),
            )?
            .into_iter()
            .next()
            .map(|doc| doc.value);
        preconditions.push(json_precondition(
            crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
            &head_key,
            head_value.clone(),
        ));
        let Some(head_value) = head_value else {
            continue;
        };
        let head = serde_json::from_value::<LongTermMemoryHeadManifest>(head_value)
            .map_err(|error| Error::config("memory_facet_index_plan", error.to_string()))?;
        let material_key = long_term_version_material_key(
            memory_space_id,
            memory_space_id,
            owner_ref,
            head.current_revision,
        )?;
        let material_value = store_platform
            .read_known_json_docs(
                crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
                std::slice::from_ref(&material_key),
            )?
            .into_iter()
            .next()
            .map(|doc| doc.value);
        preconditions.push(json_precondition(
            crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
            &material_key,
            material_value,
        ));
    }
    let mut normalized_preconditions = Vec::new();
    merge_json_preconditions(&mut normalized_preconditions, preconditions)?;
    Ok(MemoryStoreMutationPlan {
        mutations,
        preconditions: normalized_preconditions,
        blob_preconditions: Vec::new(),
    })
}

pub(crate) fn project_long_term_evidence(
    reader: &dyn RuntimeKnownJsonRead,
    entry: &LongTermMemoryEntry,
    changed: &[(TranscriptTurnRecord, TranscriptTurnRecord)],
) -> Result<LongTermMemoryEntry> {
    let mut available = BTreeSet::new();
    for source in &entry.transcript_sources {
        let Some(subject) = source.subject_id.as_deref() else {
            continue;
        };
        let key = ConversationKey::new(
            &source.memory_space_id,
            &source.channel_id,
            &source.conversation_id,
        )?;
        let physical_key =
            crate::store_internal::transcript_turn_storage_key(&key, subject, &source.turn_id);
        let overlay = changed.iter().find(|(_, after)| {
            after.key == key && after.subject == subject && after.turn_id == source.turn_id
        });
        let record = match overlay {
            Some((_, after)) => Some(after.clone()),
            None => reader
                .read_known_json_docs("conversation_transcript", &[physical_key])?
                .into_iter()
                .next()
                .map(|doc| serde_json::from_value::<TranscriptTurnRecord>(doc.value))
                .transpose()
                .map_err(|error| Error::config("long_term_source_evidence", error.to_string()))?,
        };
        if record
            .as_ref()
            .is_some_and(|record| source.is_available_in(record))
        {
            available.insert(source.display_citation());
        }
    }
    Ok(entry.project_transcript_evidence(|source| available.contains(&source.display_citation())))
}

/// Composes factual evidence indexes into the existing Transcript lifecycle transaction.
/// The accepted factual material is deliberately not rewritten.
pub(crate) fn plan_transcript_factual_source_withdrawal(
    store: &StorePlatform,
    space: &str,
    changed: &[(TranscriptTurnRecord, TranscriptTurnRecord)],
    budget: &RuntimeBudgetReport,
) -> Result<MemoryStoreMutationPlan> {
    let withdrawn = changed
        .iter()
        .filter(|(before, after)| {
            before.redaction_state == bm_core::memory::TranscriptRedactionState::RawAvailable
                && after.redaction_state != bm_core::memory::TranscriptRedactionState::RawAvailable
        })
        .collect::<Vec<_>>();
    if withdrawn.is_empty() {
        return Ok(MemoryStoreMutationPlan::default());
    }
    let owners = store.memory_space_long_term_memory_read_store(space)?;
    let entries = owners.list(usize::MAX)?;
    if entries.len() > budget.governed_state_budget.max_validity_joins {
        return Err(Error::config(
            "transcript_factual_source_budget",
            "factual owner withdrawal exceeds the pinned join budget",
        ));
    }
    let mut changes = BTreeMap::new();
    let mut rejected_by_owner = BTreeMap::new();
    for entry in entries {
        let affected = entry.transcript_sources.iter().any(|source| {
            withdrawn.iter().any(|(_, after)| {
                source.memory_space_id == after.key.memory_space_id
                    && source.channel_id == after.key.channel_id
                    && source.conversation_id == after.key.conversation_id
                    && source.subject_id.as_deref() == Some(after.subject.as_str())
                    && source.turn_id == after.turn_id
            })
        });
        if !affected {
            continue;
        }
        let projected = project_long_term_evidence(store, &entry, changed)?;
        let rejected = entry
            .transcript_sources
            .iter()
            .filter(|source| !projected.transcript_sources.contains(source))
            .map(TranscriptEvidenceRef::display_citation)
            .collect::<BTreeSet<_>>();
        rejected_by_owner.insert(
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, entry.id.clone()),
            rejected,
        );
        let facet = build_long_term_memory_facet_index_doc(
            &projected,
            space.to_string(),
            vec![space.to_string()],
            1,
        )
        .map_err(|error| Error::config("transcript_factual_source_facet", error.to_string()))?;
        changes.insert(
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, entry.id),
            Some(facet),
        );
    }
    let mut plan = plan_governed_owner_facet_index_changes(store, space, changes, space)?;
    if rejected_by_owner.is_empty() {
        return Ok(plan);
    }
    let manifests = store.read_json_namespace(MEMORY_GRAPH_MANIFEST_NAMESPACE)?;
    if manifests.len() > budget.governed_state_budget.max_validity_joins {
        return Err(Error::config(
            "transcript_factual_source_budget",
            "graph scopes exceed the pinned join budget",
        ));
    }
    for document in manifests {
        let manifest: MemoryGraphScopeManifest = serde_json::from_value(document.value)
            .map_err(|error| Error::config("transcript_factual_source_graph", error.to_string()))?;
        if manifest.memory_space_id != space {
            continue;
        }
        let closure = load_memory_graph_closure(
            store,
            &manifest,
            &manifest.recall_indexes,
            Some(GraphClosureLoadLimits {
                max_nodes: budget.graph_expansion_budget.max_graph_nodes_loaded,
                max_edges: budget.graph_expansion_budget.max_graph_edges_loaded,
                max_backlinks: budget.graph_expansion_budget.max_backlinks_loaded,
            }),
        )
        .map_err(|failures| Error::config("transcript_factual_source_graph", failures.join(",")))?;
        let mut nodes = closure.nodes.clone();
        let rejected_by_node = closure
            .node_memberships
            .iter()
            .filter_map(|membership| {
                rejected_by_owner
                    .get(&membership.owner_ref)
                    .map(|denied| (membership.node_id.clone(), denied))
            })
            .collect::<BTreeMap<_, _>>();
        for node in &mut nodes {
            if let Some(denied) = rejected_by_node.get(&node.node_id) {
                node.evidence_refs
                    .retain(|reference| !denied.contains(reference));
            }
        }
        nodes.retain(|node| !node.evidence_refs.is_empty());
        let ids = nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<BTreeSet<_>>();
        let mut edges = closure.edges.clone();
        for edge in &mut edges {
            edge.evidence_refs.retain(|reference| {
                ![&edge.from_node_id, &edge.to_node_id].iter().any(|node| {
                    rejected_by_node
                        .get(*node)
                        .is_some_and(|denied| denied.contains(reference))
                })
            });
        }
        edges.retain(|edge| {
            !edge.evidence_refs.is_empty()
                && ids.contains(edge.from_node_id.as_str())
                && ids.contains(edge.to_node_id.as_str())
        });
        let evidence = nodes
            .iter()
            .flat_map(|node| &node.evidence_refs)
            .chain(edges.iter().flat_map(|edge| &edge.evidence_refs))
            .collect::<BTreeSet<_>>();
        let backlinks = closure
            .backlinks
            .iter()
            .filter(|backlink| evidence.contains(&backlink.source_id))
            .cloned()
            .collect::<Vec<_>>();
        if nodes == closure.nodes && edges == closure.edges && backlinks == closure.backlinks {
            continue;
        }
        merge_json_preconditions(
            &mut plan.preconditions,
            memory_graph_closure_preconditions(
                Some(&closure),
                space,
                &manifest.mounted_subject_id,
            )?,
        )?;
        if nodes.is_empty() {
            plan.mutations
                .extend(memory_graph_closure_delete_mutations(&closure));
            continue;
        }
        let bindings = closure
            .node_memberships
            .iter()
            .filter(|member| ids.contains(member.node_id.as_str()))
            .map(|member| MemoryGraphOwnerBinding {
                node_id: member.node_id.clone(),
                owner_ref: member.owner_ref.clone(),
                owner_revision: member.owner_revision,
                visible: true,
            })
            .collect();
        let graph = plan_temporal_memory_graph_write(
            "transcript.source_withdrawal",
            nodes,
            edges,
            backlinks,
        );
        if !graph.accepted {
            return Err(Error::config(
                "transcript_factual_source_graph",
                graph.gate_failures.join(","),
            ));
        }
        let persistence = build_memory_graph_persistence_plan(
            space.to_string(),
            manifest.mounted_subject_id.clone(),
            checked_next_persisted_revision(manifest.manifest_generation, "graph_generation")?,
            graph.nodes.clone(),
            graph.edges.clone(),
            graph.backlinks.clone(),
            bindings,
        );
        if !persistence.accepted {
            return Err(Error::config(
                "transcript_factual_source_graph",
                persistence.failures.join(","),
            ));
        }
        plan.mutations.extend(
            temporal_memory_graph_plan_mutations(&graph, &persistence, Some(&closure))?.mutations,
        );
    }
    Ok(plan)
}
