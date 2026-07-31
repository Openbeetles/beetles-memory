//! P8.5 质量实验的唯一 source / release provenance owner。

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use super::trusted_execution::engineering_gate::P8EngineeringGateReceiptV1;
use super::trusted_execution::P8SealedProcessReceiptV1;
use super::{
    domain_separated_sha256, has_typed_sha256_prefix, P8QualityArmKind, P8QualityContractFailure,
    P8QualityDigest, P8QualityId, P8QualityPurpose,
};

const P8_P84_RAW_SOURCE_AUDIT_SCHEMA: &str = "beetle-memory.p8.p84-raw-source-audit-manifest.v1";
const P8_P84_SEMANTIC_SOURCE_ANCHOR_SCHEMA: &str = "beetle-memory.p8.p84-semantic-source-anchor.v1";
const P8_HARNESS_SOURCE_INPUT_SCHEMA: &str = "beetle-memory.p8.quality-harness-source-input.v1";
const P8_SEALED_EXECUTION_RECEIPT_SCHEMA: &str =
    "beetle-memory.p8.quality-sealed-execution-receipt.v1";
const P8_HARNESS_RELEASE_SCHEMA: &str = "beetle-memory.p8.quality-harness-release.v1";
const P8_ARM_SOURCE_INPUT_SCHEMA: &str = "beetle-memory.p8.quality-arm-implementation-input.v1";
const P8_SOURCE_RELEASE_SET_SCHEMA: &str = "beetle-memory.p8.quality-source-release-set.v1";
const P8_ROOT_MANIFEST_FINGERPRINT: &str = env!("BM_P8_ROOT_MANIFEST_FINGERPRINT");
const P8_LOCK_FINGERPRINT: &str = env!("BM_P8_LOCK_FINGERPRINT");
const P8_TOOLCHAIN_FINGERPRINT: &str = env!("BM_P8_TOOLCHAIN_FINGERPRINT");
const P8_BUILD_TARGET: &str = env!("BM_P8_OPERATOR_BUILD_TARGET");
const P8_BUILD_PROFILE: &str = env!("BM_P8_OPERATOR_BUILD_PROFILE");
const P8_BUILD_FEATURES: &str = env!("BM_P8_OPERATOR_BUILD_FEATURES");
const P8_BUILD_FINGERPRINT: &str = env!("BM_P8_OPERATOR_BUILD_FINGERPRINT");

mod generated_common_source {
    include!(concat!(env!("OUT_DIR"), "/p8_common_source_inventory.rs"));
}

macro_rules! p8_source_domain_ref {
    ($name:ident, $prefix:literal, $domain:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub(crate) struct $name(String);

        impl $name {
            fn derive(value: &impl Serialize) -> Self {
                let bytes = serde_json::to_vec(value)
                    .expect("P8 source identity serialization must be infallible");
                Self(format!(
                    "{}{}",
                    $prefix,
                    domain_separated_sha256($domain, &[bytes.as_slice()])
                ))
            }

            #[cfg(test)]
            pub(crate) fn derive_for_test(value: &str) -> Self {
                Self::derive(&value)
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

p8_source_domain_ref!(
    P8RawSourceAuditManifestRef,
    "p8_raw_source_audit_manifest:sha256:",
    "p8_raw_source_audit_manifest_v1"
);
p8_source_domain_ref!(
    P8SemanticSourceAnchorRef,
    "p8_semantic_source_anchor:sha256:",
    "p8_semantic_source_anchor_v1"
);
p8_source_domain_ref!(
    P8HarnessSourceInputRef,
    "p8_harness_source_input:sha256:",
    "p8_harness_source_input_v1"
);
p8_source_domain_ref!(
    P8CommonHarnessSemanticSourceRef,
    "p8_common_harness_semantic_source:sha256:",
    "p8_common_harness_semantic_source_v1"
);
p8_source_domain_ref!(
    P8HarnessReleaseRef,
    "p8_harness_release:sha256:",
    "p8_harness_release_v1"
);
p8_source_domain_ref!(P8ArmInputRef, "p8_arm_input:sha256:", "p8_arm_input_v1");
p8_source_domain_ref!(
    P8ArmReleaseRef,
    "p8_arm_release:sha256:",
    "p8_arm_release_v1"
);
p8_source_domain_ref!(
    P8SourceReleaseSetRef,
    "p8_source_release_set:sha256:",
    "p8_source_release_set_v1"
);
p8_source_domain_ref!(
    P8SealedExecutionReceiptRef,
    "p8_sealed_execution_receipt:sha256:",
    "p8_sealed_execution_receipt_v1"
);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8RawSourceVerifierPassV1 {
    PackagerReconstruction,
    IndependentLocalByteWalk,
    IndependentReadOnlyReview,
}

impl P8RawSourceVerifierPassV1 {
    const ALL: [Self; 3] = [
        Self::PackagerReconstruction,
        Self::IndependentLocalByteWalk,
        Self::IndependentReadOnlyReview,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8SourceEvidenceBoundaryV1 {
    SourceReconstructionOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8P84RawSourceAuditManifestV1 {
    schema: String,
    raw_bundle_digest: P8QualityDigest,
    raw_manifest_digest: P8QualityDigest,
    base_commit: P8QualityId,
    status_porcelain_v2_digest: P8QualityDigest,
    source_inventory_digest: P8QualityDigest,
    source_file_count: u64,
    allowed_untracked_count: u64,
    verifier_passes: Vec<P8RawSourceVerifierPassV1>,
    evidence_boundary: P8SourceEvidenceBoundaryV1,
    manifest_digest: P8RawSourceAuditManifestRef,
}

impl P8P84RawSourceAuditManifestV1 {
    pub(crate) fn materialized_from_cutover_audit() -> Self {
        let mut value = Self {
            schema: P8_P84_RAW_SOURCE_AUDIT_SCHEMA.into(),
            raw_bundle_digest: materialized_digest(
                "f5dc4492c9ad78ac43887fb7b06bcaffc6dba3e585a21d4c4a77b06af94b0832",
            ),
            raw_manifest_digest: materialized_digest(
                "732793d3f1c9f766950933378b59a6740f211a6e112c583b6e4aef94d0e8578e",
            ),
            base_commit: P8QualityId::parse("fea3d933171d8926c49e53b6b2ab099f078813cf")
                .expect("materialized base commit is canonical"),
            status_porcelain_v2_digest: materialized_digest(
                "bc68caabf1f8ede95fcf74cbf27eae7a699f782b5be2a08b92d238080164c0fd",
            ),
            source_inventory_digest: materialized_digest(
                "156b5e09ecd4e1f059cdc91f7863bb05fbfb89e0434ac6f89ea602a7c546be93",
            ),
            source_file_count: 618,
            allowed_untracked_count: 38,
            verifier_passes: P8RawSourceVerifierPassV1::ALL.to_vec(),
            evidence_boundary: P8SourceEvidenceBoundaryV1::SourceReconstructionOnly,
            manifest_digest: P8RawSourceAuditManifestRef::derive(&()),
        };
        value.manifest_digest = value.derived_digest();
        value
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let canonical = Self::materialized_from_cutover_audit();
        let mut failures = Vec::new();
        if self.schema != P8_P84_RAW_SOURCE_AUDIT_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.raw_bundle_digest != canonical.raw_bundle_digest
            || self.raw_manifest_digest != canonical.raw_manifest_digest
            || self.base_commit != canonical.base_commit
            || self.status_porcelain_v2_digest != canonical.status_porcelain_v2_digest
            || self.source_inventory_digest != canonical.source_inventory_digest
            || self.source_file_count != canonical.source_file_count
            || self.allowed_untracked_count != canonical.allowed_untracked_count
            || self.verifier_passes != canonical.verifier_passes
            || self.evidence_boundary != P8SourceEvidenceBoundaryV1::SourceReconstructionOnly
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        if self.manifest_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8RawSourceAuditManifestRef {
        P8RawSourceAuditManifestRef::derive(&(
            &self.schema,
            &self.raw_bundle_digest,
            &self.raw_manifest_digest,
            &self.base_commit,
            &self.status_porcelain_v2_digest,
            &self.source_inventory_digest,
            self.source_file_count,
            self.allowed_untracked_count,
            &self.verifier_passes,
            self.evidence_boundary,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8P84SemanticSourceAnchorV1 {
    schema: String,
    raw_source_audit: P8P84RawSourceAuditManifestV1,
    evidence_boundary: P8SourceEvidenceBoundaryV1,
    anchor_digest: P8SemanticSourceAnchorRef,
}

impl P8P84SemanticSourceAnchorV1 {
    pub(crate) fn build(
        audit: &P8P84RawSourceAuditManifestV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let failures = audit.validate_contract();
        if !failures.is_empty() {
            return Err(failures);
        }
        let mut value = Self {
            schema: P8_P84_SEMANTIC_SOURCE_ANCHOR_SCHEMA.into(),
            raw_source_audit: audit.clone(),
            evidence_boundary: P8SourceEvidenceBoundaryV1::SourceReconstructionOnly,
            anchor_digest: P8SemanticSourceAnchorRef::derive(&()),
        };
        value.anchor_digest = value.derived_digest();
        Ok(value)
    }

    pub(crate) fn anchor_digest(&self) -> &P8SemanticSourceAnchorRef {
        &self.anchor_digest
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.raw_source_audit.validate_contract();
        if self.schema != P8_P84_SEMANTIC_SOURCE_ANCHOR_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.evidence_boundary != P8SourceEvidenceBoundaryV1::SourceReconstructionOnly {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        if self.anchor_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8SemanticSourceAnchorRef {
        P8SemanticSourceAnchorRef::derive(&(
            &self.schema,
            &self.raw_source_audit,
            self.evidence_boundary,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8HarnessExecutableRoleV1 {
    SourcePublisher,
    QualityRunner,
    QualityOperator,
    TrustedSupervisor,
}

impl P8HarnessExecutableRoleV1 {
    pub(crate) const ALL: [Self; 4] = [
        Self::SourcePublisher,
        Self::QualityRunner,
        Self::QualityOperator,
        Self::TrustedSupervisor,
    ];

    pub(crate) const fn executable_file_name(self) -> &'static str {
        match self {
            Self::SourcePublisher => "source-publisher.bin",
            Self::QualityRunner => "quality-runner.bin",
            Self::QualityOperator => "quality-operator.bin",
            Self::TrustedSupervisor => "trusted-supervisor.bin",
        }
    }

    pub(crate) const fn schema_name(self) -> &'static str {
        match self {
            Self::SourcePublisher => "source_publisher",
            Self::QualityRunner => "quality_runner",
            Self::QualityOperator => "quality_operator",
            Self::TrustedSupervisor => "trusted_supervisor",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CommonHarnessSemanticSourceManifestV1 {
    source_inventory: Vec<P8CommonSemanticSourceInventoryEntryV1>,
    exclusions: Vec<P8CommonSemanticSourceExclusionV1>,
    inventory_rule_digest: P8QualityDigest,
    aggregate_source_fingerprint: P8QualityDigest,
    evidence_boundary: P8CommonSourceEvidenceBoundaryV1,
    inventory_digest: P8QualityDigest,
    manifest_digest: P8CommonHarnessSemanticSourceRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8CommonSemanticSourceComponentIdV1 {
    ReplayQuality,
    Runner,
    Operator,
    Publisher,
    Supervisor,
}

impl P8CommonSemanticSourceComponentIdV1 {
    const ALL: [Self; 5] = [
        Self::ReplayQuality,
        Self::Runner,
        Self::Operator,
        Self::Publisher,
        Self::Supervisor,
    ];

    fn from_generated(value: &str) -> Option<Self> {
        match value {
            "replay_quality" => Some(Self::ReplayQuality),
            "quality_runner" => Some(Self::Runner),
            "quality_operator" => Some(Self::Operator),
            "source_publisher" => Some(Self::Publisher),
            "trusted_supervisor" => Some(Self::Supervisor),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CommonSemanticSourceInventoryEntryV1 {
    component: P8CommonSemanticSourceComponentIdV1,
    relative_path: String,
    byte_len: u64,
    source_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CommonSemanticSourceExclusionV1 {
    relative_path: String,
    reason: P8CommonSemanticSourceExclusionReasonV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8CommonSemanticSourceExclusionReasonV1 {
    FrozenQualityPolicySelfReference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8CommonSourceEvidenceBoundaryV1 {
    FixtureContractOnlyNoSourceOrExclusionProof,
    WorkspaceInventoryWithFrozenPolicyExclusionProof,
}

impl P8CommonHarnessSemanticSourceManifestV1 {
    #[cfg(test)]
    fn build(
        replay_quality_source_digest: P8QualityDigest,
        runner_source_digest: P8QualityDigest,
        operator_source_digest: P8QualityDigest,
        publisher_source_digest: P8QualityDigest,
        supervisor_source_digest: P8QualityDigest,
    ) -> Self {
        let source_inventory = vec![
            P8CommonSemanticSourceInventoryEntryV1 {
                component: P8CommonSemanticSourceComponentIdV1::ReplayQuality,
                relative_path: "fixture/replay-quality".into(),
                byte_len: 1,
                source_digest: replay_quality_source_digest,
            },
            P8CommonSemanticSourceInventoryEntryV1 {
                component: P8CommonSemanticSourceComponentIdV1::Runner,
                relative_path: "fixture/quality-runner".into(),
                byte_len: 1,
                source_digest: runner_source_digest,
            },
            P8CommonSemanticSourceInventoryEntryV1 {
                component: P8CommonSemanticSourceComponentIdV1::Operator,
                relative_path: "fixture/quality-operator".into(),
                byte_len: 1,
                source_digest: operator_source_digest,
            },
            P8CommonSemanticSourceInventoryEntryV1 {
                component: P8CommonSemanticSourceComponentIdV1::Publisher,
                relative_path: "fixture/source-publisher".into(),
                byte_len: 1,
                source_digest: publisher_source_digest,
            },
            P8CommonSemanticSourceInventoryEntryV1 {
                component: P8CommonSemanticSourceComponentIdV1::Supervisor,
                relative_path: "fixture/trusted-supervisor".into(),
                byte_len: 1,
                source_digest: supervisor_source_digest,
            },
        ];
        let evidence_boundary =
            P8CommonSourceEvidenceBoundaryV1::FixtureContractOnlyNoSourceOrExclusionProof;
        let exclusions = Vec::new();
        let inventory_rule_digest =
            P8QualityDigest::derive("p8_common_harness_fixture_inventory_rule_v1", &());
        let aggregate_source_fingerprint = P8QualityDigest::derive(
            "p8_common_harness_fixture_aggregate_fingerprint_v1",
            &source_inventory,
        );
        let inventory_digest = P8QualityDigest::derive(
            "p8_common_harness_fixture_component_inputs_v1",
            &(
                &source_inventory,
                &exclusions,
                &inventory_rule_digest,
                &aggregate_source_fingerprint,
                evidence_boundary,
            ),
        );
        let mut value = Self {
            source_inventory,
            exclusions,
            inventory_rule_digest,
            aggregate_source_fingerprint,
            evidence_boundary,
            inventory_digest,
            manifest_digest: P8CommonHarnessSemanticSourceRef::derive(&()),
        };
        value.manifest_digest = value.derived_digest();
        value
    }

    pub(crate) fn materialized_from_workspace_build() -> Result<Self, Vec<P8QualityContractFailure>>
    {
        if !generated_common_source::WORKSPACE_ATTESTED {
            return Err(vec![P8QualityContractFailure::TrustedSourceMissing]);
        }
        Self::materialized_workspace_unchecked()
            .map_err(|_| vec![P8QualityContractFailure::CoverageMismatch])
    }

    fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        match self.evidence_boundary {
            P8CommonSourceEvidenceBoundaryV1::FixtureContractOnlyNoSourceOrExclusionProof => {
                let component_ids = self
                    .source_inventory
                    .iter()
                    .map(|entry| entry.component)
                    .collect::<Vec<_>>();
                let expected_rule =
                    P8QualityDigest::derive("p8_common_harness_fixture_inventory_rule_v1", &());
                let expected_aggregate = P8QualityDigest::derive(
                    "p8_common_harness_fixture_aggregate_fingerprint_v1",
                    &self.source_inventory,
                );
                if component_ids != P8CommonSemanticSourceComponentIdV1::ALL
                    || !self.exclusions.is_empty()
                    || self.inventory_rule_digest != expected_rule
                    || self.aggregate_source_fingerprint != expected_aggregate
                    || self.inventory_digest
                        != P8QualityDigest::derive(
                            "p8_common_harness_fixture_component_inputs_v1",
                            &(
                                &self.source_inventory,
                                &self.exclusions,
                                &self.inventory_rule_digest,
                                &self.aggregate_source_fingerprint,
                                self.evidence_boundary,
                            ),
                        )
                {
                    failures.push(P8QualityContractFailure::CoverageMismatch);
                }
            }
            P8CommonSourceEvidenceBoundaryV1::WorkspaceInventoryWithFrozenPolicyExclusionProof => {
                match Self::materialized_workspace_unchecked() {
                    Ok(canonical) if &canonical == self => {}
                    _ => failures.push(P8QualityContractFailure::CoverageMismatch),
                }
            }
        }
        if self.manifest_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn materialized_workspace_unchecked() -> Result<Self, &'static str> {
        let mut source_inventory = generated_common_source::SOURCE_INPUTS
            .iter()
            .map(|(component, relative_path, byte_len, source_digest)| {
                let component = P8CommonSemanticSourceComponentIdV1::from_generated(component)
                    .ok_or("unknown P8 common source component")?;
                let source_digest = P8QualityDigest::parse(format!("sha256:{source_digest}"))
                    .map_err(|_| "invalid P8 common source digest")?;
                Ok(P8CommonSemanticSourceInventoryEntryV1 {
                    component,
                    relative_path: (*relative_path).to_string(),
                    byte_len: *byte_len,
                    source_digest,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if source_inventory.is_empty()
            || source_inventory
                .windows(2)
                .any(|pair| pair[0].relative_path >= pair[1].relative_path)
            || source_inventory.iter().any(|entry| {
                entry.byte_len == 0
                    || !valid_common_source_relative_path(&entry.relative_path)
                    || generated_common_source::EXCLUDED_RELATIVE_PATHS
                        .contains(&entry.relative_path.as_str())
            })
        {
            return Err("invalid or unordered P8 common source inventory");
        }
        source_inventory.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let components = source_inventory
            .iter()
            .map(|entry| entry.component)
            .collect::<BTreeSet<_>>();
        if components
            != P8CommonSemanticSourceComponentIdV1::ALL
                .into_iter()
                .collect::<BTreeSet<_>>()
        {
            return Err("P8 common source component coverage is incomplete");
        }
        let exclusions = generated_common_source::EXCLUDED_RELATIVE_PATHS
            .iter()
            .map(|path| P8CommonSemanticSourceExclusionV1 {
                relative_path: (*path).to_string(),
                reason: P8CommonSemanticSourceExclusionReasonV1::FrozenQualityPolicySelfReference,
            })
            .collect::<Vec<_>>();
        if exclusions.is_empty()
            || exclusions
                .windows(2)
                .any(|pair| pair[0].relative_path >= pair[1].relative_path)
            || exclusions
                .iter()
                .any(|entry| !valid_common_source_relative_path(&entry.relative_path))
        {
            return Err("P8 common source exclusion proof is invalid");
        }
        let inventory_rule_digest = P8QualityDigest::parse(format!(
            "sha256:{}",
            generated_common_source::INVENTORY_RULE_SHA256
        ))
        .map_err(|_| "P8 common source inventory rule digest is invalid")?;
        let aggregate_source_fingerprint = P8QualityDigest::parse(format!(
            "sha256:{}",
            generated_common_source::AGGREGATE_FINGERPRINT_SHA256
        ))
        .map_err(|_| "P8 common source aggregate fingerprint is invalid")?;
        if aggregate_source_fingerprint.as_str()
            != format!("sha256:{}", super::P8_REPLAY_VALIDATOR_FINGERPRINT)
        {
            return Err("P8 common source aggregate fingerprint drifted");
        }
        let evidence_boundary =
            P8CommonSourceEvidenceBoundaryV1::WorkspaceInventoryWithFrozenPolicyExclusionProof;
        let inventory_digest = P8QualityDigest::derive(
            "p8_common_harness_workspace_inventory_v1",
            &(
                &source_inventory,
                &exclusions,
                &inventory_rule_digest,
                &aggregate_source_fingerprint,
                evidence_boundary,
            ),
        );
        let mut value = Self {
            source_inventory,
            exclusions,
            inventory_rule_digest,
            aggregate_source_fingerprint,
            evidence_boundary,
            inventory_digest,
            manifest_digest: P8CommonHarnessSemanticSourceRef::derive(&()),
        };
        value.manifest_digest = value.derived_digest();
        Ok(value)
    }

    fn derived_digest(&self) -> P8CommonHarnessSemanticSourceRef {
        P8CommonHarnessSemanticSourceRef::derive(&(
            &self.source_inventory,
            &self.exclusions,
            &self.inventory_rule_digest,
            &self.aggregate_source_fingerprint,
            self.evidence_boundary,
            &self.inventory_digest,
        ))
    }
}

fn valid_common_source_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains("//")
        && !path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        && !path
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HarnessSourceInputManifestV1 {
    schema: String,
    root_manifest_digest: P8QualityDigest,
    lock_digest: P8QualityDigest,
    common_harness_semantic_source: P8CommonHarnessSemanticSourceManifestV1,
    target_triple: P8QualityId,
    toolchain_digest: P8QualityDigest,
    profile_digest: P8QualityDigest,
    feature_set_digest: P8QualityDigest,
    compile_fingerprint: P8QualityDigest,
    source_input_digest: P8HarnessSourceInputRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct P8WorkspaceSourceObservationV1 {
    pub(crate) retained_source_lease_digest: P8QualityDigest,
    pub(crate) physical_identity_digest: P8QualityDigest,
    pub(crate) inventory_digest: P8QualityDigest,
}

impl P8HarnessSourceInputManifestV1 {
    pub(super) fn materialized_from_workspace_build() -> Result<Self, Vec<P8QualityContractFailure>>
    {
        if super::P8_VALIDATOR_SOURCE_ATTESTATION != "workspace_source" {
            return Err(vec![P8QualityContractFailure::TrustedSourceMissing]);
        }
        let common_harness_semantic_source =
            P8CommonHarnessSemanticSourceManifestV1::materialized_from_workspace_build()?;
        let parsed = (|| {
            Ok::<_, P8QualityContractFailure>((
                P8QualityDigest::parse(format!("sha256:{P8_ROOT_MANIFEST_FINGERPRINT}"))?,
                P8QualityDigest::parse(format!("sha256:{P8_LOCK_FINGERPRINT}"))?,
                P8QualityId::parse(P8_BUILD_TARGET)?,
                P8QualityDigest::parse(format!("sha256:{P8_TOOLCHAIN_FINGERPRINT}"))?,
                P8QualityDigest::derive("p8_harness_build_profile_v1", &P8_BUILD_PROFILE),
                P8QualityDigest::derive("p8_harness_build_features_v1", &P8_BUILD_FEATURES),
                P8QualityDigest::parse(format!("sha256:{P8_BUILD_FINGERPRINT}"))?,
            ))
        })();
        let (
            root_manifest_digest,
            lock_digest,
            target_triple,
            toolchain_digest,
            profile_digest,
            feature_set_digest,
            compile_fingerprint,
        ) = parsed.map_err(|failure| vec![failure])?;
        let mut value = Self {
            schema: P8_HARNESS_SOURCE_INPUT_SCHEMA.into(),
            root_manifest_digest,
            lock_digest,
            common_harness_semantic_source,
            target_triple,
            toolchain_digest,
            profile_digest,
            feature_set_digest,
            compile_fingerprint,
            source_input_digest: P8HarnessSourceInputRef::derive(&()),
        };
        value.source_input_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg(test)]
    pub(crate) fn build(
        root_manifest_digest: P8QualityDigest,
        lock_digest: P8QualityDigest,
        replay_quality_source_digest: P8QualityDigest,
        runner_source_digest: P8QualityDigest,
        operator_source_digest: P8QualityDigest,
        publisher_source_digest: P8QualityDigest,
        supervisor_source_digest: P8QualityDigest,
        target_triple: P8QualityId,
        toolchain_digest: P8QualityDigest,
        profile_digest: P8QualityDigest,
        feature_set_digest: P8QualityDigest,
        compile_fingerprint: P8QualityDigest,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let common_harness_semantic_source = P8CommonHarnessSemanticSourceManifestV1::build(
            replay_quality_source_digest,
            runner_source_digest,
            operator_source_digest,
            publisher_source_digest,
            supervisor_source_digest,
        );
        let mut value = Self {
            schema: P8_HARNESS_SOURCE_INPUT_SCHEMA.into(),
            root_manifest_digest,
            lock_digest,
            common_harness_semantic_source,
            target_triple,
            toolchain_digest,
            profile_digest,
            feature_set_digest,
            compile_fingerprint,
            source_input_digest: P8HarnessSourceInputRef::derive(&()),
        };
        value.source_input_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) fn source_input_digest(&self) -> &P8HarnessSourceInputRef {
        &self.source_input_digest
    }

    pub(crate) fn common_semantic_source_digest(&self) -> &P8CommonHarnessSemanticSourceRef {
        &self.common_harness_semantic_source.manifest_digest
    }

    pub(crate) fn toolchain_digest(&self) -> &P8QualityDigest {
        &self.toolchain_digest
    }

    pub(crate) fn target_triple(&self) -> &P8QualityId {
        &self.target_triple
    }

    pub(crate) fn profile_digest(&self) -> &P8QualityDigest {
        &self.profile_digest
    }

    pub(crate) fn feature_set_digest(&self) -> &P8QualityDigest {
        &self.feature_set_digest
    }

    pub(crate) fn build_contract_digest(&self) -> &P8QualityDigest {
        &self.compile_fingerprint
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.common_harness_semantic_source.validate_contract();
        if self.schema != P8_HARNESS_SOURCE_INPUT_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.source_input_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn observe_workspace_source(
        &self,
        source_root: &Path,
    ) -> Result<P8WorkspaceSourceObservationV1, P8QualityContractFailure> {
        if super::P8_VALIDATOR_SOURCE_ATTESTATION != "workspace_source"
            || std::fs::canonicalize(source_root).ok().as_deref() != Some(source_root)
            || self
                != &Self::materialized_from_workspace_build()
                    .map_err(|_| P8QualityContractFailure::TrustedSourceMissing)?
        {
            return Err(P8QualityContractFailure::TrustedSourceMissing);
        }
        let replay_root = source_root.join("crates/replay");
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        for selector in generated_common_source::SOURCE_SELECTORS {
            crate::build_support::collect_regular_files(&replay_root.join(selector), &mut files)
                .map_err(|_| P8QualityContractFailure::SourceDrift)?;
        }
        crate::build_support::sort_regular_files_relative_to(&replay_root, &mut files)
            .map_err(|_| P8QualityContractFailure::SourceDrift)?;
        if files.windows(2).any(|pair| pair[0] == pair[1])
            || files.len() != generated_common_source::SOURCE_INPUTS.len()
        {
            return Err(P8QualityContractFailure::SourceDrift);
        }
        let mut observed = Vec::with_capacity(files.len());
        let mut aggregate = Sha256::new();
        hash_workspace_field(&mut aggregate, b"p8_verifier_source_inputs_sha256_v1")?;
        aggregate.update(
            u64::try_from(files.len())
                .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?
                .to_le_bytes(),
        );
        for (file, expected) in files.iter().zip(generated_common_source::SOURCE_INPUTS) {
            let relative = file
                .strip_prefix(&replay_root)
                .ok()
                .and_then(Path::to_str)
                .map(|value| value.replace('\\', "/"))
                .ok_or(P8QualityContractFailure::SourceDrift)?;
            let bytes = crate::build_support::read_regular_file_stable(file)
                .map_err(|_| P8QualityContractFailure::SourceDrift)?;
            let byte_len = u64::try_from(bytes.len())
                .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?;
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            if relative != expected.1
                || byte_len != expected.2
                || sha256 != expected.3
                || generated_common_source::EXCLUDED_RELATIVE_PATHS.contains(&relative.as_str())
            {
                return Err(P8QualityContractFailure::SourceDrift);
            }
            hash_workspace_field(&mut aggregate, relative.as_bytes())?;
            hash_workspace_field(&mut aggregate, &bytes)?;
            observed.push((relative, byte_len, sha256));
        }
        if format!("{:x}", aggregate.finalize()) != super::P8_REPLAY_VALIDATOR_FINGERPRINT {
            return Err(P8QualityContractFailure::SourceDrift);
        }
        verify_workspace_single_file(
            source_root,
            "Cargo.toml",
            "p8_root_manifest_sha256_v1",
            &self.root_manifest_digest,
        )?;
        verify_workspace_single_file(
            source_root,
            "Cargo.lock",
            "p8_lock_sha256_v1",
            &self.lock_digest,
        )?;
        let physical_identity_digest = workspace_root_identity(source_root)?;
        let inventory_digest =
            P8QualityDigest::derive("p8_engineering_workspace_inventory_v1", &observed);
        Ok(P8WorkspaceSourceObservationV1 {
            retained_source_lease_digest: P8QualityDigest::derive(
                "p8_engineering_workspace_source_lease_v1",
                &(
                    &self.source_input_digest,
                    &physical_identity_digest,
                    &inventory_digest,
                ),
            ),
            physical_identity_digest,
            inventory_digest,
        })
    }

    fn derived_digest(&self) -> P8HarnessSourceInputRef {
        P8HarnessSourceInputRef::derive(&(
            &self.schema,
            &self.root_manifest_digest,
            &self.lock_digest,
            &self.common_harness_semantic_source,
            &self.target_triple,
            &self.toolchain_digest,
            &self.profile_digest,
            &self.feature_set_digest,
            &self.compile_fingerprint,
        ))
    }

    #[cfg(test)]
    pub(crate) fn fixture(seed: &P8QualityDigest) -> Self {
        Self::build(
            P8QualityDigest::derive("p8_harness_fixture_root", seed),
            P8QualityDigest::derive("p8_harness_fixture_lock", seed),
            P8QualityDigest::derive("p8_harness_fixture_quality", seed),
            P8QualityDigest::derive("p8_harness_fixture_runner", seed),
            P8QualityDigest::derive("p8_harness_fixture_operator", seed),
            P8QualityDigest::derive("p8_harness_fixture_publisher", seed),
            P8QualityDigest::derive("p8_harness_fixture_supervisor", seed),
            P8QualityId::parse("fixture-target").expect("fixture target"),
            P8QualityDigest::derive("p8_harness_fixture_toolchain", seed),
            P8QualityDigest::derive("p8_harness_fixture_profile", seed),
            P8QualityDigest::derive("p8_harness_fixture_features", seed),
            P8QualityDigest::derive("p8_harness_fixture_compile", seed),
        )
        .expect("fixture harness input")
    }
}

fn verify_workspace_single_file(
    root: &Path,
    relative: &str,
    contract: &str,
    expected: &P8QualityDigest,
) -> Result<(), P8QualityContractFailure> {
    let bytes = crate::build_support::read_regular_file_stable(&root.join(relative))
        .map_err(|_| P8QualityContractFailure::SourceDrift)?;
    let mut hasher = Sha256::new();
    hash_workspace_field(&mut hasher, contract.as_bytes())?;
    hasher.update(1_u64.to_le_bytes());
    hash_workspace_field(&mut hasher, relative.as_bytes())?;
    hash_workspace_field(&mut hasher, &bytes)?;
    let observed = P8QualityDigest::parse(format!("sha256:{:x}", hasher.finalize()))
        .map_err(|_| P8QualityContractFailure::DigestInvalid)?;
    if &observed == expected {
        Ok(())
    } else {
        Err(P8QualityContractFailure::SourceDrift)
    }
}

fn hash_workspace_field(hasher: &mut Sha256, value: &[u8]) -> Result<(), P8QualityContractFailure> {
    hasher.update(
        u64::try_from(value.len())
            .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?
            .to_le_bytes(),
    );
    hasher.update(value);
    Ok(())
}

#[cfg(unix)]
fn workspace_root_identity(root: &Path) -> Result<P8QualityDigest, P8QualityContractFailure> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = std::fs::metadata(root).map_err(|_| P8QualityContractFailure::SourceDrift)?;
    Ok(P8QualityDigest::derive(
        "p8_engineering_workspace_root_physical_identity_v1",
        &(metadata.dev(), metadata.ino()),
    ))
}

#[cfg(not(unix))]
fn workspace_root_identity(_root: &Path) -> Result<P8QualityDigest, P8QualityContractFailure> {
    Err(P8QualityContractFailure::TrustedExecutionMissing)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum P8SealedExecutionSubjectV1 {
    Harness {
        source_input_digest: P8HarnessSourceInputRef,
        role: P8HarnessExecutableRoleV1,
    },
    Arm {
        common_harness_semantic_source_digest: P8CommonHarnessSemanticSourceRef,
        harness_toolchain_digest: P8QualityDigest,
        arm_input_digest: P8ArmInputRef,
        arm: P8QualityArmKind,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8SealedExecutionReceiptV1 {
    schema: String,
    subject: P8SealedExecutionSubjectV1,
    executable_digest: P8QualityDigest,
    evidence: P8SealedExecutionEvidenceV1,
    receipt_digest: P8SealedExecutionReceiptRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum P8SealedExecutionEvidenceV1 {
    FixtureDigestOnly {
        sealing_contract_digest: P8QualityDigest,
    },
    EngineeringSealedProcess {
        process_receipt: Box<P8SealedProcessReceiptV1>,
    },
}

impl P8SealedExecutionReceiptV1 {
    #[cfg(test)]
    pub(crate) fn harness(
        source: &P8HarnessSourceInputManifestV1,
        role: P8HarnessExecutableRoleV1,
        executable_digest: P8QualityDigest,
        sealing_contract_digest: P8QualityDigest,
    ) -> Self {
        Self::build(
            P8SealedExecutionSubjectV1::Harness {
                source_input_digest: source.source_input_digest.clone(),
                role,
            },
            executable_digest,
            P8SealedExecutionEvidenceV1::FixtureDigestOnly {
                sealing_contract_digest,
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn arm(
        harness: &P8HarnessSourceInputManifestV1,
        input: &P8ArmImplementationInputManifestV1,
        executable_digest: P8QualityDigest,
        sealing_contract_digest: P8QualityDigest,
    ) -> Self {
        Self::build(
            P8SealedExecutionSubjectV1::Arm {
                common_harness_semantic_source_digest: harness
                    .common_harness_semantic_source
                    .manifest_digest
                    .clone(),
                harness_toolchain_digest: harness.toolchain_digest.clone(),
                arm_input_digest: input.input_digest.clone(),
                arm: input.arm,
            },
            executable_digest,
            P8SealedExecutionEvidenceV1::FixtureDigestOnly {
                sealing_contract_digest,
            },
        )
    }

    #[cfg(test)]
    fn build(
        subject: P8SealedExecutionSubjectV1,
        executable_digest: P8QualityDigest,
        evidence: P8SealedExecutionEvidenceV1,
    ) -> Self {
        let mut value = Self {
            schema: P8_SEALED_EXECUTION_RECEIPT_SCHEMA.into(),
            subject,
            executable_digest,
            evidence,
            receipt_digest: P8SealedExecutionReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_digest();
        value
    }

    pub(crate) fn from_harness_process(
        source: &P8HarnessSourceInputManifestV1,
        process_receipt: P8SealedProcessReceiptV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut failures = process_receipt.validate_contract();
        if process_receipt.source_input_digest() != source.source_input_digest()
            || !process_receipt.is_successfully_closed()
        {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        if !failures.is_empty() {
            return Err(failures);
        }
        let role = process_receipt.role();
        let executable_digest = process_receipt.executable_digest().clone();
        let mut value = Self {
            schema: P8_SEALED_EXECUTION_RECEIPT_SCHEMA.into(),
            subject: P8SealedExecutionSubjectV1::Harness {
                source_input_digest: source.source_input_digest.clone(),
                role,
            },
            executable_digest,
            evidence: P8SealedExecutionEvidenceV1::EngineeringSealedProcess {
                process_receipt: Box::new(process_receipt),
            },
            receipt_digest: P8SealedExecutionReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_digest();
        Ok(value)
    }

    fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEALED_EXECUTION_RECEIPT_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.receipt_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        if let P8SealedExecutionEvidenceV1::EngineeringSealedProcess { process_receipt } =
            &self.evidence
        {
            failures.extend(process_receipt.validate_contract());
            match &self.subject {
                P8SealedExecutionSubjectV1::Harness {
                    source_input_digest,
                    role,
                } if process_receipt.source_input_digest() == source_input_digest
                    && process_receipt.role() == *role
                    && process_receipt.executable_digest() == &self.executable_digest
                    && process_receipt.is_successfully_closed() => {}
                _ => failures.push(P8QualityContractFailure::TrustedExecutionMissing),
            }
        }
        failures
    }

    fn process_receipt(&self) -> Option<&P8SealedProcessReceiptV1> {
        match &self.evidence {
            P8SealedExecutionEvidenceV1::FixtureDigestOnly { .. } => None,
            P8SealedExecutionEvidenceV1::EngineeringSealedProcess { process_receipt } => {
                Some(process_receipt)
            }
        }
    }

    fn derived_digest(&self) -> P8SealedExecutionReceiptRef {
        P8SealedExecutionReceiptRef::derive(&(
            &self.schema,
            &self.subject,
            &self.executable_digest,
            &self.evidence,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HarnessRoleReleaseV1 {
    role: P8HarnessExecutableRoleV1,
    executable_digest: P8QualityDigest,
    sealed_execution_receipt: P8SealedExecutionReceiptV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HarnessReleaseManifestV1 {
    schema: String,
    source_input: P8HarnessSourceInputManifestV1,
    engineering_gate_receipt: P8EngineeringGateReceiptV1,
    roles: Vec<P8HarnessRoleReleaseV1>,
    release_digest: P8HarnessReleaseRef,
}

impl P8HarnessReleaseManifestV1 {
    #[cfg(target_os = "linux")]
    pub(super) fn from_trusted_execution_parts(
        _authority: super::trusted_execution::P8ReleaseAssemblyAuthority,
        source_input: P8HarnessSourceInputManifestV1,
        engineering_gate_receipt: P8EngineeringGateReceiptV1,
        process_receipts: Vec<P8SealedProcessReceiptV1>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        if !engineering_gate_receipt.has_linux_closed_process_audit_evidence() {
            return Err(vec![P8QualityContractFailure::TrustedExecutionMissing]);
        }
        let mut roles = Vec::with_capacity(process_receipts.len());
        for process_receipt in process_receipts {
            let role = process_receipt.role();
            let executable_digest = process_receipt.executable_digest().clone();
            roles.push(P8HarnessRoleReleaseV1 {
                role,
                executable_digest,
                sealed_execution_receipt: P8SealedExecutionReceiptV1::from_harness_process(
                    &source_input,
                    process_receipt,
                )?,
            });
        }
        Self::finalize(source_input, engineering_gate_receipt, roles)
    }

    #[cfg(test)]
    pub(crate) fn build(
        source_input: P8HarnessSourceInputManifestV1,
        engineering_gate_receipt: P8EngineeringGateReceiptV1,
        roles: Vec<P8HarnessRoleReleaseV1>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        Self::finalize(source_input, engineering_gate_receipt, roles)
    }

    fn finalize(
        source_input: P8HarnessSourceInputManifestV1,
        engineering_gate_receipt: P8EngineeringGateReceiptV1,
        mut roles: Vec<P8HarnessRoleReleaseV1>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        roles.sort_by_key(|role| role.role);
        let mut value = Self {
            schema: P8_HARNESS_RELEASE_SCHEMA.into(),
            source_input,
            engineering_gate_receipt,
            roles,
            release_digest: P8HarnessReleaseRef::derive(&()),
        };
        value.release_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(seed: P8QualityDigest) -> Self {
        let source = P8HarnessSourceInputManifestV1::fixture(&seed);
        let gate = P8EngineeringGateReceiptV1::fixture(&source);
        let roles = P8HarnessExecutableRoleV1::ALL
            .into_iter()
            .enumerate()
            .map(|(index, role)| {
                let executable_digest =
                    P8QualityDigest::derive("p8_harness_fixture_executable", &(&seed, index));
                P8HarnessRoleReleaseV1 {
                    role,
                    sealed_execution_receipt: P8SealedExecutionReceiptV1::harness(
                        &source,
                        role,
                        executable_digest.clone(),
                        P8QualityDigest::derive("p8_harness_fixture_sealing", &(&seed, index)),
                    ),
                    executable_digest,
                }
            })
            .collect();
        Self::build(source, gate, roles).expect("fixture harness release")
    }

    pub(crate) fn release_digest(&self) -> &P8HarnessReleaseRef {
        &self.release_digest
    }

    pub(crate) fn content_address(&self) -> &str {
        self.release_digest
            .0
            .strip_prefix("p8_harness_release:sha256:")
            .expect("validated harness release domain")
    }

    pub(crate) fn source_input_digest(&self) -> &P8HarnessSourceInputRef {
        self.source_input.source_input_digest()
    }

    pub(crate) fn common_semantic_source_digest(&self) -> &P8CommonHarnessSemanticSourceRef {
        self.source_input.common_semantic_source_digest()
    }

    pub(crate) fn toolchain_digest(&self) -> &P8QualityDigest {
        self.source_input.toolchain_digest()
    }

    pub(crate) fn build_contract_digest(&self) -> &P8QualityDigest {
        self.source_input.build_contract_digest()
    }

    fn role_release(&self, role: P8HarnessExecutableRoleV1) -> Option<&P8HarnessRoleReleaseV1> {
        self.roles.iter().find(|entry| entry.role == role)
    }

    pub(crate) fn role_executable_digest(
        &self,
        role: P8HarnessExecutableRoleV1,
    ) -> Option<&P8QualityDigest> {
        self.role_release(role)
            .map(|entry| &entry.executable_digest)
    }

    pub(crate) fn has_exact_engineering_sealed_processes(&self) -> bool {
        self.engineering_gate_receipt
            .has_linux_closed_process_audit_evidence()
            && self.roles.len() == P8HarnessExecutableRoleV1::ALL.len()
            && self.roles.iter().all(|entry| {
                entry
                    .sealed_execution_receipt
                    .process_receipt()
                    .is_some_and(P8SealedProcessReceiptV1::is_successfully_closed)
            })
    }

    #[cfg(test)]
    pub(crate) fn quality_runner_executable_digest(&self) -> &P8QualityDigest {
        &self
            .role_release(P8HarnessExecutableRoleV1::QualityRunner)
            .expect("validated harness has a quality runner")
            .executable_digest
    }

    #[cfg(test)]
    pub(crate) fn arm_sealed_execution_receipt(
        &self,
        input: &P8ArmImplementationInputManifestV1,
        executable_digest: P8QualityDigest,
        sealing_contract_digest: P8QualityDigest,
    ) -> P8SealedExecutionReceiptV1 {
        P8SealedExecutionReceiptV1::arm(
            &self.source_input,
            input,
            executable_digest,
            sealing_contract_digest,
        )
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.source_input.validate_contract();
        failures.extend(
            self.engineering_gate_receipt
                .validate_against(&self.source_input),
        );
        if self.schema != P8_HARNESS_RELEASE_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        let roles = self
            .roles
            .iter()
            .map(|entry| entry.role)
            .collect::<Vec<_>>();
        if roles != P8HarnessExecutableRoleV1::ALL {
            failures.push(P8QualityContractFailure::RoleSetMismatch);
        }
        let executable_ids = self
            .roles
            .iter()
            .map(|entry| &entry.executable_digest)
            .collect::<BTreeSet<_>>();
        let receipt_ids = self
            .roles
            .iter()
            .map(|entry| &entry.sealed_execution_receipt.receipt_digest)
            .collect::<BTreeSet<_>>();
        if executable_ids.len() != self.roles.len() || receipt_ids.len() != self.roles.len() {
            failures.push(P8QualityContractFailure::RoleIdentityAlias);
        }
        let process_receipts = self
            .roles
            .iter()
            .filter_map(|entry| entry.sealed_execution_receipt.process_receipt())
            .collect::<Vec<_>>();
        if !process_receipts.is_empty()
            && (process_receipts.len() != self.roles.len()
                || process_receipts
                    .iter()
                    .map(|receipt| receipt.pid())
                    .collect::<BTreeSet<_>>()
                    .len()
                    != self.roles.len())
        {
            failures.push(P8QualityContractFailure::RoleIdentityAlias);
        }
        for entry in &self.roles {
            failures.extend(entry.sealed_execution_receipt.validate_contract());
            let expected_subject = P8SealedExecutionSubjectV1::Harness {
                source_input_digest: self.source_input.source_input_digest.clone(),
                role: entry.role,
            };
            if entry.sealed_execution_receipt.subject != expected_subject
                || entry.sealed_execution_receipt.executable_digest != entry.executable_digest
            {
                failures.push(P8QualityContractFailure::RoleSetMismatch);
            }
        }
        if self.release_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8HarnessReleaseRef {
        P8HarnessReleaseRef::derive(&(
            &self.schema,
            &self.source_input,
            &self.engineering_gate_receipt,
            &self.roles,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum P8ArmImplementationInputV1 {
    NoMemory {
        implementation_digest: P8QualityDigest,
    },
    PublicReference {
        upstream_source_digest: P8QualityDigest,
        build_digest: P8QualityDigest,
        config_digest: P8QualityDigest,
    },
    BeetleSemantic {
        semantic_source_anchor: P8SemanticSourceAnchorRef,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ArmImplementationInputManifestV1 {
    schema: String,
    arm: P8QualityArmKind,
    input: P8ArmImplementationInputV1,
    input_digest: P8ArmInputRef,
}

impl P8ArmImplementationInputManifestV1 {
    pub(crate) fn no_memory(implementation_digest: P8QualityDigest) -> Self {
        Self::build(
            P8QualityArmKind::NoMemory,
            P8ArmImplementationInputV1::NoMemory {
                implementation_digest,
            },
        )
        .expect("no-memory input")
    }

    pub(crate) fn public_reference(
        upstream_source_digest: P8QualityDigest,
        build_digest: P8QualityDigest,
        config_digest: P8QualityDigest,
    ) -> Self {
        Self::build(
            P8QualityArmKind::PublicReference,
            P8ArmImplementationInputV1::PublicReference {
                upstream_source_digest,
                build_digest,
                config_digest,
            },
        )
        .expect("public-reference input")
    }

    pub(crate) fn beetle(
        arm: P8QualityArmKind,
        semantic_source_anchor: P8SemanticSourceAnchorRef,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        Self::build(
            arm,
            P8ArmImplementationInputV1::BeetleSemantic {
                semantic_source_anchor,
            },
        )
    }

    fn build(
        arm: P8QualityArmKind,
        input: P8ArmImplementationInputV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut value = Self {
            schema: P8_ARM_SOURCE_INPUT_SCHEMA.into(),
            arm,
            input,
            input_digest: P8ArmInputRef::derive(&()),
        };
        value.input_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_ARM_SOURCE_INPUT_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        let variant_matches = matches!(
            (&self.arm, &self.input),
            (
                P8QualityArmKind::NoMemory,
                P8ArmImplementationInputV1::NoMemory { .. }
            ) | (
                P8QualityArmKind::PublicReference,
                P8ArmImplementationInputV1::PublicReference { .. }
            ) | (
                P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate,
                P8ArmImplementationInputV1::BeetleSemantic { .. }
            )
        );
        if !variant_matches {
            failures.push(P8QualityContractFailure::PurposeMismatch);
        }
        let audit = P8P84RawSourceAuditManifestV1::materialized_from_cutover_audit();
        let canonical_anchor =
            P8P84SemanticSourceAnchorV1::build(&audit).expect("materialized P8.4 anchor");
        if let P8ArmImplementationInputV1::BeetleSemantic {
            semantic_source_anchor,
        } = &self.input
        {
            match self.arm {
                P8QualityArmKind::FrozenP84Baseline
                    if semantic_source_anchor != canonical_anchor.anchor_digest() =>
                {
                    failures.push(P8QualityContractFailure::ArmIdentityAlias);
                }
                P8QualityArmKind::P8Candidate
                    if semantic_source_anchor == canonical_anchor.anchor_digest() =>
                {
                    failures.push(P8QualityContractFailure::ArmIdentityAlias);
                }
                _ => {}
            }
        }
        if self.input_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8ArmInputRef {
        P8ArmInputRef::derive(&(&self.schema, self.arm, &self.input))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum P8ArmImplementationReleaseV1 {
    NoMemory {
        input: P8ArmImplementationInputManifestV1,
        quality_runner_executable_digest: P8QualityDigest,
        release_digest: P8ArmReleaseRef,
    },
    PublicReference {
        input: P8ArmImplementationInputManifestV1,
        upstream_release_digest: P8QualityDigest,
        adapter_executable_digest: P8QualityDigest,
        config_digest: P8QualityDigest,
        live_revision_digest: P8QualityDigest,
        sealed_execution_receipt: P8SealedExecutionReceiptV1,
        release_digest: P8ArmReleaseRef,
    },
    BeetleSemantic {
        input: P8ArmImplementationInputManifestV1,
        executable_digest: P8QualityDigest,
        sealed_execution_receipt: P8SealedExecutionReceiptV1,
        release_digest: P8ArmReleaseRef,
    },
}

impl P8ArmImplementationReleaseV1 {
    #[cfg(test)]
    pub(crate) fn no_memory(
        input: P8ArmImplementationInputManifestV1,
        harness: &P8HarnessReleaseManifestV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut failures = input.validate_contract();
        failures.extend(harness.validate_contract());
        if !failures.is_empty() {
            return Err(failures);
        }
        let runner = harness
            .role_release(P8HarnessExecutableRoleV1::QualityRunner)
            .ok_or_else(|| vec![P8QualityContractFailure::RoleSetMismatch])?;
        if input.arm != P8QualityArmKind::NoMemory
            || !matches!(
                &input.input,
                P8ArmImplementationInputV1::NoMemory {
                    implementation_digest
                } if implementation_digest == &runner.executable_digest
            )
        {
            return Err(vec![P8QualityContractFailure::PurposeMismatch]);
        }
        let release_digest = P8ArmReleaseRef::derive(&(
            P8QualityArmKind::NoMemory,
            &input,
            &runner.executable_digest,
            harness.common_semantic_source_digest(),
            harness.toolchain_digest(),
        ));
        let value = Self::NoMemory {
            input,
            quality_runner_executable_digest: runner.executable_digest.clone(),
            release_digest,
        };
        let failures = value.validate_against(harness);
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg(test)]
    pub(crate) fn public_reference(
        input: P8ArmImplementationInputManifestV1,
        harness: &P8HarnessReleaseManifestV1,
        upstream_release_digest: P8QualityDigest,
        adapter_executable_digest: P8QualityDigest,
        config_digest: P8QualityDigest,
        live_revision_digest: P8QualityDigest,
        sealed_execution_receipt: P8SealedExecutionReceiptV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut failures = input.validate_contract();
        failures.extend(harness.validate_contract());
        if !failures.is_empty() {
            return Err(failures);
        }
        if input.arm != P8QualityArmKind::PublicReference
            || !matches!(
                &input.input,
                P8ArmImplementationInputV1::PublicReference {
                    config_digest: expected,
                    ..
                } if expected == &config_digest
            )
        {
            return Err(vec![P8QualityContractFailure::PurposeMismatch]);
        }
        validate_arm_receipt(
            &sealed_execution_receipt,
            &harness.source_input,
            &input,
            &adapter_executable_digest,
        )?;
        let release_digest = P8ArmReleaseRef::derive(&(
            P8QualityArmKind::PublicReference,
            &input,
            harness.common_semantic_source_digest(),
            harness.toolchain_digest(),
            &upstream_release_digest,
            &adapter_executable_digest,
            &config_digest,
            &live_revision_digest,
            &sealed_execution_receipt,
        ));
        let value = Self::PublicReference {
            input,
            upstream_release_digest,
            adapter_executable_digest,
            config_digest,
            live_revision_digest,
            sealed_execution_receipt,
            release_digest,
        };
        let failures = value.validate_against(harness);
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    #[cfg(test)]
    pub(crate) fn beetle(
        input: P8ArmImplementationInputManifestV1,
        harness: &P8HarnessReleaseManifestV1,
        executable_digest: P8QualityDigest,
        sealed_execution_receipt: P8SealedExecutionReceiptV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut failures = input.validate_contract();
        failures.extend(harness.validate_contract());
        if !failures.is_empty() {
            return Err(failures);
        }
        if !matches!(
            input.arm,
            P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
        ) {
            return Err(vec![P8QualityContractFailure::PurposeMismatch]);
        }
        validate_arm_receipt(
            &sealed_execution_receipt,
            &harness.source_input,
            &input,
            &executable_digest,
        )?;
        let release_digest = P8ArmReleaseRef::derive(&(
            input.arm,
            &input,
            harness.common_semantic_source_digest(),
            harness.toolchain_digest(),
            &executable_digest,
            &sealed_execution_receipt,
        ));
        let value = Self::BeetleSemantic {
            input,
            executable_digest,
            sealed_execution_receipt,
            release_digest,
        };
        let failures = value.validate_against(harness);
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) fn arm(&self) -> P8QualityArmKind {
        match self {
            Self::NoMemory { .. } => P8QualityArmKind::NoMemory,
            Self::PublicReference { .. } => P8QualityArmKind::PublicReference,
            Self::BeetleSemantic { input, .. } => input.arm,
        }
    }

    pub(crate) fn release_digest(&self) -> &P8ArmReleaseRef {
        match self {
            Self::NoMemory { release_digest, .. }
            | Self::PublicReference { release_digest, .. }
            | Self::BeetleSemantic { release_digest, .. } => release_digest,
        }
    }

    fn semantic_source_anchor(&self) -> Option<&P8SemanticSourceAnchorRef> {
        match self {
            Self::BeetleSemantic { input, .. } => match &input.input {
                P8ArmImplementationInputV1::BeetleSemantic {
                    semantic_source_anchor,
                } => Some(semantic_source_anchor),
                _ => None,
            },
            Self::NoMemory { .. } | Self::PublicReference { .. } => None,
        }
    }

    pub(crate) fn executable_digest(&self) -> &P8QualityDigest {
        match self {
            Self::NoMemory {
                quality_runner_executable_digest,
                ..
            } => quality_runner_executable_digest,
            Self::PublicReference {
                adapter_executable_digest,
                ..
            } => adapter_executable_digest,
            Self::BeetleSemantic {
                executable_digest, ..
            } => executable_digest,
        }
    }

    fn sealed_receipt_digest(&self) -> Option<&P8SealedExecutionReceiptRef> {
        match self {
            Self::PublicReference {
                sealed_execution_receipt,
                ..
            }
            | Self::BeetleSemantic {
                sealed_execution_receipt,
                ..
            } => Some(&sealed_execution_receipt.receipt_digest),
            Self::NoMemory { .. } => None,
        }
    }

    fn validate_against(
        &self,
        harness: &P8HarnessReleaseManifestV1,
    ) -> Vec<P8QualityContractFailure> {
        let mut failures = harness.validate_contract();
        let expected = match self {
            Self::NoMemory {
                input,
                quality_runner_executable_digest,
                ..
            } => {
                failures.extend(input.validate_contract());
                let runner = harness.role_release(P8HarnessExecutableRoleV1::QualityRunner);
                if input.arm != P8QualityArmKind::NoMemory
                    || runner.map(|entry| &entry.executable_digest)
                        != Some(quality_runner_executable_digest)
                    || !matches!(
                        &input.input,
                        P8ArmImplementationInputV1::NoMemory {
                            implementation_digest
                        } if implementation_digest == quality_runner_executable_digest
                    )
                {
                    failures.push(P8QualityContractFailure::PurposeMismatch);
                }
                P8ArmReleaseRef::derive(&(
                    P8QualityArmKind::NoMemory,
                    input,
                    quality_runner_executable_digest,
                    harness.common_semantic_source_digest(),
                    harness.toolchain_digest(),
                ))
            }
            Self::PublicReference {
                input,
                upstream_release_digest,
                adapter_executable_digest,
                config_digest,
                live_revision_digest,
                sealed_execution_receipt,
                ..
            } => {
                failures.extend(input.validate_contract());
                if input.arm != P8QualityArmKind::PublicReference
                    || !matches!(
                        &input.input,
                        P8ArmImplementationInputV1::PublicReference {
                            config_digest: expected,
                            ..
                        } if expected == config_digest
                    )
                {
                    failures.push(P8QualityContractFailure::PurposeMismatch);
                }
                if let Err(mut receipt_failures) = validate_arm_receipt(
                    sealed_execution_receipt,
                    &harness.source_input,
                    input,
                    adapter_executable_digest,
                ) {
                    failures.append(&mut receipt_failures);
                }
                P8ArmReleaseRef::derive(&(
                    P8QualityArmKind::PublicReference,
                    input,
                    harness.common_semantic_source_digest(),
                    harness.toolchain_digest(),
                    upstream_release_digest,
                    adapter_executable_digest,
                    config_digest,
                    live_revision_digest,
                    sealed_execution_receipt,
                ))
            }
            Self::BeetleSemantic {
                input,
                executable_digest,
                sealed_execution_receipt,
                ..
            } => {
                failures.extend(input.validate_contract());
                if let Err(mut receipt_failures) = validate_arm_receipt(
                    sealed_execution_receipt,
                    &harness.source_input,
                    input,
                    executable_digest,
                ) {
                    failures.append(&mut receipt_failures);
                }
                P8ArmReleaseRef::derive(&(
                    input.arm,
                    input,
                    harness.common_semantic_source_digest(),
                    harness.toolchain_digest(),
                    executable_digest,
                    sealed_execution_receipt,
                ))
            }
        };
        if &expected != self.release_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }
}

fn validate_arm_receipt(
    receipt: &P8SealedExecutionReceiptV1,
    harness: &P8HarnessSourceInputManifestV1,
    input: &P8ArmImplementationInputManifestV1,
    executable_digest: &P8QualityDigest,
) -> Result<(), Vec<P8QualityContractFailure>> {
    let mut failures = receipt.validate_contract();
    let expected_subject = P8SealedExecutionSubjectV1::Arm {
        common_harness_semantic_source_digest: harness
            .common_harness_semantic_source
            .manifest_digest
            .clone(),
        harness_toolchain_digest: harness.toolchain_digest.clone(),
        arm_input_digest: input.input_digest.clone(),
        arm: input.arm,
    };
    if receipt.subject != expected_subject || &receipt.executable_digest != executable_digest {
        failures.push(P8QualityContractFailure::PurposeMismatch);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8SourceReleaseSetV1 {
    schema: String,
    purpose: P8QualityPurpose,
    evidence_class: P8SourceReleaseEvidenceClassV1,
    harness_release: P8HarnessReleaseManifestV1,
    arms: BTreeMap<P8QualityArmKind, P8ArmImplementationReleaseV1>,
    release_set_digest: P8SourceReleaseSetRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8SourceReleaseEvidenceClassV1 {
    FixtureContractOnlyNoSourceOrExclusionProof,
    TrustedSealed,
}

impl P8SourceReleaseSetV1 {
    pub(crate) fn build(
        purpose: P8QualityPurpose,
        harness_release: P8HarnessReleaseManifestV1,
        releases: Vec<P8ArmImplementationReleaseV1>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut arms = BTreeMap::new();
        for release in releases {
            if arms.insert(release.arm(), release).is_some() {
                return Err(vec![P8QualityContractFailure::DuplicateEntry]);
            }
        }
        let mut value = Self {
            schema: P8_SOURCE_RELEASE_SET_SCHEMA.into(),
            purpose,
            evidence_class:
                P8SourceReleaseEvidenceClassV1::FixtureContractOnlyNoSourceOrExclusionProof,
            harness_release,
            arms,
            release_set_digest: P8SourceReleaseSetRef::derive(&()),
        };
        value.release_set_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) const fn purpose(&self) -> P8QualityPurpose {
        self.purpose
    }

    pub(crate) const fn evidence_class(&self) -> P8SourceReleaseEvidenceClassV1 {
        self.evidence_class
    }

    pub(crate) fn harness_release_digest(&self) -> &P8HarnessReleaseRef {
        self.harness_release.release_digest()
    }

    pub(crate) fn harness_source_input_digest(&self) -> &P8HarnessSourceInputRef {
        self.harness_release.source_input_digest()
    }

    pub(crate) fn common_harness_semantic_source_digest(
        &self,
    ) -> &P8CommonHarnessSemanticSourceRef {
        self.harness_release.common_semantic_source_digest()
    }

    pub(crate) fn harness_toolchain_digest(&self) -> &P8QualityDigest {
        self.harness_release.toolchain_digest()
    }

    pub(crate) fn harness_build_contract_digest(&self) -> &P8QualityDigest {
        self.harness_release.build_contract_digest()
    }

    pub(crate) fn arms(&self) -> &BTreeMap<P8QualityArmKind, P8ArmImplementationReleaseV1> {
        &self.arms
    }

    pub(crate) fn arm_release_digest(&self, arm: P8QualityArmKind) -> Option<&P8ArmReleaseRef> {
        self.arms
            .get(&arm)
            .map(P8ArmImplementationReleaseV1::release_digest)
    }

    pub(crate) fn release_set_digest(&self) -> &P8SourceReleaseSetRef {
        &self.release_set_digest
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.harness_release.validate_contract();
        if self.schema != P8_SOURCE_RELEASE_SET_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.evidence_class == P8SourceReleaseEvidenceClassV1::TrustedSealed {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        let actual = self.arms.keys().copied().collect::<Vec<_>>();
        if actual != P8QualityArmKind::expected_for(self.purpose) {
            failures.push(P8QualityContractFailure::ArmSetMismatch);
        }
        for release in self.arms.values() {
            failures.extend(release.validate_against(&self.harness_release));
        }
        let release_ids = self
            .arms
            .values()
            .map(P8ArmImplementationReleaseV1::release_digest)
            .collect::<BTreeSet<_>>();
        if release_ids.len() != self.arms.len() {
            failures.push(P8QualityContractFailure::ArmIdentityAlias);
        }
        let harness_executables = self
            .harness_release
            .roles
            .iter()
            .map(|role| &role.executable_digest)
            .collect::<BTreeSet<_>>();
        let independent_arm_executables = self
            .arms
            .iter()
            .filter(|(arm, _)| **arm != P8QualityArmKind::NoMemory)
            .map(|(_, release)| release.executable_digest())
            .collect::<Vec<_>>();
        if independent_arm_executables
            .iter()
            .any(|digest| harness_executables.contains(digest))
            || independent_arm_executables
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != independent_arm_executables.len()
        {
            failures.push(P8QualityContractFailure::ArmIdentityAlias);
        }
        let audit = P8P84RawSourceAuditManifestV1::materialized_from_cutover_audit();
        let canonical_anchor =
            P8P84SemanticSourceAnchorV1::build(&audit).expect("materialized P8.4 anchor");
        let baseline = self
            .arms
            .get(&P8QualityArmKind::FrozenP84Baseline)
            .and_then(P8ArmImplementationReleaseV1::semantic_source_anchor);
        if baseline != Some(canonical_anchor.anchor_digest()) {
            failures.push(P8QualityContractFailure::ArmIdentityAlias);
        }
        if self.purpose == P8QualityPurpose::QualityCandidate {
            let frozen = self.arms.get(&P8QualityArmKind::FrozenP84Baseline);
            let candidate = self.arms.get(&P8QualityArmKind::P8Candidate);
            if candidate.is_none()
                || baseline
                    == candidate.and_then(P8ArmImplementationReleaseV1::semantic_source_anchor)
                || frozen.map(P8ArmImplementationReleaseV1::executable_digest)
                    == candidate.map(P8ArmImplementationReleaseV1::executable_digest)
                || frozen.and_then(P8ArmImplementationReleaseV1::sealed_receipt_digest)
                    == candidate.and_then(P8ArmImplementationReleaseV1::sealed_receipt_digest)
            {
                failures.push(P8QualityContractFailure::ArmIdentityAlias);
            }
        }
        if self.release_set_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8SourceReleaseSetRef {
        P8SourceReleaseSetRef::derive(&(
            &self.schema,
            self.purpose,
            self.evidence_class,
            &self.harness_release,
            &self.arms,
        ))
    }
}

fn materialized_digest(hex: &str) -> P8QualityDigest {
    P8QualityDigest::parse(format!("sha256:{hex}"))
        .expect("materialized P8.4 source evidence digest")
}

#[cfg(test)]
mod workspace_common_source_tests {
    use super::*;

    #[test]
    fn workspace_common_source_inventory_is_exact_and_excludes_frozen_policy_bytes() {
        let source = P8CommonHarnessSemanticSourceManifestV1::materialized_workspace_unchecked()
            .expect("workspace common source");
        assert_eq!(
            P8CommonHarnessSemanticSourceManifestV1::materialized_from_workspace_build()
                .expect("attested workspace common source"),
            source
        );
        assert!(source.validate_contract().is_empty());
        assert_eq!(
            source.evidence_boundary,
            P8CommonSourceEvidenceBoundaryV1::WorkspaceInventoryWithFrozenPolicyExclusionProof
        );
        assert_eq!(
            source
                .source_inventory
                .iter()
                .map(|entry| entry.component)
                .collect::<BTreeSet<_>>(),
            P8CommonSemanticSourceComponentIdV1::ALL
                .into_iter()
                .collect::<BTreeSet<_>>(),
            "all five source components must be represented"
        );
        assert!(!source.source_inventory.is_empty());
        assert_eq!(
            source.exclusions,
            vec![P8CommonSemanticSourceExclusionV1 {
                relative_path: "src/bin/bm-p8-quality-operator/p8_frozen_quality_policy.rs".into(),
                reason: P8CommonSemanticSourceExclusionReasonV1::FrozenQualityPolicySelfReference,
            }]
        );
        assert!(source.source_inventory.iter().all(|entry| {
            valid_common_source_relative_path(&entry.relative_path)
                && !entry.relative_path.contains("p7_")
                && !source
                    .exclusions
                    .iter()
                    .any(|excluded| excluded.relative_path == entry.relative_path)
        }));

        let harness = P8HarnessSourceInputManifestV1::materialized_from_workspace_build()
            .expect("workspace harness source input");
        assert!(harness.validate_contract().is_empty());
        assert_eq!(
            harness.common_semantic_source_digest(),
            &source.manifest_digest
        );
        let source_root = std::fs::canonicalize(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("workspace source root"),
        )
        .expect("canonical workspace source root");
        assert_eq!(
            harness
                .observe_workspace_source(&source_root)
                .expect("workspace source observation"),
            harness
                .observe_workspace_source(&source_root)
                .expect("repeated workspace source observation")
        );
    }

    #[test]
    fn workspace_common_source_rejects_inventory_and_exclusion_drift() {
        let canonical = P8CommonHarnessSemanticSourceManifestV1::materialized_workspace_unchecked()
            .expect("workspace common source");
        let mut variants = Vec::new();

        let mut missing = canonical.clone();
        missing.source_inventory.pop();
        variants.push(missing);

        let mut duplicate = canonical.clone();
        duplicate
            .source_inventory
            .push(duplicate.source_inventory[0].clone());
        variants.push(duplicate);

        let mut reordered = canonical.clone();
        reordered.source_inventory.swap(0, 1);
        variants.push(reordered);

        let mut missing_exclusion = canonical.clone();
        missing_exclusion.exclusions.clear();
        variants.push(missing_exclusion);

        let mut anchor_in_inventory = canonical.clone();
        let mut injected = anchor_in_inventory.source_inventory[0].clone();
        injected.relative_path =
            "src/bin/bm-p8-quality-operator/p8_frozen_quality_policy.rs".into();
        anchor_in_inventory.source_inventory.push(injected);
        variants.push(anchor_in_inventory);

        for variant in variants {
            assert!(variant
                .validate_contract()
                .contains(&P8QualityContractFailure::CoverageMismatch));
        }

        let exclusion_json = serde_json::to_value(&canonical.exclusions[0]).expect("exclusion");
        assert!(exclusion_json.get("relative_path").is_some());
        assert_eq!(
            exclusion_json.get("reason"),
            Some(&serde_json::json!("frozen_quality_policy_self_reference"))
        );
        assert!(exclusion_json.get("source_digest").is_none());
    }
}
