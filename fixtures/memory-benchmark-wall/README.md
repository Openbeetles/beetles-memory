# Memory Benchmark Wall Fixtures

These fixtures define the W1 Breakthrough Benchmark Wall baseline. They are
contract baselines: each case states the owner path, evidence refs, expected
surface, current measured/declared baseline metrics, and thresholds. Runtime
replay and judge-backed evaluations must extend this schema instead of replacing
it.

- `compact-baseline.json` files use `esp_standalone_memory`.
- `full-baseline.json` files use `server_linux_dev_full`.
- Every benchmark class must have both compact and full coverage.

Phase 0 inhabited-subject fixtures extend the baseline schema with
`semantic_contract`. That contract records gate dimensions, required keys,
forbidden keys, required semantic markers, and forbidden semantic markers. The
Rust benchmark wall owns the structured validation; shell scripts only ensure the
fixture set and marker vocabulary are present.

The current Phase 0 dimensions are:

- `projection_shape`: requires `Subject Mount`, boundary/disclosure protocol,
  work integrity covenant, and source-backed ownership instead of a flat public
  list of internal memory sections.
- `privacy_runtime_semantics`: allows protected private runtime context for the
  active LLM while forbidding foreground raw private leakage and final second LLM
  privacy judgment in the hot path.
- `soul_life_semantics`: keeps life facets and self-owned update candidates in
  the soul-owned path without splitting the subject by model provider.
- `work_integrity_semantics`: preserves user task goals, exposes tool/evidence
  failures, and blocks theatrical substitution.
- `agent_tool_experience_semantics`: keeps host agent tools outside memory
  management, requires governed experience before returning tool hints, and
  forbids cold-start routing from tool descriptions or schemas.
