mod support;

use std::sync::{Arc, Mutex};

use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, MemoryAuditEvent, MemoryAuditSink,
    MemoryIdentity, MemoryProjectionRequest, MemoryRuntime, MemoryScope, MemoryWriteRequest,
    NoopMemoryAuditSink, PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource,
};

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
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let audit = Arc::new(CapturingAuditSink::default());
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-a", "owner-a").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "chat-a").expect("scope"))
        .profile(profile)
        .store(platform)
        .audit_sink(audit.clone())
        .build()
        .expect("runtime");

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "audit_identity_contract".to_string(),
                topic: "audit identity".to_string(),
                title: "Audit identity contract".to_string(),
                summary: "Audit events must carry identity and memory space.".to_string(),
                content: "Do not collapse SDK audit to system/system.".to_string(),
                citations: vec!["audit identity contract".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");
    runtime
        .project(MemoryProjectionRequest {
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
        ProfileId::ServerLinuxDevFull,
        MemoryIdentity::new("agent-a", "owner-a").expect("identity"),
        MemoryScope::new("sdk.direct", "chat-a").expect("scope"),
        "owner-a",
        true,
        "ok",
    ));
}
