use crate::EntryGovernanceModelAuthMode;
use crate::{EntryGovernanceModelProbePlan, EntryGovernanceModelProtocol};

pub trait GovernanceModelConnectionProbe: Send + Sync {
    fn probe(
        &self,
        plan: &EntryGovernanceModelProbePlan,
    ) -> bm_sdk::Result<GovernanceModelConnectionReport>;
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GovernanceModelConnectionReport {
    pub status: String,
    pub protocol: EntryGovernanceModelProtocol,
    pub model: String,
    pub credential_used: bool,
    pub response_bytes: usize,
    pub duration_ms: u64,
    pub reason: String,
}

#[cfg(feature = "governance-model-client-std")]
#[derive(Default)]
pub struct ReqwestGovernanceModelConnectionProbe;

#[cfg(feature = "governance-model-client-std")]
impl GovernanceModelConnectionProbe for ReqwestGovernanceModelConnectionProbe {
    fn probe(
        &self,
        plan: &EntryGovernanceModelProbePlan,
    ) -> bm_sdk::Result<GovernanceModelConnectionReport> {
        use std::io::Read as _;

        let started = std::time::Instant::now();
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(plan.request_timeout_ms))
            .build()
            .map_err(|error| bm_sdk::Error::config("governance_model_probe", error.to_string()))?;
        let mut request = client
            .post(&plan.url)
            .header("content-type", "application/json")
            .body(plan.body.clone());
        let credential_used = match &plan.auth_mode {
            EntryGovernanceModelAuthMode::CredentialEnv { credential_env } => {
                let token = std::env::var(credential_env).map_err(|_| {
                    bm_sdk::Error::config(
                        "governance_model_probe",
                        format!("credential environment variable is unset: {credential_env}"),
                    )
                })?;
                request = request.bearer_auth(token);
                true
            }
            EntryGovernanceModelAuthMode::LocalUnauthenticated => false,
        };
        let response = request.send().map_err(|error| {
            bm_sdk::Error::config("governance_model_probe", error.without_url().to_string())
        })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(bm_sdk::Error::http("governance_model_probe", status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > plan.response_max_bytes as u64)
        {
            return Err(bm_sdk::Error::config(
                "governance_model_probe",
                "model probe response exceeds the configured budget",
            ));
        }
        let mut bytes = Vec::new();
        response
            .take((plan.response_max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| bm_sdk::Error::config("governance_model_probe", error.to_string()))?;
        if bytes.len() > plan.response_max_bytes {
            return Err(bm_sdk::Error::config(
                "governance_model_probe",
                "model probe response exceeds the configured budget",
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| bm_sdk::Error::config("governance_model_probe", error.to_string()))?;
        let content = match plan.protocol {
            EntryGovernanceModelProtocol::OpenAiCompatible => value
                .get("choices")
                .and_then(serde_json::Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_str),
            EntryGovernanceModelProtocol::OllamaNative => value
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_str),
        };
        if content.is_none() {
            return Err(bm_sdk::Error::config(
                "governance_model_probe",
                "model probe response does not match the configured protocol",
            ));
        }
        Ok(GovernanceModelConnectionReport {
            status: "ready".to_string(),
            protocol: plan.protocol,
            model: plan.model.clone(),
            credential_used,
            response_bytes: bytes.len(),
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            reason: "model_protocol_probe_succeeded".to_string(),
        })
    }
}

#[cfg(feature = "governance-model-client-std")]
pub struct ReqwestGovernanceLlmHttpClient {
    client: reqwest::blocking::Client,
    response_max_bytes: usize,
}

#[cfg(feature = "governance-model-client-std")]
impl ReqwestGovernanceLlmHttpClient {
    pub fn new(request_timeout_ms: u64, response_max_bytes: usize) -> bm_sdk::Result<Self> {
        if response_max_bytes == 0 {
            return Err(bm_sdk::Error::invalid_input(
                "governance_model_http",
                "response byte budget must be greater than zero",
            ));
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(request_timeout_ms))
            .build()
            .map_err(|error| bm_sdk::Error::config("governance_model_http", error.to_string()))?;
        Ok(Self {
            client,
            response_max_bytes,
        })
    }
}

#[cfg(feature = "governance-model-client-std")]
impl bm_sdk::LlmHttpClient for ReqwestGovernanceLlmHttpClient {
    fn do_post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> bm_sdk::Result<(u16, bm_sdk::ResponseBody)> {
        use std::io::Read as _;

        let mut request = self.client.post(url).body(body.to_vec());
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request.send().map_err(|error| bm_sdk::Error::Other {
            source: Box::new(error.without_url()),
            stage: "governance_model_http",
        })?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|length| length > self.response_max_bytes as u64)
        {
            return Err(bm_sdk::Error::config(
                "governance_model_http",
                "model response exceeds the configured byte budget",
            ));
        }
        let mut bytes = Vec::new();
        response
            .take((self.response_max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| bm_sdk::Error::config("governance_model_http", error.to_string()))?;
        if bytes.len() > self.response_max_bytes {
            return Err(bm_sdk::Error::config(
                "governance_model_http",
                "model response exceeds the configured byte budget",
            ));
        }
        Ok((status, bm_sdk::ResponseBody::Heap(bytes)))
    }
}

#[derive(Clone)]
pub struct ConfiguredGovernanceLlmClient {
    binding: crate::EntryGovernanceModelExecutionBinding,
}

impl ConfiguredGovernanceLlmClient {
    pub fn new(binding: crate::EntryGovernanceModelExecutionBinding) -> Self {
        Self { binding }
    }

    pub fn binding(&self) -> &crate::EntryGovernanceModelExecutionBinding {
        &self.binding
    }
}

impl bm_sdk::LlmClient for ConfiguredGovernanceLlmClient {
    fn chat(
        &self,
        http: &mut dyn bm_sdk::LlmHttpClient,
        system: &str,
        messages: &[bm_sdk::Message],
        _tools: Option<&[bm_sdk::ToolSpec]>,
        _tool_choice: bm_sdk::ToolChoicePolicy,
    ) -> bm_sdk::Result<bm_sdk::LlmResponse> {
        let input_chars = system.chars().count().saturating_add(
            messages
                .iter()
                .map(|message| message.content.chars().count())
                .sum::<usize>(),
        );
        if input_chars > self.binding.max_input_tokens {
            return Err(bm_sdk::Error::invalid_input(
                "governance_model_llm_input",
                "governance model input exceeds the configured token ceiling",
            ));
        }
        let mut request_messages = Vec::with_capacity(messages.len().saturating_add(1));
        if !system.trim().is_empty() {
            request_messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        request_messages.extend(messages.iter().map(|message| {
            serde_json::json!({
                "role": message.role.as_ref(),
                "content": message.content,
            })
        }));
        let (url, body) = match self.binding.protocol {
            EntryGovernanceModelProtocol::OpenAiCompatible => (
                format!(
                    "{}/chat/completions",
                    self.binding.endpoint.trim_end_matches('/')
                ),
                serde_json::json!({
                    "model": self.binding.model,
                    "messages": request_messages,
                    "stream": false,
                    "max_tokens": self.binding.max_output_tokens,
                }),
            ),
            EntryGovernanceModelProtocol::OllamaNative => (
                format!("{}/chat", self.binding.endpoint.trim_end_matches('/')),
                serde_json::json!({
                    "model": self.binding.model,
                    "messages": request_messages,
                    "stream": false,
                    "think": false,
                    "options": {"num_predict": self.binding.max_output_tokens},
                }),
            ),
        };
        let bearer;
        let mut headers = vec![("content-type", "application/json")];
        if let EntryGovernanceModelAuthMode::CredentialEnv { credential_env } =
            &self.binding.auth_mode
        {
            let token = std::env::var(credential_env).map_err(|_| {
                bm_sdk::Error::config(
                    "governance_model_llm",
                    "credential environment variable is unset",
                )
            })?;
            bearer = format!("Bearer {token}");
            headers.push(("authorization", bearer.as_str()));
        }
        let body = serde_json::to_vec(&body)
            .map_err(|error| bm_sdk::Error::config("governance_model_llm", error.to_string()))?;
        let (status, response) = http.do_post(&url, &headers, &body)?;
        if !(200..300).contains(&status) {
            return Err(bm_sdk::Error::http("governance_model_llm", status));
        }
        let value: serde_json::Value = serde_json::from_slice(response.as_ref())
            .map_err(|error| bm_sdk::Error::config("governance_model_llm", error.to_string()))?;
        let (content, stop_reason) = match self.binding.protocol {
            EntryGovernanceModelProtocol::OpenAiCompatible => {
                let choice = value
                    .get("choices")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|choices| choices.first())
                    .ok_or_else(|| {
                        bm_sdk::Error::config(
                            "governance_model_llm",
                            "OpenAI-compatible response is missing choices",
                        )
                    })?;
                let content = choice
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        bm_sdk::Error::config(
                            "governance_model_llm",
                            "OpenAI-compatible response is missing message content",
                        )
                    })?;
                let stop_reason = match choice
                    .get("finish_reason")
                    .and_then(serde_json::Value::as_str)
                {
                    Some("tool_calls") => bm_sdk::StopReason::ToolUse,
                    Some("length") => bm_sdk::StopReason::MaxTokens,
                    Some("stop") | None => bm_sdk::StopReason::EndTurn,
                    Some(_) => bm_sdk::StopReason::Other,
                };
                (content.to_string(), stop_reason)
            }
            EntryGovernanceModelProtocol::OllamaNative => {
                let content = value
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        bm_sdk::Error::config(
                            "governance_model_llm",
                            "Ollama response is missing message content",
                        )
                    })?;
                let stop_reason = match value.get("done_reason").and_then(serde_json::Value::as_str)
                {
                    Some("length") => bm_sdk::StopReason::MaxTokens,
                    Some("stop") | None => bm_sdk::StopReason::EndTurn,
                    Some(_) => bm_sdk::StopReason::Other,
                };
                (content.to_string(), stop_reason)
            }
        };
        Ok(bm_sdk::LlmResponse {
            content,
            stop_reason,
            tool_calls: None,
        })
    }
}
