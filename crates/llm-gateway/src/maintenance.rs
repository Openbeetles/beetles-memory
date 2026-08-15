use std::collections::BTreeMap;
use std::sync::Arc;

use bm_adapter::{
    AdapterCommand, AdapterOperation, AdapterResponse, AdapterSdkReport, TransportKind,
    TransportMode,
};
use bm_entry::EntryRuntime;
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, MaintenanceBudget, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, RuntimeLifecycleModeInput,
    TranscriptInputMessage,
};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::GatewayAuditOutcome;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GatewayInputTranscript {
    pub(crate) latest_user_text: String,
    pub(crate) messages: Vec<TranscriptInputMessage>,
}

pub(crate) struct GatewayMaintenancePlan {
    runtime: Arc<EntryRuntime>,
    user_content: String,
    input_messages: Vec<TranscriptInputMessage>,
    conversation: ConversationScope,
    turn_source: MemoryTurnSource,
    external_content_used: bool,
    runtime_skill_selected_ids: Vec<String>,
    task_learning_selected_ids: Vec<String>,
    pressure: bm_sdk::PressureLevel,
    mode_input: RuntimeLifecycleModeInput,
    budget: MaintenanceBudget,
}

pub(crate) struct GatewayMaintenancePlanInput {
    pub(crate) runtime: Arc<EntryRuntime>,
    pub(crate) user_content: String,
    pub(crate) input_messages: Vec<TranscriptInputMessage>,
    pub(crate) conversation: ConversationScope,
    pub(crate) turn_source: MemoryTurnSource,
    pub(crate) external_content_used: bool,
    pub(crate) runtime_skill_selected_ids: Vec<String>,
    pub(crate) task_learning_selected_ids: Vec<String>,
    pub(crate) pressure: bm_sdk::PressureLevel,
    pub(crate) mode_input: RuntimeLifecycleModeInput,
    pub(crate) budget: MaintenanceBudget,
}

impl GatewayMaintenancePlan {
    pub(crate) fn new(input: GatewayMaintenancePlanInput) -> Self {
        let budget = input.budget;
        Self {
            runtime: input.runtime,
            user_content: bound_text(
                &input.user_content,
                budget.user_input_max_chars,
                budget.user_input_max_bytes,
            ),
            input_messages: input.input_messages,
            conversation: input.conversation,
            turn_source: input.turn_source,
            external_content_used: input.external_content_used,
            runtime_skill_selected_ids: input.runtime_skill_selected_ids,
            task_learning_selected_ids: input.task_learning_selected_ids,
            pressure: input.pressure,
            mode_input: input.mode_input,
            budget,
        }
    }

    pub(crate) fn budget(&self) -> MaintenanceBudget {
        self.budget
    }

    fn task_from_snapshot(&self, snapshot: MaintenanceSnapshot) -> GatewayMaintenanceTask {
        GatewayMaintenanceTask {
            runtime: Arc::clone(&self.runtime),
            request: MemoryTurnFinalizeRequest {
                turn: CanonicalTurnDelta {
                    turn_id: canonical_gateway_turn_id(
                        &self.conversation,
                        &self.turn_source,
                        &self.user_content,
                        &snapshot,
                    ),
                    conversation: self.conversation.clone(),
                    subject: self.runtime.runtime().subject_id().to_string(),
                    delivery_status: snapshot.delivery_status,
                    source: self.turn_source.clone(),
                    actor: None,
                    input_messages: self.input_messages.clone(),
                    assistant_message: if snapshot.reply_content.trim().is_empty() {
                        None
                    } else {
                        Some(TranscriptInputMessage::assistant(snapshot.reply_content))
                    },
                    tool_observations: Vec::new(),
                    external_content_used: self.external_content_used || snapshot.tool_calls > 0,
                    candidate_ids: Vec::new(),
                },
                tool_calls: snapshot.tool_calls,
                runtime_skill_selected_ids: self.runtime_skill_selected_ids.clone(),
                task_learning_selected_ids: self.task_learning_selected_ids.clone(),
                reuse_outcome_note: snapshot.reuse_outcome_note,
                tool_usage_feedback: None,
                pressure: self.pressure,
                mode_input: self.mode_input,
            },
        }
    }
}

fn canonical_gateway_turn_id(
    conversation: &ConversationScope,
    source: &MemoryTurnSource,
    user_content: &str,
    snapshot: &MaintenanceSnapshot,
) -> String {
    if let Some(request_id) = source
        .request_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("gateway-request:{request_id}");
    }
    let mut hasher = Sha256::new();
    hasher.update(b"bm.llm-gateway.maintenance-turn-id.v1\0");
    hash_canonical_field(&mut hasher, "channel", &conversation.channel);
    hash_canonical_field(&mut hasher, "chat_id", &conversation.chat_id);
    hash_canonical_field(
        &mut hasher,
        "protocol",
        memory_turn_protocol_label(source.protocol),
    );
    hash_canonical_field(
        &mut hasher,
        "endpoint",
        source.endpoint.as_deref().unwrap_or_default(),
    );
    hash_canonical_field(
        &mut hasher,
        "model_alias",
        source.model_alias.as_deref().unwrap_or_default(),
    );
    hash_canonical_field(&mut hasher, "user_content", user_content.trim());
    hash_canonical_field(
        &mut hasher,
        "delivery_status",
        memory_turn_delivery_status_label(snapshot.delivery_status),
    );
    hash_canonical_field(&mut hasher, "reply_content", snapshot.reply_content.trim());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    format!("gateway-derived-sha256:{encoded}")
}

fn hash_canonical_field(hasher: &mut Sha256, name: &str, value: &str) {
    hash_length_prefixed(hasher, name.as_bytes());
    hash_length_prefixed(hasher, value.as_bytes());
}

fn hash_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

const fn memory_turn_protocol_label(protocol: MemoryTurnProtocol) -> &'static str {
    match protocol {
        MemoryTurnProtocol::OpenAiChat => "openai_chat",
        MemoryTurnProtocol::OpenAiResponses => "openai_responses",
        MemoryTurnProtocol::OllamaChat => "ollama_chat",
        MemoryTurnProtocol::OllamaGenerate => "ollama_generate",
        MemoryTurnProtocol::Native => "native",
    }
}

const fn memory_turn_delivery_status_label(status: MemoryTurnDeliveryStatus) -> &'static str {
    match status {
        MemoryTurnDeliveryStatus::Delivered => "delivered",
        MemoryTurnDeliveryStatus::UserOnly => "user_only",
        MemoryTurnDeliveryStatus::UpstreamFailed => "upstream_failed",
        MemoryTurnDeliveryStatus::Cancelled => "cancelled",
        MemoryTurnDeliveryStatus::IncompleteStream => "incomplete_stream",
        MemoryTurnDeliveryStatus::RejectedByPolicy => "rejected_by_policy",
    }
}

impl From<GatewayMaintenanceRunOutcome> for GatewayAuditOutcome {
    fn from(value: GatewayMaintenanceRunOutcome) -> Self {
        match value {
            GatewayMaintenanceRunOutcome::Succeeded => Self::Succeeded,
            GatewayMaintenanceRunOutcome::Queued => Self::Queued,
            GatewayMaintenanceRunOutcome::Failed => Self::Failed,
            GatewayMaintenanceRunOutcome::Skipped => Self::Skipped,
        }
    }
}

pub(crate) struct GatewayMaintenanceTask {
    runtime: Arc<EntryRuntime>,
    request: MemoryTurnFinalizeRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatewayMaintenanceRunOutcome {
    Succeeded,
    Queued,
    Failed,
    Skipped,
}

impl GatewayMaintenanceTask {
    pub(crate) fn run(self) -> GatewayMaintenanceRunOutcome {
        let turn_id = self.request.turn.turn_id.clone();
        let request_id = format!("llm-gateway-finalize:{turn_id}");
        let auth = self
            .runtime
            .authenticate_local_transport(bm_entry::EntryLocalTransport::InProcess, "llm-gateway");
        let context = bm_entry::EntryTransportContext::new(
            request_id.clone(),
            TransportKind::Sdk,
            TransportMode::InProcess,
            AdapterOperation::FinalizeTurn,
            "llm-gateway",
            "llm_gateway",
            request_id.clone(),
            format!("audit:{request_id}"),
            auth,
        );
        match self.runtime.handle(
            context,
            AdapterCommand::FinalizeTurn(Box::new(self.request)),
        ) {
            Ok(response) => match response.adapter {
                AdapterResponse::Accepted {
                    report: AdapterSdkReport::FinalizeTurn(report),
                    ..
                } => match report.memory_consolidation.state {
                    bm_sdk::MemoryConsolidationState::Succeeded => {
                        GatewayMaintenanceRunOutcome::Succeeded
                    }
                    bm_sdk::MemoryConsolidationState::Queued => {
                        GatewayMaintenanceRunOutcome::Queued
                    }
                    bm_sdk::MemoryConsolidationState::NotScheduled => {
                        GatewayMaintenanceRunOutcome::Skipped
                    }
                },
                AdapterResponse::Duplicated { .. } => GatewayMaintenanceRunOutcome::Queued,
                AdapterResponse::Rejected { .. } | AdapterResponse::Queued { .. } => {
                    GatewayMaintenanceRunOutcome::Failed
                }
                AdapterResponse::Accepted { .. } => GatewayMaintenanceRunOutcome::Failed,
            },
            Err(_) => GatewayMaintenanceRunOutcome::Failed,
        }
    }
}

pub(crate) struct OpenAiDeferredMaintenance {
    plan: GatewayMaintenancePlan,
    accumulator: OpenAiReplyAccumulator,
}

impl OpenAiDeferredMaintenance {
    pub(crate) fn new(plan: GatewayMaintenancePlan) -> Self {
        let budget = plan.budget;
        Self {
            plan,
            accumulator: OpenAiReplyAccumulator::new(budget),
        }
    }

    pub(crate) fn observe_sse_chunk(&mut self, chunk: &str) {
        self.accumulator.observe_sse_chunk(chunk);
    }

    pub(crate) fn finish(self) -> GatewayMaintenanceRunOutcome {
        self.plan
            .task_from_snapshot(self.accumulator.into_snapshot())
            .run()
    }
}

pub(crate) fn run_json_maintenance(
    plan: GatewayMaintenancePlan,
    body: &Value,
) -> GatewayMaintenanceRunOutcome {
    let mut accumulator = OpenAiReplyAccumulator::new(plan.budget);
    accumulator.observe_json_response(body);
    plan.task_from_snapshot(accumulator.into_snapshot()).run()
}

pub(crate) fn run_text_maintenance(
    plan: GatewayMaintenancePlan,
    reply_content: String,
    tool_calls: u32,
    reuse_outcome_note: String,
) -> GatewayMaintenanceRunOutcome {
    let budget = plan.budget;
    plan.task_from_snapshot(MaintenanceSnapshot {
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        reply_content: bound_text(
            &reply_content,
            budget.reply_input_max_chars,
            budget.reply_input_max_bytes,
        ),
        tool_calls,
        reuse_outcome_note,
    })
    .run()
}

#[derive(Debug)]
struct MaintenanceSnapshot {
    delivery_status: MemoryTurnDeliveryStatus,
    reply_content: String,
    tool_calls: u32,
    reuse_outcome_note: String,
}

struct OpenAiReplyAccumulator {
    reply: BoundedText,
    tool_calls: BTreeMap<(u64, u64), OpenAiToolCallParts>,
    sse_buffer: String,
    sse_event_parts: Vec<String>,
    saw_sse_done: bool,
    observed_sse: bool,
}

impl OpenAiReplyAccumulator {
    fn new(budget: MaintenanceBudget) -> Self {
        Self {
            reply: BoundedText::new(budget.reply_input_max_chars, budget.reply_input_max_bytes),
            tool_calls: BTreeMap::new(),
            sse_buffer: String::new(),
            sse_event_parts: Vec::new(),
            saw_sse_done: false,
            observed_sse: false,
        }
    }

    fn observe_json_response(&mut self, body: &Value) {
        self.observe_responses_json(body);
        let Some(choices) = body.get("choices").and_then(Value::as_array) else {
            return;
        };
        for (choice_index, choice) in choices.iter().enumerate() {
            if let Some(content) = choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
            {
                self.reply.push_str(content);
            }
            if let Some(tool_calls) = choice
                .get("message")
                .and_then(|message| message.get("tool_calls"))
                .and_then(Value::as_array)
            {
                for (tool_index, tool_call) in tool_calls.iter().enumerate() {
                    self.observe_tool_call(choice_index as u64, tool_index as u64, tool_call);
                }
            }
        }
    }

    fn observe_responses_json(&mut self, body: &Value) {
        if let Some(output_text) = body.get("output_text").and_then(Value::as_str) {
            self.reply.push_str(output_text);
            return;
        }
        let Some(output) = body.get("output").and_then(Value::as_array) else {
            return;
        };
        for item in output {
            let Some(content) = item.get("content").and_then(Value::as_array) else {
                continue;
            };
            for content_item in content {
                if content_item.get("type").and_then(Value::as_str) == Some("output_text") {
                    if let Some(text) = content_item.get("text").and_then(Value::as_str) {
                        self.reply.push_str(text);
                    }
                }
            }
        }
    }

    fn observe_sse_chunk(&mut self, chunk: &str) {
        self.observed_sse = true;
        self.sse_buffer.push_str(chunk);
        while let Some(line_end) = self.sse_buffer.find('\n') {
            let line = self.sse_buffer[..line_end]
                .trim_end_matches('\r')
                .to_string();
            self.sse_buffer.drain(..=line_end);
            self.observe_sse_line(&line);
        }
    }

    fn observe_sse_line(&mut self, line: &str) {
        if let Some(data) = line.strip_prefix("data:") {
            self.sse_event_parts.push(data.trim_start().to_string());
        } else if line.trim().is_empty() {
            self.flush_sse_event();
        }
    }

    fn flush_sse_event(&mut self) {
        if self.sse_event_parts.is_empty() {
            return;
        }
        let data = self.sse_event_parts.join("\n");
        self.sse_event_parts.clear();
        if data.trim() == "[DONE]" {
            self.saw_sse_done = true;
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            return;
        };
        self.observe_stream_delta(&value);
    }

    fn observe_stream_delta(&mut self, value: &Value) {
        let Some(choices) = value.get("choices").and_then(Value::as_array) else {
            return;
        };
        for (fallback_choice_index, choice) in choices.iter().enumerate() {
            let choice_index = choice
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or(fallback_choice_index as u64);
            let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
                continue;
            };
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                self.reply.push_str(content);
            }
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for (fallback_tool_index, tool_call) in tool_calls.iter().enumerate() {
                    let tool_index = tool_call
                        .get("index")
                        .and_then(Value::as_u64)
                        .unwrap_or(fallback_tool_index as u64);
                    self.observe_tool_call(choice_index, tool_index, tool_call);
                }
            }
        }
    }

    fn observe_tool_call(&mut self, choice_index: u64, tool_index: u64, tool_call: &Value) {
        let entry = self
            .tool_calls
            .entry((choice_index, tool_index))
            .or_default();
        if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
            entry.id = Some(id.to_string());
        }
        if let Some(kind) = tool_call.get("type").and_then(Value::as_str) {
            entry.kind = Some(kind.to_string());
        }
        if let Some(function) = tool_call.get("function").and_then(Value::as_object) {
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                entry.name.push_str(name);
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                entry.arguments.push_str(arguments);
            }
        }
    }

    fn into_snapshot(mut self) -> MaintenanceSnapshot {
        self.flush_sse_event();
        let tool_calls = self.tool_calls.len() as u32;
        let reuse_outcome_note = if tool_calls == 0 {
            String::new()
        } else {
            format!(
                "openai_tool_calls={tool_calls}; tool_summaries={}",
                self.tool_call_summary()
            )
        };
        MaintenanceSnapshot {
            delivery_status: if self.observed_sse && !self.saw_sse_done {
                MemoryTurnDeliveryStatus::IncompleteStream
            } else {
                MemoryTurnDeliveryStatus::Delivered
            },
            reply_content: self.reply.into_string(),
            tool_calls,
            reuse_outcome_note,
        }
    }

    fn tool_call_summary(&self) -> String {
        self.tool_calls
            .iter()
            .map(|((choice_index, tool_index), parts)| {
                let id = parts.id.as_deref().unwrap_or("unknown");
                let name = if parts.name.trim().is_empty() {
                    "unknown"
                } else {
                    parts.name.trim()
                };
                format!(
                    "choice={choice_index}:tool={tool_index}:id={id}:name={name}:arguments_bytes={}",
                    parts.arguments.len()
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Default)]
struct OpenAiToolCallParts {
    id: Option<String>,
    kind: Option<String>,
    name: String,
    arguments: String,
}

pub(crate) struct BoundedText {
    text: String,
    max_chars: usize,
    max_bytes: usize,
    chars: usize,
}

impl BoundedText {
    pub(crate) fn new(max_chars: usize, max_bytes: usize) -> Self {
        Self {
            text: String::new(),
            max_chars,
            max_bytes,
            chars: 0,
        }
    }

    pub(crate) fn push_str(&mut self, value: &str) {
        for ch in value.chars() {
            if self.chars >= self.max_chars {
                break;
            }
            let ch_len = ch.len_utf8();
            if self.text.len().saturating_add(ch_len) > self.max_bytes {
                break;
            }
            self.text.push(ch);
            self.chars += 1;
        }
    }

    pub(crate) fn into_string(self) -> String {
        self.text.trim().to_string()
    }
}

fn bound_text(value: &str, max_chars: usize, max_bytes: usize) -> String {
    let mut bounded = BoundedText::new(max_chars, max_bytes);
    bounded.push_str(value);
    bounded.into_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fallback_turn_id_fixture(
        conversation: &ConversationScope,
        source: &MemoryTurnSource,
        user_content: &str,
        delivery_status: MemoryTurnDeliveryStatus,
        reply_content: &str,
    ) -> String {
        canonical_gateway_turn_id(
            conversation,
            source,
            user_content,
            &MaintenanceSnapshot {
                delivery_status,
                reply_content: reply_content.to_string(),
                tool_calls: 0,
                reuse_outcome_note: String::new(),
            },
        )
    }

    fn fallback_turn_source() -> MemoryTurnSource {
        MemoryTurnSource {
            ingress: bm_sdk::IngressKind::User,
            channel: "llm.gateway".to_string(),
            provider: Some("openai-compatible".to_string()),
            protocol: MemoryTurnProtocol::OpenAiChat,
            endpoint: Some("/v1/chat/completions".to_string()),
            model_alias: Some("model-alias".to_string()),
            model_resolved: Some("model-resolved".to_string()),
            request_id: None,
            client_conversation_hint: Some("conversation-hint".to_string()),
        }
    }

    fn small_budget() -> MaintenanceBudget {
        MaintenanceBudget {
            user_input_max_chars: 32,
            user_input_max_bytes: 128,
            reply_input_max_chars: 5,
            reply_input_max_bytes: 128,
        }
    }

    #[test]
    fn fallback_turn_id_is_stable_sha256_over_canonical_fields() {
        let conversation = ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "chat-123".to_string(),
            conversation_id: Some("conversation-123".to_string()),
        };
        let source = fallback_turn_source();

        let turn_id = fallback_turn_id_fixture(
            &conversation,
            &source,
            " user input ",
            MemoryTurnDeliveryStatus::Delivered,
            " assistant reply ",
        );

        assert_eq!(
            turn_id,
            "gateway-derived-sha256:a51731db2407fb64140b51a7b091e768b9f21ede9905aa2e0bb447726d98fc0f"
        );
        assert_eq!(
            turn_id,
            fallback_turn_id_fixture(
                &conversation,
                &source,
                " user input ",
                MemoryTurnDeliveryStatus::Delivered,
                " assistant reply ",
            )
        );
    }

    #[test]
    fn fallback_turn_id_changes_for_each_canonical_field() {
        let conversation = ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "chat-123".to_string(),
            conversation_id: None,
        };
        let source = fallback_turn_source();
        let baseline = fallback_turn_id_fixture(
            &conversation,
            &source,
            "user input",
            MemoryTurnDeliveryStatus::Delivered,
            "assistant reply",
        );

        let changed_conversation = ConversationScope {
            channel: "llm.gateway.changed".to_string(),
            ..conversation.clone()
        };
        assert_ne!(
            baseline,
            fallback_turn_id_fixture(
                &changed_conversation,
                &source,
                "user input",
                MemoryTurnDeliveryStatus::Delivered,
                "assistant reply",
            )
        );
        let changed_conversation = ConversationScope {
            chat_id: "chat-456".to_string(),
            ..conversation.clone()
        };
        assert_ne!(
            baseline,
            fallback_turn_id_fixture(
                &changed_conversation,
                &source,
                "user input",
                MemoryTurnDeliveryStatus::Delivered,
                "assistant reply",
            )
        );

        for changed_source in [
            MemoryTurnSource {
                protocol: MemoryTurnProtocol::OpenAiResponses,
                ..source.clone()
            },
            MemoryTurnSource {
                endpoint: Some("/v1/responses".to_string()),
                ..source.clone()
            },
            MemoryTurnSource {
                model_alias: Some("other-model".to_string()),
                ..source.clone()
            },
        ] {
            assert_ne!(
                baseline,
                fallback_turn_id_fixture(
                    &conversation,
                    &changed_source,
                    "user input",
                    MemoryTurnDeliveryStatus::Delivered,
                    "assistant reply",
                )
            );
        }
        assert_ne!(
            baseline,
            fallback_turn_id_fixture(
                &conversation,
                &source,
                "different input",
                MemoryTurnDeliveryStatus::Delivered,
                "assistant reply",
            )
        );
        assert_ne!(
            baseline,
            fallback_turn_id_fixture(
                &conversation,
                &source,
                "user input",
                MemoryTurnDeliveryStatus::IncompleteStream,
                "assistant reply",
            )
        );
        assert_ne!(
            baseline,
            fallback_turn_id_fixture(
                &conversation,
                &source,
                "user input",
                MemoryTurnDeliveryStatus::Delivered,
                "different reply",
            )
        );
    }

    #[test]
    fn protocol_and_delivery_status_labels_are_stable() {
        assert_eq!(
            memory_turn_protocol_label(MemoryTurnProtocol::OpenAiChat),
            "openai_chat"
        );
        assert_eq!(
            memory_turn_protocol_label(MemoryTurnProtocol::OpenAiResponses),
            "openai_responses"
        );
        assert_eq!(
            memory_turn_protocol_label(MemoryTurnProtocol::OllamaChat),
            "ollama_chat"
        );
        assert_eq!(
            memory_turn_protocol_label(MemoryTurnProtocol::OllamaGenerate),
            "ollama_generate"
        );
        assert_eq!(
            memory_turn_protocol_label(MemoryTurnProtocol::Native),
            "native"
        );

        assert_eq!(
            memory_turn_delivery_status_label(MemoryTurnDeliveryStatus::Delivered),
            "delivered"
        );
        assert_eq!(
            memory_turn_delivery_status_label(MemoryTurnDeliveryStatus::UserOnly),
            "user_only"
        );
        assert_eq!(
            memory_turn_delivery_status_label(MemoryTurnDeliveryStatus::UpstreamFailed),
            "upstream_failed"
        );
        assert_eq!(
            memory_turn_delivery_status_label(MemoryTurnDeliveryStatus::Cancelled),
            "cancelled"
        );
        assert_eq!(
            memory_turn_delivery_status_label(MemoryTurnDeliveryStatus::IncompleteStream),
            "incomplete_stream"
        );
        assert_eq!(
            memory_turn_delivery_status_label(MemoryTurnDeliveryStatus::RejectedByPolicy),
            "rejected_by_policy"
        );
    }

    #[test]
    fn sse_accumulator_keeps_passthrough_bounded_reply_and_tool_call_count() {
        let mut accumulator = OpenAiReplyAccumulator::new(small_budget());

        accumulator.observe_sse_chunk(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hel\"}}]}\n\n",
        );
        accumulator.observe_sse_chunk(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo!\",\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"query\\\"\"}}]}}]}\n\n",
        );
        accumulator.observe_sse_chunk(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"release\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        );
        accumulator.observe_sse_chunk("data: [DONE]\n\n");

        let snapshot = accumulator.into_snapshot();
        assert_eq!(snapshot.reply_content, "hello");
        assert_eq!(snapshot.tool_calls, 1);
        assert!(snapshot.reuse_outcome_note.contains("openai_tool_calls=1"));
        assert!(snapshot.reuse_outcome_note.contains("call_1"));
        assert!(snapshot.reuse_outcome_note.contains("lookup"));
        assert!(snapshot.reuse_outcome_note.contains("arguments_bytes="));
        assert!(!snapshot.reuse_outcome_note.contains("release"));
    }

    #[test]
    fn sse_accumulator_keeps_partial_events_until_json_is_complete() {
        let mut accumulator = OpenAiReplyAccumulator::new(small_budget());

        accumulator
            .observe_sse_chunk("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"he");
        accumulator.observe_sse_chunk("llo\"}}]}\n\n");

        let snapshot = accumulator.into_snapshot();
        assert_eq!(snapshot.reply_content, "hello");
    }

    #[test]
    fn json_accumulator_counts_non_streaming_tool_calls_without_raw_arguments() {
        let mut accumulator = OpenAiReplyAccumulator::new(small_budget());

        accumulator.observe_json_response(&json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "done",
                    "tool_calls": [
                        { "id": "call_a", "type": "function", "function": { "name": "a", "arguments": "{\"x\":1}" } },
                        { "id": "call_b", "type": "function", "function": { "name": "b", "arguments": "{\"y\":2}" } }
                    ]
                }
            }]
        }));

        let snapshot = accumulator.into_snapshot();
        assert_eq!(snapshot.reply_content, "done");
        assert_eq!(snapshot.tool_calls, 2);
        assert!(snapshot.reuse_outcome_note.contains("openai_tool_calls=2"));
        assert!(snapshot.reuse_outcome_note.contains("call_a"));
        assert!(snapshot.reuse_outcome_note.contains("call_b"));
        assert!(!snapshot.reuse_outcome_note.contains("\"x\""));
    }
}
