use std::io::Write;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use crate::{GatewayError, Result};

pub trait GatewayHttpConnectionHandler: Send {
    fn handle(&mut self, stream: &mut TcpStream) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayHttpFrontConfig {
    pub max_connections: usize,
    pub request_timeout: Duration,
    pub idle_timeout: Duration,
    pub force_close_client_connections: bool,
}

impl Default for GatewayHttpFrontConfig {
    fn default() -> Self {
        Self {
            max_connections: 128,
            request_timeout: Duration::from_secs(600),
            idle_timeout: Duration::from_secs(10),
            force_close_client_connections: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GatewayHttpFront {
    config: GatewayHttpFrontConfig,
}

impl GatewayHttpFront {
    pub fn new(config: GatewayHttpFrontConfig) -> Result<Self> {
        if config.max_connections == 0 {
            return Err(GatewayError::invalid_config(
                "http front max_connections must be greater than zero",
            ));
        }
        if config.request_timeout.is_zero() {
            return Err(GatewayError::invalid_config(
                "http front request_timeout must be greater than zero",
            ));
        }
        if config.idle_timeout.is_zero() {
            return Err(GatewayError::invalid_config(
                "http front idle_timeout must be greater than zero",
            ));
        }
        Ok(Self { config })
    }

    pub fn config(&self) -> GatewayHttpFrontConfig {
        self.config
    }

    pub fn serve_listener_with_factory<F>(&self, listener: TcpListener, factory: F) -> Result<()>
    where
        F: Fn() -> Box<dyn GatewayHttpConnectionHandler> + Send + Sync + 'static,
    {
        self.serve_listener_inner(listener, Arc::new(factory), None)
    }

    pub fn serve_listener_n_with_factory<F>(
        &self,
        listener: TcpListener,
        accept_count: usize,
        factory: F,
    ) -> Result<()>
    where
        F: Fn() -> Box<dyn GatewayHttpConnectionHandler> + Send + Sync + 'static,
    {
        self.serve_listener_inner(listener, Arc::new(factory), Some(accept_count))
    }

    fn serve_listener_inner(
        &self,
        listener: TcpListener,
        factory: Arc<dyn Fn() -> Box<dyn GatewayHttpConnectionHandler> + Send + Sync>,
        accept_limit: Option<usize>,
    ) -> Result<()> {
        let active_connections = Arc::new(AtomicUsize::new(0));

        for (accepted_index, stream) in listener.incoming().enumerate() {
            let accepted = accepted_index + 1;
            let stream =
                stream.map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            self.configure_stream(&stream)?;

            if active_connections.load(Ordering::SeqCst) >= self.config.max_connections {
                reject_over_capacity(stream)?;
                if accept_limit.is_some_and(|limit| accepted >= limit) {
                    break;
                }
                continue;
            }

            active_connections.fetch_add(1, Ordering::SeqCst);
            let active = active_connections.clone();
            let handler_factory = factory.clone();
            let request_timeout = self.config.request_timeout;
            let force_close_client_connections = self.config.force_close_client_connections;
            thread::spawn(move || {
                let mut stream = stream;
                let mut handler = handler_factory();
                let watchdog_state = Arc::new((Mutex::new(false), Condvar::new()));
                let watchdog_state_for_thread = watchdog_state.clone();
                let watchdog_stream = stream.try_clone().ok();
                thread::spawn(move || {
                    let (lock, condvar) = &*watchdog_state_for_thread;
                    let finished = lock.lock().expect("http front watchdog lock");
                    let (_finished, wait_result) = condvar
                        .wait_timeout_while(finished, request_timeout, |finished| !*finished)
                        .expect("http front watchdog wait");
                    if wait_result.timed_out() {
                        if let Some(stream) = watchdog_stream {
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                        }
                    }
                });
                if let Err(error) = handler.handle(&mut stream) {
                    let _ = write_front_error(&mut stream, &error);
                }
                let _ = stream.flush();
                if force_close_client_connections {
                    let _ = stream.shutdown(Shutdown::Both);
                }
                let (lock, condvar) = &*watchdog_state;
                *lock.lock().expect("http front watchdog lock") = true;
                condvar.notify_one();
                active.fetch_sub(1, Ordering::SeqCst);
            });

            if accept_limit.is_some_and(|limit| accepted >= limit) {
                break;
            }
        }

        Ok(())
    }

    fn configure_stream(&self, stream: &TcpStream) -> Result<()> {
        stream
            .set_read_timeout(Some(self.config.idle_timeout))
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        stream
            .set_write_timeout(Some(self.config.request_timeout))
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        Ok(())
    }
}

fn reject_over_capacity(mut stream: TcpStream) -> Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        )
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
    stream
        .flush()
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
}

fn write_front_error(stream: &mut TcpStream, error: &GatewayError) -> Result<()> {
    let body = serde_json::json!({
        "error": {
            "type": format!("{:?}", error.key()),
            "message": error.message(),
        }
    })
    .to_string();
    write!(
        stream,
        "HTTP/1.1 502 Bad Gateway\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
}
