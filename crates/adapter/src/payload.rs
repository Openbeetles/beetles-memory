use bm_sdk::{
    AgentToolRegistryRef, CanonicalTurnDelta, IngressKind, LongTermMemoryQuery, MemoryCloseRequest,
    MemoryGovernancePolicyMutation, MemoryInspectionRequest, MemoryLongTermControlView,
    MemoryLongTermDetailRequest, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest, MemoryLongTermTarget,
    MemoryMaintenanceRequest, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRecallTemporalOperation, MemoryRecoverRequest, MemoryReplayRequest,
    MemoryTranscriptAttrWriteRequest, MemoryTurnFinalizeRequest, PostTurnLearningInputV2,
    PressureLevel, ProceduralProjectionBindingV1, QueryFacetInput, Result,
    RuntimeLifecycleModeInput, RuntimeLifecycleTrigger, TranscriptAttrEnvelope,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{AdapterCommand, AdapterOperation};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedAdapterJsonCommandSchema {
    pub field_names: &'static [&'static str],
    /// Top-level transport discovery only. Nested typed payloads are deliberately
    /// shallow here; `decode_json_adapter_command` remains their single strict
    /// serde authority instead of duplicating Core contracts in hand-written JSON Schema.
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
        AdapterOperation::Write => Some(GovernedAdapterJsonCommandSchema {
            field_names: &["kind", "candidates", "extraction", "mutations"],
            input_schema: json!({
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "kind": {"const": "candidates"},
                            "candidates": {"type": "array", "items": {"type": "object"}}
                        },
                        "required": ["kind", "candidates"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "kind": {"const": "long_term_extraction"},
                            "extraction": {"type": "object"}
                        },
                        "required": ["kind", "extraction"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "kind": {"const": "governed_evidence_documents"},
                            "mutations": {"type": "array", "items": {"type": "object"}}
                        },
                        "required": ["kind", "mutations"],
                        "additionalProperties": false
                    }
                ]
            }),
        }),
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
                "binding",
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
                    "binding": {
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {"kind": {"const": "preview"}},
                                "required": ["kind"],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "kind": {"const": "turn"},
                                    "turn_id": {"type": "string", "minLength": 1}
                                },
                                "required": ["kind", "turn_id"],
                                "additionalProperties": false
                            }
                        ]
                    },
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
                "required": ["binding", "temporal_operation", "user_query", "system_max_len"],
                "additionalProperties": false
            }),
        }),
        AdapterOperation::FinalizeTurn => Some(GovernedAdapterJsonCommandSchema {
            field_names: &["turn", "learning", "pressure", "mode_input"],
            input_schema: json!({
                "type": "object",
                "properties": {
                    "turn": {"type": "object"},
                    "learning": {"type": "object"},
                    "pressure": {"type": "string"},
                    "mode_input": {"type": "object"}
                },
                "required": ["turn", "learning"],
                "additionalProperties": false
            }),
        }),
        _ => None,
    }
}

pub fn decode_json_adapter_command(
    operation: AdapterOperation,
    body: &str,
) -> Result<AdapterCommand> {
    match operation {
        AdapterOperation::Capabilities => Ok(AdapterCommand::Capabilities),
        AdapterOperation::Write => parse_json(body).map(AdapterCommand::Write),
        AdapterOperation::FinalizeTurn => {
            let payload: FinalizeTurnPayload = parse_json(body)?;
            Ok(AdapterCommand::FinalizeTurn(Box::new(
                MemoryTurnFinalizeRequest {
                    turn: payload.turn,
                    learning: payload.learning,
                    pressure: payload.pressure,
                    mode_input: payload.mode_input,
                },
            )))
        }
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
                binding: payload.binding,
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

fn parse_json<T: for<'de> Deserialize<'de>>(body: &str) -> Result<T> {
    serde_json::from_str(body)
        .map_err(|err| bm_sdk::Error::config("adapter_json_command", err.to_string()))
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
    binding: ProceduralProjectionBindingV1,
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
#[serde(deny_unknown_fields)]
struct FinalizeTurnPayload {
    turn: CanonicalTurnDelta,
    learning: PostTurnLearningInputV2,
    #[serde(default)]
    pressure: PressureLevel,
    #[serde(default)]
    mode_input: RuntimeLifecycleModeInput,
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
