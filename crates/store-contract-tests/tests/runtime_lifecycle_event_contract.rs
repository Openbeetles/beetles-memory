mod support;
use bm_core::feature_gate::ProfileId;
use bm_core::orchestrator::PressureLevel;
use bm_core::runtime::{
    RuntimeLifecycleEffect, RuntimeLifecycleEngine, RuntimeLifecycleEvent,
    RuntimeLifecycleEventKind, RuntimeLifecycleEventSink, RuntimeLifecycleModeInput,
    RuntimeLifecycleOperation, RuntimeLifecycleReport, RuntimeLifecycleTrigger,
};
use bm_sdk::nonproduction_replay_harness::StoreBackendConfig;

#[test]
fn store_platform_persists_lifecycle_and_operator_events() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let engine = RuntimeLifecycleEngine;

    let maintenance_admission = engine.admit(
        RuntimeLifecycleOperation::Maintain,
        RuntimeLifecycleTrigger::PostReply,
        RuntimeLifecycleModeInput {
            profile: ProfileId::native_dev_full().expect("native dev-full profile"),
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
            profile: ProfileId::native_dev_full().expect("native dev-full profile"),
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
fn lifecycle_payload_cannot_override_typed_completion_fields() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let engine = RuntimeLifecycleEngine;
    let admission = engine.admit(
        RuntimeLifecycleOperation::Maintain,
        RuntimeLifecycleTrigger::SdkCall,
        RuntimeLifecycleModeInput {
            profile: ProfileId::native_dev_full().expect("native dev-full profile"),
            pressure: PressureLevel::Normal,
            ..RuntimeLifecycleModeInput::default()
        },
    );
    let report = RuntimeLifecycleReport::from_admission(admission, 1_800_000_004).finish_success(
        1_800_000_005,
        true,
        "maintenance_completed",
    );
    let event = RuntimeLifecycleEvent::from_report(
        RuntimeLifecycleEventKind::RuntimeLifecycle,
        RuntimeLifecycleEffect::RunMaintenance,
        &report,
        1_800_000_005,
    )
    .with_payload("success", "false");

    let error = RuntimeLifecycleEventSink::record_lifecycle_event(&platform, event)
        .expect_err("free payload cannot override a typed lifecycle field");

    assert_eq!(error.stage(), "runtime_lifecycle_event_payload");
}

#[test]
fn runtime_lifecycle_events_survive_store_snapshot_import() {
    let source = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("source config"),
    )
    .expect("source store");
    let engine = RuntimeLifecycleEngine;
    let admission = engine.admit(
        RuntimeLifecycleOperation::Close,
        RuntimeLifecycleTrigger::SdkCall,
        RuntimeLifecycleModeInput {
            profile: ProfileId::native_dev_full().expect("native dev-full profile"),
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
        ),
    )
    .expect("close event");

    let snapshot = source.export_store_snapshot().expect("snapshot");
    let target = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("target config"),
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
