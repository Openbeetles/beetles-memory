//! P8 quality policy freeze schema.
//!
//! This module owns the typed protocol + threshold binding. The mutable physical anchor remains
//! a separate operator-owned `Option` so its bytes can be excluded from the source fingerprint
//! it authorizes.

use serde::{Deserialize, Serialize};

use super::{P8PolicyAnchorRef, P8ProtocolLockRef, P8QualityContractFailure, P8ThresholdLockRef};

const P8_FROZEN_QUALITY_POLICY_SCHEMA: &str = "beetle-memory.p8.frozen-quality-policy.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FrozenQualityPolicyV1 {
    schema: String,
    protocol_digest: P8ProtocolLockRef,
    threshold_digest: P8ThresholdLockRef,
    policy_digest: P8PolicyAnchorRef,
}

impl P8FrozenQualityPolicyV1 {
    pub(crate) fn build(
        protocol_digest: P8ProtocolLockRef,
        threshold_digest: P8ThresholdLockRef,
    ) -> Self {
        let mut value = Self {
            schema: P8_FROZEN_QUALITY_POLICY_SCHEMA.into(),
            protocol_digest,
            threshold_digest,
            policy_digest: P8PolicyAnchorRef::derive(&()),
        };
        value.policy_digest = value.derived_digest();
        value
    }

    pub(crate) fn validate_against(
        &self,
        protocol_digest: &P8ProtocolLockRef,
        threshold_digest: &P8ThresholdLockRef,
    ) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_FROZEN_QUALITY_POLICY_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if &self.protocol_digest != protocol_digest || &self.threshold_digest != threshold_digest {
            failures.push(P8QualityContractFailure::ThresholdMismatch);
        }
        if self.policy_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8PolicyAnchorRef {
        P8PolicyAnchorRef::derive(&(&self.schema, &self.protocol_digest, &self.threshold_digest))
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    const PHYSICAL_ANCHOR: &[u8] =
        include_bytes!("../bin/bm-p8-quality-operator/p8_frozen_quality_policy.rs");

    #[test]
    fn physical_policy_anchor_is_none_and_has_a_p8_only_generator_receipt() {
        let source = std::str::from_utf8(PHYSICAL_ANCHOR).expect("anchor source");
        assert!(source.contains("P8_FROZEN_QUALITY_POLICY: Option<P8FrozenQualityPolicy> = None;"));
        assert!(!source.contains("BM_P7_"));
        assert_eq!(
            format!("{:x}", Sha256::digest(PHYSICAL_ANCHOR)),
            env!("BM_P8_FROZEN_POLICY_SHA256")
        );

        let mut receipt = Sha256::new();
        for field in [
            b"p8_frozen_quality_policy_generator_receipt_v1".as_slice(),
            env!("BM_P8_OPERATOR_BUILD_FINGERPRINT").as_bytes(),
            env!("BM_P8_FROZEN_POLICY_SHA256").as_bytes(),
        ] {
            receipt.update(
                u64::try_from(field.len())
                    .expect("generator field length")
                    .to_le_bytes(),
            );
            receipt.update(field);
        }
        assert_eq!(
            format!("{:x}", receipt.finalize()),
            env!("BM_P8_FROZEN_POLICY_GENERATOR_RECEIPT_SHA256")
        );
    }
}
