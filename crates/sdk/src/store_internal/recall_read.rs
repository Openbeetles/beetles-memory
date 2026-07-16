use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use bm_core::agent::{ActiveWorkRecord, ActiveWorkStore};
use bm_core::memory::{
    ContinuityCapsule, ContinuityCapsuleDraft, ContinuityCapsuleScopeKind, ContinuityCapsuleStore,
    ContinuityCapsuleWriteOutcome, ConversationKey, ConversationTranscriptStore, DerivedMemoryRef,
    LongTermMemoryEntry, LongTermMemoryQuery, LongTermMemoryReadStore, MemoryStore,
    RedactedTranscriptSlice, SessionMessage, SessionMessageRecord, SessionStore,
    SessionSummaryStore, TranscriptAttrEnvelope, TranscriptAttrWriteReport, TranscriptCommitReport,
    TranscriptConversationAlias, TranscriptLifecycleReport, TranscriptLifecycleRequest,
    TranscriptReplayView, TranscriptTurnRecord, TurnLedger, TurnLedgerStore,
};
use bm_core::platform::SkillStorage;
use bm_core::task_execution::{TaskLearningRecord, TaskLearningStore, TaskRunRecord, TaskRunStore};
use bm_core::{Error, Result};
use serde::{de::DeserializeOwned, Deserialize};
use sha2::{Digest, Sha256};

use super::recall_index::{decode_typed_recall_index, TypedRecallIndex};
use super::transaction::StoreImmutableReadSession;
use crate::StoreReadReceipt;

type RecallJsonRead = ((String, String), Option<serde_json::Value>);

#[allow(
    dead_code,
    reason = "foundation type consumed by the production recall integration"
)]
pub(crate) struct RecallImmutableReadContext<'a> {
    session: Box<dyn StoreImmutableReadSession + 'a>,
    json_cache: BTreeMap<(String, String), Option<serde_json::Value>>,
    blob_cache: BTreeMap<(String, String), Option<Vec<u8>>>,
}

#[allow(
    dead_code,
    reason = "foundation API consumed by the production recall integration"
)]
impl<'a> RecallImmutableReadContext<'a> {
    pub(crate) fn new(session: Box<dyn StoreImmutableReadSession + 'a>) -> Self {
        Self {
            session,
            json_cache: BTreeMap::new(),
            blob_cache: BTreeMap::new(),
        }
    }

    pub(crate) fn read_json_value(
        &mut self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>> {
        let address = (namespace.to_string(), key.to_string());
        if let Some(value) = self.json_cache.get(&address) {
            return Ok(value.clone());
        }
        let reads = self
            .session
            .read_json_known_keys(std::slice::from_ref(&address))?;
        let [read] = reads.as_slice() else {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend did not return exactly one JSON address",
            ));
        };
        if read.namespace != namespace || read.key != key {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend returned a different JSON address",
            ));
        }
        self.json_cache.insert(address, read.value.clone());
        Ok(read.value.clone())
    }

    pub(crate) fn read_json_values(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<RecallJsonRead>> {
        let mut unique = BTreeSet::new();
        let addresses = addresses
            .iter()
            .filter(|address| unique.insert((*address).clone()))
            .cloned()
            .collect::<Vec<_>>();
        let missing = addresses
            .iter()
            .filter(|address| !self.json_cache.contains_key(*address))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            let reads = self.session.read_json_known_keys(&missing)?;
            if reads.len() != missing.len() {
                return Err(Error::config(
                    "recall_immutable_read_session",
                    "backend did not return every requested JSON address",
                ));
            }
            let expected = missing.iter().cloned().collect::<BTreeSet<_>>();
            let mut observed = BTreeSet::new();
            for read in reads {
                let address = (read.namespace, read.key);
                if !expected.contains(&address) || !observed.insert(address.clone()) {
                    return Err(Error::config(
                        "recall_immutable_read_session",
                        "backend returned a different or duplicate JSON address",
                    ));
                }
                self.json_cache.insert(address, read.value);
            }
        }
        Ok(addresses
            .into_iter()
            .map(|address| {
                let value = self.json_cache.get(&address).cloned().unwrap_or(None);
                (address, value)
            })
            .collect())
    }

    pub(crate) fn read_json_values_transient(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<RecallJsonRead>> {
        let mut unique = BTreeSet::new();
        let addresses = addresses
            .iter()
            .filter(|address| unique.insert((*address).clone()))
            .cloned()
            .collect::<Vec<_>>();
        let missing = addresses
            .iter()
            .filter(|address| !self.json_cache.contains_key(*address))
            .cloned()
            .collect::<Vec<_>>();
        let reads = if missing.is_empty() {
            BTreeMap::new()
        } else {
            let reads = self.session.read_json_known_keys(&missing)?;
            if reads.len() != missing.len() {
                return Err(Error::config(
                    "recall_immutable_read_session",
                    "backend did not return every transient JSON address",
                ));
            }
            let expected = missing.iter().cloned().collect::<BTreeSet<_>>();
            let mut observed = BTreeMap::new();
            for read in reads {
                let address = (read.namespace, read.key);
                if !expected.contains(&address) || observed.insert(address, read.value).is_some() {
                    return Err(Error::config(
                        "recall_immutable_read_session",
                        "backend returned a different or duplicate transient JSON address",
                    ));
                }
            }
            observed
        };
        Ok(addresses
            .into_iter()
            .map(|address| {
                let value = self
                    .json_cache
                    .get(&address)
                    .cloned()
                    .or_else(|| reads.get(&address).cloned())
                    .unwrap_or(None);
                (address, value)
            })
            .collect())
    }

    pub(crate) fn read_json<T: DeserializeOwned>(
        &mut self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<T>> {
        self.read_json_value(namespace, key)?
            .map(|value| {
                serde_json::from_value(value).map_err(|error| {
                    Error::config(
                        "recall_immutable_read_session",
                        format!("invalid typed JSON at {namespace}/{key}: {error}"),
                    )
                })
            })
            .transpose()
    }

    pub(crate) fn read_typed_index<T: TypedRecallIndex>(
        &mut self,
        physical_key: &str,
    ) -> Result<Option<T>> {
        self.read_json_value(T::NAMESPACE, physical_key)?
            .map(|value| decode_typed_recall_index::<T>(physical_key, value))
            .transpose()
    }

    pub(crate) fn materialize_typed_index<T: TypedRecallIndex>(
        &mut self,
        physical_key: &str,
    ) -> Result<Option<T>> {
        let Some(index) = self.read_typed_index::<T>(physical_key)? else {
            return Ok(None);
        };
        let json_addresses = index
            .entries()
            .iter()
            .filter(|entry| entry.kind == super::recall_index::RecallIndexAddressKind::Json)
            .map(|entry| (entry.namespace.clone(), entry.key.clone()))
            .collect::<Vec<_>>();
        self.read_json_values(&json_addresses)?;
        for entry in index.entries() {
            match entry.kind {
                super::recall_index::RecallIndexAddressKind::Json => {
                    let value = self
                        .json_cache
                        .get(&(entry.namespace.clone(), entry.key.clone()))
                        .and_then(Option::as_ref)
                        .ok_or_else(|| {
                            Error::config(
                                "typed_recall_index_entry",
                                format!(
                                    "{} declared JSON owner is missing at {}/{}",
                                    T::KIND,
                                    entry.namespace,
                                    entry.key
                                ),
                            )
                        })?;
                    let bytes = serde_json::to_vec(value).map_err(|error| {
                        Error::config("typed_recall_index_entry", error.to_string())
                    })?;
                    verify_entry_digest::<T>(entry, &bytes)?;
                }
                super::recall_index::RecallIndexAddressKind::Blob => {
                    let value = self
                        .read_blob(&entry.namespace, &entry.key)?
                        .ok_or_else(|| {
                            Error::config(
                                "typed_recall_index_entry",
                                format!(
                                    "{} declared blob owner is missing at {}/{}",
                                    T::KIND,
                                    entry.namespace,
                                    entry.key
                                ),
                            )
                        })?;
                    verify_entry_digest::<T>(entry, &value)?;
                }
            }
        }
        Ok(Some(index))
    }

    pub(crate) fn read_blob(&mut self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let address = (namespace.to_string(), key.to_string());
        if let Some(value) = self.blob_cache.get(&address) {
            return Ok(value.clone());
        }
        let reads = self
            .session
            .read_blob_known_keys(std::slice::from_ref(&address))?;
        let [read] = reads.as_slice() else {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend did not return exactly one blob address",
            ));
        };
        if read.namespace != namespace || read.key != key {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend returned a different blob address",
            ));
        }
        self.blob_cache.insert(address, read.value.clone());
        Ok(read.value.clone())
    }

    pub(crate) fn receipt(&self) -> Result<StoreReadReceipt> {
        self.session.receipt()
    }

    pub(crate) fn cached_json<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<T>> {
        self.json_cache
            .get(&(namespace.to_string(), key.to_string()))
            .and_then(Option::as_ref)
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|error| {
                    Error::config(
                        "recall_immutable_read_session",
                        format!("invalid cached typed JSON at {namespace}/{key}: {error}"),
                    )
                })
            })
            .transpose()
    }

    pub(crate) fn cached_json_docs<T: DeserializeOwned>(&self, namespace: &str) -> Result<Vec<T>> {
        self.json_cache
            .iter()
            .filter_map(|((observed_namespace, _), value)| {
                (observed_namespace == namespace)
                    .then_some(value.as_ref())
                    .flatten()
            })
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|error| {
                    Error::config("recall_immutable_read_session", error.to_string())
                })
            })
            .collect()
    }

    pub(crate) fn take_materialized_view(&mut self) -> RecallReadView {
        RecallReadView {
            json: std::mem::take(&mut self.json_cache),
            blobs: std::mem::take(&mut self.blob_cache),
        }
    }

    #[cfg(test)]
    pub(crate) fn cached_address_counts(&self) -> (usize, usize) {
        (self.json_cache.len(), self.blob_cache.len())
    }
}

fn verify_entry_digest<T: TypedRecallIndex>(
    entry: &super::recall_index::RecallIndexAddress,
    bytes: &[u8],
) -> Result<()> {
    let observed = format!("sha256:{:x}", Sha256::digest(bytes));
    if observed != entry.content_sha256 {
        return Err(Error::config(
            "typed_recall_index_entry",
            format!("{} declared owner content digest drift", T::KIND),
        ));
    }
    Ok(())
}

#[derive(Clone, Default)]
pub(crate) struct RecallReadView {
    json: BTreeMap<(String, String), Option<serde_json::Value>>,
    blobs: BTreeMap<(String, String), Option<Vec<u8>>>,
}

pub(crate) struct RecallReadJsonDoc {
    pub(crate) key: String,
    pub(crate) value: serde_json::Value,
}

impl RecallReadView {
    pub(crate) fn json_value(&self, namespace: &str, key: &str) -> Option<&serde_json::Value> {
        self.json
            .get(&(namespace.to_string(), key.to_string()))
            .and_then(Option::as_ref)
    }

    pub(crate) fn json<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<T>> {
        self.json_value(namespace, key)
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|error| {
                    Error::config(
                        "recall_read_view",
                        format!("invalid typed JSON at {namespace}/{key}: {error}"),
                    )
                })
            })
            .transpose()
    }

    pub(crate) fn json_docs<T: DeserializeOwned>(&self, namespace: &str) -> Result<Vec<T>> {
        self.json
            .iter()
            .filter_map(|((observed_namespace, _), value)| {
                (observed_namespace == namespace)
                    .then_some(value.as_ref())
                    .flatten()
            })
            .cloned()
            .map(|value| {
                serde_json::from_value(value)
                    .map_err(|error| Error::config("recall_read_view", error.to_string()))
            })
            .collect()
    }

    pub(crate) fn json_docs_by_keys(
        &self,
        namespace: &str,
        keys: &[String],
    ) -> Result<Vec<RecallReadJsonDoc>> {
        let mut seen = BTreeSet::new();
        Ok(keys
            .iter()
            .filter(|key| seen.insert((*key).clone()))
            .filter_map(|key| {
                self.json_value(namespace, key)
                    .cloned()
                    .map(|value| RecallReadJsonDoc {
                        key: key.clone(),
                        value,
                    })
            })
            .collect())
    }

    pub(crate) fn blob(&self, namespace: &str, key: &str) -> Option<&[u8]> {
        self.blobs
            .get(&(namespace.to_string(), key.to_string()))
            .and_then(Option::as_deref)
    }

    fn reject_write<T>(&self) -> Result<T> {
        Err(Error::config(
            "recall_read_view",
            "materialized recall view is immutable",
        ))
    }
}

#[derive(Deserialize)]
struct MaterializedSessionSummary {
    summary: String,
    message_count: usize,
}

impl SessionSummaryStore for RecallReadView {
    fn get(&self, chat_id: &str) -> Result<Option<String>> {
        Ok(self
            .json::<MaterializedSessionSummary>("session_summary", chat_id)?
            .map(|record| record.summary))
    }

    fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
        self.reject_write()
    }

    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        Ok(self
            .json::<MaterializedSessionSummary>("session_summary", chat_id)?
            .map(|record| (record.summary, record.message_count)))
    }
}

impl SessionStore for RecallReadView {
    fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
        self.reject_write()
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let mut messages = self
            .json::<Vec<SessionMessage>>("session", chat_id)?
            .unwrap_or_default();
        if messages.len() > n {
            messages = messages.split_off(messages.len() - n);
        }
        Ok(messages)
    }

    fn load_recent_records(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessageRecord>> {
        Ok(self
            .load_recent(chat_id, n)?
            .into_iter()
            .map(SessionMessageRecord::from)
            .collect())
    }

    fn message_count(&self, chat_id: &str) -> Result<usize> {
        Ok(self
            .json::<Vec<SessionMessage>>("session", chat_id)?
            .map_or(0, |messages| messages.len()))
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        self.reject_write()
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .json
            .iter()
            .filter_map(|((namespace, key), value)| {
                (namespace == "session" && value.is_some()).then_some(key.clone())
            })
            .collect())
    }
}

impl MemoryStore for RecallReadView {
    fn get_memory(&self) -> Result<String> {
        Ok(self
            .blob("memory", "MEMORY.md")
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default())
    }

    fn set_memory(&self, _content: &str) -> Result<()> {
        self.reject_write()
    }

    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
        let mut names = self
            .blobs
            .iter()
            .filter_map(|((namespace, key), value)| {
                (namespace == "daily" && value.is_some()).then_some(key.clone())
            })
            .collect::<Vec<_>>();
        names.sort_by(|left, right| right.cmp(left));
        names.truncate(recent_n);
        Ok(names)
    }

    fn get_daily_note(&self, name: &str) -> Result<String> {
        Ok(self
            .blob("daily", name)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default())
    }

    fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
        self.reject_write()
    }
}

impl ActiveWorkStore for RecallReadView {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
        self.json("active_work", chat_id)
    }

    fn set(&self, _chat_id: &str, _record: &ActiveWorkRecord) -> Result<()> {
        self.reject_write()
    }
}

impl TurnLedgerStore for RecallReadView {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        self.json("turn_ledger", chat_id)
    }

    fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
        self.reject_write()
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        self.reject_write()
    }
}

impl SkillStorage for RecallReadView {
    fn list_names(&self) -> Result<Vec<String>> {
        Ok(self
            .blobs
            .iter()
            .filter_map(|((namespace, key), value)| {
                (namespace == "skills" && value.is_some()).then_some(key.clone())
            })
            .collect())
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.blob("skills", name)
            .map(ToOwned::to_owned)
            .ok_or_else(|| Error::config("recall_read_view", "runtime skill is not materialized"))
    }

    fn write(&self, _name: &str, _content: &[u8]) -> Result<()> {
        self.reject_write()
    }

    fn remove(&self, _name: &str) -> Result<()> {
        self.reject_write()
    }
}

impl ContinuityCapsuleStore for RecallReadView {
    fn upsert_many(
        &self,
        _drafts: &[ContinuityCapsuleDraft],
        _now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome> {
        self.reject_write()
    }

    fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
        self.json("continuity_capsule", capsule_id)
    }

    fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>> {
        let mut values = self.json_docs::<ContinuityCapsule>("continuity_capsule")?;
        values.sort_by_key(|capsule| Reverse(capsule.updated_at));
        values.truncate(limit);
        Ok(values)
    }

    fn count(&self) -> Result<usize> {
        Ok(self
            .json_docs::<ContinuityCapsule>("continuity_capsule")?
            .len())
    }

    fn list_for_scope(
        &self,
        scope_kind: ContinuityCapsuleScopeKind,
        scope_id: &str,
        limit: usize,
    ) -> Result<Vec<ContinuityCapsule>> {
        let mut values = ContinuityCapsuleStore::list(self, usize::MAX)?;
        values.retain(|capsule| capsule.scope_kind == scope_kind && capsule.scope_id == scope_id);
        values.truncate(limit.max(1));
        Ok(values)
    }
}

impl TaskRunStore for RecallReadView {
    fn get(&self, run_id: &str) -> Result<Option<TaskRunRecord>> {
        self.json("task_run", run_id)
    }

    fn upsert(&self, _record: &TaskRunRecord) -> Result<()> {
        self.reject_write()
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskRunRecord>> {
        let mut values = self.json_docs::<TaskRunRecord>("task_run")?;
        values.sort_by_key(|record| Reverse(record.run.updated_at));
        values.truncate(limit);
        Ok(values)
    }

    fn list_active_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskRunRecord>> {
        let mut values = TaskRunStore::list_recent(self, usize::MAX)?;
        values.retain(|record| {
            record.run.source_channel == channel
                && record.run.source_chat_id == chat_id
                && record.run.status.is_active()
        });
        values.truncate(limit);
        Ok(values)
    }
}

impl TaskLearningStore for RecallReadView {
    fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>> {
        self.json("task_learning", learning_id)
    }

    fn upsert(&self, _record: &TaskLearningRecord) -> Result<()> {
        self.reject_write()
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut values = self.json_docs::<TaskLearningRecord>("task_learning")?;
        values.sort_by_key(|record| Reverse(record.observed_at));
        values.truncate(limit);
        Ok(values)
    }

    fn list_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskLearningRecord>> {
        let mut values = TaskLearningStore::list_recent(self, usize::MAX)?;
        values
            .retain(|record| record.source_channel == channel && record.source_chat_id == chat_id);
        values.truncate(limit);
        Ok(values)
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut values = TaskLearningStore::list_recent(self, usize::MAX)?;
        values.retain(|record| record.run_id == run_id);
        values.truncate(limit);
        Ok(values)
    }
}

impl LongTermMemoryReadStore for RecallReadView {
    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = LongTermMemoryReadStore::list(self, usize::MAX)?;
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            entries.retain(|entry| {
                entry.topic.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query)
                    || entry
                        .keywords
                        .iter()
                        .any(|keyword| keyword.to_lowercase().contains(&query))
            });
        }
        entries.sort_by(|left, right| {
            let left_scope = usize::from(left.source_chat_id.as_deref() == source_chat_id);
            let right_scope = usize::from(right.source_chat_id.as_deref() == source_chat_id);
            right_scope
                .cmp(&left_scope)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
        });
        entries.truncate(limit);
        Ok(entries)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        Ok(self
            .json_docs::<LongTermMemoryEntry>("long_term")?
            .into_iter()
            .find(|entry| entry.id == id))
    }

    fn query(&self, query: &LongTermMemoryQuery) -> Result<Vec<LongTermMemoryEntry>> {
        Ok(query.filter_sort_entries(
            LongTermMemoryReadStore::list(self, usize::MAX)?,
            bm_core::util::current_unix_secs(),
        ))
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = self.json_docs::<LongTermMemoryEntry>("long_term")?;
        entries.sort_by_key(|entry| Reverse(entry.updated_at));
        entries.truncate(limit);
        Ok(entries)
    }

    fn count(&self) -> Result<usize> {
        Ok(self.json_docs::<LongTermMemoryEntry>("long_term")?.len())
    }
}

impl ConversationTranscriptStore for RecallReadView {
    fn append_turn(&self, _record: &TranscriptTurnRecord) -> Result<TranscriptCommitReport> {
        self.reject_write()
    }

    fn remember_conversation_alias(&self, _alias: &TranscriptConversationAlias) -> Result<()> {
        self.reject_write()
    }

    fn resolve_conversation_alias(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<Option<String>> {
        let key = TranscriptConversationAlias::storage_key_for(
            memory_space_id,
            mounted_subject_id,
            channel_id,
            chat_id,
        );
        Ok(self
            .json::<TranscriptConversationAlias>("conversation_transcript_alias", &key)?
            .map(|alias| alias.conversation_id))
    }

    fn get_turn(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>> {
        Ok(self
            .list_turns(key, mounted_subject_id, usize::MAX)?
            .into_iter()
            .find(|record| record.turn_id == turn_id))
    }

    fn list_turns(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        let mut records = self.json_docs::<TranscriptTurnRecord>("conversation_transcript")?;
        records.retain(|record| record.key == *key);
        if records
            .iter()
            .any(|record| record.subject != mounted_subject_id)
        {
            return Err(Error::config(
                "recall_read_view",
                "conversation transcript owner differs from requested subject",
            ));
        }
        records.sort_by_key(|record| record.sequence);
        if records.len() > limit {
            records = records.split_off(records.len() - limit);
        }
        Ok(records)
    }

    fn upsert_transcript_attrs(
        &self,
        _key: &ConversationKey,
        _mounted_subject_id: &str,
        _attrs: &[TranscriptAttrEnvelope],
    ) -> Result<TranscriptAttrWriteReport> {
        self.reject_write()
    }

    fn list_transcript_attrs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<TranscriptAttrEnvelope>> {
        let mut attrs = self.json_docs::<TranscriptAttrEnvelope>("conversation_transcript_attr")?;
        attrs.retain(|attr| {
            attr.target.key == *key && turn_id.is_none_or(|turn_id| attr.target.turn_id == turn_id)
        });
        for attr in &attrs {
            let turn = self
                .get_turn(key, mounted_subject_id, &attr.target.turn_id)?
                .ok_or_else(|| {
                    Error::config(
                        "recall_read_view",
                        "transcript attr target owner is missing",
                    )
                })?;
            attr.validate_for_record(&turn)?;
        }
        attrs.sort_by(|left, right| left.attr_id.cmp(&right.attr_id));
        Ok(attrs)
    }

    fn append_derived_memory_ref(
        &self,
        _key: &ConversationKey,
        _derived: &DerivedMemoryRef,
    ) -> Result<()> {
        self.reject_write()
    }

    fn list_derived_memory_refs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>> {
        let mut refs = self.json_docs::<DerivedMemoryRef>("conversation_transcript_derived_ref")?;
        refs.retain(|derived| {
            derived.source.memory_space_id == key.memory_space_id
                && derived.source.channel_id == key.channel_id
                && derived.source.conversation_id == key.conversation_id
                && turn_id.is_none_or(|turn_id| derived.source.turn_id == turn_id)
        });
        for derived in &refs {
            let owner = derived
                .subject_id
                .as_deref()
                .or(derived.source.subject_id.as_deref());
            if owner != Some(mounted_subject_id)
                || derived
                    .subject_id
                    .as_deref()
                    .zip(derived.source.subject_id.as_deref())
                    .is_some_and(|(owner, source_owner)| owner != source_owner)
            {
                return Err(Error::config(
                    "recall_read_view",
                    "transcript derived owner differs from requested subject",
                ));
            }
        }
        Ok(refs)
    }

    fn apply_lifecycle_request(
        &self,
        _mounted_subject_id: &str,
        _request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport> {
        self.reject_write()
    }

    fn redacted_replay(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        limit: usize,
        view: TranscriptReplayView,
    ) -> Result<RedactedTranscriptSlice> {
        let records = self.list_turns(key, mounted_subject_id, limit)?;
        let attrs = self.list_transcript_attrs(key, mounted_subject_id, None)?;
        Ok(RedactedTranscriptSlice::from_records_with_attrs(
            key.clone(),
            view,
            &records,
            &attrs,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::store_internal::transaction::{
        StoreBoundedKnownBlobRead, StoreBoundedKnownJsonRead, StoreImmutableReadSession,
    };

    struct CountingSession {
        json_calls: Arc<Mutex<usize>>,
        blob_calls: Arc<Mutex<usize>>,
    }

    impl StoreImmutableReadSession for CountingSession {
        fn read_json_known_keys(
            &mut self,
            addresses: &[(String, String)],
        ) -> Result<Vec<StoreBoundedKnownJsonRead>> {
            *self.json_calls.lock().unwrap() += 1;
            Ok(addresses
                .iter()
                .map(|(namespace, key)| StoreBoundedKnownJsonRead {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: (key == "present").then(|| serde_json::json!({"ok": true})),
                })
                .collect())
        }

        fn read_blob_known_keys(
            &mut self,
            addresses: &[(String, String)],
        ) -> Result<Vec<StoreBoundedKnownBlobRead>> {
            *self.blob_calls.lock().unwrap() += 1;
            Ok(addresses
                .iter()
                .map(|(namespace, key)| StoreBoundedKnownBlobRead {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: (key == "present").then(|| b"ok".to_vec()),
                })
                .collect())
        }

        fn receipt(&self) -> Result<StoreReadReceipt> {
            Ok(StoreReadReceipt {
                state_digest: "0".repeat(64),
                json_doc_count: 1,
                blob_count: 1,
                event_count: 0,
                entry_count: 4,
                json_bytes: 8,
                blob_bytes: 2,
            })
        }
    }

    #[test]
    fn cached_context_reads_each_present_and_absent_address_once() {
        let json_calls = Arc::new(Mutex::new(0));
        let blob_calls = Arc::new(Mutex::new(0));
        let session = CountingSession {
            json_calls: json_calls.clone(),
            blob_calls: blob_calls.clone(),
        };
        let mut context = RecallImmutableReadContext::new(Box::new(session));

        assert!(context.read_json_value("ns", "present").unwrap().is_some());
        assert!(context.read_json_value("ns", "present").unwrap().is_some());
        assert!(context.read_json_value("ns", "absent").unwrap().is_none());
        assert!(context.read_json_value("ns", "absent").unwrap().is_none());
        assert!(context.read_blob("blob", "present").unwrap().is_some());
        assert!(context.read_blob("blob", "present").unwrap().is_some());
        assert!(context.read_blob("blob", "absent").unwrap().is_none());
        assert!(context.read_blob("blob", "absent").unwrap().is_none());

        assert_eq!(*json_calls.lock().unwrap(), 2);
        assert_eq!(*blob_calls.lock().unwrap(), 2);
        assert_eq!(context.cached_address_counts(), (2, 2));
        assert_eq!(context.receipt().unwrap().entry_count, 4);
    }

    #[test]
    fn materialized_view_takes_the_cached_read_set_once_without_copying_it() {
        let mut context = RecallImmutableReadContext::new(Box::new(CountingSession {
            json_calls: Arc::new(Mutex::new(0)),
            blob_calls: Arc::new(Mutex::new(0)),
        }));
        context.read_json_value("ns", "present").unwrap();
        context.read_blob("blob", "present").unwrap();

        let view = context.take_materialized_view();

        assert_eq!(context.cached_address_counts(), (0, 0));
        assert!(view.json_value("ns", "present").is_some());
        assert_eq!(view.blob("blob", "present"), Some(b"ok".as_slice()));
        assert_eq!(context.receipt().unwrap().entry_count, 4);
    }
}
