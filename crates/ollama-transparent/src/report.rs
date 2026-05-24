use serde::{Deserialize, Serialize};

use crate::OllamaTransparentState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionStep {
    Preflight,
    StopOfficialOllama,
    InstallManagedRunner,
    StartManagedUpstream,
    ProbeManagedUpstream,
    StartTransparentFront,
    ProbePublicFront,
    OpenOfficialApp,
    StopTransparentFront,
    StopManagedUpstream,
    RestoreOfficialApp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionOutcome {
    Completed,
    Rejected,
    Failed,
    RolledBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionStepReport {
    pub step: TransitionStep,
    pub ok: bool,
    pub message: Option<String>,
}

impl TransitionStepReport {
    pub fn ok(step: TransitionStep) -> Self {
        Self {
            step,
            ok: true,
            message: None,
        }
    }

    pub fn failed(step: TransitionStep, message: impl Into<String>) -> Self {
        Self {
            step,
            ok: false,
            message: Some(message.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackReport {
    pub attempted: bool,
    pub completed: bool,
    pub steps: Vec<TransitionStepReport>,
}

impl RollbackReport {
    pub fn not_attempted() -> Self {
        Self {
            attempted: false,
            completed: false,
            steps: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaTransparentTransitionReport {
    pub from_state: OllamaTransparentState,
    pub to_state: OllamaTransparentState,
    pub outcome: TransitionOutcome,
    pub steps: Vec<TransitionStepReport>,
    pub failing_step: Option<TransitionStepReport>,
    pub rollback: Option<RollbackReport>,
}

impl OllamaTransparentTransitionReport {
    pub fn completed(
        from_state: OllamaTransparentState,
        to_state: OllamaTransparentState,
        steps: Vec<TransitionStepReport>,
    ) -> Self {
        Self {
            from_state,
            to_state,
            outcome: TransitionOutcome::Completed,
            steps,
            failing_step: None,
            rollback: None,
        }
    }
}
