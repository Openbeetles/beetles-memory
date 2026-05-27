# Memory Benchmark Wall Fixtures

These fixtures define the W1 Breakthrough Benchmark Wall baseline. They are
contract baselines: each case states the owner path, evidence refs, expected
surface, current measured/declared baseline metrics, and thresholds. Runtime
replay and judge-backed evaluations must extend this schema instead of replacing
it.

- `compact-baseline.json` files use `esp_standalone_memory`.
- `full-baseline.json` files use `server_linux_dev_full`.
- Every benchmark class must have both compact and full coverage.
