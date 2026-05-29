use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bm_sdk::MemoryProjectionReport;

use crate::{GatewayAuditConfig, GatewayError, GatewayScopeResolution, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayAuditStage {
    Projection,
    Upstream,
    Maintenance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayAuditOutcome {
    Succeeded,
    Failed,
    Skipped,
    NotExecuted,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayAuditStageReport {
    pub stage: GatewayAuditStage,
    pub outcome: GatewayAuditOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayProjectionAuditStatus {
    NotRecorded,
    Recorded,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayProjectionAuditRecord {
    pub status: GatewayProjectionAuditStatus,
    pub reason: String,
    pub projection_chars: usize,
    pub redacted: bool,
    pub redacted_source_ids: Vec<String>,
    pub local_diagnostic_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
}

impl GatewayProjectionAuditRecord {
    pub fn not_recorded(reason: impl Into<String>, projection_chars: usize) -> Self {
        Self {
            status: GatewayProjectionAuditStatus::NotRecorded,
            reason: reason.into(),
            projection_chars,
            redacted: true,
            redacted_source_ids: Vec::new(),
            local_diagnostic_path: None,
            block: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayAuditReport {
    pub audit_id: String,
    pub endpoint: String,
    pub client_profile: String,
    pub model_alias: String,
    pub scope: GatewayScopeResolution,
    pub projection_record: GatewayProjectionAuditRecord,
    pub stages: Vec<GatewayAuditStageReport>,
    pub notes: Vec<String>,
}

impl GatewayAuditReport {
    pub fn new(
        audit_id: impl Into<String>,
        endpoint: impl Into<String>,
        client_profile: impl Into<String>,
        model_alias: impl Into<String>,
        scope: GatewayScopeResolution,
    ) -> Self {
        Self {
            audit_id: audit_id.into(),
            endpoint: endpoint.into(),
            client_profile: client_profile.into(),
            model_alias: model_alias.into(),
            scope,
            projection_record: GatewayProjectionAuditRecord::not_recorded(
                "projection_not_attempted",
                0,
            ),
            stages: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn record_stage(&mut self, stage: GatewayAuditStage, outcome: GatewayAuditOutcome) {
        self.stages.push(GatewayAuditStageReport { stage, outcome });
    }

    pub fn record_note(&mut self, note: impl Into<String>) {
        let note = note.into();
        if !note.trim().is_empty() {
            self.notes.push(note);
        }
    }

    pub fn record_projection(
        &mut self,
        config: &GatewayAuditConfig,
        projection: &MemoryProjectionReport,
    ) -> Result<()> {
        let projection_chars = projection.system_memory_block.chars().count();
        if !config.enabled {
            self.projection_record = GatewayProjectionAuditRecord::not_recorded(
                "gateway_audit_disabled",
                projection_chars,
            );
            return Ok(());
        }
        if !config.record_raw_projection {
            self.projection_record = GatewayProjectionAuditRecord::not_recorded(
                "raw_projection_recording_disabled",
                projection_chars,
            );
            return Ok(());
        }

        let redaction = redacted_projection_block(projection);
        let mut record = GatewayProjectionAuditRecord {
            status: GatewayProjectionAuditStatus::Recorded,
            reason: if redaction.redacted {
                "raw_projection_recorded_redacted".to_string()
            } else {
                "raw_projection_recorded".to_string()
            },
            projection_chars,
            redacted: redaction.redacted,
            redacted_source_ids: redaction.redacted_source_ids,
            local_diagnostic_path: None,
            block: Some(redaction.block),
        };

        if let Some(dir) = &config.raw_projection_diagnostic_path {
            let diagnostic_path = projection_diagnostic_path(
                dir,
                &self.audit_id,
                &projection.runtime_projection.projection_id,
            );
            record.local_diagnostic_path = Some(diagnostic_path.display().to_string());
            write_projection_diagnostic(dir, &diagnostic_path, &record)?;
            enforce_projection_diagnostic_retention(dir, config.raw_projection_retention_limit)?;
        }
        self.projection_record = record;
        Ok(())
    }
}

struct ProjectionRedaction {
    block: String,
    redacted: bool,
    redacted_source_ids: Vec<String>,
}

fn redacted_projection_block(projection: &MemoryProjectionReport) -> ProjectionRedaction {
    let mut block = projection.system_memory_block.clone();
    let mut redacted = false;
    let mut redacted_source_ids = projection
        .private_disclosure_integrity
        .redacted_source_ids
        .clone();
    for source in &projection
        .runtime_projection
        .protected_private_runtime_context
    {
        if source.source_id.trim().is_empty() {
            continue;
        }
        redacted_source_ids.push(source.source_id.clone());
        let content = source.content.trim();
        if content.is_empty() {
            continue;
        }
        let replacement = format!("[redacted:protected_runtime_context:{}]", source.source_id);
        if block.contains(content) {
            block = block.replace(content, &replacement);
            redacted = true;
        }
    }
    let scrubbed_lines = block
        .lines()
        .map(|line| {
            if contains_private_raw_marker(line) {
                redacted = true;
                "[redacted:private_raw_marker]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>();
    block = scrubbed_lines.join("\n");
    redacted_source_ids.sort();
    redacted_source_ids.dedup();
    ProjectionRedaction {
        block,
        redacted: redacted || !redacted_source_ids.is_empty(),
        redacted_source_ids,
    }
}

fn contains_private_raw_marker(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    lowered.contains("private_raw:")
        || lowered.contains("private-garden-raw:")
        || lowered.contains("private garden raw:")
        || lowered.contains("<private_raw>")
}

fn projection_diagnostic_path(dir: &Path, audit_id: &str, projection_id: &str) -> PathBuf {
    let seed = format!("{audit_id}-{projection_id}");
    let sanitized = sanitize_diagnostic_name(&seed);
    dir.join(format!("gateway-projection-{sanitized}.json"))
}

fn sanitize_diagnostic_name(seed: &str) -> String {
    let mut out = seed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "projection".to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

fn write_projection_diagnostic(
    dir: &Path,
    diagnostic_path: &Path,
    record: &GatewayProjectionAuditRecord,
) -> Result<()> {
    fs::create_dir_all(dir).map_err(|error| projection_io_error("create_dir", error))?;
    let payload = serde_json::to_vec_pretty(record).map_err(|error| {
        GatewayError::runtime_unavailable(format!(
            "projection diagnostic serialize failed: {error}"
        ))
    })?;
    fs::write(diagnostic_path, payload).map_err(|error| projection_io_error("write", error))
}

fn enforce_projection_diagnostic_retention(dir: &Path, limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(GatewayError::invalid_config(
            "audit.raw_projection_retention_limit must be greater than zero",
        ));
    }
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(dir).map_err(|error| projection_io_error("read_dir", error))?;
    for entry in read_dir {
        let entry = entry.map_err(|error| projection_io_error("read_dir_entry", error))?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("gateway-projection-") || !file_name.ends_with(".json") {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|error| projection_io_error("metadata", error))?;
        entries.push((modified, path));
    }
    if entries.len() <= limit {
        return Ok(());
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let remove_count = entries.len() - limit;
    for (_, path) in entries.into_iter().take(remove_count) {
        fs::remove_file(path).map_err(|error| projection_io_error("remove_old", error))?;
    }
    Ok(())
}

fn projection_io_error(action: &str, error: io::Error) -> GatewayError {
    GatewayError::runtime_unavailable(format!("projection diagnostic {action} failed: {error}"))
}
