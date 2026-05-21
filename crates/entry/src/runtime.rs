use bm_adapter::{
    dispatch_adapter_command_with_services, AdapterCommand, AdapterEnvelope, AdapterErrorKey,
    AdapterResponse, AdapterRuntimeServices,
};
use bm_sdk::{
    resolve_memory_capabilities, Error, MemoryCapabilityPolicy, MemoryIdentity,
    MemoryPrivacyPolicy, MemoryRuntime, MemoryScope, NoopMemoryAuditSink, ProfileId, Result,
    StoreBackendConfig, StoreBackendKind, StorePlatform,
};

use crate::config::{enabled_capability_policy, privacy_policy};
use crate::{
    EntryAuthConfig, EntryCapabilityView, EntryIdempotencyCache, EntryIdempotencyConfig,
    EntryIdentity, EntryResponse, EntryScope, EntryStoreConfig, EntryTransportConfig,
    EntryTransportContext,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRuntimeConfig {
    pub profile: ProfileId,
    pub identity: EntryIdentity,
    pub scope: EntryScope,
    pub store: EntryStoreConfig,
    pub transports: EntryTransportConfig,
    pub auth: EntryAuthConfig,
    pub idempotency: EntryIdempotencyConfig,
    pub privacy: MemoryPrivacyPolicy,
    pub capability: MemoryCapabilityPolicy,
}

pub struct EntryRuntime {
    config: EntryRuntimeConfig,
    runtime: MemoryRuntime,
    capability: EntryCapabilityView,
    idempotency: EntryIdempotencyCache,
}

impl EntryRuntime {
    pub fn open(config: EntryRuntimeConfig) -> Result<Self> {
        let store = open_store(&config.store, config.profile)?;
        let capability_policy = enabled_capability_policy(config.capability.clone());
        let privacy = privacy_policy(config.privacy.clone());
        let runtime = MemoryRuntime::builder()
            .identity(MemoryIdentity::new(
                config.identity.agent_id.clone(),
                config.identity.owner_id.clone(),
            )?)
            .scope(MemoryScope::new(
                config.scope.channel.clone(),
                config.scope.chat_id.clone(),
            )?)
            .profile(config.profile)
            .store_platform(store)
            .capability_policy(capability_policy.clone())
            .privacy_policy(privacy.clone())
            .audit_sink(std::sync::Arc::new(NoopMemoryAuditSink))
            .build()?;
        let capability = entry_capability_view(
            config.profile,
            &capability_policy,
            &privacy,
            &config.transports,
        )?;
        let idempotency = EntryIdempotencyCache::new(config.idempotency.max_keys);
        Ok(Self {
            config,
            runtime,
            capability,
            idempotency,
        })
    }

    pub fn runtime(&self) -> &MemoryRuntime {
        &self.runtime
    }

    pub fn capability(&self) -> &EntryCapabilityView {
        &self.capability
    }

    pub fn handle(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
    ) -> Result<EntryResponse> {
        self.handle_with_services(context, command, AdapterRuntimeServices::none())
    }

    pub fn handle_with_services(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
        services: AdapterRuntimeServices<'_>,
    ) -> Result<EntryResponse> {
        if self.config.auth.require_auth && !context.auth.authenticated {
            return Ok(EntryResponse::from_adapter(AdapterResponse::Rejected {
                request_id: context.request_id,
                audit_id: context.audit_id,
                error_key: AdapterErrorKey::Unauthorized,
                reason: "entry auth rejected request".to_string(),
            }));
        }
        if is_mutation(command.operation()) && !self.idempotency.remember(&context.idempotency_key)
        {
            return Ok(EntryResponse::from_adapter(AdapterResponse::Duplicated {
                request_id: context.request_id,
                audit_id: context.audit_id,
                idempotency_key: context.idempotency_key,
            }));
        }

        let source = context.source(&self.config.identity, &self.config.scope);
        let auth = context.auth.into_adapter();
        let envelope = AdapterEnvelope {
            request_id: context.request_id,
            transport: context.transport,
            mode: context.mode,
            operation: context.operation,
            source,
            auth,
            idempotency_key: context.idempotency_key,
            audit_id: context.audit_id,
            payload: command,
        };
        dispatch_adapter_command_with_services(&self.runtime, envelope, services)
            .map(EntryResponse::from_adapter)
    }
}

pub fn entry_capability_view(
    profile: ProfileId,
    policy: &MemoryCapabilityPolicy,
    privacy: &MemoryPrivacyPolicy,
    transports: &EntryTransportConfig,
) -> Result<EntryCapabilityView> {
    let catalog = resolve_memory_capabilities(profile, policy, privacy)?;
    Ok(EntryCapabilityView::from_catalog(
        profile, &catalog, transports,
    ))
}

fn open_store(config: &EntryStoreConfig, profile: ProfileId) -> Result<StorePlatform> {
    let store_config = match config.backend {
        StoreBackendKind::InMemory => StoreBackendConfig::in_memory(profile)?,
        StoreBackendKind::Embedded => StoreBackendConfig::embedded(profile)?,
        StoreBackendKind::File => {
            let path = config.data_path.clone().ok_or_else(|| {
                Error::config("entry_store_config", "file store requires data_path")
            })?;
            StoreBackendConfig::file(path, profile)?
        }
        StoreBackendKind::Sqlite => {
            let path = config.data_path.clone().ok_or_else(|| {
                Error::config("entry_store_config", "sqlite store requires data_path")
            })?;
            StoreBackendConfig::sqlite(path, profile)?
        }
    }
    .with_fsync(config.fsync);
    StorePlatform::open(store_config)
}

const fn is_mutation(operation: bm_adapter::AdapterOperation) -> bool {
    matches!(
        operation,
        bm_adapter::AdapterOperation::Write
            | bm_adapter::AdapterOperation::Maintain
            | bm_adapter::AdapterOperation::Recover
            | bm_adapter::AdapterOperation::Import
            | bm_adapter::AdapterOperation::Close
    )
}
