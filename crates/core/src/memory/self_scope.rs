//! Subject ownership scope helpers.
//! 主体/关系作用域辅助：Soul 与私域绑定 canonical Subject；关系 key 进一步绑定该主体与会话边。

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub type MemorySpaceId = String;
pub type SubjectId = String;
pub type RelationshipId = String;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationshipScope {
    pub relationship_id: RelationshipId,
    pub channel: String,
    pub conversation_id: Option<String>,
}

pub fn default_memory_space_id(owner_id: &str) -> MemorySpaceId {
    let owner = encode_scope_component(owner_id);
    format!("space:{owner}")
}

pub fn system_governor_subject_id(owner_id: &str) -> SubjectId {
    let owner = encode_scope_component(owner_id);
    format!("system:{owner}")
}

pub fn primary_human_subject_id(user_id: &str) -> SubjectId {
    let user = encode_scope_component(user_id);
    format!("user:{user}")
}

pub fn default_agent_subject_id(agent_id: &str) -> SubjectId {
    let agent = encode_scope_component(agent_id);
    format!("agent:{agent}")
}

pub fn relationship_scope_id(mounted_subject_id: &str, channel: &str, chat_id: &str) -> String {
    let subject = encode_scope_component(mounted_subject_id);
    let channel = encode_scope_component(channel);
    let chat = encode_scope_component(chat_id);
    format!("rel:{subject}:{channel}:{chat}")
}

/// Resolves the one relationship owner used by relationship-local Core surfaces.
///
/// A governed multi-subject host passes the exact typed relationship id. The deterministic
/// channel/chat form remains only the explicit single-agent convenience path; callers never
/// probe both identities.
pub fn resolve_relationship_id(
    mounted_subject_id: &str,
    exact_relationship_id: Option<&str>,
    channel: &str,
    chat_id: &str,
) -> Result<RelationshipId> {
    match exact_relationship_id {
        Some(relationship_id)
            if !relationship_id.is_empty()
                && relationship_id.trim() == relationship_id
                && relationship_id.len() <= 256
                && !relationship_id.chars().any(char::is_control) =>
        {
            Ok(relationship_id.to_string())
        }
        Some(_) => Err(Error::config(
            "relationship_id",
            "exact relationship_id must be non-empty canonical text",
        )),
        None => Ok(relationship_scope_id(mounted_subject_id, channel, chat_id)),
    }
}

pub fn relationship_scope(
    mounted_subject_id: &str,
    channel: &str,
    chat_id: &str,
    conversation_id: Option<String>,
) -> RelationshipScope {
    RelationshipScope {
        relationship_id: relationship_scope_id(mounted_subject_id, channel, chat_id),
        channel: channel.trim().to_string(),
        conversation_id,
    }
}

fn encode_scope_component(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "_".to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    for byte in trimmed.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            push_hex_escape(&mut out, byte);
        }
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

fn push_hex_escape(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('_');
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0x0f) as usize] as char);
}

#[cfg(test)]
mod tests {
    use super::{relationship_scope_id, resolve_relationship_id};

    #[test]
    fn relationship_scope_id_binds_subject_channel_and_chat() {
        assert_eq!(
            relationship_scope_id("agent:alpha", "chat/channel", "user:1"),
            "rel:agent_3aalpha:chat_2fchannel:user_3a1"
        );
        assert_ne!(
            relationship_scope_id("agent:alpha", "chat/channel", "user:1"),
            relationship_scope_id("agent:beta", "chat/channel", "user:1")
        );
    }

    #[test]
    fn exact_relationship_id_never_aliases_or_falls_back() {
        assert_eq!(
            resolve_relationship_id(
                "agent:alpha",
                Some("relationship:custom"),
                "chat/channel",
                "user:1",
            )
            .unwrap(),
            "relationship:custom"
        );
        assert!(resolve_relationship_id(
            "agent:alpha",
            Some(" relationship:custom "),
            "chat/channel",
            "user:1",
        )
        .is_err());
        assert_eq!(
            resolve_relationship_id("agent:alpha", None, "chat/channel", "user:1").unwrap(),
            relationship_scope_id("agent:alpha", "chat/channel", "user:1")
        );
    }
}
