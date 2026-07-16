#![cfg(all(feature = "server-async", feature = "client-reqwest"))]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
};
use std::thread;
use std::time::Duration;

use bm_llm_gateway::{
    serve_llm_gateway_http_accepted_stream_with_services_in_request, GatewayConfig,
    GatewayHttpConnectionHandler, GatewayHttpFront, GatewayHttpFrontConfig,
    GatewayHttpRequestBindings, GatewayProviderConfig, GatewayRequestBudgetContext, GatewayRuntime,
    OpenAiCompatibleUpstream, OpenAiGatewayServices, OpenAiUpstreamRequest, OpenAiUpstreamResponse,
    ReqwestOllamaNativeUpstream, Result,
};

fn gateway_config(upstream_addr: SocketAddr) -> GatewayConfig {
    let mut config = GatewayConfig::default_for_local_dev();
    config.providers.clear();
    config.providers.insert(
        "ollama".to_string(),
        GatewayProviderConfig::ollama_native(format!("http://{upstream_addr}/api")),
    );
    config.default_provider = "ollama".to_string();
    config.maintenance.enabled = false;
    config
}

fn start_front(
    upstream_addr: SocketAddr,
    request_timeout: Duration,
    idle_timeout: Duration,
    accept_count: usize,
) -> RunningFront {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind gateway front");
    let addr = listener.local_addr().expect("gateway front addr");
    let config = gateway_config(upstream_addr);
    let gateway = Arc::new(GatewayRuntime::open(config).expect("gateway runtime"));
    let front = GatewayHttpFront::new(
        Arc::clone(&gateway),
        GatewayHttpFrontConfig {
            worker_count: 4,
            max_in_flight: 64,
            request_timeout,
            idle_timeout,
            force_close_client_connections: true,
        },
    )
    .expect("front config");
    let handle = thread::spawn(move || {
        front.serve_listener_n_with_factory(listener, accept_count, move || {
            Box::new(TestGatewayConnection {
                gateway: Arc::clone(&gateway),
            })
        })
    });
    RunningFront { addr, handle }
}

struct RunningFront {
    addr: SocketAddr,
    handle: thread::JoinHandle<Result<()>>,
}

impl RunningFront {
    fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn join(self) {
        self.handle
            .join()
            .expect("gateway front thread")
            .expect("gateway front result");
    }
}

struct TestGatewayConnection {
    gateway: Arc<GatewayRuntime>,
}

impl GatewayHttpConnectionHandler for TestGatewayConnection {
    fn handle(
        &mut self,
        context: &GatewayRequestBudgetContext,
        stream: &mut bm_entry::EntryAcceptedTcpStream,
    ) -> Result<()> {
        let mut openai = MockOpenAiUpstream;
        let mut ollama = ReqwestOllamaNativeUpstream::new()?;
        let mut services = OpenAiGatewayServices::new();
        serve_llm_gateway_http_accepted_stream_with_services_in_request(
            &self.gateway,
            context,
            GatewayHttpRequestBindings::new(&mut openai, &mut ollama, &mut services),
            stream,
        )
    }
}

#[test]
fn app_like_idle_connection_does_not_block_tags() {
    let upstream = MockOllamaServer::start();
    let front = start_front(
        upstream.addr(),
        Duration::from_secs(2),
        Duration::from_secs(2),
        2,
    );
    let front_addr = front.addr();

    let idle = TcpStream::connect(front_addr).expect("open idle app-like connection");
    let tags = get(front_addr, "/api/tags");

    drop(idle);
    front.join();

    assert!(tags.starts_with("HTTP/1.1 200 OK"), "{tags}");
    assert!(tags.contains(r#""models""#), "{tags}");
    assert_eq!(upstream.count("/api/tags"), 1);
}

#[test]
fn concurrent_chat_and_show_both_return() {
    let upstream = MockOllamaServer::start();
    let front = start_front(
        upstream.addr(),
        Duration::from_secs(3),
        Duration::from_secs(3),
        2,
    );
    let front_addr = front.addr();

    let chat = thread::spawn(move || {
        post_json(
            front_addr,
            "/api/chat",
            r#"{"model":"local","stream":false,"messages":[{"role":"user","content":"hello"}]}"#,
            Duration::from_secs(2),
        )
    });
    let show = thread::spawn(move || {
        post_json(
            front_addr,
            "/api/show",
            r#"{"model":"local","verbose":true}"#,
            Duration::from_secs(2),
        )
    });

    let chat = chat.join().expect("chat client thread");
    let show = show.join().expect("show client thread");
    front.join();

    assert!(chat.starts_with("HTTP/1.1 200 OK"), "{chat}");
    assert!(chat.contains(r#""message""#), "{chat}");
    assert!(show.starts_with("HTTP/1.1 200 OK"), "{show}");
    assert!(show.contains(r#""capabilities""#), "{show}");
    assert_eq!(upstream.count("/api/chat"), 1);
    assert_eq!(upstream.count("/api/show"), 1);
}

#[test]
fn streaming_ndjson_is_not_truncated() {
    let upstream = MockOllamaServer::start();
    let front = start_front(
        upstream.addr(),
        Duration::from_secs(3),
        Duration::from_secs(3),
        1,
    );
    let front_addr = front.addr();

    let response = post_json(
        front_addr,
        "/api/chat",
        r#"{"model":"local","stream":true,"messages":[{"role":"user","content":"stream"}]}"#,
        Duration::from_secs(3),
    );
    front.join();

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(
        response.contains("content-type: application/x-ndjson"),
        "{response}"
    );
    assert!(
        response.contains(
            "{\"message\":{\"role\":\"assistant\",\"content\":\"hel\"},\"done\":false}\n"
        ),
        "{response}"
    );
    assert!(
        response
            .contains("{\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":false}\n"),
        "{response}"
    );
    assert!(
        response
            .contains("{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n"),
        "{response}"
    );
}

#[test]
fn idle_timeout_closes_connection_and_releases_front() {
    let upstream = MockOllamaServer::start();
    let front = start_front(
        upstream.addr(),
        Duration::from_secs(2),
        Duration::from_millis(150),
        2,
    );
    let front_addr = front.addr();
    let mut idle = TcpStream::connect(front_addr).expect("open idle connection");
    idle.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set idle read timeout");

    thread::sleep(Duration::from_millis(350));
    assert_connection_released(&mut idle);

    let tags = get(front_addr, "/api/tags");
    front.join();

    assert!(tags.starts_with("HTTP/1.1 200 OK"), "{tags}");
    assert_eq!(upstream.count("/api/tags"), 1);
}

#[test]
fn request_timeout_releases_slow_handler_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind timeout front");
    let front_addr = listener.local_addr().expect("timeout front addr");
    let gateway = Arc::new(
        GatewayRuntime::open(GatewayConfig::default_for_local_dev()).expect("timeout runtime"),
    );
    let front = GatewayHttpFront::new(
        gateway,
        GatewayHttpFrontConfig {
            worker_count: 1,
            max_in_flight: 1,
            request_timeout: Duration::from_millis(120),
            idle_timeout: Duration::from_secs(2),
            force_close_client_connections: true,
        },
    )
    .expect("front config");
    let handle = thread::spawn(move || {
        front.serve_listener_n_with_factory(listener, 1, move || Box::new(SlowConnection))
    });

    let mut stream = TcpStream::connect(front_addr).expect("connect timeout front");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");
    stream
        .write_all(b"GET /api/tags HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("write request");

    thread::sleep(Duration::from_millis(260));
    assert_connection_released(&mut stream);
    handle
        .join()
        .expect("timeout front thread")
        .expect("timeout front result");
}

fn get(addr: SocketAddr, path: &str) -> String {
    request(
        addr,
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
        Duration::from_secs(2),
    )
}

fn post_json(addr: SocketAddr, path: &str, body: &str, timeout: Duration) -> String {
    request(
        addr,
        format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
        timeout,
    )
}

fn request(addr: SocketAddr, request: String, timeout: Duration) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect front");
    stream
        .set_read_timeout(Some(timeout))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(timeout))
        .expect("set write timeout");
    stream.write_all(request.as_bytes()).expect("write request");
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown request write");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}

fn assert_connection_released(stream: &mut TcpStream) {
    let mut response = String::new();
    match stream.read_to_string(&mut response) {
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
            ) => {}
        Err(error) => panic!("idle connection was not released by timeout: {error}"),
    }
}

struct SlowConnection;

impl GatewayHttpConnectionHandler for SlowConnection {
    fn handle(
        &mut self,
        _context: &GatewayRequestBudgetContext,
        _stream: &mut bm_entry::EntryAcceptedTcpStream,
    ) -> Result<()> {
        thread::sleep(Duration::from_millis(500));
        Ok(())
    }
}

struct MockOpenAiUpstream;

impl OpenAiCompatibleUpstream for MockOpenAiUpstream {
    fn models(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
    ) -> Result<OpenAiUpstreamResponse> {
        Ok(OpenAiUpstreamResponse::json(
            200,
            serde_json::json!({ "data": [] }),
        ))
    }

    fn chat_completion(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        _request: OpenAiUpstreamRequest,
    ) -> Result<OpenAiUpstreamResponse> {
        Ok(OpenAiUpstreamResponse::json(
            200,
            serde_json::json!({ "choices": [] }),
        ))
    }
}

struct MockOllamaServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    state: Arc<MockState>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockOllamaServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
        listener
            .set_nonblocking(true)
            .expect("mock listener nonblocking");
        let addr = listener.local_addr().expect("mock upstream addr");
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(MockState::default());
        let server_shutdown = Arc::clone(&shutdown);
        let server_state = Arc::clone(&state);
        let handle = thread::spawn(move || {
            while !server_shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("mock upstream stream blocking");
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .expect("mock upstream read timeout");
                        let state = Arc::clone(&server_state);
                        thread::spawn(move || serve_mock_upstream_connection(stream, state));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("mock upstream accept failed: {error}"),
                }
            }
        });
        Self {
            addr,
            shutdown,
            state,
            handle: Some(handle),
        }
    }

    fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn count(&self, path: &str) -> usize {
        self.state.count(path)
    }
}

impl Drop for MockOllamaServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            handle.join().expect("mock upstream thread");
        }
    }
}

#[derive(Default)]
struct MockState {
    inner: Mutex<MockInner>,
    condvar: Condvar,
}

impl MockState {
    fn observe(&self, path: &str) {
        let mut inner = self.inner.lock().expect("mock state lock");
        inner.requests.push(path.to_string());
        self.condvar.notify_all();
    }

    fn count(&self, path: &str) -> usize {
        let inner = self.inner.lock().expect("mock state lock");
        inner
            .requests
            .iter()
            .filter(|request_path| request_path.as_str() == path)
            .count()
    }

    fn wait_for_show(&self) {
        let inner = self.inner.lock().expect("mock state lock");
        let (_inner, wait_result) = self
            .condvar
            .wait_timeout_while(inner, Duration::from_secs(2), |inner| {
                !inner
                    .requests
                    .iter()
                    .any(|request_path| request_path == "/api/show")
            })
            .expect("mock wait for show");
        assert!(
            !wait_result.timed_out(),
            "chat upstream request did not overlap with show"
        );
    }
}

#[derive(Default)]
struct MockInner {
    requests: Vec<String>,
}

fn serve_mock_upstream_connection(mut stream: TcpStream, state: Arc<MockState>) {
    let request = read_mock_request(&mut stream).expect("read mock upstream request");
    state.observe(&request.path);
    match request.path.as_str() {
        "/api/tags" => write_json(
            &mut stream,
            r#"{"models":[{"name":"qwen2.5:7b","model":"qwen2.5:7b"}]}"#,
        ),
        "/api/show" => write_json(
            &mut stream,
            r#"{"capabilities":["completion"],"details":{"family":"qwen2"}}"#,
        ),
        "/api/chat" if request.body.contains(r#""stream":true"#) => write_ndjson_stream(
            &mut stream,
            &[
                "{\"message\":{\"role\":\"assistant\",\"content\":\"hel\"},\"done\":false}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":false}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n",
            ],
        ),
        "/api/chat" => {
            state.wait_for_show();
            write_json(
                &mut stream,
                r#"{"model":"qwen2.5:7b","message":{"role":"assistant","content":"ok"},"done":true}"#,
            );
        }
        path => write_status(
            &mut stream,
            404,
            "Not Found",
            &format!("unknown path: {path}"),
        ),
    }
}

struct MockHttpRequest {
    path: String,
    body: String,
}

fn read_mock_request(stream: &mut TcpStream) -> std::io::Result<MockHttpRequest> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    let mut content_length = 0_usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0_u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(MockHttpRequest {
        path,
        body: String::from_utf8(body).unwrap_or_default(),
    })
}

fn write_json(stream: &mut TcpStream, body: &str) {
    write_status(stream, 200, "OK", body);
}

fn write_status(stream: &mut TcpStream, status: u16, reason: &str, body: &str) {
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write mock json response");
    stream.flush().expect("flush mock json response");
}

fn write_ndjson_stream(stream: &mut TcpStream, chunks: &[&str]) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\nconnection: close\r\n\r\n"
    )
    .expect("write mock ndjson headers");
    for chunk in chunks {
        stream
            .write_all(chunk.as_bytes())
            .expect("write mock ndjson chunk");
        stream.flush().expect("flush mock ndjson chunk");
        thread::sleep(Duration::from_millis(30));
    }
}
