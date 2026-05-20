use crate::bus::PcMsg;
use crate::error::Result;
use crate::llm::Message;
use crate::memory::{
    append_system_prompt_base, append_system_prompt_daily_note, build_context_messages,
    ExecutionState, ExecutionStatus, ImportantMessageStore, MemoryStore, SessionMessage,
    SessionStore,
};
use crate::task_execution::{
    current_or_next_step, TaskRunRecord, TaskRunStatus, TaskStep, TaskStepStatus,
};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

pub mod soul_feedback;
pub mod subject_state;

pub mod deliberation {
    use crate::memory::TurnDeliberationClass;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct TurnDeliberationGate {
        pub(crate) class: TurnDeliberationClass,
        pub(crate) compact_reply: bool,
        pub(crate) prefer_explicit_blocker: bool,
        pub(crate) rationale: Vec<String>,
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActiveWorkKind {
    InteractiveAction,
    TaskExecution,
}

impl ActiveWorkKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::InteractiveAction => "interactive_action",
            Self::TaskExecution => "task_execution",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundWorkStatus {
    Running,
    AwaitingUser,
    Suspended,
    Completed,
    Aborted,
    FailedTerminal,
}

impl ForegroundWorkStatus {
    pub const fn continuity_open(self) -> bool {
        matches!(self, Self::Running | Self::AwaitingUser | Self::Suspended)
    }

    pub const fn default_blocks_background_llm(self) -> bool {
        matches!(self, Self::Running | Self::AwaitingUser)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Running => "active",
            Self::AwaitingUser => "awaiting_user",
            Self::Suspended => "suspended",
            Self::Completed => "completed",
            Self::Aborted => "aborted",
            Self::FailedTerminal => "failed_terminal",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveWorkRecord {
    pub kind: ActiveWorkKind,
    #[serde(default)]
    pub title: String,
    pub status: ForegroundWorkStatus,
    #[serde(default)]
    pub continuity_open: bool,
    #[serde(default)]
    pub blocks_background_llm: bool,
    #[serde(default)]
    pub progress_summary: String,
    #[serde(default)]
    pub blocker: String,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub recent_outcome: String,
    #[serde(default)]
    pub active_artifact_refs: Vec<String>,
    #[serde(default)]
    pub updated_at: u64,
}

impl ActiveWorkRecord {
    pub fn is_meaningful(&self) -> bool {
        !self.title.trim().is_empty()
            || !self.progress_summary.trim().is_empty()
            || !self.blocker.trim().is_empty()
            || !self.next_action.trim().is_empty()
            || !self.recent_outcome.trim().is_empty()
            || !self.active_artifact_refs.is_empty()
    }

    pub fn from_interactive_execution_state(
        state: &ExecutionState,
        user_request: &str,
    ) -> Option<Self> {
        let status = match state.status {
            ExecutionStatus::Done => ForegroundWorkStatus::Completed,
            ExecutionStatus::Blocked => ForegroundWorkStatus::AwaitingUser,
            ExecutionStatus::Active => ForegroundWorkStatus::Running,
        };
        let record = Self {
            kind: ActiveWorkKind::InteractiveAction,
            title: first_non_empty([Some(state.goal.as_str()), Some(user_request)]),
            status,
            continuity_open: status.continuity_open(),
            blocks_background_llm: status.default_blocks_background_llm(),
            progress_summary: state.progress.clone(),
            blocker: state.blocker.clone(),
            next_action: state.next_action.clone(),
            recent_outcome: state.last_output.clone(),
            active_artifact_refs: Vec::new(),
            updated_at: state.updated_at,
        };
        record.is_meaningful().then_some(record)
    }

    pub fn from_task_run(record: &TaskRunRecord) -> Option<Self> {
        let step = current_or_next_step(record);
        let status = foreground_status_from_task_run(record, step);
        let candidate = Self {
            kind: ActiveWorkKind::TaskExecution,
            title: first_non_empty([
                Some(record.run.title.as_str()),
                Some(record.plan.goal.as_str()),
                Some(record.run.user_request.as_str()),
            ]),
            status,
            continuity_open: status.continuity_open(),
            blocks_background_llm: status.default_blocks_background_llm(),
            progress_summary: first_non_empty([
                step.map(|value| value.last_result_summary.as_str()),
                step.and_then(task_step_progress_fallback),
            ]),
            blocker: first_non_empty([
                Some(record.run.failure_reason.as_str()),
                step.and_then(task_step_blocker_fallback),
            ]),
            next_action: if status.continuity_open() {
                first_non_empty([
                    step.map(|value| value.instruction.as_str()),
                    step.map(|value| value.title.as_str()),
                ])
            } else {
                String::new()
            },
            recent_outcome: first_non_empty([
                Some(record.run.final_summary.as_str()),
                step.and_then(task_step_recent_outcome_fallback),
            ]),
            active_artifact_refs: step
                .map(|value| {
                    value
                        .expected_artifacts
                        .iter()
                        .map(|item| item.trim().to_string())
                        .filter(|item| !item.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            updated_at: record.run.updated_at,
        };
        candidate.is_meaningful().then_some(candidate)
    }

    pub fn execution_state_projection(&self) -> ExecutionState {
        ExecutionState {
            status: match self.status {
                ForegroundWorkStatus::Completed => ExecutionStatus::Done,
                ForegroundWorkStatus::AwaitingUser
                | ForegroundWorkStatus::Suspended
                | ForegroundWorkStatus::FailedTerminal => ExecutionStatus::Blocked,
                ForegroundWorkStatus::Running | ForegroundWorkStatus::Aborted => {
                    ExecutionStatus::Active
                }
            },
            goal: self.title.clone(),
            progress: self.progress_summary.clone(),
            blocker: self.blocker.clone(),
            next_action: self.next_action.clone(),
            last_output: self.recent_outcome.clone(),
            updated_at: self.updated_at,
            ..ExecutionState::default()
        }
    }
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> String {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_default()
}

fn foreground_status_from_task_run(
    record: &TaskRunRecord,
    step: Option<&TaskStep>,
) -> ForegroundWorkStatus {
    match record.run.status {
        TaskRunStatus::Planning | TaskRunStatus::Running => ForegroundWorkStatus::Running,
        TaskRunStatus::Blocked => {
            if !record.run.failure_reason.trim().is_empty()
                || step.is_some_and(|value| {
                    matches!(
                        value.status,
                        TaskStepStatus::Blocked | TaskStepStatus::Failed
                    ) && !value.last_review_summary.trim().is_empty()
                })
            {
                ForegroundWorkStatus::AwaitingUser
            } else {
                ForegroundWorkStatus::Suspended
            }
        }
        TaskRunStatus::Failed => ForegroundWorkStatus::FailedTerminal,
        TaskRunStatus::Aborted => ForegroundWorkStatus::Aborted,
        TaskRunStatus::Completed | TaskRunStatus::PartialComplete => {
            ForegroundWorkStatus::Completed
        }
    }
}

fn task_step_progress_fallback(step: &TaskStep) -> Option<&str> {
    if !step.last_review_summary.trim().is_empty() {
        Some(step.last_review_summary.as_str())
    } else if !step.title.trim().is_empty() {
        Some(step.title.as_str())
    } else {
        None
    }
}

fn task_step_blocker_fallback(step: &TaskStep) -> Option<&str> {
    if matches!(
        step.status,
        TaskStepStatus::Blocked | TaskStepStatus::Failed
    ) {
        if !step.last_review_summary.trim().is_empty() {
            Some(step.last_review_summary.as_str())
        } else if !step.title.trim().is_empty() {
            Some(step.title.as_str())
        } else {
            None
        }
    } else {
        None
    }
}

fn task_step_recent_outcome_fallback(step: &TaskStep) -> Option<&str> {
    if !step.last_result_summary.trim().is_empty() {
        Some(step.last_result_summary.as_str())
    } else if step.status.is_terminal() && !step.last_review_summary.trim().is_empty() {
        Some(step.last_review_summary.as_str())
    } else {
        None
    }
}

pub trait ActiveWorkStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>>;
    fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()>;
    fn clear(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }
}

pub fn load_active_work_for_chat(
    store: &dyn ActiveWorkStore,
    active_task_run: Option<&crate::task_execution::TaskRunRecord>,
    chat_id: &str,
) -> Result<Option<ActiveWorkRecord>> {
    if let Some(record) = store.get(chat_id)? {
        if record.is_meaningful() {
            return Ok(Some(record));
        }
    }
    Ok(active_task_run.and_then(ActiveWorkRecord::from_task_run))
}

pub fn has_meaningful_foreground_work_for_chat(
    store: &dyn ActiveWorkStore,
    chat_id: &str,
) -> Result<bool> {
    Ok(store
        .get(chat_id)?
        .as_ref()
        .is_some_and(ActiveWorkRecord::is_meaningful))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DetachedJobKind {
    SelfRuntimePostReply,
    SelfRuntimeIdleTick,
    OperatorMaintenance,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DetachedWorkKey {
    pub channel: String,
    pub chat_id: String,
    pub kind: DetachedJobKind,
}

impl DetachedWorkKey {
    pub fn new(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        kind: DetachedJobKind,
    ) -> Self {
        Self {
            channel: channel.into(),
            chat_id: chat_id.into(),
            kind,
        }
    }

    pub fn storage_key(&self) -> String {
        format!("{}:{}:{:?}", self.channel, self.chat_id, self.kind)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetachedWorkState {
    Pending,
    Queued,
    Running,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetachedWorkRecord {
    pub key: DetachedWorkKey,
    pub job: PcMsg,
    pub state: DetachedWorkState,
    pub wake_at_ms: u64,
    pub revision: u64,
    #[serde(default)]
    pub last_reason: String,
    #[serde(default)]
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetachedWorkUpsertOutcome {
    pub changed: bool,
    pub record: DetachedWorkRecord,
}

pub trait DetachedWorkStore: Send + Sync {
    fn get(&self, key: &DetachedWorkKey) -> Result<Option<DetachedWorkRecord>>;
    fn list(&self) -> Result<Vec<DetachedWorkRecord>>;
    fn upsert(
        &self,
        key: &DetachedWorkKey,
        job: &PcMsg,
        wake_at_ms: u64,
        reason: &str,
    ) -> Result<DetachedWorkUpsertOutcome>;
    fn mark_queued(&self, key: &DetachedWorkKey, revision: u64) -> Result<bool>;
    fn claim_running(
        &self,
        key: &DetachedWorkKey,
        revision: u64,
    ) -> Result<Option<DetachedWorkRecord>>;
    fn reschedule(
        &self,
        key: &DetachedWorkKey,
        revision: u64,
        wake_at_ms: u64,
        reason: &str,
    ) -> Result<Option<DetachedWorkRecord>>;
    fn finish(&self, key: &DetachedWorkKey, revision: u64) -> Result<()>;
}

pub fn upsert_detached_work_job(
    store: &dyn DetachedWorkStore,
    key: DetachedWorkKey,
    job: &PcMsg,
    wake_at_ms: u64,
    reason: &str,
) -> Result<DetachedWorkUpsertOutcome> {
    store.upsert(&key, job, wake_at_ms, reason)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowOutcomeKind {
    Retryable,
    Permanent,
    Capability,
    NeedsUserFacts,
    NeedsUserChoice,
    NeedsConfirmation,
    ProbeFailed,
    RuntimeBlocked,
    Unsupported,
    RetryLater,
    TaskBlocked,
}

impl WorkflowOutcomeKind {
    pub fn next_action_hint(self) -> &'static str {
        match self {
            Self::Retryable => "decide whether to retry or route around the retryable blocker",
            Self::Permanent => {
                "state the permanent blocker clearly and switch to a different approach"
            }
            Self::Capability => {
                "state the capability blocker clearly and switch to an alternative path"
            }
            Self::NeedsUserFacts => "ask only for the missing user facts before continuing",
            Self::NeedsUserChoice => {
                "ask the user to choose among the offered options before continuing"
            }
            Self::NeedsConfirmation => "ask for explicit confirmation before continuing",
            Self::ProbeFailed => {
                "explain the probe failure clearly before asking for corrected facts"
            }
            Self::RuntimeBlocked => "state the runtime blocker clearly before continuing",
            Self::Unsupported => "state the unsupported path clearly before choosing another route",
            Self::RetryLater => "state what must change before retrying",
            Self::TaskBlocked => {
                "state exactly what blocked the workflow and what concrete step can resume it"
            }
        }
    }
}

pub fn parse_workflow_outcome_kind(label: &str) -> Option<WorkflowOutcomeKind> {
    match label {
        "retryable" => Some(WorkflowOutcomeKind::Retryable),
        "permanent" => Some(WorkflowOutcomeKind::Permanent),
        "capability" => Some(WorkflowOutcomeKind::Capability),
        "needs_user_facts" => Some(WorkflowOutcomeKind::NeedsUserFacts),
        "needs_user_choice" => Some(WorkflowOutcomeKind::NeedsUserChoice),
        "needs_confirmation" => Some(WorkflowOutcomeKind::NeedsConfirmation),
        "probe_failed" => Some(WorkflowOutcomeKind::ProbeFailed),
        "runtime_blocked" => Some(WorkflowOutcomeKind::RuntimeBlocked),
        "unsupported" => Some(WorkflowOutcomeKind::Unsupported),
        "retry_later" => Some(WorkflowOutcomeKind::RetryLater),
        "task_blocked" => Some(WorkflowOutcomeKind::TaskBlocked),
        _ => None,
    }
}

pub const SESSION_RECENT_N: usize = 32;
const DAILY_RECENT_N: usize = 5;

#[derive(Clone, Copy, Debug)]
pub struct RuntimeContext {
    pub now_secs: u64,
    pub platform: &'static str,
    pub pressure: crate::orchestrator::PressureLevel,
    pub active_agent_tasks: u32,
    pub inbound_depth: u32,
    pub outbound_depth: u32,
}

pub struct ContextParams<'a> {
    pub msg: &'a PcMsg,
    pub memory_system_kind: crate::memory::MemorySystemKind,
    pub memory: &'a dyn MemoryStore,
    pub session: &'a dyn SessionStore,
    pub important_message_store: &'a dyn ImportantMessageStore,
    pub has_tools: bool,
    pub skill_descriptions: &'a str,
    pub system_max_len: usize,
    pub messages_max_len: usize,
    pub recent_messages_limit: usize,
    pub group_activation: &'a str,
    pub emotion_signal_suffix: Option<&'a str>,
    pub memory_health_text: Option<&'a str>,
    pub constitutional_stack_text: Option<&'a str>,
    pub subject_state_text: Option<&'a str>,
    pub deliberation_gate_text: Option<&'a str>,
    pub soul_feedback_projection_text: Option<&'a str>,
    pub active_task_context_text: Option<&'a str>,
    pub governed_memory_evidence_text: Option<&'a str>,
    pub background_governance_text: Option<&'a str>,
    pub programmable_reasoning_intent_text: Option<&'a str>,
    pub capability_package_text: Option<&'a str>,
    pub summary_text: Option<&'a str>,
    pub recent_messages: Option<&'a [SessionMessage]>,
    pub runtime: Option<RuntimeContext>,
    pub include_daily_notes: bool,
    pub llm_hint: &'a str,
}

const STRUCTURED_BLOCK: &str = concat!(
    "\n\n## Structured output\n",
    "When you want to mark the current user message as important for context truncation, include ",
    "[MARK_IMPORTANT]",
    " in your reply. When you sense the user may need comfort or encouragement, include ",
    "[SIGNAL:comfort]",
    " in your reply."
);
const TOOL_BEHAVIOR_CONSTRAINT: &str = "\n\nWhen you decide to use a tool, use the provided tool invocation mechanism directly. Never describe or narrate a tool call in plain text without actually invoking it.";
const GROUP_ALWAYS_SILENT_CONSTRAINT: &str =
    "\n\nIf no response is needed, reply with exactly SILENT and nothing else.";
const GROUP_MENTION_ONLY_CONSTRAINT: &str =
    "\n\nYou are in a group; only reply when explicitly mentioned.";
const OUTPUT_CONTRACT_PREFIX: &str = "\n\n## Output Contract\nMain replies must be plain-text safe across channels. Never return an empty assistant message; use SILENT only if group rules allow silence. Current chat channel: ";
const OUTPUT_CONTRACT_SUFFIX: &str = ".";
const REPLY_PRIORITY_CONSTRAINT: &str = "\n\n## Reply Priority\nWhen writing the main reply, follow this order of authority:\n1. Self-authored core.\n2. Relationship constitution.\n3. Current persona priority.\n4. Boundary/disclosure adjudication.\n5. Soul and user contract.\n6. Task execution.\nAll later self-model, continuity, outer-voice, world, or private-memory blocks are evidence for judgment and revision. They do not outrank the constitutional stack above.";
const REPLY_LAW_CONSTRAINT: &str = "\n\n## Reply Law\nFor relationship, self, memory, or privacy questions, answer from grounded subject-state and constitutional evidence when that evidence exists. Never exceed the evidence ceiling. Protected inner/private layers require boundary judgment before disclosure.";
const MEMORY_HEALTH_SECTION: &str = "\n\n## Memory Health\n";
const CONSTITUTIONAL_STACK_SECTION: &str = "\n\n## Constitutional Stack\n";
const SUBJECT_STATE_SECTION: &str = "\n\n## Subject State\n";
const TURN_DELIBERATION_GATE_SECTION: &str = "\n\n## Turn Deliberation Gate\n";
const PROGRAMMABLE_REASONING_INTENT_SECTION: &str = "\n\n## Programmable Reasoning Intent\n";
const SOUL_FEEDBACK_PROJECTION_SECTION: &str = "\n\n## Soul Feedback Projection\n";
const ACTIVE_TASK_CONTEXT_SECTION: &str = "\n\n## Active Task Context\n";
const GOVERNED_MEMORY_EVIDENCE_SECTION: &str = "\n\n## Governed Memory Evidence\n";
const BACKGROUND_GOVERNANCE_SECTION: &str = "\n\n## Background Governance\n";

pub fn build_context(p: &ContextParams<'_>) -> Result<(String, Vec<Message>)> {
    let include_expanded = matches!(
        p.memory_system_kind,
        crate::memory::MemorySystemKind::LinuxFull
    ) || matches!(p.msg.ingress, crate::bus::IngressKind::System);
    let memory_text = if include_expanded {
        p.memory.get_memory().unwrap_or_default()
    } else {
        String::new()
    };
    let mut system = String::with_capacity(p.system_max_len.min(8192));
    append_capped(&mut system, REPLY_PRIORITY_CONSTRAINT, p.system_max_len);
    append_section(
        &mut system,
        MEMORY_HEALTH_SECTION,
        p.memory_health_text,
        p.system_max_len,
    );
    append_section(
        &mut system,
        CONSTITUTIONAL_STACK_SECTION,
        p.constitutional_stack_text,
        p.system_max_len,
    );
    append_section(
        &mut system,
        SUBJECT_STATE_SECTION,
        p.subject_state_text,
        p.system_max_len,
    );
    append_section(
        &mut system,
        TURN_DELIBERATION_GATE_SECTION,
        p.deliberation_gate_text,
        p.system_max_len,
    );
    append_section(
        &mut system,
        PROGRAMMABLE_REASONING_INTENT_SECTION,
        p.programmable_reasoning_intent_text,
        p.system_max_len,
    );
    append_section(
        &mut system,
        SOUL_FEEDBACK_PROJECTION_SECTION,
        p.soul_feedback_projection_text,
        p.system_max_len,
    );
    if !memory_text.trim().is_empty() {
        let mut base = String::new();
        append_system_prompt_base(&mut base, &memory_text, p.system_max_len);
        append_capped_section(&mut system, "\n\n", &base, p.system_max_len);
    }
    append_section(
        &mut system,
        ACTIVE_TASK_CONTEXT_SECTION,
        p.active_task_context_text,
        p.system_max_len,
    );
    append_section(
        &mut system,
        GOVERNED_MEMORY_EVIDENCE_SECTION,
        p.governed_memory_evidence_text,
        p.system_max_len,
    );
    append_capped(&mut system, REPLY_LAW_CONSTRAINT, p.system_max_len);
    if include_expanded {
        append_capped_section(
            &mut system,
            "\n\n",
            p.capability_package_text.unwrap_or_default(),
            p.system_max_len,
        );
        append_section(
            &mut system,
            BACKGROUND_GOVERNANCE_SECTION,
            p.background_governance_text,
            p.system_max_len,
        );
        if p.include_daily_notes {
            for name in p
                .memory
                .list_daily_note_names(DAILY_RECENT_N)
                .unwrap_or_default()
            {
                if let Ok(note) = p.memory.get_daily_note(&name) {
                    if !append_system_prompt_daily_note(&mut system, &note, p.system_max_len) {
                        break;
                    }
                }
            }
        }
    }
    append_capped_section(
        &mut system,
        "\n\n## Skills\n",
        p.skill_descriptions,
        p.system_max_len,
    );
    if p.has_tools {
        append_capped(&mut system, TOOL_BEHAVIOR_CONSTRAINT, p.system_max_len);
    }
    append_runtime_context(&mut system, p.runtime, p.system_max_len);
    if p.msg.is_group {
        if p.group_activation == "always" {
            append_capped(
                &mut system,
                GROUP_ALWAYS_SILENT_CONSTRAINT,
                p.system_max_len,
            );
        } else if p.group_activation == "mention" {
            append_capped(&mut system, GROUP_MENTION_ONLY_CONSTRAINT, p.system_max_len);
        }
    }
    append_capped_section(
        &mut system,
        "\n\n",
        p.emotion_signal_suffix.unwrap_or_default(),
        p.system_max_len,
    );
    append_capped_section(&mut system, "\n\n", p.llm_hint, p.system_max_len);
    append_capped(&mut system, OUTPUT_CONTRACT_PREFIX, p.system_max_len);
    append_capped(&mut system, &p.msg.channel, p.system_max_len);
    append_capped(&mut system, OUTPUT_CONTRACT_SUFFIX, p.system_max_len);
    append_capped(&mut system, STRUCTURED_BLOCK, p.system_max_len);
    truncate_char_boundary(&mut system, p.system_max_len);

    let messages = build_context_messages(
        p.session,
        p.important_message_store,
        p.msg,
        p.recent_messages_limit,
        p.messages_max_len,
        p.summary_text,
        p.recent_messages,
    );
    Ok((system, messages))
}

fn append_section(out: &mut String, header: &str, content: Option<&str>, max_len: usize) -> bool {
    let Some(content) = content.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    append_capped(out, header, max_len) && append_capped(out, content, max_len)
}

fn append_capped_section(out: &mut String, header: &str, content: &str, max_len: usize) -> bool {
    let content = content.trim();
    if content.is_empty() {
        return true;
    }
    append_capped(out, header, max_len) && append_capped(out, content, max_len)
}

fn append_runtime_context(out: &mut String, runtime: Option<RuntimeContext>, max_len: usize) {
    let Some(runtime) = runtime else {
        return;
    };
    let mut block = String::new();
    let _ = write!(
        block,
        "\n\n## Runtime\nplatform={} pressure={} active_agent_tasks={} inbound_depth={} outbound_depth={} now_secs={}",
        runtime.platform,
        runtime.pressure.as_str(),
        runtime.active_agent_tasks,
        runtime.inbound_depth,
        runtime.outbound_depth,
        runtime.now_secs
    );
    append_capped(out, &block, max_len);
}

fn append_capped(out: &mut String, input: &str, max_len: usize) -> bool {
    let remaining = max_len.saturating_sub(out.len());
    if remaining == 0 || input.is_empty() {
        return false;
    }
    if input.len() <= remaining {
        out.push_str(input);
        return true;
    }
    let mut end = remaining;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    if end > 0 {
        out.push_str(&input[..end]);
    }
    false
}

fn truncate_char_boundary(value: &mut String, max_len: usize) {
    if value.len() <= max_len {
        return;
    }
    let mut end = max_len;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}
