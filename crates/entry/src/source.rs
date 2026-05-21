use bm_adapter::{AdapterSource, TransportKind, TransportMode};

use crate::{EntryAuthDecision, EntryIdentity, EntryScope};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryTransportContext {
    pub request_id: String,
    pub transport: TransportKind,
    pub mode: TransportMode,
    pub operation: bm_adapter::AdapterOperation,
    pub source_id: String,
    pub source_kind: String,
    pub idempotency_key: String,
    pub audit_id: String,
    pub auth: EntryAuthDecision,
}

impl EntryTransportContext {
    pub(crate) fn source(&self, identity: &EntryIdentity, scope: &EntryScope) -> AdapterSource {
        AdapterSource {
            source_id: self.source_id.clone(),
            source_kind: self.source_kind.clone(),
            agent_id: identity.agent_id.clone(),
            owner_id: identity.owner_id.clone(),
            channel: scope.channel.clone(),
            chat_id: scope.chat_id.clone(),
        }
    }
}
