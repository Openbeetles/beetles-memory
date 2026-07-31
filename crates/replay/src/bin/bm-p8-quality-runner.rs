#![allow(dead_code)]

#[path = "../p8_quality_process.rs"]
mod p8_quality_process;
#[path = "../retained_artifact_fs.rs"]
mod retained_artifact_fs;
#[path = "../sealed_execution.rs"]
mod sealed_execution;

fn main() {
    p8_quality_process::exit_role_entry(p8_quality_process::run_role_entry(
        p8_quality_process::P8_QUALITY_RUNNER_SELF_TEST_STDOUT,
        false,
    ));
}
