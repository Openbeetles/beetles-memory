use std::sync::Arc;

use bm_core::llm::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy,
    ToolSpec,
};
use bm_core::platform::ResponseBody;
use bm_core::runtime::RuntimeLifecycleReport;
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryClock, MemoryIdentity, MemoryInspectionRequest,
    MemoryMaintenanceRequest, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryReplayRequest, MemoryRuntime, MemoryScope, MemoryStoreHandle, MemoryWriteRequest,
    PressureLevel, RuntimeLifecycleModeInput, RuntimeSkillReuseOutcome, RuntimeSkillWriteSource,
    StoreBackendConfig,
};

use crate::{ReplayFailure, ReplayFixture, ReplayOperation, ReplayRunReport};

#[derive(Clone)]
pub struct ReplayRunnerConfig {
    pub backend: StoreBackendConfig,
    pub identity: MemoryIdentity,
    pub scope: MemoryScope,
    pub now_secs: u64,
    pub capability_policy: MemoryCapabilityPolicy,
    pub privacy_policy: MemoryPrivacyPolicy,
}

impl ReplayRunnerConfig {
    pub fn for_backend(backend: StoreBackendConfig) -> bm_core::Result<Self> {
        Ok(Self {
            backend,
            identity: MemoryIdentity::new("replay-agent", "replay-owner")?,
            scope: MemoryScope::new("replay", "replay-chat")?,
            now_secs: 1_800_000_000,
            capability_policy: MemoryCapabilityPolicy::strict_profile(),
            privacy_policy: MemoryPrivacyPolicy::standard_private_boundary(),
        })
    }
}

pub fn run_replay_fixture(
    fixture: &ReplayFixture,
    config: ReplayRunnerConfig,
) -> bm_core::Result<ReplayRunReport> {
    if fixture.profile != config.backend.profile() {
        return Ok(ReplayRunReport {
            passed: false,
            failures: vec![ReplayFailure::new(
                "fixture_profile",
                format!(
                    "fixture profile {} does not match backend profile {}",
                    fixture.profile.as_str(),
                    config.backend.profile().as_str()
                ),
            )],
            ..ReplayRunReport::new(
                fixture.fixture_id.clone(),
                fixture.profile,
                config.backend.backend().as_str().to_string(),
            )
        });
    }

    let platform = MemoryStoreHandle::open(config.backend.clone())?;
    platform.import_replay_snapshot(&fixture.store_snapshot)?;
    let runtime = MemoryRuntime::builder()
        .identity(config.identity)
        .scope(config.scope)
        .store(platform.clone())
        .clock(Arc::new(FixedReplayClock {
            now_secs: config.now_secs,
        }))
        .capability_policy(config.capability_policy)
        .privacy_policy(config.privacy_policy)
        .build()?;

    let mut report = ReplayRunReport::new(
        fixture.fixture_id.clone(),
        fixture.profile,
        platform.config().backend().as_str().to_string(),
    );

    for operation in &fixture.operations {
        match run_operation(&runtime, operation) {
            Ok(operation_report) => {
                report.operations_run = report.operations_run.saturating_add(1);
                report
                    .lifecycle_operations
                    .push(operation_report.lifecycle_operation);
                report.report_fragments.push(operation_report.fragment);
            }
            Err(error) => {
                report.failures.push(ReplayFailure::new(
                    operation.stage_label(),
                    error.to_string(),
                ));
                break;
            }
        }
    }

    let snapshot = platform.export_replay_snapshot()?;
    report.state_fingerprint = snapshot.state_fingerprint();
    report.event_fingerprint = snapshot.event_fingerprint();
    Ok(report.finish(&fixture.expected))
}

struct OperationReport {
    lifecycle_operation: String,
    fragment: String,
}

fn run_operation(
    runtime: &MemoryRuntime,
    operation: &ReplayOperation,
) -> bm_core::Result<OperationReport> {
    match operation {
        ReplayOperation::WriteProcedural {
            writes,
            owning_scope,
        } => {
            let report = runtime.write(MemoryWriteRequest::Procedural {
                writes: writes.clone(),
                owning_scope: owning_scope.clone(),
                source: RuntimeSkillWriteSource::Manual,
            })?;
            Ok(OperationReport::new(
                &report.lifecycle_report,
                format!(
                    "write accepted={} changed={} reason={}",
                    report.accepted, report.changed, report.reason
                ),
            ))
        }
        ReplayOperation::Recall { query, limit } => {
            let report = runtime.recall(MemoryRecallRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                query: query.clone(),
                limit: *limit,
                tool_registry_refs: Vec::new(),
            })?;
            let selected_count = report
                .procedural_delivery_reports
                .iter()
                .filter(|delivery| delivery.selected)
                .count();
            Ok(OperationReport::new(
                &report.lifecycle_report,
                format!(
                    "recall query={} procedural_delivery_reports={} selected_count={}",
                    report.query,
                    report.procedural_delivery_reports.len(),
                    selected_count
                ),
            ))
        }
        ReplayOperation::Project {
            user_query,
            system_max_len,
        } => {
            let report = runtime.project_safe(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: user_query.clone(),
                system_max_len: *system_max_len,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
            })?;
            Ok(OperationReport::new(
                report.lifecycle_report(),
                format!("project bytes={}", report.ui_api_chars()),
            ))
        }
        ReplayOperation::Maintain {
            ingress,
            user_content,
            reply_content,
            tool_calls,
            external_content_used,
            pressure,
        } => {
            let mut http = ReplayHttpClient;
            let llm = ReplayLlmClient;
            let report = runtime.maintain(
                &mut http,
                &llm,
                MemoryMaintenanceRequest {
                    ingress: *ingress,
                    user_content: user_content.clone(),
                    reply_content: reply_content.clone(),
                    tool_calls: *tool_calls,
                    external_content_used: *external_content_used,
                    runtime_skill_selected_ids: Vec::new(),
                    task_learning_selected_ids: Vec::new(),
                    reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
                    reuse_outcome_note: String::new(),
                    pressure: *pressure,
                    mode_input: RuntimeLifecycleModeInput::default(),
                },
            )?;
            Ok(OperationReport::new(
                &report.lifecycle_report,
                format!(
                    "maintain report_present={} refresh_enqueued={}",
                    report.report.is_some(),
                    report.long_term_refresh_enqueued
                ),
            ))
        }
        ReplayOperation::Inspect {
            query,
            system_max_len,
        } => {
            let report = runtime.inspect(MemoryInspectionRequest {
                query: query.clone(),
                system_max_len: *system_max_len,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            })?;
            Ok(OperationReport::new(
                &report.lifecycle_report,
                format!(
                    "inspect query={} capability_profile={}",
                    report.working.query,
                    report.capabilities.profile.as_str()
                ),
            ))
        }
        ReplayOperation::Replay { chat_id, limit } => {
            let report = runtime.replay(MemoryReplayRequest {
                chat_id: chat_id.clone(),
                limit: *limit,
            })?;
            Ok(OperationReport::new(
                &report.lifecycle_report,
                format!(
                    "replay chat_id={} alerts={}",
                    report.chat_id,
                    report.inspection.alerts.len()
                ),
            ))
        }
    }
}

impl ReplayOperation {
    fn stage_label(&self) -> &'static str {
        match self {
            Self::WriteProcedural { .. } => "write_procedural",
            Self::Recall { .. } => "recall",
            Self::Project { .. } => "project",
            Self::Maintain { .. } => "maintain",
            Self::Inspect { .. } => "inspect",
            Self::Replay { .. } => "replay",
        }
    }
}

impl OperationReport {
    fn new(lifecycle_report: &RuntimeLifecycleReport, fragment: String) -> Self {
        Self {
            lifecycle_operation: lifecycle_report.operation.as_str().to_string(),
            fragment,
        }
    }
}

struct FixedReplayClock {
    now_secs: u64,
}

impl MemoryClock for FixedReplayClock {
    fn now_secs(&self) -> u64 {
        self.now_secs
    }
}

struct ReplayHttpClient;

impl LlmHttpClient for ReplayHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> bm_core::Result<(u16, ResponseBody)> {
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

struct ReplayLlmClient;

impl LlmClient for ReplayLlmClient {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> bm_core::Result<LlmResponse> {
        Ok(LlmResponse {
            content: "Summary: replay maintenance".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}
