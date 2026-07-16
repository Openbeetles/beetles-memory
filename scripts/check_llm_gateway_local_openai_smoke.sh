#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "${BM_LLM_GATEWAY_SMOKE:-0}" != "1" ]]; then
  echo "check_llm_gateway_local_openai_smoke: skipped (set BM_LLM_GATEWAY_SMOKE=1)"
  exit 0
fi

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

command -v curl >/dev/null || fail "curl is required for local OpenAI-compatible smoke"

case "$(uname -s)" in
  Darwin) production_feature="profile-desktop-macos-standalone-memory" ;;
  Linux) production_feature="profile-server-linux-memory-gateway" ;;
  *) fail "bm-llm-gateway has no production profile for this smoke target" ;;
esac

upstream_base="${BM_LLM_GATEWAY_OPENAI_BASE_URL:-http://127.0.0.1:8000/v1}"
bind_addr="${BM_LLM_GATEWAY_BIND:-127.0.0.1:18787}"
model="${BM_LLM_GATEWAY_SMOKE_MODEL:-local}"
gateway_base="http://${bind_addr}"
log_file="${TMPDIR:-/tmp}/bm-llm-gateway-smoke.log"

auth_args=()
gateway_env=(
  "BM_LLM_GATEWAY_BIND=${bind_addr}"
  "BM_LLM_GATEWAY_OPENAI_BASE_URL=${upstream_base}"
)
if [[ -n "${BM_LLM_GATEWAY_OPENAI_API_KEY_ENV:-}" ]]; then
  api_key="${!BM_LLM_GATEWAY_OPENAI_API_KEY_ENV:-}"
  [[ -n "$api_key" ]] || fail "BM_LLM_GATEWAY_OPENAI_API_KEY_ENV points to an empty env"
  auth_args=(-H "Authorization: Bearer ${api_key}")
  gateway_env+=("BM_LLM_GATEWAY_OPENAI_API_KEY_ENV=${BM_LLM_GATEWAY_OPENAI_API_KEY_ENV}")
fi

curl -fsS -m 2 "${auth_args[@]}" "${upstream_base%/}/models" >/dev/null \
  || fail "OpenAI-compatible upstream is not reachable at ${upstream_base}"

env "${gateway_env[@]}" cargo run --locked -p bm-llm-gateway --no-default-features \
  --features "server-async,client-reqwest,$production_feature" \
  >"$log_file" 2>&1 &
gateway_pid=$!
trap 'kill "$gateway_pid" >/dev/null 2>&1 || true' EXIT

for _ in {1..80}; do
  if curl -fsS -m 1 "${gateway_base}/v1/models" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$gateway_pid" >/dev/null 2>&1; then
    cat "$log_file" >&2 || true
    fail "bm-llm-gateway exited before becoming ready"
  fi
  sleep 0.25
done

curl -fsS -m 2 "${gateway_base}/v1/models" | rg '"data"' >/dev/null \
  || fail "/v1/models smoke did not return an OpenAI-compatible model list"

chat_body="$(printf '{"model":"%s","messages":[{"role":"user","content":"ping"}],"stream":false}' "$model")"
curl -fsS -m 15 \
  -H 'content-type: application/json' \
  -H 'x-bm-conversation-id: local-smoke' \
  -d "$chat_body" \
  "${gateway_base}/v1/chat/completions" | rg '"choices"' >/dev/null \
  || fail "/v1/chat/completions smoke returned an unexpected payload"

echo "check_llm_gateway_local_openai_smoke: ok"
