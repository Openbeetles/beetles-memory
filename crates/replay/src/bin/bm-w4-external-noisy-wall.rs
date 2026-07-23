use bm_replay::{
    attach_p7_soul_regression_gate, attach_p7_verifier_performance,
    attest_p7_current_verifier_execution, bind_p7_verifier_identity,
    evaluate_w4_external_noisy_wall, finalize_w4_external_noisy_release_report,
    preflight_p7_runner_release_with_frozen, run_p7_bounded_retained_executable,
    run_p7_soul_regression_gate, validate_p7_runner_preflight_report_with_frozen,
    verify_p7_maximum_rss_evidence, verify_p7_wall_input_context_with_authority,
    P7ArtifactPublishOutcome, P7AuthorityBoundArtifactTransaction, P7ProcessLimits,
    P7ProcessTermination, P7VerifierExecutionAuthority, P7_MAXIMUM_RSS_REPORT_FILE_NAME,
};
#[cfg(all(test, unix))]
use std::fs;
use std::{
    io::{self, Write},
    path::PathBuf,
    process::{self, ExitCode},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static IMMUTABLE_REPORT_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[path = "bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs"]
mod p7_frozen_runner_identity;

use p7_frozen_runner_identity::P7_FROZEN_RUNNER_IDENTITY;

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
    let mode = parse_args(std::env::args().skip(1))?;
    if let OperatorMode::PublishVerifierRelease { benchmark_root } = mode {
        let report = bm_replay::publish_p7_verifier_release(&benchmark_root)
            .map_err(|error| format!("P7 verifier release publication failed: {error:?}"))?;
        println!("{}", report.executable_canonical_path.display());
        return Ok(ExitCode::SUCCESS);
    }
    let mut execution_authority = attest_p7_current_verifier_execution()
        .map_err(|error| format!("P7 verifier execution authority failed: {error:?}"))?;
    match mode {
        OperatorMode::InitializeCohort {
            benchmark_root,
            run_id,
        } => {
            let cohort = execution_authority
                .initialize_cohort(&benchmark_root, &run_id)
                .map_err(|error| format!("P7 cohort initialization failed: {error}"))?;
            println!("{}", cohort.display());
            Ok(ExitCode::SUCCESS)
        }
        OperatorMode::Preflight {
            benchmark_root,
            run_id,
        } => {
            let frozen = P7_FROZEN_RUNNER_IDENTITY
                .ok_or_else(|| "P7 runner identity is not frozen".to_string())?;
            let sdk_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(std::path::Path::parent)
                .ok_or_else(|| "bm-replay is not under the SDK workspace root".to_string())?;
            let report =
                preflight_p7_runner_release_with_frozen(&benchmark_root, sdk_root, frozen, &run_id)
                    .map_err(|error| format!("P7 runner preflight failed: {error:?}"))?;
            let mut body = serde_json::to_vec_pretty(&report)
                .map_err(|error| format!("failed to serialize P7 preflight report: {error}"))?;
            body.push(b'\n');
            let mut cohort = execution_authority
                .open_cohort(&benchmark_root, &run_id)
                .map_err(|error| format!("failed to open P7 cohort owner: {error}"))?;
            let reused = publish_immutable_report(
                &mut cohort,
                "preflight-report.json",
                &body,
                "P7 preflight report",
            )?;
            eprintln!(
                "preflight_publish={}",
                if reused {
                    "reused-identical"
                } else {
                    "published"
                }
            );
            println!("{}", report.executable_canonical_path);
            Ok(ExitCode::SUCCESS)
        }
        OperatorMode::PublishVerifierRelease { .. } => unreachable!("publisher returned above"),
        OperatorMode::VerifyMaximumRss {
            benchmark_root,
            run_id,
        } => run_maximum_rss_verifier(benchmark_root, run_id, &mut execution_authority),
        OperatorMode::OrchestrateMaximumRss {
            benchmark_root,
            run_id,
        } => run_orchestrated_maximum_rss(benchmark_root, run_id, &mut execution_authority),
        OperatorMode::OrchestrateFullWall {
            benchmark_root,
            run_id,
        } => run_orchestrated_full_wall(benchmark_root, run_id, &mut execution_authority),
        OperatorMode::Wall {
            summaries,
            preflight_report,
        } => run_wall(summaries, preflight_report, execution_authority),
    }
}

fn run_sealed_runner(executable: &std::path::Path, args: &[String]) -> Result<(), String> {
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_p7_bounded_retained_executable(
        executable,
        &refs,
        P7ProcessLimits {
            stdout_bytes: 64 * 1024 * 1024,
            stderr_bytes: 64 * 1024 * 1024,
            total_bytes: 96 * 1024 * 1024,
            timeout: Duration::from_secs(12 * 60 * 60),
        },
    )
    .map_err(|error| format!("sealed P7 runner execution failed: {error}"))?;
    if output.termination != P7ProcessTermination::Exited || !output.status.success() {
        return Err(format!(
            "sealed P7 runner failed: termination={:?} status={:?} stderr={}",
            output.termination,
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn publish_orchestrated_preflight(
    benchmark_root: &std::path::Path,
    run_id: &str,
    execution_authority: &mut P7VerifierExecutionAuthority,
) -> Result<bm_replay::P7RunnerPreflightReport, String> {
    let frozen =
        P7_FROZEN_RUNNER_IDENTITY.ok_or_else(|| "P7 runner identity is not frozen".to_string())?;
    let sdk_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| "bm-replay is not under the SDK workspace root".to_string())?;
    let report = preflight_p7_runner_release_with_frozen(benchmark_root, sdk_root, frozen, run_id)
        .map_err(|error| format!("P7 runner preflight failed: {error:?}"))?;
    let mut body = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("failed to serialize P7 preflight report: {error}"))?;
    body.push(b'\n');
    let mut cohort = execution_authority
        .open_cohort(benchmark_root, run_id)
        .map_err(|error| format!("failed to open P7 cohort owner: {error}"))?;
    publish_immutable_report(
        &mut cohort,
        "preflight-report.json",
        &body,
        "P7 preflight report",
    )?;
    Ok(report)
}

fn run_orchestrated_maximum_rss(
    benchmark_root: PathBuf,
    run_id: String,
    execution_authority: &mut P7VerifierExecutionAuthority,
) -> Result<ExitCode, String> {
    execution_authority
        .initialize_cohort(&benchmark_root, &run_id)
        .map_err(|error| format!("P7 cohort initialization failed: {error}"))?;
    let preflight = publish_orchestrated_preflight(&benchmark_root, &run_id, execution_authority)?;
    let runner = PathBuf::from(&preflight.executable_canonical_path);
    run_sealed_runner(
        &runner,
        &[
            "--measure-max-rss".to_string(),
            "--root".to_string(),
            benchmark_root.to_string_lossy().into_owned(),
            "--run-id".to_string(),
            run_id.clone(),
        ],
    )?;
    execution_authority
        .verify_retained()
        .map_err(|error| format!("P7 verifier release changed: {error:?}"))?;
    let status = run_sealed_operator(
        execution_authority.release_executable_path(),
        &[
            "--verify-max-rss".to_string(),
            "--benchmark-root".to_string(),
            benchmark_root.to_string_lossy().into_owned(),
            "--run-id".to_string(),
            run_id.clone(),
        ],
    )?;
    if status != ExitCode::SUCCESS {
        return Ok(status);
    }
    run_sealed_runner(
        &runner,
        &[
            "--admit-cohort".to_string(),
            "--root".to_string(),
            benchmark_root.to_string_lossy().into_owned(),
            "--run-id".to_string(),
            run_id.clone(),
        ],
    )?;
    bm_replay::verify_p7_cohort_admission(&benchmark_root, &run_id)
        .map_err(|error| format!("P7 cohort admission verification failed: {error:?}"))?;
    println!(
        "{}",
        benchmark_root.join("results/runs").join(run_id).display()
    );
    Ok(ExitCode::SUCCESS)
}

fn run_orchestrated_full_wall(
    benchmark_root: PathBuf,
    run_id: String,
    execution_authority: &mut P7VerifierExecutionAuthority,
) -> Result<ExitCode, String> {
    execution_authority
        .verify_retained()
        .map_err(|error| format!("P7 verifier release changed: {error:?}"))?;
    let (admission, _) = bm_replay::verify_p7_cohort_admission(&benchmark_root, &run_id)
        .map_err(|error| format!("P7 full wall requires governed admission: {error:?}"))?;
    let runner = benchmark_root
        .join("runner/releases")
        .join(&admission.release.executable_sha256)
        .join("beetle-memory-external-bench-runner");
    let matrix = [
        ("locomo", 10_usize),
        ("longmemeval_oracle", 1),
        ("longmemeval_s_cleaned", 1),
        ("longmemeval_m_cleaned", 8),
    ];
    for (suite, total) in matrix {
        for index in 0..total {
            run_sealed_runner(
                &runner,
                &[
                    "--root".to_string(),
                    benchmark_root.to_string_lossy().into_owned(),
                    "--run-id".to_string(),
                    run_id.clone(),
                    "--suite".to_string(),
                    suite.to_string(),
                    "--shard-index".to_string(),
                    index.to_string(),
                    "--shard-total".to_string(),
                    total.to_string(),
                ],
            )?;
        }
        run_sealed_runner(
            &runner,
            &[
                "--merge-suite".to_string(),
                suite.to_string(),
                "--root".to_string(),
                benchmark_root.to_string_lossy().into_owned(),
                "--run-id".to_string(),
                run_id.clone(),
                "--shard-total".to_string(),
                total.to_string(),
            ],
        )?;
    }
    let cohort = benchmark_root.join("results/runs").join(&run_id);
    let summaries = matrix
        .into_iter()
        .map(|(suite, _)| cohort.join(format!("{suite}.merged.summary.json")))
        .collect::<Vec<_>>();
    let mut args = vec![
        "--preflight-report".to_string(),
        cohort
            .join("preflight-report.json")
            .to_string_lossy()
            .into_owned(),
    ];
    for summary in summaries {
        args.push("--summary".to_string());
        args.push(summary.to_string_lossy().into_owned());
    }
    execution_authority
        .verify_retained()
        .map_err(|error| format!("P7 verifier release changed: {error:?}"))?;
    run_sealed_operator(execution_authority.release_executable_path(), &args)
}

fn run_sealed_operator(executable: &std::path::Path, args: &[String]) -> Result<ExitCode, String> {
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_p7_bounded_retained_executable(
        executable,
        &refs,
        P7ProcessLimits {
            stdout_bytes: 64 * 1024 * 1024,
            stderr_bytes: 64 * 1024 * 1024,
            total_bytes: 96 * 1024 * 1024,
            timeout: Duration::from_secs(12 * 60 * 60),
        },
    )
    .map_err(|error| format!("sealed P7 operator execution failed: {error}"))?;
    if output.termination != P7ProcessTermination::Exited {
        return Err(format!(
            "sealed P7 operator was terminated: {:?}",
            output.termination
        ));
    }
    io::stdout()
        .write_all(&output.stdout)
        .map_err(|error| format!("failed to forward sealed operator stdout: {error}"))?;
    io::stderr()
        .write_all(&output.stderr)
        .map_err(|error| format!("failed to forward sealed operator stderr: {error}"))?;
    let code = output
        .status
        .code()
        .ok_or_else(|| "sealed P7 operator exited without a status code".to_string())?;
    let code = u8::try_from(code)
        .map_err(|_| format!("sealed P7 operator returned invalid status {code}"))?;
    Ok(ExitCode::from(code))
}

fn publish_immutable_report(
    cohort: &mut P7AuthorityBoundArtifactTransaction<'_>,
    report_name: &str,
    body: &[u8],
    report_label: &str,
) -> Result<bool, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let sequence = IMMUTABLE_REPORT_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let staged_name = format!("{report_name}.tmp-{}-{nonce}-{sequence}", process::id());
    cohort
        .publish_immutable_bytes(body, &staged_name, report_name)
        .map(|outcome| outcome == P7ArtifactPublishOutcome::ReusedIdentical)
        .map_err(|error| {
            format!(
                "failed to publish immutable {report_label} {}: {error}",
                cohort.path().join(report_name).display()
            )
        })
}

#[cfg(all(test, unix))]
fn require_regular_report(report_path: &std::path::Path, report_label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(report_path).map_err(|error| {
        format!(
            "failed to inspect {report_label} {}: {error}",
            report_path.display()
        )
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "{report_label} must be a regular file, not a symlink: {}",
            report_path.display()
        ));
    }
    Ok(())
}

fn run_maximum_rss_verifier(
    benchmark_root: PathBuf,
    run_id: String,
    execution_authority: &mut P7VerifierExecutionAuthority,
) -> Result<ExitCode, String> {
    let evidence = verify_p7_maximum_rss_evidence(&benchmark_root, &run_id)
        .map_err(|error| format!("P7 maximum RSS verification failed: {error:?}"))?;
    let frozen =
        P7_FROZEN_RUNNER_IDENTITY.ok_or_else(|| "P7 runner identity is not frozen".to_string())?;
    validate_p7_runner_preflight_report_with_frozen(
        &benchmark_root,
        &run_id,
        &evidence.preflight,
        frozen,
    )
    .map_err(|error| format!("P7 frozen preflight verification failed: {error:?}"))?;
    let mut cohort = execution_authority
        .open_cohort(&benchmark_root, &run_id)
        .map_err(|error| format!("failed to open P7 cohort owner: {error}"))?;
    let report_path = cohort.path().join(P7_MAXIMUM_RSS_REPORT_FILE_NAME);
    let mut body = serde_json::to_vec_pretty(&evidence)
        .map_err(|error| format!("failed to serialize P7 maximum RSS evidence: {error}"))?;
    body.push(b'\n');
    let reused = publish_immutable_report(
        &mut cohort,
        P7_MAXIMUM_RSS_REPORT_FILE_NAME,
        &body,
        "P7 maximum RSS evidence",
    )?;
    eprintln!(
        "maximum_rss_publish={}",
        if reused {
            "reused-identical"
        } else {
            "published"
        }
    );
    println!("{}", report_path.display());
    if evidence.rss_gate_passed {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(12))
    }
}

fn run_wall(
    summary_paths: Vec<PathBuf>,
    preflight_report_path: PathBuf,
    execution_authority: P7VerifierExecutionAuthority,
) -> Result<ExitCode, String> {
    let verifier_started = Instant::now();
    let cohort_dir = validate_wall_preflight_owner(&summary_paths, &preflight_report_path)?;
    let run_id = cohort_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "P7 cohort path has no UTF-8 run id".to_string())?;
    let benchmark_root = cohort_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .ok_or_else(|| "P7 cohort is not under <benchmark-root>/results/runs".to_string())?;
    let mut verified = verify_p7_wall_input_context_with_authority(
        &summary_paths,
        &preflight_report_path,
        execution_authority,
    )
    .map_err(|error| format!("P7 wall input verification failed: {error:?}"))?;
    let mut evaluated = evaluate_w4_external_noisy_wall(verified.summaries());
    bind_p7_verifier_identity(
        &mut evaluated,
        verified.summaries(),
        verified.verifier_identity().clone(),
    );
    let (preflight_report, verified_maximum_rss, performance) =
        verified.release_inputs(verifier_started.elapsed());
    let sdk_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| "bm-replay is not under the SDK workspace root".to_string())?;
    attach_p7_soul_regression_gate(
        &mut evaluated,
        run_p7_soul_regression_gate(sdk_root)
            .map_err(|error| format!("P7 Soul regression gate failed to execute: {error:?}"))?,
    );
    attach_p7_verifier_performance(&mut evaluated, performance);
    let report = finalize_w4_external_noisy_release_report(
        evaluated,
        preflight_report,
        verified_maximum_rss,
    )
    .map_err(|error| format!("failed to finalize P7 release report: {error:?}"))?;
    let mut body = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("failed to serialize W4 external noisy wall report: {error}"))?;
    body.push(b'\n');
    let verifier_digest = report.verifier_identity.verifier_digest.as_str();
    if verifier_digest.len() != 64 {
        return Err("P7 verifier identity digest is invalid".to_string());
    }
    let operator_report_path = cohort_dir.join(format!("operator-report-{verifier_digest}.json"));
    let operator_report_name = format!("operator-report-{verifier_digest}.json");
    let mut cohort = verified
        .open_cohort(benchmark_root, run_id)
        .map_err(|error| format!("failed to open retained P7 cohort transaction: {error:?}"))?;
    let reused = publish_immutable_report(
        &mut cohort,
        &operator_report_name,
        &body,
        "P7 external noisy wall operator report",
    )?;
    eprintln!(
        "operator_report_publish={}",
        if reused {
            "reused-identical"
        } else {
            "published"
        }
    );
    println!("{}", operator_report_path.display());

    if report.release_gate_passed {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(11))
    }
}

fn validate_wall_preflight_owner(
    summary_paths: &[PathBuf],
    preflight_report_path: &std::path::Path,
) -> Result<PathBuf, String> {
    let cohort_dir = summary_paths
        .first()
        .and_then(|path| path.parent())
        .ok_or_else(|| "at least one --summary <path> is required".to_string())?
        .to_path_buf();
    let expected_preflight_report = cohort_dir.join("preflight-report.json");
    if preflight_report_path != expected_preflight_report {
        return Err(format!(
            "--preflight-report must be {}",
            expected_preflight_report.display()
        ));
    }
    let preflight_cohort_dir = preflight_report_path
        .parent()
        .ok_or_else(|| "preflight report has no cohort directory".to_string())?
        .to_path_buf();
    if preflight_cohort_dir != cohort_dir {
        return Err("preflight report and summaries must share one cohort owner".to_string());
    }
    Ok(cohort_dir)
}

enum OperatorMode {
    InitializeCohort {
        benchmark_root: PathBuf,
        run_id: String,
    },
    Preflight {
        benchmark_root: PathBuf,
        run_id: String,
    },
    PublishVerifierRelease {
        benchmark_root: PathBuf,
    },
    VerifyMaximumRss {
        benchmark_root: PathBuf,
        run_id: String,
    },
    OrchestrateMaximumRss {
        benchmark_root: PathBuf,
        run_id: String,
    },
    OrchestrateFullWall {
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
    let mut initialize_cohort = false;
    let mut preflight = false;
    let mut publish_verifier_release = false;
    let mut verify_maximum_rss = false;
    let mut orchestrate_maximum_rss = false;
    let mut orchestrate_full_wall = false;
    let mut benchmark_root = None;
    let mut run_id = None;
    let mut preflight_report = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--initialize-cohort" => initialize_cohort = true,
            "--preflight" => preflight = true,
            "--publish-verifier-release" => publish_verifier_release = true,
            "--verify-max-rss" => verify_maximum_rss = true,
            "--orchestrate-max-rss" => orchestrate_maximum_rss = true,
            "--orchestrate-full-wall" => orchestrate_full_wall = true,
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

    if usize::from(initialize_cohort)
        + usize::from(preflight)
        + usize::from(publish_verifier_release)
        + usize::from(verify_maximum_rss)
        + usize::from(orchestrate_maximum_rss)
        + usize::from(orchestrate_full_wall)
        > 1
    {
        return Err(
            "--initialize-cohort, --preflight, --publish-verifier-release, and --verify-max-rss are mutually exclusive".to_string(),
        );
    }
    if publish_verifier_release {
        if !summaries.is_empty() || preflight_report.is_some() || run_id.is_some() {
            return Err(
                "--publish-verifier-release accepts only --benchmark-root <path>".to_string(),
            );
        }
        let benchmark_root = benchmark_root.ok_or_else(|| {
            "--publish-verifier-release requires --benchmark-root <path>".to_string()
        })?;
        return Ok(OperatorMode::PublishVerifierRelease { benchmark_root });
    }
    if initialize_cohort
        || preflight
        || verify_maximum_rss
        || orchestrate_maximum_rss
        || orchestrate_full_wall
    {
        if !summaries.is_empty() {
            return Err("verification modes cannot be combined with --summary".to_string());
        }
        if preflight_report.is_some() {
            return Err(
                "verification modes cannot be combined with --preflight-report".to_string(),
            );
        }
        let mode = if initialize_cohort {
            "--initialize-cohort"
        } else if preflight {
            "--preflight"
        } else if verify_maximum_rss {
            "--verify-max-rss"
        } else if orchestrate_maximum_rss {
            "--orchestrate-max-rss"
        } else {
            "--orchestrate-full-wall"
        };
        let benchmark_root =
            benchmark_root.ok_or_else(|| format!("{mode} requires --benchmark-root <path>"))?;
        let run_id = run_id.ok_or_else(|| format!("{mode} requires --run-id <run-id>"))?;
        return Ok(if initialize_cohort {
            OperatorMode::InitializeCohort {
                benchmark_root,
                run_id,
            }
        } else if preflight {
            OperatorMode::Preflight {
                benchmark_root,
                run_id,
            }
        } else if verify_maximum_rss {
            OperatorMode::VerifyMaximumRss {
                benchmark_root,
                run_id,
            }
        } else if orchestrate_maximum_rss {
            OperatorMode::OrchestrateMaximumRss {
                benchmark_root,
                run_id,
            }
        } else {
            OperatorMode::OrchestrateFullWall {
                benchmark_root,
                run_id,
            }
        });
    }
    if benchmark_root.is_some() || run_id.is_some() {
        return Err(
            "--benchmark-root and --run-id are only valid with a verification mode".to_string(),
        );
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
    "usage: bm-w4-external-noisy-wall --orchestrate-max-rss --benchmark-root <path> --run-id <run-id>\n       bm-w4-external-noisy-wall --orchestrate-full-wall --benchmark-root <path> --run-id <run-id>\n       bm-w4-external-noisy-wall --publish-verifier-release --benchmark-root <path>\n       bm-w4-external-noisy-wall --preflight-report <results/runs/<run-id>/preflight-report.json> --summary <results/runs/<run-id>/suite.merged.summary.json> [--summary <...> ...]".to_string()
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::require_regular_report;
    use super::{parse_args, validate_wall_preflight_owner, OperatorMode};
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
    fn verifier_publisher_is_an_isolated_root_only_mode() {
        let mode = parse_args(
            [
                "--publish-verifier-release",
                "--benchmark-root",
                "benchmark-root",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .expect("verifier publisher mode");
        assert!(matches!(mode, OperatorMode::PublishVerifierRelease { .. }));

        for rejected in [
            vec!["--publish-verifier-release"],
            vec![
                "--publish-verifier-release",
                "--benchmark-root",
                "benchmark-root",
                "--run-id",
                "run-a",
            ],
            vec![
                "--publish-verifier-release",
                "--benchmark-root",
                "benchmark-root",
                "--summary",
                "summary.json",
            ],
        ] {
            assert!(
                parse_args(rejected.into_iter().map(str::to_string)).is_err(),
                "verifier publisher accepted conflicting arguments"
            );
        }
    }

    #[test]
    fn maximum_rss_verifier_requires_explicit_root_and_run_id() {
        let missing_root = parse_args(
            ["--verify-max-rss", "--run-id", "run-a"]
                .into_iter()
                .map(str::to_string),
        )
        .err()
        .expect("maximum RSS verifier must require the benchmark root");
        assert!(missing_root.contains("--benchmark-root"), "{missing_root}");

        let missing_run = parse_args(
            ["--verify-max-rss", "--benchmark-root", "benchmark-root"]
                .into_iter()
                .map(str::to_string),
        )
        .err()
        .expect("maximum RSS verifier must require the run id");
        assert!(missing_run.contains("--run-id"), "{missing_run}");

        let mode = parse_args(
            [
                "--verify-max-rss",
                "--benchmark-root",
                "benchmark-root",
                "--run-id",
                "run-a",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .expect("explicit maximum RSS verifier arguments must parse");
        assert!(matches!(mode, OperatorMode::VerifyMaximumRss { .. }));
    }

    #[test]
    fn maximum_rss_verifier_rejects_wall_and_preflight_inputs() {
        for extra in [
            vec!["--preflight"],
            vec!["--summary", "summary.json"],
            vec!["--preflight-report", "preflight.json"],
        ] {
            let mut args = vec![
                "--verify-max-rss",
                "--benchmark-root",
                "benchmark-root",
                "--run-id",
                "run-a",
            ];
            args.extend(extra);
            assert!(
                parse_args(args.into_iter().map(str::to_string)).is_err(),
                "maximum RSS verifier must be an isolated mode"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn report_consumption_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "bm-p7-report-consumption-symlink-{}-{}",
            std::process::id(),
            super::IMMUTABLE_REPORT_TEMP_SEQUENCE
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create report root");
        let target = root.join("target.json");
        fs::write(&target, b"{}\n").expect("write report target");
        let report = root.join("report.json");
        symlink(&target, &report).expect("create report symlink");

        let error = require_regular_report(&report, "test report")
            .expect_err("report consumption must reject a symlink");
        assert!(error.contains("regular file"), "{error}");

        let _ = fs::remove_dir_all(root);
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

        let error = validate_wall_preflight_owner(
            std::slice::from_ref(&summary),
            &cohort.join("other-preflight.json"),
        )
        .expect_err("operator must reject a report outside the cohort contract");

        assert!(error.contains("--preflight-report must be"), "{error}");
        let _ = fs::remove_dir_all(root);
    }
}
