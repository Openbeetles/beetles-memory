use crate::error::{Error, Result};
use crate::memory::SessionStore;
use crate::platform::ResponseBody;
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
