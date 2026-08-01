use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

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
            "usage": "bm-mcp-server stdio|http [--addr 127.0.0.1:8788] [--store-file PATH|--store-sqlite PATH|--store-memory] [--profile PROFILE] [--agent AGENT] [--channel CHANNEL] [--chat-id CHAT] [--connection-read-deadline-ms N --write-timeout-ms N] [--workers N] [--max-in-flight N]",
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
                "http_addr": "BM_MCP_HTTP_ADDR",
                "bearer_token": "BM_MCP_BEARER_TOKEN",
                "bearer_principal": "BM_MCP_BEARER_PRINCIPAL_ID",
                "bearer_owner": "BM_MCP_BEARER_OWNER_ID",
                "bearer_capabilities": "BM_MCP_BEARER_CAPABILITIES"
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
    connection_read_deadline: Option<std::time::Duration>,
    write_timeout: Option<std::time::Duration>,
    workers: usize,
    max_in_flight: usize,
    auth: bm_entry::EntryAuthConfig,
    principal_id: String,
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
                "--connection-read-deadline-ms" if http_mode => {
                    options.connection_read_deadline = Some(parse_timeout_ms(
                        "--connection-read-deadline-ms",
                        args.next().ok_or_else(|| {
                            "--connection-read-deadline-ms requires a value".to_string()
                        })?,
                    )?);
                }
                "--write-timeout-ms" if http_mode => {
                    options.write_timeout = Some(parse_timeout_ms(
                        "--write-timeout-ms",
                        args.next()
                            .ok_or_else(|| "--write-timeout-ms requires a value".to_string())?,
                    )?);
                }
                "--workers" if http_mode => {
                    options.workers = parse_nonzero_usize(
                        "--workers",
                        args.next()
                            .ok_or_else(|| "--workers requires a value".to_string())?,
                    )?;
                }
                "--max-in-flight" if http_mode => {
                    options.max_in_flight = parse_nonzero_usize(
                        "--max-in-flight",
                        args.next()
                            .ok_or_else(|| "--max-in-flight requires a value".to_string())?,
                    )?;
                }
                other => return Err(format!("unsupported bm-mcp-server option: {other}")),
            }
        }
        options.validate(http_mode)?;
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
        let (auth, principal_id, owner_id) = mcp_auth_from_env()?;
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
            owner_id,
            channel: std::env::var("BM_MEMORY_CHANNEL")
                .unwrap_or_else(|_| "llm.gateway".to_string()),
            chat_id: std::env::var("BM_MEMORY_CHAT_ID").unwrap_or_else(|_| "chat-1".to_string()),
            connection_read_deadline: None,
            write_timeout: None,
            workers: 8,
            max_in_flight: 64,
            auth,
            principal_id,
        })
    }

    fn validate(&self, http_mode: bool) -> Result<(), String> {
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
            ("principal", &self.principal_id),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{field} must not be empty"));
            }
        }
        if http_mode {
            self.network_front_config()?;
        }
        Ok(())
    }

    #[cfg(feature = "server-stdio")]
    fn network_front_config(&self) -> Result<bm_entry::EntryTcpNetworkFrontConfig, String> {
        let connection_read_deadline = self.connection_read_deadline.ok_or_else(|| {
            "MCP HTTP requires explicit --connection-read-deadline-ms".to_string()
        })?;
        let write_timeout = self
            .write_timeout
            .ok_or_else(|| "MCP HTTP requires explicit --write-timeout-ms".to_string())?;
        bm_entry::EntryTcpNetworkFrontConfig::new(
            self.workers,
            self.max_in_flight,
            connection_read_deadline,
            write_timeout,
        )
        .map_err(|error| error.to_string())
    }
}

fn env_truthy(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

fn parse_timeout_ms(label: &str, raw: String) -> Result<std::time::Duration, String> {
    let millis = raw
        .parse::<u64>()
        .map_err(|_| format!("{label} must be an integer"))?;
    if millis == 0 {
        return Err(format!("{label} must be greater than zero"));
    }
    Ok(std::time::Duration::from_millis(millis))
}

fn parse_nonzero_usize(label: &str, raw: String) -> Result<usize, String> {
    let value = raw
        .parse::<usize>()
        .map_err(|_| format!("{label} must be an integer"))?;
    if value == 0 {
        return Err(format!("{label} must be greater than zero"));
    }
    Ok(value)
}

fn mcp_auth_from_env() -> Result<(bm_entry::EntryAuthConfig, String, String), String> {
    use bm_entry::{EntryBearerPrincipal, EntryOperationCapability};

    let Ok(token) = std::env::var("BM_MCP_BEARER_TOKEN") else {
        return Ok((
            bm_entry::EntryAuthConfig::disabled_for_local(),
            "mcp-stdio-local".to_string(),
            std::env::var("BM_MEMORY_OWNER_ID").unwrap_or_else(|_| "owner-default".to_string()),
        ));
    };
    let principal_id = required_auth_env("BM_MCP_BEARER_PRINCIPAL_ID")?;
    let owner_id = required_auth_env("BM_MCP_BEARER_OWNER_ID")?;
    let capabilities = required_auth_env("BM_MCP_BEARER_CAPABILITIES")?
        .split(',')
        .map(|raw| match raw.trim() {
            "write" => Ok(EntryOperationCapability::Write),
            "recall" => Ok(EntryOperationCapability::Recall),
            "project" => Ok(EntryOperationCapability::Project),
            "maintain" => Ok(EntryOperationCapability::Maintain),
            "inspect" => Ok(EntryOperationCapability::Inspect),
            "recover" => Ok(EntryOperationCapability::Recover),
            "replay" => Ok(EntryOperationCapability::Replay),
            "long_term_list" => Ok(EntryOperationCapability::LongTermList),
            "long_term_detail" => Ok(EntryOperationCapability::LongTermDetail),
            "long_term_mutate" => Ok(EntryOperationCapability::LongTermMutate),
            "long_term_policy" => Ok(EntryOperationCapability::LongTermPolicy),
            "transcript_attr_write" => Ok(EntryOperationCapability::TranscriptAttrWrite),
            "capabilities" => Ok(EntryOperationCapability::Capabilities),
            "subscribe" => Ok(EntryOperationCapability::Subscribe),
            "close" => Ok(EntryOperationCapability::Close),
            "mcp_protocol" => Ok(EntryOperationCapability::McpProtocol),
            "llm_gateway_protocol" => Ok(EntryOperationCapability::LlmGatewayProtocol),
            other => Err(format!("unsupported MCP bearer capability: {other}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let principal = EntryBearerPrincipal::new(principal_id.clone(), owner_id.clone(), capabilities);
    Ok((
        bm_entry::EntryAuthConfig::required_bearer_principal(token, principal),
        principal_id,
        owner_id,
    ))
}

fn required_auth_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required when BM_MCP_BEARER_TOKEN is set"))
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
        bm_sdk::ProfileId::DesktopLinuxEmbeddedSdk,
        bm_sdk::ProfileId::DesktopWindowsEmbeddedSdk,
        bm_sdk::ProfileId::ServerLinuxMemoryGateway,
        bm_sdk::ProfileId::ServerLinuxDevFull,
    ]
}

#[cfg(feature = "server-stdio")]
fn run_stdio(options: McpServerOptions) -> Result<(), String> {
    use bm_mcp::{serve_mcp_stdio, McpToolServer};

    let runtime = runtime(&options, false)?;
    let server = McpToolServer::new("bm-mcp-server", "mcp-stdio-local");
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    serve_mcp_stdio(&server, &runtime, &mut stdin.lock(), &mut writer)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "server-stdio")]
fn run_http(options: McpServerOptions) -> Result<(), String> {
    use std::net::TcpListener;

    use bm_mcp::{
        serve_mcp_streamable_http_accepted_stream, validate_mcp_http_listener_security,
        McpToolServer,
    };

    let runtime = Arc::new(runtime(&options, true)?);
    let server = Arc::new(McpToolServer::new("bm-mcp-server", &options.principal_id));
    let listener = TcpListener::bind(&options.addr).map_err(|error| error.to_string())?;
    let local_addr = listener.local_addr().map_err(|error| error.to_string())?;
    validate_mcp_http_listener_security(&runtime, local_addr).map_err(|error| error.to_string())?;
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
    let network_front = options.network_front_config()?;
    let worker_runtime = Arc::clone(&runtime);
    let worker_server = Arc::clone(&server);
    let mut front = bm_entry::EntryTcpNetworkFront::new(network_front, move |mut stream| {
        let result =
            serve_mcp_streamable_http_accepted_stream(&worker_server, &worker_runtime, &mut stream)
                .map_err(|error| error.to_string());
        if let Err(error) = result {
            eprintln!(
                "{}",
                json!({
                    "binary": "bm-mcp-server",
                    "status": "request_error",
                    "error": error,
                })
            );
        }
    })
    .map_err(|error| error.to_string())?;
    loop {
        let stream = match bm_entry::EntryAcceptedTcpStream::accept(&listener) {
            Ok(stream) => stream,
            Err(error) => {
                front.shutdown().map_err(|error| error.to_string())?;
                return Err(error.to_string());
            }
        };
        match front.try_dispatch(stream) {
            Ok(bm_entry::EntryTcpDispatchOutcome::Accepted) => {}
            Ok(bm_entry::EntryTcpDispatchOutcome::RejectedSaturated) => eprintln!(
                "{}",
                json!({
                    "binary": "bm-mcp-server",
                    "status": "connection_rejected",
                    "error": "max in-flight reached",
                })
            ),
            Ok(bm_entry::EntryTcpDispatchOutcome::RejectedShuttingDown) => eprintln!(
                "{}",
                json!({
                    "binary": "bm-mcp-server",
                    "status": "connection_rejected",
                    "error": "network front is shutting down",
                })
            ),
            Err(error) => eprintln!(
                "{}",
                json!({
                    "binary": "bm-mcp-server",
                    "status": "connection_setup_error",
                    "error": error.to_string(),
                })
            ),
        }
    }
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
fn runtime(options: &McpServerOptions, http_mode: bool) -> Result<bm_entry::EntryRuntime, String> {
    use bm_entry::{
        EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
        EntryScope,
    };
    use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};

    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: options.agent_id.clone(),
            owner_id: options.owner_id.clone(),
        },
        scope: EntryScope {
            channel: options.channel.clone(),
            chat_id: options.chat_id.clone(),
        },
        store: StoreBackendConfig::for_backend(
            options.store_backend,
            options.store_path.clone(),
            options.profile,
        )
        .map_err(|error| error.to_string())?
        .with_fsync(options.fsync),
        transports: mcp_transport_config(),
        auth: if http_mode {
            options.auth.clone()
        } else {
            EntryAuthConfig::disabled_for_local()
        },
        idempotency: EntryIdempotencyConfig { max_keys: 1024 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .map_err(|error| error.to_string())
}

#[cfg(feature = "server-stdio")]
const fn mcp_transport_config() -> bm_entry::EntryTransportConfig {
    bm_entry::EntryTransportConfig {
        cli: false,
        http_server: false,
        wss_client: false,
        wss_server: false,
        mcp_server: true,
        a2a_bridge: false,
        llm_gateway_server: false,
    }
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
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    #[test]
    fn mcp_http_requires_explicit_nonzero_read_deadline_and_write_timeout() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        let error = McpServerOptions::parse(vec!["--store-memory".to_string()].into_iter(), true)
            .expect_err("HTTP mode without a read deadline must fail");
        assert!(error.contains("--connection-read-deadline-ms"), "{error}");

        let options = McpServerOptions::parse(
            vec![
                "--store-memory".to_string(),
                "--connection-read-deadline-ms".to_string(),
                "1000".to_string(),
                "--write-timeout-ms".to_string(),
                "1000".to_string(),
            ]
            .into_iter(),
            true,
        )
        .expect("explicit HTTP timeouts");
        assert_eq!(
            options.connection_read_deadline,
            Some(std::time::Duration::from_secs(1))
        );
        assert_eq!(
            options.write_timeout,
            Some(std::time::Duration::from_secs(1))
        );
    }

    #[test]
    fn mcp_binary_enables_only_its_real_transport() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        let _options =
            McpServerOptions::parse(vec!["--store-memory".to_string()].into_iter(), false)
                .expect("MCP stdio options");
        let transports = mcp_transport_config();

        assert!(transports.mcp_server);
        assert!(!transports.cli);
        assert!(!transports.http_server);
        assert!(!transports.wss_client);
        assert!(!transports.wss_server);
        assert!(!transports.a2a_bridge);
        assert!(!transports.llm_gateway_server);
    }

    #[test]
    fn mcp_http_rejects_unbounded_front_configuration() {
        let _lock = store_env_lock();
        let _guards = clear_store_env();
        let error = McpServerOptions::parse(
            vec![
                "--store-memory".to_string(),
                "--connection-read-deadline-ms".to_string(),
                "1000".to_string(),
                "--write-timeout-ms".to_string(),
                "1000".to_string(),
                "--workers".to_string(),
                "2".to_string(),
                "--max-in-flight".to_string(),
                "1".to_string(),
            ]
            .into_iter(),
            true,
        )
        .expect_err("front below worker count must fail");

        assert!(error.contains("max_in_flight"), "{error}");
    }
}
