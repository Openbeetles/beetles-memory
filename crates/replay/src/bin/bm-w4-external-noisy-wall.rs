use bm_replay::{
    evaluate_w4_external_noisy_wall, preflight_p7_runner_release,
    validate_p7_runner_preflight_report, verify_w4_external_noisy_summary_files,
    w4_external_noisy_summary_with_provenance, P7RunnerPreflightReport,
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::{self, ExitCode},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static PREFLIGHT_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    match parse_args(std::env::args().skip(1))? {
        OperatorMode::Preflight {
            benchmark_root,
            run_id,
        } => {
            let report = preflight_p7_runner_release(&benchmark_root, &run_id)
                .map_err(|error| format!("P7 runner preflight failed: {error:?}"))?;
            let body = serde_json::to_string_pretty(&report)
                .map_err(|error| format!("failed to serialize P7 preflight report: {error}"))?;
            let report_path = benchmark_root
                .join("results/runs")
                .join(&run_id)
                .join("preflight-report.json");
            let report_dir = report_path
                .parent()
                .ok_or_else(|| "P7 preflight report has no cohort directory".to_string())?;
            fs::create_dir_all(report_dir).map_err(|error| {
                format!(
                    "failed to create P7 preflight cohort directory {}: {error}",
                    report_dir.display()
                )
            })?;
            if report_path.exists() {
                let existing_body = fs::read_to_string(&report_path).map_err(|error| {
                    format!(
                        "failed to read existing P7 preflight report {}: {error}",
                        report_path.display()
                    )
                })?;
                let existing = serde_json::from_str::<P7RunnerPreflightReport>(&existing_body)
                    .map_err(|error| {
                        format!(
                            "invalid existing P7 preflight report {}: {error}",
                            report_path.display()
                        )
                    })?;
                validate_p7_runner_preflight_report(&benchmark_root, &run_id, &existing).map_err(
                    |error| {
                        format!(
                            "existing P7 preflight report failed current validation {}: {error:?}",
                            report_path.display()
                        )
                    },
                )?;
                if existing != report {
                    return Err(format!(
                        "existing P7 preflight report is immutable and differs from current producer: {}",
                        report_path.display()
                    ));
                }
            } else {
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default();
                let sequence = PREFLIGHT_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let tmp_path = report_path.with_file_name(format!(
                    "preflight-report.json.tmp-{}-{nonce}-{sequence}",
                    process::id()
                ));
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&tmp_path)
                    .map_err(|error| {
                        format!(
                            "failed to create P7 preflight temp report {}: {error}",
                            tmp_path.display()
                        )
                    })?;
                file.write_all(body.as_bytes()).map_err(|error| {
                    format!(
                        "failed to write P7 preflight report {}: {error}",
                        report_path.display()
                    )
                })?;
                file.write_all(b"\n").map_err(|error| {
                    format!(
                        "failed to finalize P7 preflight report {}: {error}",
                        report_path.display()
                    )
                })?;
                file.sync_all().map_err(|error| {
                    format!(
                        "failed to sync P7 preflight report {}: {error}",
                        report_path.display()
                    )
                })?;
                drop(file);
                let reused = match fs::hard_link(&tmp_path, &report_path) {
                    Ok(()) => false,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let existing_body =
                            fs::read_to_string(&report_path).map_err(|read_error| {
                                format!(
                                "failed to read concurrent P7 preflight report {}: {read_error}",
                                report_path.display()
                            )
                            })?;
                        let existing =
                            serde_json::from_str::<P7RunnerPreflightReport>(&existing_body)
                                .map_err(|parse_error| {
                                    format!(
                                        "invalid concurrent P7 preflight report {}: {parse_error}",
                                        report_path.display()
                                    )
                                })?;
                        if existing != report {
                            return Err(format!(
                                "concurrent P7 preflight report differs from current producer: {}",
                                report_path.display()
                            ));
                        }
                        true
                    }
                    Err(error) => {
                        return Err(format!(
                            "failed to publish immutable P7 preflight report {} from {}: {error}",
                            report_path.display(),
                            tmp_path.display()
                        ))
                    }
                };
                fs::remove_file(&tmp_path).map_err(|error| {
                    format!(
                        "P7 preflight report published but current temp link could not be released {}: {error}",
                        tmp_path.display()
                    )
                })?;
                eprintln!(
                    "preflight_publish={}",
                    if reused {
                        "reused-identical"
                    } else {
                        "published"
                    }
                );
            }
            println!("{}", report_path.display());
            Ok(ExitCode::SUCCESS)
        }
        OperatorMode::Wall {
            summaries,
            preflight_report,
        } => run_wall(summaries, preflight_report),
    }
}

fn run_wall(
    summary_paths: Vec<PathBuf>,
    preflight_report_path: PathBuf,
) -> Result<ExitCode, String> {
    let mut summaries = Vec::with_capacity(summary_paths.len());
    let mut cohort_dir = None;
    for path in summary_paths {
        let parent = path
            .parent()
            .ok_or_else(|| format!("summary has no cohort directory: {}", path.display()))?;
        if cohort_dir
            .as_ref()
            .is_some_and(|expected: &PathBuf| expected != parent)
        {
            return Err(
                "all merged summaries must be in the same run cohort directory".to_string(),
            );
        }
        cohort_dir.get_or_insert_with(|| parent.to_path_buf());
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
        let summary = w4_external_noisy_summary_with_provenance(&body).map_err(|error| {
            format!(
                "invalid W4 external noisy summary {}: {error:?}",
                path.display()
            )
        })?;
        summaries.push((path, summary));
    }

    let cohort_dir =
        cohort_dir.ok_or_else(|| "at least one --summary <path> is required".to_string())?;
    let run_id = summaries
        .first()
        .map(|(_, summary)| summary.run_id.as_str())
        .ok_or_else(|| "at least one --summary <path> is required".to_string())?;
    let benchmark_root = benchmark_root_for_cohort(&cohort_dir, run_id)?;
    if summaries
        .iter()
        .any(|(_, summary)| summary.run_id != run_id)
    {
        return Err("all merged summaries must carry the same P7 run_id".to_string());
    }
    let expected_preflight_report = cohort_dir.join("preflight-report.json");
    if preflight_report_path != expected_preflight_report {
        return Err(format!(
            "--preflight-report must be {}",
            expected_preflight_report.display()
        ));
    }
    let preflight_body = fs::read_to_string(&preflight_report_path).map_err(|error| {
        format!(
            "failed to read P7 preflight report {}: {error}",
            preflight_report_path.display()
        )
    })?;
    let preflight_report = serde_json::from_str::<P7RunnerPreflightReport>(&preflight_body)
        .map_err(|error| {
            format!(
                "invalid P7 preflight report {}: {error}",
                preflight_report_path.display()
            )
        })?;
    validate_p7_runner_preflight_report(&benchmark_root, run_id, &preflight_report).map_err(
        |error| {
            format!(
                "invalid P7 preflight report {}: {error:?}",
                preflight_report_path.display()
            )
        },
    )?;

    let mut verified_summaries = Vec::with_capacity(summaries.len());
    for (path, mut summary) in summaries {
        verify_w4_external_noisy_summary_files(&mut summary, &path).map_err(|error| {
            format!(
                "invalid W4 external noisy provenance {}: {error:?}",
                path.display()
            )
        })?;
        verified_summaries.push(summary);
    }

    let report = evaluate_w4_external_noisy_wall(&verified_summaries);
    let body = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("failed to serialize W4 external noisy wall report: {error}"))?;
    println!("{body}");

    if report.release_gate_passed {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(11))
    }
}

enum OperatorMode {
    Preflight {
        benchmark_root: PathBuf,
        run_id: String,
    },
    Wall {
        summaries: Vec<PathBuf>,
        preflight_report: PathBuf,
    },
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<OperatorMode, String> {
    let mut args = args.into_iter();
    let mut summaries = Vec::new();
    let mut preflight = false;
    let mut benchmark_root = None;
    let mut run_id = None;
    let mut preflight_report = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--preflight" => preflight = true,
            "--benchmark-root" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--benchmark-root requires a path".to_string())?;
                benchmark_root = Some(PathBuf::from(path));
            }
            "--run-id" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--run-id requires a value".to_string())?;
                run_id = Some(value);
            }
            "--preflight-report" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--preflight-report requires a file path".to_string())?;
                preflight_report = Some(PathBuf::from(path));
            }
            "--summary" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--summary requires a summary file path".to_string())?;
                summaries.push(PathBuf::from(path));
            }
            "--help" | "-h" => return Err(usage()),
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
    }

    if preflight {
        if !summaries.is_empty() {
            return Err("--preflight cannot be combined with --summary".to_string());
        }
        if preflight_report.is_some() {
            return Err("--preflight cannot be combined with --preflight-report".to_string());
        }
        let benchmark_root = benchmark_root
            .ok_or_else(|| "--preflight requires --benchmark-root <path>".to_string())?;
        let run_id = run_id.ok_or_else(|| "--preflight requires --run-id <run-id>".to_string())?;
        return Ok(OperatorMode::Preflight {
            benchmark_root,
            run_id,
        });
    }
    if benchmark_root.is_some() || run_id.is_some() {
        return Err("--benchmark-root and --run-id are only valid with --preflight".to_string());
    }
    if summaries.is_empty() {
        return Err("at least one --summary <path> is required".to_string());
    }

    let preflight_report = preflight_report
        .ok_or_else(|| "wall operator requires --preflight-report <path>".to_string())?;

    Ok(OperatorMode::Wall {
        summaries,
        preflight_report,
    })
}

fn usage() -> String {
    "usage: bm-w4-external-noisy-wall --preflight --benchmark-root <path> --run-id <run-id>\n       bm-w4-external-noisy-wall --preflight-report <results/runs/<run-id>/preflight-report.json> --summary <results/runs/<run-id>/suite.merged.summary.json> [--summary <...> ...]".to_string()
}

fn benchmark_root_for_cohort(
    cohort_dir: &std::path::Path,
    run_id: &str,
) -> Result<PathBuf, String> {
    let runs_dir = cohort_dir
        .parent()
        .ok_or_else(|| "summary cohort directory has no runs parent".to_string())?;
    let results_dir = runs_dir
        .parent()
        .ok_or_else(|| "summary cohort directory has no results parent".to_string())?;
    if cohort_dir.file_name().and_then(|name| name.to_str()) != Some(run_id)
        || runs_dir.file_name().and_then(|name| name.to_str()) != Some("runs")
        || results_dir.file_name().and_then(|name| name.to_str()) != Some("results")
    {
        return Err("merged summaries must be under results/runs/<run-id>".to_string());
    }
    results_dir
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "results directory has no benchmark root".to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_args, run_wall};
    use std::fs;

    #[test]
    fn preflight_requires_run_id() {
        let error = parse_args(
            ["--preflight", "--benchmark-root", "benchmark-root"]
                .into_iter()
                .map(str::to_string),
        )
        .err()
        .expect("P7 preflight must require an explicit run id");

        assert!(error.contains("--run-id"), "{error}");
    }

    #[test]
    fn wall_requires_preflight_report() {
        let error = parse_args(
            ["--summary", "results/runs/run-a/locomo.merged.summary.json"]
                .into_iter()
                .map(str::to_string),
        )
        .err()
        .expect("P7 wall operator must require its typed preflight report");

        assert!(error.contains("--preflight-report"), "{error}");
    }

    #[test]
    fn wall_requires_preflight_report_from_the_summary_cohort() {
        let root =
            std::env::temp_dir().join(format!("bm-p7-preflight-cohort-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cohort = root.join("results/runs/run-a");
        fs::create_dir_all(&cohort).expect("create cohort");
        let summary = cohort.join("locomo.merged.summary.json");
        fs::write(&summary, r#"{"suite":"locomo","run_id":"run-a"}"#).expect("write summary");

        let error = run_wall(vec![summary], cohort.join("other-preflight.json"))
            .expect_err("operator must reject a report outside the cohort contract");

        assert!(error.contains("--preflight-report must be"), "{error}");
        let _ = fs::remove_dir_all(root);
    }
}
