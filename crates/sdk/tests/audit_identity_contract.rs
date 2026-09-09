#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::{Arc, Mutex};

use bm_sdk::{MemoryAuditEvent, MemoryAuditSink, MemoryIdentity, MemoryScope, NoopMemoryAuditSink};

use support::empty_store_platform;

#[derive(Default)]
struct CapturingAuditSink {
    events: Mutex<Vec<MemoryAuditEvent>>,
}

impl CapturingAuditSink {
    fn events(&self) -> Vec<MemoryAuditEvent> {
        self.events.lock().expect("events").clone()
    }
}

impl MemoryAuditSink for CapturingAuditSink {
    fn record(&self, event: MemoryAuditEvent) {
        self.events.lock().expect("events").push(event);
    }
}

#[test]
fn sdk_audit_events_bind_operation_to_memory_identity_and_scope() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let audit = Arc::new(CapturingAuditSink::default());
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-a", "owner-a").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "chat-a").expect("scope"))
        .store(platform)
        .audit_sink(audit.clone())
        .build()
        .expect("runtime");

    use bm_sdk::*;
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Project,
        topic: "audit identity".into(),
    };
    let written = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "audit-fact".into(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: target.clone(),
                long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::Text {
                    topic: "audit identity".into(),
                    body:
                        "The audit project requires operations to retain agent and owner identity."
                            .into(),
                    keywords: vec![],
                },
                evidence_refs: vec![],
                canonical_entities: vec![],
                semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                    source: MemorySemanticJudgmentSource::RuntimeGate,
                    decision: MemoryCandidateSemanticDecision::Accept,
                    governed_target: Some(target),
                    reason: "factual audit contract".into(),
                }),
            }],
        })
        .expect("formal factual write");
    assert!(written.accepted && written.changed > 0);
    runtime
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "audit identity".to_string(),
            system_max_len: 1024,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    let events = audit.events();
    let write = events
        .iter()
        .find(|event| event.operation == "write")
        .expect("write audit");
    assert_eq!(write.identity.agent_id, "agent-a");
    assert_eq!(write.identity.owner_id, "owner-a");
    assert_eq!(write.memory_space_id, default_memory_space_id("owner-a"));
    assert_eq!(write.subject_id, default_agent_subject_id("agent-a"));
    assert_eq!(write.conversation_id.as_deref(), Some("chat-a"));
    assert_ne!(write.memory_space_id, "system");

    let project = events
        .iter()
        .find(|event| event.operation == "project")
        .expect("project audit");
    assert_eq!(project.identity, write.identity);
    assert_eq!(project.memory_space_id, write.memory_space_id);
    assert_eq!(project.subject_id, write.subject_id);
}

#[test]
fn noop_audit_sink_keeps_public_contract_constructible() {
    let sink: Arc<dyn MemoryAuditSink> = Arc::new(NoopMemoryAuditSink);
    sink.record(MemoryAuditEvent::for_runtime_operation(
        "inspect",
        support::host_test_profile(),
        MemoryIdentity::new("agent-a", "owner-a").expect("identity"),
        MemoryScope::new("sdk.direct", "chat-a").expect("scope"),
        "owner-a",
        true,
        "ok",
    ));
}
