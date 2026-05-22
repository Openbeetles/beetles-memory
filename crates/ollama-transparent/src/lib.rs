//! Ollama transparent app mode controller contracts for Beetle Memory.

mod config;
mod controller;
mod error;
mod port_owner;
mod preflight;
mod process;
mod report;
mod runner;
mod status;

pub use config::{OllamaTransparentConfig, OllamaTransparentMode};
pub use controller::{
    DisableOllamaTransparentRequest, EnableOllamaTransparentRequest, OllamaTransparentController,
    TransparentController,
};
pub use error::{OllamaTransparentError, OllamaTransparentErrorKey, Result};
pub use port_owner::{
    ClassifyPortOwnerRequest, ObservedProcess, PortBindingReport, PortOwnerClassifier,
    PortOwnerKind, PortOwnerObserver, SystemPortOwnerObserver,
};
pub use preflight::{
    OfficialOllamaStopPlan, OllamaTransparentPreflightReport, PreflightBlocker,
    PreflightBlockerCode,
};
pub use process::{
    ManagedProcessKind, ManagedProcessReport, ProbeReport, ProcessActionReport, ProcessManager,
    SystemProcessManager,
};
pub use report::{
    OllamaTransparentTransitionReport, RollbackReport, TransitionOutcome, TransitionStep,
    TransitionStepReport,
};
pub use runner::{FileSystemRunnerInstaller, ManagedRunnerReport, RunnerInstaller};
pub use status::{
    GatewayFrontReport, OllamaAppReport, OllamaTransparentState, OllamaTransparentStatus,
};
