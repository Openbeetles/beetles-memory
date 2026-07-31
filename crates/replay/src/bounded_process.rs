//! Generation-neutral bounded child-process supervisor.

use std::{
    io::{self, Read},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, SyncSender},
    thread,
    time::{Duration, Instant},
};

const DRAIN_CHUNK_BYTES: usize = 64 * 1024;
const DRAIN_QUEUE_DEPTH: usize = 8;
const TEARDOWN_GRACE: Duration = Duration::from_secs(2);

#[allow(dead_code)]
#[path = "bounded_process/linux_cgroup_v2.rs"]
pub(crate) mod linux_cgroup_v2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BoundedProcessLimits {
    pub(crate) stdout_bytes: u64,
    pub(crate) stderr_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoundedProcessTermination {
    Exited,
    TimedOut,
    StdoutLimitExceeded,
    StderrLimitExceeded,
    TotalLimitExceeded,
}

pub(crate) struct BoundedProcessOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) termination: BoundedProcessTermination,
    pub(crate) elapsed: Duration,
    pub(crate) pid: u32,
    pub(crate) process_group: i64,
    pub(crate) maximum_rss_bytes: u64,
}

/// 只能由本模块在 direct-child wait/reap 与 stdout/stderr 两个 drain worker 都观察到 EOF
/// 之后构造。该类型不实现 Clone/Serialize/Deserialize，不能由 receipt 字段重建。
#[cfg(target_os = "linux")]
pub(crate) struct ClosedBoundedProcess {
    output: BoundedProcessOutput,
}

#[cfg(target_os = "linux")]
impl ClosedBoundedProcess {
    pub(crate) fn termination(&self) -> BoundedProcessTermination {
        self.output.termination
    }

    pub(crate) fn status(&self) -> ExitStatus {
        self.output.status
    }

    pub(crate) fn stdout(&self) -> &[u8] {
        &self.output.stdout
    }

    pub(crate) fn stderr(&self) -> &[u8] {
        &self.output.stderr
    }

    pub(crate) fn stdout_eof_observed(&self) -> bool {
        true
    }

    pub(crate) fn stderr_eof_observed(&self) -> bool {
        true
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.output.elapsed
    }

    pub(crate) fn maximum_rss_bytes(&self) -> u64 {
        self.output.maximum_rss_bytes
    }

    pub(crate) fn pid(&self) -> u32 {
        self.output.pid
    }

    pub(crate) fn process_group(&self) -> i64 {
        self.output.process_group
    }
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(crate) struct LinuxCgroupV2BoundedOutput {
    pub(crate) process: BoundedProcessOutput,
    pub(crate) executable_identity: crate::sealed_execution::SealedContentIdentity,
    pub(crate) initial: linux_cgroup_v2::LinuxCgroupV2InitialObservation,
    pub(crate) barrier: linux_cgroup_v2::LinuxCgroupV2BarrierObservation,
    pub(crate) final_cgroup_procs: Vec<u8>,
    pub(crate) final_cgroup_events: Vec<u8>,
    pub(crate) memory_peak: Vec<u8>,
    pub(crate) memory_events_after: Vec<u8>,
    pub(crate) memory_events_local_after: Vec<u8>,
}

impl BoundedProcessOutput {
    #[allow(dead_code)]
    pub(crate) fn succeeded(&self) -> bool {
        self.termination == BoundedProcessTermination::Exited && self.status.success()
    }
}

enum DrainEvent {
    Chunk(Stream, Vec<u8>),
    Done(Stream, io::Result<()>),
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

pub(crate) fn run_bounded_command(
    command: &mut Command,
    limits: BoundedProcessLimits,
) -> io::Result<BoundedProcessOutput> {
    validate_limits(limits)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    platform::prepare(command);
    let started = Instant::now();
    let child = command.spawn()?;
    supervise_spawned_child(child, limits, started)
}

#[cfg(target_os = "linux")]
pub(crate) fn run_bounded_command_closed(
    mut command: Command,
    limits: BoundedProcessLimits,
) -> io::Result<ClosedBoundedProcess> {
    match run_bounded_command(&mut command, limits) {
        Ok(output) => Ok(ClosedBoundedProcess { output }),
        Err(_) => std::process::abort(),
    }
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(crate) fn run_bounded_prepared_sealed_linux_cgroup_v2(
    prepared: crate::sealed_execution::PreparedLinuxBarrierSealedExecutable,
    mut run_root: linux_cgroup_v2::LinuxCgroupV2RunRoot,
    initial: linux_cgroup_v2::LinuxCgroupV2InitialObservation,
    limits: BoundedProcessLimits,
    barrier_timeout: Duration,
) -> io::Result<LinuxCgroupV2BoundedOutput> {
    validate_limits(limits)?;
    if barrier_timeout.is_zero() || barrier_timeout > limits.timeout {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cgroup pre-exec barrier timeout is invalid",
        ));
    }
    let started = Instant::now();
    let (mut broker, launch) = prepared.into_broker_and_launch();
    let (sender, receiver) = mpsc::sync_channel(1);
    let launch_thread = thread::spawn(move || {
        if let Err(mpsc::SendError(Ok(mut spawned))) = sender.send(launch.spawn_piped()) {
            let _ = spawned.child.kill();
            let _ = spawned.child.wait();
        }
    });
    let ready_pid = match broker.wait_ready(barrier_timeout) {
        Ok(pid) => pid,
        Err(error) => {
            drop(broker);
            finish_failed_barrier_launch(receiver, launch_thread, None, barrier_timeout)?;
            return Err(error);
        }
    };
    let barrier = match run_root.attach_blocked_child(ready_pid) {
        Ok(observation) => observation,
        Err(error) => {
            drop(broker);
            let kill_result = run_root.kill_all();
            let launch_result = finish_failed_barrier_launch(
                receiver,
                launch_thread,
                Some(ready_pid),
                barrier_timeout,
            );
            let closure_result = wait_cgroup_empty(&mut run_root, barrier_timeout);
            return Err(combine_cleanup_errors(
                error,
                [kill_result, launch_result, closure_result],
            ));
        }
    };
    if let Err(error) = broker.release(ready_pid) {
        let kill_result = run_root.kill_all();
        let launch_result =
            finish_failed_barrier_launch(receiver, launch_thread, Some(ready_pid), barrier_timeout);
        let closure_result = wait_cgroup_empty(&mut run_root, barrier_timeout);
        return Err(combine_cleanup_errors(
            error,
            [kill_result, launch_result, closure_result],
        ));
    }
    let spawned = match receiver.recv_timeout(barrier_timeout) {
        Ok(Ok(spawned)) => spawned,
        Ok(Err(error)) => {
            let kill_result = run_root.kill_all();
            let join_result = finish_launch_worker(launch_thread);
            let closure_result = wait_cgroup_empty(&mut run_root, barrier_timeout);
            return Err(combine_cleanup_errors(
                error,
                [kill_result, join_result, closure_result],
            ));
        }
        Err(_) => {
            let timeout_error = io::Error::new(
                io::ErrorKind::TimedOut,
                "sealed spawn did not complete after barrier release",
            );
            let kill_result = run_root.kill_all();
            let launch_result = finish_failed_barrier_launch(
                receiver,
                launch_thread,
                Some(ready_pid),
                barrier_timeout,
            );
            let closure_result = wait_cgroup_empty(&mut run_root, barrier_timeout);
            return Err(combine_cleanup_errors(
                timeout_error,
                [kill_result, launch_result, closure_result],
            ));
        }
    };
    let mut spawned = spawned;
    if let Err(error) = finish_launch_worker(launch_thread) {
        let kill_result = run_root.kill_all();
        let _ = spawned.child.kill();
        let _ = spawned.child.wait();
        let closure_result = wait_cgroup_empty(&mut run_root, barrier_timeout);
        return Err(combine_cleanup_errors(error, [kill_result, closure_result]));
    }
    if spawned.child.id() != ready_pid {
        let mut child = spawned.child;
        let kill_result = run_root.kill_all();
        let _ = child.kill();
        let _ = child.wait();
        let closure_result = wait_cgroup_empty(&mut run_root, barrier_timeout);
        return Err(combine_cleanup_errors(
            io::Error::other("sealed spawned child differs from barrier ready PID"),
            [kill_result, closure_result],
        ));
    }
    let executable_identity = spawned.identity;
    let process =
        match supervise_spawned_child_with_kill(spawned.child, limits, started, None, || {
            run_root.kill_all()
        }) {
            Ok(process) => process,
            Err(error) => {
                let cleanup = terminate_cgroup_and_wait_empty(&mut run_root, barrier_timeout);
                return Err(combine_cleanup_errors(error, [cleanup]));
            }
        };
    let observations = (|| {
        let final_cgroup_procs = run_root.read_cgroup_procs()?;
        let final_cgroup_events = run_root.read_cgroup_events()?;
        let memory_peak = run_root.read_memory_peak()?;
        let memory_events_after = run_root.read_memory_events()?;
        let memory_events_local_after = run_root.read_memory_events_local()?;
        if !linux_cgroup_v2::parse_cgroup_procs(&final_cgroup_procs)?.is_empty()
            || linux_cgroup_v2::parse_cgroup_populated(&final_cgroup_events)?
        {
            return Err(io::Error::other(
                "cgroup run-root is not empty after sealed process closure",
            ));
        }
        let hierarchical_before =
            linux_cgroup_v2::MemoryEventCounters::parse(&initial.memory_events)?;
        let hierarchical_after = linux_cgroup_v2::MemoryEventCounters::parse(&memory_events_after)?;
        let local_before =
            linux_cgroup_v2::MemoryEventCounters::parse(&initial.memory_events_local)?;
        let local_after = linux_cgroup_v2::MemoryEventCounters::parse(&memory_events_local_after)?;
        if !hierarchical_after
            .checked_delta(&hierarchical_before)?
            .is_zero()
            || !local_after.checked_delta(&local_before)?.is_zero()
        {
            return Err(io::Error::other("cgroup run-root observed an OOM event"));
        }
        Ok((
            final_cgroup_procs,
            final_cgroup_events,
            memory_peak,
            memory_events_after,
            memory_events_local_after,
        ))
    })();
    let (
        final_cgroup_procs,
        final_cgroup_events,
        memory_peak,
        memory_events_after,
        memory_events_local_after,
    ) = match observations {
        Ok(observations) => observations,
        Err(error) => {
            let cleanup = terminate_cgroup_and_wait_empty(&mut run_root, barrier_timeout);
            return Err(combine_cleanup_errors(error, [cleanup]));
        }
    };
    Ok(LinuxCgroupV2BoundedOutput {
        process,
        executable_identity,
        initial,
        barrier,
        final_cgroup_procs,
        final_cgroup_events,
        memory_peak,
        memory_events_after,
        memory_events_local_after,
    })
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
fn finish_failed_barrier_launch(
    receiver: mpsc::Receiver<
        io::Result<crate::sealed_execution::SpawnedLinuxBarrierSealedExecutable>,
    >,
    launch_thread: thread::JoinHandle<()>,
    ready_pid: Option<u32>,
    timeout: Duration,
) -> io::Result<()> {
    let first = receiver.recv_timeout(timeout);
    match first {
        Ok(Ok(mut spawned)) => {
            let _ = spawned.child.kill();
            let _ = spawned.child.wait();
        }
        Ok(Err(_)) => {}
        Err(_) => {
            if let Some(pid) = ready_pid.and_then(|pid| i32::try_from(pid).ok()) {
                // SAFETY: this PID came from the blocked child before exec; SIGKILL is only a
                // fallback after the release writer has already been dropped fail-closed.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
            if let Ok(Ok(mut spawned)) = receiver.recv_timeout(timeout) {
                let _ = spawned.child.kill();
                let _ = spawned.child.wait();
            }
        }
    }
    finish_launch_worker(launch_thread)
}

#[cfg(target_os = "linux")]
fn finish_launch_worker(launch_thread: thread::JoinHandle<()>) -> io::Result<()> {
    let deadline = Instant::now() + TEARDOWN_GRACE;
    while !launch_thread.is_finished() {
        if Instant::now() >= deadline {
            drop(launch_thread);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "sealed launch worker did not terminate within its bounded protocol",
            ));
        }
        thread::sleep(Duration::from_millis(1));
    }
    launch_thread
        .join()
        .map_err(|_| io::Error::other("sealed launch worker panicked"))
}

#[cfg(target_os = "linux")]
fn terminate_cgroup_and_wait_empty(
    run_root: &mut linux_cgroup_v2::LinuxCgroupV2RunRoot,
    timeout: Duration,
) -> io::Result<()> {
    let kill_result = run_root.kill_all();
    let closure_result = wait_cgroup_empty(run_root, timeout);
    match (kill_result, closure_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(kill_error), Ok(())) => Err(kill_error),
        (Ok(()), Err(closure_error)) => Err(closure_error),
        (Err(kill_error), Err(closure_error)) => Err(io::Error::other(format!(
            "cgroup.kill failed ({kill_error}); cgroup closure failed ({closure_error})"
        ))),
    }
}

#[cfg(target_os = "linux")]
fn wait_cgroup_empty(
    run_root: &mut linux_cgroup_v2::LinuxCgroupV2RunRoot,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cgroup deadline overflow"))?;
    let mut last_error = None;
    loop {
        match (run_root.read_cgroup_procs(), run_root.read_cgroup_events()) {
            (Ok(procs), Ok(events))
                if linux_cgroup_v2::parse_cgroup_procs(&procs)?.is_empty()
                    && !linux_cgroup_v2::parse_cgroup_populated(&events)? =>
            {
                return Ok(());
            }
            (Ok(_), Ok(_)) => {}
            (Err(error), _) | (_, Err(error)) => last_error = Some(error),
        }
        if Instant::now() >= deadline {
            return Err(last_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "cgroup run-root did not become empty before deadline",
                )
            }));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
fn combine_cleanup_errors<const N: usize>(
    primary: io::Error,
    cleanup_results: [io::Result<()>; N],
) -> io::Error {
    let cleanup_errors = cleanup_results
        .into_iter()
        .filter_map(Result::err)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    if cleanup_errors.is_empty() {
        primary
    } else {
        io::Error::new(
            primary.kind(),
            format!("{primary}; cleanup failed: {}", cleanup_errors.join("; ")),
        )
    }
}

pub(crate) fn supervise_spawned_child(
    child: Child,
    limits: BoundedProcessLimits,
    started: Instant,
) -> io::Result<BoundedProcessOutput> {
    supervise_spawned_child_with_kill(child, limits, started, None, || Ok(()))
}

#[cfg(target_os = "linux")]
pub(crate) fn supervise_spawned_child_closed(
    child: Child,
    limits: BoundedProcessLimits,
    started: Instant,
) -> io::Result<ClosedBoundedProcess> {
    supervise_spawned_child(child, limits, started).map(|output| ClosedBoundedProcess { output })
}

#[cfg(target_os = "linux")]
pub(crate) fn supervise_spawned_child_closed_before(
    child: Child,
    limits: BoundedProcessLimits,
    started: Instant,
    hard_deadline: Instant,
) -> io::Result<ClosedBoundedProcess> {
    if hard_deadline <= Instant::now() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "bounded supervisor hard closure deadline elapsed",
        ));
    }
    supervise_spawned_child_with_kill(child, limits, started, Some(hard_deadline), || Ok(()))
        .map(|output| ClosedBoundedProcess { output })
}

fn supervise_spawned_child_with_kill(
    mut child: Child,
    limits: BoundedProcessLimits,
    started: Instant,
    hard_deadline: Option<Instant>,
    mut kill_extension: impl FnMut() -> io::Result<()>,
) -> io::Result<BoundedProcessOutput> {
    let pid = child.id();
    let mut tree = match platform::ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let extension_error = kill_extension().err();
            let _ = child.kill();
            let _ = child.wait();
            return Err(extension_error.unwrap_or(error));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            tree.kill(&mut child);
            let extension_error = kill_extension().err();
            let _ = child.wait();
            return Err(extension_error.unwrap_or_else(|| {
                io::Error::other("bounded supervisor did not receive child stdout")
            }));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            tree.kill(&mut child);
            let extension_error = kill_extension().err();
            let _ = child.wait();
            return Err(extension_error.unwrap_or_else(|| {
                io::Error::other("bounded supervisor did not receive child stderr")
            }));
        }
    };
    let (sender, receiver) = mpsc::sync_channel(DRAIN_QUEUE_DEPTH);
    let stdout_thread = spawn_drain(stdout, Stream::Stdout, sender.clone());
    let stderr_thread = spawn_drain(stderr, Stream::Stderr, sender);

    let mut stdout_body = Vec::new();
    let mut stderr_body = Vec::new();
    let mut stdout_done = false;
    let mut stderr_done = false;
    let mut drain_error = None;
    let mut kill_extension_error = None;
    let mut termination = BoundedProcessTermination::Exited;
    let mut status = None;
    let mut teardown_deadline = None;

    while status.is_none() || !stdout_done || !stderr_done {
        if termination == BoundedProcessTermination::Exited && started.elapsed() > limits.timeout {
            termination = BoundedProcessTermination::TimedOut;
            tree.kill(&mut child);
            teardown_deadline = Some(capped_deadline(
                Instant::now(),
                TEARDOWN_GRACE,
                hard_deadline,
            ));
            if let Err(error) = kill_extension() {
                kill_extension_error.get_or_insert(error);
            }
        }
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(DrainEvent::Chunk(stream, chunk)) => {
                let stdout_len = stdout_body.len();
                let stderr_len = stderr_body.len();
                let target = match stream {
                    Stream::Stdout => &mut stdout_body,
                    Stream::Stderr => &mut stderr_body,
                };
                let stream_limit = match stream {
                    Stream::Stdout => limits.stdout_bytes,
                    Stream::Stderr => limits.stderr_bytes,
                };
                let next_stream = u64::try_from(target.len())
                    .ok()
                    .and_then(|size| size.checked_add(u64::try_from(chunk.len()).ok()?));
                let next_total = u64::try_from(stdout_len)
                    .ok()
                    .and_then(|size| size.checked_add(u64::try_from(stderr_len).ok()?))
                    .and_then(|size| size.checked_add(u64::try_from(chunk.len()).ok()?));
                if termination == BoundedProcessTermination::Exited
                    && next_stream.is_none_or(|size| size > stream_limit)
                {
                    termination = match stream {
                        Stream::Stdout => BoundedProcessTermination::StdoutLimitExceeded,
                        Stream::Stderr => BoundedProcessTermination::StderrLimitExceeded,
                    };
                    tree.kill(&mut child);
                    teardown_deadline.get_or_insert(capped_deadline(
                        Instant::now(),
                        TEARDOWN_GRACE,
                        hard_deadline,
                    ));
                    if let Err(error) = kill_extension() {
                        kill_extension_error.get_or_insert(error);
                    }
                } else if termination == BoundedProcessTermination::Exited
                    && next_total.is_none_or(|size| size > limits.total_bytes)
                {
                    termination = BoundedProcessTermination::TotalLimitExceeded;
                    tree.kill(&mut child);
                    teardown_deadline.get_or_insert(capped_deadline(
                        Instant::now(),
                        TEARDOWN_GRACE,
                        hard_deadline,
                    ));
                    if let Err(error) = kill_extension() {
                        kill_extension_error.get_or_insert(error);
                    }
                } else if termination == BoundedProcessTermination::Exited {
                    target.extend_from_slice(&chunk);
                }
            }
            Ok(DrainEvent::Done(stream, result)) => {
                if let Err(error) = result {
                    tree.kill(&mut child);
                    teardown_deadline.get_or_insert(capped_deadline(
                        Instant::now(),
                        TEARDOWN_GRACE,
                        hard_deadline,
                    ));
                    if let Err(kill_error) = kill_extension() {
                        kill_extension_error.get_or_insert(kill_error);
                    }
                    drain_error.get_or_insert(error);
                }
                match stream {
                    Stream::Stdout => stdout_done = true,
                    Stream::Stderr => stderr_done = true,
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) if stdout_done && stderr_done => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tree.kill(&mut child);
                let extension_error = kill_extension().err();
                let _ = child.wait();
                return Err(extension_error.unwrap_or_else(|| {
                    io::Error::other("bounded supervisor drain workers disconnected")
                }));
            }
        }
        if status.is_none() {
            status = match tree.try_wait(&mut child) {
                Ok(status) => status,
                Err(error) => {
                    tree.kill(&mut child);
                    let extension_error = kill_extension().err();
                    let _ = bounded_reap_after_wait_error(
                        &mut child,
                        remaining_before_or_zero(capped_deadline(
                            Instant::now(),
                            TEARDOWN_GRACE,
                            hard_deadline,
                        )),
                    );
                    return Err(extension_error.unwrap_or(error));
                }
            };
        }
        if hard_deadline.is_some_and(|deadline| {
            Instant::now() >= deadline && (status.is_none() || !stdout_done || !stderr_done)
        }) || teardown_deadline.is_some_and(|deadline| {
            Instant::now() >= deadline && (status.is_none() || !stdout_done || !stderr_done)
        }) {
            tree.kill(&mut child);
            if let Err(error) = kill_extension() {
                kill_extension_error.get_or_insert(error);
            }
            // The teardown deadline is the total closure deadline, not the start of a second
            // grace window. At expiry perform one final non-blocking reap observation; callers
            // that require opaque closure abort if this function returns an error.
            let _ = bounded_reap_after_wait_error(&mut child, Duration::ZERO);
            return Err(kill_extension_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "bounded supervisor teardown did not close the process domain and pipes",
                )
            }));
        }
    }
    let observed_status =
        status.ok_or_else(|| io::Error::other("bounded supervisor lost child status"))?;
    stdout_thread
        .join()
        .map_err(|_| io::Error::other("bounded stdout drain worker panicked"))?;
    stderr_thread
        .join()
        .map_err(|_| io::Error::other("bounded stderr drain worker panicked"))?;
    let group_closure_timeout = hard_deadline
        .map(remaining_before_or_zero)
        .unwrap_or(TEARDOWN_GRACE);
    if group_closure_timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "bounded supervisor hard closure deadline elapsed before process-group closure",
        ));
    }
    let group_closure = tree.close_remaining_process_group(group_closure_timeout);
    // Even a fail-closed descendant observation must reap the pinned direct-child leader before
    // this non-authoritative helper returns. Trusted callers abort on the preserved closure error.
    let final_wait = tree.finalize_wait(&mut child, observed_status);
    if let Err(error) = group_closure {
        let _ = final_wait;
        return Err(error);
    }
    let (status, maximum_rss_bytes) = final_wait?;
    if let Some(error) = drain_error {
        return Err(error);
    }
    if let Some(error) = kill_extension_error {
        return Err(error);
    }
    Ok(BoundedProcessOutput {
        status,
        stdout: stdout_body,
        stderr: stderr_body,
        termination,
        elapsed: started.elapsed(),
        pid,
        process_group: tree.process_group(),
        maximum_rss_bytes,
    })
}

fn capped_deadline(started: Instant, grace: Duration, hard_deadline: Option<Instant>) -> Instant {
    let grace_deadline = started.checked_add(grace).unwrap_or(started);
    hard_deadline.map_or(grace_deadline, |hard| grace_deadline.min(hard))
}

fn remaining_before_or_zero(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn bounded_reap_after_wait_error(child: &mut Child, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "reap deadline overflow"))?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "bounded child was not reaped before deadline",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn validate_limits(limits: BoundedProcessLimits) -> io::Result<()> {
    if limits.stdout_bytes == 0
        || limits.stderr_bytes == 0
        || limits.total_bytes == 0
        || limits.timeout.is_zero()
        || limits.stdout_bytes > limits.total_bytes
        || limits.stderr_bytes > limits.total_bytes
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bounded supervisor limits are invalid",
        ));
    }
    Ok(())
}

fn spawn_drain<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    sender: SyncSender<DrainEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let result = (|| -> io::Result<()> {
            let mut buffer = vec![0_u8; DRAIN_CHUNK_BYTES];
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    return Ok(());
                }
                sender
                    .send(DrainEvent::Chunk(stream, buffer[..read].to_vec()))
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::BrokenPipe, "bounded supervisor stopped")
                    })?;
            }
        })();
        let _ = sender.send(DrainEvent::Done(stream, result));
    })
}

#[cfg(unix)]
mod platform {
    #[cfg(target_os = "linux")]
    use std::time::Instant;
    use std::{
        io,
        os::unix::process::CommandExt,
        process::{Child, Command, ExitStatus},
        time::Duration,
    };

    pub(super) fn prepare(command: &mut Command) {
        command.process_group(0);
    }

    pub(super) struct ProcessTree {
        process_group: i32,
        #[cfg(target_os = "linux")]
        exit_observed_without_reap: bool,
    }

    impl ProcessTree {
        pub(super) fn attach(child: &Child) -> io::Result<Self> {
            let process_group = i32::try_from(child.id())
                .map_err(|_| io::Error::other("bounded child pid exceeds i32"))?;
            Ok(Self {
                process_group,
                #[cfg(target_os = "linux")]
                exit_observed_without_reap: false,
            })
        }

        pub(super) fn kill(&self, child: &mut Child) {
            // SAFETY: a negative pid targets only the process group created for this child.
            unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
            let _ = child.kill();
        }

        pub(super) fn process_group(&self) -> i64 {
            i64::from(self.process_group)
        }

        #[cfg(target_os = "linux")]
        pub(super) fn close_remaining_process_group(&self, timeout: Duration) -> io::Result<()> {
            if !self.exit_observed_without_reap {
                return Err(io::Error::other(
                    "bounded Linux child exit was not retained before group closure",
                ));
            }
            // Keep the direct child as an unreaped zombie so its PID/PGID cannot be reused while
            // the exact group is stopped, enumerated and (if necessary) killed.
            if unsafe { libc::kill(-self.process_group, libc::SIGSTOP) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error);
                }
            }
            let leader = u32::try_from(self.process_group)
                .map_err(|_| io::Error::other("bounded process-group leader is invalid"))?;
            if !linux_group_has_descendants(leader, self.process_group)? {
                return Ok(());
            }
            // SAFETY: the unreaped leader pins this exact process-group identity.
            unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
            let deadline = Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| io::Error::other("process-group closure deadline overflow"))?;
            while linux_group_has_descendants(leader, self.process_group)? {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "bounded command descendants did not leave the pinned process group",
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(io::Error::other(
                "bounded command left a descendant after direct-child closure",
            ))
        }

        #[cfg(not(target_os = "linux"))]
        pub(super) fn close_remaining_process_group(&self, _timeout: Duration) -> io::Result<()> {
            Ok(())
        }

        #[cfg(target_os = "linux")]
        pub(super) fn try_wait(
            &mut self,
            child: &mut Child,
        ) -> io::Result<Option<(ExitStatus, u64)>> {
            use std::os::unix::process::ExitStatusExt;

            if self.exit_observed_without_reap {
                return Err(io::Error::other(
                    "bounded Linux exit observation was requested twice",
                ));
            }
            let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
            if unsafe { info.si_pid() } == 0 {
                return Ok(None);
            }
            let signal_status = unsafe { info.si_status() };
            let raw_status = match info.si_code {
                libc::CLD_EXITED => signal_status << 8,
                libc::CLD_KILLED => signal_status,
                libc::CLD_DUMPED => signal_status | 0x80,
                _ => {
                    return Err(io::Error::other(
                        "bounded Linux waitid returned a non-terminal child state",
                    ))
                }
            };
            self.exit_observed_without_reap = true;
            Ok(Some((ExitStatusExt::from_raw(raw_status), 0)))
        }

        #[cfg(not(target_os = "linux"))]
        pub(super) fn try_wait(
            &mut self,
            child: &mut Child,
        ) -> io::Result<Option<(ExitStatus, u64)>> {
            use std::os::unix::process::ExitStatusExt;

            let waited_pid = i32::try_from(child.id())
                .map_err(|_| io::Error::other("bounded child pid exceeds i32"))?;
            let (waited, status, usage) = loop {
                let mut status = 0;
                let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
                // SAFETY: wait4 writes into live storage for this direct child only.
                let waited =
                    unsafe { libc::wait4(waited_pid, &mut status, libc::WNOHANG, &mut usage) };
                if waited >= 0 {
                    break (waited, status, usage);
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            };
            if waited == 0 {
                return Ok(None);
            }
            #[cfg(target_os = "macos")]
            let maximum_rss_bytes = u64::try_from(usage.ru_maxrss).unwrap_or(u64::MAX);
            #[cfg(not(target_os = "macos"))]
            let maximum_rss_bytes = u64::try_from(usage.ru_maxrss)
                .unwrap_or(u64::MAX)
                .saturating_mul(1024);
            Ok(Some((ExitStatusExt::from_raw(status), maximum_rss_bytes)))
        }

        #[cfg(target_os = "linux")]
        pub(super) fn finalize_wait(
            &mut self,
            child: &mut Child,
            observed: (ExitStatus, u64),
        ) -> io::Result<(ExitStatus, u64)> {
            use std::os::unix::process::ExitStatusExt;

            if !self.exit_observed_without_reap {
                return Err(io::Error::other(
                    "bounded Linux child was not retained for final wait4",
                ));
            }
            let waited_pid = i32::try_from(child.id())
                .map_err(|_| io::Error::other("bounded child pid exceeds i32"))?;
            let mut raw_status = 0;
            let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
            let waited = loop {
                let waited = unsafe { libc::wait4(waited_pid, &mut raw_status, 0, &mut usage) };
                if waited >= 0 {
                    break waited;
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            };
            if waited != waited_pid {
                return Err(io::Error::other(
                    "bounded Linux final wait reaped an unexpected child",
                ));
            }
            let status = ExitStatusExt::from_raw(raw_status);
            if status != observed.0 {
                return Err(io::Error::other(
                    "bounded Linux waitid and wait4 status differ",
                ));
            }
            self.exit_observed_without_reap = false;
            Ok((
                status,
                u64::try_from(usage.ru_maxrss)
                    .unwrap_or(u64::MAX)
                    .saturating_mul(1024),
            ))
        }

        #[cfg(not(target_os = "linux"))]
        pub(super) fn finalize_wait(
            &mut self,
            _child: &mut Child,
            observed: (ExitStatus, u64),
        ) -> io::Result<(ExitStatus, u64)> {
            Ok(observed)
        }
    }

    #[cfg(target_os = "linux")]
    fn linux_group_has_descendants(leader: u32, process_group: i32) -> io::Result<bool> {
        for entry in std::fs::read_dir("/proc")? {
            let entry = entry?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == leader {
                continue;
            }
            let stat = match std::fs::read_to_string(entry.path().join("stat")) {
                Ok(stat) => stat,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let fields = stat
                .rsplit_once(") ")
                .map(|(_, fields)| fields)
                .ok_or_else(|| io::Error::other("Linux proc stat is malformed"))?
                .split_whitespace()
                .collect::<Vec<_>>();
            let observed_group = fields
                .get(2)
                .ok_or_else(|| io::Error::other("Linux proc stat lacks process group"))?
                .parse::<i32>()
                .map_err(|_| io::Error::other("Linux proc process group is invalid"))?;
            if observed_group == process_group {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn p8_bounded_process_accepts_exact_pipe_limit_and_kills_n_plus_one() {
        let limits = BoundedProcessLimits {
            stdout_bytes: 4,
            stderr_bytes: 4,
            total_bytes: 8,
            timeout: Duration::from_secs(2),
        };
        let exact =
            run_bounded_command(Command::new("/bin/sh").args(["-c", "printf 1234"]), limits)
                .expect("exact bounded output");
        assert!(exact.succeeded());
        assert_eq!(exact.stdout, b"1234");

        let n_plus_one =
            run_bounded_command(Command::new("/bin/sh").args(["-c", "printf 12345"]), limits)
                .expect("bounded N+1 result");
        assert_eq!(
            n_plus_one.termination,
            BoundedProcessTermination::StdoutLimitExceeded
        );
        assert!(n_plus_one.stdout.len() <= 4);
    }

    #[test]
    fn p8_bounded_process_timeout_terminates_descendant_pipe_holder() {
        let output = run_bounded_command(
            Command::new("/bin/sh").args(["-c", "sleep 5 & wait"]),
            BoundedProcessLimits {
                stdout_bytes: 1024,
                stderr_bytes: 1024,
                total_bytes: 2048,
                timeout: Duration::from_millis(50),
            },
        )
        .expect("bounded timeout result");
        assert_eq!(output.termination, BoundedProcessTermination::TimedOut);
        assert!(output.elapsed < Duration::from_secs(2));
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        io,
        mem::size_of,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        os::windows::process::CommandExt,
        process::{Child, Command},
        ptr::null,
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::HANDLE,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::CREATE_SUSPENDED,
        },
    };

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process_handle: HANDLE) -> i32;
    }

    pub(super) fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_SUSPENDED);
    }

    pub(super) struct ProcessTree(OwnedHandle);

    impl ProcessTree {
        pub(super) fn attach(child: &Child) -> io::Result<Self> {
            // SAFETY: null security/name arguments request a private unnamed Job object.
            let raw = unsafe { CreateJobObjectW(null(), null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: CreateJobObjectW returned a new owned handle.
            let job = unsafe { OwnedHandle::from_raw_handle(raw) };
            let mut info = unsafe { std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: the handle and fixed-layout information buffer are live.
            if unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if unsafe {
                AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle() as HANDLE)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: the child was spawned suspended and is now contained by this private Job.
            if unsafe { NtResumeProcess(child.as_raw_handle() as HANDLE) } < 0 {
                // SAFETY: the private Job contains only the still-suspended child.
                unsafe { TerminateJobObject(job.as_raw_handle(), 1) };
                return Err(io::Error::other(
                    "bounded supervisor failed to resume Job-contained child",
                ));
            }
            Ok(Self(job))
        }

        pub(super) fn kill(&self, child: &mut Child) {
            // SAFETY: this private Job contains only the supervised process tree.
            unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) };
            let _ = child.kill();
        }

        pub(super) fn process_group(&self) -> i64 {
            0
        }

        pub(super) fn close_remaining_process_group(&self, _timeout: Duration) -> io::Result<()> {
            Ok(())
        }

        pub(super) fn try_wait(
            &self,
            child: &mut Child,
        ) -> io::Result<Option<(std::process::ExitStatus, u64)>> {
            Ok(child.try_wait()?.map(|status| (status, 0)))
        }

        pub(super) fn finalize_wait(
            &mut self,
            _child: &mut Child,
            observed: (std::process::ExitStatus, u64),
        ) -> io::Result<(std::process::ExitStatus, u64)> {
            Ok(observed)
        }
    }
}
