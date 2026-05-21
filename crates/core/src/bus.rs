use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{atomic::AtomicUsize, Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemInboundSendError {
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemInboundTrySendError {
    Full,
    Disconnected,
}

#[derive(Clone)]
pub struct SystemInboundTx {
    tx: SyncSender<PcMsg>,
    capacity: usize,
}

impl SystemInboundTx {
    pub fn new(tx: SyncSender<PcMsg>, capacity: usize) -> Self {
        Self { tx, capacity }
    }

    pub fn send(&self, msg: PcMsg) -> std::result::Result<(), SystemInboundSendError> {
        self.tx
            .send(msg)
            .map_err(|_| SystemInboundSendError::Disconnected)
    }

    pub fn try_send(&self, msg: PcMsg) -> std::result::Result<(), SystemInboundTrySendError> {
        self.tx.try_send(msg).map_err(|error| match error {
            std::sync::mpsc::TrySendError::Full(_) => SystemInboundTrySendError::Full,
            std::sync::mpsc::TrySendError::Disconnected(_) => {
                SystemInboundTrySendError::Disconnected
            }
        })
    }

    pub fn remaining_capacity(&self) -> usize {
        self.capacity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageBodyKind {
    #[default]
    Text,
    Image,
    Audio,
    Video,
    File,
    Card,
    PlatformNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageTransport {
    #[default]
    Unknown,
    Wss,
    Poll,
    Internal,
}

impl MessageTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Wss => "wss",
            Self::Poll => "poll",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum OutboundKind {
    #[default]
    Primary,
    Visibility,
    Supplemental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IngressKind {
    #[default]
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TextBody {
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CanonicalMessageBody {
    Text(TextBody),
    Image(Value),
    Audio(Value),
    Video(Value),
    File(Value),
    Card(Value),
    PlatformNative(Value),
}

impl Eq for CanonicalMessageBody {}

impl Default for CanonicalMessageBody {
    fn default() -> Self {
        Self::text("")
    }
}

impl CanonicalMessageBody {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextBody { text: text.into() })
    }

    pub fn kind(&self) -> MessageBodyKind {
        match self {
            Self::Text(_) => MessageBodyKind::Text,
            Self::Image(_) => MessageBodyKind::Image,
            Self::Audio(_) => MessageBodyKind::Audio,
            Self::Video(_) => MessageBodyKind::Video,
            Self::File(_) => MessageBodyKind::File,
            Self::Card(_) => MessageBodyKind::Card,
            Self::PlatformNative(_) => MessageBodyKind::PlatformNative,
        }
    }

    pub fn has_media(&self) -> bool {
        matches!(
            self,
            Self::Image(_) | Self::Audio(_) | Self::Video(_) | Self::File(_)
        )
    }

    pub fn text_projection(&self) -> String {
        match self {
            Self::Text(body) => body.text.clone(),
            Self::Image(_) => "[image]".to_string(),
            Self::Audio(_) => "[audio]".to_string(),
            Self::Video(_) => "[video]".to_string(),
            Self::File(_) => "[file]".to_string(),
            Self::Card(_) => "[card]".to_string(),
            Self::PlatformNative(_) => "[platform_native]".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PcMsg {
    pub channel: Box<str>,
    pub chat_id: Box<str>,
    pub content: String,
    #[serde(default)]
    pub body: CanonicalMessageBody,
    #[serde(default)]
    pub platform_thread_id: String,
    #[serde(default)]
    pub req_id: Option<String>,
    #[serde(default)]
    pub outbound_kind: OutboundKind,
    #[serde(default)]
    pub ingress: IngressKind,
    #[serde(default)]
    pub enqueue_ts_ms: u64,
    #[serde(default)]
    pub source_transport: MessageTransport,
    #[serde(default)]
    pub platform_message_id: String,
    #[serde(default)]
    pub platform_event_id: String,
    #[serde(default)]
    pub inbound_dedup_key: String,
    #[serde(default)]
    pub is_group: bool,
}

impl PcMsg {
    pub fn new(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<Self> {
        Self::new_inbound(channel, chat_id, content, false)
    }

    pub fn new_inbound(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
        is_group: bool,
    ) -> Result<Self> {
        Self::new_inbound_with_ingress(channel, chat_id, content, is_group, IngressKind::User)
    }

    pub fn new_system(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<Self> {
        Self::new_inbound_with_ingress(channel, chat_id, content, false, IngressKind::System)
    }

    pub fn new_inbound_with_ingress(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
        is_group: bool,
        ingress: IngressKind,
    ) -> Result<Self> {
        let content = content.into();
        if content.len() > 256 * 1024 {
            return Err(Error::config(
                "memory_ingress_envelope",
                "content too large",
            ));
        }
        let channel = channel.into();
        let chat_id = chat_id.into();
        Ok(Self {
            channel: channel.into_boxed_str(),
            chat_id: chat_id.into_boxed_str(),
            body: CanonicalMessageBody::text(content.clone()),
            content,
            platform_thread_id: String::new(),
            req_id: None,
            outbound_kind: OutboundKind::Primary,
            ingress,
            enqueue_ts_ms: crate::util::current_unix_ms(),
            source_transport: MessageTransport::Unknown,
            platform_message_id: String::new(),
            platform_event_id: String::new(),
            inbound_dedup_key: String::new(),
            is_group,
        })
    }

    pub fn body_kind(&self) -> MessageBodyKind {
        self.body.kind()
    }

    pub fn has_media_body(&self) -> bool {
        self.body.has_media()
    }
}

pub fn new_system_inbound_channel(
    capacity: usize,
) -> (SystemInboundTx, Receiver<PcMsg>, Arc<AtomicUsize>) {
    let (tx, rx) = mpsc::sync_channel(capacity);
    (
        SystemInboundTx::new(tx, capacity),
        rx,
        Arc::new(AtomicUsize::new(0)),
    )
}
