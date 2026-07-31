use bm_sdk::{
    AgentToolRegistryRef, AgentToolUsageFeedback, GovernedRuntimeSkillWriteInput, IngressKind,
    LongTermMemoryQuery, MemoryCloseRequest, MemoryGovernancePolicyMutation,
    MemoryInspectionRequest, MemoryLongTermControlView, MemoryLongTermDetailRequest,
    MemoryLongTermListRequest, MemoryLongTermMutation, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest, MemoryLongTermTarget, MemoryMaintenanceRequest,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRecallTemporalOperation, MemoryRecoverRequest, MemoryReplayRequest,
    MemoryTranscriptAttrWriteRequest, MemoryWriteRequest, PressureLevel, QueryFacetInput, Result,
    RuntimeLifecycleModeInput, RuntimeLifecycleTrigger, RuntimeSkillCreationRef,
    RuntimeSkillOwningScope, RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteSource,
    TranscriptAttrEnvelope,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{AdapterCommand, AdapterOperation};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterJsonCommandOptions {
    pub citation: String,
    pub default_source_chat_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedAdapterJsonCommandSchema {
    pub field_names: &'static [&'static str],
    pub input_schema: Value,
}

pub fn governed_adapter_json_command_schema(
    operation: AdapterOperation,
) -> Option<GovernedAdapterJsonCommandSchema> {
    let temporal_operation = json!({
        "oneOf": [
            {
                "type": "object",
                "properties": {
                    "kind": {"const": "current"}
                },
                "required": ["kind"],
                "additionalProperties": false
            },
            {
                "type": "object",
                "properties": {
                    "kind": {"const": "historical_as_of"},
                    "as_of_time": {"type": "integer", "minimum": 1}
                },
                "required": ["kind", "as_of_time"],
                "additionalProperties": false
            }
        ]
    });
    match operation {
        AdapterOperation::Recall => Some(GovernedAdapterJsonCommandSchema {
            field_names: &[
                "temporal_operation",
                "query",
                "limit",
                "structured_query_facets",
                "tool_registry_refs",
            ],
            input_schema: json!({
                "type": "object",
                "properties": {
                    "temporal_operation": temporal_operation,
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1},
                    "structured_query_facets": {
                        "type": "array",
                        "items": {"type": "object"}
                    },
                    "tool_registry_refs": {
                        "type": "array",
                        "items": {"type": "object"}
                    }
                },
                "required": ["temporal_operation", "query"],
                "additionalProperties": false
            }),
        }),
        AdapterOperation::Project => Some(GovernedAdapterJsonCommandSchema {
            field_names: &[
                "temporal_operation",
                "user_query",
                "system_max_len",
                "recent_messages_limit",
                "pressure",
                "mode_input",
                "structured_query_facets",
                "tool_registry_refs",
            ],
            input_schema: json!({
                "type": "object",
                "properties": {
                    "temporal_operation": temporal_operation,
                    "user_query": {"type": "string"},
                    "system_max_len": {"type": "integer", "minimum": 1},
                    "recent_messages_limit": {"type": "integer", "minimum": 1},
                    "pressure": {"type": "string"},
                    "mode_input": {"type": "object"},
                    "structured_query_facets": {
                        "type": "array",
                        "items": {"type": "object"}
                    },
                    "tool_registry_refs": {
                        "type": "array",
                        "items": {"type": "object"}
                    }
                },
                "required": ["temporal_operation", "user_query", "system_max_len"],
                "additionalProperties": false
            }),
        }),
        _ => None,
    }
}

impl AdapterJsonCommandOptions {
    pub fn new(citation: impl Into<String>) -> Self {
        Self {
            citation: citation.into(),
            default_source_chat_id: None,
        }
    }

    pub fn with_default_source_chat_id(mut self, chat_id: impl Into<String>) -> Self {
        self.default_source_chat_id = Some(chat_id.into());
        self
    }
}

pub fn decode_json_adapter_command(
    operation: AdapterOperation,
    body: &str,
    options: &AdapterJsonCommandOptions,
) -> Result<AdapterCommand> {
    match operation {
        AdapterOperation::Capabilities => Ok(AdapterCommand::Capabilities),
        AdapterOperation::Write => decode_write(body, options),
        AdapterOperation::Recall => {
            let payload: RecallPayload = parse_json(body)?;
            Ok(AdapterCommand::Recall(MemoryRecallRequest {
                temporal_operation: payload.temporal_operation,
                query: payload.query,
                limit: payload.limit.unwrap_or(8),
                structured_query_facets: payload.structured_query_facets,
                tool_registry_refs: payload.tool_registry_refs,
            }))
        }
        AdapterOperation::Project => {
            let payload: ProjectPayload = parse_json(body)?;
            Ok(AdapterCommand::Project(MemoryProjectionRequest {
                temporal_operation: payload.temporal_operation,
                user_query: payload.user_query,
                system_max_len: payload.system_max_len,
                recent_messages_limit: payload.recent_messages_limit.unwrap_or(8),
                pressure: payload.pressure,
                mode_input: payload.mode_input,
                structured_query_facets: payload.structured_query_facets,
                tool_registry_refs: payload.tool_registry_refs,
            }))
        }
        AdapterOperation::Maintain => {
            let payload: MaintainPayload = parse_json(body)?;
            Ok(AdapterCommand::Maintain(MemoryMaintenanceRequest {
                ingress: payload.ingress,
                user_content: payload.user_content,
                reply_content: payload.reply_content,
                tool_calls: payload.tool_calls.unwrap_or(0),
                external_content_used: payload.external_content_used.unwrap_or(false),
                runtime_skill_selected_ids: payload.runtime_skill_selected_ids,
                task_learning_selected_ids: payload.task_learning_selected_ids,
                reuse_outcome: payload.reuse_outcome,
                reuse_outcome_note: payload.reuse_outcome_note,
                pressure: payload.pressure,
                mode_input: payload.mode_input,
            }))
        }
        AdapterOperation::Inspect => {
            let payload: InspectPayload = parse_json(body)?;
            Ok(AdapterCommand::Inspect(MemoryInspectionRequest {
                query: payload.query,
                system_max_len: payload.system_max_len,
                pressure: payload.pressure,
                mode_input: payload.mode_input,
            }))
        }
        AdapterOperation::Recover => {
            let payload: RecoverPayload = parse_json(body)?;
            Ok(AdapterCommand::Recover(MemoryRecoverRequest {
                trigger: payload.trigger,
                mode_input: payload.mode_input,
            }))
        }
        AdapterOperation::Replay => {
            let payload: ReplayPayload = parse_json(body)?;
            Ok(AdapterCommand::Replay(MemoryReplayRequest {
                chat_id: payload.chat_id,
                limit: payload.limit.unwrap_or(8),
            }))
        }
        AdapterOperation::LongTermList => {
            let payload: LongTermListPayload = parse_json(body)?;
            Ok(AdapterCommand::LongTermList(MemoryLongTermListRequest {
                query: payload.query,
                cursor: payload.cursor,
                limit: payload.limit.unwrap_or(20),
                view: payload.view,
            }))
        }
        AdapterOperation::LongTermDetail => {
            let payload: LongTermDetailPayload = parse_json(body)?;
            Ok(AdapterCommand::LongTermDetail(
                MemoryLongTermDetailRequest {
                    target: payload.target,
                    view: payload.view,
                },
            ))
        }
        AdapterOperation::LongTermMutate => {
            let payload: LongTermMutationPayload = parse_json(body)?;
            Ok(AdapterCommand::LongTermMutate(Box::new(
                MemoryLongTermMutationRequest {
                    operation: payload.operation,
                    reason: payload.reason,
                    dry_run: payload.dry_run.unwrap_or(false),
                    mode_input: payload.mode_input,
                },
            )))
        }
        AdapterOperation::LongTermPolicy => {
            let payload: LongTermPolicyPayload = parse_json(body)?;
            Ok(AdapterCommand::LongTermPolicy(
                MemoryLongTermPolicyRequest {
                    operation: payload.operation,
                    reason: payload.reason,
                    dry_run: payload.dry_run.unwrap_or(false),
                    mode_input: payload.mode_input,
                },
            ))
        }
        AdapterOperation::TranscriptAttrWrite => {
            let payload: TranscriptAttrWritePayload = parse_json(body)?;
            Ok(AdapterCommand::TranscriptAttrWrite(
                MemoryTranscriptAttrWriteRequest {
                    memory_space_id: payload.memory_space_id,
                    channel_id: payload.channel_id,
                    conversation_id: payload.conversation_id,
                    attrs: payload.attrs,
                    idempotency_key: payload.idempotency_key,
                    dry_run: payload.dry_run.unwrap_or(false),
                },
            ))
        }
        AdapterOperation::Close => {
            let payload: ClosePayload = parse_json(body)?;
            Ok(AdapterCommand::Close(MemoryCloseRequest {
                reason: payload.reason,
            }))
        }
        AdapterOperation::Subscribe => Err(bm_sdk::Error::config(
            "adapter_json_command",
            "subscribe is a transport stream operation, not an SDK memory command",
        )),
    }
}

fn decode_write(body: &str, options: &AdapterJsonCommandOptions) -> Result<AdapterCommand> {
    let payload: WritePayload = parse_json(body)?;
    if let Some(feedback) = payload.tool_usage_feedback {
        return Ok(AdapterCommand::Write(
            MemoryWriteRequest::AgentToolUsageFeedback { feedback },
        ));
    }
    let owning_scope = payload.owning_scope.ok_or_else(|| {
        bm_sdk::Error::config("adapter_json_command", "write payload missing owning_scope")
    })?;
    let mut writes = if payload.writes.is_empty() {
        vec![GovernedRuntimeSkillWriteInput {
            write: RuntimeSkillWrite {
                name: required_field(payload.name, "name")?,
                topic: required_field(payload.topic, "topic")?,
                title: required_field(payload.title, "title")?,
                summary: required_field(payload.summary, "summary")?,
                content: required_field(payload.content, "content")?,
                citations: payload
                    .citations
                    .filter(|citations| !citations.is_empty())
                    .unwrap_or_else(|| vec![options.citation.clone()]),
                source_chat_id: payload
                    .source_chat_id
                    .or_else(|| options.default_source_chat_id.clone()),
                observed_at: 0,
            },
            creation_ref: payload.creation_ref.ok_or_else(|| {
                bm_sdk::Error::config("adapter_json_command", "write payload missing creation_ref")
            })?,
            privacy_class: payload.privacy_class.ok_or_else(|| {
                bm_sdk::Error::config(
                    "adapter_json_command",
                    "write payload missing privacy_class",
                )
            })?,
        }]
    } else {
        payload
            .writes
            .into_iter()
            .map(AdapterRuntimeSkillWritePayload::into_runtime_write)
            .collect()
    };
    for write in &mut writes {
        write.write.observed_at = 0;
    }
    Ok(AdapterCommand::Write(MemoryWriteRequest::Procedural {
        writes,
        owning_scope,
        source: payload.source,
    }))
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &str) -> Result<T> {
    serde_json::from_str(body)
        .map_err(|err| bm_sdk::Error::config("adapter_json_command", err.to_string()))
}

fn required_field(value: Option<String>, field: &'static str) -> Result<String> {
    let Some(value) = value else {
        return Err(bm_sdk::Error::config(
            "adapter_json_command",
            format!("write payload missing {field}"),
        ));
    };
    if value.trim().is_empty() {
        return Err(bm_sdk::Error::config(
            "adapter_json_command",
            format!("write payload has empty {field}"),
        ));
    }
    Ok(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WritePayload {
    #[serde(default)]
    tool_usage_feedback: Option<AgentToolUsageFeedback>,
    #[serde(default)]
    writes: Vec<AdapterRuntimeSkillWritePayload>,
    #[serde(default)]
    source: RuntimeSkillWriteSource,
    #[serde(default)]
    owning_scope: Option<RuntimeSkillOwningScope>,
    #[serde(default)]
    creation_ref: Option<RuntimeSkillCreationRef>,
    #[serde(default)]
    privacy_class: Option<MemoryPrivacyClass>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    citations: Option<Vec<String>>,
    #[serde(default)]
    source_chat_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterRuntimeSkillWritePayload {
    name: String,
    topic: String,
    title: String,
    summary: String,
    content: String,
    #[serde(default)]
    citations: Vec<String>,
    #[serde(default)]
    source_chat_id: Option<String>,
    creation_ref: RuntimeSkillCreationRef,
    privacy_class: MemoryPrivacyClass,
}

impl AdapterRuntimeSkillWritePayload {
    fn into_runtime_write(self) -> GovernedRuntimeSkillWriteInput {
        GovernedRuntimeSkillWriteInput {
            write: RuntimeSkillWrite {
                name: self.name,
                topic: self.topic,
                title: self.title,
                summary: self.summary,
                content: self.content,
                citations: self.citations,
                source_chat_id: self.source_chat_id,
                observed_at: 0,
            },
            creation_ref: self.creation_ref,
            privacy_class: self.privacy_class,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecallPayload {
    temporal_operation: MemoryRecallTemporalOperation,
    query: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    structured_query_facets: Vec<QueryFacetInput>,
    #[serde(default)]
    tool_registry_refs: Vec<AgentToolRegistryRef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectPayload {
    temporal_operation: MemoryRecallTemporalOperation,
    user_query: String,
    system_max_len: usize,
    #[serde(default)]
    recent_messages_limit: Option<usize>,
    #[serde(default)]
    pressure: PressureLevel,
    #[serde(default)]
    mode_input: RuntimeLifecycleModeInput,
    #[serde(default)]
    structured_query_facets: Vec<QueryFacetInput>,
    #[serde(default)]
    tool_registry_refs: Vec<AgentToolRegistryRef>,
}

#[derive(Deserialize)]
struct MaintainPayload {
    #[serde(default)]
    ingress: IngressKind,
    #[serde(default)]
    user_content: String,
    #[serde(default)]
    reply_content: String,
    #[serde(default)]
    tool_calls: Option<u32>,
    #[serde(default)]
    external_content_used: Option<bool>,
    #[serde(default)]
    runtime_skill_selected_ids: Vec<String>,
    #[serde(default)]
    task_learning_selected_ids: Vec<String>,
    #[serde(default)]
    reuse_outcome: RuntimeSkillReuseOutcome,
    #[serde(default)]
    reuse_outcome_note: String,
    #[serde(default)]
    pressure: PressureLevel,
    #[serde(default)]
    mode_input: RuntimeLifecycleModeInput,
}

#[derive(Deserialize)]
struct InspectPayload {
    query: String,
    system_max_len: usize,
    #[serde(default)]
    pressure: PressureLevel,
    #[serde(default)]
    mode_input: RuntimeLifecycleModeInput,
}

#[derive(Deserialize)]
struct RecoverPayload {
    #[serde(default = "default_recover_trigger")]
    trigger: RuntimeLifecycleTrigger,
    #[serde(default)]
    mode_input: RuntimeLifecycleModeInput,
}

#[derive(Deserialize)]
struct ReplayPayload {
    chat_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

fn default_long_term_control_view() -> MemoryLongTermControlView {
    MemoryLongTermControlView::HostUi
}

#[derive(Deserialize)]
struct LongTermListPayload {
    #[serde(default)]
    query: LongTermMemoryQuery,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default = "default_long_term_control_view")]
    view: MemoryLongTermControlView,
}

#[derive(Deserialize)]
struct LongTermDetailPayload {
    target: MemoryLongTermTarget,
    #[serde(default = "default_long_term_control_view")]
    view: MemoryLongTermControlView,
}

#[derive(Deserialize)]
struct LongTermMutationPayload {
    operation: MemoryLongTermMutation,
    reason: String,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    mode_input: RuntimeLifecycleModeInput,
}

#[derive(Deserialize)]
struct LongTermPolicyPayload {
    operation: MemoryGovernancePolicyMutation,
    reason: String,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    mode_input: RuntimeLifecycleModeInput,
}

#[derive(Deserialize)]
struct TranscriptAttrWritePayload {
    memory_space_id: String,
    channel_id: String,
    conversation_id: String,
    #[serde(default)]
    attrs: Vec<TranscriptAttrEnvelope>,
    #[serde(default)]
    idempotency_key: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Deserialize)]
struct ClosePayload {
    #[serde(default)]
    reason: String,
}

const fn default_recover_trigger() -> RuntimeLifecycleTrigger {
    RuntimeLifecycleTrigger::OperatorRequested
}
