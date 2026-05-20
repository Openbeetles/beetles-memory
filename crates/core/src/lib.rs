//! Beetle Memory core.
//!
//! This crate contains the standalone memory kernel extracted from the Beetle
//! source truth. Host projects integrate through neutral SDK/store traits; no
//! external communication adapter is implemented here.
#![allow(dead_code, unused_imports, unexpected_cfgs)]

mod bus;
pub mod memory;
pub mod skills;

pub mod agent;
mod constants;
pub mod error;
pub mod i18n;
pub mod llm;
pub mod metrics;
pub mod orchestrator;
pub mod platform;
pub mod reasoning;
pub mod reminder;
pub mod runtime;
pub mod state;
pub mod task;
pub mod task_execution;
pub mod tools;
pub mod util;

pub use error::{Error, Result};
pub use platform::Platform;
pub use reasoning::{load_idle_memory_forge_operator_summary, IdleMemoryForgeAdjudicationState};
