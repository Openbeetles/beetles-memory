#![allow(dead_code)]

use std::path::PathBuf;
use std::process::ExitCode;

#[path = "../bounded_process.rs"]
mod bounded_process;
#[path = "../p8_artifact_dir.rs"]
mod p8_artifact_dir;
#[path = "../p8_gate_parent.rs"]
mod p8_gate_parent;
#[path = "../p8_semantic.rs"]
mod p8_semantic;
#[path = "../retained_artifact_fs.rs"]
mod retained_artifact_fs;
#[path = "../sealed_execution.rs"]
mod sealed_execution;

use p8_gate_parent::{run_p8_gate_parent, P8GateParentCommand};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        return Err(
            "usage: bm-p8-semantic-gate-parent <verifier-executable> <cwd> <receipt.json>".into(),
        );
    }
    let command = P8GateParentCommand {
        verifier_executable: PathBuf::from(&args[1]),
        cwd: PathBuf::from(&args[2]),
        receipt_path: PathBuf::from(&args[3]),
    };
    run_p8_gate_parent(command)
        .map_err(|failures| format!("P8 gate parent failed: {failures:?}"))?;
    Ok(())
}
