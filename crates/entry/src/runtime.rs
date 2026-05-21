use bm_adapter::{
    dispatch_adapter_command_with_services, AdapterCommand, AdapterEnvelope, AdapterErrorKey,
    AdapterResponse, AdapterRuntimeServices,
};
use bm_sdk::{
    resolve_memory_capabilities, Error, MemoryCapabilityPolicy, MemoryIdentity,
    MemoryPrivacyPolicy, MemoryRuntime, MemoryScope, MemorySkillDeleteRequest,
    MemorySkillDetailRequest, MemorySkillListRequest, MemorySkillSetEnabledRequest,
    MemorySkillUpsertRequest, NoopMemoryAuditSink, ProfileId, Result, StoreBackendConfig,
    StoreBackendKind, StorePlatform,
};

use crate::config::{enabled_capability_policy, privacy_policy};
use crate::{
    EntryAuthConfig, EntryCapabilityView, EntryConsoleDevice, EntryConsoleDeviceCreate,
    EntryConsoleDeviceKeyReport, EntryConsoleDeviceUpdate, EntryConsoleOverview,
    EntryConsoleSession, EntryConsoleSkillDetail, EntryConsoleSkillList, EntryConsoleSkillMutation,
    EntryConsoleSkillSetEnabled, EntryConsoleSkillUpsert, EntryConsoleState, EntryConsoleTransport,
    EntryConsoleTransportUpdate, EntryIdempotencyCache, EntryIdempotencyConfig, EntryIdentity,
    EntryResponse, EntryScope, EntryStoreConfig, EntryTransportConfig, EntryTransportContext,
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
    console: EntryConsoleState,
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
        let console = EntryConsoleState::new(&config);
        Ok(Self {
            config,
            runtime,
            capability,
            idempotency,
            console,
        })
    }

    pub fn runtime(&self) -> &MemoryRuntime {
        &self.runtime
    }

    pub fn capability(&self) -> &EntryCapabilityView {
        &self.capability
    }

    pub fn console_overview(&self) -> EntryConsoleOverview {
        self.console.overview()
    }

    pub fn console_transports(&self) -> Vec<EntryConsoleTransport> {
        self.console.transports()
    }

    pub fn console_update_transport(
        &self,
        id: &str,
        update: EntryConsoleTransportUpdate,
    ) -> Option<EntryConsoleTransport> {
        self.console.update_transport(id, update)
    }

    pub fn console_devices(&self) -> Vec<EntryConsoleDevice> {
        self.console.devices()
    }

    pub fn console_add_device(
        &self,
        request: EntryConsoleDeviceCreate,
    ) -> std::result::Result<EntryConsoleDeviceKeyReport, &'static str> {
        self.console.add_device(request)
    }

    pub fn console_update_device(
        &self,
        device_id: &str,
        update: EntryConsoleDeviceUpdate,
    ) -> Option<EntryConsoleDevice> {
        self.console.update_device(device_id, update)
    }

    pub fn console_rotate_device_key(
        &self,
        device_id: &str,
    ) -> Option<EntryConsoleDeviceKeyReport> {
        self.console.rotate_device_key(device_id)
    }

    pub fn console_session(&self) -> EntryConsoleSession {
        self.console.session()
    }

    pub fn console_skills(&self, query: Option<String>) -> Result<EntryConsoleSkillList> {
        self.runtime
            .list_skills(MemorySkillListRequest {
                query,
                include_disabled: true,
                include_retired: true,
                limit: 512,
            })
            .map(Into::into)
    }

    pub fn console_skill_detail(&self, name: &str) -> Result<Option<EntryConsoleSkillDetail>> {
        match self.runtime.get_skill(MemorySkillDetailRequest {
            name: name.to_string(),
        }) {
            Ok(report) => Ok(Some(report.into())),
            Err(error) if error.stage() == "skill_detail" => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn console_upsert_skill(
        &self,
        payload: EntryConsoleSkillUpsert,
    ) -> Result<EntryConsoleSkillMutation> {
        let existed = payload
            .name
            .as_deref()
            .and_then(|name| self.console_skill_detail(name).ok().flatten())
            .is_some();
        let report = self.runtime.upsert_skill(MemorySkillUpsertRequest {
            name: payload.name,
            title: payload.title,
            topic: payload.topic,
            summary: payload.summary,
            procedure: payload.procedure,
            citations: payload.citations,
            source_chat_id: payload
                .source_chat_id
                .or_else(|| Some(self.config.scope.chat_id.clone())),
            observed_at: current_unix_secs(),
        })?;
        let mutation: EntryConsoleSkillMutation = report.into();
        if mutation.accepted {
            self.console.record_skill_mutation(
                &mutation.name,
                if existed { "updated" } else { "imported" },
            );
        }
        Ok(mutation)
    }

    pub fn console_set_skill_enabled(
        &self,
        name: &str,
        payload: EntryConsoleSkillSetEnabled,
    ) -> Result<Option<EntryConsoleSkillMutation>> {
        match self
            .runtime
            .set_skill_enabled(MemorySkillSetEnabledRequest {
                name: name.to_string(),
                enabled: payload.enabled,
            }) {
            Ok(report) => {
                let mutation: EntryConsoleSkillMutation = report.into();
                if mutation.accepted {
                    self.console.record_skill_mutation(
                        &mutation.name,
                        if payload.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        },
                    );
                }
                Ok(Some(mutation))
            }
            Err(error) if error.stage() == "skill_set_enabled" => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn console_delete_skill(&self, name: &str) -> Result<Option<EntryConsoleSkillMutation>> {
        match self.runtime.delete_skill(MemorySkillDeleteRequest {
            name: name.to_string(),
        }) {
            Ok(report) => {
                let mutation: EntryConsoleSkillMutation = report.into();
                if mutation.accepted {
                    self.console
                        .record_skill_mutation(&mutation.name, "deleted");
                }
                Ok(Some(mutation))
            }
            Err(error) if error.stage() == "skill_delete" => Ok(None),
            Err(error) => Err(error),
        }
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

        let operation = command.operation();
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
        let response = dispatch_adapter_command_with_services(&self.runtime, envelope, services)
            .map(EntryResponse::from_adapter)?;
        self.console
            .record_adapter_response(operation, &response.adapter);
        Ok(response)
    }
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
