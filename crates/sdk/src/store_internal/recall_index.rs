use std::collections::BTreeSet;

use bm_core::{Error, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const RECALL_INDEX_SCHEMA_VERSION: u32 = 1;
pub(crate) const CONVERSATION_RECALL_MANIFEST_NAMESPACE: &str = "conversation_recall_manifests";
pub(crate) const CONVERSATION_TRANSCRIPT_PAGE_NAMESPACE: &str = "conversation_transcript_pages";
pub(crate) const CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE: &str =
    "conversation_transcript_aux_manifests";
pub(crate) const ARCHIVE_RECALL_MANIFEST_NAMESPACE: &str = "archive_recall_manifests";
pub(crate) const CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE: &str =
    "continuity_capsule_scope_indexes";
pub(crate) const ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE: &str = "active_task_run_by_chat_indexes";
pub(crate) const TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE: &str = "task_learning_by_chat_indexes";

pub(crate) const CONVERSATION_TRANSCRIPT_PAGE_SIZE: usize = 64;
pub(crate) const MAX_CONVERSATION_TRANSCRIPT_AUX_ENTRIES: usize = 128;
pub(crate) const MAX_ARCHIVE_RECALL_ENTRIES: usize = 512;
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

fn conversation_page_shape(turn_count: u64) -> Result<(u64, u64, usize)> {
    if turn_count == 0 {
        return Ok((0, 0, 0));
    }
    let page_size = u64::try_from(CONVERSATION_TRANSCRIPT_PAGE_SIZE).map_err(|_| {
        Error::config(
            "conversation_transcript_head",
            "page size does not fit the sequence domain",
        )
    })?;
    let page_count = turn_count.saturating_add(page_size - 1) / page_size;
    let active_page_id = page_count.saturating_sub(1);
    let active_page_entry_count = usize::try_from(
        (turn_count.saturating_sub(1) % page_size).saturating_add(1),
    )
    .map_err(|_| {
        Error::config(
            "conversation_transcript_head",
            "active page entry count does not fit the platform",
        )
    })?;
    Ok((page_count, active_page_id, active_page_entry_count))
}

#[allow(clippy::too_many_arguments)]
fn conversation_head_digest(
    revision: u64,
    memory_space_id: &str,
    mounted_subject_id: &str,
    channel_id: &str,
    conversation_id: &str,
    turn_count: u64,
    last_sequence: u64,
    page_count: u64,
    active_page_id: u64,
    active_page_entry_count: usize,
) -> String {
    let mut hasher = Sha256::new();
    for field in [
        "conversation_transcript_head_v2".to_string(),
        RECALL_INDEX_SCHEMA_VERSION.to_string(),
        revision.to_string(),
        memory_space_id.to_string(),
        mounted_subject_id.to_string(),
        channel_id.to_string(),
        conversation_id.to_string(),
        turn_count.to_string(),
        last_sequence.to_string(),
        page_count.to_string(),
        active_page_id.to_string(),
        active_page_entry_count.to_string(),
    ] {
        hash_field(&mut hasher, field.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConversationRecallManifest {
    pub schema_version: u32,
    pub physical_key: String,
    pub revision: u64,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub turn_count: u64,
    pub last_sequence: u64,
    pub page_count: u64,
    pub active_page_id: u64,
    pub active_page_entry_count: usize,
    pub head_digest: String,
}

impl ConversationRecallManifest {
    pub(crate) fn build(
        revision: u64,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: &str,
        conversation_id: &str,
        turn_count: u64,
        last_sequence: u64,
    ) -> Result<Self> {
        for (value, name) in [
            (memory_space_id, "memory_space_id"),
            (mounted_subject_id, "mounted_subject_id"),
            (channel_id, "channel_id"),
            (conversation_id, "conversation_id"),
        ] {
            require_component(value, name)?;
        }
        if revision == 0 {
            return Err(Error::config(
                "conversation_transcript_head",
                "revision must be greater than zero",
            ));
        }
        let (page_count, active_page_id, active_page_entry_count) =
            conversation_page_shape(turn_count)?;
        let physical_key = recall_index_physical_key(
            Self::KIND,
            &[
                memory_space_id,
                mounted_subject_id,
                channel_id,
                conversation_id,
            ],
        )?;
        let head_digest = conversation_head_digest(
            revision,
            memory_space_id,
            mounted_subject_id,
            channel_id,
            conversation_id,
            turn_count,
            last_sequence,
            page_count,
            active_page_id,
            active_page_entry_count,
        );
        let value = Self {
            schema_version: RECALL_INDEX_SCHEMA_VERSION,
            physical_key,
            revision,
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            channel_id: channel_id.to_string(),
            conversation_id: conversation_id.to_string(),
            turn_count,
            last_sequence,
            page_count,
            active_page_id,
            active_page_entry_count,
            head_digest,
        };
        value.validate()?;
        Ok(value)
    }
}

impl TypedRecallIndex for ConversationRecallManifest {
    const KIND: &'static str = "conversation_recall_manifest_v1";
    const NAMESPACE: &'static str = CONVERSATION_RECALL_MANIFEST_NAMESPACE;
    const MAX_ENTRIES: usize = 0;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn physical_key(&self) -> &str {
        &self.physical_key
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn entry_count(&self) -> usize {
        0
    }

    fn entries(&self) -> &[RecallIndexAddress] {
        &[]
    }

    fn entries_digest(&self) -> &str {
        &self.head_digest
    }

    fn expected_physical_key(&self) -> Result<String> {
        recall_index_physical_key(
            Self::KIND,
            &[
                &self.memory_space_id,
                &self.mounted_subject_id,
                &self.channel_id,
                &self.conversation_id,
            ],
        )
    }

    fn scope_digest_parts(&self) -> Vec<&str> {
        vec![
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.channel_id,
            &self.conversation_id,
        ]
    }

    fn validate(&self) -> Result<()> {
        let (page_count, active_page_id, active_page_entry_count) =
            conversation_page_shape(self.turn_count)?;
        let expected_digest = conversation_head_digest(
            self.revision,
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.channel_id,
            &self.conversation_id,
            self.turn_count,
            self.last_sequence,
            self.page_count,
            self.active_page_id,
            self.active_page_entry_count,
        );
        if self.schema_version != RECALL_INDEX_SCHEMA_VERSION
            || self.revision == 0
            || self.physical_key != self.expected_physical_key()?
            || self.last_sequence != self.turn_count
            || self.page_count != page_count
            || self.active_page_id != active_page_id
            || self.active_page_entry_count != active_page_entry_count
            || self.head_digest != expected_digest
        {
            return Err(Error::config(
                "conversation_transcript_head",
                "head schema, scope, sequence, page shape, or digest is invalid",
            ));
        }
        Ok(())
    }
}

typed_recall_index!(
    ConversationTranscriptPageIndex,
    "conversation_transcript_page_v1",
    CONVERSATION_TRANSCRIPT_PAGE_NAMESPACE,
    CONVERSATION_TRANSCRIPT_PAGE_SIZE,
    {
        memory_space_id,
        mounted_subject_id,
        channel_id,
        conversation_id,
        page_id
    }
);
typed_recall_index!(
    ConversationTranscriptAuxManifest,
    "conversation_transcript_aux_manifest_v1",
    CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE,
    MAX_CONVERSATION_TRANSCRIPT_AUX_ENTRIES,
    {
        memory_space_id,
        mounted_subject_id,
        channel_id,
        conversation_id,
        turn_id
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
        let address =
            RecallIndexAddress::json("owner", "key", 1, 10, &serde_json::json!({"body": true}))
                .unwrap();
        let index = ArchiveRecallManifest::build(1, "space", "subject", [address]).unwrap();
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
    fn all_typed_indexes_enforce_key_count_and_digest() {
        let address =
            RecallIndexAddress::json("owner", "key", 1, 9, &serde_json::json!({"value": 1}))
                .unwrap();
        let indexes: Vec<Box<dyn Fn() -> Result<()>>> = vec![
            Box::new({
                let address = address.clone();
                move || {
                    let _ = &address;
                    ConversationRecallManifest::build(1, "ms", "subject", "ch", "cv", 1, 1)?
                        .validate()
                }
            }),
            Box::new({
                let address = address.clone();
                move || {
                    ConversationTranscriptPageIndex::build(
                        1,
                        "ms",
                        "subject",
                        "ch",
                        "cv",
                        "00000000000000000000",
                        [address.clone()],
                    )?
                    .validate()
                }
            }),
            Box::new({
                let address = address.clone();
                move || {
                    ConversationTranscriptAuxManifest::build(
                        1,
                        "ms",
                        "subject",
                        "ch",
                        "cv",
                        "turn-1",
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
