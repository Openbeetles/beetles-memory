#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const SCHEMA = "beetle-memory.platform.target-gates.v3";

const EXPECTED_ROWS = [
  {
    profile: "profile-esp-standalone-memory",
    target: "xtensa-esp32s3-espidf",
    package: "bm-sdk",
    features: ["profile-esp-standalone-memory"],
    replay_all_targets: false,
    gateway_check: false,
  },
  {
    profile: "profile-esp-embedded-sdk",
    target: "xtensa-esp32s3-espidf",
    package: "bm-sdk",
    features: ["profile-esp-embedded-sdk"],
    replay_all_targets: false,
    gateway_check: false,
  },
  {
    profile: "profile-linux-device-standalone-memory",
    target: "aarch64-unknown-linux-gnu",
    package: "bm-sdk",
    features: ["profile-linux-device-standalone-memory"],
    replay_all_targets: false,
    gateway_check: false,
  },
  {
    profile: "profile-desktop-macos-standalone-memory",
    target: "aarch64-apple-darwin",
    package: "bm-sdk",
    features: ["profile-desktop-macos-standalone-memory"],
    replay_all_targets: false,
    gateway_check: true,
  },
  {
    profile: "profile-desktop-macos-embedded-sdk",
    target: "aarch64-apple-darwin",
    package: "bm-sdk",
    features: ["profile-desktop-macos-embedded-sdk"],
    replay_all_targets: false,
    gateway_check: false,
  },
  {
    profile: "profile-desktop-macos-dev-full",
    target: "aarch64-apple-darwin",
    package: "bm-sdk",
    features: ["profile-desktop-macos-dev-full"],
    replay_all_targets: false,
    gateway_check: false,
  },
  {
    profile: "profile-desktop-linux-embedded-sdk",
    target: "x86_64-unknown-linux-gnu",
    package: "bm-sdk",
    features: ["profile-desktop-linux-embedded-sdk"],
    replay_all_targets: false,
    gateway_check: false,
  },
  {
    profile: "profile-server-linux-memory-gateway",
    target: "x86_64-unknown-linux-gnu",
    package: "bm-sdk",
    features: ["profile-server-linux-memory-gateway"],
    replay_all_targets: false,
    gateway_check: true,
  },
  {
    profile: "profile-desktop-windows-embedded-sdk",
    target: "x86_64-pc-windows-msvc",
    package: "bm-sdk",
    features: ["profile-desktop-windows-embedded-sdk"],
    replay_all_targets: false,
    gateway_check: false,
  },
  {
    profile: "profile-desktop-windows-dev-full",
    target: "x86_64-pc-windows-msvc",
    package: "bm-sdk",
    features: ["profile-desktop-windows-dev-full"],
    replay_all_targets: true,
    gateway_check: false,
  },
  {
    profile: "profile-server-linux-dev-full",
    target: "x86_64-unknown-linux-gnu",
    package: "bm-sdk",
    features: ["profile-server-linux-dev-full"],
    replay_all_targets: true,
    gateway_check: true,
  },
];

const GATE_KEYS = [
  "build_std",
  "c_archiver",
  "c_compiler",
  "c_linker",
  "c_toolchain_kind",
  "executor_kind",
  "features",
  "gateway_check",
  "package",
  "profile",
  "replay_all_targets",
  "rust_toolchain",
  "target",
  "target_std_mode",
];

const safeScalar = (value) =>
  typeof value === "string" &&
  /^[A-Za-z0-9][A-Za-z0-9._+/-]*$/.test(value);

const same = (left, right) => JSON.stringify(left) === JSON.stringify(right);

export function validateTargetGateFixture(gates) {
  if (
    gates === null ||
    typeof gates !== "object" ||
    Array.isArray(gates) ||
    !same(Object.keys(gates).sort(), ["gates", "schema"])
  ) {
    throw new Error("target gate fixture has a noncanonical top-level field set");
  }
  if (gates.schema !== SCHEMA) {
    throw new Error(`unexpected target gate schema: ${gates.schema}`);
  }
  if (!Array.isArray(gates.gates)) {
    throw new Error("target gate fixture gates must be an array");
  }
  if (gates.gates.length !== EXPECTED_ROWS.length) {
    throw new Error(
      `target gate fixture must contain exactly ${EXPECTED_ROWS.length} rows`,
    );
  }

  for (const [index, gate] of gates.gates.entries()) {
    const expected = EXPECTED_ROWS[index];
    if (
      gate === null ||
      typeof gate !== "object" ||
      Array.isArray(gate) ||
      !same(Object.keys(gate).sort(), GATE_KEYS)
    ) {
      throw new Error(`target gate row ${index} has a noncanonical field set`);
    }

    const identity = {
      profile: gate.profile,
      target: gate.target,
      package: gate.package,
      features: gate.features,
      replay_all_targets: gate.replay_all_targets,
      gateway_check: gate.gateway_check,
    };
    if (!same(identity, expected)) {
      throw new Error(
        `target gate row ${index} does not match canonical identity/order`,
      );
    }

    if (
      !safeScalar(gate.rust_toolchain) ||
      !["rustup_component", "build_std"].includes(gate.target_std_mode) ||
      !Array.isArray(gate.build_std) ||
      gate.build_std.some((part) => !safeScalar(part)) ||
      !["cargo", "cargo_xwin"].includes(gate.executor_kind) ||
      !["none", "gnu_glibc", "xwin_msvc"].includes(gate.c_toolchain_kind)
    ) {
      throw new Error(`target gate ${gate.profile} has an invalid typed contract`);
    }
    if (
      (gate.target_std_mode === "build_std") !== (gate.build_std.length > 0)
    ) {
      throw new Error(
        `target gate ${gate.profile} has inconsistent target std fields`,
      );
    }

    const tools = [gate.c_compiler, gate.c_archiver, gate.c_linker];
    if (gate.c_toolchain_kind === "none") {
      if (gate.executor_kind !== "cargo" || tools.some((tool) => tool !== null)) {
        throw new Error(
          `target gate ${gate.profile} has inconsistent no-C-toolchain fields`,
        );
      }
    } else if (gate.c_toolchain_kind === "gnu_glibc") {
      if (
        gate.executor_kind !== "cargo" ||
        tools.some((tool) => !safeScalar(tool))
      ) {
        throw new Error(
          `target gate ${gate.profile} has inconsistent GNU toolchain fields`,
        );
      }
    } else if (
      gate.executor_kind !== "cargo_xwin" ||
      gate.target !== "x86_64-pc-windows-msvc" ||
      gate.target_std_mode !== "rustup_component" ||
      !same(tools, ["clang-cl", "llvm-lib", "lld-link"])
    ) {
      throw new Error(
        `target gate ${gate.profile} has inconsistent xwin toolchain fields`,
      );
    }
  }

  return gates.gates;
}

export function renderTargetGateRows(gates) {
  return validateTargetGateFixture(gates)
    .map((gate) =>
      [
        gate.profile,
        gate.target,
        gate.package,
        gate.features.join(","),
        gate.rust_toolchain,
        gate.target_std_mode,
        gate.build_std.length === 0 ? "-" : gate.build_std.join(","),
        gate.executor_kind,
        gate.c_toolchain_kind,
        gate.c_compiler ?? "-",
        gate.c_archiver ?? "-",
        gate.c_linker ?? "-",
        gate.replay_all_targets ? "true" : "false",
        gate.gateway_check ? "true" : "false",
      ].join("\t"),
    )
    .join("\n");
}

export function renderMissingTargetReport({
  profile,
  target,
  package: packageName,
  features,
  rustToolchain,
  targetStdMode,
  buildStd,
  executorKind,
  cToolchainKind,
  reason,
}) {
  const scalarFields = [
    profile,
    target,
    packageName,
    features,
    rustToolchain,
    targetStdMode,
    executorKind,
    cToolchainKind,
    reason,
  ];
  if (
    scalarFields.some((value) => !safeScalar(value)) ||
    !/^(-|[A-Za-z0-9_]+(?:,[A-Za-z0-9_]+)*)$/.test(buildStd) ||
    !["cargo", "cargo_xwin"].includes(executorKind) ||
    ![
      "rust_toolchain_unavailable",
      "target_std_unavailable",
      "c_toolchain_unavailable",
    ].includes(reason)
  ) {
    throw new Error("invalid missing target report input");
  }

  const command = ["cargo", `+${rustToolchain}`];
  if (executorKind === "cargo_xwin") {
    command.push("xwin");
  }
  command.push("check", "--locked");
  if (buildStd !== "-") {
    command.push(`-Zbuild-std=${buildStd}`);
  }
  command.push(
    "-p",
    packageName,
    "--target",
    target,
    "--no-default-features",
    "--features",
    features,
  );

  return JSON.stringify({
    schema: "beetle-memory.platform.target-gate.v3",
    status: "missing_toolchain",
    reason,
    target,
    profile,
    rust_toolchain: rustToolchain,
    target_std_mode: targetStdMode,
    executor_kind: executorKind,
    c_toolchain_kind: cToolchainKind,
    required_command: command.join(" "),
  });
}

function main() {
  if (process.argv[2] === "--missing-report") {
    if (process.argv.length !== 13) {
      throw new Error(
        "usage: node scripts/validate_target_gate_fixture.mjs --missing-report <profile> <target> <package> <features> <rust-toolchain> <target-std-mode> <build-std> <executor-kind> <c-toolchain-kind> <reason>",
      );
    }
    process.stdout.write(
      `${renderMissingTargetReport({
        profile: process.argv[3],
        target: process.argv[4],
        package: process.argv[5],
        features: process.argv[6],
        rustToolchain: process.argv[7],
        targetStdMode: process.argv[8],
        buildStd: process.argv[9],
        executorKind: process.argv[10],
        cToolchainKind: process.argv[11],
        reason: process.argv[12],
      })}\n`,
    );
    return;
  }
  if (process.argv.length === 3) {
    const fixture = JSON.parse(readFileSync(process.argv[2], "utf8"));
    process.stdout.write(`${renderTargetGateRows(fixture)}\n`);
    return;
  }
  throw new Error(
    "usage: node scripts/validate_target_gate_fixture.mjs <fixture.json>",
  );
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  try {
    main();
  } catch (error) {
    process.stderr.write(
      `${error instanceof Error ? error.message : String(error)}\n`,
    );
    process.exitCode = 1;
  }
}
