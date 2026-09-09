//! Store-incarnation authority for delivered procedural selection evidence.
//! Receipt TTL is an intake policy; durable evidence authentication has no wall clock.
use std::collections::BTreeMap;

use bm_core::memory::{ProceduralSelectionReceiptV1, TranscriptTurnRecord};
use bm_core::{Error, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) const NAMESPACE: &str = "procedural_selection_authorities_private";
const DOMAIN: &[u8] = b"beetle.procedural-selection.authority.v1\0";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProceduralSelectionAuthority {
    schema_version: u32,
    memory_space_id: String,
    incarnation: String,
    signing_key_id: String,
    created_at: u64,
    secret_hex: String,
}

/// Prepared at runtime construction, never acquired by projection's read path.
/// This capability authenticates delivery only; Store admission independently verifies it.
#[derive(Clone)]
pub(crate) struct ProceduralSelectionSigner(ProceduralSelectionAuthority);

impl std::fmt::Debug for ProceduralSelectionSigner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProceduralSelectionSigner([redacted])")
    }
}

impl ProceduralSelectionSigner {
    pub(crate) fn from_authority(authority: ProceduralSelectionAuthority) -> Self {
        Self(authority)
    }

    pub(crate) fn sign(&self, receipt: &mut ProceduralSelectionReceiptV1) -> Result<()> {
        self.0.sign(receipt)
    }
}

fn invalid() -> Error {
    Error::config(
        "procedural_selection_authority",
        "procedural selection authority or authentication is invalid",
    )
}

fn digest(domain: &[u8], bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(bytes);
    format!("sha256:{:x}", hash.finalize())
}

pub(crate) fn authority_key(space: &str) -> String {
    digest(
        b"procedural-selection-authority-address-v1\0",
        space.as_bytes(),
    )
}

impl ProceduralSelectionAuthority {
    pub(crate) fn fresh(space: &str, now: u64) -> Result<Self> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|_| invalid())?;
        let secret_hex = secret.iter().map(|byte| format!("{byte:02x}")).collect();
        let incarnation = digest(b"procedural-selection-incarnation-v1\0", &secret);
        let signing_key_id = digest(b"procedural-selection-key-id-v1\0", &secret);
        let value = Self {
            schema_version: 1,
            memory_space_id: space.to_owned(),
            incarnation,
            signing_key_id,
            created_at: now,
            secret_hex,
        };
        value.validate(&authority_key(space))?;
        Ok(value)
    }

    fn secret(&self) -> Result<[u8; 32]> {
        if self.secret_hex.len() != 64
            || !self
                .secret_hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        let mut result = [0; 32];
        for (index, byte) in result.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&self.secret_hex[index * 2..index * 2 + 2], 16)
                .map_err(|_| invalid())?;
        }
        Ok(result)
    }

    pub(crate) fn validate(&self, key: &str) -> Result<()> {
        let secret = self.secret()?;
        if self.schema_version != 1
            || self.created_at == 0
            || self.memory_space_id.is_empty()
            || self.memory_space_id.trim() != self.memory_space_id
            || key != authority_key(&self.memory_space_id)
            || self.incarnation != digest(b"procedural-selection-incarnation-v1\0", &secret)
            || self.signing_key_id != digest(b"procedural-selection-key-id-v1\0", &secret)
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn mac(&self, receipt: &ProceduralSelectionReceiptV1) -> Result<Hmac<Sha256>> {
        self.validate(&authority_key(&receipt.identity.memory_space_id))?;
        if receipt.identity.memory_space_id != self.memory_space_id
            || receipt.signing_key_id != self.signing_key_id
        {
            return Err(invalid());
        }
        let mut claims = serde_json::to_value(receipt).map_err(|_| invalid())?;
        claims
            .as_object_mut()
            .ok_or_else(invalid)?
            .remove("authority_tag");
        let encoded = serde_json::to_vec(&claims).map_err(|_| invalid())?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret()?).map_err(|_| invalid())?;
        mac.update(DOMAIN);
        mac.update(self.incarnation.as_bytes());
        mac.update(&encoded);
        Ok(mac)
    }

    pub(crate) fn sign(&self, receipt: &mut ProceduralSelectionReceiptV1) -> Result<()> {
        receipt.signing_key_id = self.signing_key_id.clone();
        receipt.selection_digest = receipt.canonical_selection_digest()?;
        receipt.receipt_ref = receipt.canonical_receipt_ref()?;
        receipt.authority_tag = format!("sha256:{:x}", self.mac(receipt)?.finalize().into_bytes());
        self.verify(receipt)
    }

    pub(crate) fn verify(&self, receipt: &ProceduralSelectionReceiptV1) -> Result<()> {
        if !receipt.validate_contract() {
            return Err(invalid());
        }
        let hex = receipt
            .authority_tag
            .strip_prefix("sha256:")
            .ok_or_else(invalid)?;
        if hex.len() != 64 || !hex.is_ascii() {
            return Err(invalid());
        }
        let mut tag = [0; 32];
        for (index, byte) in tag.iter_mut().enumerate() {
            *byte =
                u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).map_err(|_| invalid())?;
        }
        self.mac(receipt)?.verify_slice(&tag).map_err(|_| invalid())
    }
}

pub(crate) fn verify_receipt_in_json(
    receipt: &ProceduralSelectionReceiptV1,
    json: &BTreeMap<(String, String), Value>,
) -> Result<()> {
    let key = authority_key(&receipt.identity.memory_space_id);
    let value = json
        .get(&(NAMESPACE.to_owned(), key.clone()))
        .ok_or_else(invalid)?;
    let authority: ProceduralSelectionAuthority =
        serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    authority.validate(&key)?;
    authority.verify(receipt)
}

pub(crate) fn validate_store_image(json: &BTreeMap<(String, String), Value>) -> Result<()> {
    for ((namespace, key), value) in json {
        if namespace == NAMESPACE {
            let authority: ProceduralSelectionAuthority =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            authority.validate(key)?;
        } else if namespace == "conversation_transcript" {
            let record: TranscriptTurnRecord =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            record.validate_canonical_intake()?;
            if let Some(evidence) = &record.learning_evidence {
                if !evidence.validate_contract()
                    || evidence.tool_call_count as usize != record.tool_observations.len()
                    || evidence.memory_space_id != record.key.memory_space_id
                    || evidence.mounted_subject_id != record.subject
                    || evidence.conversation_id != record.key.conversation_id
                    || evidence.turn_id != record.turn_id
                {
                    return Err(invalid());
                }
                let mut ids = std::collections::BTreeSet::new();
                for observation in evidence
                    .agent_tool_feedback
                    .iter()
                    .flat_map(|feedback| &feedback.observations)
                {
                    if !ids.insert(&observation.observation_id)
                        || !record.tool_observations.iter().any(|canonical| {
                            canonical.observation_id == observation.observation_id
                                && canonical.tool_name == observation.tool_id
                                && canonical.summary == observation.summary
                                && canonical.external_content == observation.external_content
                        })
                    {
                        return Err(invalid());
                    }
                }
            }
            if let Some(receipt) = record
                .learning_evidence
                .as_ref()
                .and_then(|evidence| evidence.selection_receipt.as_ref())
            {
                if receipt.identity.memory_space_id != record.key.memory_space_id
                    || receipt.identity.mounted_subject_id != record.subject
                    || receipt.identity.channel != record.key.channel_id
                    || receipt.identity.conversation_id != record.key.conversation_id
                    || receipt.identity.turn_id != record.turn_id
                {
                    return Err(invalid());
                }
                verify_receipt_in_json(receipt, json)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn intake_dependencies(record: &TranscriptTurnRecord) -> Result<Vec<(String, String)>> {
    let Some(evidence) = &record.learning_evidence else {
        return Ok(Vec::new());
    };
    let mut addresses = Vec::new();
    for feedback in &evidence.runtime_skill_feedback {
        addresses.push((
            super::schema::RUNTIME_SKILL_RECORD_NAMESPACE.to_owned(),
            bm_core::skills::canonical_runtime_skill_owner_key(
                &record.key.memory_space_id,
                feedback.locator.owning_scope(),
                feedback.locator.owner_id(),
            )?,
        ));
    }
    for feedback in &evidence.task_learning_feedback {
        addresses.push(("task_learning".to_owned(), feedback.learning_id.clone()));
    }
    if let Some(receipt) = &evidence.selection_receipt {
        for selection in &receipt.agent_tool_experiences {
            if !evidence.agent_tool_feedback.iter().any(|feedback| {
                feedback.registry_ref == selection.registry_ref
                    && feedback.tool_id == selection.tool_id
                    && feedback.schema_fingerprint == selection.schema_fingerprint
            }) {
                continue;
            }
            let owner = bm_core::memory::GovernedMemoryOwnerRef::new(
                bm_core::memory::GovernedMemoryOwnerPlane::AgentToolExperience,
                &selection.experience_owner_id,
            );
            let scope = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
                mounted_subject_id: record.subject.clone(),
            };
            addresses.push((
                super::schema::AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.to_owned(),
                bm_core::skills::agent_tool_experience_head_key(
                    &record.key.memory_space_id,
                    &scope,
                    &owner,
                )?,
            ));
        }
    }
    Ok(addresses)
}

/// Freshness is a first-intake fence, not a rule imposed on durable historical receipts.
pub(crate) fn validate_new_intakes(
    before: &BTreeMap<(String, String), Value>,
    after: &BTreeMap<(String, String), Value>,
) -> Result<()> {
    for (address, value) in after
        .iter()
        .filter(|(address, _)| address.0 == "conversation_transcript")
    {
        let record: TranscriptTurnRecord =
            serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        let Some(evidence) = &record.learning_evidence else {
            continue;
        };
        if before.get(address).is_some_and(|previous| {
            previous.get("learning_evidence") == value.get("learning_evidence")
        }) {
            continue;
        }
        if !evidence.validate_contract() {
            return Err(invalid());
        }
        if let Some(receipt) = &evidence.selection_receipt {
            if receipt.issued_at > record.created_at
                || record.created_at >= receipt.expires_at
                || receipt.expires_at.saturating_sub(receipt.issued_at) != 86_400
            {
                return Err(invalid());
            }
        }
        let mounted_scope = bm_core::skills::RuntimeSkillOwningScope::Subject {
            mounted_subject_id: record.subject.clone(),
        };
        for feedback in &evidence.runtime_skill_feedback {
            let scope = feedback.locator.owning_scope();
            if scope != &mounted_scope
                && scope != &bm_core::skills::RuntimeSkillOwningScope::SharedProgram
            {
                return Err(invalid());
            }
            let key = bm_core::skills::canonical_runtime_skill_owner_key(
                &record.key.memory_space_id,
                scope,
                feedback.locator.owner_id(),
            )?;
            let owner: bm_core::skills::RuntimeSkillOwnerRecord = serde_json::from_value(
                before
                    .get(&(
                        super::schema::RUNTIME_SKILL_RECORD_NAMESPACE.to_owned(),
                        key,
                    ))
                    .ok_or_else(invalid)?
                    .clone(),
            )
            .map_err(|_| invalid())?;
            if !owner.validate_contract().accepted
                || owner.owner_revision != feedback.locator.owner_revision()
                || owner.content_digest != feedback.selected_content_digest
                || owner.memory_space_id != record.key.memory_space_id
                || &owner.owning_scope != scope
                || owner.lifecycle.availability
                    != bm_core::skills::RuntimeSkillAvailability::Enabled
                || matches!(
                    owner.lifecycle.state,
                    bm_core::skills::RuntimeSkillLifecycleState::Retired
                        | bm_core::skills::RuntimeSkillLifecycleState::Superseded
                )
            {
                return Err(invalid());
            }
        }
        for feedback in &evidence.task_learning_feedback {
            let owner: bm_core::task_execution::TaskLearningRecord = serde_json::from_value(
                before
                    .get(&("task_learning".to_owned(), feedback.learning_id.clone()))
                    .ok_or_else(invalid)?
                    .clone(),
            )
            .map_err(|_| invalid())?;
            let receipt = evidence.selection_receipt.as_ref().ok_or_else(invalid)?;
            if !owner.permits_usage_feedback(&record.key.channel_id, &receipt.identity.chat_id)
                || owner.canonical_content_digest()? != feedback.learning_digest
            {
                return Err(invalid());
            }
        }
        if let Some(receipt) = &evidence.selection_receipt {
            for selection in &receipt.agent_tool_experiences {
                if !evidence.agent_tool_feedback.iter().any(|feedback| {
                    feedback.registry_ref == selection.registry_ref
                        && feedback.tool_id == selection.tool_id
                        && feedback.schema_fingerprint == selection.schema_fingerprint
                }) {
                    continue;
                }
                let owner = bm_core::memory::GovernedMemoryOwnerRef::new(
                    bm_core::memory::GovernedMemoryOwnerPlane::AgentToolExperience,
                    &selection.experience_owner_id,
                );
                let scope = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
                    mounted_subject_id: record.subject.clone(),
                };
                let key = bm_core::skills::agent_tool_experience_head_key(
                    &record.key.memory_space_id,
                    &scope,
                    &owner,
                )?;
                let head: bm_core::skills::AgentToolExperienceOwnerHeadV2 = serde_json::from_value(
                    before
                        .get(&(
                            super::schema::AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.to_owned(),
                            key,
                        ))
                        .ok_or_else(invalid)?
                        .clone(),
                )
                .map_err(|_| invalid())?;
                if head.state == bm_core::skills::AgentToolExperienceHeadStateV2::Tombstoned
                    || head.current_revision != selection.experience_revision
                    || head.retained_revisions.last().is_none_or(|revision| {
                        revision.content_digest != selection.experience_content_digest
                    })
                {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bm_core::memory::{ProceduralApplicabilityContextV1, ProceduralProjectionIdentityV1};

    pub(crate) fn receipt() -> ProceduralSelectionReceiptV1 {
        ProceduralSelectionReceiptV1 {
            schema_version: 1,
            receipt_ref: String::new(),
            signing_key_id: String::new(),
            issued_at: 100,
            expires_at: 86_500,
            identity: ProceduralProjectionIdentityV1 {
                projection_id: "projection-a".into(),
                memory_space_id: "space-a".into(),
                mounted_subject_id: "subject-a".into(),
                channel: "desktop".into(),
                chat_id: "chat-a".into(),
                conversation_id: "conversation-a".into(),
                turn_id: "turn-a".into(),
            },
            applicability: ProceduralApplicabilityContextV1::try_new(None, None, None)
                .expect("applicability"),
            standard_agent_skills: Vec::new(),
            runtime_skills: Vec::new(),
            agent_tool_experiences: Vec::new(),
            task_learnings: Vec::new(),
            selection_digest: String::new(),
            delivery_digest: format!("sha256:{}", "a".repeat(64)),
            authority_tag: String::new(),
        }
    }

    #[test]
    fn procedural_selection_signing_authenticates_all_claims_and_store_incarnation() {
        let authority = ProceduralSelectionAuthority::fresh("space-a", 10).expect("authority");
        let mut receipt = receipt();
        authority.sign(&mut receipt).expect("signed");
        authority
            .verify(&receipt)
            .expect("durable validation has no expiry clock");
        let bytes = serde_json::to_vec(&authority).expect("synthetic persistence");
        let reopened: ProceduralSelectionAuthority =
            serde_json::from_slice(&bytes).expect("reopen");
        reopened.verify(&receipt).expect("same durable authority");
        let mut image = BTreeMap::new();
        verify_receipt_in_json(&receipt, &image)
            .expect_err("missing authority is never regenerated by validation");
        image.insert(
            (NAMESPACE.to_owned(), authority_key("space-a")),
            serde_json::to_value(&authority).expect("authority image"),
        );
        validate_store_image(&image).expect("canonical open image");
        verify_receipt_in_json(&receipt, &image).expect("same image verifies receipt");
        image
            .get_mut(&(NAMESPACE.to_owned(), authority_key("space-a")))
            .expect("key")["incarnation"] = Value::from("sha256:tampered");
        validate_store_image(&image)
            .expect_err("open rejects damaged incarnation without repair fallback");
        ProceduralSelectionAuthority::fresh("space-a", 10)
            .expect("new incarnation")
            .verify(&receipt)
            .expect_err("Store rebuild invalidates old receipt");
        for field in [
            "issued_at",
            "expires_at",
            "delivery_digest",
            "authority_tag",
        ] {
            let mut changed = serde_json::to_value(&receipt).expect("claims");
            changed[field] = if field.ends_with("_at") {
                Value::from(101)
            } else {
                Value::from(format!("sha256:{}", "b".repeat(64)))
            };
            let altered: ProceduralSelectionReceiptV1 =
                serde_json::from_value(changed).expect("changed receipt");
            authority
                .verify(&altered)
                .expect_err("modified full claims reject");
        }
        let mut changed = receipt.clone();
        changed.identity.mounted_subject_id = "subject-b".into();
        changed.selection_digest = changed.canonical_selection_digest().expect("digest");
        changed.receipt_ref = changed.canonical_receipt_ref().expect("ref");
        authority
            .verify(&changed)
            .expect_err("canonical recomputation cannot forge signature");
        let mut invalid = serde_json::to_value(&authority).expect("synthetic authority");
        invalid["secret_hex"] = Value::from("BAD");
        serde_json::from_value::<ProceduralSelectionAuthority>(invalid)
            .expect("typed envelope")
            .validate(&authority_key("space-a"))
            .expect_err("malformed secret rejected");
        authority
            .validate(&authority_key("space-b"))
            .expect_err("wrong exact address");
    }
}
