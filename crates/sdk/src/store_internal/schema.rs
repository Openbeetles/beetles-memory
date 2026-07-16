use bm_core::feature_gate::ProfileId;
use bm_core::memory::{GovernedEvidenceDocument, GovernedEvidenceSourceRef};
use bm_core::platform::MemorySystemKind;
use bm_core::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store_internal::config::{profile_memory_system_kind, StoreBackendKind};

pub const STORE_SCHEMA_ID: &str = "beetle_memory_store_schema_v5";
pub const STORE_SCHEMA_VERSION: u32 = 5;
pub const GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_SCHEMA_VERSION: u32 = 2;
pub const GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE: &str =
    "governed_evidence_source_claim_manifests";
pub const CONTROL_PLANE_SCOPE_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE: &str = "control_plane_scope_manifests";
pub const RECALL_OWNER_SCOPE_BINDING_SCHEMA_VERSION: u32 = 1;
pub const RECALL_OWNER_SCOPE_BINDING_NAMESPACE: &str = "recall_owner_scope_bindings";

const GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_KEY_DOMAIN: &[u8] =
    b"governed_evidence_source_claim_manifest_key_v1";
const GOVERNED_EVIDENCE_SOURCE_CLAIM_KEYS_DIGEST_DOMAIN: &[u8] =
    b"governed_evidence_source_claim_keys_digest_v2";
const GOVERNED_EVIDENCE_SOURCE_CLAIM_BINDING_DIGEST_DOMAIN: &[u8] =
    b"governed_evidence_source_claim_binding_digest_v2";
const GOVERNED_EVIDENCE_SOURCE_CLAIM_CLOSURE_DIGEST_DOMAIN: &[u8] =
    b"governed_evidence_source_claim_closure_digest_v2";
const CONTROL_PLANE_SCOPE_MANIFEST_KEY_DOMAIN: &[u8] = b"control_plane_scope_manifest_key_v1";
const CONTROL_PLANE_SCOPE_MANIFEST_DIGEST_DOMAIN: &[u8] = b"control_plane_scope_manifest_digest_v1";
const RECALL_OWNER_SCOPE_BINDING_KEY_DOMAIN: &[u8] = b"recall_owner_scope_binding_key_v1";
const RECALL_OWNER_SCOPE_BINDING_DIGEST_DOMAIN: &[u8] = b"recall_owner_scope_binding_digest_v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ControlPlaneScopeEntry {
    pub namespace: String,
    pub key: String,
    pub content_sha256: String,
}

impl ControlPlaneScopeEntry {
    pub fn from_json(namespace: &str, key: &str, value: &serde_json::Value) -> Result<Self> {
        require_scope_component(namespace, "control_plane_scope_manifest", "namespace")?;
        require_scope_component(key, "control_plane_scope_manifest", "key")?;
        Ok(Self {
            namespace: namespace.to_string(),
            key: key.to_string(),
            content_sha256: json_sha256(value, "control_plane_scope_manifest")?,
        })
    }

    pub fn validate_value(&self, value: &serde_json::Value) -> Result<()> {
        let expected = Self::from_json(&self.namespace, &self.key, value)?;
        if self == &expected {
            Ok(())
        } else {
            Err(Error::config(
                "control_plane_scope_manifest",
                "control-plane entry content digest mismatch",
            ))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ControlPlaneScopeManifest {
    pub schema_version: u32,
    pub physical_key: String,
    pub revision: u64,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub entry_count: usize,
    pub entries: Vec<ControlPlaneScopeEntry>,
    pub entries_digest: String,
}

impl ControlPlaneScopeManifest {
    pub fn build(
        revision: u64,
        memory_space_id: &str,
        mounted_subject_id: &str,
        entries: impl IntoIterator<Item = ControlPlaneScopeEntry>,
        max_entries: usize,
    ) -> Result<Self> {
        let memory_space_id = require_scope_component(
            memory_space_id,
            "control_plane_scope_manifest",
            "memory_space_id",
        )?;
        let mounted_subject_id = require_scope_component(
            mounted_subject_id,
            "control_plane_scope_manifest",
            "mounted_subject_id",
        )?;
        if revision == 0 || max_entries == 0 {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "revision and max_entries must be greater than zero",
            ));
        }
        let mut entries = entries.into_iter().collect::<Vec<_>>();
        entries.sort();
        if entries.len() > max_entries
            || entries
                .windows(2)
                .any(|pair| pair[0].namespace == pair[1].namespace && pair[0].key == pair[1].key)
        {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "control-plane entries are duplicate or exceed the pinned limit",
            ));
        }
        let physical_key = control_plane_scope_manifest_key(memory_space_id, mounted_subject_id)?;
        let entries_digest = control_plane_scope_manifest_digest(
            revision,
            memory_space_id,
            mounted_subject_id,
            &entries,
        )?;
        Ok(Self {
            schema_version: CONTROL_PLANE_SCOPE_MANIFEST_SCHEMA_VERSION,
            physical_key,
            revision,
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            entry_count: entries.len(),
            entries,
            entries_digest,
        })
    }

    pub fn validate(&self, max_entries: usize) -> Result<()> {
        let expected = Self::build(
            self.revision,
            &self.memory_space_id,
            &self.mounted_subject_id,
            self.entries.clone(),
            max_entries,
        )?;
        if self == &expected && self.schema_version == CONTROL_PLANE_SCOPE_MANIFEST_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(Error::config(
                "control_plane_scope_manifest",
                "control-plane scope manifest is not canonical",
            ))
        }
    }
}

pub fn control_plane_scope_manifest_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<String> {
    let memory_space_id = require_scope_component(
        memory_space_id,
        "control_plane_scope_manifest",
        "memory_space_id",
    )?;
    let mounted_subject_id = require_scope_component(
        mounted_subject_id,
        "control_plane_scope_manifest",
        "mounted_subject_id",
    )?;
    Ok(format!(
        "cpsm1:{}",
        digest_fields(
            CONTROL_PLANE_SCOPE_MANIFEST_KEY_DOMAIN,
            &[memory_space_id.as_bytes(), mounted_subject_id.as_bytes()],
        )
    ))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecallOwnerScopeBinding {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub owner_kind: String,
    pub owner_namespace: String,
    pub owner_key: String,
    pub owner_content_sha256: String,
    pub binding_digest: String,
}

impl RecallOwnerScopeBinding {
    pub fn build(
        memory_space_id: &str,
        mounted_subject_id: &str,
        owner_kind: &str,
        owner_namespace: &str,
        owner_key: &str,
        owner_content_sha256: &str,
    ) -> Result<Self> {
        let memory_space_id = require_scope_component(
            memory_space_id,
            "recall_owner_scope_binding",
            "memory_space_id",
        )?;
        let mounted_subject_id = require_scope_component(
            mounted_subject_id,
            "recall_owner_scope_binding",
            "mounted_subject_id",
        )?;
        let owner_kind =
            require_scope_component(owner_kind, "recall_owner_scope_binding", "owner_kind")?;
        let owner_namespace = require_scope_component(
            owner_namespace,
            "recall_owner_scope_binding",
            "owner_namespace",
        )?;
        let owner_key =
            require_scope_component(owner_key, "recall_owner_scope_binding", "owner_key")?;
        if !is_sha256_digest(owner_content_sha256) {
            return Err(Error::config(
                "recall_owner_scope_binding",
                "owner content digest is not canonical sha256",
            ));
        }
        let physical_key = recall_owner_scope_binding_key(owner_kind, owner_namespace, owner_key)?;
        let binding_digest = format!(
            "sha256:{}",
            digest_fields(
                RECALL_OWNER_SCOPE_BINDING_DIGEST_DOMAIN,
                &[
                    memory_space_id.as_bytes(),
                    mounted_subject_id.as_bytes(),
                    owner_kind.as_bytes(),
                    owner_namespace.as_bytes(),
                    owner_key.as_bytes(),
                    owner_content_sha256.as_bytes(),
                ],
            )
        );
        Ok(Self {
            schema_version: RECALL_OWNER_SCOPE_BINDING_SCHEMA_VERSION,
            physical_key,
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            owner_kind: owner_kind.to_string(),
            owner_namespace: owner_namespace.to_string(),
            owner_key: owner_key.to_string(),
            owner_content_sha256: owner_content_sha256.to_string(),
            binding_digest,
        })
    }

    pub fn validate(&self) -> Result<()> {
        let expected = Self::build(
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.owner_kind,
            &self.owner_namespace,
            &self.owner_key,
            &self.owner_content_sha256,
        )?;
        if self == &expected && self.schema_version == RECALL_OWNER_SCOPE_BINDING_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(Error::config(
                "recall_owner_scope_binding",
                "recall owner scope binding is not canonical",
            ))
        }
    }
}

pub fn recall_owner_scope_binding_key(
    owner_kind: &str,
    owner_namespace: &str,
    owner_key: &str,
) -> Result<String> {
    for (field, value) in [
        ("owner_kind", owner_kind),
        ("owner_namespace", owner_namespace),
        ("owner_key", owner_key),
    ] {
        require_scope_component(value, "recall_owner_scope_binding", field)?;
    }
    Ok(format!(
        "rosb1:{}",
        digest_fields(
            RECALL_OWNER_SCOPE_BINDING_KEY_DOMAIN,
            &[
                owner_kind.as_bytes(),
                owner_namespace.as_bytes(),
                owner_key.as_bytes(),
            ],
        )
    ))
}

fn control_plane_scope_manifest_digest(
    revision: u64,
    memory_space_id: &str,
    mounted_subject_id: &str,
    entries: &[ControlPlaneScopeEntry],
) -> Result<String> {
    let encoded = serde_json::to_vec(entries)
        .map_err(|error| Error::config("control_plane_scope_manifest", error.to_string()))?;
    Ok(format!(
        "sha256:{}",
        digest_fields(
            CONTROL_PLANE_SCOPE_MANIFEST_DIGEST_DOMAIN,
            &[
                &revision.to_be_bytes(),
                memory_space_id.as_bytes(),
                mounted_subject_id.as_bytes(),
                &encoded,
            ],
        )
    ))
}

fn json_sha256(value: &serde_json::Value, stage: &'static str) -> Result<String> {
    let encoded =
        serde_json::to_vec(value).map_err(|error| Error::config(stage, error.to_string()))?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn digest_fields(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, domain);
    for field in fields {
        hash_field(&mut hasher, field);
    }
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StoreProjectionScope {
    FullStore,
    MemorySpace {
        memory_space_id: String,
        mounted_subject_id: String,
        includes_private: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GovernedEvidenceOwnerClaimBinding {
    pub owner_physical_key: String,
    pub claim_physical_key: String,
    pub owner_revision: u64,
    pub source_revision: u64,
    pub content_digest: String,
    pub binding_digest: String,
}

impl GovernedEvidenceOwnerClaimBinding {
    pub fn from_document_claim(
        document: &GovernedEvidenceDocument,
        claim: &GovernedEvidenceSourceRef,
    ) -> Result<Self> {
        if document.memory_space_id != claim.memory_space_id
            || document.mounted_subject_id != claim.mounted_subject_id
            || document.owner_revision != claim.owner_revision
            || document.source_revision != claim.source_revision
            || document.content_digest != claim.content_digest
        {
            return Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence owner and claim cannot form an exact typed binding",
            ));
        }
        Self::new(
            document.physical_key.clone(),
            claim.physical_key.clone(),
            document.owner_revision,
            document.source_revision,
            document.content_digest.clone(),
        )
    }

    pub fn new(
        owner_physical_key: impl Into<String>,
        claim_physical_key: impl Into<String>,
        owner_revision: u64,
        source_revision: u64,
        content_digest: impl Into<String>,
    ) -> Result<Self> {
        let owner_physical_key = owner_physical_key.into();
        let claim_physical_key = claim_physical_key.into();
        let content_digest = content_digest.into();
        if owner_physical_key.trim().is_empty()
            || claim_physical_key.trim().is_empty()
            || content_digest.trim().is_empty()
            || owner_physical_key != owner_physical_key.trim()
            || claim_physical_key != claim_physical_key.trim()
            || content_digest != content_digest.trim()
            || owner_revision == 0
            || source_revision == 0
        {
            return Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence owner-claim binding is not canonical",
            ));
        }
        let binding_digest = governed_evidence_source_claim_binding_digest(
            &owner_physical_key,
            &claim_physical_key,
            owner_revision,
            source_revision,
            &content_digest,
        );
        Ok(Self {
            owner_physical_key,
            claim_physical_key,
            owner_revision,
            source_revision,
            content_digest,
            binding_digest,
        })
    }

    pub fn validate(&self) -> Result<()> {
        let expected = Self::new(
            self.owner_physical_key.clone(),
            self.claim_physical_key.clone(),
            self.owner_revision,
            self.source_revision,
            self.content_digest.clone(),
        )?;
        if self == &expected {
            Ok(())
        } else {
            Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence owner-claim binding digest mismatch",
            ))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GovernedEvidenceSourceClaimManifest {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub owner_count: usize,
    pub claim_count: usize,
    pub owner_keys: Vec<String>,
    pub claim_keys: Vec<String>,
    pub owner_keys_digest: String,
    pub claim_keys_digest: String,
    pub owner_claim_bindings: Vec<GovernedEvidenceOwnerClaimBinding>,
    pub closure_digest: String,
}

impl GovernedEvidenceSourceClaimManifest {
    pub fn build(
        memory_space_id: &str,
        mounted_subject_id: &str,
        bindings: impl IntoIterator<Item = GovernedEvidenceOwnerClaimBinding>,
        max_scope_entries: usize,
    ) -> Result<Self> {
        let memory_space_id = require_scope_component(
            memory_space_id,
            "governed_evidence_source_claim_manifest",
            "memory_space_id",
        )?;
        let mounted_subject_id = require_scope_component(
            mounted_subject_id,
            "governed_evidence_source_claim_manifest",
            "mounted_subject_id",
        )?;
        let mut owner_claim_bindings = bindings.into_iter().collect::<Vec<_>>();
        for binding in &owner_claim_bindings {
            binding.validate()?;
        }
        owner_claim_bindings.sort_by(|left, right| {
            left.owner_physical_key
                .cmp(&right.owner_physical_key)
                .then_with(|| left.claim_physical_key.cmp(&right.claim_physical_key))
        });
        if owner_claim_bindings.windows(2).any(|pair| {
            pair[0].owner_physical_key == pair[1].owner_physical_key
                || pair[0].claim_physical_key == pair[1].claim_physical_key
        }) {
            return Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence owner-claim bindings contain duplicate owner or claim keys",
            ));
        }
        let owner_keys = owner_claim_bindings
            .iter()
            .map(|binding| binding.owner_physical_key.clone())
            .collect::<Vec<_>>();
        let mut claim_keys = owner_claim_bindings
            .iter()
            .map(|binding| binding.claim_physical_key.clone())
            .collect::<Vec<_>>();
        claim_keys.sort();
        if max_scope_entries == 0
            || owner_keys.len() > max_scope_entries
            || claim_keys.len() > max_scope_entries
        {
            return Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence source claim scope exceeds the pinned profile entry limit",
            ));
        }
        let owner_keys_digest = governed_evidence_source_claim_keys_digest(&owner_keys);
        let claim_keys_digest = governed_evidence_source_claim_keys_digest(&claim_keys);
        let closure_digest = governed_evidence_source_claim_closure_digest(
            memory_space_id,
            mounted_subject_id,
            &owner_claim_bindings,
            &owner_keys_digest,
            &claim_keys_digest,
        );
        Ok(Self {
            schema_version: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_SCHEMA_VERSION,
            physical_key: governed_evidence_source_claim_manifest_key(
                memory_space_id,
                mounted_subject_id,
            )?,
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            owner_count: owner_keys.len(),
            claim_count: claim_keys.len(),
            owner_keys: owner_keys.clone(),
            claim_keys: claim_keys.clone(),
            owner_keys_digest,
            claim_keys_digest,
            owner_claim_bindings,
            closure_digest,
        })
    }

    pub fn validate_exact(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        bindings: impl IntoIterator<Item = GovernedEvidenceOwnerClaimBinding>,
        max_scope_entries: usize,
    ) -> Result<()> {
        let expected = Self::build(
            memory_space_id,
            mounted_subject_id,
            bindings,
            max_scope_entries,
        )?;
        if self == &expected {
            Ok(())
        } else {
            Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence source claim manifest does not match exact scope closure",
            ))
        }
    }

    pub fn binding_for_owner(
        &self,
        owner_physical_key: &str,
    ) -> Option<&GovernedEvidenceOwnerClaimBinding> {
        self.owner_claim_bindings
            .binary_search_by(|binding| binding.owner_physical_key.as_str().cmp(owner_physical_key))
            .ok()
            .map(|index| &self.owner_claim_bindings[index])
    }
}

pub fn governed_evidence_source_claim_manifest_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<String> {
    let memory_space_id = require_scope_component(
        memory_space_id,
        "governed_evidence_source_claim_manifest_key",
        "memory_space_id",
    )?;
    let mounted_subject_id = require_scope_component(
        mounted_subject_id,
        "governed_evidence_source_claim_manifest_key",
        "mounted_subject_id",
    )?;
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_KEY_DOMAIN,
    );
    hash_field(&mut hasher, memory_space_id.as_bytes());
    hash_field(&mut hasher, mounted_subject_id.as_bytes());
    Ok(format!(
        "{}:{:x}",
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
        hasher.finalize()
    ))
}

pub fn validate_governed_evidence_source_claim_scope_closure(
    manifest: Option<&GovernedEvidenceSourceClaimManifest>,
    memory_space_id: &str,
    mounted_subject_id: &str,
    bindings: impl IntoIterator<Item = GovernedEvidenceOwnerClaimBinding>,
    max_scope_entries: usize,
) -> Result<()> {
    let manifest = manifest.ok_or_else(|| {
        Error::config(
            "governed_evidence_source_claim_manifest",
            "evidence source claim scope manifest is missing",
        )
    })?;
    manifest.validate_exact(
        memory_space_id,
        mounted_subject_id,
        bindings,
        max_scope_entries,
    )
}

fn governed_evidence_source_claim_keys_digest(keys: &[String]) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_KEYS_DIGEST_DOMAIN,
    );
    for key in keys {
        hash_field(&mut hasher, key.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn governed_evidence_source_claim_binding_digest(
    owner_physical_key: &str,
    claim_physical_key: &str,
    owner_revision: u64,
    source_revision: u64,
    content_digest: &str,
) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_BINDING_DIGEST_DOMAIN,
    );
    hash_field(&mut hasher, owner_physical_key.as_bytes());
    hash_field(&mut hasher, claim_physical_key.as_bytes());
    hash_field(&mut hasher, &owner_revision.to_be_bytes());
    hash_field(&mut hasher, &source_revision.to_be_bytes());
    hash_field(&mut hasher, content_digest.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn governed_evidence_source_claim_closure_digest(
    memory_space_id: &str,
    mounted_subject_id: &str,
    bindings: &[GovernedEvidenceOwnerClaimBinding],
    owner_keys_digest: &str,
    claim_keys_digest: &str,
) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_CLOSURE_DIGEST_DOMAIN,
    );
    hash_field(&mut hasher, memory_space_id.as_bytes());
    hash_field(&mut hasher, mounted_subject_id.as_bytes());
    hash_field(&mut hasher, owner_keys_digest.as_bytes());
    hash_field(&mut hasher, claim_keys_digest.as_bytes());
    for binding in bindings {
        hash_field(&mut hasher, binding.binding_digest.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn require_scope_component<'a>(
    value: &'a str,
    stage: &'static str,
    field: &str,
) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::config(stage, format!("{field} must not be empty")));
    }
    Ok(value)
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoreSchemaManifest {
    pub schema_id: String,
    pub schema_version: u32,
    pub backend: String,
    pub profile: String,
    pub memory_system_kind: String,
    pub projection_scope: StoreProjectionScope,
    pub created_at_unix_secs: u64,
    pub last_opened_at_unix_secs: u64,
}

impl StoreSchemaManifest {
    pub fn new(backend: StoreBackendKind, profile: ProfileId, now_secs: u64) -> Self {
        Self {
            schema_id: STORE_SCHEMA_ID.to_string(),
            schema_version: STORE_SCHEMA_VERSION,
            backend: backend.as_str().to_string(),
            profile: profile.as_str().to_string(),
            memory_system_kind: profile_memory_system_kind(profile).as_str().to_string(),
            projection_scope: StoreProjectionScope::FullStore,
            created_at_unix_secs: now_secs,
            last_opened_at_unix_secs: now_secs,
        }
    }

    pub fn touch_opened(&mut self, now_secs: u64) {
        self.last_opened_at_unix_secs = now_secs;
    }

    pub fn validate_against(
        &self,
        backend: StoreBackendKind,
        profile: ProfileId,
        memory_system_kind: MemorySystemKind,
        stage: &'static str,
    ) -> Result<()> {
        if self.schema_id != STORE_SCHEMA_ID {
            return Err(Error::config(
                stage,
                format!("unsupported schema {}", self.schema_id),
            ));
        }
        if self.schema_version != STORE_SCHEMA_VERSION {
            return Err(Error::config(
                stage,
                format!("unsupported schema version {}", self.schema_version),
            ));
        }
        if self.backend != backend.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "backend mismatch: manifest={}, config={}",
                    self.backend,
                    backend.as_str()
                ),
            ));
        }
        if self.profile != profile.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "profile mismatch: manifest={}, config={}",
                    self.profile,
                    profile.as_str()
                ),
            ));
        }
        if self.memory_system_kind != memory_system_kind.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "memory system kind mismatch: manifest={}, config={}",
                    self.memory_system_kind,
                    memory_system_kind.as_str()
                ),
            ));
        }
        if self.projection_scope != StoreProjectionScope::FullStore {
            return Err(Error::config(
                stage,
                "store manifest must use full_store projection scope",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(owner: &str, claim: &str, revision: u64) -> GovernedEvidenceOwnerClaimBinding {
        GovernedEvidenceOwnerClaimBinding::new(
            owner,
            claim,
            revision,
            revision,
            format!("content:{revision}"),
        )
        .unwrap()
    }

    #[test]
    fn source_claim_manifest_is_order_independent_and_rejects_extra_claims() {
        let manifest = GovernedEvidenceSourceClaimManifest::build(
            "space:a",
            "subject:a",
            [
                binding("owner:b", "claim:b", 2),
                binding("owner:a", "claim:a", 1),
            ],
            8,
        )
        .unwrap();
        validate_governed_evidence_source_claim_scope_closure(
            Some(&manifest),
            "space:a",
            "subject:a",
            [
                binding("owner:a", "claim:a", 1),
                binding("owner:b", "claim:b", 2),
            ],
            8,
        )
        .unwrap();
        assert!(validate_governed_evidence_source_claim_scope_closure(
            Some(&manifest),
            "space:a",
            "subject:a",
            [
                binding("owner:a", "claim:a", 1),
                binding("owner:b", "claim:b", 2),
                binding("owner:extra", "claim:extra", 3),
            ],
            8,
        )
        .is_err());
        assert!(validate_governed_evidence_source_claim_scope_closure(
            None,
            "space:a",
            "subject:a",
            [
                binding("owner:a", "claim:a", 1),
                binding("owner:b", "claim:b", 2)
            ],
            8,
        )
        .is_err());
    }

    #[test]
    fn source_claim_manifest_v1_shape_fails_closed() {
        let old = serde_json::json!({
            "schema_version": 1,
            "physical_key": "manifest",
            "memory_space_id": "space:a",
            "mounted_subject_id": "subject:a",
            "owner_count": 1,
            "claim_count": 1,
            "owner_keys": ["owner:a"],
            "claim_keys": ["claim:a"],
            "owner_keys_digest": "old",
            "claim_keys_digest": "old"
        });
        assert!(serde_json::from_value::<GovernedEvidenceSourceClaimManifest>(old).is_err());
    }

    #[test]
    fn source_claim_manifest_key_is_scope_bound() {
        let first = governed_evidence_source_claim_manifest_key("space:a", "subject:a").unwrap();
        let other_space =
            governed_evidence_source_claim_manifest_key("space:b", "subject:a").unwrap();
        let other_subject =
            governed_evidence_source_claim_manifest_key("space:a", "subject:b").unwrap();
        assert_ne!(first, other_space);
        assert_ne!(first, other_subject);
    }
}
