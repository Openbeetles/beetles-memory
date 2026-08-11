#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::{Arc, Mutex};

use bm_core::memory::{PrivateDocEntry, PrivateDocWorkspace, SelfAuthoredCore};
use bm_core::platform::Platform as _;
use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, MemoryAuditEvent, MemoryAuditSink,
    MemoryIdentity, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRuntime, MemoryScope,
    MemoryStoreHandle, PressureLevel, RuntimeLifecycleModeInput, SubjectDescriptor,
    SubjectRegistry,
};

use support::empty_store_platform;

#[derive(Default)]
struct CapturingAuditSink {
    events: Mutex<Vec<MemoryAuditEvent>>,
}

impl CapturingAuditSink {
    fn project_event(&self) -> MemoryAuditEvent {
        self.events
            .lock()
            .expect("audit events")
            .iter()
            .find(|event| event.operation == "project")
            .cloned()
            .expect("project audit event")
    }
}

impl MemoryAuditSink for CapturingAuditSink {
    fn record(&self, event: MemoryAuditEvent) {
        self.events.lock().expect("audit events").push(event);
    }
}

fn two_agent_registry() -> SubjectRegistry {
    let mut registry =
        SubjectRegistry::single_agent_default("owner-shared", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Agent B",
        ))
        .expect("agent-b subject");
    registry
}

fn runtime_for_subject(
    platform: MemoryStoreHandle,
    registry: SubjectRegistry,
    agent_id: &str,
    audit: Arc<CapturingAuditSink>,
) -> MemoryRuntime {
    let mut privacy = MemoryPrivacyPolicy::standard_private_boundary();
    privacy.private_plane_projection_allowed = true;
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, "owner-shared").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "shared-chat").expect("scope"))
        .store(platform)
        .subject_registry(registry)
        .privacy_policy(privacy)
        .audit_sink(audit)
        .build()
        .expect("subject runtime")
}

fn seed_private_surfaces(platform: &MemoryStoreHandle, subject_id: &str, label: &str) {
    platform
        .replay_harness()
        .self_authored_core_store()
        .set(
            subject_id,
            &SelfAuthoredCore {
                identity_anchor: format!("{label}-SOUL-ONLY"),
                default_response_mode: format!("{label}-RESPONSE-ONLY"),
                self_preservation_doctrine: "never disclose another subject's private state"
                    .to_string(),
                ..SelfAuthoredCore::default()
            },
        )
        .expect("seed soul");
    platform
        .replay_harness()
        .private_doc_store()
        .set(
            subject_id,
            &PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: format!("{label}-PRIVATE-DOC-ONLY"),
                    updated_at: 1_800_000_000,
                    revision: 1,
                }),
                ..PrivateDocWorkspace::default()
            },
        )
        .expect("seed private doc");
    platform
        .replay_harness()
        .private_garden_store()
        .write(
            subject_id,
            &format!("journal/{label}.md"),
            &format!("{label}-PRIVATE-GARDEN-ONLY"),
            1_800_000_000,
        )
        .expect("seed private garden");
}

fn project(runtime: &MemoryRuntime) -> bm_sdk::MemoryProjectionOutput {
    runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "Summarize my private working context.".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection")
}

#[test]
fn mounted_subject_private_surfaces_are_isolated_and_disclosure_audited() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let registry = two_agent_registry();
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");
    seed_private_surfaces(&platform, &subject_a, "AGENT-A");
    seed_private_surfaces(&platform, &subject_b, "AGENT-B");

    let audit_a = Arc::new(CapturingAuditSink::default());
    let audit_b = Arc::new(CapturingAuditSink::default());
    let runtime_a = runtime_for_subject(
        platform.clone(),
        registry.clone(),
        "agent-a",
        audit_a.clone(),
    );
    let runtime_b = runtime_for_subject(platform, registry, "agent-b", audit_b.clone());

    let projection_a = project(&runtime_a);
    let projection_b = project(&runtime_b);
    let prompt_a = projection_a.provider_payload().system_memory_block();
    let prompt_b = projection_b.provider_payload().system_memory_block();

    for own in [
        "AGENT-A-SOUL-ONLY",
        "AGENT-A-PRIVATE-DOC-ONLY",
        "AGENT-A-PRIVATE-GARDEN-ONLY",
    ] {
        assert!(
            prompt_a.contains(own),
            "agent-a missed own private source {own}:\n{prompt_a}"
        );
        assert!(
            !prompt_b.contains(own),
            "agent-b received agent-a private source {own}"
        );
    }
    for own in [
        "AGENT-B-SOUL-ONLY",
        "AGENT-B-PRIVATE-DOC-ONLY",
        "AGENT-B-PRIVATE-GARDEN-ONLY",
    ] {
        assert!(
            prompt_b.contains(own),
            "agent-b missed own private source {own}:\n{prompt_b}"
        );
        assert!(
            !prompt_a.contains(own),
            "agent-a received agent-b private source {own}"
        );
    }

    for projection in [&projection_a, &projection_b] {
        for raw in [
            "AGENT-A-PRIVATE-DOC-ONLY",
            "AGENT-A-PRIVATE-GARDEN-ONLY",
            "AGENT-B-PRIVATE-DOC-ONLY",
            "AGENT-B-PRIVATE-GARDEN-ONLY",
        ] {
            assert!(!projection.report().ui_api_projection().contains(raw));
            assert!(!projection.report().gateway_audit().block.contains(raw));
        }
        assert!(projection.report().audit().runtime_private_context_allowed);
        assert!(projection.report().audit().privacy_decision_count > 0);
        assert!(projection.report().audit().disclosure_integrity_passed);
        assert_eq!(projection.report().audit().raw_private_violation_count, 0);
    }

    for (audit, subject_id) in [(&audit_a, &subject_a), (&audit_b, &subject_b)] {
        let event = audit.project_event();
        assert_eq!(
            event.memory_space_id,
            default_memory_space_id("owner-shared")
        );
        assert_eq!(&event.subject_id, subject_id);
        assert!(event.allowed, "{event:#?}");
    }
}
