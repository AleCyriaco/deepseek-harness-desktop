// Installs the DeepSeek Harness runtime into `backend/node_modules` so the
// Rust shell can find `backend/node_modules/@deepseek-ai/dsh/lib/bin.js`.
//
// Run from the repository root:  npm run backend:install
//
// `backend/package-lock.json` is committed, and `npm ci` is used whenever it is
// present. That is not just about speed. Resolving this dependency graph from
// scratch is genuinely expensive — npm ran the GitHub macOS runner out of heap
// doing it — and the obvious workaround, `--legacy-peer-deps`, silently drops
// peer dependencies the harness actually needs, producing a tree that installs
// cleanly and dies at runtime. A lockfile avoids both: no resolution, and the
// exact set of packages that was tested.
//
// Without a lockfile it falls back to pnpm, then bun, then npm. pnpm is given
// the hoisted linker because its default symlink layout cannot be packed into
// the portable build.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const backend = join(root, "backend");
const windows = process.platform === "win32";

function has(cmd) {
  const probe = spawnSync(cmd, ["--version"], { stdio: "ignore", shell: windows });
  return probe.error === undefined && probe.status === 0;
}

function resolveInstaller() {
  if (existsSync(join(backend, "package-lock.json"))) {
    return { command: "npm", args: ["ci", "--no-audit", "--no-fund"] };
  }
  if (has("pnpm")) {
    return { command: "pnpm", args: ["install", "--no-frozen-lockfile", "--node-linker=hoisted"] };
  }
  if (has("bun")) {
    return { command: "bun", args: ["install"] };
  }
  return { command: "npm", args: ["install", "--no-audit", "--no-fund"] };
}

const { command, args } = resolveInstaller();
console.log(`[dsh-desktop] installing backend runtime with ${command} ${args[0]} in ${backend}`);

const result = spawnSync(command, args, {
  cwd: backend,
  stdio: "inherit",
  shell: windows,
  // Resolution is memory-hungry on this graph; harmless for `npm ci`.
  env: { ...process.env, NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ""} --max-old-space-size=6144`.trim() },
});

if (result.error) {
  console.error(`[dsh-desktop] failed to run ${command}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
