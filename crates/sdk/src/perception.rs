//! Runtime-owned, bounded visual delivery. The host owns capture and the final
//! Provider request; this module only admits sources and checks host receipts.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use bm_core::memory::{
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, LongTermMemorySourceScope,
    LongTermMemoryVersionMaterial, LongTermVisualReplayLineage, PerceptionCapabilitySnapshot,
    PerceptionCapabilityState, PerceptionEpisodeSourceRef, PerceptionInputEnvelope,
    PerceptionReplayOwnerV1, PerceptionRetentionCeiling, PerceptionScope, PerceptionSourceKey,
    PerceptionSourceKind, PerceptionSourceRef, PerceptionTime, SessionMessage,
    SessionMessageReplayLineage, TranscriptExternalContentManifestV1,
    TranscriptExternalContentUseV1, TranscriptInputMessage, VisualCaptureContext,
    VisualCaptureForm, VisualEvidenceOrigin, VisualInput, VisualObservation,
    VisualObservationUncertainty, VisualView,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store_internal::perception::{
    perception_media_storage_key, perception_source_commitment, PerceptionDeliveryEvidenceV1,
    PerceptionEpisodeEvidenceV1, PerceptionMediaOwnerRefV1, PerceptionTurnEpisodeIntakeV1,
    PerceptionTurnTaintInput, PerceptionTurnTaintV1, PERCEPTION_EPISODE_SCHEMA_VERSION,
};
use crate::{CanonicalTurnDelta, Error, MemoryRuntime, ProfileId, Result, StoreEventScope};

const MAX_RUNTIME_MEDIA_BYTES: usize = 32 * 1024 * 1024;
const MAX_ACTIVE_INPUTS: usize = 128;
const MAX_TAINTED_TURNS: usize = 4096;
const MEDIA_TTL_SECS: u64 = 5 * 60;
const MAX_FINAL_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROVIDER_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_PREPARED_PACKETS: usize = 128;
const MAX_EPHEMERAL_MESSAGES: usize = 32;
const MAX_EPHEMERAL_TEXT_BYTES: usize = 64 * 1024;
const EPHEMERAL_TTL_SECS: u64 = 30 * 60;

pub(crate) type VisualTurnEpisodeCandidates = (
    Option<PerceptionRetentionCeiling>,
    Vec<(PerceptionTurnEpisodeIntakeV1, u32)>,
);

#[derive(Clone)]
pub struct PerceptionHostHandle {
    host_id: String,
    device_id: Option<String>,
    secret: [u8; 32],
}

impl fmt::Debug for PerceptionHostHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PerceptionHostHandle")
            .field("host_id", &self.host_id)
            .field("device_id", &self.device_id)
            .field("secret", &"[redacted]")
            .finish()
    }
}

impl PerceptionHostHandle {
    pub fn host_id(&self) -> &str {
        &self.host_id
    }

    pub fn device_id(&self) -> Option<&str> {
        self.device_id.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualSubmissionReport {
    pub input_id: String,
    pub revision: u64,
    pub admitted_media_bytes: usize,
    pub evicted_input_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualSourceRevocation {
    pub host_id: String,
    pub device_id: Option<String>,
    pub source_id: String,
    pub prior_source_generation: u64,
    pub revoke_epoch: u64,
}

/// The host's ordered turn position at which the visual step was submitted.
/// A mid-turn position is bound to the same step_id in the media receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualTurnPosition {
    BeforeFirstAssistant,
    AfterAssistantMessages { completed_count: u32 },
}

impl VisualTurnPosition {
    fn completed_count(self) -> u32 {
        match self {
            Self::BeforeFirstAssistant => 0,
            Self::AfterAssistantMessages { completed_count } => completed_count,
        }
    }
}

#[derive(Clone)]
pub struct ProviderVisualPart {
    pub input_id: String,
    pub source_key: PerceptionSourceKey,
    pub source_kind: PerceptionSourceKind,
    pub ordinal: usize,
    pub step_id: Option<String>,
    pub mime: String,
    pub sha256: String,
    pub frame_sequence: Option<u64>,
    pub observed_at_unix_ms: Option<u64>,
    bytes: Arc<[u8]>,
}

impl fmt::Debug for ProviderVisualPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderVisualPart")
            .field("input_id", &self.input_id)
            .field("source_key", &self.source_key)
            .field("source_kind", &self.source_kind)
            .field("ordinal", &self.ordinal)
            .field("step_id", &self.step_id)
            .field("mime", &self.mime)
            .field("sha256", &self.sha256)
            .field("byte_len", &self.bytes.len())
            .finish()
    }
}

impl ProviderVisualPart {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug)]
pub struct ProviderVisualDeliveryPacket {
    pub packet_id: String,
    pub turn_id: String,
    pub revision: u64,
    pub parts: Vec<ProviderVisualPart>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum ProviderVisualWireRole {
    User,
    Tool,
}

#[derive(Clone)]
pub struct HostSentVisualPart {
    pub input_id: String,
    pub source_key: PerceptionSourceKey,
    pub ordinal: usize,
    pub step_id: Option<String>,
    pub message_index: usize,
    pub content_index: usize,
    pub wire_role: ProviderVisualWireRole,
    pub synthetic_user_role: bool,
    /// Digest of the SDK-prepared visual part, before any host conversion.
    pub parent_sha256: String,
    /// Exact media bytes in the final host API request, before Provider internals.
    pub sent_bytes: Vec<u8>,
    /// Declared host transformation steps; required if sent bytes differ.
    pub transform_steps: Vec<String>,
}

impl fmt::Debug for HostSentVisualPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostSentVisualPart")
            .field("input_id", &self.input_id)
            .field("source_key", &self.source_key)
            .field("ordinal", &self.ordinal)
            .field("message_index", &self.message_index)
            .field("content_index", &self.content_index)
            .field("sent_byte_len", &self.sent_bytes.len())
            .finish()
    }
}

#[derive(Clone)]
pub struct HostVisualDeliveryReceipt {
    pub packet_id: String,
    pub turn_id: String,
    /// Host attestation of the entire ordered request at its API boundary.
    /// The SDK cannot independently inspect text parts not included below.
    pub final_request_sha256: String,
    pub final_request_body: Vec<u8>,
    pub ordered_parts: Vec<HostSentVisualPart>,
}

impl fmt::Debug for HostVisualDeliveryReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostVisualDeliveryReceipt")
            .field("packet_id", &self.packet_id)
            .field("turn_id", &self.turn_id)
            .field("final_request_sha256", &self.final_request_sha256)
            .field("final_request_byte_len", &self.final_request_body.len())
            .field("ordered_parts", &self.ordered_parts)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualDeliveryProofLevel {
    SdkPrepared,
    HostAttested,
    ProviderResponded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualInferenceAuthorization {
    pub recipient_id: String,
    pub source_key: PerceptionSourceKey,
    pub media_sha256: Vec<String>,
    pub authorization_generation: u64,
    pub user_consent_attested: bool,
}

/// Host-attested consent ceiling for one exact source generation. The Runtime
/// verifies the grant against the registered host handle and revocation state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualSourceAuthorization {
    pub source_key: PerceptionSourceKey,
    pub source_kind: PerceptionSourceKind,
    pub revoke_epoch: u64,
    pub authorization_generation: u64,
    pub retention_ceiling: PerceptionRetentionCeiling,
    pub user_consent_attested: bool,
    pub user_attachment: Option<UserVisualAttachmentBinding>,
    /// Exact media digest after the host removed every audio track from Video.
    /// This is a host attestation, not an SDK container inspection claim.
    pub audio_free_video_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserVisualAttachmentBinding {
    pub turn_id: String,
    pub message_id: String,
    pub attachment_id: String,
    pub input_message_index: usize,
    pub user_message: TranscriptInputMessage,
    pub media_sha256: Vec<String>,
}

/// The host's single device tool/permission registry snapshot. The Runtime
/// derives subject-visible capability from this record only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualToolRegistrySnapshot {
    pub capability: PerceptionCapabilitySnapshot,
    pub retention_ceiling: PerceptionRetentionCeiling,
    pub user_consent_attested: bool,
}

impl VisualToolRegistrySnapshot {
    pub fn new(
        capability: PerceptionCapabilitySnapshot,
        retention_ceiling: PerceptionRetentionCeiling,
        user_consent_attested: bool,
    ) -> Self {
        Self {
            capability,
            retention_ceiling,
            user_consent_attested,
        }
    }
}

#[derive(Clone)]
pub(crate) struct VerifiedVisualInferenceInput {
    pub input_id: String,
    pub source_key: PerceptionSourceKey,
    pub source_kind: PerceptionSourceKind,
    pub scope: PerceptionScope,
    pub time: PerceptionTime,
    pub revoke_epoch: u64,
    pub retention: PerceptionRetentionCeiling,
    pub authorization_generation: u64,
    pub capture_context: Option<VisualCaptureContext>,
    pub recipient_id: String,
    pub media_parts: Vec<ProviderVisualPart>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualDeliveryProof {
    pub packet_id: String,
    pub turn_id: String,
    pub level: VisualDeliveryProofLevel,
    pub receipt_sha256: Option<String>,
}

#[derive(Clone)]
struct RegisteredHost {
    device_id: Option<String>,
    secret: [u8; 32],
}

#[derive(Clone)]
struct ActiveVisual {
    input_id: String,
    scope: PerceptionScope,
    time: PerceptionTime,
    source_kind: PerceptionSourceKind,
    source_key: PerceptionSourceKey,
    source_ref: PerceptionSourceRef,
    revoke_epoch: u64,
    view: VisualView,
    parts: Vec<ProviderVisualPart>,
    retention: PerceptionRetentionCeiling,
    authorization_generation: u64,
    position: VisualTurnPosition,
    revision: u64,
    admitted_at_secs: u64,
    media_bytes: usize,
    capture_context: Option<VisualCaptureContext>,
    audio_free_video_sha256: Option<String>,
    inference_authorization: Option<VisualInferenceAuthorization>,
    observation: Option<VisualObservation>,
    retain_media: bool,
    independent_persisted: bool,
}

#[derive(Clone)]
struct PreparedPacket {
    packet: ProviderVisualDeliveryPacket,
    level: VisualDeliveryProofLevel,
    receipt_sha256: Option<String>,
    final_request_sha256: Option<String>,
    response_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SourceIdentity {
    host_id: String,
    device_id: Option<String>,
    source_id: String,
}

impl SourceIdentity {
    fn from_key(key: &PerceptionSourceKey) -> Self {
        Self {
            host_id: key.host_id.clone(),
            device_id: key.device_id.clone(),
            source_id: key.source_id.clone(),
        }
    }
}

#[derive(Clone)]
struct SourceEpoch {
    generation: u64,
    revoke_epoch: u64,
    revoked: bool,
    kind: Option<PerceptionSourceKind>,
    sensor_id: Option<String>,
}

#[derive(Clone)]
struct TaintedVisual {
    input_id: String,
    source_key: PerceptionSourceKey,
    source_kind: PerceptionSourceKind,
    source_ref: PerceptionSourceRef,
    scope: PerceptionScope,
    time: PerceptionTime,
    revoke_epoch: u64,
    retention: PerceptionRetentionCeiling,
    authorization_generation: u64,
    position: VisualTurnPosition,
    delivery: Option<PerceptionDeliveryEvidenceV1>,
    revoked: bool,
    observation: Option<VisualObservation>,
    retain_media: bool,
    audio_free_video_sha256: Option<String>,
    independent_persisted: bool,
}

#[derive(Clone)]
struct EphemeralVisualTurn {
    turn_id: String,
    source_keys: Vec<PerceptionSourceKey>,
    messages: Vec<SessionMessage>,
    text_bytes: usize,
    admitted_at_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct VisualReplayUse {
    pub source: PerceptionEpisodeSourceRef,
    pub owner: PerceptionReplayOwnerV1,
}

#[derive(Clone, Default)]
struct TurnReplayReceipt {
    uses: Vec<VisualReplayUse>,
    ephemeral_context_present: bool,
    ephemeral_source_keys: Vec<PerceptionSourceKey>,
    subject_visual_unresolved: bool,
    unresolved_subject_sources: Vec<PerceptionEpisodeSourceRef>,
}

#[derive(Clone, Default)]
pub(crate) struct RuntimePerceptionState {
    hosts: HashMap<(String, Option<String>), RegisteredHost>,
    tool_registry: Vec<VisualToolRegistrySnapshot>,
    source_authorizations: HashMap<SourceIdentity, VisualSourceAuthorization>,
    sources: HashMap<SourceIdentity, SourceEpoch>,
    active: VecDeque<ActiveVisual>,
    packets: HashMap<String, PreparedPacket>,
    tainted_turns: HashMap<String, Vec<TaintedVisual>>,
    ephemeral_turns: VecDeque<EphemeralVisualTurn>,
    ephemeral_finalized_turns: HashSet<String>,
    suppressed_turns: HashSet<String>,
    replay_receipts: HashMap<String, TurnReplayReceipt>,
    external_content_manifests:
        HashMap<String, (PerceptionHostHandle, TranscriptExternalContentManifestV1)>,
    next_revision: u64,
    media_bytes: usize,
}

impl RuntimePerceptionState {
    pub(crate) fn authorize_source(
        &mut self,
        handle: &PerceptionHostHandle,
        authorization: VisualSourceAuthorization,
    ) -> Result<()> {
        self.validate_handle(handle)?;
        let key = &authorization.source_key;
        valid_id(&key.source_id, "source_id")?;
        if key.host_id != handle.host_id
            || key.device_id != handle.device_id
            || key.source_generation == 0
            || authorization.authorization_generation == 0
            || !authorization.user_consent_attested
            || authorization.source_kind == PerceptionSourceKind::HostStructuredObservation
        {
            return Err(invalid(
                "visual source authorization lacks trusted host consent",
            ));
        }
        match authorization.source_kind {
            PerceptionSourceKind::HumanUserSharedMedia => {
                let binding = authorization.user_attachment.as_ref().ok_or_else(|| {
                    invalid("user-shared visual lacks canonical attachment binding")
                })?;
                valid_id(&binding.turn_id, "turn_id")?;
                valid_id(&binding.message_id, "message_id")?;
                valid_id(&binding.attachment_id, "attachment_id")?;
                if binding.user_message.role != "user"
                    || binding.user_message.authority
                        != bm_core::memory::MemoryEvidenceAuthority::UserAsserted
                    || binding.media_sha256.is_empty()
                    || binding
                        .media_sha256
                        .iter()
                        .any(|digest| !valid_sha256(digest))
                    || authorization
                        .audio_free_video_sha256
                        .as_ref()
                        .is_some_and(|digest| !valid_sha256(digest))
                {
                    return Err(invalid(
                        "user visual attachment is not a canonical user ingress",
                    ));
                }
            }
            PerceptionSourceKind::HostVisualObservation => {
                if authorization.user_attachment.is_some()
                    || authorization
                        .audio_free_video_sha256
                        .as_ref()
                        .is_some_and(|digest| !valid_sha256(digest))
                {
                    return Err(invalid(
                        "camera authorization contains invalid attachment or video proof",
                    ));
                }
            }
            PerceptionSourceKind::HostStructuredObservation => unreachable!(),
        }
        let identity = SourceIdentity::from_key(key);
        if let Some(epoch) = self.sources.get(&identity) {
            if key.source_generation < epoch.generation
                || (epoch.revoked && key.source_generation <= epoch.generation)
                || authorization.revoke_epoch != epoch.revoke_epoch
            {
                return Err(invalid(
                    "visual source authorization uses a revoked generation",
                ));
            }
        }
        if self.source_authorizations.contains_key(&identity) {
            return Err(invalid(
                "visual source authorization is immutable until revocation",
            ));
        }
        self.source_authorizations.insert(identity, authorization);
        Ok(())
    }

    fn check_source_authorization(
        &self,
        input: &PerceptionInputEnvelope<VisualInput>,
        retention: PerceptionRetentionCeiling,
        media_sha256: &[String],
        video_sha256: Option<&str>,
    ) -> Result<()> {
        let source_key = &input.source.key;
        let source_kind = input.source.kind;
        let grant = self
            .source_authorizations
            .get(&SourceIdentity::from_key(source_key));
        if grant.is_none()
            && retention == PerceptionRetentionCeiling::CurrentTurn
            && source_kind == PerceptionSourceKind::HostVisualObservation
            && video_sha256.is_none()
        {
            return Ok(());
        }
        let grant = grant.ok_or_else(|| invalid("visual source has no consent authorization"))?;
        if grant.source_key != *source_key
            || grant.source_kind != source_kind
            || grant.revoke_epoch != input.revoke_epoch
            || !grant.user_consent_attested
            || retention > grant.retention_ceiling
            || video_sha256 != grant.audio_free_video_sha256.as_deref()
        {
            return Err(invalid("visual use exceeds its exact source authorization"));
        }
        match source_kind {
            PerceptionSourceKind::HostVisualObservation => {
                let PerceptionSourceRef::Sensor { sensor_id } = &input.source.source_ref else {
                    return Err(invalid("visual sensor identity is absent"));
                };
                if !self.tool_registry.iter().any(|snapshot| {
                    let capability = &snapshot.capability;
                    capability.host_id == source_key.host_id
                        && source_key.device_id.as_deref() == Some(capability.device_id.as_str())
                        && capability.sensor_id.as_str() == sensor_id.as_str()
                        && capability.state == PerceptionCapabilityState::Available
                        && capability.authorization_generation == grant.authorization_generation
                        && snapshot.user_consent_attested
                        && grant.retention_ceiling <= snapshot.retention_ceiling
                }) {
                    return Err(invalid(
                        "visual grant differs from current sensor permission",
                    ));
                }
            }
            PerceptionSourceKind::HumanUserSharedMedia => {
                let PerceptionSourceRef::UserAttachment {
                    message_id,
                    attachment_id,
                } = &input.source.source_ref
                else {
                    return Err(invalid("user attachment identity is absent"));
                };
                let turn_id = input
                    .scope
                    .turn_id
                    .as_deref()
                    .ok_or_else(|| invalid("user attachment lacks turn"))?;
                let binding = grant
                    .user_attachment
                    .as_ref()
                    .ok_or_else(|| invalid("canonical user attachment is not registered"))?;
                if binding.turn_id != turn_id
                    || binding.message_id.as_str() != message_id.as_str()
                    || binding.attachment_id.as_str() != attachment_id.as_str()
                    || binding.media_sha256 != media_sha256
                {
                    return Err(invalid(
                        "visual media differs from registered user attachment",
                    ));
                }
            }
            PerceptionSourceKind::HostStructuredObservation => {
                return Err(invalid("structured source cannot carry visual media"))
            }
        }
        Ok(())
    }

    pub(crate) fn record_beetle_observation(
        &mut self,
        input_id: &str,
        response_body: &[u8],
        request_receipt_digest: &str,
        observation: VisualObservation,
        retain_media: bool,
        now_secs: u64,
    ) -> Result<()> {
        self.prune_expired(now_secs);
        observation.validate_contract()?;
        if response_body.is_empty() || response_body.len() > MAX_PROVIDER_RESPONSE_BYTES {
            return Err(invalid(
                "Beetle visual inference response is unavailable or oversized",
            ));
        }
        let entry = self
            .active
            .iter()
            .find(|entry| entry.input_id == input_id)
            .ok_or_else(|| invalid("visual media expired before Beetle observation"))?;
        let authorization = entry
            .inference_authorization
            .as_ref()
            .ok_or_else(|| invalid("Beetle inference recipient was not authorized"))?;
        let VisualEvidenceOrigin::BeetleInference {
            provider_id,
            request_receipt_digest: origin_receipt_digest,
            inference_digest,
        } = &observation.evidence_origin
        else {
            return Err(invalid(
                "Beetle observation needs independent inference provenance",
            ));
        };
        if provider_id != &authorization.recipient_id
            || !valid_sha256(request_receipt_digest)
            || origin_receipt_digest != request_receipt_digest
            || inference_digest != &format!("sha256:{:x}", Sha256::digest(response_body))
            || observation.input_id != entry.input_id
            || observation.source_key != entry.source_key
            || observation.scope != entry.scope
            || observation.revoke_epoch != entry.revoke_epoch
            || observation.observed_at_unix_ms != entry.time.observed_at_unix_ms
            || !entry.retention.permits_durable_semantic()
        {
            return Err(invalid(
                "Beetle observation differs from authorized visual inference",
            ));
        }
        if retain_media
            && (!entry.retention.permits_durable_media()
                || !observation
                    .classification
                    .media_evidence_allowed_for_source(entry.source_kind)
                || observation.uncertainty != VisualObservationUncertainty::Low
                || entry.parts.len() != 1
                || entry.parts[0].bytes().len() > 8 * 1024 * 1024)
        {
            return Err(invalid(
                "Beetle observation cannot retain this visual media",
            ));
        }
        if let Some(existing) = &entry.observation {
            if existing == &observation && entry.retain_media == retain_media {
                return Ok(());
            }
            return Err(invalid("Beetle visual observation cannot be replaced"));
        }
        if self.tainted_turns.values().flatten().any(|taint| {
            taint.input_id == input_id && (taint.revoked || taint.source_key != entry.source_key)
        }) {
            return Err(invalid("visual source was revoked before observation"));
        }
        let entry = self
            .active
            .iter_mut()
            .find(|entry| entry.input_id == input_id)
            .expect("validated active visual input remains in the same state mutation");
        entry.observation = Some(observation.clone());
        entry.retain_media = retain_media;
        for taints in self.tainted_turns.values_mut() {
            for taint in taints.iter_mut().filter(|taint| taint.input_id == input_id) {
                taint.observation = Some(observation.clone());
                taint.retain_media = retain_media;
            }
        }
        Ok(())
    }
    pub(crate) fn authorize_inference(
        &mut self,
        handle: &PerceptionHostHandle,
        input_id: &str,
        authorization: VisualInferenceAuthorization,
        now_secs: u64,
    ) -> Result<()> {
        self.validate_handle(handle)?;
        self.prune_expired(now_secs);
        valid_id(&authorization.recipient_id, "recipient_id")?;
        if !authorization.user_consent_attested || authorization.authorization_generation == 0 {
            return Err(invalid(
                "additional visual inference recipient lacks host-attested consent",
            ));
        }
        let entry = self
            .active
            .iter_mut()
            .find(|entry| entry.input_id == input_id)
            .ok_or_else(|| invalid("visual input is unavailable for inference"))?;
        if entry.source_key.host_id != handle.host_id
            || entry.source_key.device_id != handle.device_id
            || entry.source_key != authorization.source_key
            || entry
                .parts
                .iter()
                .map(|part| part.sha256.clone())
                .collect::<Vec<_>>()
                != authorization.media_sha256
        {
            return Err(invalid(
                "inference authorization differs from visual source or media",
            ));
        }
        if entry.inference_authorization.as_ref().is_some_and(|old| {
            old.authorization_generation >= authorization.authorization_generation
        }) {
            return Err(invalid(
                "inference authorization generation did not advance",
            ));
        }
        entry.inference_authorization = Some(authorization);
        Ok(())
    }

    pub(crate) fn inference_input(
        &mut self,
        input_id: &str,
        now_secs: u64,
    ) -> Result<VerifiedVisualInferenceInput> {
        self.prune_expired(now_secs);
        let entry = self
            .active
            .iter()
            .find(|entry| entry.input_id == input_id)
            .ok_or_else(|| invalid("visual media expired before inference"))?;
        let authorization = entry
            .inference_authorization
            .as_ref()
            .ok_or_else(|| invalid("additional visual inference recipient is not authorized"))?;
        let source = self
            .sources
            .get(&SourceIdentity::from_key(&entry.source_key))
            .ok_or_else(|| invalid("visual inference source state is unavailable"))?;
        if source.revoked
            || source.generation != entry.source_key.source_generation
            || source.revoke_epoch != entry.revoke_epoch
        {
            return Err(invalid("visual inference source was revoked"));
        }
        Ok(VerifiedVisualInferenceInput {
            input_id: entry.input_id.clone(),
            source_key: entry.source_key.clone(),
            source_kind: entry.source_kind,
            scope: entry.scope.clone(),
            time: entry.time,
            revoke_epoch: entry.revoke_epoch,
            retention: entry.retention,
            authorization_generation: entry.authorization_generation,
            capture_context: entry.capture_context.clone(),
            recipient_id: authorization.recipient_id.clone(),
            media_parts: entry.parts.clone(),
        })
    }

    pub(crate) fn independent_episode_intake(
        &mut self,
        input_id: &str,
        now_secs: u64,
    ) -> Result<PerceptionTurnEpisodeIntakeV1> {
        let input = self.inference_input(input_id, now_secs)?;
        let active = self
            .active
            .iter()
            .find(|entry| entry.input_id == input_id)
            .ok_or_else(|| invalid("independent visual episode lost its media owner"))?;
        if active.independent_persisted {
            return Err(invalid("independent visual episode is already persisted"));
        }
        let observation = active
            .observation
            .clone()
            .ok_or_else(|| invalid("independent visual episode has no Beetle observation"))?;
        let episode_id = format!(
            "visual-independent:sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&(&input.input_id, &input.source_key, input.revoke_epoch))
                    .map_err(|error| invalid(error.to_string()))?
            )
        );
        let mut episode = PerceptionEpisodeEvidenceV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: input.scope.memory_space_id,
            subject_id: input.scope.subject_id,
            relationship_id: input.scope.relationship_id,
            source: input.source_key,
            revoke_epoch: input.revoke_epoch,
            episode_id,
            input_id: input.input_id,
            modality: bm_core::memory::PerceptionModality::Visual,
            source_kind: input.source_kind,
            origin_ref: active.source_ref.clone(),
            time: input.time,
            retention_ceiling: input.retention,
            authorization_generation: active.authorization_generation,
            audio_free_video_sha256: active.audio_free_video_sha256.clone(),
            observation: Some(observation),
            delivery: None,
            media: None,
        };
        let media_bytes = if active.retain_media {
            if input.media_parts.len() != 1 || input.media_parts[0].bytes().len() > 8 * 1024 * 1024
            {
                return Err(invalid("independent visual media shape is not admissible"));
            }
            let part = &input.media_parts[0];
            let bytes = part.bytes().to_vec();
            episode.media = Some(PerceptionMediaOwnerRefV1 {
                key: perception_media_storage_key(&episode.key()?),
                sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
                mime: part.mime.clone(),
                byte_len: bytes.len(),
            });
            Some(bytes)
        } else {
            None
        };
        episode.validate_contract()?;
        Ok(PerceptionTurnEpisodeIntakeV1 {
            episode,
            media_bytes,
        })
    }

    pub(crate) fn mark_independent_persisted(&mut self, input_id: &str) -> Result<()> {
        let entry = self
            .active
            .iter_mut()
            .find(|entry| entry.input_id == input_id)
            .ok_or_else(|| {
                invalid("independent visual source expired before persistence acknowledgement")
            })?;
        if entry.observation.is_none() {
            return Err(invalid("independent visual source lacks observation"));
        }
        entry.independent_persisted = true;
        for taints in self.tainted_turns.values_mut() {
            for taint in taints.iter_mut().filter(|taint| taint.input_id == input_id) {
                taint.independent_persisted = true;
            }
        }
        Ok(())
    }
    pub(crate) fn register_host(
        &mut self,
        host_id: String,
        device_id: Option<String>,
    ) -> Result<PerceptionHostHandle> {
        valid_id(&host_id, "host_id")?;
        if let Some(id) = &device_id {
            valid_id(id, "device_id")?;
        }
        let registration_key = (host_id.clone(), device_id.clone());
        if self.hosts.contains_key(&registration_key) {
            return Err(invalid("host/device is already registered in this Runtime"));
        }
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret)
            .map_err(|_| invalid("host registration secret is unavailable"))?;
        self.hosts.insert(
            registration_key,
            RegisteredHost {
                device_id: device_id.clone(),
                secret,
            },
        );
        Ok(PerceptionHostHandle {
            host_id,
            device_id,
            secret,
        })
    }

    pub(crate) fn set_tool_registry(
        &mut self,
        handle: &PerceptionHostHandle,
        snapshot: VisualToolRegistrySnapshot,
    ) -> Result<()> {
        self.validate_handle(handle)?;
        let capability = &snapshot.capability;
        capability.validate_contract()?;
        if capability.state == PerceptionCapabilityState::Available
            && !snapshot.user_consent_attested
        {
            return Err(invalid(
                "available visual tool lacks host-attested permission",
            ));
        }
        if capability.host_id != handle.host_id
            || handle.device_id.as_deref() != Some(capability.device_id.as_str())
        {
            return Err(invalid(
                "capability does not belong to the registered host device",
            ));
        }
        let updated_host_id = capability.host_id.clone();
        let updated_device_id = capability.device_id.clone();
        let updated_sensor_id = capability.sensor_id.clone();
        let updated_state = capability.state;
        let index = self.tool_registry.iter().position(|old| {
            let old = &old.capability;
            old.host_id == capability.host_id
                && old.device_id == capability.device_id
                && old.sensor_id == capability.sensor_id
        });
        let changed_existing_sensor = index.is_some();
        if let Some(index) = index {
            let old = &self.tool_registry[index].capability;
            if capability.tool_registry_revision <= old.tool_registry_revision
                || capability.authorization_generation < old.authorization_generation
            {
                return Err(invalid("capability or authorization revision regressed"));
            }
            self.tool_registry[index] = snapshot;
        } else {
            self.tool_registry.push(snapshot);
        }
        if changed_existing_sensor || updated_state != PerceptionCapabilityState::Available {
            self.revoke_sensor_sources(&updated_host_id, &updated_device_id, &updated_sensor_id);
        }
        Ok(())
    }

    pub(crate) fn capabilities(&self) -> Vec<PerceptionCapabilitySnapshot> {
        self.tool_registry
            .iter()
            .map(|entry| entry.capability.clone())
            .collect()
    }

    pub(crate) fn visual_views(&mut self, now_secs: u64) -> Vec<VisualView> {
        self.prune_expired(now_secs);
        self.active
            .iter()
            .filter(|entry| {
                self.tainted_turns.values().flatten().any(|taint| {
                    taint.input_id == entry.input_id
                        && taint.source_key == entry.source_key
                        && !taint.revoked
                        && taint.delivery.is_some()
                })
            })
            .map(|entry| entry.view.clone())
            .collect()
    }

    pub(crate) fn submit_visual(
        &mut self,
        handle: &PerceptionHostHandle,
        input: PerceptionInputEnvelope<VisualInput>,
        retention: PerceptionRetentionCeiling,
        position: VisualTurnPosition,
        capture_context: Option<VisualCaptureContext>,
        now_secs: u64,
    ) -> Result<VisualSubmissionReport> {
        self.validate_handle(handle)?;
        input.validate_contract()?;
        if input.source.key.host_id != handle.host_id
            || input.source.key.device_id != handle.device_id
        {
            return Err(invalid(
                "visual source is outside the registered host device",
            ));
        }
        let media_sha256 = input
            .payload
            .media_parts()
            .iter()
            .map(|part| part.sha256.to_string())
            .collect::<Vec<_>>();
        let video_sha256 = match &input.payload {
            VisualInput::Video(video) => Some(video.media.sha256.as_str()),
            _ => None,
        };
        self.check_source_authorization(&input, retention, &media_sha256, video_sha256)?;
        if let Some(context) = &capture_context {
            context.validate_contract()?;
            if input.source.kind != PerceptionSourceKind::HostVisualObservation
                || context.source_key != input.source.key
                || input.time.observed_at_unix_ms != Some(context.captured_at_unix_ms)
                || input.scope.turn_id.as_deref() != context.active_interaction_turn_id.as_deref()
            {
                return Err(invalid(
                    "capture context differs from the admitted visual source",
                ));
            }
        }
        if input.source.kind == PerceptionSourceKind::HostVisualObservation {
            let PerceptionSourceRef::Sensor { sensor_id } = &input.source.source_ref else {
                return Err(invalid("host visual source lacks sensor identity"));
            };
            let form = match &input.payload {
                VisualInput::Image(_) => VisualCaptureForm::Image,
                VisualInput::Video(_) => VisualCaptureForm::Video,
                VisualInput::FrameStream(_) => VisualCaptureForm::FrameStream,
            };
            if !self.tool_registry.iter().any(|snapshot| {
                let capability = &snapshot.capability;
                capability.host_id == handle.host_id
                    && handle.device_id.as_deref() == Some(capability.device_id.as_str())
                    && capability.sensor_id == *sensor_id
                    && capability.state == PerceptionCapabilityState::Available
                    && snapshot.user_consent_attested
                    && capability.visual_forms.contains(&form)
            }) {
                return Err(invalid(
                    "host visual source has no current available capability",
                ));
            }
        }
        if matches!(
            position,
            VisualTurnPosition::AfterAssistantMessages { completed_count: 0 }
        ) || (matches!(position, VisualTurnPosition::AfterAssistantMessages { .. })
            != input.scope.step_id.is_some())
            || (input.scope.turn_id.is_none()
                && position != VisualTurnPosition::BeforeFirstAssistant)
        {
            return Err(invalid(
                "visual turn position does not match its step binding",
            ));
        }
        self.prune_expired(now_secs);
        if self
            .active
            .iter()
            .any(|entry| entry.input_id == input.input_id)
        {
            return Err(invalid("visual input_id has already been admitted"));
        }
        let source = SourceIdentity::from_key(&input.source.key);
        let authorization_generation = self
            .source_authorizations
            .get(&source)
            .map_or(0, |grant| grant.authorization_generation);
        let sensor_id = match &input.source.source_ref {
            PerceptionSourceRef::Sensor { sensor_id } => Some(sensor_id.clone()),
            _ => None,
        };
        let audio_free_video_sha256 = self
            .source_authorizations
            .get(&source)
            .and_then(|authorization| authorization.audio_free_video_sha256.clone());
        let epoch = self.sources.entry(source).or_insert(SourceEpoch {
            generation: input.source.key.source_generation,
            revoke_epoch: input.revoke_epoch,
            revoked: false,
            kind: Some(input.source.kind),
            sensor_id: sensor_id.clone(),
        });
        if input.revoke_epoch != epoch.revoke_epoch
            || input.source.key.source_generation < epoch.generation
            || (epoch.revoked && input.source.key.source_generation <= epoch.generation)
            || epoch.kind.is_some_and(|kind| kind != input.source.kind)
            || epoch.sensor_id != sensor_id
        {
            return Err(invalid(
                "visual input uses a revoked or stale source generation",
            ));
        }
        epoch.generation = input.source.key.source_generation;
        epoch.revoked = false;
        epoch.kind = Some(input.source.kind);
        let view = VisualView::from_input(&input)?;
        let parts: Vec<ProviderVisualPart> = input
            .payload
            .media_parts()
            .into_iter()
            .map(|part| ProviderVisualPart {
                input_id: input.input_id.clone(),
                source_key: input.source.key.clone(),
                source_kind: input.source.kind,
                ordinal: part.ordinal,
                step_id: input.scope.step_id.clone(),
                mime: part.mime.to_string(),
                sha256: part.sha256.to_string(),
                frame_sequence: part.frame_sequence,
                observed_at_unix_ms: part.observed_at_unix_ms,
                bytes: Arc::from(part.bytes),
            })
            .collect();
        let media_bytes = parts.iter().map(|part| part.bytes.len()).sum::<usize>();
        if media_bytes > MAX_RUNTIME_MEDIA_BYTES {
            return Err(invalid("visual input exceeds Runtime media capacity"));
        }
        if let Some(turn_id) = &input.scope.turn_id {
            if !self.tainted_turns.contains_key(turn_id)
                && self.tainted_turns.len() >= MAX_TAINTED_TURNS
            {
                return Err(invalid("visual turn lineage capacity is exhausted"));
            }
        }
        let mut evicted_input_ids = Vec::new();
        while self.media_bytes.saturating_add(media_bytes) > MAX_RUNTIME_MEDIA_BYTES
            || self.active.len() >= MAX_ACTIVE_INPUTS
        {
            let Some(old) = self.active.pop_front() else {
                return Err(invalid("visual Runtime media capacity is unavailable"));
            };
            self.media_bytes = self.media_bytes.saturating_sub(old.media_bytes);
            evicted_input_ids.push(old.input_id);
        }
        self.remove_packets_using_inputs(&evicted_input_ids);
        self.next_revision = self.next_revision.saturating_add(1);
        let revision = self.next_revision;
        let input_id = input.input_id.clone();
        self.active.push_back(ActiveVisual {
            input_id: input_id.clone(),
            scope: input.scope,
            time: input.time,
            source_kind: input.source.kind,
            source_key: input.source.key,
            source_ref: input.source.source_ref,
            revoke_epoch: input.revoke_epoch,
            view,
            parts,
            retention,
            authorization_generation,
            position,
            revision,
            admitted_at_secs: now_secs,
            media_bytes,
            capture_context: capture_context.clone(),
            audio_free_video_sha256,
            inference_authorization: None,
            observation: None,
            retain_media: false,
            independent_persisted: false,
        });
        self.media_bytes = self.media_bytes.saturating_add(media_bytes);
        let entry = self.active.back().expect("visual input was just inserted");
        if let Some(turn_id) = &entry.scope.turn_id {
            self.tainted_turns
                .entry(turn_id.clone())
                .or_default()
                .push(TaintedVisual {
                    input_id: entry.input_id.clone(),
                    source_key: entry.source_key.clone(),
                    source_kind: entry.source_kind,
                    source_ref: entry.source_ref.clone(),
                    scope: entry.scope.clone(),
                    time: entry.time,
                    revoke_epoch: entry.revoke_epoch,
                    retention,
                    authorization_generation: entry.authorization_generation,
                    position,
                    delivery: None,
                    revoked: false,
                    observation: None,
                    retain_media: false,
                    audio_free_video_sha256: entry.audio_free_video_sha256.clone(),
                    independent_persisted: false,
                });
        }
        Ok(VisualSubmissionReport {
            input_id,
            revision,
            admitted_media_bytes: media_bytes,
            evicted_input_ids,
        })
    }

    pub(crate) fn prepare_turn(
        &mut self,
        session_id: &str,
        turn_id: &str,
        since_revision: u64,
        now_secs: u64,
    ) -> Result<Option<ProviderVisualDeliveryPacket>> {
        self.prune_expired(now_secs);
        let selected: Vec<&ActiveVisual> = self
            .active
            .iter()
            .filter(|entry| {
                entry.revision > since_revision
                    && entry.scope.session_id == session_id
                    && entry
                        .scope
                        .turn_id
                        .as_deref()
                        .is_none_or(|bound| bound == turn_id)
            })
            .collect();
        if selected.is_empty() {
            return Ok(None);
        }
        let first_source = &selected[0].source_key;
        if selected.iter().any(|entry| {
            entry.source_key.host_id != first_source.host_id
                || entry.source_key.device_id != first_source.device_id
        }) {
            return Err(invalid(
                "one visual delivery packet cannot span multiple host devices",
            ));
        }
        if !self.tainted_turns.contains_key(turn_id)
            && self.tainted_turns.len() >= MAX_TAINTED_TURNS
        {
            return Err(invalid("visual turn lineage capacity is exhausted"));
        }
        let taints = self.tainted_turns.entry(turn_id.to_string()).or_default();
        for entry in &selected {
            if !taints.iter().any(|taint| taint.input_id == entry.input_id) {
                taints.push(TaintedVisual {
                    input_id: entry.input_id.clone(),
                    source_key: entry.source_key.clone(),
                    source_kind: entry.source_kind,
                    source_ref: entry.source_ref.clone(),
                    scope: entry.scope.clone(),
                    time: entry.time,
                    revoke_epoch: entry.revoke_epoch,
                    retention: entry.retention,
                    authorization_generation: entry.authorization_generation,
                    position: entry.position,
                    delivery: None,
                    revoked: false,
                    observation: entry.observation.clone(),
                    retain_media: entry.retain_media,
                    audio_free_video_sha256: entry.audio_free_video_sha256.clone(),
                    independent_persisted: entry.independent_persisted,
                });
            }
        }
        let revision = selected
            .iter()
            .map(|entry| entry.revision)
            .max()
            .unwrap_or(0);
        let parts: Vec<ProviderVisualPart> = selected
            .iter()
            .flat_map(|entry| entry.parts.iter().cloned())
            .collect();
        let packet_id = packet_digest(turn_id, revision, &parts);
        if !self.packets.contains_key(&packet_id) && self.packets.len() >= MAX_PREPARED_PACKETS {
            return Err(invalid("visual delivery packet capacity is exhausted"));
        }
        let packet = ProviderVisualDeliveryPacket {
            packet_id: packet_id.clone(),
            turn_id: turn_id.to_string(),
            revision,
            parts,
        };
        self.packets
            .entry(packet_id)
            .or_insert_with(|| PreparedPacket {
                packet: packet.clone(),
                level: VisualDeliveryProofLevel::SdkPrepared,
                receipt_sha256: None,
                final_request_sha256: None,
                response_sha256: None,
            });
        Ok(Some(packet))
    }

    pub(crate) fn attest_host_send(
        &mut self,
        handle: &PerceptionHostHandle,
        receipt: HostVisualDeliveryReceipt,
        now_secs: u64,
    ) -> Result<VisualDeliveryProof> {
        self.validate_handle(handle)?;
        self.prune_expired(now_secs);
        let prepared = self
            .packets
            .get(&receipt.packet_id)
            .ok_or_else(|| invalid("visual delivery packet is unknown or expired"))?;
        if prepared.packet.turn_id != receipt.turn_id
            || prepared.packet.parts.len() != receipt.ordered_parts.len()
            || !valid_sha256(&receipt.final_request_sha256)
            || receipt.final_request_body.is_empty()
            || receipt.final_request_body.len() > MAX_FINAL_REQUEST_BYTES
            || receipt.final_request_sha256
                != format!("sha256:{:x}", Sha256::digest(&receipt.final_request_body))
        {
            return Err(invalid(
                "visual delivery receipt does not match the prepared packet",
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(receipt.final_request_sha256.as_bytes());
        let mut last_position: Option<(usize, usize)> = None;
        let mut total_sent_media_bytes = 0usize;
        let mut sent_media_by_input: HashMap<String, Vec<String>> = HashMap::new();
        for (expected, sent) in prepared.packet.parts.iter().zip(&receipt.ordered_parts) {
            if expected.input_id != sent.input_id
                || expected.source_key != sent.source_key
                || expected.ordinal != sent.ordinal
                || expected.step_id != sent.step_id
                || expected.sha256 != sent.parent_sha256
                || expected.source_key.host_id != handle.host_id
                || expected.source_key.device_id != handle.device_id
                || sent.sent_bytes.is_empty()
                || sent.sent_bytes.len() > MAX_RUNTIME_MEDIA_BYTES
            {
                return Err(invalid("visual sent part differs from the prepared source"));
            }
            total_sent_media_bytes = total_sent_media_bytes
                .checked_add(sent.sent_bytes.len())
                .ok_or_else(|| invalid("visual receipt media size overflow"))?;
            if total_sent_media_bytes > MAX_RUNTIME_MEDIA_BYTES {
                return Err(invalid("visual receipt media exceeds the runtime limit"));
            }
            let position = (sent.message_index, sent.content_index);
            if last_position.is_some_and(|previous| position <= previous) {
                return Err(invalid(
                    "visual sent positions must be in exact request order",
                ));
            }
            last_position = Some(position);
            let source = SourceIdentity::from_key(&sent.source_key);
            let Some(epoch) = self.sources.get(&source) else {
                return Err(invalid("visual source state is unavailable"));
            };
            if epoch.revoked || epoch.generation != sent.source_key.source_generation {
                return Err(invalid("visual source was revoked before host receipt"));
            }
            let current = self.active.iter().any(|entry| {
                entry.input_id == sent.input_id
                    && entry.source_key == sent.source_key
                    && entry.revoke_epoch == epoch.revoke_epoch
            });
            if !current {
                return Err(invalid(
                    "visual media expired or was evicted before host receipt",
                ));
            }
            match expected.source_kind {
                PerceptionSourceKind::HumanUserSharedMedia => {
                    if sent.wire_role != ProviderVisualWireRole::User || sent.synthetic_user_role {
                        return Err(invalid(
                            "user-shared visual must remain a real user message",
                        ));
                    }
                    let entry = self
                        .active
                        .iter()
                        .find(|entry| {
                            entry.input_id == sent.input_id && entry.source_key == sent.source_key
                        })
                        .ok_or_else(|| invalid("user visual source expired before delivery"))?;
                    let binding = self
                        .source_authorizations
                        .get(&SourceIdentity::from_key(&sent.source_key))
                        .and_then(|grant| grant.user_attachment.as_ref())
                        .ok_or_else(|| invalid("user visual attachment binding was revoked"))?;
                    if entry.scope.turn_id.as_deref() != Some(binding.turn_id.as_str())
                        || sent.message_index != binding.input_message_index
                    {
                        return Err(invalid("user visual part is not on its canonical message"));
                    }
                }
                PerceptionSourceKind::HostVisualObservation => {
                    if (sent.wire_role == ProviderVisualWireRole::User) != sent.synthetic_user_role
                    {
                        return Err(invalid(
                            "host visual wire role needs exact synthetic marker",
                        ));
                    }
                }
                PerceptionSourceKind::HostStructuredObservation => {
                    return Err(invalid("structured input is not a visual media part"));
                }
            }
            let actual = format!("sha256:{:x}", Sha256::digest(&sent.sent_bytes));
            if actual != expected.sha256 && sent.transform_steps.is_empty() {
                return Err(invalid(
                    "changed media needs a declared host transformation",
                ));
            }
            if sent.transform_steps.len() > 16
                || sent
                    .transform_steps
                    .iter()
                    .any(|step| step.is_empty() || step.len() > 160)
            {
                return Err(invalid("host transformation descriptor is invalid"));
            }
            hasher.update(sent.input_id.as_bytes());
            hasher.update(sent.message_index.to_be_bytes());
            hasher.update(sent.content_index.to_be_bytes());
            hasher.update([match sent.wire_role {
                ProviderVisualWireRole::User => 1,
                ProviderVisualWireRole::Tool => 2,
            }]);
            hasher.update([u8::from(sent.synthetic_user_role)]);
            if let Some(step_id) = &sent.step_id {
                hasher.update(step_id.as_bytes());
            }
            hasher.update(sent.parent_sha256.as_bytes());
            hasher.update(actual.as_bytes());
            sent_media_by_input
                .entry(sent.input_id.clone())
                .or_default()
                .push(actual);
            for step in &sent.transform_steps {
                hasher.update(step.as_bytes());
            }
        }
        let digest = format!("sha256:{:x}", hasher.finalize());
        if prepared.level != VisualDeliveryProofLevel::SdkPrepared {
            if prepared.receipt_sha256.as_deref() == Some(digest.as_str()) {
                return Ok(VisualDeliveryProof {
                    packet_id: receipt.packet_id,
                    turn_id: receipt.turn_id,
                    level: prepared.level,
                    receipt_sha256: prepared.receipt_sha256.clone(),
                });
            }
            return Err(invalid("visual delivery receipt cannot be replaced"));
        }
        let prepared = self
            .packets
            .get_mut(&receipt.packet_id)
            .ok_or_else(|| invalid("visual delivery packet disappeared"))?;
        prepared.level = VisualDeliveryProofLevel::HostAttested;
        prepared.receipt_sha256 = Some(digest.clone());
        prepared.final_request_sha256 = Some(receipt.final_request_sha256.clone());
        if let Some(taints) = self.tainted_turns.get_mut(&receipt.turn_id) {
            for taint in taints {
                if let Some(sent_digests) = sent_media_by_input.get(&taint.input_id) {
                    let sent_media_sha256 = if sent_digests.len() == 1 {
                        sent_digests[0].clone()
                    } else {
                        let mut media_hasher = Sha256::new();
                        for sent_digest in sent_digests {
                            media_hasher.update(sent_digest.as_bytes());
                        }
                        format!("sha256:{:x}", media_hasher.finalize())
                    };
                    taint.delivery = Some(PerceptionDeliveryEvidenceV1 {
                        input_id: taint.input_id.clone(),
                        packet_id: receipt.packet_id.clone(),
                        turn_id: receipt.turn_id.clone(),
                        final_request_sha256: receipt.final_request_sha256.clone(),
                        sent_media_sha256,
                        receipt_sha256: digest.clone(),
                    });
                }
            }
        }
        Ok(VisualDeliveryProof {
            packet_id: receipt.packet_id,
            turn_id: receipt.turn_id,
            level: VisualDeliveryProofLevel::HostAttested,
            receipt_sha256: Some(digest),
        })
    }

    pub(crate) fn turn_sources(
        &mut self,
        turn_id: &str,
        now_secs: u64,
    ) -> Vec<(
        PerceptionSourceKey,
        PerceptionRetentionCeiling,
        Option<String>,
    )> {
        self.prune_expired(now_secs);
        self.tainted_turns
            .get(turn_id)
            .into_iter()
            .flat_map(|taints| taints.iter())
            .map(|taint| {
                (
                    taint.source_key.clone(),
                    taint.retention,
                    if taint.revoked {
                        None
                    } else {
                        taint
                            .delivery
                            .as_ref()
                            .map(|delivery| delivery.receipt_sha256.clone())
                    },
                )
            })
            .collect()
    }

    pub(crate) fn turn_episode_candidates(
        &mut self,
        turn_id: &str,
    ) -> Result<VisualTurnEpisodeCandidates> {
        let Some(taints) = self.tainted_turns.get(turn_id) else {
            return Ok((None, Vec::new()));
        };
        let retention = taints
            .iter()
            .map(|taint| taint.retention)
            .min()
            .ok_or_else(|| invalid("visual turn has an empty source lineage"))?;
        if self.suppressed_turns.contains(turn_id)
            || taints.iter().any(|taint| {
                taint.revoked
                    || taint.delivery.is_none()
                    || (taint.retention.permits_durable_semantic()
                        && self
                            .source_authorizations
                            .get(&SourceIdentity::from_key(&taint.source_key))
                            .is_none_or(|grant| {
                                grant.source_key != taint.source_key
                                    || grant.revoke_epoch != taint.revoke_epoch
                                    || taint.retention > grant.retention_ceiling
                            }))
            })
        {
            self.suppressed_turns.insert(turn_id.into());
            return Ok((Some(PerceptionRetentionCeiling::CurrentTurn), Vec::new()));
        }
        if !retention.permits_durable_semantic() {
            return Ok((Some(retention), Vec::new()));
        }
        let mut episodes = Vec::with_capacity(taints.len());
        for taint in taints {
            let delivery = taint
                .delivery
                .clone()
                .ok_or_else(|| invalid("durable visual reply lacks a verified host receipt"))?;
            let mut hasher = Sha256::new();
            hasher.update(turn_id.as_bytes());
            hasher.update(taint.input_id.as_bytes());
            hasher.update(taint.source_key.host_id.as_bytes());
            hasher.update(taint.source_key.source_id.as_bytes());
            let episode_id = format!("visual:sha256:{:x}", hasher.finalize());
            let mut episode = PerceptionEpisodeEvidenceV1 {
                schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
                memory_space_id: taint.scope.memory_space_id.clone(),
                subject_id: taint.scope.subject_id.clone(),
                relationship_id: taint.scope.relationship_id.clone(),
                source: taint.source_key.clone(),
                revoke_epoch: taint.revoke_epoch,
                episode_id,
                input_id: taint.input_id.clone(),
                modality: bm_core::memory::PerceptionModality::Visual,
                source_kind: taint.source_kind,
                origin_ref: taint.source_ref.clone(),
                time: taint.time,
                retention_ceiling: taint.retention,
                authorization_generation: taint.authorization_generation,
                audio_free_video_sha256: taint.audio_free_video_sha256.clone(),
                observation: (!taint.independent_persisted)
                    .then(|| taint.observation.clone())
                    .flatten(),
                delivery: Some(delivery),
                media: None,
            };
            let media_bytes = if taint.retain_media && !taint.independent_persisted {
                let active = self
                    .active
                    .iter()
                    .find(|entry| {
                        entry.input_id == taint.input_id
                            && entry.source_key == taint.source_key
                            && entry.revoke_epoch == taint.revoke_epoch
                    })
                    .ok_or_else(|| {
                        invalid("durable visual media expired before canonical intake")
                    })?;
                if active.parts.len() != 1 || active.parts[0].bytes().len() > 8 * 1024 * 1024 {
                    return Err(invalid("durable visual media shape is not admissible"));
                }
                let part = &active.parts[0];
                let bytes = part.bytes().to_vec();
                episode.media = Some(PerceptionMediaOwnerRefV1 {
                    key: perception_media_storage_key(&episode.key()?),
                    sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
                    mime: part.mime.clone(),
                    byte_len: bytes.len(),
                });
                Some(bytes)
            } else {
                None
            };
            episode.validate_contract()?;
            episodes.push((
                PerceptionTurnEpisodeIntakeV1 {
                    episode,
                    media_bytes,
                },
                taint.position.completed_count(),
            ));
        }
        Ok((Some(retention), episodes))
    }

    pub(crate) fn mark_provider_response(
        &mut self,
        handle: &PerceptionHostHandle,
        packet_id: &str,
        final_request_sha256: &str,
        response_body: Vec<u8>,
    ) -> Result<VisualDeliveryProof> {
        self.validate_handle(handle)?;
        if response_body.is_empty() || response_body.len() > MAX_PROVIDER_RESPONSE_BYTES {
            return Err(invalid(
                "provider response exceeds the bounded receipt size",
            ));
        }
        let response_sha256 = format!("sha256:{:x}", Sha256::digest(&response_body));
        let packet = self
            .packets
            .get_mut(packet_id)
            .ok_or_else(|| invalid("visual delivery packet is unavailable"))?;
        if packet.packet.parts.iter().any(|part| {
            part.source_key.host_id != handle.host_id
                || part.source_key.device_id != handle.device_id
        }) {
            return Err(invalid(
                "provider response is outside the registered host device",
            ));
        }
        if packet.final_request_sha256.as_deref() != Some(final_request_sha256) {
            return Err(invalid(
                "provider response request differs from host receipt",
            ));
        }
        if packet.level == VisualDeliveryProofLevel::ProviderResponded {
            if packet.response_sha256.as_ref() == Some(&response_sha256) {
                return Ok(VisualDeliveryProof {
                    packet_id: packet_id.to_string(),
                    turn_id: packet.packet.turn_id.clone(),
                    level: VisualDeliveryProofLevel::ProviderResponded,
                    receipt_sha256: packet.receipt_sha256.clone(),
                });
            }
            return Err(invalid("provider response cannot be replaced"));
        }
        if packet.level != VisualDeliveryProofLevel::HostAttested {
            return Err(invalid(
                "provider response cannot replace missing host receipt",
            ));
        }
        packet.level = VisualDeliveryProofLevel::ProviderResponded;
        packet.response_sha256 = Some(response_sha256);
        Ok(VisualDeliveryProof {
            packet_id: packet_id.to_string(),
            turn_id: packet.packet.turn_id.clone(),
            level: VisualDeliveryProofLevel::ProviderResponded,
            receipt_sha256: packet.receipt_sha256.clone(),
        })
    }

    pub(crate) fn clear_session(&mut self) -> Vec<VisualSourceRevocation> {
        let mut revoked = self
            .sources
            .iter_mut()
            .map(|(key, source)| {
                source.revoke_epoch = source.revoke_epoch.saturating_add(1);
                source.revoked = true;
                VisualSourceRevocation {
                    host_id: key.host_id.clone(),
                    device_id: key.device_id.clone(),
                    source_id: key.source_id.clone(),
                    prior_source_generation: source.generation,
                    revoke_epoch: source.revoke_epoch,
                }
            })
            .collect::<Vec<_>>();
        revoked.sort_by(|left, right| {
            (&left.host_id, &left.device_id, &left.source_id).cmp(&(
                &right.host_id,
                &right.device_id,
                &right.source_id,
            ))
        });
        self.active.clear();
        self.packets.clear();
        self.source_authorizations.clear();
        self.ephemeral_turns.clear();
        self.replay_receipts.clear();
        self.external_content_manifests.clear();
        for taints in self.tainted_turns.values_mut() {
            for taint in taints {
                taint.revoked = true;
                taint.delivery = None;
            }
        }
        self.media_bytes = 0;
        revoked
    }

    pub(crate) fn revoke_all_current_visual_sources(
        &mut self,
        unavailable_capabilities: Vec<PerceptionCapabilitySnapshot>,
    ) -> Result<Vec<VisualSourceRevocation>> {
        if unavailable_capabilities.len() != self.tool_registry.len() {
            return Err(invalid(
                "global visual revoke must cover every registered sensor",
            ));
        }
        let mut seen = HashSet::new();
        for capability in &unavailable_capabilities {
            capability.validate_contract()?;
            if capability.state == PerceptionCapabilityState::Available
                || !seen.insert((
                    capability.host_id.clone(),
                    capability.device_id.clone(),
                    capability.sensor_id.clone(),
                ))
            {
                return Err(invalid(
                    "global visual revoke contains available or duplicate sensor",
                ));
            }
            let old = self
                .tool_registry
                .iter()
                .map(|snapshot| &snapshot.capability)
                .find(|old| {
                    old.host_id == capability.host_id
                        && old.device_id == capability.device_id
                        && old.sensor_id == capability.sensor_id
                })
                .ok_or_else(|| invalid("global visual revoke contains an unregistered sensor"))?;
            if capability.tool_registry_revision <= old.tool_registry_revision
                || capability.authorization_generation < old.authorization_generation
            {
                return Err(invalid(
                    "global visual revoke revision or authorization regressed",
                ));
            }
        }
        for capability in unavailable_capabilities {
            let snapshot = self
                .tool_registry
                .iter_mut()
                .find(|snapshot| {
                    snapshot.capability.host_id == capability.host_id
                        && snapshot.capability.device_id == capability.device_id
                        && snapshot.capability.sensor_id == capability.sensor_id
                })
                .expect("validated registered sensor");
            snapshot.capability = capability;
            snapshot.user_consent_attested = false;
        }
        Ok(self.clear_session())
    }

    pub(crate) fn revoke_source(
        &mut self,
        handle: &PerceptionHostHandle,
        source_id: &str,
    ) -> Result<u64> {
        self.validate_handle(handle)?;
        valid_id(source_id, "source_id")?;
        let key = SourceIdentity {
            host_id: handle.host_id.clone(),
            device_id: handle.device_id.clone(),
            source_id: source_id.to_string(),
        };
        let epoch = self.sources.entry(key).or_insert(SourceEpoch {
            generation: 0,
            revoke_epoch: 0,
            revoked: false,
            kind: None,
            sensor_id: None,
        });
        epoch.revoke_epoch = epoch.revoke_epoch.saturating_add(1);
        epoch.revoked = true;
        let new_epoch = epoch.revoke_epoch;
        self.source_authorizations.remove(&SourceIdentity {
            host_id: handle.host_id.clone(),
            device_id: handle.device_id.clone(),
            source_id: source_id.to_string(),
        });
        self.mark_source_taints_revoked(&handle.host_id, &handle.device_id, source_id);
        self.remove_source_inputs(&handle.host_id, &handle.device_id, source_id);
        self.ephemeral_turns.retain(|turn| {
            !turn.source_keys.iter().any(|key| {
                key.host_id == handle.host_id
                    && key.device_id == handle.device_id
                    && key.source_id == source_id
            })
        });
        Ok(new_epoch)
    }

    fn revoke_sensor_sources(&mut self, host_id: &str, device_id: &str, sensor_id: &str) {
        let ids: Vec<String> = self
            .sources
            .iter()
            .filter(|(key, epoch)| {
                key.host_id == host_id
                    && key.device_id.as_deref() == Some(device_id)
                    && epoch.kind == Some(PerceptionSourceKind::HostVisualObservation)
                    && epoch.sensor_id.as_deref() == Some(sensor_id)
            })
            .map(|(key, _)| key.source_id.clone())
            .collect();
        for source_id in ids {
            let key = SourceIdentity {
                host_id: host_id.to_string(),
                device_id: Some(device_id.to_string()),
                source_id: source_id.clone(),
            };
            if let Some(epoch) = self.sources.get_mut(&key) {
                if !epoch.revoked {
                    epoch.revoke_epoch = epoch.revoke_epoch.saturating_add(1);
                    epoch.revoked = true;
                }
            }
            self.source_authorizations.remove(&key);
            self.mark_source_taints_revoked(host_id, &Some(device_id.to_string()), &source_id);
            self.remove_source_inputs(host_id, &Some(device_id.to_string()), &source_id);
            self.ephemeral_turns.retain(|turn| {
                !turn.source_keys.iter().any(|key| {
                    key.host_id == host_id
                        && key.device_id.as_deref() == Some(device_id)
                        && key.source_id == source_id
                })
            });
        }
    }

    fn remove_source_inputs(&mut self, host_id: &str, device_id: &Option<String>, source_id: &str) {
        self.external_content_manifests.retain(|_, (_, manifest)| {
            !manifest.uses.iter().any(|usage| {
                matches!(usage, TranscriptExternalContentUseV1::Visual { source, .. }
                    if source.source.host_id == host_id
                        && source.source.device_id == *device_id
                        && source.source.source_id == source_id)
            })
        });
        self.active.retain(|entry| {
            let key = &entry.source_key;
            let keep =
                key.host_id != host_id || key.device_id != *device_id || key.source_id != source_id;
            if !keep {
                self.media_bytes = self.media_bytes.saturating_sub(entry.media_bytes);
            }
            keep
        });
        self.packets.retain(|_, packet| {
            !packet.packet.parts.iter().any(|part| {
                part.source_key.host_id == host_id
                    && part.source_key.device_id == *device_id
                    && part.source_key.source_id == source_id
            })
        });
    }

    fn mark_source_taints_revoked(
        &mut self,
        host_id: &str,
        device_id: &Option<String>,
        source_id: &str,
    ) {
        for taints in self.tainted_turns.values_mut() {
            for taint in taints {
                if taint.source_key.host_id == host_id
                    && taint.source_key.device_id == *device_id
                    && taint.source_key.source_id == source_id
                {
                    taint.revoked = true;
                    taint.delivery = None;
                }
            }
        }
    }

    fn prune_expired(&mut self, now_secs: u64) {
        self.ephemeral_turns
            .retain(|turn| now_secs.saturating_sub(turn.admitted_at_secs) < EPHEMERAL_TTL_SECS);
        let expired: Vec<String> = self
            .active
            .iter()
            .filter(|entry| now_secs.saturating_sub(entry.admitted_at_secs) >= MEDIA_TTL_SECS)
            .map(|entry| entry.input_id.clone())
            .collect();
        if expired.is_empty() {
            return;
        }
        self.active.retain(|entry| {
            let keep = !expired.contains(&entry.input_id);
            if !keep {
                self.media_bytes = self.media_bytes.saturating_sub(entry.media_bytes);
            }
            keep
        });
        self.remove_packets_using_inputs(&expired);
    }

    pub(crate) fn remember_ephemeral_turn(
        &mut self,
        turn_id: &str,
        messages: Vec<SessionMessage>,
        inherited_source_keys: &[PerceptionSourceKey],
        now_secs: u64,
    ) -> Result<()> {
        self.prune_expired(now_secs);
        if self.ephemeral_finalized_turns.contains(turn_id) {
            return Ok(());
        }
        let taints = self.tainted_turns.get(turn_id);
        if taints.is_some_and(|taints| {
            taints.iter().any(|taint| {
                taint.revoked
                    || taint.delivery.is_none()
                    || taint.retention < PerceptionRetentionCeiling::EphemeralSession
            })
        }) {
            return Err(invalid(
                "ephemeral turn has revoked or unverified visual sources",
            ));
        }
        let mut source_keys = taints
            .into_iter()
            .flatten()
            .map(|taint| taint.source_key.clone())
            .collect::<Vec<_>>();
        let mut admitted_at_secs = now_secs;
        for key in inherited_source_keys {
            let origin = self
                .ephemeral_turns
                .iter()
                .find(|turn| turn.source_keys.contains(key))
                .ok_or_else(|| invalid("inherited ephemeral visual source expired"))?;
            admitted_at_secs = admitted_at_secs.min(origin.admitted_at_secs);
            source_keys.push(key.clone());
        }
        source_keys.sort_by(|left, right| {
            (
                &left.host_id,
                &left.device_id,
                &left.source_id,
                left.source_generation,
            )
                .cmp(&(
                    &right.host_id,
                    &right.device_id,
                    &right.source_id,
                    right.source_generation,
                ))
        });
        source_keys.dedup();
        if source_keys.is_empty() {
            return Err(invalid("ephemeral turn lacks visual source lineage"));
        }
        let text_bytes = messages
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>();
        if text_bytes > MAX_EPHEMERAL_TEXT_BYTES || messages.len() > MAX_EPHEMERAL_MESSAGES {
            return Err(invalid("ephemeral turn exceeds the in-memory text limit"));
        }
        if self
            .ephemeral_turns
            .iter()
            .any(|turn| turn.turn_id == turn_id)
        {
            return Ok(());
        }
        while self
            .ephemeral_turns
            .iter()
            .map(|turn| turn.messages.len())
            .sum::<usize>()
            .saturating_add(messages.len())
            > MAX_EPHEMERAL_MESSAGES
            || self
                .ephemeral_turns
                .iter()
                .map(|turn| turn.text_bytes)
                .sum::<usize>()
                .saturating_add(text_bytes)
                > MAX_EPHEMERAL_TEXT_BYTES
        {
            self.ephemeral_turns.pop_front();
        }
        self.ephemeral_turns.push_back(EphemeralVisualTurn {
            turn_id: turn_id.into(),
            source_keys,
            messages,
            text_bytes,
            admitted_at_secs,
        });
        self.ephemeral_finalized_turns.insert(turn_id.into());
        Ok(())
    }

    pub(crate) fn ephemeral_context(
        &mut self,
        now_secs: u64,
    ) -> (Vec<SessionMessage>, Vec<PerceptionSourceKey>) {
        self.prune_expired(now_secs);
        let messages = self
            .ephemeral_turns
            .iter()
            .flat_map(|turn| turn.messages.clone())
            .collect();
        let mut source_keys = self
            .ephemeral_turns
            .iter()
            .flat_map(|turn| turn.source_keys.clone())
            .collect::<Vec<_>>();
        source_keys.sort_by(|left, right| {
            (
                &left.host_id,
                &left.device_id,
                &left.source_id,
                left.source_generation,
            )
                .cmp(&(
                    &right.host_id,
                    &right.device_id,
                    &right.source_id,
                    right.source_generation,
                ))
        });
        source_keys.dedup();
        (messages, source_keys)
    }

    fn remove_packets_using_inputs(&mut self, input_ids: &[String]) {
        if input_ids.is_empty() {
            return;
        }
        self.packets.retain(|_, packet| {
            !packet
                .packet
                .parts
                .iter()
                .any(|part| input_ids.contains(&part.input_id))
        });
    }

    fn validate_handle(&self, handle: &PerceptionHostHandle) -> Result<()> {
        let Some(registered) = self
            .hosts
            .get(&(handle.host_id.clone(), handle.device_id.clone()))
        else {
            return Err(invalid("perception host is not registered"));
        };
        if registered.device_id != handle.device_id || registered.secret != handle.secret {
            return Err(invalid(
                "perception host capability does not match registration",
            ));
        }
        Ok(())
    }
}

impl MemoryRuntime {
    fn assert_visual_source_live(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
        revoke_epoch: u64,
    ) -> Result<()> {
        self.config
            .store_platform
            .as_ref()
            .ok_or_else(|| invalid("visual source generation check requires the Store"))?
            .assert_perception_source_live(scope, source, revoke_epoch)
    }

    fn assert_visual_source_authorized(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
        kind: PerceptionSourceKind,
        revoke_epoch: u64,
        authorization_generation: u64,
        retention: PerceptionRetentionCeiling,
    ) -> Result<()> {
        if authorization_generation == 0 {
            if kind == PerceptionSourceKind::HostVisualObservation
                && retention == PerceptionRetentionCeiling::CurrentTurn
            {
                return self.assert_visual_source_live(scope, source, revoke_epoch);
            }
            return Err(invalid("visual use lacks durable source authorization"));
        }
        self.config
            .store_platform
            .as_ref()
            .ok_or_else(|| invalid("visual source authorization requires the Store"))?
            .assert_perception_source_authorized(
                scope,
                source,
                kind,
                revoke_epoch,
                authorization_generation,
                retention,
            )
    }

    /// Registers the host's exact consent/attachment/audio boundary before
    /// media admission. This is host-attested evidence, not Provider proof.
    pub fn authorize_visual_source(
        &self,
        handle: &PerceptionHostHandle,
        authorization: VisualSourceAuthorization,
    ) -> Result<()> {
        self.ensure_visual_sdk_profile()?;
        let scope = self.visual_authorization_scope(
            authorization
                .user_attachment
                .as_ref()
                .map(|binding| binding.turn_id.as_str()),
        );
        let mut state = self.perception_state()?;
        let mut trial = state.clone();
        trial.authorize_source(handle, authorization.clone())?;
        let transaction_id = format!(
            "visual-authorize:sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    &authorization.source_key,
                    authorization.revoke_epoch,
                    authorization.authorization_generation,
                    authorization.retention_ceiling,
                ))
                .map_err(|error| invalid(error.to_string()))?
            )
        );
        self.config
            .store_platform
            .as_ref()
            .ok_or_else(|| invalid("visual authorization requires the Store"))?
            .register_perception_source_authorization(
                &scope,
                &authorization.source_key,
                authorization.source_kind,
                authorization.revoke_epoch,
                authorization.authorization_generation,
                authorization.retention_ceiling,
                authorization.user_consent_attested,
                self.visual_event_scope(),
                transaction_id,
                self.config.clock.now_secs(),
            )?;
        *state = trial;
        Ok(())
    }

    fn visual_authorization_scope(&self, turn_id: Option<&str>) -> PerceptionScope {
        PerceptionScope {
            memory_space_id: self.config.memory_space_id.clone(),
            subject_id: self.config.subject_id.clone(),
            relationship_id: self
                .config
                .scoped_runtime
                .relationship_scope
                .as_ref()
                .map(|relationship| relationship.relationship_id.clone()),
            session_id: self.config.scope.conversation_id_or_chat_id().to_string(),
            turn_id: turn_id.map(str::to_string),
            step_id: None,
        }
    }

    fn visual_event_scope(&self) -> StoreEventScope {
        StoreEventScope::new(
            self.config.identity.agent_id.clone(),
            self.config.identity.owner_id.clone(),
            self.config.scope.channel.clone(),
            self.config.scope.chat_id.clone(),
        )
        .with_memory_space(self.config.memory_space_id.clone())
        .with_subject(self.config.subject_id.clone())
        .with_conversation(self.config.scope.conversation_id_or_chat_id().to_string())
    }

    pub(crate) fn verify_visual_user_attachments(&self, turn: &CanonicalTurnDelta) -> Result<()> {
        let state = self.perception_state()?;
        let Some(taints) = state.tainted_turns.get(&turn.turn_id) else {
            return Ok(());
        };
        for taint in taints.iter().filter(|taint| {
            taint.source_kind == PerceptionSourceKind::HumanUserSharedMedia && !taint.revoked
        }) {
            let grant = state
                .source_authorizations
                .get(&SourceIdentity::from_key(&taint.source_key))
                .ok_or_else(|| invalid("user visual attachment authorization disappeared"))?;
            let binding = grant
                .user_attachment
                .as_ref()
                .ok_or_else(|| invalid("user visual attachment binding disappeared"))?;
            if binding.turn_id != turn.turn_id
                || turn.input_messages.get(binding.input_message_index)
                    != Some(&binding.user_message)
            {
                return Err(invalid(
                    "visual attachment is not in the canonical user turn",
                ));
            }
        }
        Ok(())
    }

    fn cross_relationship_user_visual_owner(
        &self,
        source: &PerceptionEpisodeSourceRef,
        owner: &PerceptionReplayOwnerV1,
    ) -> Result<bool> {
        let PerceptionReplayOwnerV1::LongTermMemory {
            owner_id,
            owner_revision,
        } = owner
        else {
            return Ok(false);
        };
        let owner_ref =
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id.clone());
        let material_key = bm_core::memory::long_term_version_material_key(
            &source.memory_space_id,
            &source.memory_space_id,
            &owner_ref,
            *owner_revision,
        )?;
        let store = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| invalid("visual replay requires Store"))?;
        let Some(document) = store
            .read_json_docs_by_keys(
                crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
                &[material_key],
            )?
            .into_iter()
            .next()
        else {
            return Ok(false);
        };
        let material: LongTermMemoryVersionMaterial = serde_json::from_value(document.value)
            .map_err(|_| invalid("visual replay long-term owner is malformed"))?;
        Ok(material.validate_contract().accepted
            && material.memory_space_id == source.memory_space_id
            && material.owner_ref == owner_ref
            && material.owner_revision == *owner_revision
            && material.governed_content.source_scope == LongTermMemorySourceScope::User
            && material
                .governed_content
                .perception_sources
                .contains(source))
    }

    pub(crate) fn record_visual_projection_replay(
        &self,
        turn_id: &str,
        recent: &[SessionMessageReplayLineage],
        long_term: &[LongTermVisualReplayLineage],
        ephemeral_source_keys: &[PerceptionSourceKey],
        subject_visual_sources: &[PerceptionEpisodeSourceRef],
    ) -> Result<()> {
        if !self.visual_sdk_profile_supported() {
            return Ok(());
        }
        valid_id(turn_id, "turn_id")?;
        let mut uses = Vec::<VisualReplayUse>::new();
        for message in recent {
            if message.perception_sources.is_empty() {
                continue;
            }
            let reference = message
                .transcript_ref
                .as_ref()
                .ok_or_else(|| invalid("visual replay message has no transcript owner"))?;
            for source in &message.perception_sources {
                uses.push(VisualReplayUse {
                    source: source.clone(),
                    owner: PerceptionReplayOwnerV1::TranscriptMessage {
                        reference: reference.clone(),
                    },
                });
            }
        }
        for owner in long_term {
            for source in &owner.perception_sources {
                uses.push(VisualReplayUse {
                    source: source.clone(),
                    owner: PerceptionReplayOwnerV1::LongTermMemory {
                        owner_id: owner.owner_id.clone(),
                        owner_revision: owner.owner_revision,
                    },
                });
            }
        }
        uses.sort();
        uses.dedup_by(|left, right| left.source == right.source);
        for use_ in &uses {
            use_.source.validate_contract()?;
            if use_.source.memory_space_id != self.config.memory_space_id
                || use_.source.subject_id != self.config.subject_id
            {
                return Err(invalid("visual replay source escapes the mounted subject"));
            }
            let mounted_relationship = self
                .config
                .scoped_runtime
                .relationship_scope
                .as_ref()
                .map(|relationship| relationship.relationship_id.as_str());
            if use_.source.relationship_id.as_deref() != mounted_relationship
                && !self.cross_relationship_user_visual_owner(&use_.source, &use_.owner)?
            {
                return Err(invalid(
                    "visual replay source escapes the mounted relationship",
                ));
            }
            self.config
                .store_platform
                .as_ref()
                .ok_or_else(|| invalid("visual replay requires Store"))?
                .assert_perception_episode_replay_allowed(&use_.source)?;
            let scope = self.visual_authorization_scope(Some(turn_id));
            self.mark_visual_turn_taint(
                &scope,
                &use_.source.source,
                &use_.source.episode_id,
                use_.source.revoke_epoch,
                PerceptionRetentionCeiling::DurableSemantic,
                self.config.clock.now_secs(),
            )?;
        }
        for source in subject_visual_sources {
            source.validate_contract()?;
            if source.memory_space_id != self.config.memory_space_id
                || source.subject_id != self.config.subject_id
            {
                return Err(invalid("Soul visual source escapes mounted subject"));
            }
            let scope = self.visual_authorization_scope(Some(turn_id));
            self.mark_visual_turn_taint(
                &scope,
                &source.source,
                &source.episode_id,
                source.revoke_epoch,
                PerceptionRetentionCeiling::CurrentTurn,
                self.config.clock.now_secs(),
            )?;
        }
        let mut state = self.perception_state()?;
        if !state.replay_receipts.contains_key(turn_id)
            && state.replay_receipts.len() >= MAX_TAINTED_TURNS
        {
            return Err(invalid("visual replay receipt capacity is exhausted"));
        }
        let receipt = state
            .replay_receipts
            .entry(turn_id.to_string())
            .or_default();
        receipt.ephemeral_context_present |= !ephemeral_source_keys.is_empty();
        receipt.subject_visual_unresolved |= !subject_visual_sources.is_empty();
        receipt
            .unresolved_subject_sources
            .extend_from_slice(subject_visual_sources);
        receipt.unresolved_subject_sources.sort();
        receipt.unresolved_subject_sources.dedup();
        receipt
            .ephemeral_source_keys
            .extend_from_slice(ephemeral_source_keys);
        receipt.ephemeral_source_keys.sort_by(|left, right| {
            (
                &left.host_id,
                &left.device_id,
                &left.source_id,
                left.source_generation,
            )
                .cmp(&(
                    &right.host_id,
                    &right.device_id,
                    &right.source_id,
                    right.source_generation,
                ))
        });
        receipt.ephemeral_source_keys.dedup();
        receipt.uses.extend(uses);
        receipt.uses.sort();
        receipt
            .uses
            .dedup_by(|left, right| left.source == right.source);
        Ok(())
    }

    pub(crate) fn visual_replay_receipt(
        &self,
        turn_id: &str,
    ) -> Result<Option<(Vec<VisualReplayUse>, bool, bool)>> {
        Ok(self
            .perception_state()?
            .replay_receipts
            .get(turn_id)
            .map(|receipt| {
                (
                    receipt.uses.clone(),
                    receipt.ephemeral_context_present,
                    receipt.subject_visual_unresolved,
                )
            }))
    }

    fn verify_turn_external_content_manifest(
        &self,
        turn_id: &str,
        manifest: &TranscriptExternalContentManifestV1,
    ) -> Result<()> {
        valid_id(turn_id, "turn_id")?;
        if manifest.request_sha256.is_empty()
            || manifest.request_sha256.len() > 64
            || manifest.uses.len() > 128
            || manifest
                .request_sha256
                .iter()
                .any(|digest| !valid_sha256(digest))
            || manifest
                .request_sha256
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || manifest.uses.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(invalid("turn external-content manifest is not canonical"));
        }
        let (retention, candidates) = self.visual_turn_episode_candidates(turn_id)?;
        let replayed = self
            .visual_replay_receipt(turn_id)?
            .map(|(uses, _, _)| uses)
            .unwrap_or_default();
        if (!candidates.is_empty() || !replayed.is_empty())
            && retention.is_none_or(|ceiling| !ceiling.permits_durable_semantic())
        {
            return Err(invalid(
                "turn external-content manifest has no durable visual source closure",
            ));
        }
        let mut seen_direct = HashSet::new();
        let mut seen_replayed = HashSet::new();
        for usage in &manifest.uses {
            let request_sha256 = match usage {
                TranscriptExternalContentUseV1::Visual {
                    source,
                    input_id,
                    packet_id,
                    request_sha256,
                    receipt_digest,
                    tool_observation_id,
                } => {
                    source.validate_contract()?;
                    valid_id(input_id, "input_id")?;
                    valid_id(packet_id, "packet_id")?;
                    if tool_observation_id
                        .as_deref()
                        .is_some_and(|id| valid_id(id, "tool_observation_id").is_err())
                    {
                        return Err(invalid("visual tool observation ID is invalid"));
                    }
                    let matched = candidates.iter().any(|(intake, _)| {
                        let episode = &intake.episode;
                        episode.source_ref().ok().as_ref() == Some(source)
                            && episode.input_id == *input_id
                            && episode.delivery.as_ref().is_some_and(|delivery| {
                                delivery.packet_id == *packet_id
                                    && delivery.final_request_sha256 == *request_sha256
                                    && delivery.receipt_sha256 == *receipt_digest
                            })
                    });
                    if !matched || !seen_direct.insert(source.clone()) {
                        return Err(invalid(
                            "visual external-content use differs from verified episode",
                        ));
                    }
                    request_sha256
                }
                TranscriptExternalContentUseV1::ReplayedVisual {
                    source,
                    owner,
                    request_sha256,
                } => {
                    source.validate_contract()?;
                    if !replayed
                        .iter()
                        .any(|use_| use_.source == *source && use_.owner == *owner)
                        || !seen_replayed.insert(source.clone())
                    {
                        return Err(invalid(
                            "replayed visual external-content use differs from its owner",
                        ));
                    }
                    request_sha256
                }
                TranscriptExternalContentUseV1::Other {
                    content_digest,
                    request_sha256,
                    tool_observation_id,
                    ..
                } => {
                    if !valid_sha256(content_digest)
                        || tool_observation_id
                            .as_deref()
                            .is_some_and(|id| valid_id(id, "tool_observation_id").is_err())
                    {
                        return Err(invalid("other external-content use is invalid"));
                    }
                    request_sha256
                }
            };
            if !manifest.request_sha256.contains(request_sha256) {
                return Err(invalid(
                    "external-content use has no attested final request",
                ));
            }
        }
        if seen_direct.len() != candidates.len() || seen_replayed.len() != replayed.len() {
            return Err(invalid(
                "external-content manifest omits a visual source in this turn",
            ));
        }
        Ok(())
    }

    /// Gives the host exact SDK-known visual entries for its complete final
    /// request inventory. The host must add every other external item and
    /// attest completeness itself; this helper does not make that claim.
    pub fn prepare_turn_visual_external_uses(
        &self,
        handle: &PerceptionHostHandle,
        turn_id: &str,
        replay_request_sha256: Option<&str>,
    ) -> Result<Vec<TranscriptExternalContentUseV1>> {
        self.ensure_visual_sdk_profile()?;
        valid_id(turn_id, "turn_id")?;
        self.perception_state()?.validate_handle(handle)?;
        let (retention, candidates) = self.visual_turn_episode_candidates(turn_id)?;
        let replayed = self
            .visual_replay_receipt(turn_id)?
            .map(|(uses, _, _)| uses)
            .unwrap_or_default();
        if (!candidates.is_empty() || !replayed.is_empty())
            && retention.is_none_or(|ceiling| !ceiling.permits_durable_semantic())
        {
            return Err(invalid("turn has no durable visual source closure"));
        }
        let mut uses = Vec::with_capacity(candidates.len().saturating_add(replayed.len()));
        for (intake, _) in candidates {
            let episode = intake.episode;
            let delivery = episode
                .delivery
                .as_ref()
                .ok_or_else(|| invalid("visual source lacks host delivery proof"))?;
            uses.push(TranscriptExternalContentUseV1::Visual {
                source: episode.source_ref()?,
                input_id: episode.input_id,
                packet_id: delivery.packet_id.clone(),
                request_sha256: delivery.final_request_sha256.clone(),
                receipt_digest: delivery.receipt_sha256.clone(),
                tool_observation_id: None,
            });
        }
        if !replayed.is_empty() {
            let request_sha256 = replay_request_sha256
                .filter(|digest| valid_sha256(digest))
                .ok_or_else(|| {
                    invalid("replayed visual use needs the current final request digest")
                })?;
            for use_ in replayed {
                uses.push(TranscriptExternalContentUseV1::ReplayedVisual {
                    source: use_.source,
                    owner: use_.owner,
                    request_sha256: request_sha256.to_string(),
                });
            }
        }
        uses.sort();
        Ok(uses)
    }

    /// Accepts the host's complete, ordered final-request inventory for a
    /// turn. Completeness remains host-attested; the SDK independently binds
    /// every known visual use to its admitted receipt or replay owner.
    pub fn submit_turn_external_content_manifest(
        &self,
        handle: &PerceptionHostHandle,
        turn_id: &str,
        manifest: TranscriptExternalContentManifestV1,
    ) -> Result<()> {
        self.ensure_visual_sdk_profile()?;
        self.verify_turn_external_content_manifest(turn_id, &manifest)?;
        let mut state = self.perception_state()?;
        state.validate_handle(handle)?;
        if state.external_content_manifests.len() >= MAX_TAINTED_TURNS
            && !state.external_content_manifests.contains_key(turn_id)
        {
            return Err(invalid(
                "turn external-content manifest capacity is exhausted",
            ));
        }
        if let Some((existing_handle, existing)) = state.external_content_manifests.get(turn_id) {
            if existing_handle.host_id != handle.host_id
                || existing_handle.device_id != handle.device_id
                || existing != &manifest
            {
                return Err(invalid("turn external-content manifest cannot be replaced"));
            }
            return Ok(());
        }
        state
            .external_content_manifests
            .insert(turn_id.to_string(), (handle.clone(), manifest));
        Ok(())
    }

    pub(crate) fn turn_external_content_manifest(
        &self,
        turn_id: &str,
    ) -> Result<Option<TranscriptExternalContentManifestV1>> {
        let stored = self
            .perception_state()?
            .external_content_manifests
            .get(turn_id)
            .cloned();
        let Some((handle, manifest)) = stored else {
            return Ok(None);
        };
        self.perception_state()?.validate_handle(&handle)?;
        self.verify_turn_external_content_manifest(turn_id, &manifest)?;
        Ok(Some(manifest))
    }

    pub(crate) fn clear_turn_external_content_manifest(&self, turn_id: &str) -> Result<()> {
        self.perception_state()?
            .external_content_manifests
            .remove(turn_id);
        Ok(())
    }

    pub(crate) fn ephemeral_replay_source_keys(
        &self,
        turn_id: &str,
    ) -> Result<Vec<PerceptionSourceKey>> {
        Ok(self
            .perception_state()?
            .replay_receipts
            .get(turn_id)
            .map_or_else(Vec::new, |receipt| receipt.ephemeral_source_keys.clone()))
    }
    pub(crate) fn record_beetle_visual_observation(
        &self,
        input_id: &str,
        inference_response_body: &[u8],
        request_receipt_digest: &str,
        observation: VisualObservation,
        retain_media: bool,
    ) -> Result<()> {
        self.ensure_visual_sdk_profile()?;
        self.perception_state()?.record_beetle_observation(
            input_id,
            inference_response_body,
            request_receipt_digest,
            observation,
            retain_media,
            self.config.clock.now_secs(),
        )
    }
    pub fn authorize_visual_inference(
        &self,
        handle: &PerceptionHostHandle,
        input_id: &str,
        authorization: VisualInferenceAuthorization,
    ) -> Result<()> {
        self.ensure_visual_sdk_profile()?;
        self.perception_state()?.authorize_inference(
            handle,
            input_id,
            authorization,
            self.config.clock.now_secs(),
        )
    }

    pub(crate) fn visual_inference_input(
        &self,
        input_id: &str,
    ) -> Result<VerifiedVisualInferenceInput> {
        self.ensure_visual_sdk_profile()?;
        let input = self
            .perception_state()?
            .inference_input(input_id, self.config.clock.now_secs())?;
        self.assert_visual_source_authorized(
            &input.scope,
            &input.source_key,
            input.source_kind,
            input.revoke_epoch,
            input.authorization_generation,
            input.retention,
        )?;
        Ok(input)
    }

    pub(crate) fn independent_visual_episode_intake(
        &self,
        input_id: &str,
    ) -> Result<PerceptionTurnEpisodeIntakeV1> {
        self.ensure_visual_sdk_profile()?;
        let intake = self
            .perception_state()?
            .independent_episode_intake(input_id, self.config.clock.now_secs())?;
        let episode = &intake.episode;
        self.assert_visual_source_authorized(
            &self.visual_authorization_scope(None),
            &episode.source,
            episode.source_kind,
            episode.revoke_epoch,
            episode.authorization_generation,
            episode.retention_ceiling,
        )?;
        Ok(intake)
    }

    pub(crate) fn mark_independent_visual_episode_persisted(&self, input_id: &str) -> Result<()> {
        self.perception_state()?
            .mark_independent_persisted(input_id)
    }

    pub(crate) fn has_verified_host_delivery(&self, input_id: &str) -> Result<bool> {
        valid_id(input_id, "input_id")?;
        let candidate = {
            let state = self.perception_state()?;
            state.tainted_turns.iter().find_map(|(turn_id, taints)| {
                taints
                    .iter()
                    .find(|taint| taint.input_id == input_id)
                    .map(|taint| (turn_id.clone(), taint.clone()))
            })
        };
        let Some((turn_id, taint)) = candidate else {
            return Ok(false);
        };
        if taint.revoked || taint.delivery.is_none() {
            return Ok(false);
        }
        let marker = self.visual_turn_persisted_taint(&turn_id)?;
        let Some(marker) = marker else {
            return Ok(false);
        };
        let commitment =
            perception_source_commitment(&taint.source_key, &taint.input_id, taint.revoke_epoch)?;
        Ok(marker.source_commitments.contains(&commitment))
    }
    pub fn register_perception_host(
        &self,
        host_id: impl Into<String>,
        device_id: Option<String>,
    ) -> Result<PerceptionHostHandle> {
        self.ensure_visual_sdk_profile()?;
        self.perception_state()?
            .register_host(host_id.into(), device_id)
    }

    pub fn register_visual_tool_snapshot(
        &self,
        handle: &PerceptionHostHandle,
        snapshot: VisualToolRegistrySnapshot,
    ) -> Result<()> {
        self.ensure_visual_sdk_profile()?;
        self.perception_state()?.set_tool_registry(handle, snapshot)
    }

    pub fn submit_visual_input(
        &self,
        handle: &PerceptionHostHandle,
        input: PerceptionInputEnvelope<VisualInput>,
        retention: PerceptionRetentionCeiling,
        position: VisualTurnPosition,
        capture_context: Option<VisualCaptureContext>,
    ) -> Result<VisualSubmissionReport> {
        self.ensure_visual_sdk_profile()?;
        if input.scope.memory_space_id != self.config.memory_space_id
            || input.scope.subject_id != self.config.subject_id
            || input.scope.relationship_id.as_deref()
                != self
                    .config
                    .scoped_runtime
                    .relationship_scope
                    .as_ref()
                    .map(|relationship| relationship.relationship_id.as_str())
            || input.scope.session_id != self.config.scope.conversation_id_or_chat_id()
        {
            return Err(invalid(
                "visual input scope differs from the mounted Runtime",
            ));
        }
        if input.source.kind == PerceptionSourceKind::HumanUserSharedMedia
            && input.scope.turn_id.is_none()
        {
            return Err(invalid("user-shared media must be bound to its turn"));
        }
        let now_secs = self.config.clock.now_secs();
        let mark_scope = input.scope.clone();
        let mark_source = input.source.key.clone();
        let mark_kind = input.source.kind;
        let mark_input_id = input.input_id.clone();
        let mark_epoch = input.revoke_epoch;
        let mut state = self.perception_state()?;
        let authorization_generation = state
            .source_authorizations
            .get(&SourceIdentity::from_key(&mark_source))
            .map_or(0, |authorization| authorization.authorization_generation);
        self.assert_visual_source_authorized(
            &mark_scope,
            &mark_source,
            mark_kind,
            mark_epoch,
            authorization_generation,
            retention,
        )?;
        let mut trial = state.clone();
        let report = trial.submit_visual(
            handle,
            input,
            retention,
            position,
            capture_context,
            now_secs,
        )?;
        if mark_scope.turn_id.is_some() {
            self.mark_visual_turn_taint(
                &mark_scope,
                &mark_source,
                &mark_input_id,
                mark_epoch,
                retention,
                now_secs,
            )?;
        }
        *state = trial;
        Ok(report)
    }

    /// Returns only later revisions of the current turn. It does not mutate a
    /// previously delivered system text block.
    pub fn append_visual_delivery(
        &self,
        handle: &PerceptionHostHandle,
        turn_id: &str,
        since_revision: u64,
    ) -> Result<Option<ProviderVisualDeliveryPacket>> {
        self.ensure_visual_sdk_profile()?;
        valid_id(turn_id, "turn_id")?;
        self.prepare_visual_delivery_with_taint(handle, turn_id, since_revision)
    }

    pub fn attest_visual_delivery(
        &self,
        handle: &PerceptionHostHandle,
        receipt: HostVisualDeliveryReceipt,
    ) -> Result<VisualDeliveryProof> {
        self.ensure_visual_sdk_profile()?;
        let mut state = self.perception_state()?;
        for sent in &receipt.ordered_parts {
            let entry = state
                .active
                .iter()
                .find(|entry| {
                    entry.input_id == sent.input_id && entry.source_key == sent.source_key
                })
                .ok_or_else(|| invalid("visual receipt source has expired"))?;
            self.assert_visual_source_authorized(
                &entry.scope,
                &entry.source_key,
                entry.source_kind,
                entry.revoke_epoch,
                entry.authorization_generation,
                entry.retention,
            )?;
        }
        state.attest_host_send(handle, receipt, self.config.clock.now_secs())
    }

    pub fn record_visual_provider_response(
        &self,
        handle: &PerceptionHostHandle,
        packet_id: &str,
        final_request_sha256: &str,
        response_body: Vec<u8>,
    ) -> Result<VisualDeliveryProof> {
        self.ensure_visual_sdk_profile()?;
        let mut state = self.perception_state()?;
        let packet = state
            .packets
            .get(packet_id)
            .ok_or_else(|| invalid("visual provider packet is unavailable"))?;
        for part in &packet.packet.parts {
            let entry = state
                .active
                .iter()
                .find(|entry| {
                    entry.input_id == part.input_id && entry.source_key == part.source_key
                })
                .ok_or_else(|| invalid("visual provider source has expired"))?;
            self.assert_visual_source_authorized(
                &entry.scope,
                &entry.source_key,
                entry.source_kind,
                entry.revoke_epoch,
                entry.authorization_generation,
                entry.retention,
            )?;
        }
        state.mark_provider_response(handle, packet_id, final_request_sha256, response_body)
    }

    pub fn revoke_visual_source(
        &self,
        handle: &PerceptionHostHandle,
        source_id: &str,
    ) -> Result<u64> {
        self.ensure_visual_sdk_profile()?;
        let mut state = self.perception_state()?;
        let mut trial = state.clone();
        let epoch = trial.revoke_source(handle, source_id)?;
        let identity = SourceIdentity {
            host_id: handle.host_id.clone(),
            device_id: handle.device_id.clone(),
            source_id: source_id.to_string(),
        };
        if let Some(source) = trial
            .sources
            .get(&identity)
            .filter(|source| source.generation > 0)
        {
            let scope = self.visual_authorization_scope(None);
            let key = PerceptionSourceKey {
                host_id: identity.host_id,
                device_id: identity.device_id,
                source_id: identity.source_id,
                source_generation: source.generation,
            };
            self.config
                .store_platform
                .as_ref()
                .ok_or_else(|| invalid("visual revocation requires the Store"))?
                .advance_perception_source_fence(
                    &scope,
                    &key,
                    epoch,
                    self.visual_event_scope(),
                    format!(
                        "visual-revoke:sha256:{:x}",
                        Sha256::digest(
                            serde_json::to_vec(&(&key, epoch))
                                .map_err(|error| invalid(error.to_string()))?
                        )
                    ),
                    self.config.clock.now_secs(),
                )?;
        }
        *state = trial;
        Ok(epoch)
    }

    pub fn clear_perception_session(&self) -> Result<Vec<VisualSourceRevocation>> {
        self.ensure_visual_sdk_profile()?;
        let mut state = self.perception_state()?;
        let mut trial = state.clone();
        let revoked = trial.clear_session();
        self.persist_visual_revocations(&revoked)?;
        *state = trial;
        Ok(revoked)
    }

    /// Used only by a Store composite deletion that advances its own fences in
    /// the same transaction as episode and owner deletion.
    pub(crate) fn clear_perception_session_ram_only(&self) -> Result<Vec<VisualSourceRevocation>> {
        self.ensure_visual_sdk_profile()?;
        Ok(self.perception_state()?.clear_session())
    }

    /// Atomically invalidates all current visual media and source generations
    /// while applying the host registry's unavailable snapshots.
    pub fn revoke_all_current_visual_sources(
        &self,
        unavailable_capabilities: Vec<PerceptionCapabilitySnapshot>,
    ) -> Result<Vec<VisualSourceRevocation>> {
        self.ensure_visual_sdk_profile()?;
        let mut state = self.perception_state()?;
        let mut trial = state.clone();
        let revoked = trial.revoke_all_current_visual_sources(unavailable_capabilities)?;
        self.persist_visual_revocations(&revoked)?;
        *state = trial;
        Ok(revoked)
    }

    fn persist_visual_revocations(&self, revoked: &[VisualSourceRevocation]) -> Result<()> {
        let scope = self.visual_authorization_scope(None);
        let fences = revoked
            .iter()
            .filter(|entry| entry.prior_source_generation > 0)
            .map(|entry| {
                (
                    scope.clone(),
                    PerceptionSourceKey {
                        host_id: entry.host_id.clone(),
                        device_id: entry.device_id.clone(),
                        source_id: entry.source_id.clone(),
                        source_generation: entry.prior_source_generation,
                    },
                    entry.revoke_epoch,
                )
            })
            .collect::<Vec<_>>();
        if fences.is_empty() {
            return Ok(());
        }
        let transaction_id = format!(
            "visual-revoke-all:sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&fences).map_err(|error| invalid(error.to_string()))?
            )
        );
        self.config
            .store_platform
            .as_ref()
            .ok_or_else(|| invalid("visual revocation requires the Store"))?
            .advance_perception_source_fences(
                &fences,
                self.visual_event_scope(),
                transaction_id,
                self.config.clock.now_secs(),
            )?;
        Ok(())
    }

    /// Current typed sensory views for a self-runtime operation. Media bytes
    /// remain in the separately governed delivery packet, never in a text field.
    pub fn current_visual_views(&self) -> Result<Vec<VisualView>> {
        self.ensure_visual_sdk_profile()?;
        Ok(self
            .perception_state()?
            .visual_views(self.config.clock.now_secs()))
    }

    pub(crate) fn remember_ephemeral_visual_turn(
        &self,
        turn: &CanonicalTurnDelta,
        inherited_source_keys: &[PerceptionSourceKey],
    ) -> Result<()> {
        self.ensure_visual_sdk_profile()?;
        let now_secs = self.config.clock.now_secs();
        let mut messages =
            Vec::with_capacity(turn.input_messages.len() + turn.assistant_messages.len());
        for (index, input) in turn.input_messages.iter().enumerate() {
            messages.push(SessionMessage::new(
                format!("visual-ephemeral:{}:input:{index}", turn.turn_id),
                input.role.clone(),
                input.content.clone(),
                input.observed_at.max(now_secs),
                now_secs,
                input.speaker_id.clone(),
                input.speaker_kind.clone(),
            ));
        }
        for (index, assistant) in turn.assistant_messages.iter().enumerate() {
            let message = &assistant.message;
            messages.push(SessionMessage::new(
                format!("visual-ephemeral:{}:assistant:{index}", turn.turn_id),
                message.role.clone(),
                message.content.clone(),
                message.observed_at.max(now_secs),
                now_secs,
                message.speaker_id.clone(),
                message.speaker_kind.clone(),
            ));
        }
        self.perception_state()?.remember_ephemeral_turn(
            &turn.turn_id,
            messages,
            inherited_source_keys,
            now_secs,
        )
    }

    pub(crate) fn ephemeral_visual_context(
        &self,
    ) -> Result<(Vec<SessionMessage>, Vec<PerceptionSourceKey>)> {
        if !self.visual_sdk_profile_supported() {
            return Ok((Vec::new(), Vec::new()));
        }
        Ok(self
            .perception_state()?
            .ephemeral_context(self.config.clock.now_secs()))
    }

    pub(crate) fn current_perception_capabilities(
        &self,
        active_host: Option<&PerceptionHostHandle>,
    ) -> Result<Vec<PerceptionCapabilitySnapshot>> {
        let state = self.perception_state()?;
        if let Some(handle) = active_host {
            state.validate_handle(handle)?;
        }
        Ok(state
            .capabilities()
            .into_iter()
            .map(|mut capability| {
                let local = active_host.is_some_and(|handle| {
                    handle.host_id == capability.host_id
                        && handle.device_id.as_deref() == Some(capability.device_id.as_str())
                });
                if !local && capability.state == PerceptionCapabilityState::Available {
                    capability.state = PerceptionCapabilityState::PresentUnavailable;
                    capability.unavailable_reason =
                        Some(bm_core::memory::PerceptionUnavailableReason::ToolUnavailable);
                }
                capability
            })
            .collect())
    }

    pub(crate) fn prepare_visual_for_project(
        &self,
        binding: &bm_core::memory::ProceduralProjectionBindingV1,
        handle: Option<&PerceptionHostHandle>,
    ) -> Result<Option<ProviderVisualDeliveryPacket>> {
        let bm_core::memory::ProceduralProjectionBindingV1::Turn { turn_id } = binding else {
            return Ok(None);
        };
        if !self.visual_sdk_profile_supported() || handle.is_none() {
            return Ok(None);
        }
        self.prepare_visual_delivery_with_taint(handle.expect("checked"), turn_id, 0)
    }

    pub(crate) fn visual_turn_sources(
        &self,
        turn_id: &str,
    ) -> Result<
        Vec<(
            PerceptionSourceKey,
            PerceptionRetentionCeiling,
            Option<String>,
        )>,
    > {
        Ok(self
            .perception_state()?
            .turn_sources(turn_id, self.config.clock.now_secs()))
    }

    pub(crate) fn visual_turn_episode_candidates(
        &self,
        turn_id: &str,
    ) -> Result<VisualTurnEpisodeCandidates> {
        let marker = self.visual_turn_persisted_taint(turn_id)?;
        let replay = self.visual_replay_receipt(turn_id)?;
        let (ram_retention, episodes) =
            self.perception_state()?.turn_episode_candidates(turn_id)?;
        for (intake, _) in &episodes {
            let scope = PerceptionScope {
                memory_space_id: intake.episode.memory_space_id.clone(),
                subject_id: intake.episode.subject_id.clone(),
                relationship_id: intake.episode.relationship_id.clone(),
                session_id: self.config.scope.conversation_id_or_chat_id().to_string(),
                turn_id: Some(turn_id.to_string()),
                step_id: None,
            };
            self.assert_visual_source_authorized(
                &scope,
                &intake.episode.source,
                intake.episode.source_kind,
                intake.episode.revoke_epoch,
                intake.episode.authorization_generation,
                intake.episode.retention_ceiling,
            )?;
        }
        let Some(marker) = marker else {
            if ram_retention.is_some()
                || replay.as_ref().is_some_and(|(uses, _, _)| !uses.is_empty())
            {
                return Err(invalid("visual turn lacks its durable taint marker"));
            }
            return Ok((
                replay.and_then(|(_, ephemeral, unresolved)| {
                    (ephemeral || unresolved).then_some(PerceptionRetentionCeiling::CurrentTurn)
                }),
                Vec::new(),
            ));
        };
        let mut commitments = self
            .perception_state()?
            .tainted_turns
            .get(turn_id)
            .into_iter()
            .flatten()
            .map(|taint| {
                perception_source_commitment(&taint.source_key, &taint.input_id, taint.revoke_epoch)
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some((uses, _, _)) = &replay {
            commitments.extend(
                uses.iter()
                    .map(|use_| {
                        perception_source_commitment(
                            &use_.source.source,
                            &use_.source.episode_id,
                            use_.source.revoke_epoch,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        commitments.sort();
        commitments.dedup();
        if commitments != marker.source_commitments {
            return Ok((Some(PerceptionRetentionCeiling::CurrentTurn), Vec::new()));
        }
        if replay
            .as_ref()
            .is_some_and(|(_, ephemeral, unresolved)| *ephemeral || *unresolved)
            || ram_retention == Some(PerceptionRetentionCeiling::CurrentTurn)
        {
            return Ok((Some(PerceptionRetentionCeiling::CurrentTurn), Vec::new()));
        }
        let replay_retention = replay.as_ref().and_then(|(uses, _, _)| {
            (!uses.is_empty()).then_some(PerceptionRetentionCeiling::DurableSemantic)
        });
        let effective = match (ram_retention, replay_retention) {
            (Some(left), Some(right)) => left.min(right),
            (Some(left), None) => left,
            (None, Some(right)) => right,
            (None, None) => return Ok((Some(PerceptionRetentionCeiling::CurrentTurn), Vec::new())),
        };
        if marker.retention_ceiling != effective
            && !(effective == PerceptionRetentionCeiling::CurrentTurn && episodes.is_empty())
        {
            return Err(invalid("visual turn retention differs from durable taint"));
        }
        Ok((Some(effective), episodes))
    }

    pub(crate) fn visual_turn_persisted_taint(
        &self,
        turn_id: &str,
    ) -> Result<Option<PerceptionTurnTaintV1>> {
        let platform = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| invalid("visual taint verification requires the Store"))?;
        platform.read_perception_turn_taint(
            &self.config.memory_space_id,
            &self.config.subject_id,
            self.config.scope.conversation_id_or_chat_id(),
            turn_id,
        )
    }

    fn perception_state(&self) -> Result<std::sync::MutexGuard<'_, RuntimePerceptionState>> {
        self.perception
            .lock()
            .map_err(|_| Error::config("perception_runtime", "perception state lock is poisoned"))
    }

    fn prepare_visual_delivery_with_taint(
        &self,
        handle: &PerceptionHostHandle,
        turn_id: &str,
        since_revision: u64,
    ) -> Result<Option<ProviderVisualDeliveryPacket>> {
        let now_secs = self.config.clock.now_secs();
        let mut state = self.perception_state()?;
        state.validate_handle(handle)?;
        let mut trial = state.clone();
        let packet = trial.prepare_turn(
            self.config.scope.conversation_id_or_chat_id(),
            turn_id,
            since_revision,
            now_secs,
        )?;
        if packet.as_ref().is_some_and(|packet| {
            packet.parts.iter().any(|part| {
                part.source_key.host_id != handle.host_id
                    || part.source_key.device_id != handle.device_id
            })
        }) {
            return Err(invalid("visual packet belongs to a different host device"));
        }
        if let Some(packet) = &packet {
            for part in &packet.parts {
                let entry = trial
                    .active
                    .iter()
                    .find(|entry| {
                        entry.input_id == part.input_id && entry.source_key == part.source_key
                    })
                    .ok_or_else(|| invalid("visual delivery source expired"))?;
                self.assert_visual_source_authorized(
                    &entry.scope,
                    &entry.source_key,
                    entry.source_kind,
                    entry.revoke_epoch,
                    entry.authorization_generation,
                    entry.retention,
                )?;
            }
        }
        if let Some(packet) = &packet {
            if let Some(taints) = trial.tainted_turns.get(turn_id) {
                for taint in taints {
                    if packet
                        .parts
                        .iter()
                        .any(|part| part.input_id == taint.input_id)
                    {
                        let mut scope = taint.scope.clone();
                        scope.turn_id = Some(turn_id.to_string());
                        self.mark_visual_turn_taint(
                            &scope,
                            &taint.source_key,
                            &taint.input_id,
                            taint.revoke_epoch,
                            taint.retention,
                            now_secs,
                        )?;
                    }
                }
            }
        }
        *state = trial;
        Ok(packet)
    }

    fn mark_visual_turn_taint(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
        input_id: &str,
        revoke_epoch: u64,
        retention: PerceptionRetentionCeiling,
        now_secs: u64,
    ) -> Result<()> {
        let platform = self
            .config
            .store_platform
            .as_ref()
            .ok_or_else(|| invalid("visual turn taint requires the governed Store"))?;
        platform.mark_perception_turn_taint(
            scope,
            PerceptionTurnTaintInput {
                source,
                input_id,
                revoke_epoch,
                retention_ceiling: retention,
            },
            StoreEventScope::new(
                self.config.identity.agent_id.clone(),
                self.config.identity.owner_id.clone(),
                self.config.scope.channel.clone(),
                self.config.scope.chat_id.clone(),
            )
            .with_memory_space(self.config.memory_space_id.clone())
            .with_subject(self.config.subject_id.clone())
            .with_conversation(self.config.scope.conversation_id_or_chat_id().to_string()),
            now_secs,
        )?;
        Ok(())
    }

    fn ensure_visual_sdk_profile(&self) -> Result<()> {
        if self.visual_sdk_profile_supported() {
            Ok(())
        } else {
            Err(invalid(
                "visual perception is unavailable for this SDK profile",
            ))
        }
    }

    pub(crate) fn visual_sdk_profile_supported(&self) -> bool {
        matches!(
            self.config.profile,
            ProfileId::DesktopMacosStandaloneMemory
                | ProfileId::DesktopMacosEmbeddedSdk
                | ProfileId::DesktopMacosDevFull
        )
    }
}

fn packet_digest(turn_id: &str, revision: u64, parts: &[ProviderVisualPart]) -> String {
    let mut hasher = Sha256::new();
    hash_framed(&mut hasher, turn_id.as_bytes());
    hasher.update(revision.to_be_bytes());
    hasher.update((parts.len() as u64).to_be_bytes());
    for part in parts {
        hash_framed(&mut hasher, part.input_id.as_bytes());
        hash_framed(&mut hasher, part.source_key.host_id.as_bytes());
        hash_framed(
            &mut hasher,
            part.source_key
                .device_id
                .as_deref()
                .unwrap_or("")
                .as_bytes(),
        );
        hash_framed(&mut hasher, part.source_key.source_id.as_bytes());
        hasher.update(part.source_key.source_generation.to_be_bytes());
        hasher.update([match part.source_kind {
            PerceptionSourceKind::HostVisualObservation => 1,
            PerceptionSourceKind::HumanUserSharedMedia => 2,
            PerceptionSourceKind::HostStructuredObservation => 3,
        }]);
        hasher.update((part.ordinal as u64).to_be_bytes());
        hash_framed(
            &mut hasher,
            part.step_id.as_deref().unwrap_or("").as_bytes(),
        );
        hash_framed(&mut hasher, part.mime.as_bytes());
        hash_framed(&mut hasher, part.sha256.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn hash_framed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_id(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 160
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(invalid(format!("{field} is invalid")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> Error {
    Error::invalid_input("perception_runtime", message)
}
