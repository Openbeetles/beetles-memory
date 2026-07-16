use bm_adapter::{AdapterSource, TransportKind, TransportMode};

use crate::{EntryAuthDecision, EntryIdentity, EntryScope};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryTransportContext {
    request_id: String,
    transport: TransportKind,
    mode: TransportMode,
    operation: bm_adapter::AdapterOperation,
    source_id: String,
    source_kind: String,
    idempotency_key: String,
    audit_id: String,
    auth: EntryAuthDecision,
}

impl EntryTransportContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: impl Into<String>,
        transport: TransportKind,
        mode: TransportMode,
        operation: bm_adapter::AdapterOperation,
        source_id: impl Into<String>,
        source_kind: impl Into<String>,
        idempotency_key: impl Into<String>,
        audit_id: impl Into<String>,
        auth: EntryAuthDecision,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            transport,
            mode,
            operation,
            source_id: source_id.into(),
            source_kind: source_kind.into(),
            idempotency_key: idempotency_key.into(),
            audit_id: audit_id.into(),
            auth,
        }
    }

    pub(crate) fn operation(&self) -> bm_adapter::AdapterOperation {
        self.operation
    }

    pub(crate) fn request_id(&self) -> &str {
        &self.request_id
    }

    pub(crate) fn audit_id(&self) -> &str {
        &self.audit_id
    }

    pub(crate) fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    pub(crate) fn auth(&self) -> &EntryAuthDecision {
        &self.auth
    }

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

    pub(crate) fn into_parts(self) -> EntryTransportContextParts {
        EntryTransportContextParts {
            request_id: self.request_id,
            transport: self.transport,
            mode: self.mode,
            operation: self.operation,
            idempotency_key: self.idempotency_key,
            audit_id: self.audit_id,
            auth: self.auth,
        }
    }
}

pub(crate) struct EntryTransportContextParts {
    pub request_id: String,
    pub transport: TransportKind,
    pub mode: TransportMode,
    pub operation: bm_adapter::AdapterOperation,
    pub idempotency_key: String,
    pub audit_id: String,
    pub auth: EntryAuthDecision,
}
