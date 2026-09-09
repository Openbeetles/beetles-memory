use bm_core::memory::{
    CanonicalEntityKey, CanonicalEntityKind, CanonicalEntityRef, CanonicalEvidenceRef,
};
use bm_sdk::*;
use serde_json::Value;

fn entity() -> CanonicalEntityRef {
    CanonicalEntityRef {
        key: CanonicalEntityKey {
            kind: CanonicalEntityKind::Project,
            canonical_id: "wire-project".into(),
        },
        display_label: Some("Wire project".into()),
        aliases: vec![],
        evidence_refs: vec![CanonicalEvidenceRef {
            source_ref: "fixture:wire".into(),
            canonical_evidence_group: "wire-evidence".into(),
            evidence_family_group: None,
            source_kind: "structured_material".into(),
            source_authority_score: 1,
        }],
    }
}

fn requests() -> Vec<MemoryWriteRequest> {
    let draft = LongTermMemoryDraft {
        kind: LongTermMemoryKind::Project,
        topic: "wire-project".into(),
        content: "The project uses one canonical write contract.".into(),
        keywords: vec!["wire".into()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("wire-chat".into()),
        source_type: None,
        source_scope: None,
        subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
        provenance: LongTermMemoryProvenance::new(MemoryEvidenceAuthority::UserAsserted),
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec!["fixture:wire".into()],
        canonical_entities: vec![entity()],
        evidence_count: Some(1),
        observed_at: Some(1_800_000_000),
        source_revision: Some(1),
    };
    let candidate = MemoryWriteCandidate {
        candidate_id: "wire-candidate".into(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Project,
            topic: "wire-project".into(),
        },
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "wire-project".into(),
            body: draft.content.clone(),
            keywords: vec![],
        },
        evidence_refs: vec!["fixture:wire".into()],
        canonical_entities: vec![entity()],
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::RuntimeGate,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: Some(MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Project,
                topic: "wire-project".into(),
            }),
            reason: "factual source classification".into(),
        }),
    };
    let body = "One canonical evidence document.";
    let locator = "opaque://wire/document";
    let group = "wire-document";
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "chunk:body".into(),
        ordinal: 0,
        body: body.into(),
    }];
    let document = GovernedEvidenceDocumentDraft {
        memory_space_id: "space:wire".into(),
        mounted_subject_id: "agent:wire".into(),
        document_id: "document:wire".into(),
        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
        source_locator: locator.into(),
        canonical_evidence_group: group.into(),
        evidence_family_group: None,
        source_revision: 1,
        body: body.into(),
        content_digest: governed_evidence_document_content_digest(
            locator, group, None, body, &chunks,
        ),
        chunks,
        authority: MemoryEvidenceAuthority::UserAsserted,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        observed_at: 1_800_000_000,
    };
    vec![
        MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![draft],
                deletes: vec![LongTermMemorySlot {
                    kind: LongTermMemoryKind::Fact,
                    topic: "superseded-wire-fact".into(),
                }],
                skill_writes: vec![],
            },
        },
        MemoryWriteRequest::Candidates {
            candidates: vec![candidate],
        },
        MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![
                MemoryEvidenceDocumentMutation::Upsert {
                    draft: Box::new(document),
                },
                MemoryEvidenceDocumentMutation::Delete {
                    document_id: "document:old-wire".into(),
                    expected_owner_revision: 1,
                },
            ],
        },
    ]
}

#[test]
fn public_memory_write_wire_roundtrips_all_factual_intent_variants() {
    for request in requests() {
        let encoded = serde_json::to_value(&request).unwrap();
        let decoded: MemoryWriteRequest =
            serde_json::from_value(encoded.clone()).expect("public typed decoder");
        assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
    }
}

#[test]
fn published_deployment_write_examples_use_the_same_typed_contract() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (language, marker) in [("en", "Example write body:"), ("zh-CN", "写入 body 示例：")] {
        let document =
            std::fs::read_to_string(root.join(format!("docs/{language}/deployment.md"))).unwrap();
        let example = document
            .split_once(marker)
            .unwrap()
            .1
            .split_once("```json\n")
            .unwrap()
            .1
            .split_once("```")
            .unwrap()
            .0;
        let decoded: MemoryWriteRequest =
            serde_json::from_str(example).expect("published typed write example");
        let MemoryWriteRequest::Candidates { candidates } = decoded else {
            panic!("published example must submit factual candidates");
        };
        assert_eq!(candidates.len(), 1);
        assert!(matches!(
            candidates[0].target,
            MemoryCandidateTarget::LongTermMemory { .. }
        ));
    }
}

fn object_paths(value: &Value, path: String, paths: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            paths.push(path.clone());
            for (key, child) in fields {
                object_paths(
                    child,
                    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                    paths,
                );
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                object_paths(child, format!("{path}/{index}"), paths);
            }
        }
        _ => {}
    }
}

#[test]
fn public_memory_write_wire_rejects_unknown_fields_at_every_typed_object_boundary() {
    for request in requests() {
        let encoded = serde_json::to_value(request).unwrap();
        let mut paths = Vec::new();
        object_paths(&encoded, String::new(), &mut paths);
        assert!(!paths.is_empty());
        for path in paths {
            let mut injected = encoded.clone();
            injected
                .pointer_mut(&path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("untrusted_governance_override".into(), Value::Bool(true));
            assert!(
                serde_json::from_value::<MemoryWriteRequest>(injected).is_err(),
                "typed public write must reject unknown field at {path}"
            );
        }
    }
}
