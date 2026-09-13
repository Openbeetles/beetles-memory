//! Synthetic producers use the same explicit public control/issue contract as
//! trusted hosts. Submitting never registers, refreshes, or expands authority.
use bm_sdk::*;

pub fn governor(runtime: &MemoryRuntime, store: MemoryStoreHandle) -> MemoryRuntime {
    let config = runtime.config();
    let mut scoped = runtime.scoped_runtime().clone();
    scoped.actor_subject_id = runtime
        .subject_registry()
        .system_governor()
        .expect("SystemGovernor")
        .subject_id
        .clone();
    MemoryRuntime::builder()
        .identity(config.identity.clone())
        .scope(config.scope.clone())
        .store(store)
        .subject_registry(runtime.subject_registry().clone())
        .subject_relationship_graph(runtime.subject_relationship_graph().clone())
        .scoped_runtime(scoped)
        .clock(config.clock.clone())
        .capability_policy(config.capability_policy.clone())
        .privacy_policy(config.privacy_policy.clone())
        .audit_sink(config.audit_sink.clone())
        .agent_tool_registries(runtime.agent_tool_registries())
        .build()
        .expect("same-owner governor")
}

pub fn runtime_observer_spec(
    runtime: &MemoryRuntime,
    binding_id: &str,
) -> ProceduralProducerSpecV1 {
    let mut tools = runtime
        .agent_tool_registries()
        .iter()
        .flat_map(|registry| {
            registry.tools.iter().map(|tool| ProceduralProducerToolV1 {
                registry_ref: registry.registry_ref(),
                tool_id: tool.tool_id.clone(),
                schema_fingerprint: tool.schema_fingerprint.clone(),
            })
        })
        .collect::<Vec<_>>();
    tools.sort_by_cached_key(|tool| serde_json::to_vec(tool).expect("canonical synthetic tool"));
    let declares_tools = !tools.is_empty();
    ProceduralProducerSpecV1 {
        binding_id: binding_id.into(),
        scope: ProceduralProducerScopeV1 {
            memory_space_id: runtime.memory_space_id().into(),
            mounted_subject_id: runtime.subject_id().into(),
            channel_id: runtime.scope().channel.clone(),
            chat_id: runtime.scope().chat_id.clone(),
        },
        principal: ProceduralProducerPrincipalV1::LocalCapability {
            capability_id: format!("synthetic-{binding_id}"),
        },
        source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
        claims: ProceduralProducerClaimsV1 {
            execution_facts: declares_tools,
            method_declarations: declares_tools,
            usage_feedback: true,
            source_classifications: if declares_tools {
                vec![
                    ProceduralSourceSensitivity::NonPrivate,
                    ProceduralSourceSensitivity::Private,
                    ProceduralSourceSensitivity::Unknown,
                ]
            } else {
                Vec::new()
            },
        },
        tools,
        source_config_ref: format!("synthetic-source-{binding_id}"),
    }
}

pub fn register_and_issue(
    governor: &MemoryRuntime,
    spec: ProceduralProducerSpecV1,
    operation_id: &str,
) -> MemoryProceduralSubmissionCapability {
    let registered = governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: operation_id.into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec,
        })
        .expect("explicit synthetic producer registration");
    governor
        .procedural_submission_capability(&registered.binding.revision_ref().expect("revision"))
        .expect("explicit scoped submission capability")
}

pub fn canonical_observations(
    tool: &str,
    facts: &[ToolExecutionFactV1],
) -> Vec<ToolObservationDigest> {
    facts
        .iter()
        .map(|fact| ToolObservationDigest {
            observation_id: fact.observation_id.clone(),
            call_id: fact.call_id.clone(),
            tool_name: tool.into(),
            summary: "Synthetic execution outcome".into(),
            external_content: false,
        })
        .collect()
}
