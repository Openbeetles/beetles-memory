//! Protocol-neutral adapter contracts for Beetle Memory.

mod contract;
mod dispatch;
mod error;
mod payload;
mod policy;

pub use contract::{
    AdapterAuthContext, AdapterCommand, AdapterEnvelope, AdapterEvent, AdapterOperation,
    AdapterProjectionAuditSummary, AdapterProjectionReport, AdapterRequestIdentity,
    AdapterRequestIdentityError, AdapterRequestIdentityOwner, AdapterResponse, AdapterSdkReport,
    AdapterSource, TransportKind, TransportMode,
};
pub use dispatch::{
    dispatch_adapter_command, dispatch_adapter_command_with_services, project_adapter_report,
    AdapterRuntimeServices,
};
pub use error::{AdapterError, AdapterErrorKey};
pub use payload::{decode_json_adapter_command, AdapterJsonCommandOptions};
pub use policy::{AdapterBudget, AdapterPolicy};
