import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(scriptDir, "../../..");
const targetDir = path.join(workspaceRoot, "target");
const sidecarDir = path.join(targetDir, "release");
const gatewayName = process.platform === "win32" ? "bm-llm-gateway.exe" : "bm-llm-gateway";

function run(command, args, options = {}) {
  console.log(`[build-sidecars] ${command} ${args.join(" ")}`);
  execFileSync(command, args, {
    cwd: workspaceRoot,
    stdio: "inherit",
    env: {
      ...process.env,
      CARGO_TARGET_DIR: targetDir,
      ...options.env,
    },
  });
}

function capture(command, args) {
  return execFileSync(command, args, {
    cwd: workspaceRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      CARGO_TARGET_DIR: targetDir,
    },
  });
}

function targetTriple() {
  if (process.env.TAURI_ENV_TARGET_TRIPLE) return process.env.TAURI_ENV_TARGET_TRIPLE;
  if (process.env.CARGO_BUILD_TARGET) return process.env.CARGO_BUILD_TARGET;

  const version = capture("rustc", ["-vV"]);
  const host = version
    .split("\n")
    .find((line) => line.startsWith("host: "))
    ?.slice("host: ".length)
    .trim();
  if (!host) throw new Error("failed to resolve rustc host target triple");
  return host;
}

run("npm", ["--prefix", path.join(workspaceRoot, "apps/console"), "run", "build"]);
if (process.platform !== "darwin") {
  throw new Error(
    `desktop release sidecars have no governed production profile for ${process.platform}`,
  );
}
const triple = targetTriple();
if (!triple.endsWith("-apple-darwin")) {
  throw new Error(`macOS standalone sidecar profile cannot build target ${triple}`);
}
run("cargo", [
  "build",
  "--locked",
  "-p",
  "bm-llm-gateway",
  "--no-default-features",
  "--features",
  "server-async,client-reqwest,profile-desktop-macos-standalone-memory",
  "--release",
  "--target",
  triple,
]);

mkdirSync(sidecarDir, { recursive: true });
const source = path.join(targetDir, triple, "release", gatewayName);
const sidecar = path.join(
  sidecarDir,
  process.platform === "win32"
    ? `bm-llm-gateway-${triple}.exe`
    : `bm-llm-gateway-${triple}`,
);

copyFileSync(source, sidecar);
chmodSync(sidecar, statSync(source).mode | 0o111);
console.log(`[build-sidecars] prepared ${path.relative(workspaceRoot, sidecar)}`);
