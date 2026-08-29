use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bm_sdk::{
    AuthorizedGovernanceEnvelope, GovernanceEgressAuthority, GovernanceExecutionOperation,
    GovernanceExecutionPort, GovernanceExecutionPortFailure, ImmutableGovernanceExecutionBinding,
    MemoryGovernanceBindingInstallRequest, MemoryLearningAttachmentIdentity,
    MemoryLearningCycleOutcome, MemoryLearningCycleRequest, MemoryLearningEngine,
    MemoryLearningWakeSink, MemoryRuntime, PostTurnGovernanceProviderProtocolV1,
};
use sha2::{Digest, Sha256};

#[cfg(feature = "governance-model-client-std")]
use crate::ConfiguredGovernanceLlmClient;
use crate::{
    EntryGovernanceModelAuthMode, EntryGovernanceModelExecutionBinding,
    EntryGovernanceModelProtocol,
};

#[cfg(feature = "governance-model-client-std")]
const DEFAULT_RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceProviderBinding {
    pub source_owner_id: String,
    pub source_config_id: String,
    pub source_revision: u64,
    pub protocol: EntryGovernanceModelProtocol,
    pub endpoint: String,
    pub model_id: String,
    pub credential_reference: Option<String>,
    pub request_timeout_ms: u64,
    pub max_input_tokens: usize,
    pub max_output_tokens: usize,
    pub provider_permission_generation: u64,
}

impl GovernanceProviderBinding {
    pub fn validate(&self) -> bm_sdk::Result<()> {
        if self.source_owner_id.trim().is_empty()
            || self.source_owner_id.trim() != self.source_owner_id
            || self.source_config_id.trim().is_empty()
            || self.source_config_id.trim() != self.source_config_id
            || self.source_revision == 0
            || self.model_id.trim().is_empty()
            || self.model_id.trim() != self.model_id
            || self.request_timeout_ms == 0
            || self.max_input_tokens == 0
            || self.max_output_tokens == 0
            || self.provider_permission_generation == 0
        {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_binding",
                "binding identity, revision, model, timeout, and budgets must be exact",
            ));
        }
        if self
            .credential_reference
            .as_ref()
            .is_some_and(|reference| reference.trim().is_empty() || reference.trim() != reference)
        {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_binding",
                "credential reference must be canonical",
            ));
        }
        let endpoint = url::Url::parse(&self.endpoint).map_err(|error| {
            bm_sdk::Error::invalid_input("memory_learning_binding", error.to_string())
        })?;
        if !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_binding",
                "endpoint must not contain userinfo, query, or fragment",
            ));
        }
        let host = endpoint.host_str().ok_or_else(|| {
            bm_sdk::Error::invalid_input("memory_learning_binding", "endpoint host is missing")
        })?;
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && loopback) {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_binding",
                "non-loopback endpoint must use HTTPS",
            ));
        }
        if self.credential_reference.is_none() && !loopback {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_binding",
                "remote endpoint requires a credential reference",
            ));
        }
        Ok(())
    }

    fn install_request(&self) -> MemoryGovernanceBindingInstallRequest {
        MemoryGovernanceBindingInstallRequest {
            source_owner_id: self.source_owner_id.clone(),
            source_config_id: self.source_config_id.clone(),
            source_revision: self.source_revision,
            protocol: match self.protocol {
                EntryGovernanceModelProtocol::OpenAiCompatible => {
                    PostTurnGovernanceProviderProtocolV1::OpenAiCompatible
                }
                EntryGovernanceModelProtocol::OllamaNative => {
                    PostTurnGovernanceProviderProtocolV1::OllamaNative
                }
            },
            endpoint: self.endpoint.clone(),
            model_id: self.model_id.clone(),
            credential_reference: self.credential_reference.clone(),
            request_timeout_ms: self.request_timeout_ms,
            max_input_tokens: self.max_input_tokens as u64,
            max_output_tokens: self.max_output_tokens as u64,
            provider_permission_generation: self.provider_permission_generation,
        }
    }

    #[cfg(feature = "governance-model-client-std")]
    fn llm_binding(
        binding: &ImmutableGovernanceExecutionBinding,
    ) -> bm_sdk::Result<EntryGovernanceModelExecutionBinding> {
        Ok(EntryGovernanceModelExecutionBinding {
            binding_id: binding.binding_id.clone(),
            protocol: match binding.protocol {
                PostTurnGovernanceProviderProtocolV1::OpenAiCompatible => {
                    EntryGovernanceModelProtocol::OpenAiCompatible
                }
                PostTurnGovernanceProviderProtocolV1::OllamaNative => {
                    EntryGovernanceModelProtocol::OllamaNative
                }
            },
            endpoint: binding.endpoint.clone(),
            model: binding.model_id.clone(),
            auth_mode: EntryGovernanceModelAuthMode::LocalUnauthenticated,
            request_timeout_ms: binding.request_timeout_ms,
            max_input_tokens: usize::try_from(binding.max_input_tokens).map_err(|_| {
                bm_sdk::Error::config("memory_learning_binding", "input token budget overflow")
            })?,
            max_output_tokens: usize::try_from(binding.max_output_tokens).map_err(|_| {
                bm_sdk::Error::config("memory_learning_binding", "output token budget overflow")
            })?,
            config_revision: binding.binding_revision,
        })
    }
}

impl GovernanceProviderBinding {
    pub(crate) fn from_entry_binding(
        binding: EntryGovernanceModelExecutionBinding,
        source_owner_id: String,
        source_config_id: String,
    ) -> Self {
        let credential_reference = match &binding.auth_mode {
            EntryGovernanceModelAuthMode::CredentialEnv { credential_env } => {
                Some(credential_env.clone())
            }
            EntryGovernanceModelAuthMode::LocalUnauthenticated => None,
        };
        Self {
            source_owner_id,
            source_config_id,
            source_revision: binding.config_revision,
            protocol: binding.protocol,
            endpoint: binding.endpoint,
            model_id: binding.model,
            credential_reference,
            request_timeout_ms: binding.request_timeout_ms,
            max_input_tokens: binding.max_input_tokens,
            max_output_tokens: binding.max_output_tokens,
            provider_permission_generation: 1,
        }
    }
}

pub trait GovernanceBindingSource: Send + Sync {
    fn current_binding(&self) -> bm_sdk::Result<Option<GovernanceProviderBinding>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceCredentialRequest {
    pub binding_id: String,
    pub binding_revision: u64,
    pub credential_reference: String,
    pub credential_reference_safe_id: String,
    pub purpose: &'static str,
}

pub trait GovernanceCredentialResolver: Send + Sync {
    fn resolve(
        &self,
        request: &GovernanceCredentialRequest,
    ) -> bm_sdk::Result<ResolvedGovernanceCredential>;
}

pub struct ResolvedGovernanceCredential {
    #[cfg_attr(not(feature = "governance-model-client-std"), allow(dead_code))]
    reference_safe_id: String,
    generation: u64,
    secret: Vec<u8>,
}

impl ResolvedGovernanceCredential {
    pub fn bearer(
        reference_safe_id: impl Into<String>,
        generation: u64,
        secret: impl Into<String>,
    ) -> bm_sdk::Result<Self> {
        let reference_safe_id = reference_safe_id.into();
        let secret = secret.into().into_bytes();
        if reference_safe_id.trim().is_empty() || generation == 0 || secret.is_empty() {
            return Err(bm_sdk::Error::invalid_input(
                "governance_credential",
                "safe reference, generation, and secret are required",
            ));
        }
        Ok(Self {
            reference_safe_id,
            generation,
            secret,
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(feature = "governance-model-client-std")]
    fn authorization_value(&self) -> bm_sdk::Result<String> {
        let secret = std::str::from_utf8(&self.secret).map_err(|_| {
            bm_sdk::Error::invalid_input("governance_credential", "credential secret must be UTF-8")
        })?;
        Ok(format!("Bearer {secret}"))
    }
}

impl Drop for ResolvedGovernanceCredential {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

#[derive(Default)]
pub struct EnvironmentGovernanceCredentialResolver;

impl GovernanceCredentialResolver for EnvironmentGovernanceCredentialResolver {
    fn resolve(
        &self,
        request: &GovernanceCredentialRequest,
    ) -> bm_sdk::Result<ResolvedGovernanceCredential> {
        let secret = std::env::var(&request.credential_reference).map_err(|_| {
            bm_sdk::Error::config(
                "governance_credential_missing",
                "credential resolver could not acquire the requested secret",
            )
        })?;
        let generation = std::env::var(format!("{}_GENERATION", request.credential_reference))
            .ok()
            .map(|value| {
                value.parse::<u64>().map_err(|_| {
                    bm_sdk::Error::config(
                        "governance_credential_generation",
                        "credential generation environment variable must be a positive integer",
                    )
                })
            })
            .transpose()?
            .unwrap_or(1);
        ResolvedGovernanceCredential::bearer(
            request.credential_reference_safe_id.clone(),
            generation,
            secret,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryLearningWorkerLimits {
    pub max_attachments: usize,
    pub poll_interval_ms: u64,
    pub lease_duration_secs: u64,
}

impl Default for MemoryLearningWorkerLimits {
    fn default() -> Self {
        Self {
            max_attachments: 32,
            poll_interval_ms: 250,
            lease_duration_secs: 120,
        }
    }
}

impl MemoryLearningWorkerLimits {
    fn validate(self) -> bm_sdk::Result<Self> {
        if self.max_attachments == 0
            || self.max_attachments > 256
            || self.poll_interval_ms == 0
            || self.poll_interval_ms > 60_000
            || self.lease_duration_secs == 0
            || self.lease_duration_secs > 900
        {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_service",
                "worker limits exceed the bounded service contract",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningServiceReport {
    pub state: String,
    pub attachment_count: usize,
    pub cycles: u64,
    pub completed_jobs: u64,
    pub retrying_jobs: u64,
    pub blocked_jobs: u64,
    pub cancelled_jobs: u64,
    pub failed_jobs: u64,
    pub reason: String,
    pub binding_ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLearningAttachmentReport {
    pub mounted_subject_id: String,
    pub state: String,
    pub last_job_id: Option<String>,
    pub reason: String,
}

pub struct MemoryLearningServiceStatusRequest {
    pub authority: bm_sdk::MemoryLearningServiceStatusAuthority,
}

pub struct MemoryLearningAttachmentStatusRequest {
    pub authority: bm_sdk::MemoryLearningAttachmentStatusAuthority,
}

#[derive(Clone)]
struct AttachedRuntime {
    engine: MemoryLearningEngine,
    identity: MemoryLearningAttachmentIdentity,
    control_authorities: bm_sdk::MemoryLearningServiceControlAuthorities,
    report: Arc<Mutex<MemoryLearningAttachmentReport>>,
    active: Arc<AtomicBool>,
}

struct WakeState {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl WakeState {
    fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    fn notify(&self) {
        let mut generation = self.generation.lock().expect("learning wake lock");
        *generation = generation.saturating_add(1);
        self.changed.notify_all();
    }
}

impl MemoryLearningWakeSink for WakeState {
    fn wake(&self) {
        self.notify();
    }
}

struct ServiceInner {
    attachments: Mutex<Vec<AttachedRuntime>>,
    binding_source: Arc<dyn GovernanceBindingSource>,
    credential_resolver: Arc<dyn GovernanceCredentialResolver>,
    limits: MemoryLearningWorkerLimits,
    stop: Arc<AtomicBool>,
    done: AtomicBool,
    wake: Arc<WakeState>,
    done_signal: Condvar,
    done_lock: Mutex<()>,
    handle: Mutex<Option<JoinHandle<()>>>,
    worker_sequence: AtomicU64,
    service_handles: AtomicU64,
    report: Mutex<MemoryLearningServiceReport>,
}

pub struct MemoryLearningServiceBuilder {
    first_runtime: Arc<MemoryRuntime>,
    control_authorities: Option<bm_sdk::MemoryLearningServiceControlAuthorities>,
    binding_source: Option<Arc<dyn GovernanceBindingSource>>,
    credential_resolver: Option<Arc<dyn GovernanceCredentialResolver>>,
    limits: MemoryLearningWorkerLimits,
}

fn validate_control_authorities(
    identity: &MemoryLearningAttachmentIdentity,
    authorities: &bm_sdk::MemoryLearningServiceControlAuthorities,
) -> bm_sdk::Result<()> {
    if !authorities.credential_recovery().authorizes(
        identity,
        bm_sdk::MemoryLearningServiceControlOperation::CredentialRecovery,
    ) || !authorities.provider_permission_recovery().authorizes(
        identity,
        bm_sdk::MemoryLearningServiceControlOperation::ProviderPermissionRecovery,
    ) {
        return Err(bm_sdk::Error::config(
            "memory_learning_service",
            "SystemGovernor control authority differs from the exact runtime attachment",
        ));
    }
    Ok(())
}

impl MemoryLearningServiceBuilder {
    pub fn control_authorities(
        mut self,
        authorities: bm_sdk::MemoryLearningServiceControlAuthorities,
    ) -> Self {
        self.control_authorities = Some(authorities);
        self
    }

    pub fn binding_source(mut self, source: Arc<dyn GovernanceBindingSource>) -> Self {
        self.binding_source = Some(source);
        self
    }

    pub fn credential_resolver(mut self, resolver: Arc<dyn GovernanceCredentialResolver>) -> Self {
        self.credential_resolver = Some(resolver);
        self
    }

    pub fn worker_limits(mut self, limits: MemoryLearningWorkerLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn start(self) -> bm_sdk::Result<(MemoryLearningService, MemoryLearningAttachment)> {
        let limits = self.limits.validate()?;
        let binding_source = self.binding_source.ok_or_else(|| {
            bm_sdk::Error::config("memory_learning_service", "binding source is required")
        })?;
        let credential_resolver = self.credential_resolver.ok_or_else(|| {
            bm_sdk::Error::config("memory_learning_service", "credential resolver is required")
        })?;
        let control_authorities = self.control_authorities.ok_or_else(|| {
            bm_sdk::Error::config(
                "memory_learning_service",
                "SystemGovernor control authorities are required",
            )
        })?;
        let engine = MemoryLearningEngine::attach(Arc::clone(&self.first_runtime))?;
        let identity = engine.attachment_identity()?;
        validate_control_authorities(&identity, &control_authorities)?;
        let binding_ready = install_current_binding(&self.first_runtime, binding_source.as_ref())?;
        let attachment_report = Arc::new(Mutex::new(MemoryLearningAttachmentReport {
            mounted_subject_id: identity.mounted_subject_id().to_string(),
            state: "idle".to_string(),
            last_job_id: None,
            reason: "service_started".to_string(),
        }));
        let attachment_active = Arc::new(AtomicBool::new(true));
        let wake = Arc::new(WakeState::new());
        self.first_runtime
            .register_learning_wake_sink(Arc::clone(&wake) as Arc<dyn MemoryLearningWakeSink>);
        let inner = Arc::new(ServiceInner {
            attachments: Mutex::new(vec![AttachedRuntime {
                engine,
                identity: identity.clone(),
                control_authorities,
                report: Arc::clone(&attachment_report),
                active: Arc::clone(&attachment_active),
            }]),
            binding_source,
            credential_resolver,
            limits,
            stop: Arc::new(AtomicBool::new(false)),
            done: AtomicBool::new(false),
            wake,
            done_signal: Condvar::new(),
            done_lock: Mutex::new(()),
            handle: Mutex::new(None),
            worker_sequence: AtomicU64::new(1),
            service_handles: AtomicU64::new(1),
            report: Mutex::new(MemoryLearningServiceReport {
                state: "running".to_string(),
                attachment_count: 1,
                cycles: 0,
                completed_jobs: 0,
                retrying_jobs: 0,
                blocked_jobs: 0,
                cancelled_jobs: 0,
                failed_jobs: 0,
                reason: "service_started".to_string(),
                binding_ready,
            }),
        });
        let thread_inner = Arc::clone(&inner);
        let handle = std::thread::Builder::new()
            .name("beetle-memory-learning".to_string())
            .spawn(move || service_loop(thread_inner))
            .map_err(|error| bm_sdk::Error::io("memory_learning_service_start", error))?;
        *inner.handle.lock().expect("learning service handle lock") = Some(handle);
        let service = MemoryLearningService {
            inner: Arc::clone(&inner),
        };
        let attachment = MemoryLearningAttachment {
            inner: Arc::downgrade(&inner),
            report: attachment_report,
            identity,
            active: attachment_active,
        };
        Ok((service, attachment))
    }
}

pub struct MemoryLearningService {
    inner: Arc<ServiceInner>,
}

impl Clone for MemoryLearningService {
    fn clone(&self) -> Self {
        self.inner.service_handles.fetch_add(1, Ordering::AcqRel);
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl MemoryLearningService {
    pub fn builder(runtime: Arc<MemoryRuntime>) -> MemoryLearningServiceBuilder {
        MemoryLearningServiceBuilder {
            first_runtime: runtime,
            control_authorities: None,
            binding_source: None,
            credential_resolver: None,
            limits: MemoryLearningWorkerLimits::default(),
        }
    }

    pub fn attach_runtime(
        &self,
        runtime: Arc<MemoryRuntime>,
        control_authorities: bm_sdk::MemoryLearningServiceControlAuthorities,
    ) -> bm_sdk::Result<MemoryLearningAttachment> {
        if self.inner.stop.load(Ordering::Acquire) {
            return Err(bm_sdk::Error::conflict(
                "memory_learning_service",
                "service is stopping",
            ));
        }
        let engine = MemoryLearningEngine::attach(Arc::clone(&runtime))?;
        let identity = engine.attachment_identity()?;
        validate_control_authorities(&identity, &control_authorities)?;
        let mut attachments = self
            .inner
            .attachments
            .lock()
            .expect("learning attachments lock");
        if self.inner.stop.load(Ordering::Acquire) {
            return Err(bm_sdk::Error::conflict(
                "memory_learning_service",
                "service is stopping",
            ));
        }
        let first = attachments.first().ok_or_else(|| {
            bm_sdk::Error::config("memory_learning_service", "service has no Store authority")
        })?;
        if !identity.shares_store_and_registry_with(&first.identity) {
            return Err(bm_sdk::Error::conflict(
                "memory_learning_service",
                "runtime Store, MemorySpace, or SubjectRegistry authority differs",
            ));
        }
        if let Some(existing) = attachments
            .iter()
            .find(|attached| attached.identity == identity)
        {
            return Ok(MemoryLearningAttachment {
                inner: Arc::downgrade(&self.inner),
                report: Arc::clone(&existing.report),
                identity,
                active: Arc::clone(&existing.active),
            });
        }
        if attachments.len() >= self.inner.limits.max_attachments {
            return Err(bm_sdk::Error::config(
                "memory_learning_service",
                "attachment capacity exhausted",
            ));
        }
        let _ = install_current_binding(&runtime, self.inner.binding_source.as_ref())?;
        let report = Arc::new(Mutex::new(MemoryLearningAttachmentReport {
            mounted_subject_id: identity.mounted_subject_id().to_string(),
            state: "idle".to_string(),
            last_job_id: None,
            reason: "runtime_attached".to_string(),
        }));
        let active = Arc::new(AtomicBool::new(true));
        runtime.register_learning_wake_sink(
            Arc::clone(&self.inner.wake) as Arc<dyn MemoryLearningWakeSink>
        );
        attachments.push(AttachedRuntime {
            engine,
            identity: identity.clone(),
            control_authorities,
            report: Arc::clone(&report),
            active: Arc::clone(&active),
        });
        self.inner
            .report
            .lock()
            .expect("learning service report lock")
            .attachment_count = attachments.len();
        drop(attachments);
        self.inner.wake.notify();
        Ok(MemoryLearningAttachment {
            inner: Arc::downgrade(&self.inner),
            report,
            identity,
            active,
        })
    }

    pub fn wake(&self) {
        self.inner.wake.notify();
    }

    pub fn credential_changed(
        &self,
        credential_reference: &str,
        generation: u64,
        operation_id: &str,
    ) -> bm_sdk::Result<()> {
        if credential_reference.trim().is_empty()
            || credential_reference.trim() != credential_reference
            || generation == 0
            || operation_id.trim().is_empty()
            || operation_id.trim() != operation_id
        {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_credential_changed",
                "canonical credential reference, generation, and operation id are required",
            ));
        }
        let credential_ref_safe_id = credential_reference_safe_id(credential_reference);
        let attachments = self
            .inner
            .attachments
            .lock()
            .expect("learning attachments lock")
            .clone();
        for attachment in attachments {
            attachment.engine.runtime().governance_credential_changed(
                bm_sdk::MemoryGovernanceCredentialChangedRequest {
                    authority: attachment.control_authorities.credential_recovery(),
                    credential_ref_safe_id: credential_ref_safe_id.clone(),
                    new_generation: generation,
                    operation_id: operation_id.to_string(),
                },
            )?;
        }
        self.inner.wake.notify();
        Ok(())
    }

    pub fn provider_config_changed(
        &self,
        source_config_id: &str,
        source_revision: u64,
        operation_id: &str,
    ) -> bm_sdk::Result<()> {
        if source_config_id.trim().is_empty()
            || source_config_id.trim() != source_config_id
            || source_revision == 0
            || operation_id.trim().is_empty()
            || operation_id.trim() != operation_id
        {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_provider_config_changed",
                "canonical source identity, revision, and operation id are required",
            ));
        }
        let current = self
            .inner
            .binding_source
            .current_binding()?
            .ok_or_else(|| {
                bm_sdk::Error::config(
                    "memory_learning_provider_config_changed",
                    "binding source has no current configuration",
                )
            })?;
        if current.source_config_id != source_config_id
            || current.source_revision != source_revision
        {
            return Err(bm_sdk::Error::conflict(
                "memory_learning_provider_config_changed",
                "notification differs from the authoritative binding source",
            ));
        }
        current.validate()?;
        let attachments = self
            .inner
            .attachments
            .lock()
            .expect("learning attachments lock")
            .clone();
        for attachment in attachments {
            attachment
                .engine
                .runtime()
                .install_governance_binding(current.install_request())?;
        }
        self.inner
            .report
            .lock()
            .expect("learning service report lock")
            .binding_ready = true;
        self.inner.wake.notify();
        Ok(())
    }

    pub fn provider_permission_changed(
        &self,
        source_config_id: &str,
        source_revision: u64,
        new_generation: u64,
        operation_id: &str,
    ) -> bm_sdk::Result<()> {
        if source_config_id.trim().is_empty()
            || source_config_id.trim() != source_config_id
            || source_revision == 0
            || new_generation == 0
            || operation_id.trim().is_empty()
            || operation_id.trim() != operation_id
        {
            return Err(bm_sdk::Error::invalid_input(
                "memory_learning_provider_permission_changed",
                "canonical source identity, revision, advanced generation, and operation id are required",
            ));
        }
        let current = self
            .inner
            .binding_source
            .current_binding()?
            .ok_or_else(|| {
                bm_sdk::Error::config(
                    "memory_learning_provider_permission_changed",
                    "binding source has no current configuration",
                )
            })?;
        if current.source_config_id != source_config_id
            || current.source_revision != source_revision
        {
            return Err(bm_sdk::Error::conflict(
                "memory_learning_provider_permission_changed",
                "notification differs from the authoritative binding source",
            ));
        }
        current.validate()?;
        let attachments = self
            .inner
            .attachments
            .lock()
            .expect("learning attachments lock")
            .clone();
        for attachment in attachments {
            let binding = attachment
                .engine
                .runtime()
                .install_governance_binding(current.install_request())?
                .binding;
            attachment
                .engine
                .runtime()
                .governance_provider_permission_changed(
                    bm_sdk::MemoryGovernanceProviderPermissionChangedRequest {
                        authority: attachment
                            .control_authorities
                            .provider_permission_recovery(),
                        binding_id: binding.binding_id,
                        binding_revision: binding.binding_revision,
                        new_generation,
                        operation_id: operation_id.to_string(),
                    },
                )?;
        }
        self.inner.wake.notify();
        Ok(())
    }

    pub fn status(
        &self,
        request: MemoryLearningServiceStatusRequest,
    ) -> bm_sdk::Result<MemoryLearningServiceReport> {
        let identity = self
            .inner
            .attachments
            .lock()
            .expect("learning attachments lock")
            .first()
            .map(|attachment| attachment.identity.clone())
            .ok_or_else(|| {
                bm_sdk::Error::config(
                    "memory_learning_service_status",
                    "service has no Store authority",
                )
            })?;
        if !request.authority.authorizes(&identity) {
            return Err(bm_sdk::Error::config(
                "memory_learning_service_status",
                "SystemGovernor inspection authority differs from the service",
            ));
        }
        Ok(self.safe_service_report())
    }

    fn safe_service_report(&self) -> MemoryLearningServiceReport {
        let mut report = self
            .inner
            .report
            .lock()
            .expect("learning service report lock")
            .clone();
        report.reason = "operator_safe_aggregate".to_string();
        report
    }

    pub fn shutdown(&self, deadline: Instant) -> bm_sdk::Result<MemoryLearningServiceReport> {
        {
            let _attachments = self
                .inner
                .attachments
                .lock()
                .expect("learning attachments lock");
            self.inner.stop.store(true, Ordering::Release);
        }
        self.inner.wake.notify();
        let mut guard = self.inner.done_lock.lock().expect("learning done lock");
        while !self.inner.done.load(Ordering::Acquire) {
            let now = Instant::now();
            if now >= deadline {
                return Err(bm_sdk::Error::conflict(
                    "memory_learning_service_shutdown",
                    "shutdown deadline elapsed before the worker stopped",
                ));
            }
            let (next, timeout) = self
                .inner
                .done_signal
                .wait_timeout(guard, deadline.saturating_duration_since(now))
                .expect("learning done wait");
            guard = next;
            if timeout.timed_out() && !self.inner.done.load(Ordering::Acquire) {
                return Err(bm_sdk::Error::conflict(
                    "memory_learning_service_shutdown",
                    "shutdown deadline elapsed before the worker stopped",
                ));
            }
        }
        drop(guard);
        if let Some(handle) = self
            .inner
            .handle
            .lock()
            .expect("learning service handle lock")
            .take()
        {
            handle.join().map_err(|_| {
                bm_sdk::Error::config(
                    "memory_learning_service_shutdown",
                    "learning worker panicked",
                )
            })?;
        }
        Ok(self.safe_service_report())
    }
}

impl Drop for MemoryLearningService {
    fn drop(&mut self) {
        if self.inner.service_handles.fetch_sub(1, Ordering::AcqRel) == 1 {
            {
                let _attachments = self
                    .inner
                    .attachments
                    .lock()
                    .expect("learning attachments lock");
                self.inner.stop.store(true, Ordering::Release);
            }
            self.inner.wake.notify();
        }
    }
}

pub struct MemoryLearningAttachment {
    inner: Weak<ServiceInner>,
    report: Arc<Mutex<MemoryLearningAttachmentReport>>,
    identity: MemoryLearningAttachmentIdentity,
    active: Arc<AtomicBool>,
}

impl MemoryLearningAttachment {
    pub fn wake(&self) -> bm_sdk::Result<()> {
        let inner = self.inner.upgrade().ok_or_else(|| {
            bm_sdk::Error::conflict("memory_learning_attachment", "service is unavailable")
        })?;
        if inner.stop.load(Ordering::Acquire) {
            return Err(bm_sdk::Error::conflict(
                "memory_learning_attachment",
                "service is stopping",
            ));
        }
        inner.wake.notify();
        Ok(())
    }

    pub fn status(
        &self,
        request: MemoryLearningAttachmentStatusRequest,
    ) -> bm_sdk::Result<MemoryLearningAttachmentReport> {
        if !request.authority.authorizes(&self.identity) {
            return Err(bm_sdk::Error::config(
                "memory_learning_attachment_status",
                "mounted Runtime inspection authority differs from the attachment",
            ));
        }
        Ok(self
            .report
            .lock()
            .expect("learning attachment report lock")
            .clone())
    }

    pub fn detach(&self) -> bm_sdk::Result<()> {
        if Arc::strong_count(&self.report) > 2 {
            return Ok(());
        }
        self.active.store(false, Ordering::Release);
        let Some(inner) = self.inner.upgrade() else {
            return Ok(());
        };
        let mut attachments = inner.attachments.lock().expect("learning attachments lock");
        attachments.retain(|attached| {
            attached.identity != self.identity || !Arc::ptr_eq(&attached.report, &self.report)
        });
        inner
            .report
            .lock()
            .expect("learning service report lock")
            .attachment_count = attachments.len();
        drop(attachments);
        inner.wake.notify();
        Ok(())
    }
}

impl Drop for MemoryLearningAttachment {
    fn drop(&mut self) {
        let _ = self.detach();
    }
}

fn install_current_binding(
    runtime: &MemoryRuntime,
    source: &dyn GovernanceBindingSource,
) -> bm_sdk::Result<bool> {
    let Some(binding) = source.current_binding()? else {
        return Ok(false);
    };
    binding.validate()?;
    runtime.install_governance_binding(binding.install_request())?;
    Ok(true)
}

fn service_loop(inner: Arc<ServiceInner>) {
    let poll = Duration::from_millis(inner.limits.poll_interval_ms);
    let mut observed_generation = 0_u64;
    while !inner.stop.load(Ordering::Acquire) {
        let attachments = inner
            .attachments
            .lock()
            .expect("learning attachments lock")
            .clone();
        let mut progressed = false;
        for attachment in attachments {
            if inner.stop.load(Ordering::Acquire) {
                break;
            }
            progressed |= run_attachment_cycle(&inner, &attachment);
        }
        if inner.stop.load(Ordering::Acquire) {
            break;
        }
        let generation = inner.wake.generation.lock().expect("learning wake lock");
        if *generation == observed_generation && !progressed {
            let (next, _) = inner
                .wake
                .changed
                .wait_timeout(generation, poll)
                .expect("learning wake wait");
            observed_generation = *next;
        } else {
            observed_generation = *generation;
        }
    }
    let mut report = inner.report.lock().expect("learning service report lock");
    report.state = "stopped".to_string();
    report.reason = "shutdown_complete".to_string();
    drop(report);
    inner.done.store(true, Ordering::Release);
    inner.done_signal.notify_all();
}

fn run_attachment_cycle(inner: &Arc<ServiceInner>, attachment: &AttachedRuntime) -> bool {
    if !attachment.active.load(Ordering::Acquire) {
        return false;
    }
    let sequence = inner.worker_sequence.fetch_add(1, Ordering::AcqRel);
    let mut port = OfficialLearningExecutionPort {
        credential_resolver: Arc::clone(&inner.credential_resolver),
        stop: Arc::clone(&inner.stop),
        attachment_active: Arc::clone(&attachment.active),
    };
    let outcome = attachment.engine.run_due_cycle(
        MemoryLearningCycleRequest {
            lease_owner: format!("memory-learning-service:{sequence}"),
            lease_duration_secs: inner.limits.lease_duration_secs,
        },
        &mut port,
    );
    let mut service_report = inner.report.lock().expect("learning service report lock");
    service_report.cycles = service_report.cycles.saturating_add(1);
    let mut attachment_report = attachment
        .report
        .lock()
        .expect("learning attachment report lock");
    match outcome {
        Ok(MemoryLearningCycleOutcome::Idle { reason }) => {
            attachment_report.state = "idle".to_string();
            attachment_report.reason = reason;
            false
        }
        Ok(MemoryLearningCycleOutcome::Completed(report)) => {
            service_report.completed_jobs = service_report.completed_jobs.saturating_add(1);
            service_report.reason = "governance_job_succeeded".to_string();
            attachment_report.state = "idle".to_string();
            attachment_report.last_job_id = Some(report.job.job_id);
            attachment_report.reason = "governance_job_succeeded".to_string();
            true
        }
        Ok(MemoryLearningCycleOutcome::Retrying(report)) => {
            service_report.retrying_jobs = service_report.retrying_jobs.saturating_add(1);
            service_report.reason = report.reason.clone();
            attachment_report.state = "retrying".to_string();
            attachment_report.last_job_id = Some(report.job.job_id);
            attachment_report.reason = report.reason;
            true
        }
        Ok(MemoryLearningCycleOutcome::Blocked(report)) => {
            let changed = attachment_report.state != "blocked"
                || attachment_report.last_job_id.as_deref() != Some(&report.job.job_id)
                || attachment_report.reason != report.reason;
            if changed {
                service_report.blocked_jobs = service_report.blocked_jobs.saturating_add(1);
            }
            service_report.reason = report.reason.clone();
            attachment_report.state = "blocked".to_string();
            attachment_report.last_job_id = Some(report.job.job_id);
            attachment_report.reason = report.reason;
            false
        }
        Ok(MemoryLearningCycleOutcome::Cancelled(report)) => {
            service_report.cancelled_jobs = service_report.cancelled_jobs.saturating_add(1);
            service_report.reason = report.reason.clone();
            attachment_report.state = "cancelled".to_string();
            attachment_report.last_job_id = Some(report.job.job_id);
            attachment_report.reason = report.reason;
            true
        }
        Ok(MemoryLearningCycleOutcome::Failed(report)) => {
            service_report.failed_jobs = service_report.failed_jobs.saturating_add(1);
            service_report.reason = report.reason.clone();
            attachment_report.state = "failed".to_string();
            attachment_report.last_job_id = Some(report.job.job_id);
            attachment_report.reason = report.reason;
            true
        }
        Err(error) => {
            service_report.reason = format!("learning_cycle_failed:{}", error.stage());
            attachment_report.state = "retrying".to_string();
            attachment_report.reason = service_report.reason.clone();
            false
        }
    }
}

#[cfg_attr(not(feature = "governance-model-client-std"), allow(dead_code))]
struct OfficialLearningExecutionPort {
    credential_resolver: Arc<dyn GovernanceCredentialResolver>,
    stop: Arc<AtomicBool>,
    attachment_active: Arc<AtomicBool>,
}

impl GovernanceExecutionPort for OfficialLearningExecutionPort {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        binding: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        #[cfg(not(feature = "governance-model-client-std"))]
        {
            let _ = (binding, egress, operation);
            Err(GovernanceExecutionPortFailure::CapabilityUnavailable)
        }
        #[cfg(feature = "governance-model-client-std")]
        {
            ensure_learning_execution_active(&self.stop, &self.attachment_active)
                .map_err(GovernanceExecutionPortFailure::Other)?;
            egress
                .revalidate_before_egress()
                .map_err(GovernanceExecutionPortFailure::Other)?;
            let credential = binding
                .credential_reference
                .as_ref()
                .map(|reference| {
                    let safe_id = credential_reference_safe_id(reference);
                    let request = GovernanceCredentialRequest {
                        binding_id: binding.binding_id.clone(),
                        binding_revision: binding.binding_revision,
                        credential_reference: reference.clone(),
                        credential_reference_safe_id: safe_id.clone(),
                        purpose: "post_turn_long_term_learning",
                    };
                    match self.credential_resolver.resolve(&request) {
                        Ok(resolved) => {
                            if resolved.reference_safe_id != safe_id {
                                return Err(GovernanceExecutionPortFailure::Other(
                                    bm_sdk::Error::conflict(
                                        "governance_credential",
                                        "resolver returned a different credential reference",
                                    ),
                                ));
                            }
                            Ok(resolved)
                        }
                        Err(error) if error.stage() == "governance_credential_missing" => {
                            Err(GovernanceExecutionPortFailure::CredentialMissing {
                                credential_ref_safe_id: safe_id,
                            })
                        }
                        Err(error) if error.stage() == "governance_credential_locked" => {
                            Err(GovernanceExecutionPortFailure::CredentialLocked {
                                credential_ref_safe_id: safe_id,
                            })
                        }
                        Err(error) => Err(GovernanceExecutionPortFailure::Other(error)),
                    }
                })
                .transpose()?;
            egress
                .revalidate_before_egress()
                .map_err(GovernanceExecutionPortFailure::Other)?;
            let credential_authority = credential
                .as_ref()
                .map(|credential| (credential.reference_safe_id.clone(), credential.generation));
            let mut http = crate::ReqwestGovernanceLlmHttpClient::for_endpoint(
                &binding.endpoint,
                binding.request_timeout_ms,
                DEFAULT_RESPONSE_MAX_BYTES,
            )
            .map_err(GovernanceExecutionPortFailure::Other)?;
            let llm = ConfiguredGovernanceLlmClient::new(
                GovernanceProviderBinding::llm_binding(binding)
                    .map_err(GovernanceExecutionPortFailure::Other)?,
            );
            let mut authorized_http = CredentialInjectingHttpClient {
                inner: &mut http,
                credential,
                stop: Arc::clone(&self.stop),
                attachment_active: Arc::clone(&self.attachment_active),
            };
            match operation.run(&mut authorized_http, &llm) {
                Err(bm_sdk::Error::Http {
                    status_code: 401, ..
                }) => {
                    let (credential_ref_safe_id, credential_generation) = credential_authority
                        .ok_or_else(|| {
                            GovernanceExecutionPortFailure::Other(bm_sdk::Error::config(
                                "governance_credential",
                                "remote authentication rejection lacks credential authority",
                            ))
                        })?;
                    Err(GovernanceExecutionPortFailure::CredentialRejected {
                        credential_ref_safe_id,
                        credential_generation,
                    })
                }
                Err(bm_sdk::Error::Http {
                    status_code: 403, ..
                }) => Err(GovernanceExecutionPortFailure::ProviderPermissionDenied {
                    provider_permission_generation: binding.provider_permission_generation,
                }),
                Err(error) => Err(GovernanceExecutionPortFailure::Other(error)),
                Ok(()) => Ok(()),
            }
        }
    }
}

fn credential_reference_safe_id(reference: &str) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(format!("governance_credential_ref_v1\0{reference}"))
    )
}

#[cfg(feature = "governance-model-client-std")]
struct CredentialInjectingHttpClient<'a> {
    inner: &'a mut dyn bm_sdk::LlmHttpClient,
    credential: Option<ResolvedGovernanceCredential>,
    stop: Arc<AtomicBool>,
    attachment_active: Arc<AtomicBool>,
}

#[cfg(feature = "governance-model-client-std")]
impl bm_sdk::LlmHttpClient for CredentialInjectingHttpClient<'_> {
    fn do_post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> bm_sdk::Result<(u16, bm_sdk::ResponseBody)> {
        ensure_learning_execution_active(&self.stop, &self.attachment_active)?;
        let authorization = self
            .credential
            .as_ref()
            .map(ResolvedGovernanceCredential::authorization_value)
            .transpose()?;
        let mut exact_headers = headers.to_vec();
        if let Some(authorization) = authorization.as_deref() {
            exact_headers.push(("authorization", authorization));
        }
        let response = self.inner.do_post(url, &exact_headers, body)?;
        ensure_learning_execution_active(&self.stop, &self.attachment_active)?;
        Ok(response)
    }

    fn do_post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> bm_sdk::Result<()>,
    ) -> bm_sdk::Result<u16> {
        ensure_learning_execution_active(&self.stop, &self.attachment_active)?;
        let authorization = self
            .credential
            .as_ref()
            .map(ResolvedGovernanceCredential::authorization_value)
            .transpose()?;
        let mut exact_headers = headers.to_vec();
        if let Some(authorization) = authorization.as_deref() {
            exact_headers.push(("authorization", authorization));
        }
        let status = self.inner.do_post_streaming(
            url,
            &exact_headers,
            body,
            max_response_bytes,
            on_chunk,
        )?;
        ensure_learning_execution_active(&self.stop, &self.attachment_active)?;
        Ok(status)
    }
}

#[cfg(feature = "governance-model-client-std")]
fn ensure_learning_execution_active(
    stop: &AtomicBool,
    attachment_active: &AtomicBool,
) -> bm_sdk::Result<()> {
    if stop.load(Ordering::Acquire) || !attachment_active.load(Ordering::Acquire) {
        return Err(bm_sdk::Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "memory learning execution authority is no longer active",
            )),
            stage: "memory_learning_execution_fence",
        });
    }
    Ok(())
}
