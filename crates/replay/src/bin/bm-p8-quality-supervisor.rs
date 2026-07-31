#![allow(dead_code)]

#[path = "../p8_quality_process.rs"]
mod p8_quality_process;
#[path = "../retained_artifact_fs.rs"]
mod retained_artifact_fs;
#[path = "../sealed_execution.rs"]
mod sealed_execution;

fn main() {
    if let Some(result) = bm_replay::p8_quality::try_run_trusted_supervisor_session_entry() {
        p8_quality_process::exit_role_entry(result);
    }
    p8_quality_process::exit_role_entry(p8_quality_process::run_role_entry(
        p8_quality_process::P8_TRUSTED_SUPERVISOR_SELF_TEST_STDOUT,
        true,
    ));
}
