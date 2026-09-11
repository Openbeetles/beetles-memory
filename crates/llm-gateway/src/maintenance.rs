use std::sync::Arc;

use bm_adapter::{
    AdapterCommand, AdapterOperation, AdapterResponse, AdapterSdkReport, TransportKind,
    TransportMode,
};
use bm_entry::EntryRuntime;
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, MaintenanceBudget, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, PostTurnLearningInputV1,
    ProceduralFeedbackAuthorityInputV1, ProceduralSelectionReceiptV1, RuntimeLifecycleModeInput,
    TranscriptInputMessage,
};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{GatewayAuditOutcome, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GatewayInputTranscript {
    pub(crate) latest_user_text: String,
    pub(crate) messages: Vec<TranscriptInputMessage>,
}

impl GatewayInputTranscript {
    pub(crate) fn from_protocol_window(messages: &[TranscriptInputMessage]) -> Self {
        let messages = bm_sdk::protocol_window_user_delta(messages);
        let latest_user_text = messages
            .last()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        Self {
            messages,
            latest_user_text,
        }
    }
}

pub(crate) struct GatewayMaintenancePlan {
    runtime: Arc<EntryRuntime>,
    input_messages: Vec<TranscriptInputMessage>,
    conversation: ConversationScope,
    turn_source: MemoryTurnSource,
    turn_id: String,
    external_content_used: bool,
    selection_receipt: Option<ProceduralSelectionReceiptV1>,
    pressure: bm_sdk::PressureLevel,
    mode_input: RuntimeLifecycleModeInput,
    budget: MaintenanceBudget,
}

pub(crate) struct GatewayMaintenancePlanInput {
    pub(crate) runtime: Arc<EntryRuntime>,
    pub(crate) input_messages: Vec<TranscriptInputMessage>,
    pub(crate) conversation: ConversationScope,
    pub(crate) turn_source: MemoryTurnSource,
    pub(crate) turn_id: String,
    pub(crate) external_content_used: bool,
    pub(crate) selection_receipt: Option<ProceduralSelectionReceiptV1>,
    pub(crate) pressure: bm_sdk::PressureLevel,
    pub(crate) mode_input: RuntimeLifecycleModeInput,
    pub(crate) budget: MaintenanceBudget,
}

impl GatewayMaintenancePlan {
    pub(crate) fn new(input: GatewayMaintenancePlanInput) -> Self {
        let budget = input.budget;
        Self {
            runtime: input.runtime,
            input_messages: input.input_messages,
            conversation: input.conversation,
            turn_source: input.turn_source,
            turn_id: input.turn_id,
            external_content_used: input.external_content_used,
            selection_receipt: input.selection_receipt,
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
                    turn_id: self.turn_id.clone(),
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
                    external_content_used: self.external_content_used,
                    candidate_ids: Vec::new(),
                },
                learning: PostTurnLearningInputV1 {
                    // Model tool-call proposals are passthrough, not executed observations.
                    tool_call_count: 0,
                    selection_receipt: self.selection_receipt.clone(),
                    runtime_skill_feedback: Vec::new(),
                    agent_skill_feedback: Vec::new(),
                    task_learning_feedback: Vec::new(),
                    agent_tool_feedback: Vec::new(),
                    authority: ProceduralFeedbackAuthorityInputV1::HostRuntimeObservation,
                },
                pressure: self.pressure,
                mode_input: self.mode_input,
            },
        }
    }
}

pub(crate) fn ensure_gateway_request_id(request_id: &mut Option<String>) -> Result<String> {
    if let Some(existing) = request_id.as_deref() {
        if existing.is_empty()
            || existing != existing.trim()
            || existing.chars().any(char::is_control)
        {
            return Err(crate::GatewayError::invalid_request(
                "x-request-id must be a canonical non-empty value",
            ));
        }
        return Ok(existing.to_string());
    }

    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| {
        crate::GatewayError::runtime_unavailable("gateway request identity authority unavailable")
    })?;
    let mut encoded = String::with_capacity(random.len() * 2);
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    let generated = format!("generated-{encoded}");
    *request_id = Some(generated.clone());
    Ok(generated)
}

pub(crate) fn canonical_gateway_turn_id(
    conversation: &ConversationScope,
    source: &MemoryTurnSource,
) -> Result<String> {
    let request_id = source
        .request_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            crate::GatewayError::invalid_request(
                "gateway execution turn requires a request identity",
            )
        })?;
    let mut hasher = Sha256::new();
    hasher.update(b"bm.llm-gateway.execution-turn-id.v2\0");
    hash_canonical_field(&mut hasher, "channel", &conversation.channel);
    hash_canonical_field(&mut hasher, "chat_id", &conversation.chat_id);
    hash_canonical_field(
        &mut hasher,
        "conversation_id",
        conversation
            .conversation_id
            .as_deref()
            .unwrap_or(&conversation.chat_id),
    );
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
    hash_canonical_field(&mut hasher, "request_id", request_id);
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(format!("gateway-request-sha256:{encoded}"))
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
                AdapterResponse::Replayed { .. } => GatewayMaintenanceRunOutcome::Queued,
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
) -> GatewayMaintenanceRunOutcome {
    let budget = plan.budget;
    plan.task_from_snapshot(MaintenanceSnapshot {
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        reply_content: bound_text(
            &reply_content,
            budget.reply_input_max_chars,
            budget.reply_input_max_bytes,
        ),
    })
    .run()
}

#[derive(Debug)]
struct MaintenanceSnapshot {
    delivery_status: MemoryTurnDeliveryStatus,
    reply_content: String,
}

struct OpenAiReplyAccumulator {
    reply: BoundedText,
    sse_buffer: String,
    sse_event_parts: Vec<String>,
    saw_sse_done: bool,
    observed_sse: bool,
}

impl OpenAiReplyAccumulator {
    fn new(budget: MaintenanceBudget) -> Self {
        Self {
            reply: BoundedText::new(budget.reply_input_max_chars, budget.reply_input_max_bytes),
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
        for choice in choices {
            if let Some(content) = choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
            {
                self.reply.push_str(content);
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
        for choice in choices {
            let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
                continue;
            };
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                self.reply.push_str(content);
            }
        }
    }

    fn into_snapshot(mut self) -> MaintenanceSnapshot {
        self.flush_sse_event();
        MaintenanceSnapshot {
            delivery_status: if self.observed_sse && !self.saw_sse_done {
                MemoryTurnDeliveryStatus::IncompleteStream
            } else {
                MemoryTurnDeliveryStatus::Delivered
            },
            reply_content: self.reply.into_string(),
        }
    }
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

    fn turn_source(request_id: &str) -> MemoryTurnSource {
        MemoryTurnSource {
            ingress: bm_sdk::IngressKind::User,
            channel: "llm.gateway".to_string(),
            provider: Some("openai-compatible".to_string()),
            protocol: MemoryTurnProtocol::OpenAiChat,
            endpoint: Some("/v1/chat/completions".to_string()),
            model_alias: Some("model-alias".to_string()),
            model_resolved: Some("model-resolved".to_string()),
            request_id: Some(request_id.to_string()),
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
    fn request_identity_authority_preserves_explicit_and_mints_distinct_missing_ids() {
        let mut explicit = Some("request-123".to_string());
        assert_eq!(
            ensure_gateway_request_id(&mut explicit).expect("explicit request id"),
            "request-123"
        );

        let mut first = None;
        let mut second = None;
        let first = ensure_gateway_request_id(&mut first).expect("first generated request id");
        let second = ensure_gateway_request_id(&mut second).expect("second generated request id");
        assert!(first.starts_with("generated-"));
        assert!(second.starts_with("generated-"));
        assert_ne!(first, second);
    }

    #[test]
    fn execution_turn_id_is_stable_for_retry_and_bound_to_scope_and_protocol() {
        let conversation = ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "chat-123".to_string(),
            conversation_id: Some("conversation-123".to_string()),
        };
        let source = turn_source("request-123");
        let turn_id = canonical_gateway_turn_id(&conversation, &source).expect("turn id");
        assert!(turn_id.starts_with("gateway-request-sha256:"));
        assert_eq!(
            turn_id,
            canonical_gateway_turn_id(&conversation, &source).expect("retry turn id")
        );

        let changed_conversation = ConversationScope {
            channel: "llm.gateway.changed".to_string(),
            ..conversation.clone()
        };
        assert_ne!(
            turn_id,
            canonical_gateway_turn_id(&changed_conversation, &source).expect("scope-bound turn id")
        );
        let changed_source = MemoryTurnSource {
            protocol: MemoryTurnProtocol::OpenAiResponses,
            endpoint: Some("/v1/responses".to_string()),
            ..source.clone()
        };
        assert_ne!(
            turn_id,
            canonical_gateway_turn_id(&conversation, &changed_source)
                .expect("protocol-bound turn id")
        );
    }

    #[test]
    fn protocol_labels_are_stable() {
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
    }

    #[test]
    fn sse_accumulator_retains_bounded_reply_without_interpreting_tool_proposals() {
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
        assert_eq!(
            snapshot.delivery_status,
            MemoryTurnDeliveryStatus::Delivered
        );
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
    fn json_accumulator_retains_reply_without_collecting_tool_arguments() {
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
        assert_eq!(
            snapshot.delivery_status,
            MemoryTurnDeliveryStatus::Delivered
        );
    }
}
