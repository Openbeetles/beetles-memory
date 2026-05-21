//! CLI adapter for Beetle Memory.

use bm_adapter::AdapterOperation;
use bm_sdk::{
    platform_capability_snapshot, platform_capability_snapshot_file_name,
    resolve_memory_capabilities, MemoryCapabilityCatalog, MemoryCapabilityPolicy,
    MemoryPrivacyPolicy, ProfileId,
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
