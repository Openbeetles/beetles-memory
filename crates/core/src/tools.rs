use crate::error::{Error, Result};
use crate::memory::{
    LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryKind, LongTermMemorySourceScope,
    LongTermMemorySourceType, LongTermMemoryStore, MemoryStore, SessionStore,
};
use crate::platform::{ResponseBody, SkillStorage};
use std::collections::HashMap;
use std::sync::Arc;

pub struct ToolRegistry {
    names: HashMap<String, ()>,
}

impl ToolRegistry {
    pub fn new(names: impl IntoIterator<Item = String>) -> Self {
        Self {
            names: names.into_iter().map(|name| (name, ())).collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<()> {
        self.names.get(name).copied()
    }
}

pub trait ToolContext {
    fn get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)>;

    fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)>;

    fn user_locale(&self) -> crate::i18n::Locale {
        crate::i18n::Locale::Zh
    }
}

pub trait Tool {
    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String>;
}

pub struct MemoryManageTool {
    #[allow(dead_code)]
    memory: Arc<dyn MemoryStore>,
    #[allow(dead_code)]
    long_term: Arc<dyn LongTermMemoryStore>,
    #[allow(dead_code)]
    skills: Arc<dyn SkillStorage>,
}

impl MemoryManageTool {
    pub fn new(
        memory: Arc<dyn MemoryStore>,
        long_term: Arc<dyn LongTermMemoryStore>,
        skills: Arc<dyn SkillStorage>,
    ) -> Self {
        Self {
            memory,
            long_term,
            skills,
        }
    }
}

impl Tool for MemoryManageTool {
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let value = serde_json::from_str::<serde_json::Value>(args)
            .map_err(|error| Error::config("memory_tool_args", error.to_string()))?;
        let op = value.get("op").and_then(|v| v.as_str()).unwrap_or_default();
        let mut changed = 0usize;
        let mut deleted = false;
        let plane = if op.contains("skill") {
            "skill"
        } else if op.contains("long_term") {
            "factual"
        } else {
            "memory"
        };
        match op {
            "upsert_long_term" => {
                let kind = parse_json_enum::<LongTermMemoryKind>(
                    value.get("kind").and_then(|v| v.as_str()).unwrap_or("fact"),
                    "long_term_kind",
                )?;
                let confidence = value
                    .get("confidence")
                    .and_then(|v| v.as_str())
                    .map(|raw| parse_json_enum::<LongTermMemoryConfidence>(raw, "confidence"))
                    .transpose()?;
                let source_scope = value
                    .get("source_scope")
                    .and_then(|v| v.as_str())
                    .map(|raw| parse_json_enum::<LongTermMemorySourceScope>(raw, "source_scope"))
                    .transpose()?;
                let draft = LongTermMemoryDraft {
                    kind,
                    topic: required_string(&value, "topic")?,
                    content: required_string(&value, "content")?,
                    keywords: value
                        .get("keywords")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default(),
                    source_chat_id: value
                        .get("source_chat_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string),
                    source_type: Some(LongTermMemorySourceType::ManualTool),
                    source_scope,
                    confidence,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(crate::util::current_unix_secs()),
                    last_confirmed_at: Some(crate::util::current_unix_secs()),
                    source_revision: None,
                };
                changed = self
                    .long_term
                    .upsert_many(&[draft], crate::util::current_unix_secs())?;
            }
            "delete_long_term" => {
                deleted = self.long_term.delete(&required_string(&value, "id")?)?;
                changed = usize::from(deleted);
            }
            "set_memory" => {
                self.memory
                    .set_memory(&required_string(&value, "content")?)?;
                changed = 1;
            }
            _ => {}
        }
        Ok(serde_json::json!({
            "ok": true,
            "plane": plane,
            "changed": changed,
            "deleted": deleted
        })
        .to_string())
    }
}

pub struct SessionManageTool {
    #[allow(dead_code)]
    session: Arc<dyn SessionStore>,
}

impl SessionManageTool {
    pub fn new(session: Arc<dyn SessionStore>) -> Self {
        Self { session }
    }
}

impl Tool for SessionManageTool {
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let value = serde_json::from_str::<serde_json::Value>(args)
            .map_err(|error| Error::config("session_tool_args", error.to_string()))?;
        let op = value.get("op").and_then(|v| v.as_str()).unwrap_or_default();
        let mut changed = 0usize;
        if op == "delete" || op == "clear" {
            self.session.delete(&required_string(&value, "chat_id")?)?;
            changed = 1;
        }
        Ok(serde_json::json!({ "ok": true, "changed": changed }).to_string())
    }
}

fn required_string(value: &serde_json::Value, key: &'static str) -> Result<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| Error::config("tool_args", format!("missing required field: {key}")))
}

fn parse_json_enum<T>(raw: &str, stage: &'static str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(serde_json::Value::String(raw.to_string()))
        .map_err(|error| Error::config(stage, error.to_string()))
}
