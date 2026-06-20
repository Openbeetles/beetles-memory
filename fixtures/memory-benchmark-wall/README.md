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
- `w4_eval_recall_semantics`: requires W4 eval recall report separation across
  source, graph anchor, expanded, eval candidate pool, selected, and rendered
  layers, with recall@k, MRR, missing evidence refs, W4.1 first-hit / missing /
  gold-rank diagnostics, candidate-to-evidence refs, and persistent graph
  evidence gate coverage.

W4 external LoCoMo / LongMemEval raw datasets and local runner outputs are not
stored in this fixture directory. They remain local benchmark inputs outside the
project repository; in-repo W4 fixtures only prove the benchmark-wall contract
shape and SDK-owned report semantics.

The explicit W4 external noisy wall operator is `scripts/check_w4_external_noisy_wall_operator.sh`.
It requires `BM_W4_EXTERNAL_BENCH_ROOT` and reads only the external merged summary
files plus runner source to attach hash provenance before calling `bm-replay`.
External merged summaries must include `stage_hit_counts`, `index_diagnostics`,
and `w4_1_diagnostics` before they can explain whether a noisy split failed in
source recall, graph expansion, rerank, selection, render, production index, or
W4.1 first-hit / missing / gold-rank coverage. Missing diagnostics are hard W4
external wall blockers, not release-ready baselines. The operator is not part of
the default fixture wall and must not copy external data here.
