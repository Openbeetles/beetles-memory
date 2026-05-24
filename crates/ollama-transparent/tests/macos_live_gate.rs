use std::cell::Cell;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bm_ollama_transparent::{
    DisableOllamaTransparentRequest, EnableOllamaTransparentRequest, FileSystemRunnerInstaller,
    OllamaTransparentConfig, OllamaTransparentController, OllamaTransparentState, PortOwnerKind,
    SystemPortOwnerObserver, SystemProcessManager, TransitionOutcome, TransparentController,
};
use serde_json::Value;

#[test]
#[ignore = "requires local macOS Ollama.app, bm-llm-gateway binary, and an installed live model"]
fn macos_ollama_app_transparent_live_gate() {
    if std::env::var("BM_OLLAMA_TRANSPARENT_LIVE").as_deref() != Ok("1") {
        eprintln!("set BM_OLLAMA_TRANSPARENT_LIVE=1 to run the macOS live gate");
        return;
    }

    #[cfg(not(target_os = "macos"))]
    {
        panic!("Ollama App transparent live gate is macOS-only");
    }

    #[cfg(target_os = "macos")]
    {
        run_macos_live_gate();
    }
}

#[cfg(target_os = "macos")]
fn run_macos_live_gate() {
    let model = std::env::var("BM_OLLAMA_TRANSPARENT_LIVE_MODEL")
        .unwrap_or_else(|_| "qwen3.5:0.8b".to_string());
    let mut config = OllamaTransparentConfig::for_data_dir(live_data_dir());
    config.gateway_binary_path = gateway_binary_path();
    config.maintenance_model = model.clone();
    config.open_app_after_enable = true;
    config.restore_official_after_disable = true;

    assert!(
        config.official_ollama_binary.is_file(),
        "official Ollama binary missing: {}",
        config.official_ollama_binary.display()
    );
    assert!(
        config.gateway_binary_path.is_file(),
        "bm-llm-gateway binary missing; run cargo build -p bm-llm-gateway --no-default-features --features server-async,client-reqwest first: {}",
        config.gateway_binary_path.display()
    );

    let controller = TransparentController::new(
        config.clone(),
        SystemPortOwnerObserver::new(config.port_owner_classifier()),
        FileSystemRunnerInstaller,
        SystemProcessManager::default(),
    )
    .expect("transparent controller");
    let guard = LiveGateGuard {
        controller: Some(controller),
        restore_on_drop: Cell::new(false),
    };
    let controller = guard.controller.as_ref().expect("guard controller");

    let preflight = controller.preflight().expect("preflight");
    assert!(
        preflight.accepted
            || preflight
                .blockers
                .iter()
                .all(|blocker| blocker.message.contains("allow_stop_official_ollama")),
        "unexpected preflight blockers: {:?}",
        preflight.blockers
    );

    let transition = controller
        .enable(EnableOllamaTransparentRequest {
            open_app: Some(true),
            allow_stop_official_ollama: true,
        })
        .expect("enable transparent mode");
    assert_eq!(
        transition.outcome,
        TransitionOutcome::Completed,
        "{transition:?}"
    );
    assert_eq!(transition.to_state, OllamaTransparentState::Active);
    guard.restore_on_drop.set(true);

    let status = controller.status().expect("active status");
    assert_eq!(status.state, OllamaTransparentState::Active);
    assert_eq!(
        status.public_port.owner,
        PortOwnerKind::BeetleMemoryTransparentFront,
        "{status:?}"
    );
    assert_eq!(
        status.upstream_port.owner,
        PortOwnerKind::ManagedOllamaRunner,
        "{status:?}"
    );

    retry_json("GET", "/api/version", None, Duration::from_secs(20)).expect("/api/version");
    let tags = retry_json("GET", "/api/tags", None, Duration::from_secs(20)).expect("/api/tags");
    assert!(
        tags["models"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|item| item["name"].as_str() == Some(model.as_str())),
        "live model {model} not present in /api/tags response: {tags}"
    );

    let chat = retry_json(
        "POST",
        "/api/chat",
        Some(format!(
            r#"{{"model":"{model}","stream":false,"think":false,"messages":[{{"role":"user","content":"只回答 ok"}}]}}"#
        )),
        Duration::from_secs(90),
    )
    .expect("/api/chat through transparent front");
    assert!(
        chat.get("message").is_some() || chat.get("response").is_some(),
        "unexpected chat response: {chat}"
    );

    let disabled = controller
        .disable(DisableOllamaTransparentRequest {
            restore_official_app: Some(true),
        })
        .expect("disable transparent mode");
    assert_eq!(
        disabled.outcome,
        TransitionOutcome::Completed,
        "{disabled:?}"
    );
    assert_eq!(disabled.to_state, OllamaTransparentState::Disabled);
    guard.restore_on_drop.set(false);
}

#[cfg(target_os = "macos")]
struct LiveGateGuard {
    controller: Option<
        TransparentController<
            SystemPortOwnerObserver,
            FileSystemRunnerInstaller,
            SystemProcessManager,
        >,
    >,
    restore_on_drop: Cell<bool>,
}

#[cfg(target_os = "macos")]
impl Drop for LiveGateGuard {
    fn drop(&mut self) {
        if !self.restore_on_drop.get() {
            return;
        }
        if let Some(controller) = self.controller.take() {
            let _ = controller.disable(DisableOllamaTransparentRequest {
                restore_official_app: Some(true),
            });
        }
    }
}

#[cfg(target_os = "macos")]
fn gateway_binary_path() -> PathBuf {
    if let Some(path) = std::env::var_os("BM_OLLAMA_TRANSPARENT_GATEWAY_BIN") {
        return PathBuf::from(path);
    }
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .join("target")
        .join("debug")
        .join("bm-llm-gateway")
}

#[cfg(target_os = "macos")]
fn live_data_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("bm-ollama-transparent-live-{nanos}"))
}

#[cfg(target_os = "macos")]
fn retry_json(
    method: &str,
    path: &str,
    body: Option<String>,
    timeout: Duration,
) -> Result<Value, String> {
    let started = Instant::now();
    let mut last_error = String::new();
    while started.elapsed() < timeout {
        match request_json(method, path, body.as_deref(), timeout) {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = error;
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    }
    Err(last_error)
}

#[cfg(target_os = "macos")]
fn request_json(
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> Result<Value, String> {
    let mut stream = TcpStream::connect_timeout(
        &"127.0.0.1:11434".parse().expect("transparent public bind"),
        Duration::from_secs(2),
    )
    .map_err(|error| format!("connect 11434 failed: {error}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("set read timeout failed: {error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("set write timeout failed: {error}"))?;

    let body = body.unwrap_or("");
    let request = if method == "POST" {
        format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    } else {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    };
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write request failed: {error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("read response failed: {error}"))?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("invalid HTTP response: {response}"))?;
    let status = head.lines().next().unwrap_or_default();
    if !status.contains(" 2") {
        return Err(format!("non-2xx response {status}: {body}"));
    }
    serde_json::from_str(body).map_err(|error| format!("invalid JSON response {body}: {error}"))
}
