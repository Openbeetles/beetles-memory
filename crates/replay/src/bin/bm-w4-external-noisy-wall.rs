use bm_replay::{evaluate_w4_external_noisy_wall, w4_external_noisy_summary_with_provenance};
use std::{fs, path::PathBuf, process::ExitCode};

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
    let args = parse_args(std::env::args().skip(1))?;
    let mut summaries = Vec::with_capacity(args.summaries.len());
    for (summary_sha256, path) in args.summaries {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !file_name.ends_with(".summary.json") {
            return Err(format!(
                "w4_external_noisy_operator only accepts *.summary.json files: {}",
                path.display()
            ));
        }
        let body = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let summary = w4_external_noisy_summary_with_provenance(
            &body,
            summary_sha256,
            args.runner_source_sha256.clone(),
        )
        .map_err(|error| {
            format!(
                "invalid W4 external noisy summary {}: {error:?}",
                path.display()
            )
        })?;
        summaries.push(summary);
    }

    let report = evaluate_w4_external_noisy_wall(&summaries);
    let body = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("failed to serialize W4 external noisy wall report: {error}"))?;
    println!("{body}");

    if report.release_gate_passed {
        Ok(ExitCode::SUCCESS)
    } else if is_expected_current_baseline_block(&report.blocked_reasons) {
        Ok(ExitCode::from(10))
    } else {
        Ok(ExitCode::from(11))
    }
}

fn is_expected_current_baseline_block(blocked_reasons: &[String]) -> bool {
    blocked_reasons
        .iter()
        .any(|reason| reason == "w4_external_noisy_wall_improvement_not_proven")
        && blocked_reasons.iter().all(|reason| {
            matches!(
                reason.as_str(),
                "w4_external_noisy_wall_improvement_not_proven"
                    | "w4_external_noisy_wall_stage_diagnostics_missing"
                    | "w4_external_noisy_wall_index_diagnostics_missing"
                    | "w4_external_noisy_wall_w4_1_diagnostics_missing"
                    | "w4_external_noisy_wall_stage_attribution_not_proven"
                    | "w4_external_noisy_wall_index_effect_not_proven"
            )
        })
}

struct OperatorArgs {
    runner_source_sha256: String,
    summaries: Vec<(String, PathBuf)>,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<OperatorArgs, String> {
    let mut args = args.into_iter();
    let mut runner_source_sha256 = None;
    let mut summaries = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--runner-source-sha256" => {
                runner_source_sha256 = Some(
                    args.next()
                        .ok_or_else(|| "--runner-source-sha256 requires a value".to_string())?,
                );
            }
            "--summary" => {
                let summary_sha256 = args
                    .next()
                    .ok_or_else(|| "--summary requires a sha256 value".to_string())?;
                let path = args
                    .next()
                    .ok_or_else(|| "--summary requires a summary file path".to_string())?;
                summaries.push((summary_sha256, PathBuf::from(path)));
            }
            "--help" | "-h" => return Err(usage()),
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
    }

    let runner_source_sha256 =
        runner_source_sha256.ok_or_else(|| "--runner-source-sha256 is required".to_string())?;
    if summaries.is_empty() {
        return Err("at least one --summary <sha256> <path> pair is required".to_string());
    }

    Ok(OperatorArgs {
        runner_source_sha256,
        summaries,
    })
}

fn usage() -> String {
    "usage: bm-w4-external-noisy-wall --runner-source-sha256 <sha256> --summary <sha256> <suite.merged.summary.json> [--summary <sha256> <suite.merged.summary.json> ...]".to_string()
}
