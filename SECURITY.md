# Security

## Reporting a vulnerability

Report suspected vulnerabilities privately through
[GitHub Security Advisories](https://github.com/AleCyriaco/dsh-desktop/security/advisories/new)
rather than in a public issue. Please include reproduction steps and the
affected version, and give the maintainer a reasonable window to respond before
disclosing publicly.

Vulnerabilities in the **DeepSeek Harness itself** — the agent runtime, its
tools, its web UI — belong upstream at
<https://github.com/deepseek-ai/deepseek-harness>. This repository contains only
the desktop shell.

## Security model

The shell's whole attack surface is: which process it starts, and what the
window is allowed to load.

| Property | Value |
|---|---|
| Network exposure | The shell opens no sockets. The server it starts binds `127.0.0.1` on an OS-assigned port. |
| Window origin | Only `http://127.0.0.1:<port>` — the URL announced by the process the shell itself started. |
| Native API exposure | None. `withGlobalTauri: false`, no IPC commands registered, capabilities limited to `core:default`. |
| Persisted secrets | None. The shell stores nothing; credentials live in the harness. |
| Privileges | Runs entirely as the invoking user; no elevation, no setuid, no helper daemon. |

### Content Security Policy

`app.security.csp` is `null` — no CSP is applied. This is a deliberate
trade-off:

- The harness UI evaluates client-side Cordis plugin code and opens a WebSocket
  to its own loopback origin. A `script-src` policy strict enough to be
  meaningful would break plugin evaluation, and `connect-src` would have to
  allow the dynamic port anyway.
- The mitigating factor is the origin: the window loads exactly one URL, on
  loopback, served by a process this app started moments earlier. It is not a
  general-purpose browser and never navigates to third-party content.

What this means concretely: **the shell inherits the harness UI's trust
model.** If the harness renders untrusted content, no CSP configured here would
have contained it, because the plugin system requires evaluation privileges by
design. Treat the harness as the security boundary.

### Untrusted input

The only input the shell parses is the backend's stdout. `extract_url` accepts
nothing but `http://127.0.0.1:` followed by ASCII digits — a malicious line
cannot redirect the window to another host or port scheme. The resulting string
is then parsed by the `url` crate before the window is built.

`DSH_DESKTOP_BACKEND` is treated as a trusted local path: it is a single path,
never shell-parsed, but anyone who can set it can already run arbitrary code as
that user.

### Process teardown

Closing the window signals the backend's whole **process group**, so agent tool
subprocesses do not survive the app. The one case this cannot cover is the app
being `SIGKILL`ed, where no handler runs; see
[docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md#a-server-keeps-running-after-closing-the-app)
for how to spot and stop an orphan.

## Supply chain

- The Rust side depends on `tauri`, `serde`, `serde_json`, `url` and (on Unix)
  `libc`, pinned by `src-tauri/Cargo.lock`.
- The harness runtime is installed from npm at build time and is **not** vendored
  into this repository.
- `backend/package.json` uses a semver range with no lockfile, so
  `npm run backend:install` picks up the newest matching release. Pin an exact
  version there if your threat model requires reproducible backend installs.
- The `npx --yes @deepseek-ai/dsh@latest` fallback downloads and executes the
  latest published harness. Convenient, but it is a network-fetch-and-run path:
  avoid it in hardened environments by installing the backend explicitly or
  setting `DSH_DESKTOP_BACKEND`.

## Distribution

Builds produced from this repository are **unsigned** by default. Signing and
notarization identities are not included and must be supplied by whoever
distributes a build. A bundle you did not build yourself and that is not signed
by a party you trust should be treated as untrusted code.
