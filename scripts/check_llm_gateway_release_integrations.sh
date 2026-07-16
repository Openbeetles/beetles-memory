#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/lib_contract_checks.sh"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

skip() {
  echo "SKIP $1: $2"
}

check_file() {
  local path="$1"
  [[ -s "$path" ]] || fail "missing integration release surface file: $path"
}

check_file docs/en/llm-gateway-integrations.md
check_file docs/zh-CN/llm-gateway-integrations.md

rg -q 'llm-gateway-integrations.md' docs/README.md docs/en/README.md docs/zh-CN/README.md \
  || fail "LLM gateway integration docs are not indexed"

cargo test --locked -p bm-llm-gateway --test openai_responses_embeddings_probe_contract
cargo test --locked -p bm-cli --test agent_rules_cli
cargo test --locked -p bm-mcp --features server-stdio \
  --test mcp_stdio_contract \
  --test mcp_runtime_contract \
  --test mcp_http_contract \
  --test mcp_bin_contract

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/bm-llm-gateway-integrations.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

for target in continue cline aider zed opencode open-webui vscode; do
  output="$tmp_dir/${target}.out"
  cargo run --locked -q -p bm-cli -- agent-rules export \
    --target "$target" \
    --gateway-url http://127.0.0.1:8787/v1 \
    --mcp-url http://127.0.0.1:8788/mcp >"$output"
  rg -q '127.0.0.1:8787/v1' "$output" || fail "$target rules do not mention gateway URL"
  rg -q '127.0.0.1:8788/mcp' "$output" || fail "$target rules do not mention MCP URL"
  rg -q 'memory_recall' "$output" || fail "$target rules do not mention memory_recall"
  if rg -n 'private_garden_raw|subject_state_raw|soul_governance_raw|raw memory content|real memory content' "$output"; then
    fail "$target rules contain forbidden memory payload wording"
  fi
done

if command -v aider >/dev/null 2>&1; then
  echo "SMOKE aider: installed; configuration recipe generated"
else
  skip aider "aider is not installed"
fi

if command -v continue >/dev/null 2>&1; then
  echo "SMOKE continue: installed; configuration recipe generated"
else
  skip continue "continue CLI is not installed"
fi

if command -v cline >/dev/null 2>&1; then
  echo "SMOKE cline: installed; configuration recipe generated"
else
  skip cline "cline CLI is not installed"
fi

if command -v zed >/dev/null 2>&1; then
  echo "SMOKE zed: installed; configuration recipe generated"
else
  skip zed "zed CLI is not installed"
fi

if command -v opencode >/dev/null 2>&1; then
  echo "SMOKE opencode: installed; configuration recipe generated"
else
  skip opencode "opencode is not installed"
fi

if [[ -n "${BM_OPEN_WEBUI_URL:-}" ]]; then
  command -v curl >/dev/null 2>&1 || fail "curl is required when BM_OPEN_WEBUI_URL is set"
  curl -fsS -m 2 "$BM_OPEN_WEBUI_URL" >/dev/null \
    && echo "SMOKE open-webui: reachable at $BM_OPEN_WEBUI_URL" \
    || fail "Open WebUI is not reachable at $BM_OPEN_WEBUI_URL"
else
  skip open-webui "BM_OPEN_WEBUI_URL is not set"
fi

if [[ "${BM_LLM_GATEWAY_OLLAMA_SMOKE:-0}" == "1" ]]; then
  command -v curl >/dev/null 2>&1 || fail "curl is required for Ollama native smoke"
  curl -fsS -m 2 "${BM_LLM_GATEWAY_OLLAMA_BASE_URL:-http://127.0.0.1:8787/api}/tags" >/dev/null \
    && echo "SMOKE ollama-native: reachable" \
    || fail "Ollama native gateway smoke failed"
else
  skip ollama-native "set BM_LLM_GATEWAY_OLLAMA_SMOKE=1 to run live Ollama native smoke"
fi

echo "check_llm_gateway_release_integrations: ok"
