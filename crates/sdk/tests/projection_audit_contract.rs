#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    board_subject_scope_id, relationship_scope_id, RelationshipConstitution,
    RelationshipConstitutionAlignment, RelationshipGovernanceState, SelfAuthoredCore,
    SelfContinuity,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, MemoryProjectionRequest, PressureLevel,
    ProfileId, ProjectionSourceAuthority, RuntimeLifecycleModeInput,
};

use support::{empty_store_platform, seeded_store_platform, test_runtime_with_scope};

#[test]
fn projection_report_exposes_sdk_owned_source_scope_budget_and_privacy_audit() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = seeded_store_platform(profile);
    platform
        .replay_harness()
        .self_authored_core_store()
        .set(
            board_subject_scope_id(),
            &SelfAuthoredCore {
                identity_anchor: "board-level inhabited subject".to_string(),
                default_response_mode: "work-first direct reply".to_string(),
                default_task_scope: "complete the user task".to_string(),
                default_initiative_posture: "continue without theatrical drift".to_string(),
                self_preservation_doctrine: "protect private inner material".to_string(),
                ..SelfAuthoredCore::default()
            },
        )
        .expect("seed self-authored core");
    platform
        .replay_harness()
        .self_continuity_store()
        .set(
            board_subject_scope_id(),
            &SelfContinuity {
                wake_anchor: "wake as the same inhabited board subject".to_string(),
                continuity_bridge: "carry release work forward from governed memory".to_string(),
                task_posture: "do the requested engineering work first".to_string(),
                ..SelfContinuity::default()
            },
        )
        .expect("seed self continuity");
    let relationship_id = relationship_scope_id("sdk.direct", "chat-a");
    platform
        .replay_harness()
        .relationship_constitution_store()
        .set(
            &relationship_id,
            &RelationshipConstitution {
                scope_id: relationship_id.clone(),
                channel: "sdk.direct".to_string(),
                chat_id: "chat-a".to_string(),
                governance_state: RelationshipGovernanceState::Maintain,
                alignment: RelationshipConstitutionAlignment::Adaptive,
                inherited_relationship_posture: "engineering collaborator".to_string(),
                inherited_response_mode: "work-first direct reply".to_string(),
                ..RelationshipConstitution::default()
            },
        )
        .expect("seed relationship constitution");
    assert!(platform
        .replay_harness()
        .self_authored_core_store()
        .get(board_subject_scope_id())
        .expect("read seeded core")
        .is_some());
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "chat-a");

    let report = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "How should release safety work?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    assert_eq!(report.audit.operation, "project");
    assert_eq!(report.audit.profile, profile);
    assert_eq!(report.audit.identity.agent_id, "agent-main");
    assert_eq!(report.audit.identity.owner_id, "owner-default");
    assert_eq!(report.audit.scope.channel, "sdk.direct");
    assert_eq!(report.audit.scope.chat_id, "chat-a");
    assert_eq!(
        report.audit.memory_space_id,
        default_memory_space_id("owner-default")
    );
    assert_eq!(
        report.audit.subject_id,
        default_agent_subject_id("agent-main")
    );
    assert_eq!(
        report.audit.scoped_runtime.mounted_subject_id,
        default_agent_subject_id("agent-main")
    );
    assert_eq!(
        report.audit.scoped_runtime.actor_subject_id,
        default_agent_subject_id("agent-main")
    );
    assert_eq!(report.audit.conversation_id.as_deref(), Some("chat-a"));
    assert!(report.audit.injected);
    assert_eq!(
        report.audit.system_memory_chars,
        report.system_memory_block.chars().count()
    );
    assert_eq!(
        report.audit.render_budget_chars,
        runtime
            .runtime_budget()
            .projection_render_budget
            .system_block_max_chars
            .min(4096)
    );
    assert_eq!(
        report.audit.source_budget_chars,
        runtime
            .runtime_budget()
            .projection_source_budget
            .context_assembly_max_chars
    );
    assert!(report
        .audit
        .sources
        .iter()
        .any(|source| source.plane == "shared_factual" && source.selected_count > 0));
    assert!(report
        .audit
        .sections
        .iter()
        .any(|section| section.name == "governed_memory_evidence" && section.chars > 0));
    for section in &report.audit.sections {
        assert!(!section.name.contains("self_state"));
        assert!(!section.name.contains("inner_life"));
        assert!(!section.name.contains("private_garden"));
    }
    assert!(
        !report.audit.private_gate.runtime_private_context_allowed,
        "standard SDK projection policy must not load private runtime depth by default"
    );
    assert!(
        !report.audit.private_gate.foreground_disclosure_allowed,
        "foreground disclosure must be separate from runtime-private access"
    );
    assert!(report.audit.private_gate.reason.contains("privacy_policy"));
    let long_term_authority = report
        .audit
        .source_authority
        .iter()
        .find(|source| source.source_id == "long_term_memory")
        .expect("long term source authority");
    assert!(long_term_authority.loaded);
    assert!(long_term_authority
        .authorities
        .contains(&ProjectionSourceAuthority::UserProvidedEvidence));
    assert!(long_term_authority.foreground_disclosure_allowed);
    let private_garden_authority = report
        .audit
        .source_authority
        .iter()
        .find(|source| source.source_id == "private_garden")
        .expect("private garden source authority");
    assert!(!private_garden_authority.foreground_disclosure_allowed);
    assert!(!private_garden_authority.raw_audit_plaintext_allowed);
    assert!(private_garden_authority
        .authorities
        .contains(&ProjectionSourceAuthority::PrivateInternal));
    assert_eq!(report.subject_projection.profile, profile);
    assert_eq!(
        report.subject_projection.projection_id,
        report.audit.projection_id
    );
    assert!(report
        .subject_projection
        .identity_mount
        .contains("agent-main"));
    assert!(report
        .subject_projection
        .identity_mount
        .contains("subject:agent:agent-main"));
    assert!(!report
        .subject_projection
        .identity_mount
        .to_ascii_lowercase()
        .contains("pretend"));
    assert!(report
        .subject_projection
        .relationship_position
        .contains("sdk.direct"));
    assert!(report
        .subject_projection
        .evidence_refs
        .iter()
        .any(|evidence| evidence.contains("shared_factual")
            || evidence.starts_with("opaque:evidence:")));
    assert!(report
        .subject_projection
        .budget_decisions
        .iter()
        .any(|decision| decision.surface == "prompt"));
    assert!(report
        .subject_projection
        .privacy_decisions
        .iter()
        .any(|decision| !decision.allowed && decision.reason.contains("privacy_policy")));
    assert!(report.subject_projection.validate_contract().accepted);
    assert!(report.projection_faithfulness.passed);
    assert_eq!(
        report
            .private_disclosure_integrity
            .raw_private_violation_count,
        0
    );
    assert!(report.private_disclosure_integrity.passed);
    for forbidden_marker in [
        "private_raw:",
        "private-garden-raw:",
        "private garden raw:",
        "<private_raw>",
    ] {
        assert!(
            !report
                .system_memory_block
                .to_ascii_lowercase()
                .contains(forbidden_marker),
            "projection leaked {forbidden_marker}"
        );
    }
}

#[test]
fn projection_runtime_envelope_replaces_flat_internal_sections() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = seeded_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "chat-a");

    let report = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "Prepare the release checklist without drifting into roleplay.".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    for required_heading in [
        "## LLM Runtime Projection Envelope",
        "## Subject Mount",
        "## Governed Memory Evidence",
        "## Boundary And Disclosure Protocol",
        "## Runtime Constraints",
        "## Work Integrity Covenant",
    ] {
        assert!(
            report.system_memory_block.contains(required_heading),
            "{}",
            report.system_memory_block
        );
    }
    for forbidden_heading in [
        "## Self State",
        "## Inner Life",
        "## Private Garden",
        "## Inner Workspace",
        "## Autonomy Strategy",
        "## Outer Voice",
        "## Mental Privacy Boundary",
    ] {
        assert!(
            !report.system_memory_block.contains(forbidden_heading),
            "{} must not appear in runtime envelope:\n{}",
            forbidden_heading,
            report.system_memory_block
        );
    }
    assert_eq!(
        report.runtime_projection.rendered_block,
        report.system_memory_block
    );
    assert_eq!(
        report.runtime_projection.projection_id,
        report.audit.projection_id
    );
    assert!(report
        .runtime_projection
        .section_names
        .contains(&"governed_memory_evidence".to_string()));
    assert!(report
        .runtime_projection
        .section_names
        .contains(&"protected_private_runtime_context".to_string()));
    assert!(!report
        .runtime_projection
        .section_names
        .contains(&"private_garden".to_string()));
    assert!(
        report
            .subject_projection
            .identity_mount
            .contains("Subject Mount"),
        "{:?}",
        report.subject_projection
    );
    assert!(
        report
            .subject_projection
            .evidence_refs
            .iter()
            .any(|evidence| evidence == "subject_mount:degraded"),
        "{:?}",
        report.subject_projection
    );
    assert_eq!(
        report.life_projection.identity_mount,
        report.subject_projection.subject_mount.identity_mount
    );
    assert_eq!(
        report.work_integrity.task_goal,
        report.subject_projection.work_integrity.task_goal
    );
    assert!(
        report.subject_projection.validate_contract().accepted,
        "{:?}",
        report.subject_projection.validate_contract()
    );
}

#[test]
fn projection_report_exposes_disclosure_integrity_for_runtime_surfaces() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = seeded_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "chat-a");

    let report = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "Prepare the release checklist.".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    for surface in [
        "prompt",
        "ui_api",
        "operator_raw",
        "gateway_raw_audit",
        "shared_fact_surface",
    ] {
        assert!(
            report
                .private_disclosure_integrity
                .checked_surfaces
                .contains(&surface.to_string()),
            "{:?}",
            report.private_disclosure_integrity
        );
    }
    assert!(report
        .private_disclosure_integrity
        .blocked_source_ids
        .contains(&"private_depth".to_string()));
    assert_eq!(
        report
            .private_disclosure_integrity
            .raw_private_violation_count,
        0
    );
    assert!(report.private_disclosure_integrity.passed);
    assert!(report
        .projection_faithfulness
        .checked_claims
        .contains(&"subject_mount.identity_mount".to_string()));
    assert!(report.projection_faithfulness.unsupported_claims.is_empty());
}

#[test]
fn empty_store_projection_degrades_subject_mount_without_inventing_personality() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "empty-chat");

    let report = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "Summarize what you know before answering.".to_string(),
            system_max_len: 2048,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    assert!(
        report.system_memory_block.contains("## Subject Mount"),
        "{}",
        report.system_memory_block
    );
    assert!(
        report
            .system_memory_block
            .contains("subject_mount_degraded"),
        "{}",
        report.system_memory_block
    );
    assert!(
        report
            .subject_projection
            .identity_mount
            .contains("subject_mount_degraded"),
        "{:?}",
        report.subject_projection
    );
    assert!(report
        .subject_projection
        .dropped_candidates
        .iter()
        .any(|candidate| candidate.reason == "subject_mount_degraded"));
    assert!(!report
        .subject_projection
        .identity_mount
        .to_ascii_lowercase()
        .contains("pretend"));
    assert!(
        report.subject_projection.validate_contract().accepted,
        "{:?}",
        report.subject_projection.validate_contract()
    );
}

#[test]
fn empty_store_greeting_projection_does_not_leak_identity_meta_terms() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "empty-chat");

    let report = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "你好".to_string(),
            system_max_len: 2048,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    let lower = report.system_memory_block.to_ascii_lowercase();
    for forbidden in [
        "roleplay",
        "personality",
        "model identity",
        "memory helper",
        "assistant self-description",
        "relationship theater",
        "training provenance",
        "user-facing identity",
        "ai assistant",
        "模拟角色",
        "角色扮演",
        "人设",
        "人格",
        "ai 助手",
        "人工智能模型",
    ] {
        assert!(
            !lower.contains(forbidden),
            "{forbidden} leaked into clean greeting projection:\n{}",
            report.system_memory_block
        );
    }
}
