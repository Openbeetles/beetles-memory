use bm_replay::P7FrozenRunnerIdentity;

pub(super) const P7_FROZEN_RUNNER_IDENTITY: Option<P7FrozenRunnerIdentity> =
    Some(P7FrozenRunnerIdentity {
        runner_build_fingerprint:
            "cebb71c98652847692baf7f3e2a78220f097cd37ff70e043e0280c103a3ffe27",
        runner_lock_fingerprint: "e01a535f44407f32b06e8973b0490efa65eed07babe383a13258489bdb8da8fa",
        executable_sha256: "6aa9fca7c3c09fa3a3f8428954bfc217d7b89e0bf0e335e0278e3abb612e4fb1",
        gate_attestation_sha256: "1bb26d3d6f2ecb2cd22ad367195da00c81608b078e03dbc2719f37a9931505b0",
        release_metadata_sha256: "983801d15e5fb182777731afc85787a11e58860529d32dee2015644535496cfe",
        gate_source_fingerprint: "7a922b1efc946bee509b4078dec3b9ddfaf8bc22402330c011994119d2c72568",
        gate_source_manifest_sha256:
            "f5968735b1426c560c84576239cba5d6d9727be797938dd1eb83eeb3fe73f7af",
    });
