use bm_core::feature_gate::ProfileId;
use bm_core::orchestrator::PressureLevel;
use bm_core::runtime::{
    RuntimeLifecycleEffect, RuntimeLifecycleEngine, RuntimeLifecycleEvent,
    RuntimeLifecycleEventKind, RuntimeLifecycleEventSink, RuntimeLifecycleModeInput,
    RuntimeLifecycleOperation, RuntimeLifecycleReport, RuntimeLifecycleTrigger,
};
use bm_store::{StoreBackendConfig, StoreEventLog, StorePlatform};

#[test]
fn store_platform_persists_lifecycle_and_operator_events() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    let engine = RuntimeLifecycleEngine;

    let maintenance_admission = engine.admit(
        RuntimeLifecycleOperation::Maintain,
        RuntimeLifecycleTrigger::PostReply,
        RuntimeLifecycleModeInput {
            profile: ProfileId::ServerLinuxDevFull,
            pressure: PressureLevel::Normal,
            ..RuntimeLifecycleModeInput::default()
        },
    );
    let maintenance_report =
        RuntimeLifecycleReport::from_admission(maintenance_admission, 1_800_000_000)
            .finish_success(1_800_000_001, true, "maintenance_completed");
    let maintenance_event = RuntimeLifecycleEvent::from_report(
        RuntimeLifecycleEventKind::RuntimeLifecycle,
        RuntimeLifecycleEffect::RunMaintenance,
        &maintenance_report,
        1_800_000_001,
    )
    .with_payload("changed", "true");
    RuntimeLifecycleEventSink::record_lifecycle_event(&platform, maintenance_event)
        .expect("maintenance event");

    let inspect_admission = engine.admit(
        RuntimeLifecycleOperation::Inspect,
        RuntimeLifecycleTrigger::OperatorRequested,
        RuntimeLifecycleModeInput {
            profile: ProfileId::ServerLinuxDevFull,
            pressure: PressureLevel::Normal,
            ..RuntimeLifecycleModeInput::default()
        },
    );
    let inspect_report = RuntimeLifecycleReport::from_admission(inspect_admission, 1_800_000_002)
        .finish_success(1_800_000_003, false, "inspection_completed");
    let inspect_event = RuntimeLifecycleEvent::from_report(
        RuntimeLifecycleEventKind::OperatorAction,
        RuntimeLifecycleEffect::Inspect,
        &inspect_report,
        1_800_000_003,
    )
    .with_payload("action", "inspect_memory_status")
    .with_payload("accepted", "true");
    RuntimeLifecycleEventSink::record_lifecycle_event(&platform, inspect_event)
        .expect("operator event");

    let events = platform.read_events().expect("events");
    assert!(events
        .iter()
        .any(|event| event.kind_name == "runtime.lifecycle"
            && event
                .payload
                .get("operation")
                .is_some_and(|value| value == "maintain")
            && event
                .payload
                .get("trigger")
                .is_some_and(|value| value == "post_reply")));
    assert!(events
        .iter()
        .any(|event| event.kind_name == "operator.action"
            && event
                .payload
                .get("operation")
                .is_some_and(|value| value == "inspect")
            && event
                .payload
                .get("action")
                .is_some_and(|value| value == "inspect_memory_status")));
}

#[test]
fn runtime_lifecycle_events_survive_store_snapshot_import() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("source config"),
    )
    .expect("source store");
    let engine = RuntimeLifecycleEngine;
    let admission = engine.admit(
        RuntimeLifecycleOperation::Close,
        RuntimeLifecycleTrigger::SdkCall,
        RuntimeLifecycleModeInput {
            profile: ProfileId::ServerLinuxDevFull,
            pressure: PressureLevel::Normal,
            ..RuntimeLifecycleModeInput::default()
        },
    );
    let report = RuntimeLifecycleReport::from_admission(admission, 1_800_000_010).finish_success(
        1_800_000_011,
        false,
        "runtime_closed",
    );
    RuntimeLifecycleEventSink::record_lifecycle_event(
        &source,
        RuntimeLifecycleEvent::from_report(
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::Noop,
            &report,
            1_800_000_011,
        )
        .with_payload("result_summary", "runtime_closed"),
    )
    .expect("close event");

    let snapshot = source.export_store_snapshot().expect("snapshot");
    let target = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("target config"),
    )
    .expect("target store");
    target.import_store_snapshot(&snapshot).expect("import");

    let imported = target.read_events().expect("imported events");
    assert!(imported
        .iter()
        .any(|event| event.kind_name == "runtime.lifecycle"
            && event
                .payload
                .get("operation")
                .is_some_and(|value| value == "close")
            && event
                .payload
                .get("result")
                .is_some_and(|value| value == "ok")
            && event
                .payload
                .get("result_summary")
                .is_some_and(|value| value == "runtime_closed")));
}
