use bm_cli::run_cli;

const GATEWAY_URL: &str = "http://127.0.0.1:8787/v1";
const MCP_URL: &str = "http://127.0.0.1:8788/mcp";

#[test]
fn agent_rules_export_supports_all_planned_targets_without_memory_payloads() {
    let cases = [
        ("continue", OutputShape::Yaml),
        ("cline", OutputShape::Markdown),
        ("aider", OutputShape::Markdown),
        ("zed", OutputShape::Json),
        ("opencode", OutputShape::Json),
        ("open-webui", OutputShape::Python),
        ("vscode", OutputShape::Json),
    ];

    for (target, shape) in cases {
        let output = export_target(target).unwrap_or_else(|err| panic!("{target}: {err}"));
        assert!(
            output.contains(GATEWAY_URL),
            "{target} output must point model traffic at the gateway"
        );
        assert!(
            output.contains(MCP_URL),
            "{target} output must point explicit memory tools at MCP"
        );
        assert!(
            output.contains("memory_recall")
                && output.contains("memory_project")
                && output.contains("memory_write_candidate")
                && output.contains("memory_inspect"),
            "{target} output must describe the memory tool contract"
        );
        assert!(
            output.contains("Do not paste raw memory")
                || output.contains("do not paste raw memory"),
            "{target} output must include the no-raw-memory constraint"
        );
        assert_no_memory_payload(&output, target);

        match shape {
            OutputShape::Json => {
                serde_json::from_str::<serde_json::Value>(&output)
                    .unwrap_or_else(|err| panic!("{target} must be JSON: {err}\n{output}"));
            }
            OutputShape::Yaml => {
                assert!(output.contains("models:"));
                assert!(output.contains("mcpServers:"));
            }
            OutputShape::Markdown => {
                assert!(output.starts_with("# "));
                assert!(output.contains("## Gateway"));
            }
            OutputShape::Python => {
                assert!(output.contains("class Filter"));
                assert!(output.contains("async def inlet"));
                assert!(output.contains("async def outlet"));
            }
        }
    }
}

#[test]
fn agent_rules_export_reports_unsupported_target_without_panic() {
    let err = export_target("roo").expect_err("roo is outside this Cut G target set");

    assert!(err.contains("unsupported agent rules target: roo"));
    assert!(err.contains("continue"));
    assert!(err.contains("open-webui"));
    assert!(err.contains("vscode"));
}

fn export_target(target: &str) -> Result<String, String> {
    run_cli(
        [
            "agent-rules",
            "export",
            "--target",
            target,
            "--gateway-url",
            GATEWAY_URL,
            "--mcp-url",
            MCP_URL,
        ]
        .into_iter()
        .map(str::to_string),
    )
}

fn assert_no_memory_payload(output: &str, target: &str) {
    for forbidden in [
        "private_garden_raw",
        "subject_state_raw",
        "soul_governance_raw",
        "projection-preview content",
        "real memory content",
        "raw memory content",
        "memory payload",
    ] {
        assert!(
            !output.contains(forbidden),
            "{target} output must not contain forbidden memory payload marker {forbidden}"
        );
    }
}

#[derive(Clone, Copy)]
enum OutputShape {
    Json,
    Yaml,
    Markdown,
    Python,
}
