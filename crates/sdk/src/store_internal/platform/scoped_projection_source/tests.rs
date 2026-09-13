use super::*;

#[test]
fn archive_source_closure_real_backends_fence_phantoms_and_derived_capacity() {
    let root = std::env::temp_dir().join(format!(
        "bm-archive-source-fences-{}-{}",
        std::process::id(),
        current_unix_nanos(),
    ));
    let profile = bm_core::feature_gate::ProfileId::native_dev_full().unwrap();
    let source = StorePlatform::open(StoreBackendConfig::in_memory(profile).unwrap()).unwrap();
    let memory = crate::MemoryRuntime::builder()
        .identity(crate::MemoryIdentity::new("archive-agent", "archive-owner").unwrap())
        .scope(crate::MemoryScope::new("sdk.direct", "archive-cas").unwrap())
        .store(crate::MemoryStoreHandle::from_platform(source.clone()))
        .build()
        .unwrap();
    let locator = "synthetic://archive/source";
    let group = canonical_recall_evidence_group(locator);
    let body = "Synthetic archive source with real persisted reverse root";
    memory
        .write(crate::MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![crate::MemoryEvidenceDocumentMutation::Upsert {
                draft: Box::new(GovernedEvidenceDocumentDraft {
                    memory_space_id: memory.memory_space_id().into(),
                    mounted_subject_id: memory.subject_id().into(),
                    document_id: "archive-source".into(),
                    source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
                    source_locator: locator.into(),
                    canonical_evidence_group: group.clone(),
                    evidence_family_group: None,
                    source_revision: 1,
                    content_digest: governed_evidence_document_content_digest(
                        locator,
                        &group,
                        None,
                        body,
                        &[],
                    ),
                    body: body.into(),
                    chunks: Vec::new(),
                    authority: MemoryEvidenceAuthority::WorldObservation,
                    privacy: MemoryPrivacyClass::PublicRuntime,
                    observed_at: current_unix_secs(),
                }),
            }],
        })
        .unwrap();
    let scope =
        StoreScopedProjectionScope::subject(memory.memory_space_id(), memory.subject_id()).unwrap();
    let (snapshot, _) = source
        .export_memory_space_projection_with_report(&scope, None)
        .unwrap();
    assert!(!snapshot.json_docs.is_empty());
    assert!(snapshot.json_docs.iter().all(|doc| doc.namespace
        != crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE));
    #[cfg(not(feature = "sqlite-store"))]
    let backends = ["memory", "embedded", "file"];
    #[cfg(feature = "sqlite-store")]
    let backends = ["memory", "embedded", "file", "sqlite"];
    for backend in backends {
        for case in ["capacity", "phantom", "success"] {
            let config = match backend {
                "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
                "embedded" => {
                    StoreBackendConfig::embedded(bm_core::feature_gate::ProfileId::EspEmbeddedSdk)
                        .unwrap()
                }
                "file" => StoreBackendConfig::file(root.join(format!("{backend}-{case}")), profile)
                    .unwrap(),
                "sqlite" => {
                    StoreBackendConfig::sqlite(root.join(format!("{backend}-{case}.db")), profile)
                        .unwrap()
                }
                _ => unreachable!(),
            };
            let target = StorePlatform::open(config.clone()).unwrap();
            let mut request = StoreScopedProjectionReplaceRequest {
                scope: scope.clone(),
                json_namespaces: store_memory_space_archive_json_namespaces()
                    .map(str::to_owned)
                    .collect(),
                json_docs: snapshot.json_docs.clone(),
                events: snapshot.events.clone(),
                preserve_protected_owner_state: true,
                source_closure: None,
            };
            let capacity = target.capacity();
            target
                .bind_scoped_source_closure(&mut request, capacity, current_unix_secs())
                .unwrap();
            assert!(!request
                .source_closure
                .as_ref()
                .unwrap()
                .derived_documents
                .is_empty());
            if case == "phantom" {
                let event = MemoryStoreEvent::new(
                    "archive-phantom",
                    MemoryStoreEventKind::MemoryMaintenance,
                    target
                        .config
                        .event_scope
                        .clone()
                        .with_memory_space(memory.memory_space_id())
                        .with_subject(memory.subject_id()),
                    current_unix_secs(),
                )
                .with_plane("archive_test")
                .with_record_key("phantom")
                .with_content_hash("synthetic");
                target.engine.append_event(event).unwrap();
            }
            let before = target.export_store_snapshot().unwrap();
            if case == "capacity" {
                let input_bytes = request
                    .json_docs
                    .iter()
                    .map(|doc| serde_json::to_vec(&doc.value).unwrap().len())
                    .sum::<usize>()
                    + request
                        .events
                        .iter()
                        .map(|event| serde_json::to_vec(event).unwrap().len())
                        .sum::<usize>();
                let mut constrained = capacity;
                constrained.snapshot_max_bytes = input_bytes;
                let error = target
                    .engine
                    .replace_scoped_projection_with_capacity(&request, constrained)
                    .unwrap_err();
                assert_eq!(
                    error.stage(),
                    "store_budget_exceeded",
                    "{backend}: {error:?}"
                );
            } else if case == "phantom" {
                let error = target
                    .engine
                    .replace_scoped_projection_with_capacity(&request, capacity)
                    .unwrap_err();
                assert!(
                    matches!(error, Error::Conflict { .. }),
                    "{backend}: {error:?}"
                );
            } else {
                target
                    .engine
                    .replace_scoped_projection_with_capacity(&request, capacity)
                    .unwrap();
                assert!(!target
                    .read_json_namespace(
                        crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE
                    )
                    .unwrap()
                    .is_empty());
            }
            if case != "success" {
                assert_eq!(
                    target.export_store_snapshot().unwrap(),
                    before,
                    "{backend}/{case}: partial archive write"
                );
            }
            if matches!(backend, "file" | "sqlite") {
                drop(target);
                let reopened = StorePlatform::open(config).unwrap();
                if case != "success" {
                    let after = reopened.export_store_snapshot().unwrap();
                    assert_eq!(after.json_docs, before.json_docs);
                    assert_eq!(after.blobs, before.blobs);
                    assert!(after.events.starts_with(&before.events));
                    for event in &after.events[before.events.len()..] {
                        assert_eq!(event.kind, MemoryStoreEventKind::RuntimeLifecycle);
                        assert_eq!(event.scope, StoreEventScope::system("open"));
                        assert_eq!(
                            event.payload.get("success").map(String::as_str),
                            Some("true")
                        );
                    }
                } else {
                    assert!(!reopened
                        .read_json_namespace(
                            crate::store_internal::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE
                        )
                        .unwrap()
                        .is_empty());
                }
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
