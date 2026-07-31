//! P8.5 trusted-execution 逐 gate 工程证据合同。
//!
//! 本模块只拥有 exact command registry、父进程观察 schema 与 fail-closed validator。
//! fixture receipt 永远不能升级为 trust；Linux production authority 只能由本模块的
//! opaque `VerifiedP8EngineeringGateSet` 从真实 wait/reap + 双 EOF 闭包中生成。

use std::collections::{BTreeMap, BTreeSet};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

use super::super::source_release::{P8HarnessSourceInputManifestV1, P8HarnessSourceInputRef};
use super::super::{
    domain_separated_sha256, has_typed_sha256_prefix, P8QualityContractFailure, P8QualityDigest,
    P8QualityId,
};

#[cfg(target_os = "linux")]
mod execution;
#[cfg(target_os = "linux")]
pub(crate) use execution::{execute_trusted_gate_set, VerifiedP8EngineeringGateSet};
#[cfg(target_os = "linux")]
pub(in crate::p8_quality::trusted_execution) fn consume_verified_gate_set(
    verified: VerifiedP8EngineeringGateSet,
) -> (
    P8EngineeringGateReceiptV1,
    Vec<super::P8SealedProcessReceiptV1>,
    super::supervisor_session::P8TrustedSupervisorInputs,
) {
    verified.into_parts()
}

const P8_ENGINEERING_GATE_COMMAND_RECEIPT_SCHEMA: &str =
    "beetle-memory.p8.quality-engineering-gate-command-receipt.v1";
const P8_ENGINEERING_GATE_RECEIPT_SCHEMA: &str =
    "beetle-memory.p8.quality-engineering-gate-receipt.v1";

macro_rules! p8_engineering_gate_ref {
    ($name:ident, $prefix:literal, $domain:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub(crate) struct $name(String);

        impl $name {
            fn derive(value: &impl Serialize) -> Self {
                let bytes = serde_json::to_vec(value)
                    .expect("P8 engineering gate identity serialization must be infallible");
                Self(format!(
                    "{}{}",
                    $prefix,
                    domain_separated_sha256($domain, &[bytes.as_slice()])
                ))
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                if has_typed_sha256_prefix(&value, $prefix) {
                    Ok(Self(value))
                } else {
                    Err(D::Error::custom(concat!(
                        stringify!($name),
                        " has an invalid domain or digest"
                    )))
                }
            }
        }
    };
}

p8_engineering_gate_ref!(
    P8EngineeringGateCommandReceiptRef,
    "p8_engineering_gate_command_receipt:sha256:",
    "p8_engineering_gate_command_receipt_v1"
);
p8_engineering_gate_ref!(
    P8EngineeringGateReceiptRef,
    "p8_engineering_gate_receipt:sha256:",
    "p8_engineering_gate_receipt_v1"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8EngineeringGateIdV1 {
    Format,
    UnitTests,
    Clippy,
    WorkspaceCheck,
}

impl P8EngineeringGateIdV1 {
    pub(crate) const ALL: [Self; 4] = [
        Self::Format,
        Self::UnitTests,
        Self::Clippy,
        Self::WorkspaceCheck,
    ];

    pub(crate) fn exact_argv(self) -> Vec<String> {
        let argv: &[&str] = match self {
            Self::Format => &["cargo-fmt", "--all", "--", "--check"],
            Self::UnitTests => &[
                "cargo",
                "test",
                "-p",
                "bm-replay",
                "--lib",
                "--locked",
                "--no-default-features",
            ],
            Self::Clippy => &[
                "cargo-clippy",
                "-p",
                "bm-replay",
                "--all-targets",
                "--locked",
                "--no-default-features",
                "--",
                "-D",
                "warnings",
            ],
            Self::WorkspaceCheck => &[
                "cargo",
                "check",
                "--workspace",
                "--exclude",
                "bm-desktop",
                "--locked",
            ],
        };
        argv.iter().map(|value| (*value).to_string()).collect()
    }

    fn launcher_tool(self) -> P8EngineeringToolRoleV1 {
        match self {
            Self::Format => P8EngineeringToolRoleV1::CargoFmt,
            Self::UnitTests | Self::WorkspaceCheck => P8EngineeringToolRoleV1::Cargo,
            Self::Clippy => P8EngineeringToolRoleV1::CargoClippy,
        }
    }

    pub(crate) const fn schema_name(self) -> &'static str {
        match self {
            Self::Format => "format",
            Self::UnitTests => "unit-tests",
            Self::Clippy => "clippy",
            Self::WorkspaceCheck => "workspace-check",
        }
    }

    fn required_tools(self) -> Vec<P8EngineeringToolRoleV1> {
        match self {
            Self::Format => vec![
                P8EngineeringToolRoleV1::Cargo,
                P8EngineeringToolRoleV1::CargoFmt,
                P8EngineeringToolRoleV1::Rustfmt,
            ],
            Self::UnitTests => vec![
                P8EngineeringToolRoleV1::Cargo,
                P8EngineeringToolRoleV1::Rustc,
                P8EngineeringToolRoleV1::Rustdoc,
                P8EngineeringToolRoleV1::RustLld,
            ],
            Self::Clippy => vec![
                P8EngineeringToolRoleV1::Cargo,
                P8EngineeringToolRoleV1::CargoClippy,
                P8EngineeringToolRoleV1::Rustc,
                P8EngineeringToolRoleV1::ClippyDriver,
                P8EngineeringToolRoleV1::RustLld,
            ],
            Self::WorkspaceCheck => vec![
                P8EngineeringToolRoleV1::Cargo,
                P8EngineeringToolRoleV1::Rustc,
                P8EngineeringToolRoleV1::RustLld,
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8EngineeringToolRoleV1 {
    Cargo,
    Rustc,
    Rustdoc,
    Rustfmt,
    CargoFmt,
    CargoClippy,
    ClippyDriver,
    RustLld,
}

impl P8EngineeringToolRoleV1 {
    pub(crate) const ALL: [Self; 8] = [
        Self::Cargo,
        Self::Rustc,
        Self::Rustdoc,
        Self::Rustfmt,
        Self::CargoFmt,
        Self::CargoClippy,
        Self::ClippyDriver,
        Self::RustLld,
    ];

    pub(crate) const fn executable_name(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Rustc => "rustc",
            Self::Rustdoc => "rustdoc",
            Self::Rustfmt => "rustfmt",
            Self::CargoFmt => "cargo-fmt",
            Self::CargoClippy => "cargo-clippy",
            Self::ClippyDriver => "clippy-driver",
            Self::RustLld => "rust-lld",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8EngineeringToolIdentityV1 {
    role: P8EngineeringToolRoleV1,
    executable_byte_len: u64,
    executable_digest: P8QualityDigest,
    version_child_pid: u32,
    version_exit_code: Option<i32>,
    version_stdout_byte_len: u64,
    version_stdout_digest: P8QualityDigest,
    version_stdout_eof_observed: bool,
    version_stderr_byte_len: u64,
    version_stderr_digest: P8QualityDigest,
    version_stderr_eof_observed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8EngineeringToolchainObservationV1 {
    tools: Vec<P8EngineeringToolIdentityV1>,
    observation_digest: P8QualityDigest,
}

impl P8EngineeringToolchainObservationV1 {
    fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.tools.len() != P8EngineeringToolRoleV1::ALL.len()
            || self
                .tools
                .iter()
                .map(|tool| tool.role)
                .ne(P8EngineeringToolRoleV1::ALL)
            || self.tools.iter().any(|tool| {
                tool.executable_byte_len == 0
                    || tool.version_child_pid == 0
                    || tool.version_exit_code != Some(0)
                    || tool.version_stdout_byte_len == 0
                    || !tool.version_stdout_eof_observed
                    || !tool.version_stderr_eof_observed
            })
        {
            failures.push(P8QualityContractFailure::ToolchainMismatch);
        }
        if self.observation_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive("p8_engineering_toolchain_observation_v1", &self.tools)
    }

    #[cfg(test)]
    fn fixture(seed: &str) -> Self {
        let tools = P8EngineeringToolRoleV1::ALL
            .into_iter()
            .map(|role| P8EngineeringToolIdentityV1 {
                role,
                executable_byte_len: 1,
                executable_digest: P8QualityDigest::derive(
                    "p8_engineering_tool_executable_fixture_v1",
                    &(seed, role),
                ),
                version_child_pid: 1_000 + role as u32,
                version_exit_code: Some(0),
                version_stdout_byte_len: 1,
                version_stdout_digest: P8QualityDigest::derive(
                    "p8_engineering_tool_version_fixture_v1",
                    &(seed, role),
                ),
                version_stdout_eof_observed: true,
                version_stderr_byte_len: 0,
                version_stderr_digest: P8QualityDigest::derive(
                    "p8_engineering_tool_version_stderr_fixture_v1",
                    &(seed, role),
                ),
                version_stderr_eof_observed: true,
            })
            .collect();
        let mut value = Self {
            tools,
            observation_digest: P8QualityDigest::derive(
                "p8_engineering_toolchain_observation_v1",
                &(),
            ),
        };
        value.observation_digest = value.derived_digest();
        value
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8EngineeringSourceObservationV1 {
    source_input_digest: P8HarnessSourceInputRef,
    retained_source_lease_digest: P8QualityDigest,
    physical_identity_digest: P8QualityDigest,
    inventory_before_digest: P8QualityDigest,
    inventory_after_digest: P8QualityDigest,
}

impl P8EngineeringSourceObservationV1 {
    fn validate_against(
        &self,
        source: &P8HarnessSourceInputManifestV1,
    ) -> Vec<P8QualityContractFailure> {
        if self.source_input_digest != *source.source_input_digest()
            || self.inventory_before_digest != self.inventory_after_digest
        {
            vec![P8QualityContractFailure::SourceDrift]
        } else {
            Vec::new()
        }
    }

    #[cfg(test)]
    fn fixture(source: &P8HarnessSourceInputManifestV1) -> Self {
        let inventory =
            P8QualityDigest::derive("p8_engineering_source_inventory_fixture_v1", &"inventory");
        Self {
            source_input_digest: source.source_input_digest().clone(),
            retained_source_lease_digest: P8QualityDigest::derive(
                "p8_engineering_source_lease_fixture_v1",
                &"lease",
            ),
            physical_identity_digest: P8QualityDigest::derive(
                "p8_engineering_source_physical_identity_fixture_v1",
                &"identity",
            ),
            inventory_before_digest: inventory.clone(),
            inventory_after_digest: inventory,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum P8EngineeringWorkspaceScopeV1 {
    ExactWorkspace,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8EngineeringBuildPlanV1 {
    workspace_scope: P8EngineeringWorkspaceScopeV1,
    target_triple: P8QualityId,
    profile_digest: P8QualityDigest,
    feature_set_digest: P8QualityDigest,
    compile_fingerprint: P8QualityDigest,
}

impl P8EngineeringBuildPlanV1 {
    fn from_source(source: &P8HarnessSourceInputManifestV1) -> Self {
        Self {
            workspace_scope: P8EngineeringWorkspaceScopeV1::ExactWorkspace,
            target_triple: source.target_triple().clone(),
            profile_digest: source.profile_digest().clone(),
            feature_set_digest: source.feature_set_digest().clone(),
            compile_fingerprint: source.build_contract_digest().clone(),
        }
    }

    fn validate_against(
        &self,
        source: &P8HarnessSourceInputManifestV1,
    ) -> Vec<P8QualityContractFailure> {
        if self != &Self::from_source(source) {
            vec![P8QualityContractFailure::BuildPlanMismatch]
        } else {
            Vec::new()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum P8EngineeringEnvironmentPolicyV1 {
    ClearedSupervisorOwnedTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8EngineeringGateCommandSpecV1 {
    gate: P8EngineeringGateIdV1,
    launcher_tool: P8EngineeringToolRoleV1,
    argv: Vec<String>,
    required_tools: Vec<P8EngineeringToolRoleV1>,
    environment_policy: P8EngineeringEnvironmentPolicyV1,
    spec_digest: P8QualityDigest,
}

impl P8EngineeringGateCommandSpecV1 {
    fn canonical(gate: P8EngineeringGateIdV1) -> Self {
        let mut value = Self {
            gate,
            launcher_tool: gate.launcher_tool(),
            argv: gate.exact_argv(),
            required_tools: gate.required_tools(),
            environment_policy: P8EngineeringEnvironmentPolicyV1::ClearedSupervisorOwnedTarget,
            spec_digest: P8QualityDigest::derive("p8_engineering_gate_command_spec_v1", &()),
        };
        value.spec_digest = value.derived_digest();
        value
    }

    fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        if self != &Self::canonical(self.gate) {
            vec![P8QualityContractFailure::CommandMismatch]
        } else {
            Vec::new()
        }
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_engineering_gate_command_spec_v1",
            &(
                self.gate,
                self.launcher_tool,
                &self.argv,
                &self.required_tools,
                self.environment_policy,
            ),
        )
    }

    fn validate_exact_environment(
        &self,
        source: &P8HarnessSourceInputManifestV1,
        environment: &[(String, String)],
    ) -> bool {
        let mut expected = BTreeSet::from([
            "CARGO_BUILD_TARGET".to_string(),
            "CARGO_CACHE_RUSTC_INFO".to_string(),
            "CARGO_HOME".to_string(),
            "CARGO_NET_OFFLINE".to_string(),
            "CARGO_TARGET_DIR".to_string(),
            "P8_GATE_ATTEMPT_NONCE".to_string(),
            "RUST_SYSROOT".to_string(),
            "SOURCE_ROOT".to_string(),
        ]);
        let required = self.required_tools.iter().copied().collect::<BTreeSet<_>>();
        if required.contains(&P8EngineeringToolRoleV1::Cargo) {
            expected.insert("CARGO".into());
        }
        if required.contains(&P8EngineeringToolRoleV1::Rustc) {
            expected.insert("RUSTC".into());
        }
        if required.contains(&P8EngineeringToolRoleV1::Rustdoc) {
            expected.insert("RUSTDOC".into());
            expected.insert("RUSTDOCFLAGS".into());
        }
        if required.contains(&P8EngineeringToolRoleV1::Rustfmt) {
            expected.insert("RUSTFMT".into());
        }
        if required.contains(&P8EngineeringToolRoleV1::ClippyDriver) {
            expected.insert("RUSTC_WORKSPACE_WRAPPER".into());
        }
        if required.contains(&P8EngineeringToolRoleV1::RustLld) {
            expected.insert("RUSTFLAGS".into());
            expected.insert(format!(
                "CARGO_TARGET_{}_LINKER",
                source
                    .target_triple()
                    .as_str()
                    .replace('-', "_")
                    .to_ascii_uppercase()
            ));
        }
        let actual = environment
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<BTreeSet<_>>();
        if actual != expected {
            return false;
        }
        let values = environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<BTreeMap<_, _>>();
        let sysroot = values.get("RUST_SYSROOT").copied().unwrap_or_default();
        let linker_key = format!(
            "CARGO_TARGET_{}_LINKER",
            source
                .target_triple()
                .as_str()
                .replace('-', "_")
                .to_ascii_uppercase()
        );
        let linker = values.get(linker_key.as_str()).copied();
        environment.iter().all(|(name, value)| match name.as_str() {
            "CARGO_BUILD_TARGET" => value == source.target_triple().as_str(),
            "CARGO_CACHE_RUSTC_INFO" => value == "0",
            "CARGO_NET_OFFLINE" => value == "true",
            "P8_GATE_ATTEMPT_NONCE" => has_typed_sha256_prefix(value, "sha256:"),
            "RUST_SYSROOT" => is_proc_fd_path(value),
            "RUSTFLAGS" => linker.is_some_and(|linker| {
                value == &format!("-C linker={linker} -C linker-flavor=gnu-lld --sysroot={sysroot}")
            }),
            "RUSTDOCFLAGS" => value == &format!("--sysroot={sysroot}"),
            key if key.starts_with("CARGO_TARGET_") && key.ends_with("_LINKER") => {
                is_proc_fd_path(value)
            }
            _ => is_proc_fd_path(value),
        })
    }

    #[cfg(test)]
    fn fixture_environment(
        &self,
        source: &P8HarnessSourceInputManifestV1,
    ) -> Vec<(String, String)> {
        let required = self.required_tools.iter().copied().collect::<BTreeSet<_>>();
        let mut values = vec![
            (
                "CARGO_BUILD_TARGET".into(),
                source.target_triple().as_str().into(),
            ),
            ("CARGO_CACHE_RUSTC_INFO".into(), "0".into()),
            ("CARGO_HOME".into(), "/proc/self/fd/201".into()),
            ("CARGO_NET_OFFLINE".into(), "true".into()),
            ("CARGO_TARGET_DIR".into(), "/proc/self/fd/202".into()),
            (
                "P8_GATE_ATTEMPT_NONCE".into(),
                P8QualityDigest::derive("p8_gate_attempt_fixture_env_v1", &self.gate)
                    .as_str()
                    .into(),
            ),
            ("RUST_SYSROOT".into(), "/proc/self/fd/207".into()),
            ("SOURCE_ROOT".into(), "/proc/self/fd/203".into()),
        ];
        if required.contains(&P8EngineeringToolRoleV1::Cargo) {
            values.push(("CARGO".into(), "/proc/self/fd/204".into()));
        }
        if required.contains(&P8EngineeringToolRoleV1::Rustc) {
            values.push(("RUSTC".into(), "/proc/self/fd/205".into()));
        }
        if required.contains(&P8EngineeringToolRoleV1::Rustdoc) {
            values.push(("RUSTDOC".into(), "/proc/self/fd/206".into()));
            values.push(("RUSTDOCFLAGS".into(), "--sysroot=/proc/self/fd/207".into()));
        }
        if required.contains(&P8EngineeringToolRoleV1::Rustfmt) {
            values.push(("RUSTFMT".into(), "/proc/self/fd/208".into()));
        }
        if required.contains(&P8EngineeringToolRoleV1::ClippyDriver) {
            values.push(("RUSTC_WORKSPACE_WRAPPER".into(), "/proc/self/fd/209".into()));
        }
        if required.contains(&P8EngineeringToolRoleV1::RustLld) {
            values.push((
                format!(
                    "CARGO_TARGET_{}_LINKER",
                    source
                        .target_triple()
                        .as_str()
                        .replace('-', "_")
                        .to_ascii_uppercase()
                ),
                "/proc/self/fd/210".into(),
            ));
            values.push((
                "RUSTFLAGS".into(),
                "-C linker=/proc/self/fd/210 -C linker-flavor=gnu-lld --sysroot=/proc/self/fd/207"
                    .into(),
            ));
        }
        values.sort_by(|left, right| left.0.cmp(&right.0));
        values
    }
}

fn is_proc_fd_path(value: &str) -> bool {
    value
        .strip_prefix("/proc/self/fd/")
        .is_some_and(|fd| !fd.is_empty() && fd.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(crate) fn canonical_engineering_gate_registry_digest() -> P8QualityDigest {
    let registry = P8EngineeringGateIdV1::ALL.map(P8EngineeringGateCommandSpecV1::canonical);
    P8QualityDigest::derive("p8_engineering_gate_registry_v1", &registry)
}

pub(crate) fn canonical_engineering_build_plan_digest(
    source: &P8HarnessSourceInputManifestV1,
) -> P8QualityDigest {
    P8QualityDigest::derive(
        "p8_engineering_build_plan_v1",
        &P8EngineeringBuildPlanV1::from_source(source),
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8EngineeringTargetObservationV1 {
    root_identity_digest: P8QualityDigest,
    initial_inventory_digest: P8QualityDigest,
    initial_entry_count: u64,
    final_inventory_digest: P8QualityDigest,
    final_entry_count: u64,
}

impl P8EngineeringTargetObservationV1 {
    fn validate_contract(&self, gate: P8EngineeringGateIdV1) -> Vec<P8QualityContractFailure> {
        if self.initial_entry_count != 0
            || self.initial_inventory_digest != empty_target_inventory_digest()
            || (self.final_entry_count == 0
                && self.final_inventory_digest != empty_target_inventory_digest())
            || (self.final_entry_count != 0
                && self.final_inventory_digest == empty_target_inventory_digest())
            || (gate != P8EngineeringGateIdV1::Format && self.final_entry_count == 0)
        {
            vec![P8QualityContractFailure::TargetIsolationMismatch]
        } else {
            Vec::new()
        }
    }

    #[cfg(test)]
    fn fixture(gate: P8EngineeringGateIdV1) -> Self {
        let (final_inventory_digest, final_entry_count) = if gate == P8EngineeringGateIdV1::Format {
            (empty_target_inventory_digest(), 0)
        } else {
            (
                P8QualityDigest::derive("p8_engineering_target_final_inventory_fixture_v1", &gate),
                1,
            )
        };
        Self {
            root_identity_digest: P8QualityDigest::derive(
                "p8_engineering_target_root_fixture_v1",
                &gate,
            ),
            initial_inventory_digest: empty_target_inventory_digest(),
            initial_entry_count: 0,
            final_inventory_digest,
            final_entry_count,
        }
    }
}

fn empty_target_inventory_digest() -> P8QualityDigest {
    P8QualityDigest::derive(
        "p8_engineering_target_inventory_v1",
        &Vec::<P8QualityDigest>::new(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8ClosedPipeObservationV1 {
    byte_len: u64,
    content_digest: P8QualityDigest,
    eof_observed: bool,
    truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum P8EngineeringProcessTerminationV1 {
    Exited,
    Signaled,
    TimedOut,
    ResourceLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum P8EngineeringGateEvidenceV1 {
    FixtureParentObservationOnlyNoTrustedSupervisor,
    LinuxTrustedSupervisorClosedProcess,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8EngineeringGateCommandReceiptV1 {
    schema: String,
    evidence: P8EngineeringGateEvidenceV1,
    supervisor_session_nonce: P8QualityDigest,
    attempt_nonce: P8QualityDigest,
    command_spec: P8EngineeringGateCommandSpecV1,
    source: P8EngineeringSourceObservationV1,
    toolchain: P8EngineeringToolchainObservationV1,
    build_plan: P8EngineeringBuildPlanV1,
    target: P8EngineeringTargetObservationV1,
    exact_argv_digest: P8QualityDigest,
    exact_environment: Vec<(String, String)>,
    exact_environment_digest: P8QualityDigest,
    parent_pid: u32,
    child_pid: u32,
    termination: P8EngineeringProcessTerminationV1,
    exit_code: Option<i32>,
    stdout: P8ClosedPipeObservationV1,
    stderr: P8ClosedPipeObservationV1,
    elapsed_millis: u64,
    maximum_rss_bytes: u64,
    receipt_digest: P8EngineeringGateCommandReceiptRef,
}

impl P8EngineeringGateCommandReceiptV1 {
    fn validate_against(
        &self,
        source_input: &P8HarnessSourceInputManifestV1,
    ) -> Vec<P8QualityContractFailure> {
        let mut failures = self.command_spec.validate_contract();
        failures.extend(self.source.validate_against(source_input));
        failures.extend(self.toolchain.validate_contract());
        failures.extend(self.build_plan.validate_against(source_input));
        failures.extend(self.target.validate_contract(self.command_spec.gate));
        if self.schema != P8_ENGINEERING_GATE_COMMAND_RECEIPT_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.exact_argv_digest
            != P8QualityDigest::derive("p8_engineering_gate_exact_argv_v1", &self.command_spec.argv)
        {
            failures.push(P8QualityContractFailure::CommandMismatch);
        }
        if self.exact_environment_digest != self.derived_environment_digest() {
            failures.push(P8QualityContractFailure::EnvironmentMismatch);
        }
        let environment_names = self
            .exact_environment
            .iter()
            .map(|(name, _)| name)
            .collect::<BTreeSet<_>>();
        if environment_names.len() != self.exact_environment.len()
            || self
                .exact_environment
                .windows(2)
                .any(|pair| pair[0].0 >= pair[1].0)
        {
            failures.push(P8QualityContractFailure::EnvironmentMismatch);
        }
        if !self
            .command_spec
            .validate_exact_environment(source_input, &self.exact_environment)
        {
            failures.push(P8QualityContractFailure::EnvironmentMismatch);
        }
        if self
            .exact_environment
            .iter()
            .find(|(name, _)| name == "P8_GATE_ATTEMPT_NONCE")
            .is_none_or(|(_, value)| value != self.attempt_nonce.as_str())
        {
            failures.push(P8QualityContractFailure::NonceMismatch);
        }
        if self.parent_pid == 0
            || self.child_pid == 0
            || self.parent_pid == self.child_pid
            || self.termination != P8EngineeringProcessTerminationV1::Exited
        {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        if self.exit_code != Some(0) {
            failures.push(P8QualityContractFailure::ExitMismatch);
        }
        if !self.stdout.eof_observed
            || !self.stderr.eof_observed
            || self.stdout.truncated
            || self.stderr.truncated
        {
            failures.push(P8QualityContractFailure::PipeClosureMissing);
        }
        if self.receipt_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_environment_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_engineering_gate_exact_environment_v1",
            &(
                self.command_spec.environment_policy,
                &self.exact_environment,
                &self.target.root_identity_digest,
                &self.toolchain.observation_digest,
                &self.build_plan,
            ),
        )
    }

    fn derived_digest(&self) -> P8EngineeringGateCommandReceiptRef {
        P8EngineeringGateCommandReceiptRef::derive(&(
            (
                &self.schema,
                self.evidence,
                &self.supervisor_session_nonce,
                &self.attempt_nonce,
                &self.command_spec,
                &self.source,
                &self.toolchain,
                &self.build_plan,
                &self.target,
            ),
            (
                &self.exact_argv_digest,
                &self.exact_environment,
                &self.exact_environment_digest,
                self.parent_pid,
                self.child_pid,
                self.termination,
                self.exit_code,
                &self.stdout,
                &self.stderr,
                self.elapsed_millis,
                self.maximum_rss_bytes,
            ),
        ))
    }

    #[cfg(test)]
    fn fixture(
        source: &P8HarnessSourceInputManifestV1,
        gate: P8EngineeringGateIdV1,
        session_nonce: &P8QualityDigest,
        toolchain: &P8EngineeringToolchainObservationV1,
    ) -> Self {
        let command_spec = P8EngineeringGateCommandSpecV1::canonical(gate);
        let target = P8EngineeringTargetObservationV1::fixture(gate);
        let build_plan = P8EngineeringBuildPlanV1::from_source(source);
        let mut value = Self {
            schema: P8_ENGINEERING_GATE_COMMAND_RECEIPT_SCHEMA.into(),
            evidence: P8EngineeringGateEvidenceV1::FixtureParentObservationOnlyNoTrustedSupervisor,
            supervisor_session_nonce: session_nonce.clone(),
            attempt_nonce: P8QualityDigest::derive("p8_gate_attempt_fixture_env_v1", &gate),
            exact_argv_digest: P8QualityDigest::derive(
                "p8_engineering_gate_exact_argv_v1",
                &command_spec.argv,
            ),
            exact_environment: command_spec.fixture_environment(source),
            command_spec,
            source: P8EngineeringSourceObservationV1::fixture(source),
            toolchain: toolchain.clone(),
            build_plan,
            target,
            exact_environment_digest: P8QualityDigest::derive(
                "p8_engineering_gate_exact_environment_v1",
                &(),
            ),
            parent_pid: 41,
            child_pid: 100 + gate as u32,
            termination: P8EngineeringProcessTerminationV1::Exited,
            exit_code: Some(0),
            stdout: P8ClosedPipeObservationV1 {
                byte_len: 1,
                content_digest: P8QualityDigest::derive(
                    "p8_engineering_gate_stdout_fixture_v1",
                    &gate,
                ),
                eof_observed: true,
                truncated: false,
            },
            stderr: P8ClosedPipeObservationV1 {
                byte_len: 0,
                content_digest: P8QualityDigest::derive(
                    "p8_engineering_gate_stderr_fixture_v1",
                    &gate,
                ),
                eof_observed: true,
                truncated: false,
            },
            elapsed_millis: 1,
            maximum_rss_bytes: 1,
            receipt_digest: P8EngineeringGateCommandReceiptRef::derive(&()),
        };
        value.exact_environment_digest = value.derived_environment_digest();
        value.receipt_digest = value.derived_digest();
        value
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8EngineeringGateReceiptV1 {
    schema: String,
    evidence: P8EngineeringGateEvidenceV1,
    source_input_digest: P8HarnessSourceInputRef,
    supervisor_session_nonce: P8QualityDigest,
    command_receipts: Vec<P8EngineeringGateCommandReceiptV1>,
    receipt_digest: P8EngineeringGateReceiptRef,
}

impl P8EngineeringGateReceiptV1 {
    pub(crate) fn is_engineering_sealed(&self) -> bool {
        false
    }

    pub(crate) fn validate_against(
        &self,
        source: &P8HarnessSourceInputManifestV1,
    ) -> Vec<P8QualityContractFailure> {
        let mut failures = source.validate_contract();
        if self.schema != P8_ENGINEERING_GATE_RECEIPT_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.source_input_digest != *source.source_input_digest()
            || self.command_receipts.len() != P8EngineeringGateIdV1::ALL.len()
            || self
                .command_receipts
                .iter()
                .map(|receipt| receipt.command_spec.gate)
                .ne(P8EngineeringGateIdV1::ALL)
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        let expected_command_evidence = self.evidence;
        if self
            .command_receipts
            .iter()
            .any(|receipt| receipt.evidence != expected_command_evidence)
        {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        let attempt_nonces = self
            .command_receipts
            .iter()
            .map(|receipt| &receipt.attempt_nonce)
            .collect::<BTreeSet<_>>();
        let target_roots = self
            .command_receipts
            .iter()
            .map(|receipt| &receipt.target.root_identity_digest)
            .collect::<BTreeSet<_>>();
        if attempt_nonces.len() != self.command_receipts.len()
            || target_roots.len() != self.command_receipts.len()
        {
            failures.push(P8QualityContractFailure::GateAttemptAlias);
        }
        if let Some(first) = self.command_receipts.first() {
            if self.command_receipts.iter().any(|receipt| {
                receipt.supervisor_session_nonce != self.supervisor_session_nonce
                    || receipt.source != first.source
                    || receipt.toolchain != first.toolchain
                    || receipt.build_plan != first.build_plan
            }) {
                failures.push(P8QualityContractFailure::NonceMismatch);
            }
        }
        for receipt in &self.command_receipts {
            failures.extend(receipt.validate_against(source));
        }
        if self.receipt_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn has_linux_closed_process_audit_evidence(&self) -> bool {
        self.evidence == P8EngineeringGateEvidenceV1::LinuxTrustedSupervisorClosedProcess
            && self.command_receipts.iter().all(|receipt| {
                receipt.evidence == P8EngineeringGateEvidenceV1::LinuxTrustedSupervisorClosedProcess
            })
    }

    fn derived_digest(&self) -> P8EngineeringGateReceiptRef {
        P8EngineeringGateReceiptRef::derive(&(
            &self.schema,
            self.evidence,
            &self.source_input_digest,
            &self.supervisor_session_nonce,
            &self.command_receipts,
        ))
    }

    #[cfg(test)]
    pub(in crate::p8_quality) fn fixture(source: &P8HarnessSourceInputManifestV1) -> Self {
        let supervisor_session_nonce =
            P8QualityDigest::derive("p8_engineering_gate_session_nonce_fixture_v1", &"session");
        let toolchain = P8EngineeringToolchainObservationV1::fixture("toolchain");
        let command_receipts = P8EngineeringGateIdV1::ALL
            .into_iter()
            .map(|gate| Self::fixture_command(source, gate, &supervisor_session_nonce, &toolchain))
            .collect();
        let mut value = Self {
            schema: P8_ENGINEERING_GATE_RECEIPT_SCHEMA.into(),
            evidence: P8EngineeringGateEvidenceV1::FixtureParentObservationOnlyNoTrustedSupervisor,
            source_input_digest: source.source_input_digest().clone(),
            supervisor_session_nonce,
            command_receipts,
            receipt_digest: P8EngineeringGateReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_digest();
        value
    }

    #[cfg(test)]
    fn fixture_command(
        source: &P8HarnessSourceInputManifestV1,
        gate: P8EngineeringGateIdV1,
        session_nonce: &P8QualityDigest,
        toolchain: &P8EngineeringToolchainObservationV1,
    ) -> P8EngineeringGateCommandReceiptV1 {
        P8EngineeringGateCommandReceiptV1::fixture(source, gate, session_nonce, toolchain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> P8HarnessSourceInputManifestV1 {
        P8HarnessSourceInputManifestV1::fixture(&P8QualityDigest::derive(
            "p8_engineering_gate_source_fixture_v1",
            &"source",
        ))
    }

    fn reseal_command(receipt: &mut P8EngineeringGateCommandReceiptV1) {
        receipt.receipt_digest = receipt.derived_digest();
    }

    fn reseal_aggregate(receipt: &mut P8EngineeringGateReceiptV1) {
        for command in &mut receipt.command_receipts {
            reseal_command(command);
        }
        receipt.receipt_digest = receipt.derived_digest();
    }

    #[test]
    fn fixture_parent_observations_are_structural_only_and_never_sealed() {
        let source = source();
        let receipt = P8EngineeringGateReceiptV1::fixture(&source);
        assert!(receipt.validate_against(&source).is_empty());
        assert!(!receipt.is_engineering_sealed());
    }

    #[test]
    fn gate_set_rejects_missing_extra_duplicate_order_and_aliases() {
        let source = source();
        for mutation in [
            "missing",
            "extra",
            "duplicate",
            "order",
            "attempt",
            "target",
        ] {
            let mut receipt = P8EngineeringGateReceiptV1::fixture(&source);
            match mutation {
                "missing" => {
                    receipt.command_receipts.pop();
                }
                "extra" => {
                    receipt
                        .command_receipts
                        .push(receipt.command_receipts[0].clone());
                }
                "duplicate" => {
                    receipt.command_receipts[1] = receipt.command_receipts[0].clone();
                }
                "order" => receipt.command_receipts.swap(0, 1),
                "attempt" => {
                    receipt.command_receipts[1].attempt_nonce =
                        receipt.command_receipts[0].attempt_nonce.clone();
                }
                "target" => {
                    receipt.command_receipts[1].target.root_identity_digest = receipt
                        .command_receipts[0]
                        .target
                        .root_identity_digest
                        .clone();
                    receipt.command_receipts[1].exact_environment_digest =
                        receipt.command_receipts[1].derived_environment_digest();
                }
                _ => unreachable!(),
            }
            reseal_aggregate(&mut receipt);
            assert!(!receipt.validate_against(&source).is_empty(), "{mutation}");
        }
    }

    #[test]
    fn command_receipt_rejects_source_toolchain_build_target_exit_and_pipe_drift() {
        let source = source();
        for mutation in [
            "argv",
            "environment",
            "source",
            "toolchain",
            "build",
            "target",
            "exit",
            "termination",
            "stdout_eof",
            "stderr_eof",
            "stdout_truncated",
            "stderr_truncated",
            "session",
        ] {
            let mut receipt = P8EngineeringGateReceiptV1::fixture(&source);
            let command = &mut receipt.command_receipts[0];
            match mutation {
                "argv" => command.command_spec.argv.push("--drift".into()),
                "environment" => {
                    command.exact_environment_digest =
                        P8QualityDigest::derive("wrong_environment", &"wrong");
                }
                "source" => {
                    command.source.inventory_after_digest =
                        P8QualityDigest::derive("wrong_source_inventory", &"wrong");
                }
                "toolchain" => command.toolchain.tools.swap(0, 1),
                "build" => {
                    command.build_plan.profile_digest =
                        P8QualityDigest::derive("wrong_profile", &"wrong");
                }
                "target" => command.target.initial_entry_count = 1,
                "exit" => command.exit_code = Some(1),
                "termination" => {
                    command.termination = P8EngineeringProcessTerminationV1::TimedOut;
                }
                "stdout_eof" => command.stdout.eof_observed = false,
                "stderr_eof" => command.stderr.eof_observed = false,
                "stdout_truncated" => command.stdout.truncated = true,
                "stderr_truncated" => command.stderr.truncated = true,
                "session" => {
                    command.supervisor_session_nonce =
                        P8QualityDigest::derive("wrong_session", &"wrong");
                }
                _ => unreachable!(),
            }
            reseal_aggregate(&mut receipt);
            assert!(!receipt.validate_against(&source).is_empty(), "{mutation}");
        }

        let mut self_consistent_environment = P8EngineeringGateReceiptV1::fixture(&source);
        self_consistent_environment.command_receipts[0]
            .exact_environment
            .push(("UNDECLARED_AMBIENT".into(), "/proc/self/fd/999".into()));
        self_consistent_environment.command_receipts[0]
            .exact_environment
            .sort_by(|left, right| left.0.cmp(&right.0));
        reseal_aggregate(&mut self_consistent_environment);
        assert!(!self_consistent_environment
            .validate_against(&source)
            .is_empty());

        let mut missing_final_target = P8EngineeringGateReceiptV1::fixture(&source);
        missing_final_target.command_receipts[1]
            .target
            .final_entry_count = 0;
        missing_final_target.command_receipts[1]
            .target
            .final_inventory_digest = empty_target_inventory_digest();
        reseal_aggregate(&mut missing_final_target);
        assert!(!missing_final_target.validate_against(&source).is_empty());
    }
}
