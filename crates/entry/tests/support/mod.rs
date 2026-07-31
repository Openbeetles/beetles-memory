use bm_entry::{EntryAuthConfig, EntryAuthDecision, EntryLocalTransport};
use bm_sdk::{
    default_agent_subject_id, GovernedRuntimeSkillWriteInput, MemoryPrivacyClass, ProfileId,
    RuntimeSkillCreationRef, RuntimeSkillOwningScope, RuntimeSkillWrite,
};

#[allow(dead_code)]
pub fn trusted_local_auth(principal: &str) -> EntryAuthDecision {
    EntryAuthConfig::disabled_for_local()
        .authenticate_local_transport(EntryLocalTransport::InProcess, principal)
}

pub fn host_production_profile() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosStandaloneMemory
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::ServerLinuxMemoryGateway
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        ProfileId::EspEmbeddedSdk
    }
}

#[allow(dead_code)]
pub fn governed_runtime_skill_write(write: RuntimeSkillWrite) -> GovernedRuntimeSkillWriteInput {
    GovernedRuntimeSkillWriteInput {
        write,
        creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: "test:entry-runtime-skill".to_string(),
            verification_receipt_digest:
                "sha256:3333333333333333333333333333333333333333333333333333333333333333"
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
