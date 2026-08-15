use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bm_sdk::{Error, Result, StoreBackendConfig, StoreBackendKind};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use url::{Host, Url};

const GOVERNANCE_MODEL_SCHEMA_VERSION: u32 = 3;
const DEFAULT_RESPONSE_MAX_BYTES: usize = 256 * 1024;
const MAX_INPUT_TOKENS: usize = 1_000_000;
const MAX_OUTPUT_TOKENS: usize = 131_072;
const REVISION_PREFIX: &str = "revision-";
const REVISION_SUFFIX: &str = ".json";
const LOCK_FILE: &str = ".binding.lock";
const PENDING_FILE: &str = ".revision.pending";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryGovernanceModelProtocol {
    OpenAiCompatible,
    OllamaNative,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryGovernanceModelAuthMode {
    CredentialEnv {
        #[serde(rename = "credentialEnv")]
        credential_env: String,
    },
    LocalUnauthenticated,
}

impl EntryGovernanceModelAuthMode {
    pub fn credential_env(&self) -> Option<&str> {
        match self {
            Self::CredentialEnv { credential_env } => Some(credential_env),
            Self::LocalUnauthenticated => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryGovernanceModelConfigUpdate {
    pub enabled: bool,
    pub protocol: EntryGovernanceModelProtocol,
    pub endpoint: String,
    pub model: String,
    pub auth_mode: EntryGovernanceModelAuthMode,
    pub request_timeout_ms: u64,
    pub max_input_tokens: usize,
    pub max_output_tokens: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryGovernanceModelConfigView {
    pub configured: bool,
    pub readiness: String,
    pub persistence: String,
    pub binding_id: Option<String>,
    pub enabled: bool,
    pub protocol: Option<EntryGovernanceModelProtocol>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub auth_mode: Option<EntryGovernanceModelAuthMode>,
    pub credential_env: Option<String>,
    pub credential_configured: bool,
    pub request_timeout_ms: Option<u64>,
    pub max_input_tokens: Option<usize>,
    pub max_output_tokens: Option<usize>,
    pub config_revision: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryGovernanceModelProbePlan {
    pub protocol: EntryGovernanceModelProtocol,
    pub url: String,
    pub model: String,
    pub auth_mode: EntryGovernanceModelAuthMode,
    pub request_timeout_ms: u64,
    pub response_max_bytes: usize,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryGovernanceModelExecutionBinding {
    pub binding_id: String,
    pub protocol: EntryGovernanceModelProtocol,
    pub endpoint: String,
    pub model: String,
    pub auth_mode: EntryGovernanceModelAuthMode,
    pub request_timeout_ms: u64,
    pub max_input_tokens: usize,
    pub max_output_tokens: usize,
    pub config_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredGovernanceModelBinding {
    schema_version: u32,
    binding_id: String,
    enabled: bool,
    protocol: EntryGovernanceModelProtocol,
    endpoint: String,
    model: String,
    auth_mode: EntryGovernanceModelAuthMode,
    request_timeout_ms: u64,
    max_input_tokens: usize,
    max_output_tokens: usize,
    config_revision: u64,
}

struct PersistentBindingPaths {
    legacy_path: PathBuf,
    binding_dir: PathBuf,
}

pub(crate) struct EntryGovernanceModelStore {
    paths: Option<PersistentBindingPaths>,
    binding_id: String,
    state: Mutex<Option<StoredGovernanceModelBinding>>,
    ephemeral_revisions: Mutex<BTreeMap<u64, StoredGovernanceModelBinding>>,
}

impl EntryGovernanceModelStore {
    pub(crate) fn open(
        store: &StoreBackendConfig,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel: &str,
        chat_id: &str,
        profile: &str,
    ) -> Result<Self> {
        let binding_id = binding_id(
            memory_space_id,
            mounted_subject_id,
            channel,
            chat_id,
            profile,
        );
        let paths = governance_model_paths(store, &binding_id);
        if paths
            .as_ref()
            .is_some_and(|paths| paths.legacy_path.exists())
        {
            return Err(Error::config(
                "entry_governance_model",
                "legacy_reset_required",
            ));
        }
        let binding = paths
            .as_ref()
            .map(|paths| read_latest_binding(&paths.binding_dir, &binding_id))
            .transpose()?
            .flatten();
        let ephemeral_revisions = binding
            .iter()
            .map(|binding| (binding.config_revision, binding.clone()))
            .collect();
        Ok(Self {
            paths,
            binding_id,
            state: Mutex::new(binding),
            ephemeral_revisions: Mutex::new(ephemeral_revisions),
        })
    }

    pub(crate) fn view(&self) -> EntryGovernanceModelConfigView {
        if self.refresh_latest_binding().is_err() {
            return EntryGovernanceModelConfigView {
                configured: false,
                readiness: "blocked_config_read".to_string(),
                persistence: self.persistence_label().to_string(),
                binding_id: None,
                enabled: false,
                protocol: None,
                endpoint: None,
                model: None,
                auth_mode: None,
                credential_env: None,
                credential_configured: false,
                request_timeout_ms: None,
                max_input_tokens: None,
                max_output_tokens: None,
                config_revision: None,
            };
        }
        let state = self.state.lock().expect("governance model config lock");
        match state.as_ref() {
            Some(binding) => EntryGovernanceModelConfigView {
                configured: true,
                readiness: if binding.enabled { "ready" } else { "disabled" }.to_string(),
                persistence: self.persistence_label().to_string(),
                binding_id: Some(binding.binding_id.clone()),
                enabled: binding.enabled,
                protocol: Some(binding.protocol),
                endpoint: Some(binding.endpoint.clone()),
                model: Some(binding.model.clone()),
                auth_mode: Some(binding.auth_mode.clone()),
                credential_env: binding.auth_mode.credential_env().map(str::to_string),
                credential_configured: binding.auth_mode.credential_env().is_some(),
                request_timeout_ms: Some(binding.request_timeout_ms),
                max_input_tokens: Some(binding.max_input_tokens),
                max_output_tokens: Some(binding.max_output_tokens),
                config_revision: Some(binding.config_revision),
            },
            None => EntryGovernanceModelConfigView {
                configured: false,
                readiness: "not_configured".to_string(),
                persistence: self.persistence_label().to_string(),
                binding_id: None,
                enabled: false,
                protocol: None,
                endpoint: None,
                model: None,
                auth_mode: None,
                credential_env: None,
                credential_configured: false,
                request_timeout_ms: None,
                max_input_tokens: None,
                max_output_tokens: None,
                config_revision: None,
            },
        }
    }

    pub(crate) fn update(
        &self,
        update: EntryGovernanceModelConfigUpdate,
    ) -> Result<EntryGovernanceModelConfigView> {
        let update = normalize_and_validate_update(update)?;
        let mut state = self.state.lock().expect("governance model config lock");
        let binding = if let Some(paths) = self.paths.as_ref() {
            fs::create_dir_all(&paths.binding_dir).map_err(|error| {
                Error::config(
                    "entry_governance_model",
                    format!("failed to create binding directory: {error}"),
                )
            })?;
            let lock = open_binding_lock(&paths.binding_dir)?;
            lock.lock().map_err(|error| {
                Error::config(
                    "entry_governance_model",
                    format!("failed to lock governance model binding: {error}"),
                )
            })?;
            let latest = read_latest_binding(&paths.binding_dir, &self.binding_id)?;
            let binding = build_binding(&self.binding_id, update, latest.as_ref())?;
            write_immutable_revision(&paths.binding_dir, &binding)?;
            binding
        } else {
            let binding = build_binding(&self.binding_id, update, state.as_ref())?;
            self.ephemeral_revisions
                .lock()
                .expect("governance model revision lock")
                .insert(binding.config_revision, binding.clone());
            binding
        };
        *state = Some(binding);
        drop(state);
        Ok(self.view())
    }

    pub(crate) fn probe_plan(&self) -> Result<EntryGovernanceModelProbePlan> {
        self.refresh_latest_binding()?;
        let state = self.state.lock().expect("governance model config lock");
        let binding = state.as_ref().ok_or_else(|| {
            Error::config(
                "entry_governance_model",
                "memory governance model is not configured",
            )
        })?;
        if !binding.enabled {
            return Err(Error::config(
                "entry_governance_model",
                "memory governance model is disabled",
            ));
        }
        let (suffix, body) = match binding.protocol {
            EntryGovernanceModelProtocol::OpenAiCompatible => (
                "chat/completions",
                json!({
                    "model": binding.model,
                    "messages": [{"role": "user", "content": "Reply with OK."}],
                    "stream": false,
                    "max_tokens": 8
                }),
            ),
            EntryGovernanceModelProtocol::OllamaNative => (
                "chat",
                json!({
                    "model": binding.model,
                    "messages": [{"role": "user", "content": "Reply with OK."}],
                    "stream": false,
                    "think": false,
                    "options": {"num_predict": 8}
                }),
            ),
        };
        Ok(EntryGovernanceModelProbePlan {
            protocol: binding.protocol,
            url: format!("{}/{suffix}", binding.endpoint.trim_end_matches('/')),
            model: binding.model.clone(),
            auth_mode: binding.auth_mode.clone(),
            request_timeout_ms: binding.request_timeout_ms,
            response_max_bytes: DEFAULT_RESPONSE_MAX_BYTES,
            body: serde_json::to_vec(&body)
                .map_err(|error| Error::config("entry_governance_model", error.to_string()))?,
        })
    }

    pub(crate) fn execution_binding(&self) -> Result<EntryGovernanceModelExecutionBinding> {
        self.refresh_latest_binding()?;
        let state = self.state.lock().expect("governance model config lock");
        let binding = state.as_ref().ok_or_else(|| {
            Error::config(
                "entry_governance_model",
                "memory governance model is not configured",
            )
        })?;
        if !binding.enabled {
            return Err(Error::config(
                "entry_governance_model",
                "memory governance model is disabled",
            ));
        }
        execution_binding_from_stored(binding)
    }

    pub(crate) fn execution_binding_for_revision(
        &self,
        config_revision: u64,
    ) -> Result<EntryGovernanceModelExecutionBinding> {
        if config_revision == 0 {
            return Err(Error::invalid_input(
                "entry_governance_model",
                "config revision must be greater than zero",
            ));
        }
        let binding = if let Some(paths) = self.paths.as_ref() {
            let path = revision_path(&paths.binding_dir, config_revision);
            if !path.exists() {
                return Err(Error::not_found(
                    "entry_governance_model",
                    "governance model config revision not found",
                ));
            }
            let binding = read_binding(&path)?;
            validate_stored_binding(&binding, &self.binding_id)?;
            if binding.config_revision != config_revision {
                return Err(Error::config(
                    "entry_governance_model",
                    "governance model revision filename differs from payload",
                ));
            }
            binding
        } else {
            self.ephemeral_revisions
                .lock()
                .expect("governance model revision lock")
                .get(&config_revision)
                .cloned()
                .ok_or_else(|| {
                    Error::not_found(
                        "entry_governance_model",
                        "governance model config revision not found",
                    )
                })?
        };
        execution_binding_from_stored(&binding)
    }

    pub(crate) fn current_policy_revision(&self) -> Result<u64> {
        self.refresh_latest_binding()?;
        Ok(self
            .state
            .lock()
            .expect("governance model config lock")
            .as_ref()
            .map_or(1, |binding| binding.config_revision))
    }

    fn persistence_label(&self) -> &'static str {
        if self.paths.is_some() {
            "durable"
        } else {
            "ephemeral"
        }
    }

    fn refresh_latest_binding(&self) -> Result<()> {
        let Some(paths) = self.paths.as_ref() else {
            return Ok(());
        };
        let latest = read_latest_binding(&paths.binding_dir, &self.binding_id)?;
        *self.state.lock().expect("governance model config lock") = latest;
        Ok(())
    }
}

fn governance_model_paths(
    store: &StoreBackendConfig,
    binding_id: &str,
) -> Option<PersistentBindingPaths> {
    if matches!(
        store.backend(),
        StoreBackendKind::InMemory | StoreBackendKind::Embedded
    ) {
        return None;
    }
    let data_path = store.data_path()?;
    let legacy_path = append_suffix(data_path, ".memory-governance-model.json");
    let root = append_suffix(data_path, ".memory-governance-model");
    let binding_name = binding_id
        .strip_prefix("governance-model:")
        .unwrap_or(binding_id);
    Some(PersistentBindingPaths {
        legacy_path,
        binding_dir: root.join(binding_name),
    })
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn binding_id(
    memory_space_id: &str,
    mounted_subject_id: &str,
    channel: &str,
    chat_id: &str,
    profile: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"bm.entry.memory-governance-model-binding.v2\0");
    for value in [
        memory_space_id,
        mounted_subject_id,
        channel,
        chat_id,
        profile,
    ] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    let digest = hasher.finalize();
    let short = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("governance-model:{short}")
}

fn normalize_and_validate_update(
    mut update: EntryGovernanceModelConfigUpdate,
) -> Result<EntryGovernanceModelConfigUpdate> {
    let endpoint = validate_endpoint(&update.endpoint)?;
    update.endpoint = endpoint.as_str().trim_end_matches('/').to_string();
    update.model = update.model.trim().to_string();
    if update.model.is_empty() {
        return Err(Error::invalid_input(
            "entry_governance_model",
            "model must not be empty",
        ));
    }
    if update.model.len() > 256 {
        return Err(Error::invalid_input(
            "entry_governance_model",
            "model exceeds 256 characters",
        ));
    }
    match &mut update.auth_mode {
        EntryGovernanceModelAuthMode::CredentialEnv { credential_env } => {
            *credential_env = credential_env.trim().to_string();
            if !is_valid_env_name(credential_env) {
                return Err(Error::invalid_input(
                    "entry_governance_model",
                    "credential_env must be an environment variable name",
                ));
            }
        }
        EntryGovernanceModelAuthMode::LocalUnauthenticated => {
            if !is_loopback_endpoint(&endpoint) {
                return Err(Error::invalid_input(
                    "entry_governance_model",
                    "remote_endpoint_requires_credential_env",
                ));
            }
        }
    }
    if !(1_000..=600_000).contains(&update.request_timeout_ms) {
        return Err(Error::invalid_input(
            "entry_governance_model",
            "request_timeout_ms must be between 1000 and 600000",
        ));
    }
    if !(1..=MAX_INPUT_TOKENS).contains(&update.max_input_tokens)
        || !(1..=MAX_OUTPUT_TOKENS).contains(&update.max_output_tokens)
    {
        return Err(Error::invalid_input(
            "entry_governance_model",
            "token budgets exceed governance model limits",
        ));
    }
    Ok(update)
}

fn build_binding(
    binding_id: &str,
    update: EntryGovernanceModelConfigUpdate,
    previous: Option<&StoredGovernanceModelBinding>,
) -> Result<StoredGovernanceModelBinding> {
    let config_revision = previous.map_or(Ok(1), |current| {
        current.config_revision.checked_add(1).ok_or_else(|| {
            Error::config(
                "entry_governance_model",
                "governance model config revision overflow",
            )
        })
    })?;
    Ok(StoredGovernanceModelBinding {
        schema_version: GOVERNANCE_MODEL_SCHEMA_VERSION,
        binding_id: binding_id.to_string(),
        enabled: update.enabled,
        protocol: update.protocol,
        endpoint: update.endpoint,
        model: update.model,
        auth_mode: update.auth_mode,
        request_timeout_ms: update.request_timeout_ms,
        max_input_tokens: update.max_input_tokens,
        max_output_tokens: update.max_output_tokens,
        config_revision,
    })
}

fn execution_binding_from_stored(
    binding: &StoredGovernanceModelBinding,
) -> Result<EntryGovernanceModelExecutionBinding> {
    if !binding.enabled {
        return Err(Error::config(
            "entry_governance_model",
            "memory governance model is disabled",
        ));
    }
    Ok(EntryGovernanceModelExecutionBinding {
        binding_id: binding.binding_id.clone(),
        protocol: binding.protocol,
        endpoint: binding.endpoint.clone(),
        model: binding.model.clone(),
        auth_mode: binding.auth_mode.clone(),
        request_timeout_ms: binding.request_timeout_ms,
        max_input_tokens: binding.max_input_tokens,
        max_output_tokens: binding.max_output_tokens,
        config_revision: binding.config_revision,
    })
}

fn validate_stored_binding(
    binding: &StoredGovernanceModelBinding,
    expected_binding_id: &str,
) -> Result<()> {
    if binding.schema_version != GOVERNANCE_MODEL_SCHEMA_VERSION {
        return Err(Error::config(
            "entry_governance_model",
            "unsupported memory governance model schema version",
        ));
    }
    if binding.binding_id != expected_binding_id {
        return Err(Error::config(
            "entry_governance_model",
            "memory governance model binding identity mismatch",
        ));
    }
    let update = EntryGovernanceModelConfigUpdate {
        enabled: binding.enabled,
        protocol: binding.protocol,
        endpoint: binding.endpoint.clone(),
        model: binding.model.clone(),
        auth_mode: binding.auth_mode.clone(),
        request_timeout_ms: binding.request_timeout_ms,
        max_input_tokens: binding.max_input_tokens,
        max_output_tokens: binding.max_output_tokens,
    };
    normalize_and_validate_update(update).map(|_| ())
}

fn validate_endpoint(endpoint: &str) -> Result<Url> {
    let endpoint = endpoint.trim();
    let parsed = Url::parse(endpoint).map_err(|_| {
        Error::invalid_input(
            "entry_governance_model",
            "endpoint must be an absolute http or https URL",
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::invalid_input(
            "entry_governance_model",
            "endpoint must use http or https",
        ));
    }
    if parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(Error::invalid_input(
            "entry_governance_model",
            "endpoint must not contain invalid host, userinfo, query, or fragment",
        ));
    }
    Ok(parsed)
}

fn is_loopback_endpoint(endpoint: &Url) -> bool {
    match endpoint.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

fn is_valid_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|ch| matches!(ch, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

fn open_binding_lock(binding_dir: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(binding_dir.join(LOCK_FILE))
        .map_err(|error| {
            Error::config(
                "entry_governance_model",
                format!("failed to open governance model binding lock: {error}"),
            )
        })
}

fn read_latest_binding(
    binding_dir: &Path,
    expected_binding_id: &str,
) -> Result<Option<StoredGovernanceModelBinding>> {
    if !binding_dir.exists() {
        return Ok(None);
    }
    let mut revisions = Vec::new();
    for entry in fs::read_dir(binding_dir).map_err(|error| {
        Error::config(
            "entry_governance_model",
            format!("failed to list governance model revisions: {error}"),
        )
    })? {
        let entry = entry.map_err(|error| {
            Error::config(
                "entry_governance_model",
                format!("failed to inspect governance model revision: {error}"),
            )
        })?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            Error::config(
                "entry_governance_model",
                "governance model revision filename is not UTF-8",
            )
        })?;
        if let Some(revision) = parse_revision_name(name) {
            revisions.push((revision, entry.path()));
        } else if !matches!(name, LOCK_FILE | PENDING_FILE) {
            return Err(Error::config(
                "entry_governance_model",
                "unexpected file in governance model binding directory",
            ));
        }
    }
    revisions.sort_by_key(|(revision, _)| *revision);
    let Some((revision, path)) = revisions.last() else {
        return Ok(None);
    };
    let binding = read_binding(path)?;
    validate_stored_binding(&binding, expected_binding_id)?;
    if binding.config_revision != *revision {
        return Err(Error::config(
            "entry_governance_model",
            "governance model revision filename differs from payload",
        ));
    }
    Ok(Some(binding))
}

fn parse_revision_name(name: &str) -> Option<u64> {
    let digits = name
        .strip_prefix(REVISION_PREFIX)?
        .strip_suffix(REVISION_SUFFIX)?;
    (digits.len() == 20 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

fn revision_path(binding_dir: &Path, revision: u64) -> PathBuf {
    binding_dir.join(format!("{REVISION_PREFIX}{revision:020}{REVISION_SUFFIX}"))
}

fn read_binding(path: &Path) -> Result<StoredGovernanceModelBinding> {
    let bytes = fs::read(path).map_err(|error| {
        Error::config(
            "entry_governance_model",
            format!("failed to read governance model config: {error}"),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Error::config(
            "entry_governance_model",
            format!("invalid governance model config: {error}"),
        )
    })
}

fn write_immutable_revision(
    binding_dir: &Path,
    binding: &StoredGovernanceModelBinding,
) -> Result<()> {
    let final_path = revision_path(binding_dir, binding.config_revision);
    if final_path.exists() {
        return Err(Error::conflict(
            "entry_governance_model",
            "governance model revision already exists",
        ));
    }
    let pending_path = binding_dir.join(PENDING_FILE);
    if pending_path.exists() {
        fs::remove_file(&pending_path).map_err(|error| {
            Error::config(
                "entry_governance_model",
                format!("failed to remove stale pending revision: {error}"),
            )
        })?;
    }
    let bytes = serde_json::to_vec_pretty(binding)
        .map_err(|error| Error::config("entry_governance_model", error.to_string()))?;
    let mut pending = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending_path)
        .map_err(|error| {
            Error::config(
                "entry_governance_model",
                format!("failed to create pending governance model revision: {error}"),
            )
        })?;
    pending.write_all(&bytes).map_err(|error| {
        Error::config(
            "entry_governance_model",
            format!("failed to write pending governance model revision: {error}"),
        )
    })?;
    pending.sync_all().map_err(|error| {
        Error::config(
            "entry_governance_model",
            format!("failed to sync pending governance model revision: {error}"),
        )
    })?;
    fs::rename(&pending_path, &final_path).map_err(|error| {
        Error::config(
            "entry_governance_model",
            format!("failed to publish governance model revision: {error}"),
        )
    })?;
    File::open(binding_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            Error::config(
                "entry_governance_model",
                format!("failed to sync governance model binding directory: {error}"),
            )
        })
}
