//! Governed typed facet index contract for memory recall.

use serde::{Deserialize, Serialize};

use super::long_term::{
    LongTermMemoryEntry, LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemorySourceScope,
    LongTermMemorySourceType,
};
use super::recall_anchor::{recall_evidence_group_key, recall_source_authority_score};

pub const MEMORY_FACET_INDEX_NAMESPACE: &str = "memory_facet_indexes";
pub const MEMORY_FACET_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFacetOwnerPlane {
    LongTerm,
    ConversationTranscript,
    MemoryGraph,
    RuntimeSkill,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFacetStatus {
    Active,
    Stale,
    Superseded,
    Tombstoned,
    Redacted,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFacetNamespace {
    Kind,
    Topic,
    Keyword,
    SourceScope,
    SourceType,
    Freshness,
    Temporal,
    Evidence,
    Entity,
    GraphAnchor,
}

impl MemoryFacetNamespace {
    fn label(self) -> &'static str {
        match self {
            Self::Kind => "kind",
            Self::Topic => "topic",
            Self::Keyword => "keyword",
            Self::SourceScope => "source_scope",
            Self::SourceType => "source_type",
            Self::Freshness => "freshness",
            Self::Temporal => "temporal",
            Self::Evidence => "evidence",
            Self::Entity => "entity",
            Self::GraphAnchor => "graph_anchor",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TemporalAnchorKind {
    ObservedAt,
    LastConfirmedAt,
    ValidFrom,
    ValidUntil,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TemporalAnchorPrecision {
    Second,
    Day,
    Month,
    Year,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CanonicalEvidenceRef {
    pub source_ref: String,
    pub canonical_evidence_group: String,
    pub source_kind: String,
    pub source_authority_score: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CanonicalEntityRef {
    pub entity_kind: String,
    pub canonical_id: String,
    pub display_label: Option<String>,
    pub aliases: Vec<String>,
    pub evidence_ref: CanonicalEvidenceRef,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TemporalAnchor {
    pub anchor_kind: TemporalAnchorKind,
    pub epoch_secs: u64,
    pub precision: TemporalAnchorPrecision,
    pub evidence_ref: CanonicalEvidenceRef,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryFacetValue {
    Kind {
        normalized: String,
    },
    Topic {
        normalized: String,
        segments: Vec<String>,
    },
    Keyword {
        normalized: String,
    },
    SourceScope {
        normalized: String,
    },
    SourceType {
        normalized: String,
    },
    Freshness {
        normalized: String,
    },
    Temporal {
        anchor: TemporalAnchor,
    },
    Evidence {
        evidence: CanonicalEvidenceRef,
    },
    Entity {
        entity: CanonicalEntityRef,
    },
    GraphAnchor {
        anchor_id: String,
    },
}

impl MemoryFacetValue {
    fn canonical_key(&self) -> String {
        match self {
            Self::Kind { normalized }
            | Self::Keyword { normalized }
            | Self::SourceScope { normalized }
            | Self::SourceType { normalized }
            | Self::Freshness { normalized } => normalized.clone(),
            Self::Topic { normalized, .. } => normalized.clone(),
            Self::Temporal { anchor } => format!(
                "{}:{}:{}:{}",
                anchor.anchor_kind.label(),
                anchor.epoch_secs,
                anchor.precision.label(),
                anchor.evidence_ref.canonical_evidence_group
            ),
            Self::Evidence { evidence } => evidence.canonical_evidence_group.clone(),
            Self::Entity { entity } => format!("{}:{}", entity.entity_kind, entity.canonical_id),
            Self::GraphAnchor { anchor_id } => anchor_id.clone(),
        }
    }
}

impl TemporalAnchorKind {
    fn label(self) -> &'static str {
        match self {
            Self::ObservedAt => "observed_at",
            Self::LastConfirmedAt => "last_confirmed_at",
            Self::ValidFrom => "valid_from",
            Self::ValidUntil => "valid_until",
        }
    }
}

impl TemporalAnchorPrecision {
    fn label(self) -> &'static str {
        match self {
            Self::Second => "second",
            Self::Day => "day",
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFacet {
    pub facet_id: String,
    pub namespace: MemoryFacetNamespace,
    pub value: MemoryFacetValue,
    pub source_evidence_refs: Vec<CanonicalEvidenceRef>,
    pub derived_from_exact_facet_id: Option<String>,
    pub expansion_rule_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFacetIndexDoc {
    pub owner_record_id: String,
    pub owner_plane: MemoryFacetOwnerPlane,
    pub schema_version: u32,
    pub facet_index_revision: u64,
    pub memory_space_id: String,
    pub subject_ids: Vec<String>,
    pub status: MemoryFacetStatus,
    pub exact_facets: Vec<MemoryFacet>,
    pub expanded_facets: Vec<MemoryFacet>,
    pub canonical_evidence_refs: Vec<CanonicalEvidenceRef>,
    pub source_revision: u64,
    pub updated_at: u64,
}

impl MemoryFacetIndexDoc {
    pub fn store_key(&self) -> String {
        format!("facet-index:{}", self.owner_record_id)
    }

    pub fn report_view(&self, audience: FacetReportAudience) -> FacetReportView {
        let redacted = !matches!(audience, FacetReportAudience::OwnerRaw);
        let visible_canonical_evidence_groups = if redacted {
            Vec::new()
        } else {
            self.canonical_evidence_refs
                .iter()
                .map(|evidence| evidence.canonical_evidence_group.clone())
                .collect()
        };
        let mut namespaces = self
            .exact_facets
            .iter()
            .chain(self.expanded_facets.iter())
            .map(|facet| facet.namespace)
            .collect::<Vec<_>>();
        namespaces.sort_by_key(|namespace| namespace.label());
        namespaces.dedup();

        FacetReportView {
            owner_record_id: self.owner_record_id.clone(),
            owner_plane: self.owner_plane,
            schema_version: self.schema_version,
            facet_index_revision: self.facet_index_revision,
            status: self.status,
            exact_facet_count: self.exact_facets.len(),
            expanded_facet_count: self.expanded_facets.len(),
            namespaces,
            redacted_sensitive_metadata: redacted,
            visible_canonical_evidence_groups,
            redacted_canonical_evidence_group_count: if redacted {
                self.canonical_evidence_refs.len()
            } else {
                0
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FacetReportAudience {
    BenchmarkArtifact,
    HostUi,
    OperatorAudit,
    OwnerRaw,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FacetReportView {
    pub owner_record_id: String,
    pub owner_plane: MemoryFacetOwnerPlane,
    pub schema_version: u32,
    pub facet_index_revision: u64,
    pub status: MemoryFacetStatus,
    pub exact_facet_count: usize,
    pub expanded_facet_count: usize,
    pub namespaces: Vec<MemoryFacetNamespace>,
    pub redacted_sensitive_metadata: bool,
    pub visible_canonical_evidence_groups: Vec<String>,
    pub redacted_canonical_evidence_group_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FacetIndexRebuildReport {
    pub namespace: String,
    pub scanned_owner_records: usize,
    pub rebuilt_index_docs: usize,
    pub orphan_owner_record_ids: Vec<String>,
    pub schema_failures: Vec<String>,
    pub migration_failures: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FacetRankFusionCandidateReport {
    pub candidate_id: String,
    pub source_rank: Option<usize>,
    pub exact_facet_rank: Option<usize>,
    pub expanded_facet_rank: Option<usize>,
    pub facet_rank: Option<usize>,
    pub fused_rank: usize,
    pub fused_score_bps: u32,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FacetRankFusionReport {
    pub owner: String,
    pub used: bool,
    pub strategy: String,
    pub source_pool_count: usize,
    pub exact_facet_pool_count: usize,
    pub expanded_facet_pool_count: usize,
    pub candidate_reports: Vec<FacetRankFusionCandidateReport>,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FacetCoverageSelectionReport {
    pub owner: String,
    pub used: bool,
    pub strategy: String,
    pub selected_candidate_ids: Vec<String>,
    pub covered_evidence_groups: Vec<String>,
    pub coverage_dropped_candidate_ids: Vec<String>,
    pub fusion_dropped_candidate_ids: Vec<String>,
    pub budget_truncated_candidate_ids: Vec<String>,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanFacetSuggestion {
    pub suggestion_id: String,
    pub suggested_by: String,
    pub owner_record_id: String,
    pub proposed_facets: Vec<String>,
    pub governed_proposal_id: Option<String>,
}

impl HumanFacetSuggestion {
    pub fn validate_contract(&self) -> MemoryFacetContractValidation {
        if self
            .governed_proposal_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return rejected("human_facet_suggestion_requires_governed_proposal");
        }
        if self.suggestion_id.trim().is_empty() {
            return rejected("human_facet_suggestion_id_empty");
        }
        if self.owner_record_id.trim().is_empty() {
            return rejected("human_facet_suggestion_owner_empty");
        }
        if self.proposed_facets.is_empty() {
            return rejected("human_facet_suggestion_facets_empty");
        }
        accepted()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFacetContractValidation {
    pub accepted: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuredFacetParseOutcome {
    pub accepted: bool,
    pub reason: String,
    pub entity: Option<CanonicalEntityRef>,
    pub temporal_anchor: Option<TemporalAnchor>,
}

pub struct StructuredFacetParser;

impl StructuredFacetParser {
    pub fn parse_entity_anchor(raw: &str, evidence_ref: &str) -> StructuredFacetParseOutcome {
        let trimmed = raw.trim();
        if looks_like_regex_only(trimmed) {
            return parse_rejected("regex_only_entity_facet_rejected");
        }
        let evidence = match canonical_evidence_ref(evidence_ref) {
            Some(evidence) => evidence,
            None => return parse_rejected("entity_facet_requires_canonical_evidence_ref"),
        };
        let Some((entity_kind, canonical_id)) = trimmed.split_once(':') else {
            return parse_rejected("entity_facet_requires_typed_kind_and_id");
        };
        let entity_kind = entity_kind.trim().to_ascii_lowercase();
        let canonical_id = normalize_text_value(canonical_id);
        if entity_kind.is_empty() || canonical_id.is_empty() {
            return parse_rejected("entity_facet_requires_typed_kind_and_id");
        }

        StructuredFacetParseOutcome {
            accepted: true,
            reason: "accepted".to_string(),
            entity: Some(CanonicalEntityRef {
                entity_kind,
                canonical_id,
                display_label: None,
                aliases: Vec::new(),
                evidence_ref: evidence,
            }),
            temporal_anchor: None,
        }
    }

    pub fn parse_temporal_anchor(raw: &str, evidence_ref: &str) -> StructuredFacetParseOutcome {
        let trimmed = raw.trim();
        if looks_like_regex_only(trimmed) {
            return parse_rejected("regex_only_temporal_facet_rejected");
        }
        let evidence = match canonical_evidence_ref(evidence_ref) {
            Some(evidence) => evidence,
            None => return parse_rejected("temporal_facet_requires_canonical_evidence_ref"),
        };
        let Ok(epoch_secs) = trimmed.parse::<u64>() else {
            return parse_rejected("temporal_facet_requires_epoch_seconds");
        };
        if epoch_secs == 0 {
            return parse_rejected("temporal_facet_requires_epoch_seconds");
        }

        StructuredFacetParseOutcome {
            accepted: true,
            reason: "accepted".to_string(),
            entity: None,
            temporal_anchor: Some(TemporalAnchor {
                anchor_kind: TemporalAnchorKind::ObservedAt,
                epoch_secs,
                precision: TemporalAnchorPrecision::Second,
                evidence_ref: evidence,
            }),
        }
    }
}

pub fn build_long_term_memory_facet_index_doc(
    entry: &LongTermMemoryEntry,
    memory_space_id: impl Into<String>,
    subject_ids: Vec<String>,
    facet_index_revision: u64,
) -> MemoryFacetIndexDoc {
    let canonical_evidence_refs = canonicalize_evidence_refs(entry);
    let primary_evidence = canonical_evidence_refs
        .first()
        .cloned()
        .unwrap_or_else(|| synthetic_owner_evidence_ref(&entry.id));
    let owner_record_id = entry.id.clone();
    let mut exact_facets = Vec::new();

    push_facet(
        &mut exact_facets,
        &owner_record_id,
        MemoryFacetNamespace::Kind,
        MemoryFacetValue::Kind {
            normalized: kind_value(&entry.kind).to_string(),
        },
        vec![primary_evidence.clone()],
    );

    let normalized_topic = normalize_text_value(&entry.topic);
    if !normalized_topic.is_empty() {
        push_facet(
            &mut exact_facets,
            &owner_record_id,
            MemoryFacetNamespace::Topic,
            MemoryFacetValue::Topic {
                normalized: normalized_topic.clone(),
                segments: hierarchy_segments(&normalized_topic),
            },
            vec![primary_evidence.clone()],
        );
    }

    for keyword in &entry.keywords {
        let normalized = normalize_text_value(keyword);
        if normalized.is_empty() {
            continue;
        }
        push_facet(
            &mut exact_facets,
            &owner_record_id,
            MemoryFacetNamespace::Keyword,
            MemoryFacetValue::Keyword { normalized },
            vec![primary_evidence.clone()],
        );
    }

    push_facet(
        &mut exact_facets,
        &owner_record_id,
        MemoryFacetNamespace::SourceScope,
        MemoryFacetValue::SourceScope {
            normalized: source_scope_value(entry.source_scope).to_string(),
        },
        vec![primary_evidence.clone()],
    );
    push_facet(
        &mut exact_facets,
        &owner_record_id,
        MemoryFacetNamespace::SourceType,
        MemoryFacetValue::SourceType {
            normalized: source_type_value(entry.source_type).to_string(),
        },
        vec![primary_evidence.clone()],
    );
    push_facet(
        &mut exact_facets,
        &owner_record_id,
        MemoryFacetNamespace::Freshness,
        MemoryFacetValue::Freshness {
            normalized: freshness_value(entry.freshness).to_string(),
        },
        vec![primary_evidence.clone()],
    );

    for (anchor_kind, epoch_secs) in [
        (TemporalAnchorKind::ObservedAt, entry.observed_at),
        (TemporalAnchorKind::LastConfirmedAt, entry.last_confirmed_at),
    ] {
        if epoch_secs == 0 {
            continue;
        }
        push_facet(
            &mut exact_facets,
            &owner_record_id,
            MemoryFacetNamespace::Temporal,
            MemoryFacetValue::Temporal {
                anchor: TemporalAnchor {
                    anchor_kind,
                    epoch_secs,
                    precision: TemporalAnchorPrecision::Second,
                    evidence_ref: primary_evidence.clone(),
                },
            },
            vec![primary_evidence.clone()],
        );
    }

    for evidence in &canonical_evidence_refs {
        push_facet(
            &mut exact_facets,
            &owner_record_id,
            MemoryFacetNamespace::Evidence,
            MemoryFacetValue::Evidence {
                evidence: evidence.clone(),
            },
            vec![evidence.clone()],
        );
    }

    dedup_facets(&mut exact_facets);
    let expanded_facets = build_expanded_facets(&owner_record_id, &exact_facets);

    MemoryFacetIndexDoc {
        owner_record_id,
        owner_plane: MemoryFacetOwnerPlane::LongTerm,
        schema_version: MEMORY_FACET_SCHEMA_VERSION,
        facet_index_revision,
        memory_space_id: memory_space_id.into(),
        subject_ids: normalize_subject_ids(subject_ids),
        status: MemoryFacetStatus::Active,
        exact_facets,
        expanded_facets,
        canonical_evidence_refs,
        source_revision: entry.source_revision,
        updated_at: entry.updated_at.max(entry.created_at),
    }
}

fn build_expanded_facets(owner_record_id: &str, exact_facets: &[MemoryFacet]) -> Vec<MemoryFacet> {
    let mut expanded = Vec::new();
    for exact in exact_facets {
        if exact.namespace != MemoryFacetNamespace::Topic {
            continue;
        }
        let MemoryFacetValue::Topic { segments, .. } = &exact.value else {
            continue;
        };
        if segments.len() < 2 {
            continue;
        }
        for ancestor_len in 1..segments.len() {
            let ancestor = segments[..ancestor_len].join("/");
            let value = MemoryFacetValue::Topic {
                normalized: ancestor,
                segments: segments[..ancestor_len].to_vec(),
            };
            expanded.push(MemoryFacet {
                facet_id: facet_id(
                    owner_record_id,
                    MemoryFacetNamespace::Topic,
                    &value,
                    "expanded",
                ),
                namespace: MemoryFacetNamespace::Topic,
                value,
                source_evidence_refs: exact.source_evidence_refs.clone(),
                derived_from_exact_facet_id: Some(exact.facet_id.clone()),
                expansion_rule_id: Some("topic_hierarchy_ancestor_v1".to_string()),
            });
        }
    }
    dedup_facets(&mut expanded);
    expanded
}

fn push_facet(
    facets: &mut Vec<MemoryFacet>,
    owner_record_id: &str,
    namespace: MemoryFacetNamespace,
    value: MemoryFacetValue,
    source_evidence_refs: Vec<CanonicalEvidenceRef>,
) {
    facets.push(MemoryFacet {
        facet_id: facet_id(owner_record_id, namespace, &value, "exact"),
        namespace,
        value,
        source_evidence_refs,
        derived_from_exact_facet_id: None,
        expansion_rule_id: None,
    });
}

fn facet_id(
    owner_record_id: &str,
    namespace: MemoryFacetNamespace,
    value: &MemoryFacetValue,
    tier: &str,
) -> String {
    let key = format!(
        "{}:{}:{}:{}",
        owner_record_id,
        namespace.label(),
        tier,
        value.canonical_key()
    );
    format!("facet:{}:{}:{}", namespace.label(), tier, stable_hash(&key))
}

fn canonicalize_evidence_refs(entry: &LongTermMemoryEntry) -> Vec<CanonicalEvidenceRef> {
    let mut refs = Vec::new();
    for citation in &entry.supporting_citations {
        if let Some(evidence) = canonical_evidence_ref(citation) {
            push_unique_evidence_ref(&mut refs, evidence);
        }
    }
    refs
}

fn canonical_evidence_ref(raw: &str) -> Option<CanonicalEvidenceRef> {
    let source_ref = raw.trim();
    if source_ref.is_empty() {
        return None;
    }
    let canonical_evidence_group = recall_evidence_group_key(source_ref);
    if canonical_evidence_group.is_empty() {
        return None;
    }
    Some(CanonicalEvidenceRef {
        source_ref: source_ref.to_string(),
        canonical_evidence_group,
        source_kind: source_kind(source_ref),
        source_authority_score: recall_source_authority_score(source_ref),
    })
}

fn synthetic_owner_evidence_ref(owner_record_id: &str) -> CanonicalEvidenceRef {
    let source_ref = format!("owner_record:{owner_record_id}");
    CanonicalEvidenceRef {
        canonical_evidence_group: source_ref.clone(),
        source_kind: "owner_record".to_string(),
        source_authority_score: recall_source_authority_score(&source_ref),
        source_ref,
    }
}

fn push_unique_evidence_ref(refs: &mut Vec<CanonicalEvidenceRef>, value: CanonicalEvidenceRef) {
    if !refs
        .iter()
        .any(|existing| existing.canonical_evidence_group == value.canonical_evidence_group)
    {
        refs.push(value);
    }
}

fn dedup_facets(facets: &mut Vec<MemoryFacet>) {
    facets.sort_by(|left, right| left.facet_id.cmp(&right.facet_id));
    facets.dedup_by(|left, right| left.facet_id == right.facet_id);
}

fn normalize_text_value(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn hierarchy_segments(value: &str) -> Vec<String> {
    value
        .split(['/', '>', ':', '|', '_'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_subject_ids(subject_ids: Vec<String>) -> Vec<String> {
    let mut normalized = subject_ids
        .into_iter()
        .map(|subject| subject.trim().to_string())
        .filter(|subject| !subject.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn source_kind(source_ref: &str) -> String {
    source_ref
        .split([':', '#', '/', '|'])
        .next()
        .unwrap_or("unstructured")
        .trim()
        .to_ascii_lowercase()
}

fn kind_value(kind: &LongTermMemoryKind) -> &'static str {
    kind.label()
}

fn source_scope_value(scope: LongTermMemorySourceScope) -> &'static str {
    match scope {
        LongTermMemorySourceScope::Chat => "chat",
        LongTermMemorySourceScope::User => "user",
        LongTermMemorySourceScope::World => "world",
    }
}

fn source_type_value(source_type: LongTermMemorySourceType) -> &'static str {
    match source_type {
        LongTermMemorySourceType::Conversation => "conversation",
        LongTermMemorySourceType::ManualTool => "manual_tool",
        LongTermMemorySourceType::SystemRuntime => "system_runtime",
        LongTermMemorySourceType::ExternalObservation => "external_observation",
    }
}

fn freshness_value(freshness: LongTermMemoryFreshness) -> &'static str {
    match freshness {
        LongTermMemoryFreshness::Stable => "stable",
        LongTermMemoryFreshness::Dynamic => "dynamic",
        LongTermMemoryFreshness::Volatile => "volatile",
    }
}

fn looks_like_regex_only(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && (trimmed.contains(".*")
            || trimmed.contains('[')
            || trimmed.contains(']')
            || trimmed.contains('(')
            || trimmed.contains(')')
            || trimmed.contains('*')
            || trimmed.contains('?')
            || trimmed.starts_with('/')
            || trimmed.ends_with('/'))
}

fn stable_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn accepted() -> MemoryFacetContractValidation {
    MemoryFacetContractValidation {
        accepted: true,
        reason: "accepted".to_string(),
    }
}

fn rejected(reason: &str) -> MemoryFacetContractValidation {
    MemoryFacetContractValidation {
        accepted: false,
        reason: reason.to_string(),
    }
}

fn parse_rejected(reason: &str) -> StructuredFacetParseOutcome {
    StructuredFacetParseOutcome {
        accepted: false,
        reason: reason.to_string(),
        entity: None,
        temporal_anchor: None,
    }
}
