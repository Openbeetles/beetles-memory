//! Governed typed facet index contract for memory recall.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use super::governed_post_image::{
    revision_is_exact_successor, GovernedDocumentImage, GovernedPostImageValidation,
};
use super::long_term::{
    canonical_evidence_ref_from_source, scoped_long_term_memory_storage_key, CanonicalEntityKey,
    CanonicalEntityRef, CanonicalEvidenceRef, LongTermMemoryEntry, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemorySourceScope, LongTermMemorySourceType,
};
use super::recall_anchor::{canonical_recall_evidence_group, recall_source_authority_score};
use super::MemoryPrivacyClass;

pub const MEMORY_FACET_INDEX_NAMESPACE: &str = "memory_facet_indexes";
pub const MEMORY_FACET_POSTING_NAMESPACE: &str = "memory_facet_postings";
pub const MEMORY_FACET_SCHEMA_VERSION: u32 = 3;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryFacetMatchKind {
    ExactValue,
    TopicSegment,
}

impl QueryFacetMatchKind {
    fn label(self) -> &'static str {
        match self {
            Self::ExactValue => "exact_value",
            Self::TopicSegment => "topic_segment",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QueryFacet {
    namespace: MemoryFacetNamespace,
    match_kind: QueryFacetMatchKind,
    canonical_value: String,
}

impl QueryFacet {
    pub const fn namespace(&self) -> MemoryFacetNamespace {
        self.namespace
    }

    pub const fn match_kind(&self) -> QueryFacetMatchKind {
        self.match_kind
    }

    pub fn canonical_value(&self) -> &str {
        &self.canonical_value
    }

    fn exact(namespace: MemoryFacetNamespace, value: &str) -> Option<Self> {
        Self::new(namespace, QueryFacetMatchKind::ExactValue, value)
    }

    fn topic_segment(value: &str) -> Option<Self> {
        Self::new(
            MemoryFacetNamespace::Topic,
            QueryFacetMatchKind::TopicSegment,
            value,
        )
    }

    fn new(
        namespace: MemoryFacetNamespace,
        match_kind: QueryFacetMatchKind,
        value: &str,
    ) -> Option<Self> {
        let canonical_value = normalize_text_value(value);
        if canonical_value.is_empty() || canonical_value.chars().any(char::is_control) {
            return None;
        }
        Some(Self {
            namespace,
            match_kind,
            canonical_value,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum QueryFacetInput {
    Kind(LongTermMemoryKind),
    TopicFull(String),
    TopicSegments(Vec<String>),
    Keyword(String),
    SourceScope(LongTermMemorySourceScope),
    SourceType(LongTermMemorySourceType),
    Freshness(LongTermMemoryFreshness),
    Entity(CanonicalEntityKey),
    Temporal(TemporalAnchor),
    UnresolvedEntity(String),
    UnresolvedTemporal(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryFacetParseOutcome {
    pub accepted: bool,
    pub reason: String,
    pub facets: Vec<QueryFacet>,
}

pub struct QueryFacetParser;

impl QueryFacetParser {
    pub fn parse(input: QueryFacetInput) -> QueryFacetParseOutcome {
        let facets = match input {
            QueryFacetInput::Kind(kind) => {
                one_query_facet(MemoryFacetNamespace::Kind, kind_value(&kind))
            }
            QueryFacetInput::TopicFull(topic) => {
                one_query_facet(MemoryFacetNamespace::Topic, &topic)
            }
            QueryFacetInput::TopicSegments(segments) => {
                let mut facets = Vec::new();
                for segment in segments {
                    if let Some(facet) = QueryFacet::topic_segment(&segment) {
                        push_unique_query_facet(&mut facets, facet);
                    }
                }
                facets
            }
            QueryFacetInput::Keyword(keyword) => {
                one_query_facet(MemoryFacetNamespace::Keyword, &keyword)
            }
            QueryFacetInput::SourceScope(scope) => {
                one_query_facet(MemoryFacetNamespace::SourceScope, source_scope_value(scope))
            }
            QueryFacetInput::SourceType(source_type) => one_query_facet(
                MemoryFacetNamespace::SourceType,
                source_type_value(source_type),
            ),
            QueryFacetInput::Freshness(freshness) => {
                one_query_facet(MemoryFacetNamespace::Freshness, freshness_value(freshness))
            }
            QueryFacetInput::Entity(key) => {
                let canonical_id = normalize_text_value(&key.canonical_id);
                if canonical_id.is_empty() {
                    return rejected_query_facets("entity_query_facet_typed_anchor_invalid");
                }
                one_query_facet(
                    MemoryFacetNamespace::Entity,
                    &format!("{}:{canonical_id}", canonical_entity_kind_value(key.kind)),
                )
            }
            QueryFacetInput::Temporal(anchor) => {
                if anchor.epoch_secs == 0 || !canonical_evidence_ref_is_valid(&anchor.evidence_ref)
                {
                    return rejected_query_facets("temporal_query_facet_typed_anchor_invalid");
                }
                one_query_facet(
                    MemoryFacetNamespace::Temporal,
                    &temporal_lookup_value(&anchor),
                )
            }
            QueryFacetInput::UnresolvedEntity(_) => {
                return rejected_query_facets("entity_query_facet_requires_typed_anchor")
            }
            QueryFacetInput::UnresolvedTemporal(_) => {
                return rejected_query_facets("temporal_query_facet_requires_typed_anchor")
            }
        };
        if facets.is_empty() {
            return rejected_query_facets("query_facet_canonical_value_empty");
        }
        QueryFacetParseOutcome {
            accepted: true,
            reason: "accepted".to_string(),
            facets,
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
    EntityAlias {
        key: CanonicalEntityKey,
        alias: String,
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
            Self::Entity { entity } => canonical_entity_key_value(&entity.key),
            Self::EntityAlias { key, alias } => {
                format!("{}:alias:{}", canonical_entity_key_value(key), alias)
            }
            Self::GraphAnchor { anchor_id } => anchor_id.clone(),
        }
    }

    fn posting_facets(&self) -> Vec<QueryFacet> {
        let mut facets = Vec::new();
        match self {
            Self::Kind { normalized }
            | Self::Keyword { normalized }
            | Self::SourceScope { normalized }
            | Self::SourceType { normalized }
            | Self::Freshness { normalized } => {
                if let Some(facet) = QueryFacet::exact(self.namespace(), normalized) {
                    facets.push(facet);
                }
            }
            Self::Topic {
                normalized,
                segments,
            } => {
                if let Some(facet) = QueryFacet::exact(MemoryFacetNamespace::Topic, normalized) {
                    facets.push(facet);
                }
                for segment in segments {
                    if let Some(facet) = QueryFacet::topic_segment(segment) {
                        push_unique_query_facet(&mut facets, facet);
                    }
                }
            }
            Self::Temporal { anchor } => {
                if let Some(facet) = QueryFacet::exact(
                    MemoryFacetNamespace::Temporal,
                    &temporal_lookup_value(anchor),
                ) {
                    facets.push(facet);
                }
            }
            Self::Evidence { .. } => {}
            Self::Entity { entity } => {
                if let Some(facet) = QueryFacet::exact(
                    MemoryFacetNamespace::Entity,
                    &canonical_entity_key_value(&entity.key),
                ) {
                    facets.push(facet);
                }
            }
            Self::EntityAlias { .. } => {}
            Self::GraphAnchor { anchor_id } => {
                if let Some(facet) = QueryFacet::exact(MemoryFacetNamespace::GraphAnchor, anchor_id)
                {
                    facets.push(facet);
                }
            }
        }
        facets
    }

    fn namespace(&self) -> MemoryFacetNamespace {
        match self {
            Self::Kind { .. } => MemoryFacetNamespace::Kind,
            Self::Topic { .. } => MemoryFacetNamespace::Topic,
            Self::Keyword { .. } => MemoryFacetNamespace::Keyword,
            Self::SourceScope { .. } => MemoryFacetNamespace::SourceScope,
            Self::SourceType { .. } => MemoryFacetNamespace::SourceType,
            Self::Freshness { .. } => MemoryFacetNamespace::Freshness,
            Self::Temporal { .. } => MemoryFacetNamespace::Temporal,
            Self::Evidence { .. } => MemoryFacetNamespace::Evidence,
            Self::Entity { .. } => MemoryFacetNamespace::Entity,
            Self::EntityAlias { .. } => MemoryFacetNamespace::Entity,
            Self::GraphAnchor { .. } => MemoryFacetNamespace::GraphAnchor,
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

impl MemoryFacet {
    pub fn matches_query_facet(&self, query: &QueryFacet) -> bool {
        self.value
            .posting_facets()
            .iter()
            .any(|posting_facet| posting_facet == query)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFacetIndexDoc {
    pub owner_record_id: String,
    pub owner_plane: MemoryFacetOwnerPlane,
    pub schema_version: u32,
    pub owner_revision: u64,
    pub facet_index_revision: u64,
    pub memory_space_id: String,
    pub subject_ids: Vec<String>,
    pub privacy: MemoryPrivacyClass,
    pub status: MemoryFacetStatus,
    pub exact_facets: Vec<MemoryFacet>,
    pub expanded_facets: Vec<MemoryFacet>,
    pub canonical_evidence_refs: Vec<CanonicalEvidenceRef>,
    pub updated_at: u64,
}

impl MemoryFacetIndexDoc {
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
            owner_revision: self.owner_revision,
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

    pub fn posting_keys_for_subject(
        &self,
        subject_id: &str,
    ) -> Result<Vec<String>, MemoryFacetValidationError> {
        let subject_id = validate_memory_facet_scope(&self.memory_space_id, subject_id)?.1;
        if !self.subject_ids.iter().any(|mounted| mounted == subject_id) {
            return Err(MemoryFacetValidationError::SubjectNotMounted);
        }
        let mut keys = self
            .exact_facets
            .iter()
            .chain(self.expanded_facets.iter())
            .flat_map(|facet| facet.value.posting_facets())
            .map(|facet| memory_facet_posting_key(&self.memory_space_id, subject_id, &facet))
            .collect::<Result<Vec<_>, _>>()?;
        keys.sort();
        keys.dedup();
        Ok(keys)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryFacetOwnerVersion {
    pub owner_record_id: String,
    pub owner_revision: u64,
    pub facet_index_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryFacetPostingRevision {
    pub posting_key: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFacetPostingDoc {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub subject_id: String,
    pub posting_key: String,
    pub revision: u64,
    pub owner_versions: Vec<MemoryFacetOwnerVersion>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFacetIndexManifest {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub subject_id: String,
    pub owner_doc_count: usize,
    pub posting_doc_count: usize,
    pub revision: u64,
    pub owner_versions: Vec<MemoryFacetOwnerVersion>,
    pub posting_revisions: Vec<MemoryFacetPostingRevision>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryFacetPostImageClosure {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub owner_records: Vec<GovernedDocumentImage<LongTermMemoryEntry>>,
    pub facet_owners: Vec<GovernedDocumentImage<MemoryFacetIndexDoc>>,
    pub postings: Vec<GovernedDocumentImage<MemoryFacetPostingDoc>>,
    pub manifest: GovernedDocumentImage<MemoryFacetIndexManifest>,
}

pub fn validate_memory_facet_post_image(
    closure: &MemoryFacetPostImageClosure,
) -> GovernedPostImageValidation {
    let memory_space_id = closure.memory_space_id.trim();
    let subject_id = closure.mounted_subject_id.trim();
    let mut failures = Vec::new();
    if validate_memory_facet_scope(memory_space_id, subject_id).is_err() {
        failures.push("memory_facet_post_image_scope_invalid".to_string());
        return GovernedPostImageValidation::from_failures(failures);
    }

    let mut owners = BTreeMap::new();
    for image in &closure.owner_records {
        let logical_id = image
            .after
            .as_ref()
            .or(image.before.as_ref())
            .map(|owner| owner.id.as_str())
            .unwrap_or_default();
        if scoped_long_term_memory_storage_key(memory_space_id, logical_id)
            .map(|expected| image.physical_key != expected)
            .unwrap_or(true)
        {
            failures.push("memory_facet_owner_physical_key_drift".to_string());
        }
        if image.before != image.after
            && !revision_is_exact_successor(
                image.before.as_ref().map(|owner| owner.owner_revision),
                image.after.as_ref().map(|owner| owner.owner_revision),
            )
        {
            failures.push("memory_facet_owner_revision_successor_drift".to_string());
        }
        if let Some(owner) = image.after.as_ref() {
            if owners.insert(owner.id.as_str(), owner).is_some() {
                failures.push("memory_facet_owner_duplicate".to_string());
            }
        }
    }

    let mut facet_owners = BTreeMap::new();
    for image in &closure.facet_owners {
        let logical_id = image
            .after
            .as_ref()
            .or(image.before.as_ref())
            .map(|owner| owner.owner_record_id.as_str())
            .unwrap_or_default();
        let expected_key =
            scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, logical_id);
        if expected_key.as_ref().is_err()
            || expected_key
                .as_ref()
                .is_ok_and(|key| key != &image.physical_key)
        {
            failures.push("memory_facet_owner_doc_physical_key_drift".to_string());
        }
        if image.before != image.after
            && !revision_is_exact_successor(
                image
                    .before
                    .as_ref()
                    .map(|owner| owner.facet_index_revision),
                image.after.as_ref().map(|owner| owner.facet_index_revision),
            )
        {
            failures.push("memory_facet_index_revision_successor_drift".to_string());
        }
        let Some(facet_owner) = image.after.as_ref() else {
            continue;
        };
        if facet_owner.schema_version != MEMORY_FACET_SCHEMA_VERSION
            || facet_owner.memory_space_id != memory_space_id
            || facet_owner.owner_plane != MemoryFacetOwnerPlane::LongTerm
            || !facet_owner
                .subject_ids
                .iter()
                .any(|mounted| mounted == subject_id)
            || facet_owner.status != MemoryFacetStatus::Active
        {
            failures.push("memory_facet_owner_doc_scope_or_status_drift".to_string());
        }
        match owners.get(facet_owner.owner_record_id.as_str()) {
            Some(owner)
                if owner.owner_revision == facet_owner.owner_revision
                    && owner.privacy == facet_owner.privacy => {}
            _ => failures.push("memory_facet_owner_doc_owner_binding_drift".to_string()),
        }
        if facet_owners
            .insert(facet_owner.owner_record_id.as_str(), facet_owner)
            .is_some()
        {
            failures.push("memory_facet_owner_doc_duplicate".to_string());
        }
    }

    let expected_owner_versions = facet_owners
        .values()
        .filter(|owner| owner.privacy.projection_content_allowed())
        .map(|owner| MemoryFacetOwnerVersion {
            owner_record_id: owner.owner_record_id.clone(),
            owner_revision: owner.owner_revision,
            facet_index_revision: owner.facet_index_revision,
        })
        .collect::<Vec<_>>();
    if owners.keys().copied().collect::<BTreeSet<_>>()
        != facet_owners.keys().copied().collect::<BTreeSet<_>>()
    {
        failures.push("memory_facet_owner_doc_exact_closure_drift".to_string());
    }
    let mut expected_posting_owners: BTreeMap<String, Vec<MemoryFacetOwnerVersion>> =
        BTreeMap::new();
    for owner in facet_owners
        .values()
        .filter(|owner| owner.privacy.projection_content_allowed())
    {
        match owner.posting_keys_for_subject(subject_id) {
            Ok(keys) => {
                for key in keys {
                    expected_posting_owners
                        .entry(key)
                        .or_default()
                        .push(MemoryFacetOwnerVersion {
                            owner_record_id: owner.owner_record_id.clone(),
                            owner_revision: owner.owner_revision,
                            facet_index_revision: owner.facet_index_revision,
                        });
                }
            }
            Err(_) => failures.push("memory_facet_owner_posting_derivation_failed".to_string()),
        }
    }
    for owners in expected_posting_owners.values_mut() {
        owners.sort();
        owners.dedup();
    }

    let mut postings = BTreeMap::new();
    for image in &closure.postings {
        let logical_key = image
            .after
            .as_ref()
            .or(image.before.as_ref())
            .map(|posting| posting.posting_key.as_str())
            .unwrap_or_default();
        if image.physical_key != logical_key {
            failures.push("memory_facet_posting_physical_key_drift".to_string());
        }
        if image.before != image.after
            && !revision_is_exact_successor(
                image.before.as_ref().map(|posting| posting.revision),
                image.after.as_ref().map(|posting| posting.revision),
            )
        {
            failures.push("memory_facet_posting_revision_successor_drift".to_string());
        }
        let Some(posting) = image.after.as_ref() else {
            continue;
        };
        if validate_memory_facet_posting_doc(posting, memory_space_id, subject_id).is_err() {
            failures.push("memory_facet_posting_scope_or_schema_drift".to_string());
        }
        if expected_posting_owners.get(&posting.posting_key) != Some(&posting.owner_versions) {
            failures.push("memory_facet_posting_owner_exact_closure_drift".to_string());
        }
        if postings
            .insert(posting.posting_key.as_str(), posting)
            .is_some()
        {
            failures.push("memory_facet_posting_duplicate".to_string());
        }
    }
    let expected_posting_keys = expected_posting_owners
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let actual_posting_keys = postings
        .keys()
        .map(|key| (*key).to_string())
        .collect::<BTreeSet<_>>();
    if expected_posting_keys != actual_posting_keys {
        failures.push("memory_facet_posting_exact_closure_drift".to_string());
    }

    let expected_manifest_key = memory_facet_manifest_key(memory_space_id, subject_id);
    if expected_manifest_key.as_ref().is_err()
        || expected_manifest_key
            .as_ref()
            .is_ok_and(|key| key != &closure.manifest.physical_key)
    {
        failures.push("memory_facet_manifest_physical_key_drift".to_string());
    }
    if closure.manifest.before != closure.manifest.after
        && !revision_is_exact_successor(
            closure
                .manifest
                .before
                .as_ref()
                .map(|manifest| manifest.revision),
            closure
                .manifest
                .after
                .as_ref()
                .map(|manifest| manifest.revision),
        )
    {
        failures.push("memory_facet_manifest_revision_successor_drift".to_string());
    }
    match closure.manifest.after.as_ref() {
        None if expected_owner_versions.is_empty() && postings.is_empty() => {}
        None => failures.push("memory_facet_manifest_missing".to_string()),
        Some(manifest) => {
            if validate_memory_facet_manifest_doc(manifest, memory_space_id, subject_id).is_err() {
                failures.push("memory_facet_manifest_scope_or_schema_drift".to_string());
            }
            let expected_posting_revisions = postings
                .values()
                .map(|posting| MemoryFacetPostingRevision {
                    posting_key: posting.posting_key.clone(),
                    revision: posting.revision,
                })
                .collect::<Vec<_>>();
            if manifest.owner_versions != expected_owner_versions
                || manifest.posting_revisions != expected_posting_revisions
                || manifest.owner_doc_count != expected_owner_versions.len()
                || manifest.posting_doc_count != expected_posting_revisions.len()
            {
                failures.push("memory_facet_manifest_exact_closure_drift".to_string());
            }
        }
    }

    GovernedPostImageValidation::from_failures(failures)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFacetValidationError {
    MemorySpaceIdEmpty,
    SubjectIdEmpty,
    SubjectNotMounted,
    ManifestSchemaVersionMismatch,
    ManifestScopeMismatch,
    ManifestRevisionInvalid,
    ManifestOwnerCountMismatch,
    ManifestPostingCountMismatch,
    ManifestOwnerMembershipInvalid,
    ManifestPostingMembershipInvalid,
    ManifestOwnerMembershipMissing,
    ManifestPostingMembershipMissing,
    PostingSchemaVersionMismatch,
    PostingScopeMismatch,
    PostingRevisionInvalid,
    PostingRevisionMismatch,
    PostingOwnerMembershipInvalid,
    PostingOwnerMembershipMissing,
    PostingOwnerVersionMismatch,
    OwnerSchemaVersionMismatch,
    OwnerScopeMismatch,
    OwnerVersionMismatch,
    OwnerStatusMismatch,
    OwnerPostingMembershipMismatch,
}

impl MemoryFacetValidationError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::MemorySpaceIdEmpty => "memory_facet_memory_space_id_empty",
            Self::SubjectIdEmpty => "memory_facet_subject_id_empty",
            Self::SubjectNotMounted => "memory_facet_subject_not_mounted",
            Self::ManifestSchemaVersionMismatch => "memory_facet_manifest_schema_version_mismatch",
            Self::ManifestScopeMismatch => "memory_facet_manifest_scope_mismatch",
            Self::ManifestRevisionInvalid => "memory_facet_manifest_revision_invalid",
            Self::ManifestOwnerCountMismatch => {
                "memory_facet_manifest_owner_count_membership_mismatch"
            }
            Self::ManifestPostingCountMismatch => {
                "memory_facet_manifest_posting_count_membership_mismatch"
            }
            Self::ManifestOwnerMembershipInvalid => {
                "memory_facet_manifest_owner_membership_invalid"
            }
            Self::ManifestPostingMembershipInvalid => {
                "memory_facet_manifest_posting_membership_invalid"
            }
            Self::ManifestOwnerMembershipMissing => {
                "memory_facet_manifest_owner_membership_missing"
            }
            Self::ManifestPostingMembershipMissing => {
                "memory_facet_manifest_posting_membership_missing"
            }
            Self::PostingSchemaVersionMismatch => "memory_facet_posting_schema_version_mismatch",
            Self::PostingScopeMismatch => "memory_facet_posting_scope_mismatch",
            Self::PostingRevisionInvalid => "memory_facet_posting_revision_invalid",
            Self::PostingRevisionMismatch => "memory_facet_posting_revision_mismatch",
            Self::PostingOwnerMembershipInvalid => "memory_facet_posting_owner_membership_invalid",
            Self::PostingOwnerMembershipMissing => "memory_facet_posting_owner_membership_missing",
            Self::PostingOwnerVersionMismatch => "memory_facet_posting_owner_version_mismatch",
            Self::OwnerSchemaVersionMismatch => "memory_facet_owner_schema_version_mismatch",
            Self::OwnerScopeMismatch => "memory_facet_owner_scope_mismatch",
            Self::OwnerVersionMismatch => "memory_facet_owner_version_mismatch",
            Self::OwnerStatusMismatch => "memory_facet_owner_status_mismatch",
            Self::OwnerPostingMembershipMismatch => {
                "memory_facet_owner_posting_membership_mismatch"
            }
        }
    }
}

impl std::fmt::Display for MemoryFacetValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for MemoryFacetValidationError {}

pub fn memory_facet_manifest_key(
    memory_space_id: &str,
    subject_id: &str,
) -> Result<String, MemoryFacetValidationError> {
    let (memory_space_id, subject_id) = validate_memory_facet_scope(memory_space_id, subject_id)?;
    let key = format!(
        "v3|{}:{}|{}:{}",
        memory_space_id.len(),
        memory_space_id,
        subject_id.len(),
        subject_id,
    );
    Ok(format!("facet-manifest:{}", stable_hash(&key)))
}

pub fn scoped_memory_facet_owner_storage_key(
    memory_space_id: &str,
    subject_id: &str,
    logical_owner_id: &str,
) -> Result<String, MemoryFacetValidationError> {
    let (memory_space_id, subject_id) = validate_memory_facet_scope(memory_space_id, subject_id)?;
    let logical_owner_id = logical_owner_id.trim();
    if logical_owner_id.is_empty() {
        return Err(MemoryFacetValidationError::OwnerScopeMismatch);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"memory_facet_owner_storage_v1");
    for field in [memory_space_id, subject_id, logical_owner_id] {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field.as_bytes());
    }
    Ok(format!("facet-owner:{:x}", hasher.finalize()))
}

pub fn memory_facet_posting_key(
    memory_space_id: &str,
    subject_id: &str,
    facet: &QueryFacet,
) -> Result<String, MemoryFacetValidationError> {
    let (memory_space_id, subject_id) = validate_memory_facet_scope(memory_space_id, subject_id)?;
    let key = format!(
        "v3|{}:{}|{}:{}|{}|{}|{}:{}",
        memory_space_id.len(),
        memory_space_id,
        subject_id.len(),
        subject_id,
        facet.namespace.label(),
        facet.match_kind.label(),
        facet.canonical_value.len(),
        facet.canonical_value,
    );
    Ok(format!("facet-posting:{}", stable_hash(&key)))
}

pub fn validate_memory_facet_read_chain(
    memory_space_id: &str,
    subject_id: &str,
    manifest: &MemoryFacetIndexManifest,
    posting: &MemoryFacetPostingDoc,
    owner: &MemoryFacetIndexDoc,
) -> Result<(), MemoryFacetValidationError> {
    let (memory_space_id, subject_id) = validate_memory_facet_scope(memory_space_id, subject_id)?;
    validate_memory_facet_posting(memory_space_id, subject_id, manifest, posting)?;

    let manifest_owner = manifest
        .owner_versions
        .iter()
        .find(|membership| membership.owner_record_id == owner.owner_record_id)
        .ok_or(MemoryFacetValidationError::ManifestOwnerMembershipMissing)?;
    let posting_owner = posting
        .owner_versions
        .iter()
        .find(|membership| membership.owner_record_id == owner.owner_record_id)
        .ok_or(MemoryFacetValidationError::PostingOwnerMembershipMissing)?;

    if owner.schema_version != MEMORY_FACET_SCHEMA_VERSION {
        return Err(MemoryFacetValidationError::OwnerSchemaVersionMismatch);
    }
    if owner.memory_space_id != memory_space_id
        || !owner
            .subject_ids
            .iter()
            .any(|mounted| mounted == subject_id)
    {
        return Err(MemoryFacetValidationError::OwnerScopeMismatch);
    }
    if owner.owner_revision != manifest_owner.owner_revision
        || owner.owner_revision != posting_owner.owner_revision
        || owner.facet_index_revision != manifest_owner.facet_index_revision
        || owner.facet_index_revision != posting_owner.facet_index_revision
    {
        return Err(MemoryFacetValidationError::OwnerVersionMismatch);
    }
    if owner.status != MemoryFacetStatus::Active {
        return Err(MemoryFacetValidationError::OwnerStatusMismatch);
    }
    if !owner
        .posting_keys_for_subject(subject_id)?
        .iter()
        .any(|key| key == &posting.posting_key)
    {
        return Err(MemoryFacetValidationError::OwnerPostingMembershipMismatch);
    }

    Ok(())
}

pub fn validate_memory_facet_manifest(
    memory_space_id: &str,
    subject_id: &str,
    manifest: &MemoryFacetIndexManifest,
) -> Result<(), MemoryFacetValidationError> {
    let (memory_space_id, subject_id) = validate_memory_facet_scope(memory_space_id, subject_id)?;
    validate_memory_facet_manifest_doc(manifest, memory_space_id, subject_id)
}

pub fn validate_memory_facet_posting(
    memory_space_id: &str,
    subject_id: &str,
    manifest: &MemoryFacetIndexManifest,
    posting: &MemoryFacetPostingDoc,
) -> Result<(), MemoryFacetValidationError> {
    let (memory_space_id, subject_id) = validate_memory_facet_scope(memory_space_id, subject_id)?;
    validate_memory_facet_manifest_doc(manifest, memory_space_id, subject_id)?;
    validate_memory_facet_posting_doc(posting, memory_space_id, subject_id)?;

    let manifest_posting = manifest
        .posting_revisions
        .iter()
        .find(|membership| membership.posting_key == posting.posting_key)
        .ok_or(MemoryFacetValidationError::ManifestPostingMembershipMissing)?;
    if manifest_posting.revision != posting.revision {
        return Err(MemoryFacetValidationError::PostingRevisionMismatch);
    }

    for posting_owner in &posting.owner_versions {
        let manifest_owner = manifest
            .owner_versions
            .iter()
            .find(|membership| membership.owner_record_id == posting_owner.owner_record_id)
            .ok_or(MemoryFacetValidationError::ManifestOwnerMembershipMissing)?;
        if manifest_owner.owner_revision != posting_owner.owner_revision
            || manifest_owner.facet_index_revision != posting_owner.facet_index_revision
        {
            return Err(MemoryFacetValidationError::PostingOwnerVersionMismatch);
        }
    }

    Ok(())
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
    pub owner_revision: u64,
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
    pub fn parse_entity_anchor(_raw: &str, _evidence_ref: &str) -> StructuredFacetParseOutcome {
        parse_rejected("entity_facet_requires_canonical_entity_ref")
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

    for entity in &entry.canonical_entities {
        push_facet(
            &mut exact_facets,
            &owner_record_id,
            MemoryFacetNamespace::Entity,
            MemoryFacetValue::Entity {
                entity: entity.clone(),
            },
            entity.evidence_refs.clone(),
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
        owner_revision: entry.owner_revision,
        facet_index_revision,
        memory_space_id: memory_space_id.into(),
        subject_ids: normalize_subject_ids(subject_ids),
        privacy: entry.privacy,
        status: MemoryFacetStatus::Active,
        exact_facets,
        expanded_facets,
        canonical_evidence_refs,
        updated_at: entry.updated_at.max(entry.created_at),
    }
}

fn build_expanded_facets(owner_record_id: &str, exact_facets: &[MemoryFacet]) -> Vec<MemoryFacet> {
    let mut expanded = Vec::new();
    for exact in exact_facets {
        match &exact.value {
            MemoryFacetValue::Topic { segments, .. } if segments.len() >= 2 => {
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
            MemoryFacetValue::Entity { entity } => {
                for alias in &entity.aliases {
                    let value = MemoryFacetValue::EntityAlias {
                        key: entity.key.clone(),
                        alias: alias.clone(),
                    };
                    expanded.push(MemoryFacet {
                        facet_id: facet_id(
                            owner_record_id,
                            MemoryFacetNamespace::Entity,
                            &value,
                            "expanded",
                        ),
                        namespace: MemoryFacetNamespace::Entity,
                        value,
                        source_evidence_refs: entity.evidence_refs.clone(),
                        derived_from_exact_facet_id: Some(exact.facet_id.clone()),
                        expansion_rule_id: Some("canonical_entity_alias_v1".to_string()),
                    });
                }
            }
            _ => {}
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
    canonical_evidence_ref_from_source(raw)
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

fn one_query_facet(namespace: MemoryFacetNamespace, value: &str) -> Vec<QueryFacet> {
    QueryFacet::exact(namespace, value).into_iter().collect()
}

fn push_unique_query_facet(facets: &mut Vec<QueryFacet>, facet: QueryFacet) {
    if !facets.iter().any(|existing| existing == &facet) {
        facets.push(facet);
    }
}

fn temporal_lookup_value(anchor: &TemporalAnchor) -> String {
    format!(
        "{}:{}:{}",
        anchor.anchor_kind.label(),
        anchor.epoch_secs,
        anchor.precision.label()
    )
}

fn canonical_evidence_ref_is_valid(evidence: &CanonicalEvidenceRef) -> bool {
    canonical_evidence_ref_from_source(&evidence.source_ref).as_ref() == Some(evidence)
}

fn rejected_query_facets(reason: &str) -> QueryFacetParseOutcome {
    QueryFacetParseOutcome {
        accepted: false,
        reason: reason.to_string(),
        facets: Vec::new(),
    }
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

fn canonical_entity_key_value(key: &CanonicalEntityKey) -> String {
    format!(
        "{}:{}",
        canonical_entity_kind_value(key.kind),
        normalize_text_value(&key.canonical_id)
    )
}

fn canonical_entity_kind_value(kind: super::CanonicalEntityKind) -> &'static str {
    match kind {
        super::CanonicalEntityKind::Person => "person",
        super::CanonicalEntityKind::Agent => "agent",
        super::CanonicalEntityKind::Organization => "organization",
        super::CanonicalEntityKind::Project => "project",
        super::CanonicalEntityKind::Product => "product",
        super::CanonicalEntityKind::Place => "place",
        super::CanonicalEntityKind::Service => "service",
        super::CanonicalEntityKind::System => "system",
        super::CanonicalEntityKind::Repository => "repository",
        super::CanonicalEntityKind::Document => "document",
        super::CanonicalEntityKind::Event => "event",
        super::CanonicalEntityKind::Concept => "concept",
    }
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

fn validate_memory_facet_scope<'a>(
    memory_space_id: &'a str,
    subject_id: &'a str,
) -> Result<(&'a str, &'a str), MemoryFacetValidationError> {
    let memory_space_id = memory_space_id.trim();
    if memory_space_id.is_empty() {
        return Err(MemoryFacetValidationError::MemorySpaceIdEmpty);
    }
    let subject_id = subject_id.trim();
    if subject_id.is_empty() {
        return Err(MemoryFacetValidationError::SubjectIdEmpty);
    }
    Ok((memory_space_id, subject_id))
}

fn validate_memory_facet_manifest_doc(
    manifest: &MemoryFacetIndexManifest,
    memory_space_id: &str,
    subject_id: &str,
) -> Result<(), MemoryFacetValidationError> {
    if manifest.schema_version != MEMORY_FACET_SCHEMA_VERSION {
        return Err(MemoryFacetValidationError::ManifestSchemaVersionMismatch);
    }
    if manifest.memory_space_id != memory_space_id || manifest.subject_id != subject_id {
        return Err(MemoryFacetValidationError::ManifestScopeMismatch);
    }
    if manifest.revision == 0 {
        return Err(MemoryFacetValidationError::ManifestRevisionInvalid);
    }
    if manifest.owner_doc_count != manifest.owner_versions.len() {
        return Err(MemoryFacetValidationError::ManifestOwnerCountMismatch);
    }
    if manifest.posting_doc_count != manifest.posting_revisions.len() {
        return Err(MemoryFacetValidationError::ManifestPostingCountMismatch);
    }
    if manifest.owner_versions.is_empty() {
        return Err(MemoryFacetValidationError::ManifestOwnerMembershipMissing);
    }
    if manifest.posting_revisions.is_empty() {
        return Err(MemoryFacetValidationError::ManifestPostingMembershipMissing);
    }
    if !owner_versions_are_valid(&manifest.owner_versions) {
        return Err(MemoryFacetValidationError::ManifestOwnerMembershipInvalid);
    }
    if !posting_revisions_are_valid(&manifest.posting_revisions) {
        return Err(MemoryFacetValidationError::ManifestPostingMembershipInvalid);
    }
    Ok(())
}

fn validate_memory_facet_posting_doc(
    posting: &MemoryFacetPostingDoc,
    memory_space_id: &str,
    subject_id: &str,
) -> Result<(), MemoryFacetValidationError> {
    if posting.schema_version != MEMORY_FACET_SCHEMA_VERSION {
        return Err(MemoryFacetValidationError::PostingSchemaVersionMismatch);
    }
    if posting.memory_space_id != memory_space_id || posting.subject_id != subject_id {
        return Err(MemoryFacetValidationError::PostingScopeMismatch);
    }
    if posting.revision == 0 {
        return Err(MemoryFacetValidationError::PostingRevisionInvalid);
    }
    if posting.owner_versions.is_empty() {
        return Err(MemoryFacetValidationError::PostingOwnerMembershipMissing);
    }
    if !owner_versions_are_valid(&posting.owner_versions) {
        return Err(MemoryFacetValidationError::PostingOwnerMembershipInvalid);
    }
    Ok(())
}

fn owner_versions_are_valid(memberships: &[MemoryFacetOwnerVersion]) -> bool {
    memberships.iter().all(|membership| {
        !membership.owner_record_id.trim().is_empty()
            && membership.owner_revision > 0
            && membership.facet_index_revision > 0
    }) && memberships
        .windows(2)
        .all(|pair| pair[0].owner_record_id < pair[1].owner_record_id)
}

fn posting_revisions_are_valid(memberships: &[MemoryFacetPostingRevision]) -> bool {
    memberships
        .iter()
        .all(|membership| !membership.posting_key.trim().is_empty() && membership.revision > 0)
        && memberships
            .windows(2)
            .all(|pair| pair[0].posting_key < pair[1].posting_key)
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
