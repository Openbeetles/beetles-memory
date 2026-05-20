use crate::{
    CrossPlaneRerankReport, MemoryPlane, ProjectionBlock, ProjectionSurface, PromptRecallIntent,
    RecallQuery, RecallSelectionReport, RuntimeProfile,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallAssemblyActiveContext {
    pub active_task: Option<String>,
    pub summary: Option<String>,
    pub recent_grounding: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallAssemblyLimits {
    pub per_plane_limit: usize,
    pub max_blocks: usize,
    pub max_bytes: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectionRedactionPolicy {
    pub private_fragments: Vec<String>,
    pub identifier_fragments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallAssemblyRequest {
    pub identity: String,
    pub scope: String,
    pub raw_query: String,
    pub normalized_query: String,
    pub surface: ProjectionSurface,
    pub profile: RuntimeProfile,
    pub intent_hint: Option<PromptRecallIntent>,
    pub exact_lookup: bool,
    pub active_context: RecallAssemblyActiveContext,
    pub limits: RecallAssemblyLimits,
    pub redaction: ProjectionRedactionPolicy,
}

impl RecallAssemblyRequest {
    pub fn new(
        identity: impl Into<String>,
        scope: impl Into<String>,
        raw_query: impl Into<String>,
        surface: ProjectionSurface,
        profile: RuntimeProfile,
    ) -> Self {
        let raw_query = raw_query.into();
        let budget = profile.projection_budget_profile(surface);
        Self {
            identity: identity.into(),
            scope: scope.into(),
            normalized_query: normalize_query(&raw_query),
            raw_query,
            surface,
            profile,
            intent_hint: None,
            exact_lookup: false,
            active_context: RecallAssemblyActiveContext {
                active_task: None,
                summary: None,
                recent_grounding: None,
            },
            limits: RecallAssemblyLimits {
                per_plane_limit: 4,
                max_blocks: 12,
                max_bytes: budget.total_bytes,
            },
            redaction: ProjectionRedactionPolicy::default(),
        }
    }

    pub fn intent_hint(mut self, intent: PromptRecallIntent) -> Self {
        self.intent_hint = Some(intent);
        self
    }

    pub fn exact_lookup(mut self, exact_lookup: bool) -> Self {
        self.exact_lookup = exact_lookup;
        self
    }

    pub fn active_task(mut self, active_task: impl Into<String>) -> Self {
        self.active_context.active_task = Some(active_task.into());
        self
    }

    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.active_context.summary = Some(summary.into());
        self
    }

    pub fn recent_grounding(mut self, recent_grounding: impl Into<String>) -> Self {
        self.active_context.recent_grounding = Some(recent_grounding.into());
        self
    }

    pub fn limit(mut self, per_plane_limit: usize) -> Self {
        self.limits.per_plane_limit = per_plane_limit.max(1);
        self
    }

    pub fn max_blocks(mut self, max_blocks: usize) -> Self {
        self.limits.max_blocks = max_blocks.max(1);
        self
    }

    pub fn max_bytes(mut self, max_bytes: usize) -> Self {
        self.limits.max_bytes = max_bytes.max(1);
        self
    }

    pub fn redact_fragment(mut self, fragment: impl Into<String>) -> Self {
        let fragment = fragment.into();
        if !fragment.trim().is_empty() {
            self.redaction.private_fragments.push(fragment);
        }
        self
    }

    pub fn redact_identifier(mut self, fragment: impl Into<String>) -> Self {
        let fragment = fragment.into();
        if !fragment.trim().is_empty() {
            self.redaction.identifier_fragments.push(fragment);
        }
        self
    }

    pub fn recall_query(&self, intent: PromptRecallIntent, plane: MemoryPlane) -> RecallQuery {
        RecallQuery::new(self.scope.clone())
            .identity(self.identity.clone())
            .plane(plane)
            .intent(intent)
            .limit(self.limits.per_plane_limit)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionBudgetProfile {
    pub surface: ProjectionSurface,
    pub total_bytes: usize,
    pub constitutional_bytes: usize,
    pub active_task_bytes: usize,
    pub governed_memory_bytes: usize,
    pub background_governance_bytes: usize,
    pub block_bytes: usize,
    pub deep_inspection: bool,
}

impl RuntimeProfile {
    pub fn projection_budget_profile(self, surface: ProjectionSurface) -> ProjectionBudgetProfile {
        let base = self.projection_budget_bytes();
        let surface_total = match surface {
            ProjectionSurface::Prompt => base,
            ProjectionSurface::ToolContext => (base / 2).max(256),
            ProjectionSurface::OperatorInspection => base,
            ProjectionSurface::Adapter => (base / 2).max(256),
            ProjectionSurface::Replay => base,
        };
        let compact = matches!(self, Self::EspCompact | Self::SdkEmbedded);
        ProjectionBudgetProfile {
            surface,
            total_bytes: surface_total,
            constitutional_bytes: (surface_total / 4).max(96),
            active_task_bytes: if compact {
                (surface_total / 2).max(128)
            } else {
                (surface_total / 3).max(256)
            },
            governed_memory_bytes: if compact {
                (surface_total / 3).max(128)
            } else {
                (surface_total / 2).max(384)
            },
            background_governance_bytes: (surface_total / 4).max(96),
            block_bytes: (surface_total / 4).max(128),
            deep_inspection: matches!(
                self,
                Self::ServerLinux | Self::SdkFull | Self::MemoryGateway | Self::DevFull
            ) && !matches!(surface, ProjectionSurface::Adapter),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallPlaneExecutionReport {
    pub plane: MemoryPlane,
    pub query: RecallQuery,
    pub backend: String,
    pub candidate_count: usize,
    pub selected_count: usize,
    pub selected_ids: Vec<String>,
    pub skipped_count: usize,
    pub miss_reason: Option<String>,
    pub selection_note: Option<String>,
    pub warnings: Vec<String>,
    pub recall: RecallSelectionReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptRecallRouterSignal {
    pub plane: MemoryPlane,
    pub score: u32,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptRecallRouterDecision {
    pub intent: PromptRecallIntent,
    pub reason: String,
    pub signals: Vec<PromptRecallRouterSignal>,
    pub active_task_order: Vec<MemoryPlane>,
    pub governed_memory_order: Vec<MemoryPlane>,
}

impl PromptRecallRouterDecision {
    pub fn new(intent: PromptRecallIntent, reason: impl Into<String>) -> Self {
        Self {
            intent,
            reason: reason.into(),
            signals: Vec::new(),
            active_task_order: default_active_task_order(intent),
            governed_memory_order: default_governed_memory_order(intent),
        }
    }

    pub fn with_signals(mut self, signals: Vec<PromptRecallRouterSignal>) -> Self {
        self.signals = signals;
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromptAssemblyGroups {
    pub constitutional_stack: Option<String>,
    pub active_task_context: Option<String>,
    pub governed_memory_evidence: Option<String>,
    pub background_governance: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromptAssemblyBudgetSlice {
    pub before_bytes: usize,
    pub after_bytes: usize,
    pub max_bytes: usize,
    pub trimmed: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromptAssemblyBudgetReport {
    pub total: PromptAssemblyBudgetSlice,
    pub constitutional_stack: PromptAssemblyBudgetSlice,
    pub active_task: PromptAssemblyBudgetSlice,
    pub governed_memory: PromptAssemblyBudgetSlice,
    pub background_governance: PromptAssemblyBudgetSlice,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionSanitizerReport {
    pub surface: ProjectionSurface,
    pub checked_fragments: usize,
    pub redacted_fragments: usize,
    pub credentials_redacted: usize,
    pub private_echo_redacted: usize,
    pub warnings: Vec<String>,
}

impl ProjectionSanitizerReport {
    pub fn new(surface: ProjectionSurface) -> Self {
        Self {
            surface,
            checked_fragments: 0,
            redacted_fragments: 0,
            credentials_redacted: 0,
            private_echo_redacted: 0,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptContextBlock {
    pub group: PromptContextGroup,
    pub projection: ProjectionBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PromptContextGroup {
    ConstitutionalStack,
    ActiveTaskContext,
    GovernedMemoryEvidence,
    BackgroundGovernance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptAssemblyReport {
    pub request: RecallAssemblyRequest,
    pub surface: ProjectionSurface,
    pub profile: RuntimeProfile,
    pub router: PromptRecallRouterDecision,
    pub rerank: CrossPlaneRerankReport,
    pub plane_reports: Vec<RecallPlaneExecutionReport>,
    pub groups: PromptAssemblyGroups,
    pub context_blocks: Vec<PromptContextBlock>,
    pub blocks: Vec<ProjectionBlock>,
    pub budget: PromptAssemblyBudgetReport,
    pub sanitizer: ProjectionSanitizerReport,
    pub privacy_filtered_count: usize,
    pub warnings: Vec<String>,
}

impl PromptAssemblyReport {
    pub fn empty(request: RecallAssemblyRequest, intent: PromptRecallIntent) -> Self {
        let surface = request.surface;
        let profile = request.profile;
        Self {
            request,
            surface,
            profile,
            router: PromptRecallRouterDecision::new(intent, "empty_prompt_assembly"),
            rerank: CrossPlaneRerankReport::empty(intent),
            plane_reports: Vec::new(),
            groups: PromptAssemblyGroups::default(),
            context_blocks: Vec::new(),
            blocks: Vec::new(),
            budget: PromptAssemblyBudgetReport::default(),
            sanitizer: ProjectionSanitizerReport::new(surface),
            privacy_filtered_count: 0,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingRecallInspectionReport {
    pub assembly: PromptAssemblyReport,
    pub selected: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

fn normalize_query(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn default_active_task_order(intent: PromptRecallIntent) -> Vec<MemoryPlane> {
    match intent {
        PromptRecallIntent::Continuity => vec![
            MemoryPlane::ContinuityCapsule,
            MemoryPlane::TaskRecall,
            MemoryPlane::SubjectProjection,
        ],
        PromptRecallIntent::Procedural => vec![MemoryPlane::TaskRecall, MemoryPlane::Procedural],
        _ => vec![MemoryPlane::TaskRecall, MemoryPlane::ContinuityCapsule],
    }
}

fn default_governed_memory_order(intent: PromptRecallIntent) -> Vec<MemoryPlane> {
    match intent {
        PromptRecallIntent::Evidence => vec![
            MemoryPlane::ArchiveEvidence,
            MemoryPlane::SharedFactual,
            MemoryPlane::Procedural,
        ],
        PromptRecallIntent::Procedural => vec![
            MemoryPlane::Procedural,
            MemoryPlane::SharedFactual,
            MemoryPlane::ArchiveEvidence,
        ],
        _ => vec![
            MemoryPlane::SharedFactual,
            MemoryPlane::ArchiveEvidence,
            MemoryPlane::Procedural,
        ],
    }
}
