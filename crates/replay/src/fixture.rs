use bm_sdk::nonproduction_replay_harness::StoreSnapshot;
use bm_sdk::{IngressKind, PressureLevel, ProfileId, RuntimeSkillWrite};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayFixture {
    pub fixture_id: String,
    pub profile: ProfileId,
    pub store_snapshot: StoreSnapshot,
    pub operations: Vec<ReplayOperation>,
    pub expected: ReplayExpectedOutcome,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayOperation {
    WriteProcedural {
        writes: Vec<RuntimeSkillWrite>,
    },
    Recall {
        query: String,
        limit: usize,
    },
    Project {
        user_query: String,
        system_max_len: usize,
    },
    Maintain {
        ingress: IngressKind,
        user_content: String,
        reply_content: String,
        tool_calls: u32,
        external_content_used: bool,
        pressure: PressureLevel,
    },
    Inspect {
        query: String,
        system_max_len: usize,
    },
    Replay {
        chat_id: String,
        limit: usize,
    },
}

impl ReplayOperation {
    pub fn lightweight_maintain(
        user_content: impl Into<String>,
        reply_content: impl Into<String>,
    ) -> Self {
        Self::Maintain {
            ingress: IngressKind::User,
            user_content: user_content.into(),
            reply_content: reply_content.into(),
            tool_calls: 0,
            external_content_used: false,
            pressure: PressureLevel::Normal,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayExpectedOutcome {
    #[serde(default)]
    pub state_fingerprint: String,
    #[serde(default)]
    pub event_fingerprint: String,
    #[serde(default)]
    pub lifecycle_operations: Vec<String>,
    #[serde(default)]
    pub min_reports: usize,
    #[serde(default)]
    pub required_report_fragments: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayRunReport {
    pub fixture_id: String,
    pub profile: ProfileId,
    pub backend: String,
    pub operations_run: usize,
    pub state_fingerprint: String,
    pub event_fingerprint: String,
    pub lifecycle_operations: Vec<String>,
    pub report_fragments: Vec<String>,
    pub passed: bool,
    pub failures: Vec<ReplayFailure>,
}

impl ReplayRunReport {
    pub(crate) fn new(fixture_id: String, profile: ProfileId, backend: String) -> Self {
        Self {
            fixture_id,
            profile,
            backend,
            operations_run: 0,
            state_fingerprint: String::new(),
            event_fingerprint: String::new(),
            lifecycle_operations: Vec::new(),
            report_fragments: Vec::new(),
            passed: false,
            failures: Vec::new(),
        }
    }

    pub(crate) fn finish(mut self, expected: &ReplayExpectedOutcome) -> Self {
        if !expected.state_fingerprint.is_empty()
            && self.state_fingerprint != expected.state_fingerprint
        {
            self.failures.push(ReplayFailure::new(
                "state_fingerprint",
                format!(
                    "expected {}, got {}",
                    expected.state_fingerprint, self.state_fingerprint
                ),
            ));
        }
        if !expected.event_fingerprint.is_empty()
            && self.event_fingerprint != expected.event_fingerprint
        {
            self.failures.push(ReplayFailure::new(
                "event_fingerprint",
                format!(
                    "expected {}, got {}",
                    expected.event_fingerprint, self.event_fingerprint
                ),
            ));
        }
        if self.report_fragments.len() < expected.min_reports {
            self.failures.push(ReplayFailure::new(
                "report_count",
                format!(
                    "expected at least {} reports, got {}",
                    expected.min_reports,
                    self.report_fragments.len()
                ),
            ));
        }
        for expected_operation in &expected.lifecycle_operations {
            if !self
                .lifecycle_operations
                .iter()
                .any(|operation| operation == expected_operation)
            {
                self.failures.push(ReplayFailure::new(
                    "lifecycle_operation",
                    format!("missing lifecycle operation {expected_operation}"),
                ));
            }
        }
        let joined = self.report_fragments.join("\n");
        for fragment in &expected.required_report_fragments {
            if !joined.contains(fragment) {
                self.failures.push(ReplayFailure::new(
                    "report_fragment",
                    format!("missing report fragment {fragment}"),
                ));
            }
        }
        self.passed = self.failures.is_empty();
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayFailure {
    pub stage: String,
    pub reason: String,
}

impl ReplayFailure {
    pub(crate) fn new(stage: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            stage: stage.into(),
            reason: reason.into(),
        }
    }
}
