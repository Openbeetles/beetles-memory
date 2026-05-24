use serde_json::json;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!(
            "{}",
            json!({
                "binary": "bm-mcp-server",
                "status": "error",
                "error": error,
            })
        );
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("stdio") => {
            let args = args.collect::<Vec<_>>();
            if args.iter().any(|arg| arg == "--help" || arg == "-h") {
                print_usage();
                Ok(())
            } else {
                run_stdio(McpServerOptions::parse(args.into_iter(), false)?)
            }
        }
        Some("http") => {
            let args = args.collect::<Vec<_>>();
            if args.iter().any(|arg| arg == "--help" || arg == "-h") {
                print_usage();
                Ok(())
            } else {
                run_http(McpServerOptions::parse(args.into_iter(), true)?)
            }
        }
        Some("--help") | Some("-h") | None => {
            print_usage();
            Ok(())
        }
        Some(other) => Err(format!("unsupported bm-mcp-server mode: {other}")),
    }
}

fn print_usage() {
    println!(
        "{}",
        json!({
            "binary": "bm-mcp-server",
            "usage": "bm-mcp-server stdio|http [--addr 127.0.0.1:8788] [--store-file PATH|--store-sqlite PATH|--store-memory] [--profile profile-server-linux-memory-gateway] [--owner OWNER] [--agent AGENT] [--channel CHANNEL] [--chat-id CHAT]",
            "modes": ["stdio", "http"],
            "env": {
                "store_file": "BM_MEMORY_STORE_FILE",
                "store_sqlite": "BM_MEMORY_STORE_SQLITE",
                "store_memory": "BM_MEMORY_STORE_MEMORY",
                "profile": "BM_MEMORY_PROFILE",
                "owner": "BM_MEMORY_OWNER_ID",
                "agent": "BM_MEMORY_AGENT_ID",
                "channel": "BM_MEMORY_CHANNEL",
                "chat_id": "BM_MEMORY_CHAT_ID",
                "http_addr": "BM_MCP_HTTP_ADDR"
            },
            "features": {
                "server_stdio": cfg!(feature = "server-stdio")
            }
        })
    );
}

#[derive(Clone, Debug)]
struct McpServerOptions {
    addr: String,
    profile: bm_sdk::ProfileId,
    store_backend: bm_sdk::StoreBackendKind,
    store_path: Option<PathBuf>,
    fsync: bool,
    agent_id: String,
    owner_id: String,
    channel: String,
    chat_id: String,
}

impl McpServerOptions {
    fn parse(args: impl Iterator<Item = String>, http_mode: bool) -> Result<Self, String> {
        let mut options = Self::from_env()?;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--addr" if http_mode => {
                    options.addr = args
                        .next()
                        .ok_or_else(|| "--addr requires a value".to_string())?;
                }
                "--profile" => {
                    let raw = args
                        .next()
                        .ok_or_else(|| "--profile requires a value".to_string())?;
                    options.profile = parse_profile(&raw)?;
                }
                "--store-file" => {
                    options.store_backend = bm_sdk::StoreBackendKind::File;
                    options.store_path =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            "--store-file requires a value".to_string()
                        })?));
                    options.fsync = true;
                }
                "--store-sqlite" => {
                    options.store_backend = bm_sdk::StoreBackendKind::Sqlite;
                    options.store_path =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            "--store-sqlite requires a value".to_string()
                        })?));
                    options.fsync = true;
                }
                "--store-memory" => {
                    options.store_backend = bm_sdk::StoreBackendKind::InMemory;
                    options.store_path = None;
                    options.fsync = false;
                }
                "--no-fsync" => {
                    options.fsync = false;
                }
                "--owner" => {
                    options.owner_id = args
                        .next()
                        .ok_or_else(|| "--owner requires a value".to_string())?;
                }
                "--agent" => {
                    options.agent_id = args
                        .next()
                        .ok_or_else(|| "--agent requires a value".to_string())?;
                }
                "--channel" => {
                    options.channel = args
                        .next()
                        .ok_or_else(|| "--channel requires a value".to_string())?;
                }
                "--chat-id" | "--chat" => {
                    options.chat_id = args
                        .next()
                        .ok_or_else(|| "--chat-id requires a value".to_string())?;
                }
                other => return Err(format!("unsupported bm-mcp-server option: {other}")),
            }
        }
        options.validate()?;
        Ok(options)
    }

    fn from_env() -> Result<Self, String> {
        let mut store_backend = bm_sdk::StoreBackendKind::File;
        let mut store_path = Some(PathBuf::from("target/bm-memory-gateway-store"));
        let mut fsync = true;
        if env_truthy("BM_MEMORY_STORE_MEMORY") {
            store_backend = bm_sdk::StoreBackendKind::InMemory;
            store_path = None;
            fsync = false;
        } else if let Ok(path) = std::env::var("BM_MEMORY_STORE_SQLITE") {
            store_backend = bm_sdk::StoreBackendKind::Sqlite;
            store_path = Some(PathBuf::from(path));
        } else if let Ok(path) = std::env::var("BM_MEMORY_STORE_FILE") {
            store_backend = bm_sdk::StoreBackendKind::File;
            store_path = Some(PathBuf::from(path));
        }
        let profile = match std::env::var("BM_MEMORY_PROFILE") {
            Ok(raw) => parse_profile(&raw)?,
            Err(_) => bm_sdk::ProfileId::ServerLinuxMemoryGateway,
        };
        Ok(Self {
            addr: std::env::var("BM_MCP_HTTP_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:8788".to_string()),
            profile,
            store_backend,
            store_path,
            fsync,
            agent_id: std::env::var("BM_MEMORY_AGENT_ID")
                .unwrap_or_else(|_| "agent-main".to_string()),
            owner_id: std::env::var("BM_MEMORY_OWNER_ID")
                .unwrap_or_else(|_| "owner-default".to_string()),
            channel: std::env::var("BM_MEMORY_CHANNEL")
                .unwrap_or_else(|_| "llm.gateway".to_string()),
            chat_id: std::env::var("BM_MEMORY_CHAT_ID").unwrap_or_else(|_| "chat-1".to_string()),
        })
    }

    fn validate(&self) -> Result<(), String> {
        if !matches!(self.store_backend, bm_sdk::StoreBackendKind::InMemory)
            && self.store_path.is_none()
        {
            return Err("file/sqlite stores require a store path".to_string());
        }
        for (field, value) in [
            ("owner", &self.owner_id),
            ("agent", &self.agent_id),
            ("channel", &self.channel),
            ("chat-id", &self.chat_id),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{field} must not be empty"));
            }
        }
        Ok(())
    }
}

fn env_truthy(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

fn parse_profile(raw: &str) -> Result<bm_sdk::ProfileId, String> {
    platform_profiles()
        .iter()
        .copied()
        .find(|profile| bm_sdk::platform_capability_snapshot_file_name(*profile) == raw)
        .ok_or_else(|| format!("unsupported platform profile: {raw}"))
}

fn platform_profiles() -> &'static [bm_sdk::ProfileId] {
    &[
        bm_sdk::ProfileId::EspStandaloneMemory,
        bm_sdk::ProfileId::EspEmbeddedSdk,
        bm_sdk::ProfileId::LinuxDeviceStandaloneMemory,
        bm_sdk::ProfileId::DesktopMacosStandaloneMemory,
        bm_sdk::ProfileId::DesktopMacosEmbeddedSdk,
        bm_sdk::ProfileId::DesktopWindowsEmbeddedSdk,
        bm_sdk::ProfileId::ServerLinuxMemoryGateway,
        bm_sdk::ProfileId::ServerLinuxDevFull,
    ]
}

#[cfg(feature = "server-stdio")]
fn run_stdio(options: McpServerOptions) -> Result<(), String> {
    use std::io::{BufRead, Cursor};

    use bm_mcp::{serve_mcp_stdio_once, McpToolServer};

    let runtime = runtime(&options)?;
    let server = McpToolServer::new("bm-mcp-server");
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let mut reader = Cursor::new(format!("{line}\n").into_bytes());
        serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(feature = "server-stdio")]
fn run_http(options: McpServerOptions) -> Result<(), String> {
    use std::net::TcpListener;

    use bm_mcp::{serve_mcp_streamable_http_stream, McpToolServer};

    let runtime = runtime(&options)?;
    let server = McpToolServer::new("bm-mcp-server");
    let listener = TcpListener::bind(&options.addr).map_err(|error| error.to_string())?;
    println!(
        "{}",
        json!({
            "binary": "bm-mcp-server",
            "status": "listening",
            "mode": "http",
            "addr": options.addr,
            "path": "/mcp",
            "store": store_label(options.store_backend, options.store_path.as_deref()),
            "owner": options.owner_id,
            "agent": options.agent_id,
            "channel": options.channel,
            "chat_id": options.chat_id,
        })
    );
    for stream in listener.incoming() {
        let mut stream = stream.map_err(|error| error.to_string())?;
        if let Err(error) = serve_mcp_streamable_http_stream(&server, &runtime, &mut stream) {
            eprintln!(
                "{}",
                json!({
                    "binary": "bm-mcp-server",
                    "status": "request_error",
                    "error": error.to_string(),
                })
            );
        }
    }
    Ok(())
}

#[cfg(not(feature = "server-stdio"))]
fn run_stdio(_options: McpServerOptions) -> Result<(), String> {
    print_usage();
    Err("stdio mode requires the server-stdio feature".to_string())
}

#[cfg(not(feature = "server-stdio"))]
fn run_http(_options: McpServerOptions) -> Result<(), String> {
    print_usage();
    Err("http mode requires the server-stdio feature".to_string())
}

#[cfg(feature = "server-stdio")]
fn runtime(options: &McpServerOptions) -> Result<bm_entry::EntryRuntime, String> {
    use bm_entry::{
        EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
        EntryScope, EntryStoreConfig, EntryTransportConfig,
    };
    use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy};

    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: options.profile,
        identity: EntryIdentity {
            agent_id: options.agent_id.clone(),
            owner_id: options.owner_id.clone(),
        },
        scope: EntryScope {
            channel: options.channel.clone(),
            chat_id: options.chat_id.clone(),
        },
        store: EntryStoreConfig {
            backend: options.store_backend,
            data_path: options.store_path.clone(),
            fsync: options.fsync,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 1024 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .map_err(|error| error.to_string())
}

#[cfg(feature = "server-stdio")]
fn store_label(backend: bm_sdk::StoreBackendKind, path: Option<&std::path::Path>) -> String {
    match backend {
        bm_sdk::StoreBackendKind::InMemory => "memory".to_string(),
        bm_sdk::StoreBackendKind::Embedded => "embedded".to_string(),
        bm_sdk::StoreBackendKind::File => format!(
            "file:{}",
            path.map(|path| path.display().to_string())
                .unwrap_or_else(|| "<missing>".to_string())
        ),
        bm_sdk::StoreBackendKind::Sqlite => format!(
            "sqlite:{}",
            path.map(|path| path.display().to_string())
                .unwrap_or_else(|| "<missing>".to_string())
        ),
    }
}
