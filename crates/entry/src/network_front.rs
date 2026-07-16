use std::collections::HashMap;
use std::io;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::EntryAcceptedTcpStream;

/// Resource and I/O bounds for [`EntryTcpNetworkFront`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryTcpNetworkFrontConfig {
    worker_count: usize,
    max_in_flight: usize,
    connection_read_deadline: Duration,
    write_timeout: Duration,
}

impl EntryTcpNetworkFrontConfig {
    /// Creates bounds for accepted TCP connections.
    ///
    /// `max_in_flight` counts queued and executing connections. The absolute
    /// `connection_read_deadline` starts when dispatch accepts a connection,
    /// includes queue wait, and remains armed until its handler returns. On
    /// expiry the connection's read half is shut down, so the bound covers all
    /// request/header/body reads rather than only the first read or headers.
    /// `write_timeout` is the socket timeout for each blocking write operation.
    pub fn new(
        worker_count: usize,
        max_in_flight: usize,
        connection_read_deadline: Duration,
        write_timeout: Duration,
    ) -> io::Result<Self> {
        if worker_count == 0 {
            return Err(invalid_input("worker_count must be greater than zero"));
        }
        if max_in_flight < worker_count {
            return Err(invalid_input(
                "max_in_flight must be greater than or equal to worker_count",
            ));
        }
        if connection_read_deadline.is_zero() {
            return Err(invalid_input(
                "connection_read_deadline must be greater than zero",
            ));
        }
        if write_timeout.is_zero() {
            return Err(invalid_input("write_timeout must be greater than zero"));
        }
        if Instant::now()
            .checked_add(connection_read_deadline)
            .is_none()
        {
            return Err(invalid_input(
                "connection_read_deadline is too large for this platform",
            ));
        }
        Ok(Self {
            worker_count,
            max_in_flight,
            connection_read_deadline,
            write_timeout,
        })
    }

    pub const fn worker_count(self) -> usize {
        self.worker_count
    }

    pub const fn max_in_flight(self) -> usize {
        self.max_in_flight
    }

    pub const fn connection_read_deadline(self) -> Duration {
        self.connection_read_deadline
    }

    pub const fn write_timeout(self) -> Duration {
        self.write_timeout
    }
}

/// Result of attempting to admit an accepted connection to the network front.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryTcpDispatchOutcome {
    Accepted,
    RejectedSaturated,
    RejectedShuttingDown,
}

struct InFlightPermit {
    in_flight: Arc<AtomicUsize>,
}

impl Drop for InFlightPermit {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

struct ConnectionJob {
    stream: EntryAcceptedTcpStream,
    _deadline: ConnectionReadDeadlineLease,
    _permit: InFlightPermit,
}

/// Fixed-worker, bounded-queue TCP admission front shared by process entries.
///
/// Dropping or explicitly shutting down the front rejects new work, interrupts
/// reads for every accepted connection, drains accepted jobs through the fixed
/// workers, and joins the deadline supervisor and workers.
pub struct EntryTcpNetworkFront {
    sender: Option<mpsc::SyncSender<ConnectionJob>>,
    workers: Vec<JoinHandle<()>>,
    in_flight: Arc<AtomicUsize>,
    max_in_flight: usize,
    next_connection_id: AtomicU64,
    deadlines: Arc<ConnectionReadDeadlines>,
    deadline_supervisor: Option<JoinHandle<()>>,
    connection_read_deadline: Duration,
    write_timeout: Duration,
}

impl EntryTcpNetworkFront {
    pub fn new<F>(config: EntryTcpNetworkFrontConfig, handler: F) -> io::Result<Self>
    where
        F: Fn(EntryAcceptedTcpStream) + Send + Sync + 'static,
    {
        let deadlines = Arc::new(ConnectionReadDeadlines::default());
        let deadline_state = Arc::clone(&deadlines);
        let deadline_supervisor = thread::Builder::new()
            .name("bm-entry-connection-read-deadlines".to_string())
            .spawn(move || deadline_state.supervise())?;

        let (sender, receiver) = mpsc::sync_channel(config.max_in_flight);
        let receiver = Arc::new(Mutex::new(receiver));
        let handler = Arc::new(handler);
        let mut front = Self {
            sender: Some(sender),
            workers: Vec::with_capacity(config.worker_count),
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_in_flight: config.max_in_flight,
            next_connection_id: AtomicU64::new(0),
            deadlines,
            deadline_supervisor: Some(deadline_supervisor),
            connection_read_deadline: config.connection_read_deadline,
            write_timeout: config.write_timeout,
        };

        for index in 0..config.worker_count {
            let receiver = Arc::clone(&receiver);
            let handler = Arc::clone(&handler);
            match thread::Builder::new()
                .name(format!("bm-entry-tcp-worker-{index}"))
                .spawn(move || worker_loop(receiver, handler))
            {
                Ok(worker) => front.workers.push(worker),
                Err(error) => {
                    let _ = front.shutdown();
                    return Err(error);
                }
            }
        }
        Ok(front)
    }

    /// Attempts immediate admission without waiting for queue capacity.
    ///
    /// Saturated and shutting-down connections are dropped before this method
    /// returns. I/O errors mean the accepted socket could not be configured or
    /// registered with the absolute read-deadline supervisor.
    pub fn try_dispatch(
        &self,
        stream: EntryAcceptedTcpStream,
    ) -> io::Result<EntryTcpDispatchOutcome> {
        let Some(sender) = self.sender.as_ref() else {
            return Ok(EntryTcpDispatchOutcome::RejectedShuttingDown);
        };
        let Some(permit) = self.try_acquire_permit() else {
            return Ok(EntryTcpDispatchOutcome::RejectedSaturated);
        };

        stream.set_read_timeout(Some(self.connection_read_deadline))?;
        stream.set_write_timeout(Some(self.write_timeout))?;
        let deadline = Instant::now()
            .checked_add(self.connection_read_deadline)
            .ok_or_else(|| invalid_input("connection read deadline overflowed"))?;
        let connection_id = self.next_connection_id.fetch_add(1, Ordering::Relaxed);
        let deadline_stream = stream.try_clone_transport()?;
        let deadline = self
            .deadlines
            .register(connection_id, deadline, deadline_stream)?;
        let job = ConnectionJob {
            stream,
            _deadline: deadline,
            _permit: permit,
        };

        match sender.try_send(job) {
            Ok(()) => Ok(EntryTcpDispatchOutcome::Accepted),
            Err(mpsc::TrySendError::Full(_job)) => Ok(EntryTcpDispatchOutcome::RejectedSaturated),
            Err(mpsc::TrySendError::Disconnected(_job)) => {
                Ok(EntryTcpDispatchOutcome::RejectedShuttingDown)
            }
        }
    }

    /// Returns the number of queued plus currently executing connections.
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Interrupts accepted connection reads and joins every owned thread.
    ///
    /// The method is idempotent. A handler that ignores connection I/O and
    /// blocks forever cannot be forcibly terminated by Rust threads.
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.sender.take();
        self.deadlines.stop_and_interrupt_all_reads();

        let mut worker_panicked = false;
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                worker_panicked = true;
            }
        }
        if let Some(supervisor) = self.deadline_supervisor.take() {
            if supervisor.join().is_err() {
                worker_panicked = true;
            }
        }

        if worker_panicked {
            Err(io::Error::other(
                "network front thread panicked during shutdown",
            ))
        } else {
            Ok(())
        }
    }

    fn try_acquire_permit(&self) -> Option<InFlightPermit> {
        let mut current = self.in_flight.load(Ordering::Acquire);
        loop {
            if current >= self.max_in_flight {
                return None;
            }
            match self.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(InFlightPermit {
                        in_flight: Arc::clone(&self.in_flight),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for EntryTcpNetworkFront {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn worker_loop<F>(receiver: Arc<Mutex<mpsc::Receiver<ConnectionJob>>>, handler: Arc<F>)
where
    F: Fn(EntryAcceptedTcpStream) + Send + Sync + 'static,
{
    loop {
        let job = {
            let receiver = match receiver.lock() {
                Ok(receiver) => receiver,
                Err(poisoned) => poisoned.into_inner(),
            };
            receiver.recv()
        };
        let Ok(job) = job else { break };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(job.stream);
        }));
    }
}

#[derive(Default)]
struct ConnectionReadDeadlines {
    state: Mutex<ConnectionReadDeadlineState>,
    changed: Condvar,
}

#[derive(Default)]
struct ConnectionReadDeadlineState {
    active: HashMap<u64, ActiveConnectionReadDeadline>,
    stopping: bool,
}

struct ActiveConnectionReadDeadline {
    expires_at: Instant,
    interrupt: Box<dyn ConnectionReadInterrupt>,
}

trait ConnectionReadInterrupt: Send {
    fn shutdown_read(&self) -> io::Result<()>;
}

impl ConnectionReadInterrupt for TcpStream {
    fn shutdown_read(&self) -> io::Result<()> {
        self.shutdown(Shutdown::Read)
    }
}

impl ConnectionReadDeadlines {
    fn register<I>(
        self: &Arc<Self>,
        connection_id: u64,
        expires_at: Instant,
        interrupt: I,
    ) -> io::Result<ConnectionReadDeadlineLease>
    where
        I: ConnectionReadInterrupt + 'static,
    {
        let mut state = lock_unpoisoned(&self.state);
        if state.stopping {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "network front is shutting down",
            ));
        }
        if state.active.contains_key(&connection_id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "connection deadline identifier was reused",
            ));
        }
        state.active.insert(
            connection_id,
            ActiveConnectionReadDeadline {
                expires_at,
                interrupt: Box::new(interrupt),
            },
        );
        drop(state);
        self.changed.notify_one();
        Ok(ConnectionReadDeadlineLease {
            connection_id,
            deadlines: Arc::clone(self),
        })
    }

    fn complete(&self, connection_id: u64) {
        lock_unpoisoned(&self.state).active.remove(&connection_id);
        self.changed.notify_one();
    }

    fn stop_and_interrupt_all_reads(&self) {
        let mut state = lock_unpoisoned(&self.state);
        state.stopping = true;
        for active in state.active.values() {
            let _ = active.interrupt.shutdown_read();
        }
        state.active.clear();
        drop(state);
        self.changed.notify_all();
    }

    fn supervise(&self) {
        let mut state = lock_unpoisoned(&self.state);
        loop {
            if state.stopping {
                return;
            }

            let now = Instant::now();
            state.active.retain(|_, active| {
                if active.expires_at <= now {
                    let _ = active.interrupt.shutdown_read();
                    false
                } else {
                    true
                }
            });
            let next_wait = state
                .active
                .values()
                .map(|active| active.expires_at.saturating_duration_since(now))
                .min();
            state = match next_wait {
                Some(wait) => match self.changed.wait_timeout(state, wait) {
                    Ok((state, _)) => state,
                    Err(poisoned) => poisoned.into_inner().0,
                },
                None => match self.changed.wait(state) {
                    Ok(state) => state,
                    Err(poisoned) => poisoned.into_inner(),
                },
            };
        }
    }
}

struct ConnectionReadDeadlineLease {
    connection_id: u64,
    deadlines: Arc<ConnectionReadDeadlines>,
}

impl Drop for ConnectionReadDeadlineLease {
    fn drop(&mut self) {
        self.deadlines.complete(self.connection_id);
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestReadInterrupt;

    impl ConnectionReadInterrupt for TestReadInterrupt {
        fn shutdown_read(&self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn config_names_match_their_effective_bounds() {
        let config =
            EntryTcpNetworkFrontConfig::new(3, 9, Duration::from_secs(4), Duration::from_secs(5))
                .expect("network front config");

        assert_eq!(config.worker_count(), 3);
        assert_eq!(config.max_in_flight(), 9);
        assert_eq!(config.connection_read_deadline(), Duration::from_secs(4));
        assert_eq!(config.write_timeout(), Duration::from_secs(5));
    }

    #[test]
    fn completed_deadline_lease_removes_registered_connection() {
        let deadlines = Arc::new(ConnectionReadDeadlines::default());
        let lease = deadlines
            .register(
                7,
                Instant::now() + Duration::from_secs(1),
                TestReadInterrupt,
            )
            .expect("register deadline");
        assert_eq!(lock_unpoisoned(&deadlines.state).active.len(), 1);

        drop(lease);

        assert!(lock_unpoisoned(&deadlines.state).active.is_empty());
    }
}
