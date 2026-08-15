//! Protocol-neutral adapter contracts for Beetle Memory.

mod contract;
mod dispatch;
mod error;
mod payload;
mod policy;

pub use contract::{
    AdapterAuthContext, AdapterCommand, AdapterEnvelope, AdapterEvent,
    AdapterGovernedProjectSafeReportV1, AdapterGovernedRecallSafeReportV1,
    AdapterGovernedSafeReportV1, AdapterOperation, AdapterProjectionAuditSummary,
    AdapterProjectionReport, AdapterProtocolBinding, AdapterProtocolCapabilityBinding,
    AdapterProtocolPrivacyBinding, AdapterProtocolRenderBudgetBinding, AdapterRequestIdentity,
    AdapterRequestIdentityError, AdapterRequestIdentityOwner, AdapterResponse, AdapterSdkReport,
    AdapterSource, AdapterTurnFinalizeReport, ExternalAiMemoryProtocolVersion, TransportKind,
    TransportMode,
};
pub use dispatch::{
    dispatch_adapter_command, dispatch_adapter_command_with_services, project_adapter_report,
    AdapterRuntimeServices,
};
pub use error::{AdapterError, AdapterErrorKey};
pub use payload::{
    decode_json_adapter_command, governed_adapter_json_command_schema, AdapterJsonCommandOptions,
    GovernedAdapterJsonCommandSchema,
};
pub use policy::{AdapterBudget, AdapterPolicy};
