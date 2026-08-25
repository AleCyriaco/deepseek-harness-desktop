// Installs the DeepSeek Harness runtime into `backend/node_modules` so the
// Rust shell can find `backend/node_modules/@deepseek-ai/dsh/lib/bin.js`.
//
// Run from the repository root:  npm run backend:install
//
// Prefers pnpm (much faster on this package's large peer-dependency graph),
// then bun, then npm. npm is run with `--legacy-peer-deps` to avoid the very
// slow ideal-tree resolution the harness's peer dependencies can trigger.
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
  args = ["install", "--no-frozen-lockfile"];
} else if (has("bun")) {
  command = "bun";
  args = ["install"];
} else {
  command = "npm";
  args = ["install", "--no-audit", "--no-fund", "--legacy-peer-deps"];
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
