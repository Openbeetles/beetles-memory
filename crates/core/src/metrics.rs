use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetricCounters {
    pub write_count: u64,
    pub write_changed_count: u64,
    pub recall_requests: u64,
    pub recall_hits: u64,
    pub projection_requests: u64,
    pub projection_injections: u64,
    pub maintenance_requests: u64,
    pub finalize_requests: u64,
    pub finalize_committed: u64,
    pub deferred_governance_jobs: u64,
    pub migration_exports: u64,
    pub migration_imports: u64,
    pub store_event_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetricsReport {
    pub source: String,
    pub budget_report_id: String,
    pub memory_system_occupancy: u64,
    pub storage_total_bytes: Option<u64>,
    pub counters: RuntimeMetricCounters,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeMetricEvent {
    pub kind_name: String,
    pub timestamp_unix_secs: u64,
    pub payload: BTreeMap<String, String>,
}

pub fn build_runtime_metrics_report(
    events: impl IntoIterator<Item = RuntimeMetricEvent>,
    budget_report_id: impl Into<String>,
) -> RuntimeMetricsReport {
    let mut counters = RuntimeMetricCounters::default();
    let mut storage_total_bytes = None;
    let mut raw_write_events = 0_u64;
    for event in events {
        counters.store_event_count = counters.store_event_count.saturating_add(1);
        if let Some(value) = payload_u64(&event.payload, "storage_total_bytes") {
            storage_total_bytes = Some(value);
        }
        if event.kind_name == "memory.write" {
            raw_write_events = raw_write_events.saturating_add(1);
        }
        if event.kind_name != "runtime.lifecycle" {
            continue;
        }
        let operation = payload_str(&event.payload, "operation");
        let result_summary = payload_str(&event.payload, "result_summary");
        let result_summary = if result_summary.is_empty() {
            payload_str(&event.payload, "result")
        } else {
            result_summary
        };
        if result_summary.starts_with("write.") && payload_bool(&event.payload, "success") {
            let changed_count = payload_u64(&event.payload, "changed_count").unwrap_or(1);
            counters.write_count = counters.write_count.saturating_add(1);
            counters.write_changed_count =
                counters.write_changed_count.saturating_add(changed_count);
        }
        if operation == "recall" && result_summary == "recall_completed" {
            counters.recall_requests = counters.recall_requests.saturating_add(1);
            if payload_bool(&event.payload, "memory_hit")
                || payload_u64(&event.payload, "hit_count").unwrap_or(0) > 0
            {
                counters.recall_hits = counters.recall_hits.saturating_add(1);
            }
        }
        if operation == "project" && result_summary == "projection_completed" {
            counters.projection_requests = counters.projection_requests.saturating_add(1);
            if payload_bool(&event.payload, "projection_injected") {
                counters.projection_injections = counters.projection_injections.saturating_add(1);
            }
        }
        if operation == "maintain" && result_summary == "maintenance_completed" {
            counters.maintenance_requests = counters.maintenance_requests.saturating_add(1);
        }
        if payload_bool(&event.payload, "finalize_request") {
            counters.finalize_requests = counters.finalize_requests.saturating_add(1);
            if payload_bool(&event.payload, "finalize_committed") {
                counters.finalize_committed = counters.finalize_committed.saturating_add(1);
            }
        }
        if payload_bool(&event.payload, "deferred_governance_job") {
            counters.deferred_governance_jobs = counters.deferred_governance_jobs.saturating_add(1);
        }
        if operation == "export" && result_summary == "export_completed" {
            counters.migration_exports = counters.migration_exports.saturating_add(1);
        }
        if operation == "import" && result_summary == "import_completed" {
            counters.migration_imports = counters.migration_imports.saturating_add(1);
        }
    }
    if counters.write_count == 0 && raw_write_events > 0 {
        counters.write_count = raw_write_events;
        counters.write_changed_count = raw_write_events;
    }
    let memory_system_occupancy = counters
        .write_changed_count
        .saturating_add(counters.projection_injections)
        .saturating_add(counters.deferred_governance_jobs);
    RuntimeMetricsReport {
        source: "core.runtime_events".to_string(),
        budget_report_id: budget_report_id.into(),
        memory_system_occupancy,
        storage_total_bytes,
        counters,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorReadinessReport {
    pub memory_owner: String,
    pub write_candidate_ready: bool,
    pub semantic_governance_ready: bool,
    pub subject_scope_ready: bool,
    pub migration_ready: bool,
    pub adapter_semantics_clean: bool,
    pub host_direct_write_detected: bool,
    pub metrics_source: String,
    pub unavailable_reasons: Vec<String>,
}

impl OperatorReadinessReport {
    pub fn sdk_ready(unavailable_reasons: Vec<String>) -> Self {
        Self {
            memory_owner: "sdk".to_string(),
            write_candidate_ready: true,
            semantic_governance_ready: true,
            subject_scope_ready: true,
            migration_ready: true,
            adapter_semantics_clean: true,
            host_direct_write_detected: false,
            metrics_source: "core.runtime_events".to_string(),
            unavailable_reasons,
        }
    }
}

pub fn record_runtime_spawn_failure() {}

fn payload_str<'a>(payload: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    payload.get(key).map(String::as_str).unwrap_or_default()
}

fn payload_bool(payload: &BTreeMap<String, String>, key: &str) -> bool {
    matches!(payload.get(key).map(String::as_str), Some("true" | "1"))
}

fn payload_u64(payload: &BTreeMap<String, String>, key: &str) -> Option<u64> {
    payload.get(key).and_then(|value| value.parse::<u64>().ok())
}
