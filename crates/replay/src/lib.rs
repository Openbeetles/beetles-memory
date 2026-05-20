//! Replay-facing contracts for Beetle Memory.

pub use bm_core::memory::{
    inspect_intelligence_replay, IntelligenceReplayAlert, IntelligenceReplayInspection,
    IntelligenceReplayTurnDigest, RecallSelectionReport, TurnLedger, TurnLedgerStore,
};
pub use bm_core::Result;

pub fn inspect_turn_replay(
    store: &dyn TurnLedgerStore,
    chat_id: &str,
    limit: usize,
) -> Result<IntelligenceReplayInspection> {
    inspect_intelligence_replay(store, chat_id, limit)
}
