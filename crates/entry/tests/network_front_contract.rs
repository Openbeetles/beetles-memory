use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bm_entry::{
    EntryAcceptedTcpStream, EntryTcpDispatchOutcome, EntryTcpNetworkFront,
    EntryTcpNetworkFrontConfig,
};

fn accepted_pair() -> (EntryAcceptedTcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
    let client = TcpStream::connect(listener.local_addr().expect("listener address"))
        .expect("connect test listener");
    let accepted = EntryAcceptedTcpStream::accept(&listener).expect("accept test connection");
    (accepted, client)
}

fn front_config(worker_count: usize, max_in_flight: usize) -> EntryTcpNetworkFrontConfig {
    EntryTcpNetworkFrontConfig::new(
        worker_count,
        max_in_flight,
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .expect("valid network front config")
}

#[test]
fn network_front_rejects_invalid_worker_and_in_flight_bounds() {
    let error =
        EntryTcpNetworkFrontConfig::new(0, 1, Duration::from_secs(1), Duration::from_secs(1))
            .expect_err("zero workers must fail");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

    let error =
        EntryTcpNetworkFrontConfig::new(2, 1, Duration::from_secs(1), Duration::from_secs(1))
            .expect_err("max in-flight below worker count must fail");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

    let error = EntryTcpNetworkFrontConfig::new(1, 1, Duration::ZERO, Duration::from_secs(1))
        .expect_err("zero connection read deadline must fail");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn network_front_enforces_max_in_flight_and_reports_saturation() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let handled = Arc::new(AtomicUsize::new(0));
    let worker_gate = Arc::clone(&gate);
    let worker_handled = Arc::clone(&handled);
    let mut front = EntryTcpNetworkFront::new(front_config(1, 2), move |_stream| {
        worker_handled.fetch_add(1, Ordering::AcqRel);
        let (lock, ready) = &*worker_gate;
        let mut released = lock.lock().expect("gate lock");
        while !*released {
            released = ready.wait(released).expect("gate wait");
        }
    })
    .expect("bounded network front");

    let (first, _first_client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(first).expect("dispatch first"),
        EntryTcpDispatchOutcome::Accepted
    );
    while handled.load(Ordering::Acquire) == 0 {
        thread::yield_now();
    }

    let (second, _second_client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(second).expect("dispatch second"),
        EntryTcpDispatchOutcome::Accepted
    );
    let (third, _third_client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(third).expect("dispatch third"),
        EntryTcpDispatchOutcome::RejectedSaturated
    );
    assert_eq!(front.in_flight(), 2);

    let (lock, ready) = &*gate;
    *lock.lock().expect("gate lock") = true;
    ready.notify_all();
    front.shutdown().expect("shutdown joins workers");
    assert_eq!(handled.load(Ordering::Acquire), 2);
    assert_eq!(front.in_flight(), 0);
}

#[test]
fn connection_read_deadline_includes_queue_wait() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let first_started = Arc::new((Mutex::new(false), Condvar::new()));
    let read_results = Arc::new(Mutex::new(Vec::new()));
    let call_index = Arc::new(AtomicUsize::new(0));

    let worker_gate = Arc::clone(&gate);
    let worker_started = Arc::clone(&first_started);
    let worker_results = Arc::clone(&read_results);
    let worker_index = Arc::clone(&call_index);
    let config =
        EntryTcpNetworkFrontConfig::new(1, 2, Duration::from_millis(80), Duration::from_secs(1))
            .expect("deadline config");
    let mut front = EntryTcpNetworkFront::new(config, move |mut stream| {
        let index = worker_index.fetch_add(1, Ordering::AcqRel);
        if index == 0 {
            let (started_lock, started_ready) = &*worker_started;
            *started_lock.lock().expect("started lock") = true;
            started_ready.notify_all();

            let (lock, ready) = &*worker_gate;
            let mut released = lock.lock().expect("gate lock");
            while !*released {
                released = ready.wait(released).expect("gate wait");
            }
            return;
        }

        let mut byte = [0_u8; 1];
        worker_results
            .lock()
            .expect("results lock")
            .push(stream.read(&mut byte));
    })
    .expect("deadline network front");

    let (first, _first_client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(first).expect("dispatch first"),
        EntryTcpDispatchOutcome::Accepted
    );
    let (started_lock, started_ready) = &*first_started;
    let mut started = started_lock.lock().expect("started lock");
    while !*started {
        started = started_ready.wait(started).expect("started wait");
    }
    drop(started);

    let (second, _second_client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(second).expect("dispatch queued"),
        EntryTcpDispatchOutcome::Accepted
    );
    thread::sleep(Duration::from_millis(160));

    let before_release = Instant::now();
    let (lock, ready) = &*gate;
    *lock.lock().expect("gate lock") = true;
    ready.notify_all();
    front.shutdown().expect("shutdown deadline front");

    assert!(
        before_release.elapsed() < Duration::from_millis(400),
        "expired queued connection should not receive a fresh deadline"
    );
    let results = read_results.lock().expect("results lock");
    assert_eq!(results.len(), 1);
    assert!(
        matches!(&results[0], Ok(0) | Err(_)),
        "expired read must be closed: {:?}",
        results[0]
    );
}

#[test]
fn connection_read_deadline_is_absolute_across_progressive_body_reads() {
    let completed = Arc::new((Mutex::new(None), Condvar::new()));
    let worker_completed = Arc::clone(&completed);
    let config =
        EntryTcpNetworkFrontConfig::new(1, 1, Duration::from_millis(120), Duration::from_secs(1))
            .expect("deadline config");
    let mut front = EntryTcpNetworkFront::new(config, move |mut stream| {
        let mut total = 0;
        let mut byte = [0_u8; 1];
        while let Ok(1) = stream.read(&mut byte) {
            total += 1;
        }
        let (lock, ready) = &*worker_completed;
        *lock.lock().expect("completion lock") = Some(total);
        ready.notify_all();
    })
    .expect("deadline network front");

    let (stream, mut client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(stream).expect("dispatch body reader"),
        EntryTcpDispatchOutcome::Accepted
    );
    let started = Instant::now();
    let mut sent = 0;
    for _ in 0..8 {
        if client.write_all(b"x").is_err() {
            break;
        }
        sent += 1;
        thread::sleep(Duration::from_millis(35));
    }

    let (lock, ready) = &*completed;
    let mut total = lock.lock().expect("completion lock");
    while total.is_none() {
        let (next, timeout) = ready
            .wait_timeout(total, Duration::from_secs(1))
            .expect("completion wait");
        assert!(
            !timeout.timed_out(),
            "body reader did not reach its deadline"
        );
        total = next;
    }
    let read = total.expect("completed byte count");
    drop(total);

    front.shutdown().expect("shutdown body deadline front");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(read < sent, "absolute deadline must stop progressive reads");
}

#[test]
fn shutdown_interrupts_connection_reads_and_joins_workers() {
    let started = Arc::new((Mutex::new(false), Condvar::new()));
    let worker_started = Arc::clone(&started);
    let mut front = EntryTcpNetworkFront::new(front_config(1, 1), move |mut stream| {
        let (lock, ready) = &*worker_started;
        *lock.lock().expect("started lock") = true;
        ready.notify_all();

        let mut byte = [0_u8; 1];
        let _ = stream.read(&mut byte);
    })
    .expect("network front");
    let (stream, _client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(stream).expect("dispatch blocking read"),
        EntryTcpDispatchOutcome::Accepted
    );

    let (lock, ready) = &*started;
    let mut did_start = lock.lock().expect("started lock");
    while !*did_start {
        did_start = ready.wait(did_start).expect("started wait");
    }
    drop(did_start);

    let shutdown_started = Instant::now();
    front.shutdown().expect("shutdown interrupts reads");
    assert!(shutdown_started.elapsed() < Duration::from_millis(400));

    let (late, _late_client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(late).expect("dispatch after shutdown"),
        EntryTcpDispatchOutcome::RejectedShuttingDown
    );
}

#[test]
fn handler_panic_releases_capacity_and_worker_continues() {
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = Arc::clone(&calls);
    let mut front = EntryTcpNetworkFront::new(front_config(1, 1), move |_stream| {
        if worker_calls.fetch_add(1, Ordering::AcqRel) == 0 {
            panic!("intentional handler panic");
        }
    })
    .expect("network front");

    let (first, _first_client) = accepted_pair();
    assert_eq!(
        front
            .try_dispatch(first)
            .expect("dispatch panicking handler"),
        EntryTcpDispatchOutcome::Accepted
    );
    while front.in_flight() != 0 {
        thread::yield_now();
    }

    let (second, _second_client) = accepted_pair();
    assert_eq!(
        front.try_dispatch(second).expect("dispatch after panic"),
        EntryTcpDispatchOutcome::Accepted
    );
    front.shutdown().expect("shutdown recovered worker");
    assert_eq!(calls.load(Ordering::Acquire), 2);
}
