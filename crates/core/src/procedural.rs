use crate::MemoryRecord;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub const PROCEDURAL_SKILL_IMPORT_FORMAT_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ProceduralSkillOrigin {
    #[default]
    UserProvided,
    RuntimeLearned,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ProceduralSkillState {
    Candidate,
    #[default]
    Active,
    Quarantined,
    Deprecated,
    Superseded,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ProceduralSkillReuseOutcome {
    #[default]
    Neutral,
    Succeeded,
    Mismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ProceduralSkillWriteAction {
    Inserted,
    Merged,
    Refreshed,
    Superseded,
    Quarantined,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ProceduralSkillWriteReason {
    ProceduralMemory,
    UserProvidedAccepted,
    ImportedRequiresAdjudication,
    RuntimeEvidenceAccepted,
    EmptyOrInvalid,
    RawPayloadOrLog,
    WeakProcedure,
    MissingScope,
    PrivacyRejected,
    ConflictWithoutEvidence,
    LowerQualityRejected,
    FactualRoutedAway,
    ProfileRejected,
}

impl ProceduralSkillWriteReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProceduralMemory => "procedural_memory",
            Self::UserProvidedAccepted => "user_provided_accepted",
            Self::ImportedRequiresAdjudication => "imported_requires_adjudication",
            Self::RuntimeEvidenceAccepted => "runtime_evidence_accepted",
            Self::EmptyOrInvalid => "empty_or_invalid",
            Self::RawPayloadOrLog => "raw_payload_or_log",
            Self::WeakProcedure => "weak_procedure",
            Self::MissingScope => "missing_scope",
            Self::PrivacyRejected => "privacy_rejected",
            Self::ConflictWithoutEvidence => "conflict_without_evidence",
            Self::LowerQualityRejected => "lower_quality_rejected",
            Self::FactualRoutedAway => "factual_routed_away",
            Self::ProfileRejected => "profile_rejected",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralEvidenceRef {
    pub source: String,
    pub summary: String,
}

impl ProceduralEvidenceRef {
    pub fn new(source: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            summary: summary.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkillProvenance {
    pub source_kind: String,
    pub source_ref: Option<String>,
    pub author: Option<String>,
    pub checksum: Option<String>,
    pub imported: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkillDraft {
    pub identity: String,
    pub scope: String,
    pub origin: ProceduralSkillOrigin,
    pub title: String,
    pub trigger: String,
    pub procedure: String,
    pub constraints: Vec<String>,
    pub evidence: Vec<ProceduralEvidenceRef>,
    pub provenance: ProceduralSkillProvenance,
    pub capability_hints: Vec<String>,
    pub component_topics: Vec<String>,
    pub observed_at: Option<u64>,
}

impl ProceduralSkillDraft {
    pub fn new(
        identity: impl Into<String>,
        scope: impl Into<String>,
        origin: ProceduralSkillOrigin,
        title: impl Into<String>,
        trigger: impl Into<String>,
        procedure: impl Into<String>,
    ) -> Self {
        Self {
            identity: identity.into(),
            scope: scope.into(),
            origin,
            title: title.into(),
            trigger: trigger.into(),
            procedure: procedure.into(),
            constraints: Vec::new(),
            evidence: Vec::new(),
            provenance: ProceduralSkillProvenance {
                source_kind: match origin {
                    ProceduralSkillOrigin::UserProvided => "user_provided".to_owned(),
                    ProceduralSkillOrigin::RuntimeLearned => "runtime_learned".to_owned(),
                },
                ..ProceduralSkillProvenance::default()
            },
            capability_hints: Vec::new(),
            component_topics: Vec::new(),
            observed_at: None,
        }
    }

    pub fn procedure(mut self, procedure: impl Into<String>) -> Self {
        self.procedure = procedure.into();
        self
    }

    pub fn evidence(mut self, evidence: Vec<ProceduralEvidenceRef>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn constraints(mut self, constraints: Vec<String>) -> Self {
        self.constraints = constraints;
        self
    }

    pub fn capability_hints(mut self, hints: Vec<String>) -> Self {
        self.capability_hints = hints;
        self
    }

    pub fn observed_at(mut self, observed_at: u64) -> Self {
        self.observed_at = Some(observed_at);
        self
    }

    pub fn imported(mut self, source_ref: impl Into<String>) -> Self {
        self.origin = ProceduralSkillOrigin::UserProvided;
        self.provenance.imported = true;
        self.provenance.source_kind = "user_provided".to_owned();
        self.provenance.source_ref = Some(source_ref.into());
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkillLineageNode {
    pub node_id: String,
    pub strategy_digest: String,
    pub recorded_at: u64,
    pub summary: String,
    pub state: ProceduralSkillState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkillStrategyDiff {
    pub recorded_at: u64,
    pub from_node_id: String,
    pub to_node_id: String,
    pub summary: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkillMeta {
    pub origin: ProceduralSkillOrigin,
    pub state: ProceduralSkillState,
    pub trigger: String,
    pub constraints: Vec<String>,
    pub quality_score: u8,
    #[serde(default)]
    pub evidence_count: usize,
    pub use_count: u32,
    pub validated_success_count: u32,
    pub mismatch_count: u32,
    pub revision_count: u32,
    pub revision_pending: bool,
    pub last_used_at: Option<u64>,
    pub last_outcome_at: Option<u64>,
    pub last_outcome_note: String,
    pub supersedes: Vec<String>,
    pub component_topics: Vec<String>,
    pub lineage: Vec<ProceduralSkillLineageNode>,
    pub strategy_diffs: Vec<ProceduralSkillStrategyDiff>,
    pub provenance: ProceduralSkillProvenance,
    pub capability_hints: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkillInspection {
    pub accepted: bool,
    pub reason: ProceduralSkillWriteReason,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkillWriteReport {
    pub action: ProceduralSkillWriteAction,
    pub reason: ProceduralSkillWriteReason,
    pub state: ProceduralSkillState,
    pub slot_id: String,
    pub quality_score: u8,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkillImportEnvelope {
    pub format_version: u8,
    pub draft: ProceduralSkillDraft,
    pub source_digest: String,
    pub exported_at: Option<u64>,
}

impl ProceduralSkillImportEnvelope {
    pub fn new(draft: ProceduralSkillDraft, source_digest: impl Into<String>) -> Self {
        Self {
            format_version: PROCEDURAL_SKILL_IMPORT_FORMAT_VERSION,
            draft,
            source_digest: source_digest.into(),
            exported_at: None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProceduralSkillImportReport {
    pub imported: usize,
    pub quarantined: usize,
    pub adopted: usize,
    pub rejected: usize,
    pub reports: Vec<ProceduralSkillWriteReport>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProceduralSkillOutcomeReport {
    pub submitted: usize,
    pub updated: usize,
    pub missing: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProceduralSkillRecallQuery {
    pub scope: String,
    pub query: String,
    pub limit: usize,
}

impl ProceduralSkillRecallQuery {
    pub fn new(scope: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            query: query.into(),
            limit: 4,
        }
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProceduralSkillRecallScoreBreakdown {
    pub lexical_score: u32,
    pub trigger_score: u32,
    pub procedure_score: u32,
    pub recency_score: u32,
    pub quality_score: u32,
    pub validation_score: u32,
    pub scope_affinity_score: u32,
    pub governance_score: u32,
    pub origin_score: u32,
    pub total_score: u32,
    pub reason_fragments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProceduralSkillRecallCandidate {
    pub record_id: String,
    pub title: String,
    pub trigger: String,
    pub procedure: String,
    pub state: ProceduralSkillState,
    pub origin: ProceduralSkillOrigin,
    pub quality_score: u8,
    pub validated_success_count: u32,
    pub score: ProceduralSkillRecallScoreBreakdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProceduralSkillSkippedCandidate {
    pub record_id: String,
    pub state: ProceduralSkillState,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProceduralSkillRecallReport {
    pub query: Option<ProceduralSkillRecallQuery>,
    pub backend: String,
    pub candidate_count: usize,
    pub selected_count: usize,
    pub selected_ids: Vec<String>,
    pub miss_reason: Option<String>,
    pub selection_note: Option<String>,
    pub selected: Vec<ProceduralSkillRecallCandidate>,
    pub skipped: Vec<ProceduralSkillSkippedCandidate>,
    pub warnings: Vec<String>,
}

impl Default for ProceduralSkillRecallQuery {
    fn default() -> Self {
        Self::new("", "")
    }
}

pub fn inspect_procedural_skill_draft(draft: &ProceduralSkillDraft) -> ProceduralSkillInspection {
    if draft.identity.trim().is_empty()
        || draft.scope.trim().is_empty()
        || draft.title.trim().is_empty()
        || draft.trigger.trim().is_empty()
        || draft.procedure.trim().is_empty()
    {
        return rejected_inspection(ProceduralSkillWriteReason::EmptyOrInvalid);
    }
    if draft.scope.trim().eq_ignore_ascii_case("global")
        && draft.origin == ProceduralSkillOrigin::RuntimeLearned
    {
        return rejected_inspection(ProceduralSkillWriteReason::MissingScope);
    }
    if looks_like_raw_payload_or_log(&draft.procedure) {
        return rejected_inspection(ProceduralSkillWriteReason::RawPayloadOrLog);
    }
    if looks_like_private_material(&draft.procedure)
        || draft
            .constraints
            .iter()
            .any(|value| looks_like_private_material(value))
    {
        return rejected_inspection(ProceduralSkillWriteReason::PrivacyRejected);
    }
    if !looks_like_procedure(&draft.procedure) {
        return rejected_inspection(ProceduralSkillWriteReason::FactualRoutedAway);
    }
    if draft.origin == ProceduralSkillOrigin::RuntimeLearned && draft.evidence.is_empty() {
        return rejected_inspection(ProceduralSkillWriteReason::WeakProcedure);
    }
    ProceduralSkillInspection {
        accepted: true,
        reason: match draft.origin {
            ProceduralSkillOrigin::UserProvided => ProceduralSkillWriteReason::UserProvidedAccepted,
            ProceduralSkillOrigin::RuntimeLearned => {
                ProceduralSkillWriteReason::RuntimeEvidenceAccepted
            }
        },
        detail: "accepted as procedural memory".to_owned(),
    }
}

pub fn procedural_skill_slot_id(draft: &ProceduralSkillDraft) -> String {
    format!(
        "procedural::{}::{}::{}",
        draft.identity.trim(),
        draft.scope.trim(),
        normalize_key(&draft.trigger)
    )
}

pub fn procedural_strategy_digest(title: &str, trigger: &str, procedure: &str) -> String {
    let mut hasher = DefaultHasher::new();
    normalize_text(title).hash(&mut hasher);
    normalize_text(trigger).hash(&mut hasher);
    normalize_text(procedure).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn procedural_skill_meta_from_draft(
    draft: &ProceduralSkillDraft,
    state: ProceduralSkillState,
    now: u64,
) -> ProceduralSkillMeta {
    let digest = procedural_strategy_digest(&draft.title, &draft.trigger, &draft.procedure);
    let mut component_topics = draft.component_topics.clone();
    if !component_topics.iter().any(|topic| topic == &draft.trigger) {
        component_topics.push(draft.trigger.clone());
    }
    component_topics.sort();
    component_topics.dedup();
    let mut meta = ProceduralSkillMeta {
        origin: draft.origin,
        state,
        trigger: draft.trigger.trim().to_owned(),
        constraints: normalize_vec(&draft.constraints),
        quality_score: 0,
        evidence_count: draft.evidence.len(),
        use_count: 0,
        validated_success_count: 0,
        mismatch_count: 0,
        revision_count: 0,
        revision_pending: false,
        last_used_at: None,
        last_outcome_at: None,
        last_outcome_note: String::new(),
        supersedes: Vec::new(),
        component_topics,
        lineage: vec![ProceduralSkillLineageNode {
            node_id: format!(
                "{}@{}",
                normalize_key(&draft.trigger),
                digest.chars().take(10).collect::<String>()
            ),
            strategy_digest: digest,
            recorded_at: draft.observed_at.unwrap_or(now),
            summary: first_chars(&draft.title, 120),
            state,
        }],
        strategy_diffs: Vec::new(),
        provenance: draft.provenance.clone(),
        capability_hints: normalize_vec(&draft.capability_hints),
    };
    meta.quality_score = compute_procedural_skill_quality(&meta);
    meta
}

pub fn compute_procedural_skill_quality(meta: &ProceduralSkillMeta) -> u8 {
    let mut score: i32 = 20;
    score += match meta.state {
        ProceduralSkillState::Active => 20,
        ProceduralSkillState::Candidate => 5,
        ProceduralSkillState::Quarantined => -10,
        ProceduralSkillState::Deprecated | ProceduralSkillState::Superseded => -20,
    };
    score += match meta.origin {
        ProceduralSkillOrigin::UserProvided => 12,
        ProceduralSkillOrigin::RuntimeLearned => 6,
    };
    score += (meta.validated_success_count.min(5) as i32) * 8;
    score += (meta.evidence_count.min(5) as i32) * 4;
    score += (meta.use_count.min(6) as i32) * 2;
    score += (meta.capability_hints.len().min(3) as i32) * 2;
    score -= (meta.mismatch_count.min(5) as i32) * 8;
    if meta.revision_pending {
        score -= 8;
    }
    score.clamp(0, 100) as u8
}

pub fn score_procedural_skill_record(
    record: &MemoryRecord,
    query: &str,
    scope: &str,
) -> ProceduralSkillRecallScoreBreakdown {
    let Some(meta) = record.meta.procedural.as_ref() else {
        return ProceduralSkillRecallScoreBreakdown::default();
    };
    let normalized_query = normalize_text(query);
    let normalized_trigger = normalize_text(&meta.trigger);
    let normalized_procedure = normalize_text(&record.content);
    let mut reasons = Vec::new();

    let trigger_score =
        if !normalized_query.is_empty() && normalized_trigger.contains(&normalized_query) {
            reasons.push("trigger_match".to_owned());
            30
        } else {
            token_overlap_score(&normalized_query, &normalized_trigger, 3)
        };
    let procedure_score = token_overlap_score(&normalized_query, &normalized_procedure, 2);
    if procedure_score > 0 {
        reasons.push("procedure_match".to_owned());
    }
    let scope_affinity_score = if record.scope == scope {
        reasons.push("same_scope".to_owned());
        10
    } else {
        0
    };
    let governance_score = match meta.state {
        ProceduralSkillState::Active => {
            reasons.push("active".to_owned());
            16
        }
        ProceduralSkillState::Candidate => {
            reasons.push("candidate".to_owned());
            4
        }
        ProceduralSkillState::Quarantined => {
            reasons.push("quarantined".to_owned());
            0
        }
        ProceduralSkillState::Deprecated => {
            reasons.push("deprecated".to_owned());
            0
        }
        ProceduralSkillState::Superseded => {
            reasons.push("superseded".to_owned());
            0
        }
    };
    let origin_score = match meta.origin {
        ProceduralSkillOrigin::UserProvided => 8,
        ProceduralSkillOrigin::RuntimeLearned => 4,
    };
    let validation_score = meta.validated_success_count.min(5) * 4;
    if validation_score > 0 {
        reasons.push("validated".to_owned());
    }
    let quality_score = u32::from(meta.quality_score / 4);
    let recency_score = meta.last_used_at.map(|_| 4).unwrap_or(0);
    let lexical_score = token_overlap_score(&normalized_query, &normalize_text(&record.content), 1);
    let total_score = lexical_score
        + trigger_score
        + procedure_score
        + recency_score
        + quality_score
        + validation_score
        + scope_affinity_score
        + governance_score
        + origin_score;
    ProceduralSkillRecallScoreBreakdown {
        lexical_score,
        trigger_score,
        procedure_score,
        recency_score,
        quality_score,
        validation_score,
        scope_affinity_score,
        governance_score,
        origin_score,
        total_score,
        reason_fragments: reasons,
    }
}

pub fn parse_procedural_skill_import_envelope(
    envelope_json: &str,
) -> Result<ProceduralSkillImportEnvelope, String> {
    let mut envelope: ProceduralSkillImportEnvelope =
        serde_json::from_str(envelope_json).map_err(|error| error.to_string())?;
    if envelope.format_version != PROCEDURAL_SKILL_IMPORT_FORMAT_VERSION {
        return Err(format!(
            "unsupported procedural skill import format_version {}",
            envelope.format_version
        ));
    }
    envelope.draft.origin = ProceduralSkillOrigin::UserProvided;
    envelope.draft.provenance.imported = true;
    envelope.draft.provenance.source_kind = "user_provided".to_owned();
    envelope.draft.provenance.checksum = Some(envelope.source_digest.clone());
    Ok(envelope)
}

fn rejected_inspection(reason: ProceduralSkillWriteReason) -> ProceduralSkillInspection {
    ProceduralSkillInspection {
        accepted: false,
        reason,
        detail: reason.as_str().to_owned(),
    }
}

fn looks_like_raw_payload_or_log(content: &str) -> bool {
    let trimmed = content.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || trimmed.starts_with('[')
        || trimmed.contains("level=")
        || trimmed.contains(" stack backtrace:")
        || trimmed.contains("Traceback (most recent call last)")
        || trimmed
            .lines()
            .filter(|line| line.contains("ERROR") || line.contains("WARN"))
            .count()
            >= 2
}

fn looks_like_private_material(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("secret=")
        || lower.contains("api_key")
        || lower.contains("password")
        || lower.contains("private raw")
        || lower.contains("sealed")
}

fn looks_like_procedure(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    content.contains("下次")
        || content.contains("步骤")
        || (content.contains("先") && content.contains("再"))
        || lower.contains("when ")
        || lower.contains("first ")
        || lower.contains("then ")
        || lower.contains("next time")
        || lower
            .lines()
            .filter(|line| line.trim_start().starts_with("- "))
            .count()
            >= 2
}

fn normalize_key(value: &str) -> String {
    let mut out = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').to_owned()
}

fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_vec(values: &[String]) -> Vec<String> {
    let mut out = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

fn token_overlap_score(query: &str, target: &str, weight: u32) -> u32 {
    if query.is_empty() || target.is_empty() {
        return 0;
    }
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 3)
        .filter(|token| target.contains(token))
        .count()
        .min(10) as u32
        * weight
}

fn first_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
