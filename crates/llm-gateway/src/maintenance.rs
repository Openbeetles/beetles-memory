use std::collections::BTreeMap;
use std::sync::Arc;

use bm_entry::EntryRuntime;
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse,
    MaintenanceBudget, MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnSource,
    Message, RuntimeLifecycleModeInput, StopReason, ToolChoicePolicy, ToolSpec,
    TranscriptInputMessage,
};
use serde_json::{json, Value};

use crate::{GatewayAuditOutcome, GatewayMaintenanceConfig, GatewayProviderConfig};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GatewayInputTranscript {
    pub(crate) latest_user_text: String,
    pub(crate) messages: Vec<TranscriptInputMessage>,
}

pub struct OpenAiGatewayServices<'a> {
    maintenance_http: Option<&'a mut dyn LlmHttpClient>,
    maintenance_llm: Option<&'a (dyn LlmClient + Send + Sync)>,
}

impl<'a> OpenAiGatewayServices<'a> {
    pub const fn new() -> Self {
        Self {
            maintenance_http: None,
            maintenance_llm: None,
        }
    }

    pub fn with_maintenance(
        mut self,
        http: &'a mut dyn LlmHttpClient,
        llm: &'a (dyn LlmClient + Send + Sync),
    ) -> Self {
        self.maintenance_http = Some(http);
        self.maintenance_llm = Some(llm);
        self
    }
}

impl Default for OpenAiGatewayServices<'_> {
    fn default() -> Self {
        Self::new()
    }
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
    config: GatewayMaintenanceConfig,
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
    pub(crate) config: GatewayMaintenanceConfig,
}

impl GatewayMaintenancePlan {
    pub(crate) fn new(input: GatewayMaintenancePlanInput) -> Self {
        let budget = input.runtime.runtime().runtime_budget().maintenance_budget;
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
            config: input.config,
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
                pressure: self.pressure,
                mode_input: self.mode_input,
            },
            enabled: self.config.enabled,
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
    let seed = format!(
        "{}\n{}\n{:?}\n{}\n{}\n{}\n{:?}\n{}",
        conversation.channel,
        conversation.chat_id,
        source.protocol,
        source.endpoint.as_deref().unwrap_or_default(),
        source.model_alias.as_deref().unwrap_or_default(),
        user_content.trim(),
        snapshot.delivery_status,
        snapshot.reply_content.trim()
    );
    format!("gateway-derived-{:016x}", fnv1a64(seed.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl From<GatewayMaintenanceRunOutcome> for GatewayAuditOutcome {
    fn from(value: GatewayMaintenanceRunOutcome) -> Self {
        match value {
            GatewayMaintenanceRunOutcome::Succeeded => Self::Succeeded,
            GatewayMaintenanceRunOutcome::Failed => Self::Failed,
            GatewayMaintenanceRunOutcome::Skipped => Self::Skipped,
            GatewayMaintenanceRunOutcome::NotExecuted => Self::NotExecuted,
        }
    }
}

pub(crate) struct GatewayMaintenanceTask {
    runtime: Arc<EntryRuntime>,
    request: MemoryTurnFinalizeRequest,
    enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatewayMaintenanceRunOutcome {
    Succeeded,
    Failed,
    Skipped,
    NotExecuted,
}

impl GatewayMaintenanceTask {
    pub(crate) fn run(
        self,
        services: &mut OpenAiGatewayServices<'_>,
    ) -> GatewayMaintenanceRunOutcome {
        let missing_services = self.enabled
            && (services.maintenance_http.is_none() || services.maintenance_llm.is_none());
        let http = if self.enabled {
            services.maintenance_http.as_deref_mut()
        } else {
            None
        };
        let llm = if self.enabled {
            services.maintenance_llm
        } else {
            None
        };
        match self
            .runtime
            .runtime()
            .finalize_turn_and_maintain(http, llm, self.request)
        {
            Ok(report) if report.maintenance.is_some() => GatewayMaintenanceRunOutcome::Succeeded,
            Ok(_) if !self.enabled => GatewayMaintenanceRunOutcome::Skipped,
            Ok(_) if missing_services => GatewayMaintenanceRunOutcome::NotExecuted,
            Ok(_) => GatewayMaintenanceRunOutcome::Skipped,
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

    pub(crate) fn finish(
        self,
        services: &mut OpenAiGatewayServices<'_>,
    ) -> GatewayMaintenanceRunOutcome {
        self.plan
            .task_from_snapshot(self.accumulator.into_snapshot())
            .run(services)
    }
}

pub(crate) fn run_json_maintenance(
    plan: GatewayMaintenancePlan,
    body: &Value,
    services: &mut OpenAiGatewayServices<'_>,
) -> GatewayMaintenanceRunOutcome {
    let mut accumulator = OpenAiReplyAccumulator::new(plan.budget);
    accumulator.observe_json_response(body);
    plan.task_from_snapshot(accumulator.into_snapshot())
        .run(services)
}

pub(crate) fn run_text_maintenance(
    plan: GatewayMaintenancePlan,
    reply_content: String,
    tool_calls: u32,
    reuse_outcome_note: String,
    services: &mut OpenAiGatewayServices<'_>,
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
    .run(services)
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

pub struct OpenAiMaintenanceLlmClient {
    provider: GatewayProviderConfig,
    model: String,
}

impl OpenAiMaintenanceLlmClient {
    pub fn new(provider: GatewayProviderConfig, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

impl LlmClient for OpenAiMaintenanceLlmClient {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> bm_sdk::Result<LlmResponse> {
        let mut openai_messages = Vec::new();
        if !system.trim().is_empty() {
            openai_messages.push(json!({
                "role": "system",
                "content": system,
            }));
        }
        openai_messages.extend(messages.iter().map(|message| {
            json!({
                "role": message.role.as_ref(),
                "content": message.content,
            })
        }));
        let body = json!({
            "model": self.model,
            "messages": openai_messages,
            "stream": false,
        })
        .to_string();
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        let bearer;
        if let Some(env_name) = self.provider.secret_env_name() {
            let token = std::env::var(env_name).map_err(|_| {
                bm_sdk::Error::config("openai_maintenance_llm", "provider api key env is unset")
            })?;
            bearer = format!("Bearer {token}");
            headers.push(("authorization".to_string(), bearer));
        }
        let header_refs = headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let (status, response) = http.do_post(
            &format!(
                "{}/chat/completions",
                self.provider.base_url.trim_end_matches('/')
            ),
            &header_refs,
            body.as_bytes(),
        )?;
        if !(200..300).contains(&status) {
            return Err(bm_sdk::Error::http("openai_maintenance_llm", status));
        }
        let value: Value = serde_json::from_slice(response.as_ref())
            .map_err(|error| bm_sdk::Error::config("openai_maintenance_llm", error.to_string()))?;
        let choice = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .ok_or_else(|| {
                bm_sdk::Error::config("openai_maintenance_llm", "missing choices in response")
            })?;
        let content = choice
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let stop_reason = match choice.get("finish_reason").and_then(Value::as_str) {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            Some("stop") | None => StopReason::EndTurn,
            Some(_) => StopReason::Other,
        };
        Ok(LlmResponse {
            content,
            stop_reason,
            tool_calls: None,
        })
    }
}

#[cfg(feature = "client-reqwest")]
pub struct ReqwestGatewayLlmHttpClient {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "client-reqwest")]
impl ReqwestGatewayLlmHttpClient {
    pub fn new() -> bm_sdk::Result<Self> {
        Self::new_with_timeout(std::time::Duration::from_secs(600))
    }

    pub fn new_with_timeout(timeout: std::time::Duration) -> bm_sdk::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| bm_sdk::Error::config("gateway_llm_http_client", error.to_string()))?;
        Ok(Self { client })
    }
}

#[cfg(feature = "client-reqwest")]
impl LlmHttpClient for ReqwestGatewayLlmHttpClient {
    fn do_post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> bm_sdk::Result<(u16, bm_sdk::ResponseBody)> {
        let mut request = self.client.post(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request
            .body(body.to_vec())
            .send()
            .map_err(|error| bm_sdk::Error::config("gateway_llm_http_client", error.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|error| bm_sdk::Error::config("gateway_llm_http_client", error.to_string()))?;
        Ok((status, bm_sdk::ResponseBody::Heap(body.into_bytes())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_budget() -> MaintenanceBudget {
        MaintenanceBudget {
            user_input_max_chars: 32,
            user_input_max_bytes: 128,
            reply_input_max_chars: 5,
            reply_input_max_bytes: 128,
        }
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
