#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
  cat >&2 <<'EOF'
Usage:
  bash scripts/emit_platform_capability_snapshots.sh --check
  bash scripts/emit_platform_capability_snapshots.sh --write
EOF
}

mode="${1:-}"
case "$mode" in
  --check|--write) ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

profiles=(
  profile-esp-standalone-memory
  profile-esp-embedded-sdk
  profile-linux-device-standalone-memory
  profile-desktop-macos-embedded-sdk
  profile-desktop-windows-embedded-sdk
  profile-server-linux-memory-gateway
  profile-server-linux-dev-full
)

snapshot_dir="fixtures/platform/capabilities"
mkdir -p "$snapshot_dir"

changed=0
for profile in "${profiles[@]}"; do
  path="$snapshot_dir/$profile.json"
  rendered="$(
    cargo run -q -p bm-cli --bin bm --no-default-features --features "$profile" -- \
      platform capability-snapshot --profile "$profile"
  )"

  if [[ "$mode" == "--write" ]]; then
    printf '%s\n' "$rendered" > "$path"
    continue
  fi

  if [[ ! -f "$path" ]]; then
    echo "missing platform capability snapshot fixture: $path" >&2
    changed=1
    continue
  fi

  if ! diff -u "$path" <(printf '%s\n' "$rendered") >/tmp/bm-platform-snapshot-diff.$$; then
    echo "platform capability snapshot drifted: $profile" >&2
    cat /tmp/bm-platform-snapshot-diff.$$ >&2
    changed=1
  fi
  rm -f /tmp/bm-platform-snapshot-diff.$$
done

if [[ "$changed" -ne 0 ]]; then
  exit 1
fi

if [[ "$mode" == "--write" ]]; then
  echo "OK: platform capability snapshots written"
else
  echo "OK: platform capability snapshots match fixtures"
fi
