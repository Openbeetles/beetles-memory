//! Ollama transparent app mode controller contracts for Beetle Memory.

mod config;
mod controller;
mod error;
mod lease;
mod port_owner;
mod preflight;
mod process;
mod process_authority;
mod process_receipt;
mod report;
mod runner;
mod status;

pub use config::{
    OllamaTransparentConfig, OllamaTransparentMemoryAuthority, OllamaTransparentMode,
};
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
    OfficialOllamaStopPlan, OfficialOllamaStopTarget, OllamaTransparentPreflightReport,
    PreflightBlocker, PreflightBlockerCode,
};
pub use process::{
    ManagedProcessKind, ManagedProcessOwnershipReport, ManagedProcessReport, ProbeReport,
    ProcessActionReport,
};
pub use report::{
    OllamaTransparentTransitionReport, RollbackReport, TransitionOutcome, TransitionStep,
    TransitionStepReport,
};
pub use runner::{inspect_executable_identity, ExecutableFileIdentity, ManagedRunnerReport};
pub use status::{
    GatewayFrontReport, OllamaAppReport, OllamaTransparentState, OllamaTransparentStatus,
};

#[cfg(test)]
#[path = "public_surface_tests.rs"]
mod public_surface_tests;
