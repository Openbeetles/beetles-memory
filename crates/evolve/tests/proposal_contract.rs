use std::sync::Arc;

use bm_core::llm::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy,
    ToolSpec,
};
use bm_core::platform::ResponseBody;
use bm_evolve::{
    commit_evolution_proposal, validate_evolution_proposal, EvolutionCandidate, EvolutionProposal,
    EvolutionSandboxPolicy,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryClock, MemoryIdentity, MemoryPrivacyPolicy, MemoryRecallRequest,
    MemoryRuntime, MemoryScope, ProfileId, RuntimeSkillWrite, StoreBackendConfig, StorePlatform,
};

#[test]
fn proposal_commit_uses_sdk_write_governance() {
    let runtime = test_runtime(ProfileId::ServerLinuxDevFull);
    let proposal = procedural_proposal(ProfileId::ServerLinuxDevFull);

    let report = commit_evolution_proposal(&runtime, proposal).expect("proposal commit");

    assert!(report.accepted, "{report:?}");
    assert_eq!(
        report.write_operation.as_deref(),
        Some("write.procedural_promotions")
    );
    assert_eq!(report.lifecycle_operations, vec!["maintain".to_string()]);

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
        })
        .expect("recall");
    assert!(recall
        .procedural_hits
        .iter()
        .any(|hit| hit.record.name == "runtime_skill__release_guard"));
}

#[test]
fn governance_note_only_proposal_is_report_only_until_sdk_operation_exists() {
    let runtime = test_runtime(ProfileId::ServerLinuxDevFull);
    let proposal = EvolutionProposal {
        proposal_id: "governance-note".to_string(),
        profile: ProfileId::ServerLinuxDevFull,
        candidates: vec![EvolutionCandidate::GovernanceNote {
            target: "self-authored-core".to_string(),
            summary: "Candidate evidence only, not direct write authority.".to_string(),
        }],
        evidence_refs: vec!["counterfactual-trace".to_string()],
        rationale: "Sandbox can only submit through SDK-owned operations.".to_string(),
    };

    let report = commit_evolution_proposal(&runtime, proposal).expect("proposal report");

    assert!(!report.accepted);
    assert_eq!(
        report.reason,
        "governance_note_requires_future_sdk_operation"
    );
    assert_eq!(report.committed_writes, 0);
}

#[test]
fn embedded_sdk_profile_can_preview_but_cannot_submit_sandbox_proposals() {
    let runtime = test_runtime(ProfileId::EspEmbeddedSdk);
    let proposal = procedural_proposal(ProfileId::EspEmbeddedSdk);
    let policy =
        EvolutionSandboxPolicy::for_profile(ProfileId::EspEmbeddedSdk).expect("embedded policy");
    let validation = validate_evolution_proposal(&policy, &proposal);

    assert!(policy.proposal_preview_allowed);
    assert!(!policy.compact_sandbox_allowed);
    assert!(!policy.proposal_submission_allowed);
    assert!(!validation.accepted);

    let report = commit_evolution_proposal(&runtime, proposal).expect("proposal report");
    assert!(!report.accepted);
    assert_eq!(report.reason, "proposal_submission_not_allowed_for_profile");
}

fn procedural_proposal(profile: ProfileId) -> EvolutionProposal {
    EvolutionProposal {
        proposal_id: "release-skill-proposal".to_string(),
        profile,
        candidates: vec![EvolutionCandidate::ProceduralMemory {
            write: RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["evolution proposal".to_string()],
                source_chat_id: Some("evolve-chat".to_string()),
                observed_at: 1_800_000_000,
            },
        }],
        evidence_refs: vec!["proposal-fixture".to_string()],
        rationale: "Promote repeated release checks into procedural memory.".to_string(),
    }
}

fn test_runtime(profile: ProfileId) -> MemoryRuntime {
    let platform =
        StorePlatform::open_in_memory(StoreBackendConfig::in_memory(profile).unwrap()).unwrap();
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("evolve-agent", "evolve-owner").unwrap())
        .scope(MemoryScope::new("evolve", "evolve-chat").unwrap())
        .profile(profile)
        .store_platform(platform)
        .clock(Arc::new(FixedClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .build()
        .unwrap()
}

struct FixedClock;

impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

#[allow(dead_code)]
struct StaticHttpClient;

impl LlmHttpClient for StaticHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> bm_core::Result<(u16, ResponseBody)> {
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

#[allow(dead_code)]
struct StaticLlmClient;

impl LlmClient for StaticLlmClient {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> bm_core::Result<LlmResponse> {
        Ok(LlmResponse {
            content: "Summary: evolve proposal".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}
