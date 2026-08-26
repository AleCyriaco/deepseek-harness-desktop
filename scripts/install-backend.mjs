// Installs the DeepSeek Harness runtime into `backend/node_modules` so the
// Rust shell can find `backend/node_modules/@deepseek-ai/dsh/lib/bin.js`.
//
// Run from the repository root:  npm run backend:install
//
// Prefers pnpm, then bun, then npm.
//
// Two things here are correctness requirements, not preferences:
//
//   * npm must NOT be given `--legacy-peer-deps`. The harness declares real
//     peer dependencies — `@deepseek-ai/dsh-app-boot` needs
//     `@deepseek-ai/cordis-plugin-group` — and that flag skips them, producing
//     a tree that installs cleanly and then dies at runtime with
//     ERR_MODULE_NOT_FOUND. Slower is fine; wrong is not.
//   * pnpm must use the hoisted linker. Its default layout is a farm of
//     symlinks into a content store, which the portable build's packer cannot
//     follow.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const backend = join(root, "backend");

function has(cmd) {
  const probe = spawnSync(cmd, ["--version"], { stdio: "ignore", shell: process.platform === "win32" });
  return probe.error === undefined && probe.status === 0;
}

let command;
let args;
if (has("pnpm")) {
  command = "pnpm";
  args = ["install", "--no-frozen-lockfile", "--node-linker=hoisted"];
} else if (has("bun")) {
  command = "bun";
  args = ["install"];
} else {
  command = "npm";
  args = ["install", "--no-audit", "--no-fund"];
}

console.log(`[dsh-desktop] installing backend runtime with ${command} in ${backend}`);

const result = spawnSync(command, args, {
  cwd: backend,
  stdio: "inherit",
  shell: process.platform === "win32",
});

if (result.error) {
  console.error(`[dsh-desktop] failed to run ${command}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
