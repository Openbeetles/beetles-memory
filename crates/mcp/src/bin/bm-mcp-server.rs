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
    store_explicit: bool,
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
                    options.store_path = Some(memory_store_path_from_arg(
                        "--store-file",
                        args.next()
                            .ok_or_else(|| "--store-file requires a value".to_string())?,
                    )?);
                    options.store_explicit = true;
                    options.fsync = true;
                }
                "--store-sqlite" => {
                    options.store_backend = bm_sdk::StoreBackendKind::Sqlite;
                    options.store_path = Some(memory_store_path_from_arg(
                        "--store-sqlite",
                        args.next()
                            .ok_or_else(|| "--store-sqlite requires a value".to_string())?,
                    )?);
                    options.store_explicit = true;
                    options.fsync = true;
                }
                "--store-memory" => {
                    options.store_backend = bm_sdk::StoreBackendKind::InMemory;
                    options.store_path = None;
                    options.store_explicit = true;
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
        let mut store_backend = bm_sdk::StoreBackendKind::InMemory;
        let mut store_path = None;
        let mut store_explicit = false;
        let mut fsync = true;
        if env_truthy("BM_MEMORY_STORE_MEMORY") {
            store_backend = bm_sdk::StoreBackendKind::InMemory;
            store_path = None;
            store_explicit = true;
            fsync = false;
        } else if let Ok(path) = std::env::var("BM_MEMORY_STORE_SQLITE") {
            store_backend = bm_sdk::StoreBackendKind::Sqlite;
            store_path = Some(memory_store_path_from_arg("BM_MEMORY_STORE_SQLITE", path)?);
            store_explicit = true;
        } else if let Ok(path) = std::env::var("BM_MEMORY_STORE_FILE") {
            store_backend = bm_sdk::StoreBackendKind::File;
            store_path = Some(memory_store_path_from_arg("BM_MEMORY_STORE_FILE", path)?);
            store_explicit = true;
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
            store_explicit,
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
        if !self.store_explicit {
            return Err("memory store backend must be explicit: use --store-memory or an absolute --store-file/--store-sqlite path".to_string());
        }
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

fn memory_store_path_from_arg(label: &str, raw: String) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{label} must be an absolute path"))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    struct EnvRestore {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn new(key: &'static str) -> Self {
            Self {
                key,
                old: std::env::var_os(key),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(value) = self.old.as_ref() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn clear_store_env() -> Vec<EnvRestore> {
        let guards = vec![
            EnvRestore::new("BM_MEMORY_STORE_FILE"),
            EnvRestore::new("BM_MEMORY_STORE_SQLITE"),
            EnvRestore::new("BM_MEMORY_STORE_MEMORY"),
        ];
        std::env::remove_var("BM_MEMORY_STORE_FILE");
        std::env::remove_var("BM_MEMORY_STORE_SQLITE");
        std::env::remove_var("BM_MEMORY_STORE_MEMORY");
        guards
    }

    fn store_env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn mcp_server_requires_explicit_store_backend() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        let error = McpServerOptions::parse(std::iter::empty(), false)
            .expect_err("store backend must be explicit");
        assert!(error.contains("explicit"), "{error}");
    }

    #[test]
    fn mcp_server_rejects_relative_persistent_store_path() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        let error = McpServerOptions::parse(
            vec!["--store-file".to_string(), "target/mcp-store".to_string()].into_iter(),
            false,
        )
        .expect_err("relative file store path must fail");
        assert!(error.contains("absolute"), "{error}");

        let error = McpServerOptions::parse(
            vec![
                "--store-sqlite".to_string(),
                "target/mcp.sqlite3".to_string(),
            ]
            .into_iter(),
            false,
        )
        .expect_err("relative sqlite store path must fail");
        assert!(error.contains("absolute"), "{error}");
    }

    #[test]
    fn mcp_server_accepts_explicit_volatile_memory_store() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        let options =
            McpServerOptions::parse(vec!["--store-memory".to_string()].into_iter(), false)
                .expect("explicit memory store should be accepted");
        assert_eq!(options.store_backend, bm_sdk::StoreBackendKind::InMemory);
        assert_eq!(options.store_path, None);
    }
}
