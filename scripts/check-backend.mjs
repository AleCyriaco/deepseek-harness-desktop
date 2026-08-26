// Smoke-tests the installed harness runtime: starts it exactly the way the
// desktop shell does and waits for it to announce its loopback URL.
//
//   node scripts/check-backend.mjs
//
// This exists because a tree can install cleanly and still be unusable. A
// missing peer dependency shipped in v0.1.3 and only surfaced as
// ERR_MODULE_NOT_FOUND on a user's machine — `npm install` reported success
// throughout. Running the thing is the only check that means anything.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const entry = join(
  root,
  "backend/node_modules/@deepseek-ai/dsh/lib/bin.js".replace(/\//g, "/"),
);
const TIMEOUT_MS = 120_000;

if (!existsSync(entry)) {
  console.error(`[check-backend] not installed: ${entry}`);
  console.error("[check-backend] run `npm run backend:install` first");
  process.exit(1);
}

const child = spawn(process.execPath, [entry, "web", "--no-open", "--port", "0"], {
  stdio: ["ignore", "pipe", "pipe"],
});

const output = [];
let settled = false;

function finish(code, message) {
  if (settled) return;
  settled = true;
  clearTimeout(timer);
  child.kill("SIGKILL");
  if (code === 0) {
    console.log(`[check-backend] ${message}`);
  } else {
    console.error(`[check-backend] ${message}`);
    console.error(output.join("").trimEnd() || "(the backend produced no output)");
  }
  process.exit(code);
}

const timer = setTimeout(
  () => finish(1, `no URL announced within ${TIMEOUT_MS / 1000}s`),
  TIMEOUT_MS,
);

for (const stream of [child.stdout, child.stderr]) {
  stream.setEncoding("utf8");
  stream.on("data", (chunk) => {
    output.push(chunk);
    const match = /http:\/\/127\.0\.0\.1:(\d+)/.exec(chunk);
    if (match) finish(0, `backend ready on port ${match[1]}`);
  });
}

child.on("error", (error) => finish(1, `could not start the backend: ${error.message}`));
child.on("exit", (code) => finish(1, `backend exited with code ${code} before announcing a URL`));
