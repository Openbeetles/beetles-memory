use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use bm_entry::{
    EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig, EntryIdentity,
    EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope,
    EntryTcpDispatchOutcome, EntryTcpNetworkFront, EntryTcpNetworkFrontConfig,
    EntryTransportConfig,
};
use bm_http::{serve_http_accepted_stream, validate_http_listener_security, HttpConsoleServices};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendConfig};

fn main() -> bm_sdk::Result<()> {
    let options = ConsoleServerOptions::from_args(std::env::args().skip(1))?;
    let runtime = Arc::new(EntryRuntime::open(options.runtime_config()?)?);
    let listener = TcpListener::bind(&options.addr)
        .map_err(|err| bm_sdk::Error::config("http_console_bind", err.to_string()))?;
    validate_http_listener_security(&runtime, &listener)?;
    println!(
        "Beetles Memory HTTP console listening on http://{}",
        options.addr
    );
    println!(
        "Store: {} {}",
        options.store.backend_label(),
        options.store.path_label()
    );

    let worker_runtime = Arc::clone(&runtime);
    let mut front = EntryTcpNetworkFront::new(options.network_front, move |mut stream| {
        if let Err(error) =
            serve_http_accepted_stream(&worker_runtime, &mut stream, HttpConsoleServices::none())
        {
            eprintln!("http console request failed: {error}");
        }
    })
    .map_err(|error| bm_sdk::Error::config("http_console_network_front", error.to_string()))?;

    loop {
        let stream = match bm_entry::EntryAcceptedTcpStream::accept(&listener) {
            Ok(stream) => stream,
            Err(error) => {
                front.shutdown().map_err(|shutdown_error| {
                    bm_sdk::Error::config("http_console_network_front", shutdown_error.to_string())
                })?;
                return Err(bm_sdk::Error::config(
                    "http_console_accept",
                    error.to_string(),
                ));
            }
        };
        match front.try_dispatch(stream) {
            Ok(EntryTcpDispatchOutcome::Accepted) => {}
            Ok(EntryTcpDispatchOutcome::RejectedSaturated) => {
                eprintln!("http console connection rejected: max in-flight reached");
            }
            Ok(EntryTcpDispatchOutcome::RejectedShuttingDown) => {
                eprintln!("http console connection rejected: network front is shutting down");
            }
            Err(error) => eprintln!("http console connection setup failed: {error}"),
        }
    }
}

#[derive(Clone, Debug)]
struct ConsoleServerOptions {
    addr: String,
    store: ConsoleStore,
    network_front: EntryTcpNetworkFrontConfig,
    auth: EntryAuthConfig,
    owner_id: String,
}

impl ConsoleServerOptions {
    fn from_args(mut args: impl Iterator<Item = String>) -> bm_sdk::Result<Self> {
        let mut addr = "127.0.0.1:8718".to_string();
        let mut store = None;
        let mut file_store_requested = false;
        let mut connection_read_deadline_ms = None;
        let mut write_timeout_ms = None;
        let mut workers = 8;
        let mut max_in_flight = 64;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--addr" => {
                    addr = args.next().ok_or_else(|| {
                        bm_sdk::Error::config("http_console_args", "--addr requires a value")
                    })?;
                }
                "--store" => {
                    let value = args.next().ok_or_else(|| {
                        bm_sdk::Error::config("http_console_args", "--store requires a value")
                    })?;
                    store = match value.as_str() {
                        "file" => {
                            file_store_requested = true;
                            None
                        }
                        "memory" | "in-memory" => {
                            file_store_requested = false;
                            Some(ConsoleStore::InMemory)
                        }
                        other => {
                            return Err(bm_sdk::Error::config(
                                "http_console_args",
                                format!("unsupported store: {other}"),
                            ))
                        }
                    };
                }
                "--store-path" => {
                    let path = args.next().ok_or_else(|| {
                        bm_sdk::Error::config("http_console_args", "--store-path requires a value")
                    })?;
                    file_store_requested = false;
                    store = Some(ConsoleStore::File(memory_store_path_from_arg(path)?));
                }
                "--connection-read-deadline-ms" => {
                    connection_read_deadline_ms = Some(parse_timeout_ms(
                        "--connection-read-deadline-ms",
                        args.next().ok_or_else(|| {
                            bm_sdk::Error::config(
                                "http_console_args",
                                "--connection-read-deadline-ms requires a value",
                            )
                        })?,
                    )?);
                }
                "--write-timeout-ms" => {
                    write_timeout_ms = Some(parse_timeout_ms(
                        "--write-timeout-ms",
                        args.next().ok_or_else(|| {
                            bm_sdk::Error::config(
                                "http_console_args",
                                "--write-timeout-ms requires a value",
                            )
                        })?,
                    )?);
                }
                "--workers" => {
                    workers = parse_nonzero_usize(
                        "--workers",
                        args.next().ok_or_else(|| {
                            bm_sdk::Error::config("http_console_args", "--workers requires a value")
                        })?,
                    )?;
                }
                "--max-in-flight" => {
                    max_in_flight = parse_nonzero_usize(
                        "--max-in-flight",
                        args.next().ok_or_else(|| {
                            bm_sdk::Error::config(
                                "http_console_args",
                                "--max-in-flight requires a value",
                            )
                        })?,
                    )?;
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                other => {
                    return Err(bm_sdk::Error::config(
                        "http_console_args",
                        format!("unknown argument: {other}"),
                    ));
                }
            }
        }

        let store = match store {
            Some(store) => store,
            None if file_store_requested => {
                return Err(bm_sdk::Error::config(
                    "http_console_args",
                    "--store file requires --store-path with an absolute path",
                ))
            }
            None => {
                return Err(bm_sdk::Error::config(
                    "http_console_args",
                    "memory store backend must be explicit: use --store memory or --store-path with an absolute path",
                ))
            }
        };
        let connection_read_deadline = connection_read_deadline_ms.ok_or_else(|| {
            bm_sdk::Error::config(
                "http_console_args",
                "--connection-read-deadline-ms must be configured explicitly",
            )
        })?;
        let write_timeout = write_timeout_ms.ok_or_else(|| {
            bm_sdk::Error::config(
                "http_console_args",
                "--write-timeout-ms must be configured explicitly",
            )
        })?;
        let network_front = EntryTcpNetworkFrontConfig::new(
            workers,
            max_in_flight,
            connection_read_deadline,
            write_timeout,
        )
        .map_err(|error| bm_sdk::Error::config("http_console_args", error.to_string()))?;
        let (auth, owner_id) = auth_from_env()?;
        Ok(Self {
            addr,
            store,
            network_front,
            auth,
            owner_id,
        })
    }

    fn runtime_config(&self) -> bm_sdk::Result<EntryRuntimeConfig> {
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        let profile = ProfileId::native_dev_full().ok_or_else(|| {
            bm_sdk::Error::config(
                "http_console_profile",
                "host-native dev-full profile is unavailable",
            )
        })?;
        Ok(EntryRuntimeConfig {
            identity: EntryIdentity {
                agent_id: "bm-http-console".to_string(),
                owner_id: self.owner_id.clone(),
            },
            scope: EntryScope {
                channel: "console".to_string(),
                chat_id: "local-console".to_string(),
            },
            store: self.store.store_config(profile)?,
            transports: EntryTransportConfig {
                cli: false,
                http_server: true,
                wss_client: false,
                wss_server: false,
                mcp_server: false,
                a2a_bridge: false,
                llm_gateway_server: false,
            },
            auth: self.auth.clone(),
            idempotency: EntryIdempotencyConfig { max_keys: 4096 },
            privacy: MemoryPrivacyPolicy::standard_private_boundary(),
            capability,
        })
    }
}

#[derive(Clone, Debug)]
enum ConsoleStore {
    File(PathBuf),
    InMemory,
}

impl ConsoleStore {
    fn store_config(&self, profile: ProfileId) -> bm_sdk::Result<StoreBackendConfig> {
        match self {
            Self::File(path) => StoreBackendConfig::file(path, profile),
            Self::InMemory => {
                StoreBackendConfig::in_memory(profile).map(|config| config.with_fsync(false))
            }
        }
    }

    fn backend_label(&self) -> &'static str {
        match self {
            Self::File(_) => "file",
            Self::InMemory => "in-memory",
        }
    }

    fn path_label(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::InMemory => "(volatile)".to_string(),
        }
    }
}

fn print_usage() {
    println!(
        "Usage: bm-http-console [--addr 127.0.0.1:8718] [--store file|memory] [--store-path PATH] --connection-read-deadline-ms N --write-timeout-ms N [--workers N] [--max-in-flight N]"
    );
}

fn parse_timeout_ms(label: &str, raw: String) -> bm_sdk::Result<std::time::Duration> {
    let millis = raw.parse::<u64>().map_err(|_| {
        bm_sdk::Error::config("http_console_args", format!("{label} must be an integer"))
    })?;
    if millis == 0 {
        return Err(bm_sdk::Error::config(
            "http_console_args",
            format!("{label} must be greater than zero"),
        ));
    }
    Ok(std::time::Duration::from_millis(millis))
}

fn parse_nonzero_usize(label: &str, raw: String) -> bm_sdk::Result<usize> {
    let value = raw.parse::<usize>().map_err(|_| {
        bm_sdk::Error::config("http_console_args", format!("{label} must be an integer"))
    })?;
    if value == 0 {
        return Err(bm_sdk::Error::config(
            "http_console_args",
            format!("{label} must be greater than zero"),
        ));
    }
    Ok(value)
}

fn auth_from_env() -> bm_sdk::Result<(EntryAuthConfig, String)> {
    let Ok(token) = std::env::var("BM_HTTP_BEARER_TOKEN") else {
        return Ok((
            EntryAuthConfig::disabled_for_local(),
            "local-owner".to_string(),
        ));
    };
    let principal_id = required_env("BM_HTTP_BEARER_PRINCIPAL_ID")?;
    let owner_id = required_env("BM_HTTP_BEARER_OWNER_ID")?;
    let capabilities = required_env("BM_HTTP_BEARER_CAPABILITIES")?
        .split(',')
        .map(parse_capability)
        .collect::<bm_sdk::Result<Vec<_>>>()?;
    let principal = EntryBearerPrincipal::new(principal_id, owner_id.clone(), capabilities);
    Ok((
        EntryAuthConfig::required_bearer_principal(token, principal),
        owner_id,
    ))
}

fn required_env(name: &str) -> bm_sdk::Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            bm_sdk::Error::config(
                "http_console_auth",
                format!("{name} is required when BM_HTTP_BEARER_TOKEN is set"),
            )
        })
}

fn parse_capability(raw: &str) -> bm_sdk::Result<EntryOperationCapability> {
    let capability = match raw.trim() {
        "write" => EntryOperationCapability::Write,
        "recall" => EntryOperationCapability::Recall,
        "project" => EntryOperationCapability::Project,
        "maintain" => EntryOperationCapability::Maintain,
        "inspect" => EntryOperationCapability::Inspect,
        "recover" => EntryOperationCapability::Recover,
        "replay" => EntryOperationCapability::Replay,
        "long_term_list" => EntryOperationCapability::LongTermList,
        "long_term_detail" => EntryOperationCapability::LongTermDetail,
        "long_term_mutate" => EntryOperationCapability::LongTermMutate,
        "long_term_policy" => EntryOperationCapability::LongTermPolicy,
        "transcript_attr_write" => EntryOperationCapability::TranscriptAttrWrite,
        "capabilities" => EntryOperationCapability::Capabilities,
        "subscribe" => EntryOperationCapability::Subscribe,
        "close" => EntryOperationCapability::Close,
        "console_read" => EntryOperationCapability::ConsoleRead,
        "console_write" => EntryOperationCapability::ConsoleWrite,
        "mcp_protocol" => EntryOperationCapability::McpProtocol,
        "llm_gateway_protocol" => EntryOperationCapability::LlmGatewayProtocol,
        other => {
            return Err(bm_sdk::Error::config(
                "http_console_auth",
                format!("unsupported bearer capability: {other}"),
            ))
        }
    };
    Ok(capability)
}

fn memory_store_path_from_arg(raw: String) -> bm_sdk::Result<PathBuf> {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(bm_sdk::Error::config(
            "http_console_args",
            "--store-path must be an absolute path",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_console_requires_explicit_store_backend() {
        let error = ConsoleServerOptions::from_args(std::iter::empty())
            .expect_err("store backend must be explicit");
        assert!(error.to_string().contains("explicit"), "{error}");
    }

    #[test]
    fn http_console_rejects_file_store_without_absolute_path() {
        let error = ConsoleServerOptions::from_args(
            vec!["--store".to_string(), "file".to_string()].into_iter(),
        )
        .expect_err("file store without path must fail");
        assert!(error.to_string().contains("--store-path"), "{error}");

        let error = ConsoleServerOptions::from_args(
            vec![
                "--store-path".to_string(),
                "target/http-console-store".to_string(),
            ]
            .into_iter(),
        )
        .expect_err("relative file store path must fail");
        assert!(error.to_string().contains("absolute"), "{error}");
    }

    #[test]
    fn http_console_accepts_explicit_volatile_memory_store() {
        let options = ConsoleServerOptions::from_args(
            vec![
                "--store".to_string(),
                "memory".to_string(),
                "--connection-read-deadline-ms".to_string(),
                "1000".to_string(),
                "--write-timeout-ms".to_string(),
                "1000".to_string(),
            ]
            .into_iter(),
        )
        .expect("explicit memory store");
        assert!(matches!(options.store, ConsoleStore::InMemory));
    }

    #[test]
    fn http_console_requires_explicit_nonzero_read_deadline_and_write_timeout() {
        let error = ConsoleServerOptions::from_args(
            vec!["--store".to_string(), "memory".to_string()].into_iter(),
        )
        .expect_err("missing read deadline must fail");
        assert!(
            error.to_string().contains("--connection-read-deadline-ms"),
            "{error}"
        );

        let error = ConsoleServerOptions::from_args(
            vec![
                "--store".to_string(),
                "memory".to_string(),
                "--connection-read-deadline-ms".to_string(),
                "0".to_string(),
            ]
            .into_iter(),
        )
        .expect_err("zero timeout must fail");
        assert!(error.to_string().contains("greater than zero"), "{error}");
    }

    #[test]
    fn http_console_enables_only_its_real_transport() {
        let options = ConsoleServerOptions::from_args(
            vec![
                "--store".to_string(),
                "memory".to_string(),
                "--connection-read-deadline-ms".to_string(),
                "1000".to_string(),
                "--write-timeout-ms".to_string(),
                "1000".to_string(),
            ]
            .into_iter(),
        )
        .expect("HTTP options");
        let transports = options.runtime_config().expect("runtime config").transports;

        assert!(transports.http_server);
        assert!(!transports.cli);
        assert!(!transports.wss_client);
        assert!(!transports.wss_server);
        assert!(!transports.mcp_server);
        assert!(!transports.a2a_bridge);
        assert!(!transports.llm_gateway_server);
    }

    #[test]
    fn http_console_rejects_unbounded_front_configuration() {
        let error = ConsoleServerOptions::from_args(
            vec![
                "--store".to_string(),
                "memory".to_string(),
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
        )
        .expect_err("front below worker count must fail");

        assert!(error.to_string().contains("max_in_flight"), "{error}");
    }
}
