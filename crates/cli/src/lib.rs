//! CLI adapter for Beetle Memory.

use std::path::PathBuf;

pub mod agent_rules;

use agent_rules::{render_agent_rules_export, AgentRulesExportRequest, AgentRulesTarget};
use bm_adapter::{
    decode_json_adapter_command, AdapterCommand, AdapterJsonCommandOptions,
    AdapterMutationReliability, AdapterOperation, AdapterRequestIdentityOwner, AdapterResponse,
    AdapterSdkReport,
};
use bm_entry::{
    EntryAuthConfig, EntryConsoleRuntimeSkillEdit, EntryConsoleSkillSetEnabled,
    EntryIdempotencyConfig, EntryIdentity, EntryLocalTransport, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    platform_capability_snapshot, platform_capability_snapshot_file_name,
    resolve_memory_capabilities, GovernedRuntimeSkillWriteInput, LongTermMemoryKind,
    LongTermMemoryQuery, MemoryCapabilityCatalog, MemoryCapabilityPolicy,
    MemoryGovernancePolicyMutation, MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration,
    MemoryInspectionRequest, MemoryLongTermControlView, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest,
    MemoryLongTermTarget, MemoryPrivacyClass, MemoryPrivacyPolicy, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryReplayRequest, MemoryTranscriptAttrWriteRequest, MemoryWriteRequest,
    PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillCreationRef,
    RuntimeSkillOwnerLocator, RuntimeSkillOwningScope, RuntimeSkillWrite, RuntimeSkillWriteSource,
    StoreBackendConfig, StoreBackendKind, TranscriptAttrEnvelope,
};
use serde::Deserialize;
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
        name: "write-procedural",
        usage: "bm memory write-procedural --idempotency-key <stable-non-sensitive-key> --name <name> --content <content> --runtime-skill-subject <subject-id>|--runtime-skill-shared-program --replay-candidate-ref <safe-ref> --verification-receipt-digest <sha256> --runtime-skill-privacy <public-runtime|shared-with-subject>",
        operation: AdapterOperation::Write,
    },
    CommandSpec {
        name: "finalize-turn",
        usage: "bm memory finalize-turn --input <request.json>",
        operation: AdapterOperation::FinalizeTurn,
    },
    CommandSpec {
        name: "long-term-list",
        usage: "bm memory long-term-list --query <query> --limit <n>",
        operation: AdapterOperation::LongTermList,
    },
    CommandSpec {
        name: "long-term-detail",
        usage: "bm memory long-term-detail --record-id <id>",
        operation: AdapterOperation::LongTermDetail,
    },
    CommandSpec {
        name: "long-term-delete",
        usage: "bm memory long-term-delete --idempotency-key <stable-non-sensitive-key> --record-id <id> --reason <reason>",
        operation: AdapterOperation::LongTermMutate,
    },
    CommandSpec {
        name: "long-term-policy-suppress",
        usage: "bm memory long-term-policy-suppress --topic <pattern> --reason <reason>",
        operation: AdapterOperation::LongTermPolicy,
    },
    CommandSpec {
        name: "transcript-attr-write",
        usage: "bm memory transcript-attr-write --input <request.json> --reason <reason>",
        operation: AdapterOperation::TranscriptAttrWrite,
    },
    CommandSpec {
        name: "skill-list",
        usage: "bm memory skill-list (--runtime-skill-subject <subject-id>|--runtime-skill-shared-program) --query <query>",
        operation: AdapterOperation::Inspect,
    },
    CommandSpec {
        name: "skill-show",
        usage: "bm memory skill-show (--runtime-skill-subject <subject-id>|--runtime-skill-shared-program) --runtime-skill-owner-id <owner-id> --runtime-skill-owner-revision <n>",
        operation: AdapterOperation::Inspect,
    },
    CommandSpec {
        name: "skill-edit",
        usage: "bm memory skill-edit (--runtime-skill-subject <subject-id>|--runtime-skill-shared-program) --runtime-skill-owner-id <owner-id> --runtime-skill-owner-revision <n> --title <title> --topic <topic> --content <content>",
        operation: AdapterOperation::Write,
    },
    CommandSpec {
        name: "skill-enable",
        usage: "bm memory skill-enable (--runtime-skill-subject <subject-id>|--runtime-skill-shared-program) --runtime-skill-owner-id <owner-id> --runtime-skill-owner-revision <n>",
        operation: AdapterOperation::Write,
    },
    CommandSpec {
        name: "skill-disable",
        usage: "bm memory skill-disable (--runtime-skill-subject <subject-id>|--runtime-skill-shared-program) --runtime-skill-owner-id <owner-id> --runtime-skill-owner-revision <n>",
        operation: AdapterOperation::Write,
    },
    CommandSpec {
        name: "skill-retire",
        usage: "bm memory skill-retire (--runtime-skill-subject <subject-id>|--runtime-skill-shared-program) --runtime-skill-owner-id <owner-id> --runtime-skill-owner-revision <n>",
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
        ProfileId::DesktopMacosStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopMacosDevFull,
        ProfileId::DesktopLinuxEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
        ProfileId::DesktopWindowsDevFull,
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
    if args.first().is_some_and(|scope| scope == "agent-rules") {
        return run_agent_rules_cli(&args[1..]);
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
            Err("usage: bm memory <command> [options] | bm platform capability-snapshot --profile <profile-feature-id> | bm agent-rules export --target <target> --gateway-url <url> --mcp-url <url>".to_string())
        }
    }
}

pub fn render_capabilities(catalog: &MemoryCapabilityCatalog) -> serde_json::Result<String> {
    serde_json::to_string_pretty(&json!({
        "profile": catalog.profile.as_str(),
        "adapter": {
            "cli": visibility_json(catalog.adapter.cli.visible, catalog.adapter.cli.client_allowed, catalog.adapter.cli.server_allowed),
            "http": visibility_json(catalog.adapter.http.visible, catalog.adapter.http.client_allowed, catalog.adapter.http.server_allowed),
            "wss": visibility_json(catalog.adapter.wss.visible, catalog.adapter.wss.client_allowed, catalog.adapter.wss.server_allowed),
            "mcp": visibility_json(catalog.adapter.mcp.visible, catalog.adapter.mcp.client_allowed, catalog.adapter.mcp.server_allowed),
            "a2a": visibility_json(catalog.adapter.a2a.visible, catalog.adapter.a2a.client_allowed, catalog.adapter.a2a.server_allowed),
        },
        "entry": {
            "cli": visibility_json(catalog.entry.cli.visible, catalog.entry.cli.client_allowed, catalog.entry.cli.server_allowed),
            "http_server": visibility_json(catalog.entry.http_server.visible, catalog.entry.http_server.client_allowed, catalog.entry.http_server.server_allowed),
            "wss_client": visibility_json(catalog.entry.wss_client.visible, catalog.entry.wss_client.client_allowed, catalog.entry.wss_client.server_allowed),
            "wss_server": visibility_json(catalog.entry.wss_server.visible, catalog.entry.wss_server.client_allowed, catalog.entry.wss_server.server_allowed),
            "mcp_server": visibility_json(catalog.entry.mcp_server.visible, catalog.entry.mcp_server.client_allowed, catalog.entry.mcp_server.server_allowed),
            "a2a_bridge": visibility_json(catalog.entry.a2a_bridge.visible, catalog.entry.a2a_bridge.client_allowed, catalog.entry.a2a_bridge.server_allowed),
            "llm_gateway_server": visibility_json(catalog.entry.llm_gateway_server.visible, catalog.entry.llm_gateway_server.client_allowed, catalog.entry.llm_gateway_server.server_allowed),
        },
        "lifecycle": {
            "recover": catalog.lifecycle.recover.visible,
            "maintain_full": catalog.lifecycle.maintain_full.visible,
            "maintain_lightweight": catalog.lifecycle.maintain_lightweight.visible,
            "operator_diagnosis": catalog.lifecycle.operator_diagnosis.visible,
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
    let operation = command_operation(command)?;
    let options = CliOptions::parse(rest)?;
    if is_skill_command(command) && !options.name.trim().is_empty() {
        return Err(
            "--name is not accepted by runtime skill management; use the typed owner locator"
                .to_string(),
        );
    }
    let entry = EntryRuntime::open(options.entry_config()).map_err(|err| err.to_string())?;
    if is_skill_command(command) {
        return run_skill_cli(&entry, command, &options);
    }
    if operation.mutation_reliability() == AdapterMutationReliability::DurableStoreReceipt
        && options.idempotency_key.is_none()
    {
        return Err(
            "durable mutation command requires --idempotency-key <stable-non-sensitive-key>"
                .to_string(),
        );
    }
    let adapter_command = options.adapter_command(command)?;
    let response = entry
        .handle(
            options.transport_context(&entry, operation)?,
            adapter_command,
        )
        .map_err(|err| err.to_string())?;
    render_entry_response(response.adapter)
}

fn run_agent_rules_cli(args: &[String]) -> Result<String, String> {
    match args.split_first() {
        Some((command, rest)) if command == "export" => {
            let request = AgentRulesCliOptions::parse(rest)?.export_request()?;
            render_agent_rules_export(&request)
        }
        Some((command, _)) => Err(format!("unsupported agent-rules command: {command}")),
        None => Err(
            "usage: bm agent-rules export --target <target> --gateway-url <url> --mcp-url <url>"
                .to_string(),
        ),
    }
}

struct AgentRulesCliOptions {
    target: Option<AgentRulesTarget>,
    gateway_url: String,
    mcp_url: String,
}

impl AgentRulesCliOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut target = None;
        let mut gateway_url = String::new();
        let mut mcp_url = String::new();
        let mut index = 0;
        while index < args.len() {
            let key = args[index].as_str();
            index += 1;
            match key {
                "--target" => {
                    let raw = next_value(args, &mut index, key)?;
                    target = Some(AgentRulesTarget::parse(raw)?);
                }
                "--gateway-url" => gateway_url = next_value(args, &mut index, key)?.to_string(),
                "--mcp-url" => mcp_url = next_value(args, &mut index, key)?.to_string(),
                other => return Err(format!("unsupported agent-rules option: {other}")),
            }
        }
        Ok(Self {
            target,
            gateway_url,
            mcp_url,
        })
    }

    fn export_request(self) -> Result<AgentRulesExportRequest, String> {
        Ok(AgentRulesExportRequest {
            target: self
                .target
                .ok_or_else(|| "--target is required".to_string())?,
            gateway_url: required_value(&self.gateway_url, "--gateway-url")?.to_string(),
            mcp_url: required_value(&self.mcp_url, "--mcp-url")?.to_string(),
        })
    }
}

fn is_skill_command(command: &str) -> bool {
    matches!(
        command,
        "skill-list"
            | "skill-show"
            | "skill-edit"
            | "skill-enable"
            | "skill-disable"
            | "skill-retire"
    )
}

fn run_skill_cli(
    entry: &EntryRuntime,
    command: &str,
    options: &CliOptions,
) -> Result<String, String> {
    let value = match command {
        "skill-list" => {
            let report = entry
                .console_skills_in_scope(
                    options.runtime_skill_scope()?,
                    non_empty_string(&options.query),
                )
                .map_err(|err| err.to_string())?;
            json!({
                "status": "accepted",
                "skills": report,
            })
        }
        "skill-show" => {
            let locator = options.runtime_skill_locator()?;
            let skill = entry
                .console_skill_detail(locator)
                .map_err(|err| err.to_string())?;
            json!({
                "status": "accepted",
                "skill": skill,
            })
        }
        "skill-edit" => {
            let content = options.skill_content()?;
            let payload = EntryConsoleRuntimeSkillEdit {
                locator: options.runtime_skill_locator()?,
                title: required_value(&options.title, "--title")?.to_string(),
                topic: required_value(&options.topic, "--topic")?.to_string(),
                summary: required_value(&options.summary, "--summary")?.to_string(),
                procedure: content,
                edit_reason: Some("cli_runtime_skill_edit".to_string()),
            };
            let mutation = entry
                .console_edit_runtime_skill(payload)
                .map_err(|err| err.to_string())?;
            json!({
                "status": "accepted",
                "mutation": mutation,
            })
        }
        "skill-enable" | "skill-disable" => {
            let mutation = entry
                .console_set_skill_enabled(EntryConsoleSkillSetEnabled {
                    locator: options.runtime_skill_locator()?,
                    enabled: command == "skill-enable",
                })
                .map_err(|err| err.to_string())?;
            json!({
                "status": "accepted",
                "mutation": mutation,
            })
        }
        "skill-retire" => {
            let mutation = entry
                .console_retire_skill(options.runtime_skill_locator()?)
                .map_err(|err| err.to_string())?;
            json!({
                "status": "accepted",
                "mutation": mutation,
            })
        }
        other => return Err(format!("unsupported memory command: {other}")),
    };
    serde_json::to_string_pretty(&value).map_err(|err| err.to_string())
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
    record_id: String,
    input_path: Option<PathBuf>,
    reason: String,
    reason_provided: bool,
    runtime_skill_owning_scope: Option<RuntimeSkillOwningScope>,
    runtime_skill_owner_id: String,
    runtime_skill_owner_revision: Option<u64>,
    replay_candidate_ref: String,
    verification_receipt_digest: String,
    runtime_skill_privacy: Option<MemoryPrivacyClass>,
    idempotency_key: Option<String>,
}

#[derive(Deserialize)]
struct CliTranscriptAttrWritePayload {
    memory_space_id: String,
    channel_id: String,
    conversation_id: String,
    #[serde(default)]
    attrs: Vec<TranscriptAttrEnvelope>,
    #[serde(default)]
    idempotency_key: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

impl CliOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut profile = None;
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
        let mut record_id = String::new();
        let mut input_path = None;
        let mut reason = "cli close".to_string();
        let mut reason_provided = false;
        let mut runtime_skill_owning_scope = None;
        let mut runtime_skill_owner_id = String::new();
        let mut runtime_skill_owner_revision = None;
        let mut replay_candidate_ref = String::new();
        let mut verification_receipt_digest = String::new();
        let mut runtime_skill_privacy = None;
        let mut idempotency_key = None;
        let mut index = 0;
        while index < args.len() {
            let key = args[index].as_str();
            index += 1;
            match key {
                "--profile" => {
                    let raw = next_value(args, &mut index, key)?;
                    profile = Some(
                        parse_platform_profile_id(raw)
                            .ok_or_else(|| format!("unsupported platform profile: {raw}"))?,
                    );
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
                "--runtime-skill-subject" => {
                    if runtime_skill_owning_scope.is_some() {
                        return Err("runtime skill owning scope may be specified once".to_string());
                    }
                    runtime_skill_owning_scope = Some(RuntimeSkillOwningScope::Subject {
                        mounted_subject_id: next_value(args, &mut index, key)?.to_string(),
                    });
                }
                "--runtime-skill-shared-program" => {
                    if runtime_skill_owning_scope.is_some() {
                        return Err("runtime skill owning scope may be specified once".to_string());
                    }
                    runtime_skill_owning_scope = Some(RuntimeSkillOwningScope::SharedProgram);
                }
                "--runtime-skill-owner-id" => {
                    runtime_skill_owner_id = next_value(args, &mut index, key)?.to_string()
                }
                "--runtime-skill-owner-revision" => {
                    runtime_skill_owner_revision =
                        Some(next_value(args, &mut index, key)?.parse().map_err(|_| {
                            "runtime skill owner revision must be a positive integer".to_string()
                        })?)
                }
                "--replay-candidate-ref" => {
                    replay_candidate_ref = next_value(args, &mut index, key)?.to_string()
                }
                "--verification-receipt-digest" => {
                    verification_receipt_digest = next_value(args, &mut index, key)?.to_string()
                }
                "--runtime-skill-privacy" => {
                    runtime_skill_privacy = Some(match next_value(args, &mut index, key)? {
                        "public-runtime" => MemoryPrivacyClass::PublicRuntime,
                        "shared-with-subject" => MemoryPrivacyClass::SharedWithSubject,
                        other => {
                            return Err(format!("unsupported runtime skill privacy class: {other}"))
                        }
                    });
                }
                "--record-id" => record_id = next_value(args, &mut index, key)?.to_string(),
                "--idempotency-key" => {
                    idempotency_key =
                        Some(required_value(next_value(args, &mut index, key)?, key)?.to_string())
                }
                "--input" => input_path = Some(PathBuf::from(next_value(args, &mut index, key)?)),
                "--reason" => {
                    reason = next_value(args, &mut index, key)?.to_string();
                    reason_provided = true;
                }
                other => return Err(format!("unsupported memory option: {other}")),
            }
        }
        let profile = profile.ok_or_else(|| {
            "memory commands require an explicit --profile deployment contract".to_string()
        })?;
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
            record_id,
            input_path,
            reason,
            reason_provided,
            runtime_skill_owning_scope,
            runtime_skill_owner_id,
            runtime_skill_owner_revision,
            replay_candidate_ref,
            verification_receipt_digest,
            runtime_skill_privacy,
            idempotency_key,
        })
    }

    fn entry_config(&self) -> EntryRuntimeConfig {
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        EntryRuntimeConfig {
            identity: EntryIdentity {
                agent_id: self.agent.clone(),
                owner_id: self.owner.clone(),
            },
            scope: EntryScope {
                channel: self.channel.clone(),
                chat_id: self.chat.clone(),
            },
            store: StoreBackendConfig::for_backend(
                self.store_backend,
                self.store_path.clone(),
                self.profile,
            )
            .expect("validated CLI store configuration"),
            transports: EntryTransportConfig::all_disabled().with_cli(true),
            auth: EntryAuthConfig::disabled_for_local(),
            idempotency: EntryIdempotencyConfig { max_keys: 1024 },
            privacy: MemoryPrivacyPolicy::standard_private_boundary(),
            capability,
        }
    }

    fn runtime_skill_scope(&self) -> Result<RuntimeSkillOwningScope, String> {
        self.runtime_skill_owning_scope
            .clone()
            .ok_or_else(|| "runtime skill command requires an explicit owning scope".to_string())
    }

    fn runtime_skill_locator(&self) -> Result<RuntimeSkillOwnerLocator, String> {
        let owner_id =
            required_value(&self.runtime_skill_owner_id, "--runtime-skill-owner-id")?.to_string();
        let owner_revision = self.runtime_skill_owner_revision.ok_or_else(|| {
            "--runtime-skill-owner-revision is required for this command".to_string()
        })?;
        RuntimeSkillOwnerLocator::try_new(self.runtime_skill_scope()?, owner_id, owner_revision)
            .map_err(|error| error.to_string())
    }

    fn transport_context(
        &self,
        runtime: &EntryRuntime,
        operation: AdapterOperation,
    ) -> Result<EntryTransportContext, String> {
        let identity =
            AdapterRequestIdentityOwner::new(bm_adapter::TransportKind::Cli, "bm-cli", "operator")
                .issue(self.idempotency_key.as_deref())
                .map_err(|error| error.to_string())?;
        Ok(EntryTransportContext::new(
            identity.request_id,
            bm_adapter::TransportKind::Cli,
            bm_adapter::TransportMode::InProcess,
            operation,
            "bm-cli",
            "local_cli",
            identity.mutation_operation_id.unwrap_or_default(),
            identity.audit_id,
            runtime.authenticate_local_transport(EntryLocalTransport::InProcess, "operator"),
        ))
    }

    fn adapter_command(&self, command: &str) -> Result<AdapterCommand, String> {
        match command {
            "capabilities" => Ok(AdapterCommand::Capabilities),
            "write-procedural" => Ok(AdapterCommand::Write(MemoryWriteRequest::Procedural {
                writes: vec![GovernedRuntimeSkillWriteInput {
                    write: RuntimeSkillWrite {
                        name: self.name.clone(),
                        topic: self.topic.clone(),
                        title: self.title.clone(),
                        summary: self.summary.clone(),
                        content: self.content.clone(),
                        citations: vec!["bm-cli".to_string()],
                        source_chat_id: Some(self.chat.clone()),
                        observed_at: 1_800_000_000,
                    },
                    creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                        candidate_ref: required_value(
                            &self.replay_candidate_ref,
                            "--replay-candidate-ref",
                        )?
                        .to_string(),
                        verification_receipt_digest: required_value(
                            &self.verification_receipt_digest,
                            "--verification-receipt-digest",
                        )?
                        .to_string(),
                    },
                    privacy_class: self.runtime_skill_privacy.ok_or_else(|| {
                        "write-procedural requires --runtime-skill-privacy".to_string()
                    })?,
                }],
                owning_scope: self.runtime_skill_owning_scope.clone().ok_or_else(|| {
                    "write-procedural requires an explicit runtime skill owning scope".to_string()
                })?,
                source: RuntimeSkillWriteSource::Manual,
            })),
            "finalize-turn" => {
                let path = self
                    .input_path
                    .as_ref()
                    .ok_or_else(|| "finalize-turn requires --input <request.json>".to_string())?;
                let raw = std::fs::read_to_string(path)
                    .map_err(|err| format!("failed to read finalize turn request: {err}"))?;
                decode_json_adapter_command(
                    AdapterOperation::FinalizeTurn,
                    &raw,
                    &AdapterJsonCommandOptions::new("bm-cli")
                        .with_default_source_chat_id(self.chat.clone()),
                )
                .map_err(|err| err.to_string())
            }
            "long-term-list" => Ok(AdapterCommand::LongTermList(MemoryLongTermListRequest {
                query: LongTermMemoryQuery {
                    topic: non_empty_string(&self.query),
                    limit: self.limit,
                    ..LongTermMemoryQuery::default()
                },
                cursor: None,
                limit: self.limit,
                view: MemoryLongTermControlView::HostUi,
            })),
            "long-term-detail" => Ok(AdapterCommand::LongTermDetail(
                bm_sdk::MemoryLongTermDetailRequest {
                    target: MemoryLongTermTarget::RecordId(
                        required_value(&self.record_id, "--record-id")?.to_string(),
                    ),
                    view: MemoryLongTermControlView::HostUi,
                },
            )),
            "long-term-delete" => Ok(AdapterCommand::LongTermMutate(Box::new(
                MemoryLongTermMutationRequest {
                    operation: MemoryLongTermMutation::Delete {
                        target: MemoryLongTermTarget::RecordId(
                            required_value(&self.record_id, "--record-id")?.to_string(),
                        ),
                    },
                    reason: self.required_reason()?,
                    dry_run: false,
                    mode_input: RuntimeLifecycleModeInput::default(),
                },
            ))),
            "long-term-policy-suppress" => Ok(AdapterCommand::LongTermPolicy(
                MemoryLongTermPolicyRequest {
                    operation: MemoryGovernancePolicyMutation::Suppress {
                        selector: MemoryGovernanceSelector {
                            memory_space_id: None,
                            subject_id: None,
                            kind: Some(LongTermMemoryKind::Preference),
                            topic_pattern: Some(
                                required_value(&self.topic, "--topic")?.to_string(),
                            ),
                            source_chat_id: None,
                            source_scope: None,
                        },
                        duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
                    },
                    reason: self.required_reason()?,
                    dry_run: false,
                    mode_input: RuntimeLifecycleModeInput::default(),
                },
            )),
            "transcript-attr-write" => {
                self.required_reason()?;
                let path = self.input_path.as_ref().ok_or_else(|| {
                    "transcript-attr-write requires --input <request.json>".to_string()
                })?;
                let raw = std::fs::read_to_string(path)
                    .map_err(|err| format!("failed to read transcript attr request: {err}"))?;
                let payload: CliTranscriptAttrWritePayload = serde_json::from_str(&raw)
                    .map_err(|err| format!("failed to parse transcript attr request: {err}"))?;
                Ok(AdapterCommand::TranscriptAttrWrite(
                    MemoryTranscriptAttrWriteRequest {
                        memory_space_id: payload.memory_space_id,
                        channel_id: payload.channel_id,
                        conversation_id: payload.conversation_id,
                        attrs: payload.attrs,
                        idempotency_key: payload.idempotency_key,
                        dry_run: payload.dry_run.unwrap_or(false),
                    },
                ))
            }
            "recall" => Ok(AdapterCommand::Recall(MemoryRecallRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                query: self.query.clone(),
                limit: self.limit,
                tool_registry_refs: Vec::new(),
            })),
            "project" => Ok(AdapterCommand::Project(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: self.query.clone(),
                system_max_len: self.max_len,
                recent_messages_limit: self.limit,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
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
            "close" => Ok(AdapterCommand::Close(bm_sdk::MemoryCloseRequest {
                reason: self.reason.clone(),
            })),
            other => Err(format!("unsupported memory command: {other}")),
        }
    }

    fn skill_content(&self) -> Result<String, String> {
        if !self.content.trim().is_empty() {
            return Ok(self.content.clone());
        }
        let path = self.input_path.as_ref().ok_or_else(|| {
            "skill content requires --content <content> or --input <path>".to_string()
        })?;
        std::fs::read_to_string(path).map_err(|err| format!("failed to read skill content: {err}"))
    }

    fn required_reason(&self) -> Result<String, String> {
        if !self.reason_provided {
            return Err("--reason is required".to_string());
        }
        Ok(required_value(&self.reason, "--reason")?.to_string())
    }
}

fn next_value<'a>(args: &'a [String], index: &mut usize, key: &str) -> Result<&'a str, String> {
    let value = args
        .get(*index)
        .ok_or_else(|| format!("{key} requires a value"))?;
    *index += 1;
    Ok(value)
}

fn required_value<'a>(value: &'a str, name: &str) -> Result<&'a str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{name} is required"));
    }
    Ok(trimmed)
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn render_entry_response(response: AdapterResponse<AdapterSdkReport>) -> Result<String, String> {
    match response {
        AdapterResponse::Accepted {
            report, receipt, ..
        } => {
            let rendered = render_sdk_report(report)?;
            let mut value: serde_json::Value =
                serde_json::from_str(&rendered).map_err(|error| error.to_string())?;
            if let Some(receipt) = receipt {
                value["receipt"] =
                    serde_json::to_value(receipt).map_err(|error| error.to_string())?;
            }
            serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
        }
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
        AdapterResponse::Replayed {
            mutation_operation_id,
            receipt,
            ..
        } => serde_json::to_string_pretty(&json!({
            "status": "replayed",
            "mutation_operation_id": mutation_operation_id,
            "receipt": receipt,
        }))
        .map_err(|err| err.to_string()),
    }
}

fn render_sdk_report(report: AdapterSdkReport) -> Result<String, String> {
    if let Some(governed) = report.governed_safe_report() {
        return serde_json::to_string_pretty(&json!({
            "status": "accepted",
            "result": governed,
        }))
        .map_err(|error| error.to_string());
    }
    let value = match report {
        AdapterSdkReport::Write(report) => json!({
            "status": "accepted",
            "operation": report.operation,
            "accepted": report.accepted,
            "changed": report.changed,
            "reason": report.reason,
        }),
        AdapterSdkReport::FinalizeTurn(report) => json!({
            "status": "accepted",
            "operation": "finalize_turn",
            "result": report,
        }),
        AdapterSdkReport::Recall(_) | AdapterSdkReport::Project(_) => {
            unreachable!("governed DTO handled above")
        }
        AdapterSdkReport::Maintain(report) => json!({
            "status": "accepted",
            "long_term_refresh_enqueued": report.long_term_refresh_enqueued,
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
        AdapterSdkReport::LongTermList(report) => json!({
            "status": "accepted",
            "records": report.records,
            "total_visible": report.total_visible,
            "next_cursor": report.next_cursor,
        }),
        AdapterSdkReport::LongTermDetail(report) => json!({
            "status": "accepted",
            "record": report.record,
            "revisions": report.revisions,
            "tombstone": report.tombstone,
            "transcript_refs": report.transcript_refs,
        }),
        AdapterSdkReport::LongTermMutate(report) => json!({
            "status": "accepted",
            "accepted": report.accepted,
            "operation": report.operation,
            "affected_records": report.affected_records,
            "tombstones": report.tombstones,
            "transcript_refs": report.transcript_refs,
            "policy_decision": report.policy_decision,
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::LongTermPolicy(report) => json!({
            "status": "accepted",
            "accepted": report.accepted,
            "operation": report.operation,
            "policy_id": report.policy_id,
            "affected_future_writes": report.affected_future_writes,
            "policy_decision": report.policy_decision,
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::TranscriptAttrWrite(report) => json!({
            "status": "accepted",
            "memory_space_id": report.key.memory_space_id,
            "channel_id": report.key.channel_id,
            "conversation_id": report.key.conversation_id,
            "accepted_attrs": report.accepted_attrs,
            "rejected_attrs": report.rejected_attrs,
            "redactions_preview": report.redactions_preview,
            "profile_budget_applied": report.profile_budget_applied,
            "audit_event_id": report.audit_event_id,
            "dry_run": report.dry_run,
            "lifecycle": report.lifecycle_report.result_summary,
        }),
        AdapterSdkReport::Capabilities(report) => json!({
            "status": "accepted",
            "profile": report.profile.as_str(),
            "capabilities": report.capabilities,
            "sdk_mutation_inventory": report.sdk_mutation_inventory,
        }),
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
    };
    serde_json::to_string_pretty(&value).map_err(|err| err.to_string())
}
