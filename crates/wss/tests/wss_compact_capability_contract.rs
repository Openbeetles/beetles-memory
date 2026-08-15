#![cfg(feature = "client-compact")]

#[test]
fn compact_client_does_not_pull_in_the_std_governance_model_client() {
    assert!(!bm_entry::entry_governance_model_client_compiled());
}
