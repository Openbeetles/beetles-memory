//! 感官输入合同。媒体字节只属于类型化载荷，不能进入通用信封或世界状态文本。

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

const MAX_ID_LEN: usize = 160;
const MAX_MEDIA_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAME_COUNT: usize = 64;
const MAX_DERIVATION_PARENTS: usize = 64;
pub const MAX_WORLD_PERCEPTION_VIEWS: usize = 64;
pub const PERCEPTION_SOURCE_DEPENDENTS_SCHEMA_V1: u16 = 1;
pub const MAX_PERCEPTION_SOURCE_DEPENDENT_EDGES_V1: usize = 256;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PerceptionModality {
    Visual,
    Structured,
    Audio,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PerceptionSourceKey {
    pub host_id: String,
    /// A user-shared attachment can have no capture device.
    pub device_id: Option<String>,
    pub source_id: String,
    pub source_generation: u64,
}

impl PerceptionSourceKey {
    pub fn validate_contract(&self) -> Result<()> {
        valid_id(&self.host_id, "host_id")?;
        if let Some(device_id) = &self.device_id {
            valid_id(device_id, "device_id")?;
        }
        valid_id(&self.source_id, "source_id")?;
        if self.source_generation == 0 {
            return Err(invalid("source_generation must be positive"));
        }
        Ok(())
    }
}

/// Durable reference shared by transcript, governed memory and Soul lineage.
/// The referenced episode is the owner; this value never embeds media bytes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PerceptionEpisodeSourceRef {
    pub memory_space_id: String,
    pub subject_id: String,
    pub relationship_id: Option<String>,
    pub source: PerceptionSourceKey,
    pub revoke_epoch: u64,
    pub episode_id: String,
    pub content_digest: String,
}

impl PerceptionEpisodeSourceRef {
    pub fn validate_contract(&self) -> Result<()> {
        valid_id(&self.memory_space_id, "memory_space_id")?;
        valid_id(&self.subject_id, "subject_id")?;
        if let Some(relationship_id) = &self.relationship_id {
            valid_id(relationship_id, "relationship_id")?;
        }
        self.source.validate_contract()?;
        valid_id(&self.episode_id, "episode_id")?;
        if !valid_sha256(&self.content_digest) {
            return Err(invalid("perception episode content digest is invalid"));
        }
        Ok(())
    }
}

/// Seals the durable episode's semantic evidence while leaving its optional
/// retained media outside source identity. The caller must validate the typed
/// episode first; the media owner and bytes have separate exact SHA/root edges.
/// A semantic-only archive can therefore omit media without rewriting every
/// Transcript, memory and Soul source reference.
pub fn perception_episode_semantic_digest<T: Serialize>(episode: &T) -> Result<String> {
    let mut value = serde_json::to_value(episode)
        .map_err(|error| invalid(format!("cannot encode perception episode: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| invalid("perception episode must be a JSON object"))?;
    let media = object
        .get_mut("media")
        .ok_or_else(|| invalid("perception episode lacks its optional media field"))?;
    if !media.is_null() && !media.is_object() {
        return Err(invalid("perception episode media owner has invalid shape"));
    }
    *media = serde_json::Value::Null;
    let canonical = serde_json::to_vec(&value)
        .map_err(|error| invalid(format!("cannot seal perception episode: {error}")))?;
    let mut hash = Sha256::new();
    hash_text(&mut hash, "beetle_memory_perception_episode_semantic_v1");
    hash.update((canonical.len() as u64).to_be_bytes());
    hash.update(canonical);
    Ok(format!("sha256:{:x}", hash.finalize()))
}

/// Every durable object that can carry material derived from one perception episode.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PerceptionSourceDependentKindV1 {
    TranscriptTurn,
    LongTermOwnerRevision,
    SoulRoot,
    SoulRevision,
    DerivedSummary,
    WorldSense,
    MediaEvidence,
    EmbeddingIndex,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PerceptionSourceDependentEdgeV1 {
    pub kind: PerceptionSourceDependentKindV1,
    pub memory_space_id: String,
    pub subject_id: String,
    /// None is allowed for a global Soul owner derived from a relationship-local episode.
    pub relationship_id: Option<String>,
    pub owner_key: String,
    pub owner_revision: Option<u64>,
    pub content_digest: String,
}

impl PerceptionSourceDependentEdgeV1 {
    pub fn validate_for(&self, source: &PerceptionEpisodeSourceRef) -> Result<()> {
        valid_id(&self.memory_space_id, "dependent memory_space_id")?;
        valid_id(&self.subject_id, "dependent subject_id")?;
        if let Some(relationship_id) = &self.relationship_id {
            valid_id(relationship_id, "dependent relationship_id")?;
            if Some(relationship_id) != source.relationship_id.as_ref() {
                return Err(invalid("dependent relationship escapes source scope"));
            }
        }
        if self.memory_space_id != source.memory_space_id || self.subject_id != source.subject_id {
            return Err(invalid("dependent owner escapes source scope"));
        }
        if self.owner_key.is_empty()
            || self.owner_key.len() > 192
            || !valid_dependent_owner_key(&self.owner_key)
            || self.owner_revision == Some(0)
            || !valid_sha256(&self.content_digest)
        {
            return Err(invalid(
                "dependent owner key, revision or digest is invalid",
            ));
        }
        Ok(())
    }
}

/// The Store owns this root transactionally with its episode. Core only validates
/// its canonical shape and digest; no dependent content or media lives here.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerceptionSourceDependentsRootV1 {
    pub schema_version: u16,
    pub source: PerceptionEpisodeSourceRef,
    pub edges: Vec<PerceptionSourceDependentEdgeV1>,
    pub closure_digest: String,
}

impl PerceptionSourceDependentsRootV1 {
    pub fn new(
        source: PerceptionEpisodeSourceRef,
        mut edges: Vec<PerceptionSourceDependentEdgeV1>,
    ) -> Result<Self> {
        source.validate_contract()?;
        edges.sort();
        let mut root = Self {
            schema_version: PERCEPTION_SOURCE_DEPENDENTS_SCHEMA_V1,
            source,
            edges,
            closure_digest: String::new(),
        };
        root.validate_edges()?;
        root.closure_digest = root.compute_digest();
        Ok(root)
    }

    pub fn validate_contract(&self) -> Result<()> {
        if self.schema_version != PERCEPTION_SOURCE_DEPENDENTS_SCHEMA_V1 {
            return Err(invalid("perception dependents schema version is invalid"));
        }
        self.source.validate_contract()?;
        self.validate_edges()?;
        if !valid_sha256(&self.closure_digest) || self.closure_digest != self.compute_digest() {
            return Err(invalid("perception dependent closure digest is invalid"));
        }
        Ok(())
    }

    fn validate_edges(&self) -> Result<()> {
        if self.edges.len() > MAX_PERCEPTION_SOURCE_DEPENDENT_EDGES_V1 {
            return Err(invalid("perception dependent edge budget exceeded"));
        }
        for edge in &self.edges {
            edge.validate_for(&self.source)?;
        }
        if self.edges.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invalid(
                "perception dependent edges are not unique and sorted",
            ));
        }
        Ok(())
    }

    fn compute_digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"perception-source-dependents-v1\0");
        hash.update(self.schema_version.to_be_bytes());
        hash_text(&mut hash, &self.source.memory_space_id);
        hash_text(&mut hash, &self.source.subject_id);
        hash_optional_text(&mut hash, self.source.relationship_id.as_deref());
        hash_text(&mut hash, &self.source.source.host_id);
        hash_optional_text(&mut hash, self.source.source.device_id.as_deref());
        hash_text(&mut hash, &self.source.source.source_id);
        hash.update(self.source.source.source_generation.to_be_bytes());
        hash.update(self.source.revoke_epoch.to_be_bytes());
        hash_text(&mut hash, &self.source.episode_id);
        hash_text(&mut hash, &self.source.content_digest);
        hash.update((self.edges.len() as u32).to_be_bytes());
        for edge in &self.edges {
            hash.update([edge.kind.digest_tag()]);
            hash_text(&mut hash, &edge.memory_space_id);
            hash_text(&mut hash, &edge.subject_id);
            hash_optional_text(&mut hash, edge.relationship_id.as_deref());
            hash_text(&mut hash, &edge.owner_key);
            match edge.owner_revision {
                Some(revision) => {
                    hash.update([1]);
                    hash.update(revision.to_be_bytes());
                }
                None => hash.update([0]),
            }
            hash_text(&mut hash, &edge.content_digest);
        }
        format!("sha256:{:x}", hash.finalize())
    }
}

impl PerceptionSourceDependentKindV1 {
    fn digest_tag(self) -> u8 {
        match self {
            Self::TranscriptTurn => 1,
            Self::LongTermOwnerRevision => 2,
            Self::SoulRoot => 3,
            Self::SoulRevision => 4,
            Self::DerivedSummary => 5,
            Self::WorldSense => 6,
            Self::MediaEvidence => 7,
            Self::EmbeddingIndex => 8,
        }
    }
}

fn hash_text(hash: &mut Sha256, text: &str) {
    hash.update((text.len() as u32).to_be_bytes());
    hash.update(text.as_bytes());
}

fn hash_optional_text(hash: &mut Sha256, text: Option<&str>) {
    match text {
        Some(text) => {
            hash.update([1]);
            hash_text(hash, text);
        }
        None => hash.update([0]),
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PerceptionSourceKind {
    HostVisualObservation,
    HumanUserSharedMedia,
    HostStructuredObservation,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PerceptionSourceRef {
    Sensor {
        sensor_id: String,
    },
    UserAttachment {
        message_id: String,
        attachment_id: String,
    },
    Structured {
        reference_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerceptionSource {
    pub key: PerceptionSourceKey,
    pub kind: PerceptionSourceKind,
    pub source_ref: PerceptionSourceRef,
}

impl PerceptionSource {
    pub fn validate_contract(&self) -> Result<()> {
        self.key.validate_contract()?;
        match (&self.kind, &self.source_ref) {
            (
                PerceptionSourceKind::HostVisualObservation,
                PerceptionSourceRef::Sensor { sensor_id },
            ) => {
                if self.key.device_id.is_none() {
                    return Err(invalid("host visual observation requires a device_id"));
                }
                valid_id(sensor_id, "sensor_id")
            }
            (
                PerceptionSourceKind::HumanUserSharedMedia,
                PerceptionSourceRef::UserAttachment {
                    message_id,
                    attachment_id,
                },
            ) => {
                valid_id(message_id, "message_id")?;
                valid_id(attachment_id, "attachment_id")
            }
            (
                PerceptionSourceKind::HostStructuredObservation,
                PerceptionSourceRef::Structured { reference_id },
            ) => valid_id(reference_id, "reference_id"),
            _ => Err(invalid("source kind and source ref do not match")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerceptionScope {
    pub memory_space_id: String,
    pub subject_id: String,
    pub relationship_id: Option<String>,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub step_id: Option<String>,
}

impl PerceptionScope {
    pub fn validate_contract(&self) -> Result<()> {
        valid_id(&self.memory_space_id, "memory_space_id")?;
        valid_id(&self.subject_id, "subject_id")?;
        valid_id(&self.session_id, "session_id")?;
        for (name, id) in [
            ("relationship_id", &self.relationship_id),
            ("turn_id", &self.turn_id),
            ("step_id", &self.step_id),
        ] {
            if let Some(id) = id {
                valid_id(id, name)?;
            }
        }
        if self.step_id.is_some() && self.turn_id.is_none() {
            return Err(invalid("step_id requires turn_id"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerceptionTime {
    pub observed_at_unix_ms: Option<u64>,
    pub presented_at_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerceptionInputEnvelope<T> {
    pub input_id: String,
    pub modality: PerceptionModality,
    pub source: PerceptionSource,
    pub scope: PerceptionScope,
    pub time: PerceptionTime,
    pub revoke_epoch: u64,
    pub payload: T,
}

impl<T> PerceptionInputEnvelope<T> {
    pub fn validate_common(&self) -> Result<()> {
        valid_id(&self.input_id, "input_id")?;
        self.source.validate_contract()?;
        self.scope.validate_contract()?;
        if self.time.presented_at_unix_ms == 0 {
            return Err(invalid("presented_at_unix_ms must be positive"));
        }
        if self
            .time
            .observed_at_unix_ms
            .is_some_and(|at| at > self.time.presented_at_unix_ms)
        {
            return Err(invalid("observed time cannot follow presented time"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PerceptionCapabilityState {
    Absent,
    PresentUnavailable,
    Available,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PerceptionUnavailableReason {
    UserDisabled,
    SystemPermissionRevoked,
    DeviceOffline,
    ToolUnavailable,
}

/// Maximum permitted use, not an instruction to persist every admitted input.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PerceptionRetentionCeiling {
    CurrentTurn,
    EphemeralSession,
    DurableSemantic,
    DurableMedia,
}

impl PerceptionRetentionCeiling {
    pub fn permits_durable_semantic(self) -> bool {
        self >= Self::DurableSemantic
    }

    pub fn permits_durable_media(self) -> bool {
        self >= Self::DurableMedia
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SubjectAttribution {
    VerifiedCurrentUser,
    ContextualCurrentUser,
    Unknown,
    ThirdParty,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VisualSensitivity {
    Ordinary,
    Private,
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AttributionConfidence {
    Low,
    Moderate,
    High,
}

/// Host-attested circumstances. This is evidence about a capture, not a claim
/// that a particular face is the user.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VisualDeviceUse {
    PersonalSingleUser,
    SharedOrUnknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualCaptureContext {
    pub source_key: PerceptionSourceKey,
    pub device_use: VisualDeviceUse,
    pub authenticated_user_session: bool,
    pub active_interaction_turn_id: Option<String>,
    pub interaction_at_unix_ms: Option<u64>,
    pub captured_at_unix_ms: u64,
    pub host_context_digest: String,
}

impl VisualCaptureContext {
    pub fn new(
        source_key: PerceptionSourceKey,
        device_use: VisualDeviceUse,
        authenticated_user_session: bool,
        active_interaction_turn_id: Option<String>,
        interaction_at_unix_ms: Option<u64>,
        captured_at_unix_ms: u64,
    ) -> Result<Self> {
        let mut value = Self {
            source_key,
            device_use,
            authenticated_user_session,
            active_interaction_turn_id,
            interaction_at_unix_ms,
            captured_at_unix_ms,
            host_context_digest: String::new(),
        };
        value.validate_shape()?;
        value.host_context_digest = value.compute_digest();
        Ok(value)
    }

    pub fn validate_contract(&self) -> Result<()> {
        self.validate_shape()?;
        if self.host_context_digest != self.compute_digest() {
            return Err(invalid("visual capture context digest is invalid"));
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<()> {
        self.source_key.validate_contract()?;
        if self.source_key.device_id.is_none() || self.captured_at_unix_ms == 0 {
            return Err(invalid("visual capture requires a device and capture time"));
        }
        if self.active_interaction_turn_id.is_some() != self.interaction_at_unix_ms.is_some() {
            return Err(invalid(
                "interaction turn and time must be supplied together",
            ));
        }
        if let Some(turn_id) = &self.active_interaction_turn_id {
            valid_id(turn_id, "active_interaction_turn_id")?;
        }
        Ok(())
    }

    fn compute_digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"visual-capture-context-v1\0");
        hash_text(&mut hash, &self.source_key.host_id);
        hash_optional_text(&mut hash, self.source_key.device_id.as_deref());
        hash_text(&mut hash, &self.source_key.source_id);
        hash.update(self.source_key.source_generation.to_be_bytes());
        hash.update([match self.device_use {
            VisualDeviceUse::PersonalSingleUser => 1,
            VisualDeviceUse::SharedOrUnknown => 2,
        }]);
        hash.update([u8::from(self.authenticated_user_session)]);
        hash_optional_text(&mut hash, self.active_interaction_turn_id.as_deref());
        match self.interaction_at_unix_ms {
            Some(at) => {
                hash.update([1]);
                hash.update(at.to_be_bytes());
            }
            None => hash.update([0]),
        }
        hash.update(self.captured_at_unix_ms.to_be_bytes());
        format!("sha256:{:x}", hash.finalize())
    }
}

/// Produced by Beetle's governed visual inference, never by a host-supplied
/// free-form identity label.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualSceneAttributionEvidence {
    pub input_id: String,
    pub source_media_sha256: Vec<String>,
    pub visible_person_count: u8,
    pub third_party_or_visitor_present: bool,
    pub conflicting_identity_evidence: bool,
    pub inference_digest: String,
}

impl VisualSceneAttributionEvidence {
    pub fn validate_contract(&self) -> Result<()> {
        valid_id(&self.input_id, "input_id")?;
        if self.source_media_sha256.is_empty()
            || self.source_media_sha256.len() > MAX_FRAME_COUNT
            || !self
                .source_media_sha256
                .iter()
                .all(|digest| valid_sha256(digest))
            || !valid_sha256(&self.inference_digest)
        {
            return Err(invalid("visual scene attribution evidence is invalid"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualAttributionDecision {
    pub subject_attribution: SubjectAttribution,
    pub confidence: AttributionConfidence,
    pub evidence_refs: Vec<String>,
}

/// Metadata retained after a visual payload has been admitted and its bytes
/// transferred to the SDK's bounded media owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualAttributionInput {
    pub input_id: String,
    pub source_kind: PerceptionSourceKind,
    pub source_key: PerceptionSourceKey,
    pub scope: PerceptionScope,
    pub time: PerceptionTime,
    pub media_sha256: Vec<String>,
}

impl VisualAttributionInput {
    pub fn from_envelope(input: &PerceptionInputEnvelope<VisualInput>) -> Result<Self> {
        input.validate_contract()?;
        Ok(Self {
            input_id: input.input_id.clone(),
            source_kind: input.source.kind,
            source_key: input.source.key.clone(),
            scope: input.scope.clone(),
            time: input.time,
            media_sha256: input
                .payload
                .media_parts()
                .iter()
                .map(|part| part.sha256.to_owned())
                .collect(),
        })
    }

    pub fn validate_contract(&self) -> Result<()> {
        valid_id(&self.input_id, "input_id")?;
        self.source_key.validate_contract()?;
        self.scope.validate_contract()?;
        if self.time.presented_at_unix_ms == 0
            || self.media_sha256.is_empty()
            || self.media_sha256.len() > MAX_FRAME_COUNT
            || !self.media_sha256.iter().all(|digest| valid_sha256(digest))
        {
            return Err(invalid("visual attribution input metadata is invalid"));
        }
        Ok(())
    }
}

/// Context alone never identifies a person. Beetle may infer the current user
/// only when host capture circumstances and model-observed scene agree.
pub fn derive_contextual_user_attribution(
    input: &VisualAttributionInput,
    context: &VisualCaptureContext,
    scene: &VisualSceneAttributionEvidence,
) -> Result<VisualAttributionDecision> {
    input.validate_contract()?;
    context.validate_contract()?;
    scene.validate_contract()?;
    if context.source_key != input.source_key
        || scene.input_id != input.input_id
        || scene.source_media_sha256 != input.media_sha256
    {
        return Err(invalid("capture context or scene does not bind the input"));
    }
    let evidence_refs = vec![
        context.host_context_digest.clone(),
        scene.inference_digest.clone(),
    ];
    let supported = input.source_kind == PerceptionSourceKind::HostVisualObservation
        && input.time.observed_at_unix_ms.is_some_and(|observed| {
            observed.abs_diff(context.captured_at_unix_ms) <= 5_000
                && context
                    .interaction_at_unix_ms
                    .is_some_and(|interaction| observed.abs_diff(interaction) <= 30_000)
        })
        && context.device_use == VisualDeviceUse::PersonalSingleUser
        && context.authenticated_user_session
        && context.active_interaction_turn_id.as_deref() == input.scope.turn_id.as_deref()
        && input.scope.turn_id.is_some()
        && scene.visible_person_count == 1
        && !scene.third_party_or_visitor_present
        && !scene.conflicting_identity_evidence;
    Ok(VisualAttributionDecision {
        subject_attribution: if supported {
            SubjectAttribution::ContextualCurrentUser
        } else {
            SubjectAttribution::Unknown
        },
        confidence: if supported {
            AttributionConfidence::High
        } else {
            AttributionConfidence::Low
        },
        evidence_refs,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualClassification {
    pub subject_attribution: SubjectAttribution,
    pub attribution_confidence: AttributionConfidence,
    /// None means the visual inference did not reliably count people.
    pub visible_person_count: Option<u8>,
    pub third_party_present: bool,
    pub sensitivity: VisualSensitivity,
    pub minor_present: bool,
    pub sensitive_document_present: bool,
    pub attribution_evidence_refs: Vec<String>,
    pub conflicting_evidence: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VisualObservationUncertainty {
    Low,
    Moderate,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VisualEvidenceOrigin {
    BeetleInference {
        provider_id: String,
        /// Attestation of the exact independent visual inference request and media list.
        request_receipt_digest: String,
        inference_digest: String,
    },
    /// This only makes a reply eligible for candidate governance; it is not a fact.
    HostAttestedReply {
        delivery_receipt_digest: String,
        reply_digest: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualObservation {
    pub input_id: String,
    pub source_key: PerceptionSourceKey,
    pub scope: PerceptionScope,
    pub revoke_epoch: u64,
    pub observed_at_unix_ms: Option<u64>,
    pub text: String,
    /// Canonical retrieval anchors emitted by Beetle's governed visual
    /// inference. A host description must not supply these terms.
    pub search_terms: Vec<String>,
    pub uncertainty: VisualObservationUncertainty,
    pub classification: VisualClassification,
    pub evidence_origin: VisualEvidenceOrigin,
}

impl VisualObservation {
    pub fn validate_contract(&self) -> Result<()> {
        valid_id(&self.input_id, "input_id")?;
        self.source_key.validate_contract()?;
        self.scope.validate_contract()?;
        self.classification.validate_contract()?;
        if self.text.trim().is_empty()
            || self.text.len() > 2048
            || self.text.chars().any(char::is_control)
        {
            return Err(invalid("visual observation text is invalid"));
        }
        let mut seen_terms = std::collections::BTreeSet::new();
        if (matches!(
            self.evidence_origin,
            VisualEvidenceOrigin::BeetleInference { .. }
        ) && self.search_terms.is_empty())
            || (matches!(
                self.evidence_origin,
                VisualEvidenceOrigin::HostAttestedReply { .. }
            ) && !self.search_terms.is_empty())
            || self.search_terms.len() > super::MAX_LONG_TERM_MEMORY_KEYWORDS
            || self.search_terms.iter().any(|term| {
                term.len() < 2
                    || term.len() > super::MAX_LONG_TERM_MEMORY_KEYWORD_LEN
                    || term.trim() != term
                    || term.chars().any(char::is_control)
                    || crate::util::normalize_retrieval_text(term) != *term
                    || !seen_terms.insert(term)
            })
        {
            return Err(invalid(
                "visual observation search terms are not canonical and bounded",
            ));
        }
        match &self.evidence_origin {
            VisualEvidenceOrigin::BeetleInference {
                provider_id,
                request_receipt_digest,
                inference_digest,
            } => {
                valid_id(provider_id, "provider_id")?;
                if !valid_sha256(request_receipt_digest) || !valid_sha256(inference_digest) {
                    return Err(invalid(
                        "visual inference request or result digest is invalid",
                    ));
                }
            }
            VisualEvidenceOrigin::HostAttestedReply {
                delivery_receipt_digest,
                reply_digest,
            } => {
                if !valid_sha256(delivery_receipt_digest) || !valid_sha256(reply_digest) {
                    return Err(invalid("host reply evidence digest is invalid"));
                }
            }
        }
        Ok(())
    }
}

impl VisualClassification {
    pub fn validate_contract(&self) -> Result<()> {
        if (self.visible_person_count == Some(0) && self.third_party_present)
            || (self.visible_person_count == Some(0)
                && matches!(
                    self.subject_attribution,
                    SubjectAttribution::VerifiedCurrentUser
                        | SubjectAttribution::ContextualCurrentUser
                        | SubjectAttribution::ThirdParty
                ))
            || (self.subject_attribution == SubjectAttribution::ContextualCurrentUser
                && self.visible_person_count != Some(1))
        {
            return Err(invalid("visual person count contradicts attribution"));
        }
        if self.attribution_evidence_refs.len() > 16 {
            return Err(invalid("too many attribution evidence refs"));
        }
        for reference in &self.attribution_evidence_refs {
            valid_id(reference, "attribution_evidence_ref")?;
        }
        if matches!(
            self.subject_attribution,
            SubjectAttribution::VerifiedCurrentUser | SubjectAttribution::ContextualCurrentUser
        ) && (self.attribution_evidence_refs.is_empty() || self.conflicting_evidence)
        {
            return Err(invalid("user attribution needs unconflicted evidence"));
        }
        if self.subject_attribution == SubjectAttribution::ContextualCurrentUser
            && self.attribution_confidence == AttributionConfidence::Low
        {
            return Err(invalid("low confidence cannot attribute the current user"));
        }
        Ok(())
    }

    pub fn media_evidence_default_allowed(&self) -> bool {
        self.sensitivity == VisualSensitivity::Ordinary
            && self.visible_person_count == Some(1)
            && !self.third_party_present
            && !self.minor_present
            && !self.sensitive_document_present
            && !self.conflicting_evidence
            && matches!(
                self.subject_attribution,
                SubjectAttribution::VerifiedCurrentUser | SubjectAttribution::ContextualCurrentUser
            )
            && (self.subject_attribution != SubjectAttribution::ContextualCurrentUser
                || self.attribution_confidence == AttributionConfidence::High)
            && self.validate_contract().is_ok()
    }

    /// A directly shared ordinary scene may retain its media under a durable
    /// media ceiling even when no person appears. This never attributes the
    /// scene to the user and does not relax third-party or sensitive gates.
    pub fn media_evidence_allowed_for_source(&self, source_kind: PerceptionSourceKind) -> bool {
        self.media_evidence_default_allowed()
            || source_kind == PerceptionSourceKind::HumanUserSharedMedia
                && self.ordinary_nonhuman_semantic_allowed()
    }

    /// A user-shared picture of an ordinary place or object may produce a
    /// sourced semantic candidate without identifying any depicted person.
    pub fn ordinary_nonhuman_semantic_allowed(&self) -> bool {
        self.subject_attribution == SubjectAttribution::Unknown
            && self.visible_person_count == Some(0)
            && !self.third_party_present
            && self.sensitivity == VisualSensitivity::Ordinary
            && !self.minor_present
            && !self.sensitive_document_present
            && !self.conflicting_evidence
            && self.validate_contract().is_ok()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VisualCaptureForm {
    Image,
    FrameStream,
    Video,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerceptionCapabilitySnapshot {
    pub host_id: String,
    pub device_id: String,
    pub sensor_id: String,
    pub tool_id: String,
    pub tool_registry_revision: u64,
    pub authorization_generation: u64,
    pub state: PerceptionCapabilityState,
    pub unavailable_reason: Option<PerceptionUnavailableReason>,
    pub visual_forms: Vec<VisualCaptureForm>,
}

impl PerceptionCapabilitySnapshot {
    pub fn validate_contract(&self) -> Result<()> {
        for (name, id) in [
            ("host_id", &self.host_id),
            ("device_id", &self.device_id),
            ("sensor_id", &self.sensor_id),
            ("tool_id", &self.tool_id),
        ] {
            valid_id(id, name)?;
        }
        if self.tool_registry_revision == 0 {
            return Err(invalid("tool_registry_revision must be positive"));
        }
        if self.authorization_generation == 0 {
            return Err(invalid("authorization_generation must be positive"));
        }
        if (self.state == PerceptionCapabilityState::PresentUnavailable)
            != self.unavailable_reason.is_some()
        {
            return Err(invalid("unavailable reason must match capability state"));
        }
        if self.state == PerceptionCapabilityState::Available && self.visual_forms.is_empty() {
            return Err(invalid("available visual capability needs a capture form"));
        }
        if self.visual_forms.len() > 3
            || self
                .visual_forms
                .iter()
                .enumerate()
                .any(|(index, form)| self.visual_forms[..index].contains(form))
            || (self.state == PerceptionCapabilityState::Absent && !self.visual_forms.is_empty())
        {
            return Err(invalid("visual capture forms are invalid"));
        }
        Ok(())
    }

    pub fn subject_sentence(&self) -> Result<String> {
        self.validate_contract()?;
        let device = format!("{}/{}", self.host_id, self.device_id);
        let line = match (self.state, self.unavailable_reason) {
            (PerceptionCapabilityState::Absent, _) => {
                format!("I have no visual sensor on device {device}.")
            }
            (
                PerceptionCapabilityState::PresentUnavailable,
                Some(PerceptionUnavailableReason::UserDisabled),
            ) => format!("I have visual ability on device {device}, but its camera is currently off at the user's choice. I cannot look through it now and must not pressure them to enable it."),
            (PerceptionCapabilityState::PresentUnavailable, Some(_)) => format!(
                "I have visual ability on device {device}, but it is currently unavailable. I cannot look through it now."
            ),
            (PerceptionCapabilityState::Available, _) => format!(
                "I can request a visual look through device {device} while its tool remains available; I do not see continuously without new input or seek access as a relationship reward."
            ),
            _ => return Err(invalid("capability state is inconsistent")),
        };
        Ok(line)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct VisualMediaUnit {
    pub mime: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

impl fmt::Debug for VisualMediaUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VisualMediaUnit")
            .field("mime", &self.mime)
            .field("byte_len", &self.bytes.len())
            .field("sha256", &self.sha256)
            .finish()
    }
}

impl VisualMediaUnit {
    pub fn new(mime: impl Into<String>, bytes: Vec<u8>) -> Result<Self> {
        let media = Self {
            mime: mime.into(),
            sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
            bytes,
        };
        media.validate_contract()?;
        Ok(media)
    }

    pub fn validate_contract(&self) -> Result<()> {
        if self.bytes.is_empty() || self.bytes.len() > MAX_MEDIA_BYTES {
            return Err(invalid("visual media size is outside the admitted range"));
        }
        if !valid_sha256(&self.sha256)
            || self.sha256 != format!("sha256:{:x}", Sha256::digest(&self.bytes))
        {
            return Err(invalid("visual media digest does not match bytes"));
        }
        if self.mime.len() > 80 || self.mime.trim() != self.mime || self.mime.is_empty() {
            return Err(invalid("visual media mime is invalid"));
        }
        if !visual_media_signature_matches(&self.mime, &self.bytes) {
            return Err(invalid("visual media bytes do not match declared mime"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualTransformKind {
    FrameSelection,
    Compression,
    Transcoding,
    Resizing,
    Tiling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualTransformStep {
    pub kind: VisualTransformKind,
    pub parameters_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualTileSource {
    pub parent_index: usize,
    pub frame_sequence: Option<u64>,
    pub relative_time_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualDerivationManifest {
    pub parent_media_sha256: Vec<String>,
    pub transform_steps: Vec<VisualTransformStep>,
    pub tile_map: Vec<VisualTileSource>,
    pub derived_media_sha256: String,
}

impl VisualDerivationManifest {
    pub fn validate_contract(&self, actual_digest: &str) -> Result<()> {
        if self.parent_media_sha256.is_empty()
            || self.parent_media_sha256.len() > MAX_DERIVATION_PARENTS
            || !self
                .parent_media_sha256
                .iter()
                .all(|digest| valid_sha256(digest))
            || !valid_sha256(&self.derived_media_sha256)
            || self.derived_media_sha256 != actual_digest
            || self.transform_steps.is_empty()
            || self.transform_steps.len() > MAX_DERIVATION_PARENTS
            || !self
                .transform_steps
                .iter()
                .all(|step| valid_sha256(&step.parameters_digest))
            || self
                .tile_map
                .iter()
                .any(|tile| tile.parent_index >= self.parent_media_sha256.len())
        {
            return Err(invalid("visual derivation manifest is invalid"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualImage {
    pub media: VisualMediaUnit,
    pub width: u32,
    pub height: u32,
    pub derivation: Option<VisualDerivationManifest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualVideo {
    pub media: VisualMediaUnit,
    pub duration_ms: u64,
    pub timebase_num: u32,
    pub timebase_den: u32,
    /// The visual contract never admits an audio track implicitly.
    pub audio_track_present: bool,
    pub derivation: Option<VisualDerivationManifest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualFrame {
    pub sequence: u64,
    pub observed_at_unix_ms: u64,
    pub image: VisualImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualFrameStream {
    pub stream_id: String,
    pub frames: Vec<VisualFrame>,
    pub stopped: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisualInput {
    Image(VisualImage),
    Video(VisualVideo),
    FrameStream(VisualFrameStream),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerceptionMediaPart<'a> {
    pub ordinal: usize,
    pub mime: &'a str,
    pub bytes: &'a [u8],
    pub sha256: &'a str,
    pub frame_sequence: Option<u64>,
    pub observed_at_unix_ms: Option<u64>,
}

impl VisualInput {
    pub fn media_parts(&self) -> Vec<PerceptionMediaPart<'_>> {
        match self {
            Self::Image(image) => vec![part(0, &image.media, None, None)],
            Self::Video(video) => vec![part(0, &video.media, None, None)],
            Self::FrameStream(stream) => stream
                .frames
                .iter()
                .enumerate()
                .map(|(ordinal, frame)| {
                    part(
                        ordinal,
                        &frame.image.media,
                        Some(frame.sequence),
                        Some(frame.observed_at_unix_ms),
                    )
                })
                .collect(),
        }
    }

    pub fn validate_contract(&self) -> Result<()> {
        match self {
            Self::Image(image) => image.validate_contract(),
            Self::Video(video) => {
                video.media.validate_contract()?;
                if !matches!(video.media.mime.as_str(), "video/mp4" | "video/webm")
                    || video.duration_ms == 0
                    || video.timebase_num == 0
                    || video.timebase_den == 0
                    || video.audio_track_present
                {
                    return Err(invalid("visual video metadata or audio track is invalid"));
                }
                if let Some(manifest) = &video.derivation {
                    manifest.validate_contract(&video.media.sha256)?;
                }
                Ok(())
            }
            Self::FrameStream(stream) => {
                valid_id(&stream.stream_id, "stream_id")?;
                if stream.frames.len() > MAX_FRAME_COUNT
                    || (stream.frames.is_empty() && !stream.stopped)
                {
                    return Err(invalid("visual frame stream batch is invalid"));
                }
                let mut previous: Option<(u64, u64)> = None;
                for frame in &stream.frames {
                    frame.image.validate_contract()?;
                    if let Some((sequence, time)) = previous {
                        if frame.sequence <= sequence || frame.observed_at_unix_ms < time {
                            return Err(invalid("visual frame order or time is invalid"));
                        }
                    }
                    previous = Some((frame.sequence, frame.observed_at_unix_ms));
                }
                Ok(())
            }
        }
    }
}

impl VisualImage {
    pub fn validate_contract(&self) -> Result<()> {
        self.media.validate_contract()?;
        if !matches!(
            self.media.mime.as_str(),
            "image/jpeg" | "image/png" | "image/webp"
        ) || self.width == 0
            || self.height == 0
        {
            return Err(invalid("visual image metadata is invalid"));
        }
        if self.media.mime == "image/png"
            && (u32::from_be_bytes([
                self.media.bytes[16],
                self.media.bytes[17],
                self.media.bytes[18],
                self.media.bytes[19],
            ]) != self.width
                || u32::from_be_bytes([
                    self.media.bytes[20],
                    self.media.bytes[21],
                    self.media.bytes[22],
                    self.media.bytes[23],
                ]) != self.height)
        {
            return Err(invalid("png dimensions do not match declared dimensions"));
        }
        if let Some(manifest) = &self.derivation {
            manifest.validate_contract(&self.media.sha256)?;
        }
        Ok(())
    }
}

impl PerceptionInputEnvelope<VisualInput> {
    pub fn validate_contract(&self) -> Result<()> {
        self.validate_common()?;
        if self.modality != PerceptionModality::Visual
            || self.source.kind == PerceptionSourceKind::HostStructuredObservation
        {
            return Err(invalid("visual payload requires a visual source"));
        }
        self.payload.validate_contract()?;
        let total_bytes: usize = self
            .payload
            .media_parts()
            .iter()
            .map(|part| part.bytes.len())
            .sum();
        if total_bytes > MAX_MEDIA_BYTES {
            return Err(invalid("visual input exceeds the media budget"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VisualInputKind {
    Image,
    Video,
    FrameStream,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualView {
    pub input_id: String,
    pub source_key: PerceptionSourceKey,
    pub source_kind: PerceptionSourceKind,
    pub kind: VisualInputKind,
    pub presented_at_unix_ms: u64,
    pub media_sha256: Vec<String>,
    pub revoke_epoch: u64,
}

impl VisualView {
    pub fn from_input(input: &PerceptionInputEnvelope<VisualInput>) -> Result<Self> {
        input.validate_contract()?;
        let kind = match &input.payload {
            VisualInput::Image(_) => VisualInputKind::Image,
            VisualInput::Video(_) => VisualInputKind::Video,
            VisualInput::FrameStream(_) => VisualInputKind::FrameStream,
        };
        Ok(Self {
            input_id: input.input_id.clone(),
            source_key: input.source.key.clone(),
            source_kind: input.source.kind,
            kind,
            presented_at_unix_ms: input.time.presented_at_unix_ms,
            media_sha256: input
                .payload
                .media_parts()
                .iter()
                .map(|part| part.sha256.to_owned())
                .collect(),
            revoke_epoch: input.revoke_epoch,
        })
    }

    pub fn validate_contract(&self) -> Result<()> {
        valid_id(&self.input_id, "input_id")?;
        self.source_key.validate_contract()?;
        if self.presented_at_unix_ms == 0
            || self.media_sha256.len() > MAX_FRAME_COUNT
            || !self.media_sha256.iter().all(|digest| valid_sha256(digest))
            || (self.kind != VisualInputKind::FrameStream && self.media_sha256.len() != 1)
            || self.source_kind == PerceptionSourceKind::HostStructuredObservation
        {
            return Err(invalid("visual view metadata is invalid"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "modality", content = "view", rename_all = "snake_case")]
pub enum PerceptionView {
    Visual(VisualView),
}

impl PerceptionView {
    pub fn validate_contract(&self) -> Result<()> {
        match self {
            Self::Visual(view) => view.validate_contract(),
        }
    }
}

fn part<'a>(
    ordinal: usize,
    media: &'a VisualMediaUnit,
    frame_sequence: Option<u64>,
    observed_at_unix_ms: Option<u64>,
) -> PerceptionMediaPart<'a> {
    PerceptionMediaPart {
        ordinal,
        mime: &media.mime,
        bytes: &media.bytes,
        sha256: &media.sha256,
        frame_sequence,
        observed_at_unix_ms,
    }
}

fn valid_id(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_ID_LEN
        || value.trim() != value
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err(invalid(format!("{field} is invalid")));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_dependent_owner_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphanumeric() || b"._:-/".contains(&byte) {
            index += 1;
            continue;
        }
        if byte == b'%' {
            if let (Some(&high), Some(&low)) = (bytes.get(index + 1), bytes.get(index + 2)) {
                if is_upper_hex(high) && is_upper_hex(low) {
                    index += 3;
                    continue;
                }
            }
        }
        return false;
    }
    true
}

fn is_upper_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte)
}

fn visual_media_signature_matches(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => {
            bytes.len() >= 24
                && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                && &bytes[12..16] == b"IHDR"
        }
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        "video/mp4" => bytes.len() >= 12 && &bytes[4..8] == b"ftyp",
        "video/webm" => bytes.starts_with(b"\x1a\x45\xdf\xa3"),
        _ => false,
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::invalid_input("perception_contract", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episode_semantic_digest_ignores_only_optional_media_owner() {
        let semantic = serde_json::json!({
            "memory_space_id": "space",
            "subject_id": "subject",
            "source": {"host_id": "host", "source_id": "camera"},
            "observation": {"text": "a red coat"},
            "media": null
        });
        let mut with_media = semantic.clone();
        with_media["media"] = serde_json::json!({
            "key": "media:1", "sha256": format!("sha256:{}", "a".repeat(64))
        });
        let source_digest = perception_episode_semantic_digest(&semantic).unwrap();
        assert_eq!(
            source_digest,
            perception_episode_semantic_digest(&with_media).unwrap()
        );
        assert!(source_digest.starts_with("sha256:"));
        assert_eq!(source_digest.len(), 71);

        let mut changed_observation = with_media.clone();
        changed_observation["observation"]["text"] = "a blue coat".into();
        assert_ne!(
            source_digest,
            perception_episode_semantic_digest(&changed_observation).unwrap()
        );
        let mut changed_source = with_media;
        changed_source["source"]["source_id"] = "other-camera".into();
        assert_ne!(
            source_digest,
            perception_episode_semantic_digest(&changed_source).unwrap()
        );
    }

    #[test]
    fn episode_semantic_digest_rejects_missing_or_invalid_media_field() {
        assert!(perception_episode_semantic_digest(&serde_json::json!({
            "subject_id": "subject"
        }))
        .is_err());
        assert!(perception_episode_semantic_digest(&serde_json::json!({
            "subject_id": "subject", "media": "opaque"
        }))
        .is_err());
        assert!(perception_episode_semantic_digest(&"not an episode").is_err());
    }

    fn image() -> VisualImage {
        // Valid 1x1 RGBA PNG, generated from one scanline and a checked CRC.
        let hex = "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63f8cfc0f01f00050001ff89993d1d0000000049454e44ae426082";
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        VisualImage {
            media: VisualMediaUnit::new("image/png", bytes).unwrap(),
            width: 1,
            height: 1,
            derivation: None,
        }
    }

    fn envelope(payload: VisualInput) -> PerceptionInputEnvelope<VisualInput> {
        PerceptionInputEnvelope {
            input_id: "input-1".into(),
            modality: PerceptionModality::Visual,
            source: PerceptionSource {
                key: PerceptionSourceKey {
                    host_id: "host".into(),
                    device_id: Some("camera".into()),
                    source_id: "source".into(),
                    source_generation: 1,
                },
                kind: PerceptionSourceKind::HostVisualObservation,
                source_ref: PerceptionSourceRef::Sensor {
                    sensor_id: "camera".into(),
                },
            },
            scope: PerceptionScope {
                memory_space_id: "space".into(),
                subject_id: "subject".into(),
                relationship_id: None,
                session_id: "session".into(),
                turn_id: None,
                step_id: None,
            },
            time: PerceptionTime {
                observed_at_unix_ms: Some(1),
                presented_at_unix_ms: 2,
            },
            revoke_epoch: 0,
            payload,
        }
    }

    #[test]
    fn real_media_is_validated_and_world_view_has_no_bytes() {
        let input = envelope(VisualInput::Image(image()));
        input.validate_contract().unwrap();
        let view = VisualView::from_input(&input).unwrap();
        assert_eq!(view.media_sha256.len(), 1);
        assert!(input.payload.media_parts()[0]
            .bytes
            .starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn shared_photo_needs_message_binding_but_no_capture_device() {
        let mut input = envelope(VisualInput::Image(image()));
        input.source.key.device_id = None;
        input.source.kind = PerceptionSourceKind::HumanUserSharedMedia;
        input.source.source_ref = PerceptionSourceRef::UserAttachment {
            message_id: "message".into(),
            attachment_id: "attachment".into(),
        };
        assert!(input.validate_contract().is_ok());
        input.source.source_ref = PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        };
        assert!(input.validate_contract().is_err());
    }

    #[test]
    fn contextual_user_requires_matching_capture_and_single_person_inference() {
        let mut visual = envelope(VisualInput::Image(image()));
        visual.scope.turn_id = Some("turn-1".into());
        visual.time = PerceptionTime {
            observed_at_unix_ms: Some(100_000),
            presented_at_unix_ms: 100_010,
        };
        let input = VisualAttributionInput::from_envelope(&visual).unwrap();
        let context = VisualCaptureContext::new(
            input.source_key.clone(),
            VisualDeviceUse::PersonalSingleUser,
            true,
            Some("turn-1".into()),
            Some(99_500),
            100_000,
        )
        .unwrap();
        let scene = VisualSceneAttributionEvidence {
            input_id: input.input_id.clone(),
            source_media_sha256: input.media_sha256.clone(),
            visible_person_count: 1,
            third_party_or_visitor_present: false,
            conflicting_identity_evidence: false,
            inference_digest: format!("sha256:{}", "d".repeat(64)),
        };
        let decision = derive_contextual_user_attribution(&input, &context, &scene).unwrap();
        assert_eq!(
            decision.subject_attribution,
            SubjectAttribution::ContextualCurrentUser
        );
        assert_eq!(decision.confidence, AttributionConfidence::High);
        assert_eq!(decision.evidence_refs.len(), 2);

        let mut visitor = scene.clone();
        visitor.third_party_or_visitor_present = true;
        assert_eq!(
            derive_contextual_user_attribution(&input, &context, &visitor)
                .unwrap()
                .subject_attribution,
            SubjectAttribution::Unknown
        );
        let stale = VisualCaptureContext::new(
            input.source_key.clone(),
            VisualDeviceUse::PersonalSingleUser,
            true,
            Some("turn-1".into()),
            Some(50_000),
            100_000,
        )
        .unwrap();
        assert_eq!(
            derive_contextual_user_attribution(&input, &stale, &scene)
                .unwrap()
                .subject_attribution,
            SubjectAttribution::Unknown
        );
        let mut mismatched = scene;
        mismatched.source_media_sha256[0] = format!("sha256:{}", "e".repeat(64));
        assert!(derive_contextual_user_attribution(&input, &context, &mismatched).is_err());
    }

    #[test]
    fn ordinary_user_shared_scene_can_form_semantics_without_identity_or_media() {
        let ordinary_scene = VisualClassification {
            subject_attribution: SubjectAttribution::Unknown,
            attribution_confidence: AttributionConfidence::Low,
            visible_person_count: Some(0),
            third_party_present: false,
            sensitivity: VisualSensitivity::Ordinary,
            minor_present: false,
            sensitive_document_present: false,
            attribution_evidence_refs: Vec::new(),
            conflicting_evidence: false,
        };
        assert!(ordinary_scene.ordinary_nonhuman_semantic_allowed());
        assert!(!ordinary_scene.media_evidence_default_allowed());
        assert!(ordinary_scene
            .media_evidence_allowed_for_source(PerceptionSourceKind::HumanUserSharedMedia));
        assert!(!ordinary_scene
            .media_evidence_allowed_for_source(PerceptionSourceKind::HostVisualObservation));
        let mut third_party = ordinary_scene.clone();
        third_party.third_party_present = true;
        assert!(!third_party.ordinary_nonhuman_semantic_allowed());
        assert!(third_party.validate_contract().is_err());
        let mut uncounted = ordinary_scene;
        uncounted.visible_person_count = None;
        assert!(!uncounted.ordinary_nonhuman_semantic_allowed());
    }

    #[test]
    fn beetle_inference_needs_a_request_receipt_as_well_as_result_digest() {
        let input = envelope(VisualInput::Image(image()));
        let observation = VisualObservation {
            input_id: input.input_id.clone(),
            source_key: input.source.key.clone(),
            scope: input.scope.clone(),
            revoke_epoch: input.revoke_epoch,
            observed_at_unix_ms: input.time.observed_at_unix_ms,
            text: "A red cup is on the table.".into(),
            search_terms: vec!["red cup".into()],
            uncertainty: VisualObservationUncertainty::Low,
            classification: VisualClassification {
                subject_attribution: SubjectAttribution::Unknown,
                attribution_confidence: AttributionConfidence::Low,
                visible_person_count: Some(0),
                third_party_present: false,
                sensitivity: VisualSensitivity::Ordinary,
                minor_present: false,
                sensitive_document_present: false,
                attribution_evidence_refs: Vec::new(),
                conflicting_evidence: false,
            },
            evidence_origin: VisualEvidenceOrigin::BeetleInference {
                provider_id: "visual-provider".into(),
                request_receipt_digest: format!("sha256:{}", "a".repeat(64)),
                inference_digest: format!("sha256:{}", "b".repeat(64)),
            },
        };
        assert!(observation.validate_contract().is_ok());
        let mut invalid_terms = observation.clone();
        invalid_terms.search_terms = vec!["red cup".into(), "red cup".into()];
        assert!(invalid_terms.validate_contract().is_err());
        invalid_terms.search_terms = vec!["A".repeat(25)];
        assert!(invalid_terms.validate_contract().is_err());
        invalid_terms.search_terms = vec!["Red Cup".into()];
        assert!(invalid_terms.validate_contract().is_err());
        let mut host_reply = observation.clone();
        host_reply.evidence_origin = VisualEvidenceOrigin::HostAttestedReply {
            delivery_receipt_digest: format!("sha256:{}", "a".repeat(64)),
            reply_digest: format!("sha256:{}", "b".repeat(64)),
        };
        assert!(host_reply.validate_contract().is_err());
        host_reply.search_terms.clear();
        assert!(host_reply.validate_contract().is_ok());
        let mut missing_receipt = observation;
        if let VisualEvidenceOrigin::BeetleInference {
            request_receipt_digest,
            ..
        } = &mut missing_receipt.evidence_origin
        {
            request_receipt_digest.clear();
        }
        assert!(missing_receipt.validate_contract().is_err());
    }

    #[test]
    fn revoked_tool_keeps_sense_installed_and_cannot_claim_availability() {
        let capability = PerceptionCapabilitySnapshot {
            host_id: "host".into(),
            device_id: "device".into(),
            sensor_id: "camera".into(),
            tool_id: "look".into(),
            tool_registry_revision: 2,
            authorization_generation: 1,
            state: PerceptionCapabilityState::PresentUnavailable,
            unavailable_reason: Some(PerceptionUnavailableReason::UserDisabled),
            visual_forms: vec![VisualCaptureForm::Image],
        };
        assert!(capability.validate_contract().is_ok());
        assert_ne!(capability.state, PerceptionCapabilityState::Absent);
        let user_disabled = capability.subject_sentence().unwrap();
        assert!(user_disabled.contains("user's choice"));
        assert!(user_disabled.contains("must not pressure"));
        let system_disabled = PerceptionCapabilitySnapshot {
            unavailable_reason: Some(PerceptionUnavailableReason::SystemPermissionRevoked),
            ..capability
        }
        .subject_sentence()
        .unwrap();
        assert!(!system_disabled.contains("user's choice"));
    }

    #[test]
    fn frame_stream_rejects_non_monotonic_frames() {
        let first = VisualFrame {
            sequence: 1,
            observed_at_unix_ms: 10,
            image: image(),
        };
        let mut second = first.clone();
        second.sequence = 2;
        second.observed_at_unix_ms = 9;
        let input = envelope(VisualInput::FrameStream(VisualFrameStream {
            stream_id: "stream".into(),
            frames: vec![first, second],
            stopped: false,
        }));
        assert!(input.validate_contract().is_err());
    }

    fn episode_source() -> PerceptionEpisodeSourceRef {
        PerceptionEpisodeSourceRef {
            memory_space_id: "space".into(),
            subject_id: "subject".into(),
            relationship_id: Some("relationship".into()),
            source: PerceptionSourceKey {
                host_id: "host".into(),
                device_id: Some("camera".into()),
                source_id: "source".into(),
                source_generation: 1,
            },
            revoke_epoch: 0,
            episode_id: "episode".into(),
            content_digest: format!("sha256:{}", "a".repeat(64)),
        }
    }

    fn edge(
        kind: PerceptionSourceDependentKindV1,
        owner_key: &str,
    ) -> PerceptionSourceDependentEdgeV1 {
        PerceptionSourceDependentEdgeV1 {
            kind,
            memory_space_id: "space".into(),
            subject_id: "subject".into(),
            relationship_id: Some("relationship".into()),
            owner_key: owner_key.into(),
            owner_revision: Some(1),
            content_digest: format!("sha256:{}", "b".repeat(64)),
        }
    }

    #[test]
    fn dependent_root_sorts_edges_and_seals_exact_digest() {
        let root = PerceptionSourceDependentsRootV1::new(
            episode_source(),
            vec![
                edge(PerceptionSourceDependentKindV1::SoulRevision, "soul/1"),
                edge(PerceptionSourceDependentKindV1::TranscriptTurn, "turn/1"),
            ],
        )
        .unwrap();
        assert!(root.validate_contract().is_ok());
        assert_eq!(
            root.edges[0].kind,
            PerceptionSourceDependentKindV1::TranscriptTurn
        );
        let json = serde_json::to_string(&root).unwrap();
        assert!(!json.contains("media_bytes"));
        let mut tampered = root;
        tampered.edges[0].content_digest = format!("sha256:{}", "c".repeat(64));
        assert!(tampered.validate_contract().is_err());
    }

    #[test]
    fn dependent_root_rejects_duplicate_cross_scope_and_over_budget() {
        let source = episode_source();
        let transcript = edge(PerceptionSourceDependentKindV1::TranscriptTurn, "turn/1");
        assert!(PerceptionSourceDependentsRootV1::new(
            source.clone(),
            vec![transcript.clone(), transcript.clone()],
        )
        .is_err());
        let mut foreign = transcript;
        foreign.relationship_id = Some("other".into());
        assert!(PerceptionSourceDependentsRootV1::new(source.clone(), vec![foreign]).is_err());
        let too_many = (0..=MAX_PERCEPTION_SOURCE_DEPENDENT_EDGES_V1)
            .map(|index| {
                edge(
                    PerceptionSourceDependentKindV1::TranscriptTurn,
                    &format!("turn/{index}"),
                )
            })
            .collect();
        assert!(PerceptionSourceDependentsRootV1::new(source, too_many).is_err());
    }

    #[test]
    fn dependent_root_accepts_canonical_percent_encoded_store_key() {
        let conversation =
            crate::memory::ConversationKey::new("space:owner-default", "sdk.direct", "chat-1")
                .unwrap();
        let turn_key = conversation.turn_storage_key("turn:1");
        assert!(turn_key.contains("%3A"));
        let source = PerceptionEpisodeSourceRef {
            memory_space_id: "space:owner-default".into(),
            ..episode_source()
        };
        let mut transcript_edge = edge(PerceptionSourceDependentKindV1::TranscriptTurn, &turn_key);
        transcript_edge.memory_space_id = "space:owner-default".into();
        assert!(PerceptionSourceDependentsRootV1::new(
            source.clone(),
            vec![transcript_edge.clone()]
        )
        .is_ok());
        transcript_edge.owner_key = "ms%3aowner".into();
        assert!(PerceptionSourceDependentsRootV1::new(
            source.clone(),
            vec![transcript_edge.clone()]
        )
        .is_err());
        transcript_edge.owner_key = "ms%ZZowner".into();
        assert!(PerceptionSourceDependentsRootV1::new(source, vec![transcript_edge]).is_err());
    }
}
