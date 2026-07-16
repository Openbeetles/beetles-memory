use crate::{OllamaTransparentConfig, Result, TransparentController};

#[test]
fn controller_is_the_only_public_transition_constructor() {
    let _constructor: fn(OllamaTransparentConfig) -> Result<TransparentController> =
        TransparentController::new;

    let public_exports = include_str!("lib.rs");
    for forbidden in [
        "ProcessManager",
        "SystemProcessManager",
        "RunnerInstaller",
        "FileSystemRunnerInstaller",
    ] {
        assert!(
            !public_exports.contains(forbidden),
            "{forbidden} must remain crate-private"
        );
    }
}

#[test]
fn stop_plan_fields_are_not_public_construction_inputs() {
    let contract = include_str!("preflight.rs");
    assert!(contract.contains("pub(crate) allowed: bool"));
    assert!(contract.contains("pub(crate) targets: Vec<OfficialOllamaStopTarget>"));
    assert!(contract.contains("pub(crate) bind: SocketAddr"));
    assert!(contract.contains("pub(crate) process: ObservedProcess"));
}
