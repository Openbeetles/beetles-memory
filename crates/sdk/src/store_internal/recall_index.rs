use std::collections::BTreeSet;

use bm_core::{Error, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const RECALL_INDEX_SCHEMA_VERSION: u32 = 1;
pub(crate) const CONVERSATION_RECALL_MANIFEST_NAMESPACE: &str = "conversation_recall_manifests";
pub(crate) const ARCHIVE_RECALL_MANIFEST_NAMESPACE: &str = "archive_recall_manifests";
pub(crate) const RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE: &str = "runtime_skill_recall_manifests";
pub(crate) const CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE: &str =
    "continuity_capsule_scope_indexes";
pub(crate) const ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE: &str = "active_task_run_by_chat_indexes";
pub(crate) const TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE: &str = "task_learning_by_chat_indexes";

pub(crate) const MAX_CONVERSATION_RECALL_ENTRIES: usize = 512;
pub(crate) const MAX_ARCHIVE_RECALL_ENTRIES: usize = 512;
pub(crate) const MAX_RUNTIME_SKILL_RECALL_ENTRIES: usize = 128;
pub(crate) const MAX_CONTINUITY_SCOPE_RECALL_ENTRIES: usize = 16;
pub(crate) const MAX_ACTIVE_TASK_RUN_RECALL_ENTRIES: usize = 16;
pub(crate) const MAX_TASK_LEARNING_RECALL_ENTRIES: usize = 64;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecallIndexAddressKind {
    Json,
    Blob,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecallIndexAddress {
    pub kind: RecallIndexAddressKind,
    pub namespace: String,
    pub key: String,
    pub revision: u64,
    pub updated_at: u64,
    pub content_sha256: String,
}

impl RecallIndexAddress {
    pub(crate) fn json(
        namespace: &str,
        key: &str,
        revision: u64,
        updated_at: u64,
        value: &serde_json::Value,
    ) -> Result<Self> {
        let bytes = serde_json::to_vec(value)
            .map_err(|error| Error::config("recall_index_address", error.to_string()))?;
        Self::new(
            RecallIndexAddressKind::Json,
            namespace,
            key,
            revision,
            updated_at,
            &bytes,
        )
    }

    pub(crate) fn blob(
        namespace: &str,
        key: &str,
        revision: u64,
        updated_at: u64,
        value: &[u8],
    ) -> Result<Self> {
        Self::new(
            RecallIndexAddressKind::Blob,
            namespace,
            key,
            revision,
            updated_at,
            value,
        )
    }

    fn new(
        kind: RecallIndexAddressKind,
        namespace: &str,
        key: &str,
        revision: u64,
        updated_at: u64,
        value: &[u8],
    ) -> Result<Self> {
        require_component(namespace, "namespace")?;
        require_component(key, "key")?;
        if revision == 0 {
            return Err(Error::config(
                "recall_index_address",
                "revision must be greater than zero",
            ));
        }
        Ok(Self {
            kind,
            namespace: namespace.to_string(),
            key: key.to_string(),
            revision,
            updated_at,
            content_sha256: format!("sha256:{:x}", Sha256::digest(value)),
        })
    }

    pub(crate) fn validate(&self) -> Result<()> {
        require_component(&self.namespace, "namespace")?;
        require_component(&self.key, "key")?;
        if self.revision == 0 || !is_sha256(&self.content_sha256) {
            return Err(Error::config(
                "recall_index_address",
                "address revision or content digest is invalid",
            ));
        }
        Ok(())
    }
}

pub(crate) trait TypedRecallIndex: Serialize + DeserializeOwned + Sized {
    const KIND: &'static str;
    const NAMESPACE: &'static str;
    const MAX_ENTRIES: usize;

    fn schema_version(&self) -> u32;
    fn physical_key(&self) -> &str;
    fn revision(&self) -> u64;
    fn entry_count(&self) -> usize;
    fn entries(&self) -> &[RecallIndexAddress];
    fn entries_digest(&self) -> &str;
    fn expected_physical_key(&self) -> Result<String>;
    fn scope_digest_parts(&self) -> Vec<&str>;

    fn validate(&self) -> Result<()> {
        if self.schema_version() != RECALL_INDEX_SCHEMA_VERSION
            || self.revision() == 0
            || self.entry_count() != self.entries().len()
            || self.entry_count() > Self::MAX_ENTRIES
            || self.physical_key() != self.expected_physical_key()?
        {
            return Err(Error::config(
                "typed_recall_index",
                format!("{} schema, key, revision, or count is invalid", Self::KIND),
            ));
        }
        let mut previous = None;
        for entry in self.entries() {
            entry.validate()?;
            let identity = (entry.kind, entry.namespace.as_str(), entry.key.as_str());
            if previous >= Some(identity) {
                return Err(Error::config(
                    "typed_recall_index",
                    format!("{} entries are not unique canonical order", Self::KIND),
                ));
            }
            previous = Some(identity);
        }
        let expected = recall_index_entries_digest(
            Self::KIND,
            self.schema_version(),
            self.revision(),
            &self.scope_digest_parts(),
            self.entries(),
        )?;
        if self.entries_digest() != expected {
            return Err(Error::config(
                "typed_recall_index",
                format!("{} entries digest mismatch", Self::KIND),
            ));
        }
        Ok(())
    }
}

macro_rules! typed_recall_index {
    (
        $name:ident, $kind:literal, $namespace:ident, $max:ident,
        { $( $field:ident ),+ $(,)? }
    ) => {
        #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct $name {
            pub schema_version: u32,
            pub physical_key: String,
            pub revision: u64,
            $( pub $field: String, )+
            pub entry_count: usize,
            pub entries: Vec<RecallIndexAddress>,
            pub entries_digest: String,
        }

        impl $name {
            pub(crate) fn build(
                revision: u64,
                $( $field: &str, )+
                entries: impl IntoIterator<Item = RecallIndexAddress>,
            ) -> Result<Self> {
                $( require_component($field, stringify!($field))?; )+
                if revision == 0 {
                    return Err(Error::config("typed_recall_index", "revision must be greater than zero"));
                }
                let entries = canonical_entries(entries, $max)?;
                let physical_key = recall_index_physical_key($kind, &[$($field),+])?;
                let entries_digest = recall_index_entries_digest(
                    $kind,
                    RECALL_INDEX_SCHEMA_VERSION,
                    revision,
                    &[$($field),+],
                    &entries,
                )?;
                let value = Self {
                    schema_version: RECALL_INDEX_SCHEMA_VERSION,
                    physical_key,
                    revision,
                    $( $field: $field.to_string(), )+
                    entry_count: entries.len(),
                    entries,
                    entries_digest,
                };
                value.validate()?;
                Ok(value)
            }
        }

        impl TypedRecallIndex for $name {
            const KIND: &'static str = $kind;
            const NAMESPACE: &'static str = $namespace;
            const MAX_ENTRIES: usize = $max;

            fn schema_version(&self) -> u32 { self.schema_version }
            fn physical_key(&self) -> &str { &self.physical_key }
            fn revision(&self) -> u64 { self.revision }
            fn entry_count(&self) -> usize { self.entry_count }
            fn entries(&self) -> &[RecallIndexAddress] { &self.entries }
            fn entries_digest(&self) -> &str { &self.entries_digest }
            fn expected_physical_key(&self) -> Result<String> {
                recall_index_physical_key($kind, &[$(self.$field.as_str()),+])
            }
            fn scope_digest_parts(&self) -> Vec<&str> {
                vec![$(self.$field.as_str()),+]
            }
        }
    };
}

typed_recall_index!(
    ConversationRecallManifest,
    "conversation_recall_manifest_v1",
    CONVERSATION_RECALL_MANIFEST_NAMESPACE,
    MAX_CONVERSATION_RECALL_ENTRIES,
    {
        memory_space_id,
        mounted_subject_id,
        channel_id,
        conversation_id
    }
);
typed_recall_index!(
    ArchiveRecallManifest,
    "archive_recall_manifest_v1",
    ARCHIVE_RECALL_MANIFEST_NAMESPACE,
    MAX_ARCHIVE_RECALL_ENTRIES,
    { memory_space_id, mounted_subject_id }
);
typed_recall_index!(
    RuntimeSkillRecallManifest,
    "runtime_skill_recall_manifest_v1",
    RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE,
    MAX_RUNTIME_SKILL_RECALL_ENTRIES,
    { memory_space_id, agent_id }
);
typed_recall_index!(
    ContinuityCapsuleScopeIndex,
    "continuity_capsule_scope_index_v1",
    CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE,
    MAX_CONTINUITY_SCOPE_RECALL_ENTRIES,
    { memory_space_id, scope_kind, scope_id }
);
typed_recall_index!(
    ActiveTaskRunByChatIndex,
    "active_task_run_by_chat_index_v1",
    ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE,
    MAX_ACTIVE_TASK_RUN_RECALL_ENTRIES,
    { memory_space_id, channel_id, chat_id }
);
typed_recall_index!(
    TaskLearningByChatIndex,
    "task_learning_by_chat_index_v1",
    TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
    MAX_TASK_LEARNING_RECALL_ENTRIES,
    { memory_space_id, channel_id, chat_id }
);

pub(crate) fn decode_typed_recall_index<T: TypedRecallIndex>(
    physical_key: &str,
    value: serde_json::Value,
) -> Result<T> {
    let index = serde_json::from_value::<T>(value).map_err(|error| {
        Error::config(
            "typed_recall_index_decode",
            format!("{} cannot be decoded: {error}", T::KIND),
        )
    })?;
    index.validate()?;
    if physical_key != index.physical_key() {
        return Err(Error::config(
            "typed_recall_index_decode",
            format!("{} storage key does not match payload", T::KIND),
        ));
    }
    Ok(index)
}

pub(crate) fn recall_index_physical_key(kind: &str, scope: &[&str]) -> Result<String> {
    require_component(kind, "kind")?;
    for component in scope {
        require_component(component, "scope")?;
    }
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, b"beetle_recall_index_physical_key_v1");
    hash_field(&mut hasher, kind.as_bytes());
    for component in scope {
        hash_field(&mut hasher, component.as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    Ok(format!("ridx1:{}", &digest[..56]))
}

pub(crate) fn next_entry_revision(
    entries: &[RecallIndexAddress],
    kind: RecallIndexAddressKind,
    namespace: &str,
    key: &str,
) -> u64 {
    entries
        .iter()
        .find(|entry| entry.kind == kind && entry.namespace == namespace && entry.key == key)
        .map(|entry| entry.revision.saturating_add(1))
        .unwrap_or(1)
}

pub(crate) fn replace_recall_index_address(
    entries: &[RecallIndexAddress],
    next: RecallIndexAddress,
) -> Vec<RecallIndexAddress> {
    let identity = (next.kind, next.namespace.clone(), next.key.clone());
    entries
        .iter()
        .filter(|entry| {
            entry.kind != identity.0 || entry.namespace != identity.1 || entry.key != identity.2
        })
        .cloned()
        .chain(std::iter::once(next))
        .collect()
}

pub(crate) fn remove_recall_index_address(
    entries: &[RecallIndexAddress],
    kind: RecallIndexAddressKind,
    namespace: &str,
    key: &str,
) -> Vec<RecallIndexAddress> {
    entries
        .iter()
        .filter(|entry| entry.kind != kind || entry.namespace != namespace || entry.key != key)
        .cloned()
        .collect()
}

fn canonical_entries(
    entries: impl IntoIterator<Item = RecallIndexAddress>,
    max_entries: usize,
) -> Result<Vec<RecallIndexAddress>> {
    let mut entries = entries.into_iter().collect::<Vec<_>>();
    for entry in &entries {
        entry.validate()?;
    }
    entries.sort_by(|left, right| {
        (left.kind, left.namespace.as_str(), left.key.as_str()).cmp(&(
            right.kind,
            right.namespace.as_str(),
            right.key.as_str(),
        ))
    });
    let identities = entries
        .iter()
        .map(|entry| (entry.kind, entry.namespace.as_str(), entry.key.as_str()))
        .collect::<BTreeSet<_>>();
    if identities.len() != entries.len() || entries.len() > max_entries {
        return Err(Error::config(
            "typed_recall_index",
            "entries are duplicate or exceed the hard scope ceiling",
        ));
    }
    Ok(entries)
}

fn recall_index_entries_digest(
    kind: &str,
    schema_version: u32,
    revision: u64,
    scope: &[&str],
    entries: &[RecallIndexAddress],
) -> Result<String> {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, b"beetle_recall_index_entries_digest_v1");
    hash_field(&mut hasher, kind.as_bytes());
    hash_field(&mut hasher, &schema_version.to_be_bytes());
    hash_field(&mut hasher, &revision.to_be_bytes());
    for component in scope {
        hash_field(&mut hasher, component.as_bytes());
    }
    for entry in entries {
        let encoded = serde_json::to_vec(entry)
            .map_err(|error| Error::config("typed_recall_index", error.to_string()))?;
        hash_field(&mut hasher, &encoded);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn require_component(value: &str, field: &str) -> Result<()> {
    if value.is_empty() || value != value.trim() {
        return Err(Error::config(
            "typed_recall_index",
            format!("{field} must be canonical and non-empty"),
        ));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_index_rejects_old_schema_and_digest_drift() {
        let address = RecallIndexAddress::blob("skills", "a.md", 1, 10, b"body").unwrap();
        let index = RuntimeSkillRecallManifest::build(1, "space", "agent", [address]).unwrap();
        index.validate().unwrap();

        let mut old = index.clone();
        old.schema_version = 0;
        assert!(old.validate().is_err());
        let mut drifted = index;
        drifted.entries[0].key = "b.md".to_string();
        assert!(drifted.validate().is_err());
    }

    #[test]
    fn physical_key_is_scope_bound_and_bounded() {
        let first = recall_index_physical_key("archive_recall_manifest_v1", &["a", "b"]).unwrap();
        let second = recall_index_physical_key("archive_recall_manifest_v1", &["a", "c"]).unwrap();
        assert_ne!(first, second);
        assert!(first.len() <= 64);
    }

    #[test]
    fn all_six_typed_indexes_enforce_key_count_and_digest() {
        let address =
            RecallIndexAddress::json("owner", "key", 1, 9, &serde_json::json!({"value": 1}))
                .unwrap();
        let indexes: Vec<Box<dyn Fn() -> Result<()>>> = vec![
            Box::new({
                let address = address.clone();
                move || {
                    ConversationRecallManifest::build(
                        1,
                        "ms",
                        "subject",
                        "ch",
                        "cv",
                        [address.clone()],
                    )?
                    .validate()
                }
            }),
            Box::new({
                let address = address.clone();
                move || {
                    ArchiveRecallManifest::build(1, "ms", "subject", [address.clone()])?.validate()
                }
            }),
            Box::new({
                let address = address.clone();
                move || {
                    RuntimeSkillRecallManifest::build(1, "ms", "agent", [address.clone()])?
                        .validate()
                }
            }),
            Box::new({
                let address = address.clone();
                move || {
                    ContinuityCapsuleScopeIndex::build(
                        1,
                        "ms",
                        "chat",
                        "chat-1",
                        [address.clone()],
                    )?
                    .validate()
                }
            }),
            Box::new({
                let address = address.clone();
                move || {
                    ActiveTaskRunByChatIndex::build(1, "ms", "ch", "chat", [address.clone()])?
                        .validate()
                }
            }),
            Box::new(move || {
                TaskLearningByChatIndex::build(1, "ms", "ch", "chat", [address.clone()])?.validate()
            }),
        ];
        for validate in indexes {
            validate().unwrap();
        }
    }
}
