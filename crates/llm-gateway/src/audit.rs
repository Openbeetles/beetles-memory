use crate::GatewayScopeResolution;

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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GatewayAuditReport {
    pub audit_id: String,
    pub endpoint: String,
    pub client_profile: String,
    pub model_alias: String,
    pub scope: GatewayScopeResolution,
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
}
