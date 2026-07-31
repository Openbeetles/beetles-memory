use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const RUNTIME_METRICS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeMetricsQuery {
    pub write_since_unix_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeMetricEvidenceSummary {
    pub source_store_count: u64,
    pub input_event_count: u64,
    pub admitted_unique_event_count: u64,
    pub duplicate_event_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeMetricsReport {
    pub schema_version: u32,
    pub source: RuntimeMetricsSource,
    pub budget_report_id: String,
    pub write_window_start_unix_secs: Option<u64>,
    pub latest_projection_timestamp_unix_secs: Option<u64>,
    pub latest_projection_chars: Option<u64>,
    pub evidence: RuntimeMetricEvidenceSummary,
    pub counters: RuntimeMetricCounters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeMetricsSource {
    #[serde(rename = "core.runtime_events")]
    CoreRuntimeEvents,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMetricEventKind {
    MemoryWrite,
    RuntimeLifecycle,
    NonMetric,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeMetricEvent {
    pub event_id: String,
    pub kind: RuntimeMetricEventKind,
    pub timestamp_unix_secs: u64,
    pub payload: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RawWriteIdentity {
    EventId(String),
    TransactionId(String),
}

pub fn build_runtime_metrics_report(
    events: impl IntoIterator<Item = RuntimeMetricEvent>,
    query: RuntimeMetricsQuery,
    evidence: RuntimeMetricEvidenceSummary,
    budget_report_id: impl Into<String>,
) -> Result<RuntimeMetricsReport> {
    let mut events = events.into_iter().collect::<Vec<_>>();
    validate_evidence_summary(&events, &evidence)?;

    let mut event_ids = BTreeSet::new();
    for event in &events {
        if event.event_id.trim().is_empty() || !event_ids.insert(event.event_id.clone()) {
            return Err(Error::config(
                "runtime_metrics_event_identity",
                "runtime metric events require unique non-empty event ids",
            ));
        }
    }
    events.sort_by(|left, right| {
        (left.timestamp_unix_secs, left.event_id.as_str())
            .cmp(&(right.timestamp_unix_secs, right.event_id.as_str()))
    });
    for event in &events {
        validate_lifecycle_completion(event)?;
    }

    let mut lifecycle_write_transactions = BTreeMap::new();
    for event in &events {
        let Some(changed_count) = successful_lifecycle_write_changed_count(event)? else {
            continue;
        };
        let transaction_id = payload_str(&event.payload, "transaction_id");
        if changed_count > 0 && transaction_id.is_empty() {
            return Err(Error::config(
                "runtime_metrics_write_transaction",
                "changed write lifecycle event requires a non-empty transaction id",
            ));
        }
        if !transaction_id.is_empty()
            && lifecycle_write_transactions
                .insert(
                    transaction_id.to_string(),
                    (event.timestamp_unix_secs, changed_count),
                )
                .is_some()
        {
            return Err(Error::config(
                "runtime_metrics_write_transaction",
                "one write transaction cannot contain multiple lifecycle summaries",
            ));
        }
    }

    let mut raw_write_transactions = BTreeMap::new();
    for event in &events {
        if event.kind != RuntimeMetricEventKind::MemoryWrite
            || !payload_str(&event.payload, "operation").starts_with("write.")
        {
            continue;
        }
        let transaction_id = payload_str(&event.payload, "transaction_id");
        let transaction_key = if transaction_id.is_empty() {
            RawWriteIdentity::EventId(event.event_id.clone())
        } else {
            RawWriteIdentity::TransactionId(transaction_id.to_string())
        };
        match raw_write_transactions.entry(transaction_key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((event.timestamp_unix_secs, 1_u64));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if entry.get().0 != event.timestamp_unix_secs {
                    return Err(Error::config(
                        "runtime_metrics_write_transaction",
                        "one raw write transaction cannot span multiple timestamps",
                    ));
                }
                entry.get_mut().1 = entry.get().1.saturating_add(1);
            }
        }
    }

    let mut counters = RuntimeMetricCounters {
        store_event_count: evidence.admitted_unique_event_count,
        ..RuntimeMetricCounters::default()
    };
    for (identity, (timestamp_unix_secs, changed_count)) in &raw_write_transactions {
        if let RawWriteIdentity::TransactionId(transaction_id) = identity {
            if let Some((lifecycle_timestamp_unix_secs, lifecycle_changed_count)) =
                lifecycle_write_transactions.get(transaction_id)
            {
                if *lifecycle_changed_count == 0 {
                    return Err(Error::config(
                        "runtime_metrics_write_transaction_conflict",
                        "zero-change lifecycle summary cannot own a durable write event",
                    ));
                }
                if lifecycle_timestamp_unix_secs != timestamp_unix_secs {
                    return Err(Error::config(
                        "runtime_metrics_write_transaction",
                        "one write transaction cannot span multiple timestamps",
                    ));
                }
                continue;
            }
        }
        if write_is_in_window(*timestamp_unix_secs, &query) {
            counters.write_count = counters.write_count.saturating_add(1);
            counters.write_changed_count =
                counters.write_changed_count.saturating_add(*changed_count);
        }
    }
    let mut latest_projection = None;

    for event in &events {
        if event.kind == RuntimeMetricEventKind::MemoryWrite {
            continue;
        }
        if event.kind != RuntimeMetricEventKind::RuntimeLifecycle {
            continue;
        }
        if !lifecycle_succeeded(event)? {
            continue;
        }

        let operation = payload_str(&event.payload, "operation");
        let result_summary = lifecycle_result_summary(event);
        if let Some(changed_count) = successful_lifecycle_write_changed_count(event)? {
            if changed_count > 0 && write_is_in_window(event.timestamp_unix_secs, &query) {
                counters.write_count = counters.write_count.saturating_add(1);
                counters.write_changed_count =
                    counters.write_changed_count.saturating_add(changed_count);
            }
        }
        if operation == "recall"
            && result_summary == "recall_completed"
            && payload_str(&event.payload, "trigger") == "sdk_call"
        {
            let memory_hit = required_payload_bool(&event.payload, "memory_hit")?;
            let hit_count = required_payload_u64(&event.payload, "hit_count")?;
            if memory_hit != (hit_count > 0) {
                return Err(Error::config(
                    "runtime_metrics_event_payload",
                    "memory_hit must exactly match the typed hit_count",
                ));
            }
            counters.recall_requests = counters.recall_requests.saturating_add(1);
            if memory_hit {
                counters.recall_hits = counters.recall_hits.saturating_add(1);
            }
        }
        if operation == "project" && result_summary == "projection_completed" {
            let projection_injected = required_payload_bool(&event.payload, "projection_injected")?;
            counters.projection_requests = counters.projection_requests.saturating_add(1);
            if projection_injected {
                counters.projection_injections = counters.projection_injections.saturating_add(1);
            }
            let chars = required_payload_u64(&event.payload, "system_memory_chars")?;
            latest_projection = Some((event.timestamp_unix_secs, event.event_id.as_str(), chars));
        }
        if operation == "maintain" && result_summary == "maintenance_completed" {
            counters.maintenance_requests = counters.maintenance_requests.saturating_add(1);
        }
        if optional_payload_bool(&event.payload, "finalize_request")?.unwrap_or(false) {
            counters.finalize_requests = counters.finalize_requests.saturating_add(1);
            if optional_payload_bool(&event.payload, "finalize_committed")?.unwrap_or(false) {
                counters.finalize_committed = counters.finalize_committed.saturating_add(1);
            }
        }
        if optional_payload_bool(&event.payload, "deferred_governance_job")?.unwrap_or(false) {
            counters.deferred_governance_jobs = counters.deferred_governance_jobs.saturating_add(1);
        }
        if operation == "export" && result_summary == "export_completed" {
            counters.migration_exports = counters.migration_exports.saturating_add(1);
        }
        if operation == "import" && result_summary == "import_completed" {
            counters.migration_imports = counters.migration_imports.saturating_add(1);
        }
    }

    Ok(RuntimeMetricsReport {
        schema_version: RUNTIME_METRICS_SCHEMA_VERSION,
        source: RuntimeMetricsSource::CoreRuntimeEvents,
        budget_report_id: budget_report_id.into(),
        write_window_start_unix_secs: query.write_since_unix_secs,
        latest_projection_timestamp_unix_secs: latest_projection.map(|(timestamp, _, _)| timestamp),
        latest_projection_chars: latest_projection.map(|(_, _, chars)| chars),
        evidence,
        counters,
    })
}

fn validate_evidence_summary(
    events: &[RuntimeMetricEvent],
    evidence: &RuntimeMetricEvidenceSummary,
) -> Result<()> {
    let admitted = u64::try_from(events.len()).map_err(|_| {
        Error::config(
            "runtime_metrics_evidence",
            "runtime metric event count exceeds u64",
        )
    })?;
    if evidence.source_store_count == 0
        || evidence.admitted_unique_event_count != admitted
        || evidence.input_event_count
            != evidence
                .admitted_unique_event_count
                .saturating_add(evidence.duplicate_event_count)
    {
        return Err(Error::config(
            "runtime_metrics_evidence",
            "runtime metric evidence summary does not match the admitted event closure",
        ));
    }
    Ok(())
}

fn write_is_in_window(timestamp_unix_secs: u64, query: &RuntimeMetricsQuery) -> bool {
    query
        .write_since_unix_secs
        .is_none_or(|start| timestamp_unix_secs >= start)
}

fn successful_lifecycle_write_changed_count(event: &RuntimeMetricEvent) -> Result<Option<u64>> {
    if event.kind != RuntimeMetricEventKind::RuntimeLifecycle
        || !payload_str(&event.payload, "operation").starts_with("write.")
    {
        return Ok(None);
    }
    if !lifecycle_succeeded(event)? {
        return Ok(None);
    }
    if lifecycle_result_summary(event) != payload_str(&event.payload, "operation") {
        return Err(Error::config(
            "runtime_metrics_write_event",
            "successful write lifecycle event requires an exact operation result summary",
        ));
    }
    lifecycle_changed_count(event).map(Some)
}

fn lifecycle_changed_count(event: &RuntimeMetricEvent) -> Result<u64> {
    optional_payload_u64(&event.payload, "changed_count")?.ok_or_else(|| {
        Error::config(
            "runtime_metrics_write_event",
            "successful write lifecycle event requires a typed changed count",
        )
    })
}

fn lifecycle_result_summary(event: &RuntimeMetricEvent) -> &str {
    payload_str(&event.payload, "result_summary")
}

fn validate_lifecycle_completion(event: &RuntimeMetricEvent) -> Result<()> {
    if event.kind != RuntimeMetricEventKind::RuntimeLifecycle {
        return Ok(());
    }
    let success = required_payload_bool(&event.payload, "success").map_err(|_| {
        Error::config(
            "runtime_metrics_lifecycle_completion",
            "runtime lifecycle event requires a typed success field",
        )
    })?;
    let result = payload_str(&event.payload, "result");
    let result_matches = matches!((success, result), (true, "ok") | (false, "failed"));
    if !result_matches {
        return Err(Error::config(
            "runtime_metrics_lifecycle_completion",
            "runtime lifecycle result must exactly match its typed success field",
        ));
    }
    let finalize_request =
        optional_payload_bool(&event.payload, "finalize_request")?.unwrap_or(false);
    let finalize_committed =
        optional_payload_bool(&event.payload, "finalize_committed")?.unwrap_or(false);
    if finalize_committed && !finalize_request {
        return Err(Error::config(
            "runtime_metrics_lifecycle_completion",
            "finalize_committed requires finalize_request in the same lifecycle event",
        ));
    }
    if !success && lifecycle_claims_success_terminal(event)? {
        return Err(Error::config(
            "runtime_metrics_lifecycle_completion",
            "failed runtime lifecycle event cannot claim a successful terminal state",
        ));
    }
    Ok(())
}

fn lifecycle_succeeded(event: &RuntimeMetricEvent) -> Result<bool> {
    validate_lifecycle_completion(event)?;
    Ok(event.kind == RuntimeMetricEventKind::RuntimeLifecycle
        && required_payload_bool(&event.payload, "success")?)
}

fn lifecycle_claims_success_terminal(event: &RuntimeMetricEvent) -> Result<bool> {
    let operation = payload_str(&event.payload, "operation");
    let result_summary = lifecycle_result_summary(event);
    Ok(
        (operation.starts_with("write.") && result_summary.starts_with("write."))
            || matches!(
                (operation, result_summary),
                ("recall", "recall_completed")
                    | ("project", "projection_completed")
                    | ("maintain", "maintenance_completed")
                    | ("export", "export_completed")
                    | ("import", "import_completed")
            )
            || optional_payload_bool(&event.payload, "finalize_request")?.unwrap_or(false)
            || optional_payload_bool(&event.payload, "finalize_committed")?.unwrap_or(false)
            || optional_payload_bool(&event.payload, "deferred_governance_job")?.unwrap_or(false),
    )
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
    pub metrics_source: RuntimeMetricsSource,
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
            metrics_source: RuntimeMetricsSource::CoreRuntimeEvents,
            unavailable_reasons,
        }
    }
}

pub fn record_runtime_spawn_failure() {}

fn payload_str<'a>(payload: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    payload.get(key).map(String::as_str).unwrap_or_default()
}

fn optional_payload_bool(payload: &BTreeMap<String, String>, key: &str) -> Result<Option<bool>> {
    match payload.get(key).map(String::as_str) {
        None | Some("") => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(_) => Err(Error::config(
            "runtime_metrics_event_payload",
            format!("{key} must be a typed boolean"),
        )),
    }
}

fn required_payload_bool(payload: &BTreeMap<String, String>, key: &str) -> Result<bool> {
    optional_payload_bool(payload, key)?.ok_or_else(|| {
        Error::config(
            "runtime_metrics_event_payload",
            format!("{key} must be present as a typed boolean"),
        )
    })
}

fn optional_payload_u64(payload: &BTreeMap<String, String>, key: &str) -> Result<Option<u64>> {
    payload
        .get(key)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                Error::config(
                    "runtime_metrics_event_payload",
                    format!("{key} must be a typed u64"),
                )
            })
        })
        .transpose()
}

fn required_payload_u64(payload: &BTreeMap<String, String>, key: &str) -> Result<u64> {
    optional_payload_u64(payload, key)?.ok_or_else(|| {
        Error::config(
            "runtime_metrics_event_payload",
            format!("{key} must be present as a typed u64"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lifecycle_event(
        event_id: &str,
        timestamp: u64,
        operation: &str,
        result: &str,
        payload: &[(&str, &str)],
    ) -> RuntimeMetricEvent {
        let mut fields = BTreeMap::from([
            ("operation".to_string(), operation.to_string()),
            ("result_summary".to_string(), result.to_string()),
            ("success".to_string(), "true".to_string()),
            ("result".to_string(), "ok".to_string()),
            ("trigger".to_string(), "sdk_call".to_string()),
        ]);
        fields.extend(
            payload
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string())),
        );
        RuntimeMetricEvent {
            event_id: event_id.to_string(),
            kind: RuntimeMetricEventKind::RuntimeLifecycle,
            timestamp_unix_secs: timestamp,
            payload: fields,
        }
    }

    fn evidence(count: u64) -> RuntimeMetricEvidenceSummary {
        RuntimeMetricEvidenceSummary {
            source_store_count: 1,
            input_event_count: count,
            admitted_unique_event_count: count,
            duplicate_event_count: 0,
        }
    }

    fn build(
        events: Vec<RuntimeMetricEvent>,
        write_since_unix_secs: Option<u64>,
    ) -> RuntimeMetricsReport {
        let count = events.len() as u64;
        build_runtime_metrics_report(
            events,
            RuntimeMetricsQuery {
                write_since_unix_secs,
            },
            evidence(count),
            "budget-1",
        )
        .expect("metrics report")
    }

    #[test]
    fn zero_change_write_does_not_count_as_an_accepted_write() {
        let report = build(
            vec![lifecycle_event(
                "event-1",
                10,
                "write.candidates",
                "write.candidates",
                &[("changed_count", "0")],
            )],
            None,
        );

        assert_eq!(report.counters.write_count, 0);
        assert_eq!(report.counters.write_changed_count, 0);
    }

    #[test]
    fn lifecycle_and_raw_write_from_one_transaction_count_exactly_once() {
        let lifecycle = lifecycle_event(
            "lifecycle-1",
            10,
            "write.candidates",
            "write.candidates",
            &[("changed_count", "1"), ("transaction_id", "transaction-1")],
        );
        let raw = RuntimeMetricEvent {
            event_id: "raw-1".to_string(),
            kind: RuntimeMetricEventKind::MemoryWrite,
            timestamp_unix_secs: 10,
            payload: BTreeMap::from([
                ("transaction_id".to_string(), "transaction-1".to_string()),
                ("operation".to_string(), "write.candidates".to_string()),
            ]),
        };

        let report = build(vec![raw, lifecycle], None);

        assert_eq!(report.counters.write_count, 1);
        assert_eq!(report.counters.write_changed_count, 1);
    }

    #[test]
    fn duplicate_lifecycle_summaries_for_one_transaction_fail_closed() {
        let first = lifecycle_event(
            "lifecycle-1",
            10,
            "write.candidates",
            "write.candidates",
            &[("changed_count", "1"), ("transaction_id", "transaction-1")],
        );
        let second = lifecycle_event(
            "lifecycle-2",
            10,
            "write.candidates",
            "write.candidates",
            &[("changed_count", "1"), ("transaction_id", "transaction-1")],
        );

        let error = build_runtime_metrics_report(
            vec![first, second],
            RuntimeMetricsQuery::default(),
            evidence(2),
            "budget-1",
        )
        .expect_err("one transaction cannot have duplicate lifecycle summaries");

        assert_eq!(error.stage(), "runtime_metrics_write_transaction");
    }

    #[test]
    fn zero_change_lifecycle_cannot_own_a_raw_durable_write() {
        let lifecycle = lifecycle_event(
            "lifecycle-1",
            10,
            "write.candidates",
            "write.candidates",
            &[("changed_count", "0"), ("transaction_id", "transaction-1")],
        );
        let raw = RuntimeMetricEvent {
            event_id: "raw-1".to_string(),
            kind: RuntimeMetricEventKind::MemoryWrite,
            timestamp_unix_secs: 10,
            payload: BTreeMap::from([
                ("transaction_id".to_string(), "transaction-1".to_string()),
                ("operation".to_string(), "write.candidates".to_string()),
            ]),
        };

        let error = build_runtime_metrics_report(
            vec![lifecycle, raw],
            RuntimeMetricsQuery::default(),
            evidence(2),
            "budget-1",
        )
        .expect_err("zero-change lifecycle cannot hide a raw durable write");

        assert_eq!(error.stage(), "runtime_metrics_write_transaction_conflict");
    }

    #[test]
    fn failed_lifecycle_cannot_claim_a_success_terminal_or_increment_counters() {
        let mut contradictory = lifecycle_event(
            "failed-recall",
            10,
            "recall",
            "recall_completed",
            &[("memory_hit", "true"), ("hit_count", "1")],
        );
        contradictory
            .payload
            .insert("success".to_string(), "false".to_string());
        contradictory
            .payload
            .insert("result".to_string(), "failed".to_string());
        let error = build_runtime_metrics_report(
            vec![contradictory],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("failed lifecycle cannot advertise a success terminal");
        assert_eq!(error.stage(), "runtime_metrics_lifecycle_completion");

        let mut failed = lifecycle_event("failed", 11, "recall", "recall_failed", &[]);
        failed
            .payload
            .insert("success".to_string(), "false".to_string());
        failed
            .payload
            .insert("result".to_string(), "failed".to_string());
        let report = build(vec![failed], None);
        assert_eq!(report.counters.recall_requests, 0);
        assert_eq!(report.counters.recall_hits, 0);
    }

    #[test]
    fn lifecycle_result_must_exactly_match_typed_success() {
        let mut mismatched = lifecycle_event(
            "mismatched",
            10,
            "project",
            "projection_completed",
            &[
                ("projection_injected", "true"),
                ("system_memory_chars", "1"),
            ],
        );
        mismatched
            .payload
            .insert("result".to_string(), "failed".to_string());
        let error = build_runtime_metrics_report(
            vec![mismatched],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("success and result must agree");
        assert_eq!(error.stage(), "runtime_metrics_lifecycle_completion");

        let mut missing = lifecycle_event("missing", 11, "maintain", "maintenance_completed", &[]);
        missing.payload.remove("result");
        let error = build_runtime_metrics_report(
            vec![missing],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("current lifecycle schema requires result");
        assert_eq!(error.stage(), "runtime_metrics_lifecycle_completion");
    }

    #[test]
    fn lifecycle_and_raw_transaction_timestamps_must_match_in_both_input_orders() {
        let lifecycle = lifecycle_event(
            "lifecycle",
            20,
            "write.candidates",
            "write.candidates",
            &[("changed_count", "1"), ("transaction_id", "transaction-1")],
        );
        let raw = RuntimeMetricEvent {
            event_id: "raw".to_string(),
            kind: RuntimeMetricEventKind::MemoryWrite,
            timestamp_unix_secs: 19,
            payload: BTreeMap::from([
                ("transaction_id".to_string(), "transaction-1".to_string()),
                ("operation".to_string(), "write.candidates".to_string()),
            ]),
        };

        for events in [
            vec![lifecycle.clone(), raw.clone()],
            vec![raw.clone(), lifecycle.clone()],
        ] {
            let error = build_runtime_metrics_report(
                events,
                RuntimeMetricsQuery::default(),
                evidence(2),
                "budget-1",
            )
            .expect_err("one transaction cannot cross a metric window boundary");
            assert_eq!(error.stage(), "runtime_metrics_write_transaction");
        }
    }

    #[test]
    fn raw_events_from_one_transaction_count_as_one_write_with_each_change() {
        let raw = |event_id: &str| RuntimeMetricEvent {
            event_id: event_id.to_string(),
            kind: RuntimeMetricEventKind::MemoryWrite,
            timestamp_unix_secs: 10,
            payload: BTreeMap::from([
                ("transaction_id".to_string(), "transaction-1".to_string()),
                ("operation".to_string(), "write.candidates".to_string()),
            ]),
        };

        let report = build(vec![raw("raw-1"), raw("raw-2")], None);

        assert_eq!(report.counters.write_count, 1);
        assert_eq!(report.counters.write_changed_count, 2);
    }

    #[test]
    fn raw_event_identity_cannot_collide_with_a_distinct_transaction_identity() {
        let lifecycle = lifecycle_event(
            "lifecycle",
            10,
            "write.candidates",
            "write.candidates",
            &[("changed_count", "1"), ("transaction_id", "shared-id")],
        );
        let raw_without_transaction = RuntimeMetricEvent {
            event_id: "shared-id".to_string(),
            kind: RuntimeMetricEventKind::MemoryWrite,
            timestamp_unix_secs: 10,
            payload: BTreeMap::from([("operation".to_string(), "write.candidates".to_string())]),
        };

        let report = build(vec![lifecycle, raw_without_transaction], None);

        assert_eq!(report.counters.write_count, 2);
        assert_eq!(report.counters.write_changed_count, 2);
    }

    #[test]
    fn successful_write_operation_and_result_summary_must_match_exactly() {
        let event = lifecycle_event(
            "mismatched-write",
            10,
            "write.candidates",
            "write.maintenance",
            &[("changed_count", "1"), ("transaction_id", "transaction-1")],
        );

        let error = build_runtime_metrics_report(
            vec![event],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("write operation and terminal summary must match");

        assert_eq!(error.stage(), "runtime_metrics_write_event");
    }

    #[test]
    fn finalize_commit_requires_a_finalize_request_in_the_same_event() {
        let event = lifecycle_event(
            "contradictory-finalize",
            10,
            "maintain",
            "maintenance_completed",
            &[
                ("finalize_request", "false"),
                ("finalize_committed", "true"),
            ],
        );

        let error = build_runtime_metrics_report(
            vec![event],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("a finalize commit without its request must fail closed");

        assert_eq!(error.stage(), "runtime_metrics_lifecycle_completion");
    }

    #[test]
    fn recall_project_and_inspect_have_disjoint_semantics() {
        let report = build(
            vec![
                lifecycle_event(
                    "recall-hit",
                    10,
                    "recall",
                    "recall_completed",
                    &[("memory_hit", "true"), ("hit_count", "1")],
                ),
                lifecycle_event(
                    "project",
                    11,
                    "project",
                    "projection_completed",
                    &[
                        ("memory_hit", "true"),
                        ("projection_injected", "true"),
                        ("system_memory_chars", "42"),
                    ],
                ),
                lifecycle_event(
                    "inspect",
                    12,
                    "inspect",
                    "recall_completed",
                    &[("memory_hit", "true")],
                ),
            ],
            None,
        );

        assert_eq!(report.counters.recall_requests, 1);
        assert_eq!(report.counters.recall_hits, 1);
        assert_eq!(report.counters.projection_requests, 1);
        assert_eq!(report.counters.projection_injections, 1);
        assert_eq!(report.latest_projection_chars, Some(42));
    }

    #[test]
    fn strict_current_payload_rejects_legacy_booleans_and_missing_projection_fields() {
        let mut numeric_success = lifecycle_event(
            "numeric-success",
            10,
            "write.candidates",
            "write.candidates",
            &[("changed_count", "1"), ("transaction_id", "transaction-1")],
        );
        numeric_success
            .payload
            .insert("success".to_string(), "1".to_string());
        let error = build_runtime_metrics_report(
            vec![numeric_success],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("numeric boolean is not current schema");
        assert_eq!(error.stage(), "runtime_metrics_lifecycle_completion");

        let missing_projection_chars = lifecycle_event(
            "projection",
            11,
            "project",
            "projection_completed",
            &[("projection_injected", "true")],
        );
        let error = build_runtime_metrics_report(
            vec![missing_projection_chars],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("projection chars are required");
        assert_eq!(error.stage(), "runtime_metrics_event_payload");

        let mut legacy_result = lifecycle_event(
            "legacy-result",
            12,
            "write.candidates",
            "write.candidates",
            &[("changed_count", "1"), ("transaction_id", "transaction-2")],
        );
        legacy_result.payload.remove("result_summary");
        legacy_result
            .payload
            .insert("result".to_string(), "write.candidates".to_string());
        let error = build_runtime_metrics_report(
            vec![legacy_result],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("legacy result fallback is forbidden");
        assert_eq!(error.stage(), "runtime_metrics_lifecycle_completion");

        let mut legacy_changed = lifecycle_event(
            "legacy-changed",
            13,
            "write.candidates",
            "write.candidates",
            &[("changed", "true"), ("transaction_id", "transaction-3")],
        );
        legacy_changed.payload.remove("changed_count");
        let error = build_runtime_metrics_report(
            vec![legacy_changed],
            RuntimeMetricsQuery::default(),
            evidence(1),
            "budget-1",
        )
        .expect_err("legacy changed boolean is forbidden");
        assert_eq!(error.stage(), "runtime_metrics_write_event");
    }

    #[test]
    fn raw_transaction_timestamp_drift_fails_closed() {
        let raw = |event_id: &str, timestamp_unix_secs: u64| RuntimeMetricEvent {
            event_id: event_id.to_string(),
            kind: RuntimeMetricEventKind::MemoryWrite,
            timestamp_unix_secs,
            payload: BTreeMap::from([
                ("transaction_id".to_string(), "transaction-1".to_string()),
                ("operation".to_string(), "write.candidates".to_string()),
            ]),
        };
        let error = build_runtime_metrics_report(
            vec![raw("raw-1", 10), raw("raw-2", 11)],
            RuntimeMetricsQuery::default(),
            evidence(2),
            "budget-1",
        )
        .expect_err("one transaction cannot span metric windows");
        assert_eq!(error.stage(), "runtime_metrics_write_transaction");
    }

    #[test]
    fn runtime_metrics_source_has_one_exact_serialized_identity() {
        assert_eq!(
            serde_json::to_value(RuntimeMetricsSource::CoreRuntimeEvents).expect("serialize"),
            serde_json::Value::String("core.runtime_events".to_string())
        );
        assert!(serde_json::from_str::<RuntimeMetricsSource>("\"legacy.runtime_events\"").is_err());
    }

    #[test]
    fn report_schema_contains_only_truthful_owned_metrics() {
        let report = build(Vec::new(), None);
        let value = serde_json::to_value(report).expect("serialize report");
        let object = value.as_object().expect("report object");
        assert!(!object.contains_key("memorySystemOccupancy"));
        assert!(!object.contains_key("storageTotalBytes"));
    }

    #[test]
    fn projection_tie_break_and_write_window_are_deterministic_and_inclusive() {
        let at_start = RuntimeMetricEvent {
            event_id: "raw-at-start".to_string(),
            kind: RuntimeMetricEventKind::MemoryWrite,
            timestamp_unix_secs: 20,
            payload: BTreeMap::from([("operation".to_string(), "write.candidates".to_string())]),
        };
        let before_start = RuntimeMetricEvent {
            event_id: "raw-before-start".to_string(),
            kind: RuntimeMetricEventKind::MemoryWrite,
            timestamp_unix_secs: 19,
            payload: BTreeMap::from([("operation".to_string(), "write.candidates".to_string())]),
        };
        let later_id = lifecycle_event(
            "projection-z",
            30,
            "project",
            "projection_completed",
            &[
                ("projection_injected", "true"),
                ("system_memory_chars", "99"),
            ],
        );
        let earlier_id = lifecycle_event(
            "projection-a",
            30,
            "project",
            "projection_completed",
            &[
                ("projection_injected", "true"),
                ("system_memory_chars", "10"),
            ],
        );

        let report = build(vec![later_id, at_start, earlier_id, before_start], Some(20));

        assert_eq!(report.counters.write_count, 1);
        assert_eq!(report.latest_projection_chars, Some(99));
        assert_eq!(report.latest_projection_timestamp_unix_secs, Some(30));
    }
}
