use bm_replay::{exec_p7_retained_executable, run_p7_retained_executable};
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut args = std::env::args().skip(1);
    let exec = args.next().as_deref() == Some("--exec");
    if !exec {
        args = std::env::args().skip(1);
    }
    if args.next().as_deref() != Some("--executable") {
        return Err(usage());
    }
    let executable = args.next().map(PathBuf::from).ok_or_else(usage)?;
    if args.next().as_deref() != Some("--") {
        return Err(usage());
    }
    let child_args = args.collect::<Vec<_>>();
    if exec {
        exec_p7_retained_executable(&executable, &child_args)
            .map_err(|error| format!("retained executable exec failed: {error}"))?;
        unreachable!("successful retained exec does not return");
    }
    let status = run_p7_retained_executable(&executable, &child_args)
        .map_err(|error| format!("retained executable launch failed: {error}"))?;
    Ok(ExitCode::from(
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1),
    ))
}

fn usage() -> String {
    "usage: bm-p7-retained-launch [--exec] --executable <absolute-path> -- [args...]".to_string()
}
