//! CLI adapter for Beetle Memory.

use std::path::PathBuf;

use bm_adapter::{AdapterCommand, AdapterOperation, AdapterResponse, AdapterSdkReport};
use bm_entry::{
    EntryAuthConfig, EntryAuthDecision, EntryIdempotencyConfig, EntryIdentity, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryStoreConfig, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    platform_capability_snapshot, platform_capability_snapshot_file_name,
    resolve_memory_capabilities, ContinuitySnapshot, ContinuitySnapshotImportMode,
    MemoryCapabilityCatalog, MemoryCapabilityPolicy, MemoryExportRequest, MemoryImportRequest,
    MemoryInspectionRequest, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryReplayRequest, MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput,
    RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendKind,
};
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub usage: &'static str,
    pub operation: AdapterOperation,
}

const COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "capabilities",
        usage: "bm memory capabilities",
        operation: AdapterOperation::Capabilities,
    },
    CommandSpec {
        name: "inspect",
        usage: "bm memory inspect --query <query>",
        operation: AdapterOperation::Inspect,
    },
    CommandSpec {
        name: "recall",
        usage: "bm memory recall --query <query> --limit <n>",
        operation: AdapterOperation::Recall,
    },
    CommandSpec {
        name: "project",
        usage: "bm memory project --query <query> --max-len <n>",
        operation: AdapterOperation::Project,
    },
    CommandSpec {
        name: "replay",
        usage: "bm memory replay --chat-id <chat_id> --limit <n>",
        operation: AdapterOperation::Replay,
    },
    CommandSpec {
        name: "export",
        usage: "bm memory export --chat-id <chat_id> --output <path>",
        operation: AdapterOperation::Export,
    },
    CommandSpec {
        name: "import",
        usage: "bm memory import --input <path> --target-chat-id <chat_id>",
        operation: AdapterOperation::Import,
    },
    CommandSpec {
        name: "write-procedural",
        usage: "bm memory write-procedural --name <name> --content <content>",
        operation: AdapterOperation::Write,
    },
    CommandSpec {
        name: "close",
        usage: "bm memory close --reason <reason>",
        operation: AdapterOperation::Close,
    },
];

pub const fn command_specs() -> &'static [CommandSpec] {
    COMMAND_SPECS
}

pub const fn platform_profiles() -> &'static [ProfileId] {
    &[
        ProfileId::EspStandaloneMemory,
        ProfileId::EspEmbeddedSdk,
        ProfileId::LinuxDeviceStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
        ProfileId::ServerLinuxMemoryGateway,
        ProfileId::ServerLinuxDevFull,
    ]
}

pub fn parse_platform_profile_id(raw: &str) -> Option<ProfileId> {
    platform_profiles()
        .iter()
        .copied()
        .find(|profile| platform_capability_snapshot_file_name(*profile) == raw)
}

pub fn render_platform_capability_snapshot(profile: ProfileId) -> Result<String, String> {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();
    let catalog = resolve_memory_capabilities(profile, &policy, &privacy)
        .map_err(|err| format!("failed to resolve platform capability catalog: {err}"))?;
    let snapshot = platform_capability_snapshot(&catalog);
    serde_json::to_string_pretty(&snapshot)
        .map_err(|err| format!("failed to render platform capability snapshot: {err}"))
}

pub fn run_cli<I>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    if args.first().is_some_and(|scope| scope == "memory") {
        return run_memory_cli(&args[1..]);
    }
    match args.as_slice() {
        [scope, command, flag, profile]
            if scope == "platform" && command == "capability-snapshot" && flag == "--profile" =>
        {
            let profile = parse_platform_profile_id(profile)
                .ok_or_else(|| format!("unsupported platform profile: {profile}"))?;
            render_platform_capability_snapshot(profile)
        }
        _ => {
            Err("usage: bm platform capability-snapshot --profile <profile-feature-id>".to_string())
        }
    }
}

pub fn render_capabilities(catalog: &MemoryCapabilityCatalog) -> serde_json::Result<String> {
    serde_json::to_string_pretty(&json!({
        "profile": catalog.profile.as_str(),
        "adapter": {
            "cli": visibility_json(catalog.adapter.cli.visible, catalog.adapter.cli.client_allowed, catalog.adapter.cli.server_allowed),
            "http": visibility_json(catalog.adapter.http.visible, catalog.adapter.http.client_allowed, catalog.adapter.http.server_allowed),
            "webhook": visibility_json(catalog.adapter.webhook.visible, catalog.adapter.webhook.client_allowed, catalog.adapter.webhook.server_allowed),
            "wss": visibility_json(catalog.adapter.wss.visible, catalog.adapter.wss.client_allowed, catalog.adapter.wss.server_allowed),
            "mqtt": visibility_json(catalog.adapter.mqtt.visible, catalog.adapter.mqtt.client_allowed, catalog.adapter.mqtt.server_allowed),
            "mcp": visibility_json(catalog.adapter.mcp.visible, catalog.adapter.mcp.client_allowed, catalog.adapter.mcp.server_allowed),
            "a2a": visibility_json(catalog.adapter.a2a.visible, catalog.adapter.a2a.client_allowed, catalog.adapter.a2a.server_allowed),
        },
        "entry": {
            "cli": visibility_json(catalog.entry.cli.visible, catalog.entry.cli.client_allowed, catalog.entry.cli.server_allowed),
            "http_server": visibility_json(catalog.entry.http_server.visible, catalog.entry.http_server.client_allowed, catalog.entry.http_server.server_allowed),
            "webhook_receiver": visibility_json(catalog.entry.webhook_receiver.visible, catalog.entry.webhook_receiver.client_allowed, catalog.entry.webhook_receiver.server_allowed),
            "webhook_sender": visibility_json(catalog.entry.webhook_sender.visible, catalog.entry.webhook_sender.client_allowed, catalog.entry.webhook_sender.server_allowed),
            "wss_client": visibility_json(catalog.entry.wss_client.visible, catalog.entry.wss_client.client_allowed, catalog.entry.wss_client.server_allowed),
            "wss_server": visibility_json(catalog.entry.wss_server.visible, catalog.entry.wss_server.client_allowed, catalog.entry.wss_server.server_allowed),
            "mqtt_client": visibility_json(catalog.entry.mqtt_client.visible, catalog.entry.mqtt_client.client_allowed, catalog.entry.mqtt_client.server_allowed),
            "mqtt_bridge": visibility_json(catalog.entry.mqtt_bridge.visible, catalog.entry.mqtt_bridge.client_allowed, catalog.entry.mqtt_bridge.server_allowed),
            "mcp_server": visibility_json(catalog.entry.mcp_server.visible, catalog.entry.mcp_server.client_allowed, catalog.entry.mcp_server.server_allowed),
            "a2a_bridge": visibility_json(catalog.entry.a2a_bridge.visible, catalog.entry.a2a_bridge.client_allowed, catalog.entry.a2a_bridge.server_allowed),
        },
        "lifecycle": {
            "recover": catalog.lifecycle.recover.visible,
            "maintain_full": catalog.lifecycle.maintain_full.visible,
            "maintain_lightweight": catalog.lifecycle.maintain_lightweight.visible,
            "operator_diagnosis": catalog.lifecycle.operator_diagnosis.visible,
            "export_snapshot": catalog.lifecycle.export_snapshot.visible,
            "import_snapshot": catalog.lifecycle.import_snapshot.visible,
            "replay_inspection": catalog.lifecycle.replay_inspection.visible,
        },
        "validation": {
            "compact_replay_fixture": catalog.validation.compact_replay_fixture.visible,
            "memory_harness": catalog.validation.memory_harness.visible,
            "full_replay_suite": catalog.validation.full_replay_suite.visible,
            "benchmark_gate": catalog.validation.benchmark_gate.visible,
            "proposal_preview": catalog.validation.proposal_preview.visible,
            "compact_proposal_sandbox": catalog.validation.compact_proposal_sandbox.visible,
            "full_proposal_sandbox": catalog.validation.full_proposal_sandbox.visible,
            "proposal_submission": catalog.validation.proposal_submission.visible,
        }
    }))
}

fn visibility_json(visible: bool, client_allowed: bool, server_allowed: bool) -> serde_json::Value {
    json!({
        "visible": visible,
        "client_allowed": client_allowed,
        "server_allowed": server_allowed,
    })
}

fn run_memory_cli(args: &[String]) -> Result<String, String> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| "usage: bm memory <command> [options]".to_string())?;
    let options = CliOptions::parse(rest)?;
    let entry = EntryRuntime::open(options.entry_config()).map_err(|err| err.to_string())?;
    let operation = command_operation(command)?;
    let adapter_command = options.adapter_command(command)?;
    let response = entry
        .handle(options.transport_context(operation), adapter_command)
        .map_err(|err| err.to_string())?;
    render_entry_response(response.adapter, options.output_path.as_deref())
}

fn command_operation(command: &str) -> Result<AdapterOperation, String> {
    command_specs()
        .iter()
        .find(|spec| spec.name == command)
        .map(|spec| spec.operation)
        .ok_or_else(|| format!("unsupported memory command: {command}"))
}

struct CliOptions {
    profile: ProfileId,
    store_backend: StoreBackendKind,
    store_path: Option<PathBuf>,
    agent: String,
    owner: String,
    channel: String,
    chat: String,
    query: String,
    limit: usize,
    max_len: usize,
    name: String,
    topic: String,
    title: String,
    summary: String,
    content: String,
    input_path: Option<PathBuf>,
    output_path: Option<PathBuf>,
    reason: String,
}

impl CliOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut profile = ProfileId::ServerLinuxDevFull;
        let mut store_backend = StoreBackendKind::InMemory;
        let mut store_path = None;
        let mut agent = "agent-main".to_string();
        let mut owner = "owner-default".to_string();
        let mut channel = "local".to_string();
        let mut chat = "chat-1".to_string();
        let mut query = String::new();
        let mut limit = 8usize;
        let mut max_len = 4096usize;
        let mut name = String::new();
        let mut topic = String::new();
        let mut title = String::new();
        let mut summary = String::new();
        let mut content = String::new();
        let mut input_path = None;
        let mut output_path = None;
        let mut reason = "cli close".to_string();
        let mut index = 0;
        while index < args.len() {
            let key = args[index].as_str();
            index += 1;
            match key {
                "--profile" => {
                    let raw = next_value(args, &mut index, key)?;
                    profile = parse_platform_profile_id(raw)
                        .ok_or_else(|| format!("unsupported platform profile: {raw}"))?;
                }
                "--store-file" => {
                    store_backend = StoreBackendKind::File;
                    store_path = Some(PathBuf::from(next_value(args, &mut index, key)?));
                }
                "--store-sqlite" => {
                    store_backend = StoreBackendKind::Sqlite;
                    store_path = Some(PathBuf::from(next_value(args, &mut index, key)?));
                }
                "--store-embedded" => {
                    store_backend = StoreBackendKind::Embedded;
                }
                "--agent" => agent = next_value(args, &mut index, key)?.to_string(),
                "--owner" => owner = next_value(args, &mut index, key)?.to_string(),
                "--channel" => channel = next_value(args, &mut index, key)?.to_string(),
                "--chat" | "--chat-id" => chat = next_value(args, &mut index, key)?.to_string(),
                "--query" => query = next_value(args, &mut index, key)?.to_string(),
                "--limit" => {
                    limit = next_value(args, &mut index, key)?
                        .parse()
                        .map_err(|_| "limit must be a positive integer".to_string())?;
                }
                "--max-len" => {
                    max_len = next_value(args, &mut index, key)?
                        .parse()
                        .map_err(|_| "max-len must be a positive integer".to_string())?;
                }
                "--name" => name = next_value(args, &mut index, key)?.to_string(),
                "--topic" => topic = next_value(args, &mut index, key)?.to_string(),
                "--title" => title = next_value(args, &mut index, key)?.to_string(),
                "--summary" => summary = next_value(args, &mut index, key)?.to_string(),
                "--content" => content = next_value(args, &mut index, key)?.to_string(),
                "--input" => input_path = Some(PathBuf::from(next_value(args, &mut index, key)?)),
                "--output" => output_path = Some(PathBuf::from(next_value(args, &mut index, key)?)),
                "--reason" => reason = next_value(args, &mut index, key)?.to_string(),
                other => return Err(format!("unsupported memory option: {other}")),
            }
        }
        Ok(Self {
            profile,
            store_backend,
            store_path,
            agent,
            owner,
            channel,
            chat,
            query,
            limit,
            max_len,
            name,
            topic,
            title,
            summary,
            content,
            input_path,
            output_path,
            reason,
        })
    }

    fn entry_config(&self) -> EntryRuntimeConfig {
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        EntryRuntimeConfig {
            profile: self.profile,
            identity: EntryIdentity {
                agent_id: self.agent.clone(),
                owner_id: self.owner.clone(),
            },
            scope: EntryScope {
                channel: self.channel.clone(),
                chat_id: self.chat.clone(),
            },
            store: EntryStoreConfig {
                backend: self.store_backend,
                data_path: self.store_path.clone(),
                fsync: true,
            },
            transports: EntryTransportConfig::all_disabled().with_cli(true),
            auth: EntryAuthConfig::disabled_for_local(),
            idempotency: EntryIdempotencyConfig { max_keys: 1024 },
            privacy: MemoryPrivacyPolicy::standard_private_boundary(),
            capability,
        }
    }

    fn transport_context(&self, operation: AdapterOperation) -> EntryTransportContext {
        EntryTransportContext {
            request_id: format!("cli-{operation:?}-{}", self.chat),
            transport: bm_adapter::TransportKind::Cli,
            mode: bm_adapter::TransportMode::InProcess,
            operation,
            source_id: "bm-cli".to_string(),
            source_kind: "local_cli".to_string(),
            idempotency_key: format!("cli-{operation:?}-{}-{}", self.chat, self.name),
            audit_id: format!("audit-cli-{operation:?}-{}", self.chat),
            auth: EntryAuthDecision::authenticated("local", "operator"),
        }
    }

    fn adapter_command(&self, command: &str) -> Result<AdapterCommand, String> {
        match command {
            "capabilities" => Ok(AdapterCommand::Capabilities),
            "write-procedural" => Ok(AdapterCommand::Write(MemoryWriteRequest::Procedural {
                writes: vec![RuntimeSkillWrite {
                    name: self.name.clone(),
                    topic: self.topic.clone(),
                    title: self.title.clone(),
                    summary: self.summary.clone(),
                    content: self.content.clone(),
                    citations: vec!["bm-cli".to_string()],
                    source_chat_id: Some(self.chat.clone()),
                    observed_at: 1_800_000_000,
                }],
                source: RuntimeSkillWriteSource::Manual,
            })),
            "recall" => Ok(AdapterCommand::Recall(MemoryRecallRequest {
                query: self.query.clone(),
                limit: self.limit,
            })),
            "project" => Ok(AdapterCommand::Project(MemoryProjectionRequest {
                user_query: self.query.clone(),
                system_max_len: self.max_len,
                recent_messages_limit: self.limit,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            })),
            "inspect" => Ok(AdapterCommand::Inspect(MemoryInspectionRequest {
                query: self.query.clone(),
                system_max_len: self.max_len,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            })),
            "replay" => Ok(AdapterCommand::Replay(MemoryReplayRequest {
                chat_id: self.chat.clone(),
                limit: self.limit,
            })),
            "export" => Ok(AdapterCommand::Export(MemoryExportRequest {
                chat_id: self.chat.clone(),
            })),
            "import" => {
                let path = self
                    .input_path
                    .as_ref()
                    .ok_or_else(|| "memory import requires --input <path>".to_string())?;
                let raw = std::fs::read_to_string(path)
                    .map_err(|err| format!("failed to read import snapshot: {err}"))?;
                let snapshot: ContinuitySnapshot = serde_json::from_str(&raw)
                    .map_err(|err| format!("failed to parse import snapshot: {err}"))?;
                Ok(AdapterCommand::Import(Box::new(MemoryImportRequest {
                    snapshot,
                    target_chat_id: self.chat.clone(),
                    mode: ContinuitySnapshotImportMode::FullRestore,
                })))
            }
            "close" => Ok(AdapterCommand::Close(bm_sdk::MemoryCloseRequest {
                reason: self.reason.clone(),
            })),
            other => Err(format!("unsupported memory command: {other}")),
        }
    }
}

fn next_value<'a>(args: &'a [String], index: &mut usize, key: &str) -> Result<&'a str, String> {
    let value = args
        .get(*index)
        .ok_or_else(|| format!("{key} requires a value"))?;
    *index += 1;
    Ok(value)
}

fn render_entry_response(
    response: AdapterResponse<AdapterSdkReport>,
    output_path: Option<&std::path::Path>,
) -> Result<String, String> {
    match response {
        AdapterResponse::Accepted { report, .. } => render_sdk_report(report, output_path),
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => serde_json::to_string_pretty(&json!({
            "status": "rejected",
            "error_key": format!("{error_key:?}"),
            "reason": reason,
        }))
        .map_err(|err| err.to_string()),
        AdapterResponse::Queued { queue, .. } => serde_json::to_string_pretty(&json!({
            "status": "queued",
            "queue": queue,
        }))
        .map_err(|err| err.to_string()),
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => serde_json::to_string_pretty(&json!({
            "status": "duplicated",
            "idempotency_key": idempotency_key,
        }))
        .map_err(|err| err.to_string()),
    }
}

fn render_sdk_report(
    report: AdapterSdkReport,
    output_path: Option<&std::path::Path>,
) -> Result<String, String> {
    let value = match report {
        AdapterSdkReport::Write(report) => json!({
            "status": "accepted",
            "operation": report.operation,
            "accepted": report.accepted,
            "changed": report.changed,
            "reason": report.reason,
        }),
        AdapterSdkReport::Recall(report) => json!({
            "status": "accepted",
            "query": report.query,
            "procedural_hits": report.procedural_hits.iter().map(|hit| json!({
                "name": hit.record.name,
                "title": hit.record.title,
                "score": hit.score,
            })).collect::<Vec<_>>(),
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::Project(report) => json!({
            "status": "accepted",
            "system_memory_block": report.system_memory_block,
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::Inspect(report) => json!({
            "status": "accepted",
            "safe_actions_available": report.operator_action_report.safe_actions_available,
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::Replay(report) => json!({
            "status": "accepted",
            "chat_id": report.chat_id,
            "turns": report.inspection.total_turns,
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::Export(report) => {
            if let Some(path) = output_path {
                let rendered = serde_json::to_string_pretty(&report.snapshot)
                    .map_err(|err| err.to_string())?;
                std::fs::write(path, rendered)
                    .map_err(|err| format!("failed to write export snapshot: {err}"))?;
            }
            json!({
                "status": "accepted",
                "chat_id": report.snapshot.chat_id,
                "snapshot_version": report.snapshot.version,
                "lifecycle": report.lifecycle_report.result_summary,
            })
        }
        AdapterSdkReport::Import(report) => json!({
            "status": "accepted",
            "long_term_imported": report.outcome.long_term_imported,
            "summary_restored": report.outcome.summary_restored,
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::Capabilities(catalog) => {
            let rendered = render_capabilities(&catalog).map_err(|err| err.to_string())?;
            return Ok(rendered);
        }
        AdapterSdkReport::Close(report) => json!({
            "status": "accepted",
            "operation": "close",
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::Recover(report) => json!({
            "status": "accepted",
            "action": format!("{:?}", report.report.action),
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::MaintainUnsupported(reason) => json!({
            "status": "rejected",
            "reason": reason,
        }),
    };
    serde_json::to_string_pretty(&value).map_err(|err| err.to_string())
}
