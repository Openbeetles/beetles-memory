use std::io::Write;
use std::net::{Shutdown, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use bm_entry::{
    EntryAcceptedTcpStream, EntryTcpDispatchOutcome, EntryTcpNetworkFront,
    EntryTcpNetworkFrontConfig,
};

use crate::{GatewayError, GatewayErrorKey, GatewayRequestBudgetContext, GatewayRuntime, Result};

pub trait GatewayHttpConnectionHandler: Send {
    fn handle(
        &mut self,
        context: &GatewayRequestBudgetContext,
        stream: &mut EntryAcceptedTcpStream,
    ) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayHttpFrontConfig {
    pub worker_count: usize,
    pub max_in_flight: usize,
    pub request_timeout: Duration,
    pub idle_timeout: Duration,
    pub force_close_client_connections: bool,
}

impl Default for GatewayHttpFrontConfig {
    fn default() -> Self {
        Self {
            worker_count: 4,
            max_in_flight: 64,
            request_timeout: Duration::from_secs(600),
            idle_timeout: Duration::from_secs(10),
            force_close_client_connections: true,
        }
    }
}

#[derive(Clone)]
pub struct GatewayHttpFront {
    gateway: Arc<GatewayRuntime>,
    config: GatewayHttpFrontConfig,
}

impl GatewayHttpFront {
    pub fn new(gateway: Arc<GatewayRuntime>, config: GatewayHttpFrontConfig) -> Result<Self> {
        EntryTcpNetworkFrontConfig::new(
            config.worker_count,
            config.max_in_flight,
            config.request_timeout,
            config.request_timeout,
        )
        .map_err(|error| GatewayError::invalid_config(format!("http front: {error}")))?;
        if config.idle_timeout.is_zero() {
            return Err(GatewayError::invalid_config(
                "http front idle_timeout must be greater than zero",
            ));
        }
        Ok(Self { gateway, config })
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
        if accept_limit == Some(0) {
            return Ok(());
        }
        let gateway = Arc::clone(&self.gateway);
        let force_close = self.config.force_close_client_connections;
        let idle_timeout = self.config.idle_timeout;
        let mut front = EntryTcpNetworkFront::new(
            EntryTcpNetworkFrontConfig::new(
                self.config.worker_count,
                self.config.max_in_flight,
                self.config.request_timeout,
                self.config.request_timeout,
            )
            .map_err(|error| GatewayError::invalid_config(error.to_string()))?,
            move |mut stream| {
                let _ = stream.set_read_timeout(Some(idle_timeout));
                let context = match gateway.begin_request() {
                    Ok(context) => context,
                    Err(error) if error.key() == GatewayErrorKey::CapacityExceeded => {
                        let _ = reject_over_capacity(&mut stream);
                        return;
                    }
                    Err(error) => {
                        let _ = write_front_error(&mut stream, &error);
                        return;
                    }
                };
                let mut handler = factory();
                if let Err(error) = gateway.execute_with_request_context(&context, || {
                    handler.handle(&context, &mut stream)
                }) {
                    let _ = write_front_error(&mut stream, &error);
                }
                let _ = stream.flush();
                if force_close {
                    let _ = stream.shutdown(Shutdown::Both);
                }
            },
        )
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;

        let mut accepted = 0usize;
        loop {
            let stream = EntryAcceptedTcpStream::accept(&listener)
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            accepted += 1;
            match front
                .try_dispatch(stream)
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?
            {
                EntryTcpDispatchOutcome::Accepted | EntryTcpDispatchOutcome::RejectedSaturated => {}
                EntryTcpDispatchOutcome::RejectedShuttingDown => {
                    return Err(GatewayError::upstream_unavailable(
                        "gateway network front shut down during accept",
                    ));
                }
            }
            if accept_limit.is_some_and(|limit| accepted >= limit) {
                break;
            }
        }
        front
            .shutdown()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
    }
}

fn reject_over_capacity(stream: &mut EntryAcceptedTcpStream) -> Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        )
        .and_then(|_| stream.flush())
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
}

fn write_front_error(stream: &mut EntryAcceptedTcpStream, error: &GatewayError) -> Result<()> {
    let body = serde_json::json!({
        "error": {
            "type": error.key().as_str(),
            "message": error.message(),
        }
    })
    .to_string();
    let response = format!(
        "HTTP/1.1 502 Bad Gateway\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
}
