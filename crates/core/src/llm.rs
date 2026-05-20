use crate::error::{Error, Result};
use crate::platform::ResponseBody;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub type StreamProgressFn<'a> = &'a mut dyn FnMut(&str, &str);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCallSupport {
    Native,
    PromptGuided,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LlmModelCompat {
    pub tool_call_support: ToolCallSupport,
}

impl LlmModelCompat {
    pub const fn native() -> Self {
        Self {
            tool_call_support: ToolCallSupport::Native,
        }
    }
}

impl Default for LlmModelCompat {
    fn default() -> Self {
        Self::native()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolChoicePolicy {
    Auto,
    Require,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Cow<'static, str>,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters_json: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: String,
}

#[derive(Clone, Debug)]
pub struct LlmResponse {
    pub content: String,
    pub stop_reason: StopReason,
    pub tool_calls: Option<Vec<ToolCall>>,
}

pub trait LlmHttpClient {
    fn do_post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)>;

    fn do_post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        let (status, response) = self.do_post(url, headers, body)?;
        let bytes = response.as_ref();
        if let Some(max_len) = max_response_bytes.filter(|max_len| bytes.len() > *max_len) {
            on_chunk(&bytes[..max_len])?;
            return Err(Error::Other {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "streaming response truncated",
                )),
                stage: crate::constants::HTTP_RESPONSE_TRUNCATED_STAGE,
            });
        }
        on_chunk(bytes)?;
        Ok(status)
    }

    fn reset_connection_for_retry(&mut self) {}
}

pub trait LlmClient: Send + Sync {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        tools: Option<&[ToolSpec]>,
        tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse>;

    fn chat_with_progress(
        &self,
        http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        tools: Option<&[ToolSpec]>,
        tool_choice: ToolChoicePolicy,
        _on_progress: StreamProgressFn,
    ) -> Result<LlmResponse> {
        self.chat(http, system, messages, tools, tool_choice)
    }
}
