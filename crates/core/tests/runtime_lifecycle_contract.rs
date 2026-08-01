use bm_core::feature_gate::ProfileId;
use bm_core::orchestrator::PressureLevel;
use bm_core::platform::{
    MemoryOperatorInspectView, MemoryOperatorRepairView, MemoryOperatorSurfaceSummary,
};
use bm_core::runtime::{
    build_runtime_lifecycle_diagnosis, RuntimeLifecycleDisposition, RuntimeLifecycleEngine,
    RuntimeLifecycleEventKind, RuntimeLifecycleModeInput, RuntimeLifecycleOperation,
    RuntimeLifecycleTrigger,
};

#[test]
fn runtime_lifecycle_enum_names_are_stable_snake_case() {
    assert_eq!(
        serde_json::to_string(&RuntimeLifecycleOperation::Maintain).unwrap(),
        "\"maintain\""
    );
    assert_eq!(
        serde_json::to_string(&RuntimeLifecycleTrigger::PostReply).unwrap(),
        "\"post_reply\""
    );
    assert_eq!(
        serde_json::to_string(&RuntimeLifecycleDisposition::ExecuteNow).unwrap(),
        "\"execute_now\""
    );
    assert_eq!(
        serde_json::to_string(&RuntimeLifecycleEventKind::OperatorAction).unwrap(),
        "\"operator_action\""
    );
}

#[test]
fn runtime_lifecycle_admission_respects_mode_pressure_and_private_depth() {
    let engine = RuntimeLifecycleEngine;

    let normal = RuntimeLifecycleModeInput {
        profile: ProfileId::ServerLinuxDevFull,
        pressure: PressureLevel::Normal,
        ..RuntimeLifecycleModeInput::default()
    };
    let admission = engine.admit(
        RuntimeLifecycleOperation::Maintain,
        RuntimeLifecycleTrigger::PostReply,
        normal,
    );
    assert_eq!(
        admission.disposition,
        RuntimeLifecycleDisposition::ExecuteNow
    );
    assert_eq!(admission.reason, "admitted");
    assert!(!admission.lightweight_allowed);

    let critical = RuntimeLifecycleModeInput {
        profile: ProfileId::ServerLinuxDevFull,
        pressure: PressureLevel::Critical,
        ..RuntimeLifecycleModeInput::default()
    };
    let admission = engine.admit(
        RuntimeLifecycleOperation::Maintain,
        RuntimeLifecycleTrigger::PostReply,
        critical,
    );
    assert_eq!(admission.disposition, RuntimeLifecycleDisposition::Defer);
    assert_eq!(admission.reason, "critical_pressure");
    assert_eq!(admission.retry_after_ms, Some(1_000));

    let recovery = RuntimeLifecycleModeInput {
        profile: ProfileId::EspStandaloneMemory,
        recovery_safe_mode_active: true,
        pressure: PressureLevel::Critical,
        ..RuntimeLifecycleModeInput::default()
    };
    let admission = engine.admit(
        RuntimeLifecycleOperation::Recover,
        RuntimeLifecycleTrigger::BootRecovery,
        recovery,
    );
    assert_eq!(
        admission.disposition,
        RuntimeLifecycleDisposition::ExecuteNow
    );
    assert_eq!(admission.reason, "recovery_admitted");

    let voice_projection = RuntimeLifecycleModeInput {
        profile: ProfileId::ServerLinuxDevFull,
        voice_exclusive_active: true,
        pressure: PressureLevel::Normal,
        ..RuntimeLifecycleModeInput::default()
    };
    let admission = engine.admit(
        RuntimeLifecycleOperation::Project,
        RuntimeLifecycleTrigger::SdkCall,
        voice_projection,
    );
    assert_eq!(
        admission.disposition,
        RuntimeLifecycleDisposition::ExecuteNow
    );
    assert_eq!(admission.reason, "private_depth_blocked_by_mode");
    assert!(!admission.private_depth_allowed);
}

#[test]
fn linux_desktop_embedded_sdk_uses_bounded_embedded_maintenance() {
    let admission = RuntimeLifecycleEngine.admit(
        RuntimeLifecycleOperation::Maintain,
        RuntimeLifecycleTrigger::PostReply,
        RuntimeLifecycleModeInput {
            profile: ProfileId::DesktopLinuxEmbeddedSdk,
            pressure: PressureLevel::Cautious,
            post_reply_defer_elapsed_ms: Some(30_000),
            ..RuntimeLifecycleModeInput::default()
        },
    );

    assert_eq!(
        admission.disposition,
        RuntimeLifecycleDisposition::ExecuteNow
    );
    assert_eq!(admission.reason, "bounded_lightweight_maintenance");
    assert!(admission.lightweight_allowed);
}

#[test]
fn lifecycle_diagnosis_explains_repair_and_safe_actions() {
    let surface = MemoryOperatorSurfaceSummary {
        inspect: MemoryOperatorInspectView {
            memory_system_kind: "linux_full".to_string(),
            continuity_snapshot_supported: true,
            long_term_count: 0,
            continuity_capsule_count: 0,
            runtime_skill_count: 0,
            ..MemoryOperatorInspectView::default()
        },
        repair: MemoryOperatorRepairView {
            repair_needed: true,
            primary_action: "repair_self_authored_core".to_string(),
            continuity_snapshot_supported: true,
            reasons: vec!["board_core_review_due".to_string()],
            ..MemoryOperatorRepairView::default()
        },
        ..MemoryOperatorSurfaceSummary::default()
    };

    let diagnosis = build_runtime_lifecycle_diagnosis(&surface);

    assert!(diagnosis
        .evidence
        .iter()
        .any(|item| item.key == "memory_system_kind" && item.value == "linux_full"));
    assert!(diagnosis
        .root_causes
        .iter()
        .any(|cause| cause.code == "memory_runtime_repair_needed"));
    assert!(diagnosis
        .safe_actions_available
        .iter()
        .any(|action| action == "inspect_memory_status"));
    assert!(diagnosis
        .safe_actions_available
        .iter()
        .any(|action| action == "inspect_continuity_snapshot"));
}
