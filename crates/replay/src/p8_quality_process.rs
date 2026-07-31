//! P8 quality role-process protocol shared by the library launcher and the four role binaries.
//!
//! Typed role/release schema remains in `p8_quality::source_release`; this module owns only the
//! reserved execution environment and fixed byte-level self-test protocol.

use std::io::{self, Write};

use crate::sealed_execution::{ClaimedSealedExecution, SealedExecutionDomain};

pub(crate) const P8_ROLE_SELF_TEST_ARG: &str = "--p8-quality-role-self-test";
pub(crate) const P8_ENGINEERING_GATE_SELF_TEST_ARG: &str =
    "--p8-quality-engineering-gate-self-test";
pub(crate) const P8_SOURCE_PUBLISHER_SELF_TEST_STDOUT: &[u8] =
    b"P8_QUALITY_ROLE_OK:source_publisher\n";
pub(crate) const P8_QUALITY_RUNNER_SELF_TEST_STDOUT: &[u8] = b"P8_QUALITY_ROLE_OK:quality_runner\n";
pub(crate) const P8_QUALITY_OPERATOR_SELF_TEST_STDOUT: &[u8] =
    b"P8_QUALITY_ROLE_OK:quality_operator\n";
pub(crate) const P8_TRUSTED_SUPERVISOR_SELF_TEST_STDOUT: &[u8] =
    b"P8_QUALITY_ROLE_OK:trusted_supervisor\n";
pub(crate) const P8_ENGINEERING_GATE_SEALED_STDOUT: &[u8] =
    b"P8_ENGINEERING_GATE_OK:format,unit_tests,clippy,workspace_check\n";

const P8_SEALED_EXECUTION_DOMAIN: SealedExecutionDomain = SealedExecutionDomain::new(
    "beetle-memory-p8-quality-sealed",
    "beetle-memory-p8-quality-sealed",
    "BM_P8_SEALED_EXECUTABLE_FD",
    "BM_P8_SEALED_EXECUTABLE_PATH",
    "BM_P8_SEALED_EXECUTABLE_SHA256",
    &["BM_P8_", "BM_P7_"],
);

pub(crate) fn claim_p8_quality_execution() -> io::Result<ClaimedSealedExecution> {
    ClaimedSealedExecution::claim(P8_SEALED_EXECUTION_DOMAIN)
}

pub(crate) fn p8_quality_execution_domain() -> SealedExecutionDomain {
    P8_SEALED_EXECUTION_DOMAIN
}

pub(crate) fn role_self_test_stdout(role: &str) -> Option<&'static [u8]> {
    match role {
        "source_publisher" => Some(P8_SOURCE_PUBLISHER_SELF_TEST_STDOUT),
        "quality_runner" => Some(P8_QUALITY_RUNNER_SELF_TEST_STDOUT),
        "quality_operator" => Some(P8_QUALITY_OPERATOR_SELF_TEST_STDOUT),
        "trusted_supervisor" => Some(P8_TRUSTED_SUPERVISOR_SELF_TEST_STDOUT),
        _ => None,
    }
}

pub(crate) fn run_role_entry(
    role_self_test_stdout: &'static [u8],
    supports_engineering_gate: bool,
) -> io::Result<()> {
    let mut authority = claim_p8_quality_execution()?;
    authority.verify()?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let stdout = match args.as_slice() {
        [arg] if arg == P8_ROLE_SELF_TEST_ARG => role_self_test_stdout,
        [arg] if supports_engineering_gate && arg == P8_ENGINEERING_GATE_SELF_TEST_ARG => {
            P8_ENGINEERING_GATE_SEALED_STDOUT
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "P8 quality role command is not exact",
            ));
        }
    };
    io::stdout().lock().write_all(stdout)?;
    io::stdout().lock().flush()?;
    authority.verify()
}

pub(crate) fn exit_role_entry(result: io::Result<()>) -> ! {
    match result {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "{error}");
            std::process::exit(2);
        }
    }
}
