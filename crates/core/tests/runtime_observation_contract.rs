use bm_core::orchestrator::PressureLevel;
use bm_core::runtime::{RuntimeForegroundSource, RuntimeLifecycleModeInput, RuntimeObservation};

#[test]
fn runtime_observation_expresses_foreground_voice_and_critical_turn() {
    let observation = RuntimeObservation::foreground(RuntimeForegroundSource::RealtimeVoiceSession)
        .with_streaming_response(true)
        .with_critical_turn(true)
        .with_pressure(PressureLevel::Cautious);

    let mode_input = RuntimeLifecycleModeInput::from_observation(observation);
    assert!(mode_input.foreground.active);
    assert_eq!(
        mode_input.foreground.primary_source,
        Some(RuntimeForegroundSource::RealtimeVoiceSession)
    );
    assert!(mode_input.voice_exclusive_active);
    assert_eq!(mode_input.pressure, PressureLevel::Critical);
}

#[test]
fn runtime_foreground_sources_cover_host_integration_shapes() {
    let sources = [
        RuntimeForegroundSource::ExternalUserMessage,
        RuntimeForegroundSource::LocalAppChat,
        RuntimeForegroundSource::RealtimeVoiceSession,
        RuntimeForegroundSource::VoiceFallbackInteraction,
        RuntimeForegroundSource::ManualOperatorAction,
        RuntimeForegroundSource::System,
    ];

    assert_eq!(sources.len(), 6);
}
