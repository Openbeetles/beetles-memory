//! Protocol-neutral adapter contracts for Beetle Memory.

mod contract;
mod dispatch;
mod error;
mod policy;

pub use contract::{
    AdapterAuthContext, AdapterCommand, AdapterEnvelope, AdapterEvent, AdapterOperation,
    AdapterResponse, AdapterSdkReport, AdapterSource, TransportKind, TransportMode,
};
pub use dispatch::dispatch_adapter_command;
pub use error::{AdapterError, AdapterErrorKey};
pub use policy::{AdapterBudget, AdapterPolicy};
