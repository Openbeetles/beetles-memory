//! Beetle Memory core.
//!
//! This crate contains the standalone memory kernel extracted from the Beetle
//! source truth. Host projects integrate through neutral SDK/store traits; no
//! external communication adapter is implemented here.
#![allow(dead_code, unused_imports)]

mod bus;
pub mod memory;
pub mod skills;

pub mod agent;
pub mod budget;
mod constants;
pub mod error;
pub mod feature_gate;
pub mod i18n;
pub mod llm;
pub mod metrics;
pub mod orchestrator;
pub mod platform;
pub mod reasoning;
pub mod reminder;
pub mod resource;
pub mod runtime;
pub mod state;
pub mod task;
pub mod task_execution;
pub mod tools;
pub mod util;

pub use budget::{EvidenceDocumentRuntimeBudget, RuntimeBudgetAuthority, RuntimeBudgetReport};
pub use error::{Error, Result};
pub use platform::Platform;
pub use reasoning::{load_idle_memory_forge_operator_summary, IdleMemoryForgeAdjudicationState};
