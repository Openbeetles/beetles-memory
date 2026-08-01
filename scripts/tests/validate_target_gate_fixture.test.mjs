import { readFileSync } from "node:fs";
import test from "node:test";
import assert from "node:assert/strict";

import {
  renderMissingTargetReport,
  renderTargetGateRows,
  validateTargetGateFixture,
} from "../validate_target_gate_fixture.mjs";

const canonicalPath = new URL(
  "../../fixtures/platform/target-gates.json",
  import.meta.url,
);
const canonical = JSON.parse(readFileSync(canonicalPath, "utf8"));
const clone = (value) => JSON.parse(JSON.stringify(value));

test("canonical target gate fixture is the exact eleven-row matrix", () => {
  const rows = validateTargetGateFixture(clone(canonical));
  assert.equal(rows.length, 11);
  assert.equal(renderTargetGateRows(clone(canonical)).split("\n").length, 11);
});

test("legacy schema is rejected", () => {
  const fixture = clone(canonical);
  fixture.schema = "beetle-memory.platform.target-gates.v2";
  assert.throws(() => validateTargetGateFixture(fixture), /unexpected.*schema/);
});

test("missing row is rejected", () => {
  const fixture = clone(canonical);
  fixture.gates.pop();
  assert.throws(() => validateTargetGateFixture(fixture), /exactly 11 rows/);
});

test("extra row is rejected", () => {
  const fixture = clone(canonical);
  fixture.gates.push(clone(fixture.gates[0]));
  assert.throws(() => validateTargetGateFixture(fixture), /exactly 11 rows/);
});

test("duplicate row is rejected even when row count remains eleven", () => {
  const fixture = clone(canonical);
  fixture.gates[10] = clone(fixture.gates[0]);
  assert.throws(() => validateTargetGateFixture(fixture), /identity\/order/);
});

test("row order drift is rejected", () => {
  const fixture = clone(canonical);
  [fixture.gates[0], fixture.gates[1]] = [
    fixture.gates[1],
    fixture.gates[0],
  ];
  assert.throws(() => validateTargetGateFixture(fixture), /identity\/order/);
});

test("profile feature drift is rejected", () => {
  const fixture = clone(canonical);
  fixture.gates[0].features = ["profile-esp-embedded-sdk"];
  assert.throws(() => validateTargetGateFixture(fixture), /identity\/order/);
});

test("dev-full replay requirement is exact", () => {
  const fixture = clone(canonical);
  fixture.gates[9].replay_all_targets = false;
  assert.throws(() => validateTargetGateFixture(fixture), /identity\/order/);
});

test("xwin row cannot downgrade to plain cargo", () => {
  const fixture = clone(canonical);
  fixture.gates[8].executor_kind = "cargo";
  assert.throws(() => validateTargetGateFixture(fixture), /xwin toolchain/);
});

test("xwin row requires the exact LLVM command set", () => {
  const fixture = clone(canonical);
  fixture.gates[8].c_linker = "link.exe";
  assert.throws(() => validateTargetGateFixture(fixture), /xwin toolchain/);
});

test("command-bearing scalar quote and backslash are rejected", () => {
  for (const unsafe of ['stable"broken', String.raw`stable\broken`]) {
    const fixture = clone(canonical);
    fixture.gates[0].rust_toolchain = unsafe;
    assert.throws(
      () => validateTargetGateFixture(fixture),
      /invalid typed contract/,
    );
  }
});

test("missing toolchain report is serialized as parseable JSON", () => {
  const rendered = renderMissingTargetReport({
    profile: "profile-desktop-windows-embedded-sdk",
    target: "x86_64-pc-windows-msvc",
    package: "bm-sdk",
    features: "profile-desktop-windows-embedded-sdk",
    rustToolchain: "stable",
    targetStdMode: "rustup_component",
    buildStd: "-",
    executorKind: "cargo_xwin",
    cToolchainKind: "xwin_msvc",
    reason: "c_toolchain_unavailable",
  });
  const report = JSON.parse(rendered);
  assert.equal(report.schema, "beetle-memory.platform.target-gate.v3");
  assert.equal(report.executor_kind, "cargo_xwin");
  assert.equal(
    report.required_command,
    "cargo +stable xwin check --locked -p bm-sdk --target x86_64-pc-windows-msvc --no-default-features --features profile-desktop-windows-embedded-sdk",
  );
});
