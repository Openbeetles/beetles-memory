use std::net::TcpListener;
use std::path::PathBuf;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{serve_http_listener_once_with_console_services, HttpConsoleServices};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn main() -> bm_sdk::Result<()> {
    let options = ConsoleServerOptions::from_args(std::env::args().skip(1))?;
    let runtime = EntryRuntime::open(options.runtime_config())?;
    let listener = TcpListener::bind(&options.addr)
        .map_err(|err| bm_sdk::Error::config("http_console_bind", err.to_string()))?;
    println!(
        "Beetles Memory HTTP console listening on http://{}",
        options.addr
    );
    println!(
        "Store: {} {}",
        options.store.backend_label(),
        options.store.path_label()
    );

    loop {
        if let Err(err) = serve_http_listener_once_with_console_services(
            &runtime,
            &listener,
            HttpConsoleServices::none(),
        ) {
            eprintln!("http console request failed: {err}");
        }
    }
}

#[derive(Clone, Debug)]
struct ConsoleServerOptions {
    addr: String,
    store: ConsoleStore,
}

impl ConsoleServerOptions {
    fn from_args(mut args: impl Iterator<Item = String>) -> bm_sdk::Result<Self> {
        let mut addr = "127.0.0.1:8718".to_string();
        let mut store = None;
        let mut file_store_requested = false;

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
        Ok(Self { addr, store })
    }

    fn runtime_config(&self) -> EntryRuntimeConfig {
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        EntryRuntimeConfig {
            profile: ProfileId::ServerLinuxDevFull,
            identity: EntryIdentity {
                agent_id: "bm-http-console".to_string(),
                owner_id: "local-owner".to_string(),
            },
            scope: EntryScope {
                channel: "console".to_string(),
                chat_id: "local-console".to_string(),
            },
            store: self.store.entry_store_config(),
            transports: EntryTransportConfig {
                cli: false,
                http_server: true,
                wss_client: false,
                wss_server: true,
                mcp_server: true,
                a2a_bridge: true,
                llm_gateway_server: false,
            },
            auth: EntryAuthConfig::disabled_for_local(),
            idempotency: EntryIdempotencyConfig { max_keys: 4096 },
            privacy: MemoryPrivacyPolicy::standard_private_boundary(),
            capability,
        }
    }
}

#[derive(Clone, Debug)]
enum ConsoleStore {
    File(PathBuf),
    InMemory,
}

impl ConsoleStore {
    fn entry_store_config(&self) -> EntryStoreConfig {
        match self {
            Self::File(path) => EntryStoreConfig {
                backend: StoreBackendKind::File,
                data_path: Some(path.clone()),
                fsync: true,
            },
            Self::InMemory => EntryStoreConfig {
                backend: StoreBackendKind::InMemory,
                data_path: None,
                fsync: false,
            },
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
        "Usage: bm-http-console [--addr 127.0.0.1:8718] [--store file|memory] [--store-path PATH]"
    );
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
            vec!["--store".to_string(), "memory".to_string()].into_iter(),
        )
        .expect("explicit memory store");
        assert!(matches!(options.store, ConsoleStore::InMemory));
    }
}
