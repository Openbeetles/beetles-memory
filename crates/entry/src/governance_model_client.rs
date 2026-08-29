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
        let started = std::time::Instant::now();
        let mut http = ReqwestGovernanceLlmHttpClient::for_endpoint(
            &plan.url,
            plan.request_timeout_ms,
            plan.response_max_bytes,
        )?;
        let bearer = match &plan.auth_mode {
            EntryGovernanceModelAuthMode::CredentialEnv { credential_env } => {
                let token = std::env::var(credential_env).map_err(|_| {
                    bm_sdk::Error::config(
                        "governance_model_probe",
                        format!("credential environment variable is unset: {credential_env}"),
                    )
                })?;
                Some(format!("Bearer {token}"))
            }
            EntryGovernanceModelAuthMode::LocalUnauthenticated => None,
        };
        let mut headers = vec![("content-type", "application/json")];
        if let Some(bearer) = bearer.as_deref() {
            headers.push(("authorization", bearer));
        }
        let (status, response) =
            bm_sdk::LlmHttpClient::do_post(&mut http, &plan.url, &headers, &plan.body)?;
        if !(200..300).contains(&status) {
            return Err(bm_sdk::Error::http("governance_model_probe", status));
        }
        let bytes = response.as_ref();
        let value: serde_json::Value = serde_json::from_slice(bytes)
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
            credential_used: bearer.is_some(),
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
    expected_origin: Option<(String, String, u16)>,
}

#[cfg(feature = "governance-model-client-std")]
impl ReqwestGovernanceLlmHttpClient {
    pub fn new(request_timeout_ms: u64, response_max_bytes: usize) -> bm_sdk::Result<Self> {
        Self::build(request_timeout_ms, response_max_bytes, None)
    }

    pub fn for_endpoint(
        endpoint: &str,
        request_timeout_ms: u64,
        response_max_bytes: usize,
    ) -> bm_sdk::Result<Self> {
        let endpoint = url::Url::parse(endpoint).map_err(|error| {
            bm_sdk::Error::invalid_input("governance_model_http", error.to_string())
        })?;
        let expected_origin = exact_origin(&endpoint)?;
        Self::build(
            request_timeout_ms,
            response_max_bytes,
            Some(expected_origin),
        )
    }

    fn build(
        request_timeout_ms: u64,
        response_max_bytes: usize,
        expected_origin: Option<(String, String, u16)>,
    ) -> bm_sdk::Result<Self> {
        if response_max_bytes == 0 {
            return Err(bm_sdk::Error::invalid_input(
                "governance_model_http",
                "response byte budget must be greater than zero",
            ));
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(request_timeout_ms))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .tls_backend_rustls()
            .build()
            .map_err(|error| bm_sdk::Error::config("governance_model_http", error.to_string()))?;
        Ok(Self {
            client,
            response_max_bytes,
            expected_origin,
        })
    }
}

#[cfg(feature = "governance-model-client-std")]
fn exact_origin(url: &url::Url) -> bm_sdk::Result<(String, String, u16)> {
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bm_sdk::Error::invalid_input(
            "governance_model_http",
            "governance endpoint must not contain userinfo, query, or fragment",
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        bm_sdk::Error::invalid_input(
            "governance_model_http",
            "governance endpoint host is missing",
        )
    })?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(bm_sdk::Error::invalid_input(
            "governance_model_http",
            "non-loopback governance endpoint must use HTTPS",
        ));
    }
    let port = url.port_or_known_default().ok_or_else(|| {
        bm_sdk::Error::invalid_input(
            "governance_model_http",
            "governance endpoint port is missing",
        )
    })?;
    Ok((url.scheme().to_string(), host.to_ascii_lowercase(), port))
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

        let target = url::Url::parse(url).map_err(|error| {
            bm_sdk::Error::invalid_input("governance_model_http", error.to_string())
        })?;
        let target_origin = exact_origin(&target)?;
        if self
            .expected_origin
            .as_ref()
            .is_some_and(|expected| expected != &target_origin)
        {
            return Err(bm_sdk::Error::conflict(
                "governance_model_http",
                "governance request origin differs from the immutable binding",
            ));
        }

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
