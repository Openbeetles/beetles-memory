use bm_sdk::{
    AgentToolRegistryRef, AgentToolUsageFeedback, ContinuitySnapshot, ContinuitySnapshotImportMode,
    IngressKind, LongTermMemoryQuery, MemoryCloseRequest, MemoryExportRequest,
    MemoryGovernancePolicyMutation, MemoryImportRequest, MemoryInspectionRequest,
    MemoryLongTermControlView, MemoryLongTermDetailRequest, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest,
    MemoryLongTermTarget, MemoryMaintenanceRequest, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRecoverRequest, MemoryReplayRequest, MemoryTranscriptAttrWriteRequest,
    MemoryWriteRequest, PressureLevel, Result, RuntimeLifecycleModeInput, RuntimeLifecycleTrigger,
    RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteSource, TranscriptAttrEnvelope,
};
use serde::Deserialize;

use crate::{AdapterCommand, AdapterOperation};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterJsonCommandOptions {
    pub citation: String,
    pub default_source_chat_id: Option<String>,
    pub default_observed_at: u64,
}

impl AdapterJsonCommandOptions {
    pub fn new(citation: impl Into<String>) -> Self {
        Self {
            citation: citation.into(),
            default_source_chat_id: None,
            default_observed_at: 1_800_000_000,
        }
    }

    pub fn with_default_source_chat_id(mut self, chat_id: impl Into<String>) -> Self {
        self.default_source_chat_id = Some(chat_id.into());
        self
    }

    pub const fn with_default_observed_at(mut self, observed_at: u64) -> Self {
        self.default_observed_at = observed_at;
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
                query: payload.query,
                limit: payload.limit.unwrap_or(8),
                tool_registry_refs: payload.tool_registry_refs,
            }))
        }
        AdapterOperation::Project => {
            let payload: ProjectPayload = parse_json(body)?;
            Ok(AdapterCommand::Project(MemoryProjectionRequest {
                user_query: payload.user_query,
                system_max_len: payload.system_max_len.unwrap_or(4096),
                recent_messages_limit: payload.recent_messages_limit.unwrap_or(8),
                pressure: payload.pressure,
                mode_input: payload.mode_input,
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
                system_max_len: payload.system_max_len.unwrap_or(4096),
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
        AdapterOperation::Export => {
            let payload: ExportPayload = parse_json(body)?;
            Ok(AdapterCommand::Export(MemoryExportRequest {
                chat_id: payload.chat_id,
            }))
        }
        AdapterOperation::Import => {
            let payload: ImportPayload = parse_json(body)?;
            Ok(AdapterCommand::Import(Box::new(MemoryImportRequest {
                snapshot: payload.snapshot,
                target_chat_id: payload.target_chat_id,
                mode: payload.mode,
            })))
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
            Ok(AdapterCommand::LongTermMutate(
                MemoryLongTermMutationRequest {
                    operation: payload.operation,
                    reason: payload.reason,
                    dry_run: payload.dry_run.unwrap_or(false),
                    mode_input: payload.mode_input,
                },
            ))
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
    let writes = if payload.writes.is_empty() {
        vec![RuntimeSkillWrite {
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
            observed_at: payload.observed_at.unwrap_or(options.default_observed_at),
        }]
    } else {
        payload.writes
    };
    Ok(AdapterCommand::Write(MemoryWriteRequest::Procedural {
        writes,
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
struct WritePayload {
    #[serde(default)]
    tool_usage_feedback: Option<AgentToolUsageFeedback>,
    #[serde(default)]
    writes: Vec<RuntimeSkillWrite>,
    #[serde(default)]
    source: RuntimeSkillWriteSource,
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
    #[serde(default)]
    observed_at: Option<u64>,
}

#[derive(Deserialize)]
struct RecallPayload {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    tool_registry_refs: Vec<AgentToolRegistryRef>,
}

#[derive(Deserialize)]
struct ProjectPayload {
    #[serde(alias = "query")]
    user_query: String,
    #[serde(default, alias = "max_len")]
    system_max_len: Option<usize>,
    #[serde(default)]
    recent_messages_limit: Option<usize>,
    #[serde(default)]
    pressure: PressureLevel,
    #[serde(default)]
    mode_input: RuntimeLifecycleModeInput,
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
    #[serde(default, alias = "max_len")]
    system_max_len: Option<usize>,
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

#[derive(Deserialize)]
struct ExportPayload {
    chat_id: String,
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
struct ImportPayload {
    snapshot: ContinuitySnapshot,
    target_chat_id: String,
    #[serde(default = "default_import_mode")]
    mode: ContinuitySnapshotImportMode,
}

#[derive(Deserialize)]
struct ClosePayload {
    #[serde(default)]
    reason: String,
}

const fn default_recover_trigger() -> RuntimeLifecycleTrigger {
    RuntimeLifecycleTrigger::OperatorRequested
}

const fn default_import_mode() -> ContinuitySnapshotImportMode {
    ContinuitySnapshotImportMode::FullRestore
}
