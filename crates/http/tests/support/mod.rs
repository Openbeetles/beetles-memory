use bm_sdk::{
    default_agent_subject_id, GovernedRuntimeSkillWriteInput, MemoryPrivacyClass, ProfileId,
    RuntimeSkillCreationRef, RuntimeSkillOwningScope, RuntimeSkillWrite,
};

pub fn native_runtime_profile() -> ProfileId {
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        ProfileId::native_dev_full().expect("native dev-full profile")
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "macos"))]
    {
        ProfileId::DesktopMacosEmbeddedSdk
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "windows"))]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "linux"))]
    {
        ProfileId::ServerLinuxMemoryGateway
    }
    #[cfg(all(
        not(feature = "nonproduction-replay-harness"),
        not(any(target_os = "macos", target_os = "windows", target_os = "linux"))
    ))]
    {
        compile_error!("HTTP contract tests require a supported production host target");
    }
}

#[allow(dead_code)]
pub fn governed_runtime_skill_write(write: RuntimeSkillWrite) -> GovernedRuntimeSkillWriteInput {
    GovernedRuntimeSkillWriteInput {
        write,
        creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: "test:http-console-runtime-skill".to_string(),
            verification_receipt_digest:
                "sha256:8888888888888888888888888888888888888888888888888888888888888888"
                    .to_string(),
        },
        privacy_class: MemoryPrivacyClass::SharedWithSubject,
    }
}

#[allow(dead_code)]
pub fn runtime_skill_subject_scope(agent_id: &str) -> RuntimeSkillOwningScope {
    RuntimeSkillOwningScope::Subject {
        mounted_subject_id: default_agent_subject_id(agent_id),
    }
}
