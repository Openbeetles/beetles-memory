//! 0.8.0 API example; not compatible with the v0.7.0 tag.
//! Uses only an isolated InMemory Store and synthetic text. No provider, key,
//! network listener, disk Store or nonproduction harness is used.
use std::sync::Arc;
use std::time::{Duration, Instant};

use bm_entry::{
    GovernanceBindingSource, GovernanceCredentialRequest, GovernanceCredentialResolver,
    GovernanceProviderBinding, MemoryLearningService, MemoryLearningServiceStatusRequest,
    ResolvedGovernanceCredential,
};
use bm_sdk::{
    default_agent_subject_id, AgentToolDescriptor, AgentToolRegistrySnapshot,
    AgentToolUsageFeedbackV3, CanonicalTurnDelta, ConversationScope, IngressKind, MemoryIdentity,
    MemoryProceduralProducerControlRequest, MemoryRuntime, MemoryScope, MemoryStoreHandle,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    PostTurnLearningInputV2, PressureLevel, ProceduralProducerClaimsV1,
    ProceduralProducerPrincipalV1, ProceduralProducerScopeV1, ProceduralProducerSourceAuthorityV1,
    ProceduralProducerSpecV1, ProceduralProducerStateV1, ProceduralProducerToolV1,
    ProceduralSourceSensitivity, ProfileId, RuntimeLifecycleModeInput, StoreBackendConfig,
    SubjectDescriptor, SubjectRegistry, ToolExecutionFactV1, ToolExecutionOutcome,
    ToolMethodEvidenceV1, ToolObservationDigest, TranscriptInputMessage,
};

// Adapt these traits to the product's ONE configuration/credential owner.
// This example deliberately has no model binding and never reads credentials.
struct UnconfiguredProvider;
impl GovernanceBindingSource for UnconfiguredProvider {
    fn current_binding(&self) -> bm_sdk::Result<Option<GovernanceProviderBinding>> {
        Ok(None)
    }
}
impl GovernanceCredentialResolver for UnconfiguredProvider {
    fn resolve(
        &self,
        _: &GovernanceCredentialRequest,
    ) -> bm_sdk::Result<ResolvedGovernanceCredential> {
        Err(bm_sdk::Error::config("example", "no provider configured"))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile())?)?;
    let mut registry = SubjectRegistry::single_agent_default("example-owner", "agent-a")?;
    registry.upsert_subject(SubjectDescriptor::agent_persona(
        default_agent_subject_id("agent-b"),
        "Agent B",
    ))?;
    let tool_registry = AgentToolRegistrySnapshot::compact(
        "example-tools",
        "example-host",
        vec![AgentToolDescriptor::compact(
            "word-count",
            "Word count",
            "words-v1",
        )],
        1,
    );
    let agent_a = runtime(&store, &registry, &tool_registry, "agent-a")?;
    let agent_b = runtime(&store, &registry, &tool_registry, "agent-b")?;

    // Trusted composition root only: do not hand these governor runtimes to
    // model tools, plugins or remote callers. Preserve the same registry/graph.
    let governor_a = governor(&agent_a, store.clone())?;
    let governor_b = governor(&agent_b, store)?;
    let status_authority = governor_a.learning_service_status_authority()?;
    let (service, attachment_a) = MemoryLearningService::builder(Arc::clone(&agent_a))
        .control_authorities(governor_a.learning_service_control_authorities()?)
        .binding_source(Arc::new(UnconfiguredProvider))
        .credential_resolver(Arc::new(UnconfiguredProvider))
        .start()?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let attachment_b = service.attach_runtime(
            Arc::clone(&agent_b),
            governor_b.learning_service_control_authorities()?,
        )?;

        // Explicit ONE-TIME provisioning in this new ephemeral Store. In a
        // persistent product, reuse the existing grant; never regrant on each
        // request/reopen. Changes require the exact expected revision.
        let granted =
            governor_a.control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "example-provision-word-count".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec: ProceduralProducerSpecV1 {
                    binding_id: "example-word-count-producer".into(),
                    scope: ProceduralProducerScopeV1 {
                        memory_space_id: agent_a.memory_space_id().into(),
                        mounted_subject_id: agent_a.subject_id().into(),
                        channel_id: agent_a.scope().channel.clone(),
                        chat_id: agent_a.scope().chat_id.clone(),
                    },
                    principal: ProceduralProducerPrincipalV1::LocalCapability {
                        capability_id: "example-local-word-count".into(),
                    },
                    source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                    claims: ProceduralProducerClaimsV1 {
                        execution_facts: true,
                        method_declarations: true,
                        usage_feedback: false,
                        source_classifications: vec![ProceduralSourceSensitivity::NonPrivate],
                    },
                    tools: vec![ProceduralProducerToolV1 {
                        registry_ref: tool_registry.registry_ref(),
                        tool_id: "word-count".into(),
                        schema_fingerprint: "words-v1".into(),
                    }],
                    source_config_ref: "example-compiled-tool".into(),
                },
            })?;
        let capability =
            governor_a.procedural_submission_capability(&granted.binding.revision_ref()?)?;

        // The host actually performs this tiny deterministic operation. Only
        // the literal synthetic input justifies NonPrivate here; never apply
        // this classification to arbitrary tool results or user input.
        let input = "synthetic example text";
        let word_count = input.split_whitespace().count();
        let mut turn = request(
            &agent_a,
            "example-tool-turn",
            input,
            &format!("{word_count} words"),
        );
        turn.turn.tool_observations.push(ToolObservationDigest {
            observation_id: "example-observation-1".into(),
            call_id: "example-call-1".into(),
            tool_name: "word-count".into(),
            summary: "Synthetic word count completed".into(),
            external_content: false,
        });
        turn.learning = PostTurnLearningInputV2 {
            agent_tool_feedback: vec![AgentToolUsageFeedbackV3 {
                registry_ref: tool_registry.registry_ref(),
                tool_id: "word-count".into(),
                schema_fingerprint: "words-v1".into(),
                execution_facts: vec![ToolExecutionFactV1 {
                    observation_id: "example-observation-1".into(),
                    call_id: "example-call-1".into(),
                    outcome: ToolExecutionOutcome::Succeeded,
                    source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
                    started_at: None,
                    completed_at: None,
                }],
                method_evidence: vec![ToolMethodEvidenceV1 {
                    method_id: "example-word-count-method".into(),
                    task_signature: "count-whitespace-separated-words".into(),
                    body:
                        "1. Split the provided text on whitespace.\n2. Count the resulting words."
                            .into(),
                    execution_refs: vec!["example-observation-1".into()],
                    source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
                    external_content: false,
                }],
            }],
            ..PostTurnLearningInputV2::with_tool_call_count(1)
        };
        // Same turn + exact payload for a delivery retry. A new real execution
        // needs new turn/call/observation identities. No selection was used here;
        // feedback on projected skills must carry the original selection receipt.
        agent_a.finalize_turn_with_procedural_evidence(&capability, turn)?;
        attachment_a.wake()?;

        // A normal turn has no procedural grant requirement.
        agent_b.finalize_turn(request(
            &agent_b,
            "example-plain-turn",
            "Synthetic hello",
            "Hello",
        ))?;
        attachment_b.wake()?;
        let status = service.status(MemoryLearningServiceStatusRequest {
            authority: status_authority,
        })?;
        assert_eq!(status.attachment_count, 2);
        // Do not print raw turns, provider secrets or unfiltered internal reports.
        println!("Two runtimes attached; turns submitted. No model configured; learning completion not asserted.");
        Ok(())
    })();
    // Attempt an orderly bounded shutdown on both success and error. Drop also
    // owns worker cleanup; the host must not implement another worker/queue.
    let shutdown = service.shutdown(Instant::now() + Duration::from_secs(5));
    result?;
    shutdown?;
    Ok(())
}

fn runtime(
    store: &MemoryStoreHandle,
    registry: &SubjectRegistry,
    tools: &AgentToolRegistrySnapshot,
    agent: &str,
) -> bm_sdk::Result<Arc<MemoryRuntime>> {
    Ok(Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new(agent, "example-owner")?)
            // Share the memory space, not one subject-owned conversation or
            // Session shadow. The host assigns a distinct conversation per
            // dialog; it must not reuse another subject's conversation ID.
            .scope(MemoryScope::new(
                "example.embedded",
                format!("example-chat-{agent}"),
            )?)
            .store(store.clone())
            .subject_registry(registry.clone())
            .subject_id(default_agent_subject_id(agent))
            .agent_tool_registry(tools.clone())
            .build()?,
    ))
}

fn governor(agent: &MemoryRuntime, store: MemoryStoreHandle) -> bm_sdk::Result<MemoryRuntime> {
    let mut scoped = agent.scoped_runtime().clone();
    scoped.actor_subject_id = agent
        .subject_registry()
        .system_governor()
        .ok_or_else(|| bm_sdk::Error::config("example", "SystemGovernor missing"))?
        .subject_id
        .clone();
    MemoryRuntime::builder()
        .identity(agent.config().identity.clone())
        .scope(agent.scope().clone())
        .store(store)
        .subject_registry(agent.subject_registry().clone())
        .subject_relationship_graph(agent.subject_relationship_graph().clone())
        .scoped_runtime(scoped)
        .agent_tool_registries(agent.agent_tool_registries())
        .build()
}

fn request(agent: &MemoryRuntime, id: &str, input: &str, reply: &str) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: id.into(),
            conversation: ConversationScope {
                channel: agent.scope().channel.clone(),
                chat_id: agent.scope().chat_id.clone(),
                conversation_id: Some(agent.scope().chat_id.clone()),
            },
            subject: agent.subject_id().into(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: IngressKind::User,
                channel: agent.scope().channel.clone(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: None,
                model_alias: None,
                model_resolved: None,
                request_id: Some(id.into()),
                client_conversation_hint: None,
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user(input)],
            assistant_message: Some(TranscriptInputMessage::assistant(reply)),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        learning: PostTurnLearningInputV2::empty(),
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

#[cfg(feature = "desktop-macos")]
fn profile() -> ProfileId {
    ProfileId::DesktopMacosEmbeddedSdk
}
#[cfg(feature = "desktop-windows")]
fn profile() -> ProfileId {
    ProfileId::DesktopWindowsEmbeddedSdk
}
#[cfg(feature = "desktop-linux")]
fn profile() -> ProfileId {
    ProfileId::DesktopLinuxEmbeddedSdk
}
