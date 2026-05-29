#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo test -p bm-core agent_skill
cargo test -p bm-sdk --test agent_skill_directory_contract
cargo test -p bm-sdk --test skill_management_contract
cargo test -p bm-sdk --test public_surface public_skill_surface_does_not_expose_memory_owned_agent_skill_crud
cargo test -p bm-entry --test console_skill_contract
cargo test -p bm-cli --test cli_contract memory_cli_skill_management_uses_entry_runtime_facade
cargo test -p bm-http --features server-std --test http_console_contract console_http_skill_routes_edit_runtime_skills_without_store_shortcut

for needle in \
  "MemorySkillOrigin" \
  "MemorySkillKind" \
  "MemorySkillUpsertRequest" \
  "EntryConsoleSkillUpsert" \
  "console_upsert_skill" \
  "upsertSkill" \
  "skill-import" \
  "user_provided" \
  "ManualDocument" \
  "manual_document"; do
  ! rg -q "$needle" crates/core/src/skills crates/sdk/src crates/entry/src crates/http/src crates/cli/src apps/console/src docs
done

rg -q "AgentSkillDirConfig" crates/core/src/skills crates/sdk/src crates/sdk/tests
rg -q "agent_skill_hits" crates/sdk/src crates/sdk/tests
rg -q "agent_skill_hints" crates/sdk/src crates/sdk/tests
rg -q "BM_AGENT_SKILL_DIRS" crates/entry/src docs dev-docs
