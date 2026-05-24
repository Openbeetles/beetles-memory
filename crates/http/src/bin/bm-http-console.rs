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
        let mut store = ConsoleStore::File(PathBuf::from("target/bm-http-console-store"));

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
                        "file" => ConsoleStore::File(PathBuf::from("target/bm-http-console-store")),
                        "memory" | "in-memory" => ConsoleStore::InMemory,
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
                    store = ConsoleStore::File(PathBuf::from(path));
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
