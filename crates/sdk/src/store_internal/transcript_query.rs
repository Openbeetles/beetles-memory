use bm_core::memory::{
    transcript_message_is_query_index_eligible, ConversationCatalogHead, ConversationKey,
    TranscriptLifecycleState, TranscriptLocator, TranscriptRedactionState, TranscriptTurnRecord,
};
use bm_core::{Error, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const TRANSCRIPT_CATALOG_PAGE_NAMESPACE: &str = "conversation_transcript_catalog_pages";
pub(crate) const TRANSCRIPT_CATALOG_ROOT_NAMESPACE: &str = "conversation_transcript_catalog_roots";
pub(crate) const TRANSCRIPT_TIME_POSTING_NAMESPACE: &str = "conversation_transcript_time_postings";
pub(crate) const TRANSCRIPT_TIME_ROOT_NAMESPACE: &str = "conversation_transcript_time_roots";
pub(crate) const TRANSCRIPT_SEARCH_POSTING_NAMESPACE: &str =
    "conversation_transcript_search_postings";
pub(crate) const TRANSCRIPT_SEARCH_ROOT_NAMESPACE: &str = "conversation_transcript_search_roots";
pub(crate) const TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE: &str =
    "conversation_transcript_search_message_manifests";
pub(crate) const TRANSCRIPT_QUERY_KEYRING_NAMESPACE: &str =
    "conversation_transcript_query_keyring_private";

pub(crate) const TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION: u32 = 1;
pub(crate) const TRANSCRIPT_QUERY_PAGE_CAPACITY: usize = 128;

pub(crate) fn transcript_query_namespace_is_derived(namespace: &str) -> bool {
    matches!(
        namespace,
        TRANSCRIPT_CATALOG_PAGE_NAMESPACE
            | TRANSCRIPT_CATALOG_ROOT_NAMESPACE
            | TRANSCRIPT_TIME_POSTING_NAMESPACE
            | TRANSCRIPT_TIME_ROOT_NAMESPACE
            | TRANSCRIPT_SEARCH_POSTING_NAMESPACE
            | TRANSCRIPT_SEARCH_ROOT_NAMESPACE
            | TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE
    )
}

fn hash_parts(domain: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in std::iter::once(domain).chain(parts.iter().copied()) {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

pub(crate) fn catalog_page_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
    channel_id: Option<&str>,
    page_id: u64,
) -> String {
    hash_parts(
        "conversation_transcript_catalog_page_v1",
        &[
            memory_space_id,
            mounted_subject_id,
            channel_id.unwrap_or("*"),
            &page_id.to_string(),
        ],
    )
}

pub(crate) fn catalog_root_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
    channel_id: Option<&str>,
) -> String {
    hash_parts(
        "conversation_transcript_catalog_root_v1",
        &[
            memory_space_id,
            mounted_subject_id,
            channel_id.unwrap_or("*"),
        ],
    )
}

pub(crate) fn time_root_key(key: &ConversationKey, subject: &str, utc_day: u64) -> String {
    hash_parts(
        "conversation_transcript_time_root_v1",
        &[
            &key.memory_space_id,
            subject,
            &key.channel_id,
            &key.conversation_id,
            &utc_day.to_string(),
        ],
    )
}

pub(crate) fn time_posting_key(
    key: &ConversationKey,
    subject: &str,
    utc_day: u64,
    page_id: u64,
) -> String {
    hash_parts(
        "conversation_transcript_time_posting_v1",
        &[
            &key.memory_space_id,
            subject,
            &key.channel_id,
            &key.conversation_id,
            &utc_day.to_string(),
            &page_id.to_string(),
        ],
    )
}

pub(crate) fn term_digest(term: &str) -> String {
    hash_parts("conversation_transcript_search_term_v1", &[term])
}

pub(crate) fn term_set_digest(term_digests: &[String]) -> String {
    let parts = term_digests.iter().map(String::as_str).collect::<Vec<_>>();
    hash_parts("conversation_transcript_search_term_set_v1", &parts)
}

pub(crate) fn search_root_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
    term_digest: &str,
) -> String {
    hash_parts(
        "conversation_transcript_search_root_v1",
        &[memory_space_id, mounted_subject_id, term_digest],
    )
}

pub(crate) fn search_posting_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
    term_digest: &str,
    page_id: u64,
) -> String {
    hash_parts(
        "conversation_transcript_search_posting_v1",
        &[
            memory_space_id,
            mounted_subject_id,
            term_digest,
            &page_id.to_string(),
        ],
    )
}

pub(crate) fn search_message_manifest_key(locator: &TranscriptLocator) -> String {
    hash_parts(
        "conversation_transcript_search_message_manifest_v1",
        &[
            &locator.key.memory_space_id,
            &locator.mounted_subject_id,
            &locator.key.channel_id,
            &locator.key.conversation_id,
            &locator.turn_id,
            locator.message_id.as_deref().unwrap_or(""),
        ],
    )
}

pub(crate) fn keyring_key(memory_space_id: &str) -> String {
    hash_parts(
        "conversation_transcript_query_keyring_v1",
        &[memory_space_id],
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptCatalogPageV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: Option<String>,
    pub page_id: u64,
    pub revision: u64,
    pub heads: Vec<ConversationCatalogHead>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptCatalogRootV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: Option<String>,
    pub revision: u64,
    pub page_count: u64,
    pub entry_count: u64,
}

impl TranscriptCatalogPageV1 {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION
            || self.memory_space_id.is_empty()
            || self.mounted_subject_id.is_empty()
            || self.revision == 0
            || self.heads.len() > TRANSCRIPT_QUERY_PAGE_CAPACITY
        {
            return Err(Error::config(
                "conversation_transcript_catalog_index",
                "catalog page schema, scope, revision, or capacity is invalid",
            ));
        }
        for head in &self.heads {
            head.validate()?;
            if head.key.memory_space_id != self.memory_space_id
                || head.mounted_subject_id != self.mounted_subject_id
                || self
                    .channel_id
                    .as_ref()
                    .is_some_and(|channel| channel != &head.key.channel_id)
            {
                return Err(Error::config(
                    "conversation_transcript_catalog_index",
                    "catalog head is outside its page scope",
                ));
            }
        }
        if self.heads.windows(2).any(|pair| {
            (pair[0].updated_at, &pair[0].key.conversation_id)
                < (pair[1].updated_at, &pair[1].key.conversation_id)
        }) {
            return Err(Error::config(
                "conversation_transcript_catalog_index",
                "catalog page must be in deterministic descending activity order",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptPostingLocatorV1 {
    pub locator: TranscriptLocator,
    pub lifecycle_state: TranscriptLifecycleState,
    pub redaction_state: TranscriptRedactionState,
}

impl TranscriptPostingLocatorV1 {
    pub(crate) fn visible(&self, include_archived: bool) -> bool {
        self.redaction_state == TranscriptRedactionState::RawAvailable
            && (self.lifecycle_state == TranscriptLifecycleState::Active
                || (include_archived && self.lifecycle_state == TranscriptLifecycleState::Archived))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptTimePostingPageV1 {
    pub schema_version: u32,
    pub key: ConversationKey,
    pub mounted_subject_id: String,
    pub utc_day: u64,
    pub page_id: u64,
    pub revision: u64,
    pub locators: Vec<TranscriptPostingLocatorV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptTimePostingRootV1 {
    pub schema_version: u32,
    pub key: ConversationKey,
    pub mounted_subject_id: String,
    pub utc_day: u64,
    pub revision: u64,
    pub page_count: u64,
    pub entry_count: u64,
}

impl TranscriptTimePostingRootV1 {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION
            || self.key.memory_space_id.is_empty()
            || self.key.channel_id.is_empty()
            || self.key.conversation_id.is_empty()
            || self.mounted_subject_id.is_empty()
            || self.revision == 0
            || self.page_count == 0
            || self.entry_count == 0
        {
            return Err(Error::config(
                "conversation_transcript_time_index",
                "time root schema, owner, revision, or counts are invalid",
            ));
        }
        Ok(())
    }
}

impl TranscriptTimePostingPageV1 {
    pub(crate) fn validate_for_root(&self, root: &TranscriptTimePostingRootV1) -> Result<()> {
        if self.schema_version != TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION
            || self.key != root.key
            || self.mounted_subject_id != root.mounted_subject_id
            || self.utc_day != root.utc_day
            || self.revision == 0
            || self.page_id >= root.page_count
            || self.locators.is_empty()
            || self.locators.len() > TRANSCRIPT_QUERY_PAGE_CAPACITY
            || self.locators.iter().any(|entry| {
                entry.locator.key != self.key
                    || entry.locator.mounted_subject_id != self.mounted_subject_id
                    || entry.locator.observed_at / 86_400 != self.utc_day
            })
        {
            return Err(Error::config(
                "conversation_transcript_time_index",
                "time posting page is outside its exact root closure",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptSearchPostingPageV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub term_digest: String,
    pub page_id: u64,
    pub revision: u64,
    pub locators: Vec<TranscriptPostingLocatorV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptSearchPostingRootV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub term_digest: String,
    pub revision: u64,
    pub page_count: u64,
    pub entry_count: u64,
}

impl TranscriptSearchPostingRootV1 {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION
            || self.memory_space_id.is_empty()
            || self.mounted_subject_id.is_empty()
            || !self.term_digest.starts_with("sha256:")
            || self.term_digest.len() != 71
            || self.revision == 0
            || self.page_count == 0
            || self.entry_count == 0
        {
            return Err(Error::config(
                "conversation_transcript_search_index",
                "search root schema, scope, digest, revision, or counts are invalid",
            ));
        }
        Ok(())
    }
}

impl TranscriptSearchPostingPageV1 {
    pub(crate) fn validate_for_root(&self, root: &TranscriptSearchPostingRootV1) -> Result<()> {
        if self.schema_version != TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION
            || self.memory_space_id != root.memory_space_id
            || self.mounted_subject_id != root.mounted_subject_id
            || self.term_digest != root.term_digest
            || self.revision == 0
            || self.page_id >= root.page_count
            || self.locators.is_empty()
            || self.locators.len() > TRANSCRIPT_QUERY_PAGE_CAPACITY
            || self.locators.iter().any(|entry| {
                entry.locator.key.memory_space_id != self.memory_space_id
                    || entry.locator.mounted_subject_id != self.mounted_subject_id
            })
        {
            return Err(Error::config(
                "conversation_transcript_search_index",
                "search posting page is outside its exact root closure",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptMessageSearchManifestV1 {
    pub locator: TranscriptLocator,
    #[serde(rename = "d")]
    pub term_set_digest: String,
}

impl TranscriptMessageSearchManifestV1 {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.locator.turn_id.is_empty()
            || self.locator.message_id.as_deref().is_none_or(str::is_empty)
            || !self.term_set_digest.starts_with("sha256:")
            || self.term_set_digest.len() != 71
        {
            return Err(Error::config(
                "conversation_transcript_search_manifest",
                "message search manifest is not canonical",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptQueryKeyringV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub incarnation: String,
    pub current: TranscriptQuerySigningKeyV1,
    pub previous: Option<TranscriptQuerySigningKeyV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptQuerySigningKeyV1 {
    pub key_id: String,
    pub key_hex: String,
    pub created_at: u64,
    pub expires_at: u64,
}

impl TranscriptQueryKeyringV1 {
    pub(crate) fn validate_for_memory_space(&self, memory_space_id: &str) -> Result<()> {
        let valid_sha256 = |value: &str| {
            value.len() == 71
                && value.starts_with("sha256:")
                && value[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        let valid_key = |key: &TranscriptQuerySigningKeyV1| {
            valid_sha256(&key.key_id)
                && key.key_hex.len() == 64
                && key
                    .key_hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && key.created_at > 0
                && key.expires_at > key.created_at
        };
        if self.schema_version != TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION
            || self.memory_space_id != memory_space_id
            || !valid_sha256(&self.incarnation)
            || !valid_key(&self.current)
            || self.previous.as_ref().is_some_and(|previous| {
                !valid_key(previous) || previous.key_id == self.current.key_id
            })
        {
            return Err(Error::config(
                "conversation_transcript_query_cursor",
                "query keyring is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptQueryCursorClaimsV1 {
    pub schema_version: u32,
    pub key_id: String,
    pub kind: String,
    pub direction: String,
    pub query_digest: String,
    pub lifecycle: String,
    pub view_context: String,
    pub limit: usize,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub head_revision: u64,
    pub head_digest: String,
    pub snapshot_upper_bound: u64,
    pub content_generation: u64,
    pub index_generation: u64,
    pub position: u64,
    pub incarnation: String,
    pub issued_at: u64,
    pub expires_at: u64,
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_decode(encoded: &str) -> Result<Vec<u8>> {
    fn nibble(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            _ => None,
        }
    }
    let bytes = encoded.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(Error::config(
            "conversation_transcript_query_cursor",
            "cursor hex payload is invalid",
        ));
    }
    bytes
        .chunks_exact(2)
        .map(|pair| {
            let high = nibble(pair[0]).ok_or_else(|| {
                Error::config(
                    "conversation_transcript_query_cursor",
                    "cursor hex payload is invalid",
                )
            })?;
            let low = nibble(pair[1]).ok_or_else(|| {
                Error::config(
                    "conversation_transcript_query_cursor",
                    "cursor hex payload is invalid",
                )
            })?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn cursor_mac(key_hex: &str, payload: &[u8]) -> Result<Hmac<Sha256>> {
    let key = hex_decode(key_hex)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).map_err(|_| {
        Error::config(
            "conversation_transcript_query_cursor",
            "cursor signing key is invalid",
        )
    })?;
    mac.update(b"beetle_memory_transcript_query_cursor_v1");
    mac.update(&(payload.len() as u64).to_be_bytes());
    mac.update(payload);
    Ok(mac)
}

pub(crate) fn encode_cursor(
    keyring: &TranscriptQueryKeyringV1,
    claims: &TranscriptQueryCursorClaimsV1,
) -> Result<bm_core::memory::TranscriptQueryCursor> {
    if claims.schema_version != TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION
        || claims.key_id != keyring.current.key_id
        || claims.incarnation != keyring.incarnation
        || claims.issued_at == 0
        || claims.expires_at <= claims.issued_at
        || claims.expires_at > keyring.current.expires_at
    {
        return Err(Error::config(
            "conversation_transcript_query_cursor",
            "cursor claims do not match the active signing authority",
        ));
    }
    let payload = serde_json::to_vec(claims).map_err(|error| {
        Error::config("conversation_transcript_query_cursor", error.to_string())
    })?;
    bm_core::memory::TranscriptQueryCursor::try_from_encoded(format!(
        "btq1:{}.{}",
        hex_encode(&payload),
        hex_encode(
            &cursor_mac(&keyring.current.key_hex, &payload)?
                .finalize()
                .into_bytes()
        ),
    ))
}

pub(crate) fn decode_cursor(
    keyring: &TranscriptQueryKeyringV1,
    cursor: &bm_core::memory::TranscriptQueryCursor,
) -> Result<TranscriptQueryCursorClaimsV1> {
    let encoded = cursor.as_str().strip_prefix("btq1:").ok_or_else(|| {
        Error::config(
            "conversation_transcript_query_cursor",
            "cursor prefix is invalid",
        )
    })?;
    let (payload_hex, supplied_tag) = encoded.rsplit_once('.').ok_or_else(|| {
        Error::config(
            "conversation_transcript_query_cursor",
            "cursor authentication tag is missing",
        )
    })?;
    let payload = hex_decode(payload_hex)?;
    let claims =
        serde_json::from_slice::<TranscriptQueryCursorClaimsV1>(&payload).map_err(|error| {
            Error::config("conversation_transcript_query_cursor", error.to_string())
        })?;
    let key = if claims.key_id == keyring.current.key_id {
        &keyring.current
    } else {
        keyring
            .previous
            .as_ref()
            .filter(|key| key.key_id == claims.key_id)
            .ok_or_else(|| {
                Error::config(
                    "conversation_transcript_query_cursor",
                    "cursor signing key is stale",
                )
            })?
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    if claims.schema_version != TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION
        || claims.incarnation != keyring.incarnation
        || claims.issued_at == 0
        || claims.issued_at > now.saturating_add(300)
        || claims.expires_at < now
        || claims.expires_at > key.expires_at
    {
        return Err(Error::config(
            "conversation_transcript_query_cursor_stale",
            "cursor authority, incarnation, or expiry is stale",
        ));
    }
    let supplied_tag = hex_decode(supplied_tag)?;
    cursor_mac(&key.key_hex, &payload)?
        .verify_slice(&supplied_tag)
        .map_err(|_| {
            Error::config(
                "conversation_transcript_query_cursor",
                "cursor authentication failed",
            )
        })?;
    Ok(claims)
}

pub(crate) fn message_locators(record: &TranscriptTurnRecord) -> Vec<(TranscriptLocator, &str)> {
    record
        .input_messages
        .iter()
        .chain(record.assistant_message.iter())
        .filter(|message| transcript_message_is_query_index_eligible(message))
        .map(|message| {
            (
                TranscriptLocator {
                    key: record.key.clone(),
                    mounted_subject_id: record.subject.clone(),
                    turn_id: record.turn_id.clone(),
                    message_id: Some(message.message_id.clone()),
                    turn_sequence: record.sequence,
                    observed_at: message.observed_at,
                },
                message.content.as_str(),
            )
        })
        .collect()
}
