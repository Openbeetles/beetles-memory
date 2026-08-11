#![allow(dead_code)]

#[path = "bm-p8-quality-operator/p8_frozen_quality_policy.rs"]
mod p8_frozen_quality_policy;
#[path = "../p8_quality_process.rs"]
mod p8_quality_process;
#[path = "../retained_artifact_fs.rs"]
mod retained_artifact_fs;
#[path = "../sealed_execution.rs"]
mod sealed_execution;

fn main() {
    let _policy_anchor = &p8_frozen_quality_policy::P8_FROZEN_QUALITY_POLICY;
    if let Some(result) = bm_replay::p8_quality::try_run_fixture_operator_session_entry() {
        p8_quality_process::exit_role_entry(result);
    }
    p8_quality_process::exit_role_entry(p8_quality_process::run_role_entry(
        p8_quality_process::P8_QUALITY_OPERATOR_SELF_TEST_STDOUT,
        false,
    ));
}
