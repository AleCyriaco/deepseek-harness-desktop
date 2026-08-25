# DeepSeek Harness Desktop

[![CI](https://github.com/AleCyriaco/dsh-desktop/actions/workflows/ci.yml/badge.svg)](https://github.com/AleCyriaco/dsh-desktop/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tauri v2](https://img.shields.io/badge/Tauri-v2-24C8DB.svg)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org)

A native, cross-platform desktop shell for the [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness), built with **Rust + Tauri v2**.

It launches the real `dsh web` server as a child process and hosts the exact same web GUI in a native window. **No harness logic is reimplemented**, and nothing about the agent runtime changes — you get the same sessions, settings, models and plugins as the browser version, in an app window with a dock icon.

| Platform | Webview engine | Bundle formats |
|---|---|---|
| macOS | WKWebView | `.app`, `.dmg` |
| Windows | WebView2 (Edge) | `.msi`, `.exe` (NSIS) |
| Linux | WebKitGTK | `.deb`, `.rpm`, AppImage |

---

## Table of contents

- [Why a shell instead of a port](#why-a-shell-instead-of-a-port)
- [How it works](#how-it-works)
- [Prerequisites](#prerequisites)
- [Quick start](#quick-start)
- [Building distributables](#building-distributables)
- [Shipping a self-contained app](#shipping-a-self-contained-app)
- [Backend resolution](#backend-resolution)
- [Configuration](#configuration)
- [Project layout](#project-layout)
- [Documentation](#documentation)
- [Design notes](#design-notes)
- [Contributing](#contributing)
- [License](#license)

---

## Why a shell instead of a port

The harness already ships a complete web GUI served by `dsh web`. Rewriting that UI natively would mean maintaining a second implementation of every screen, and it would drift from upstream the moment the harness shipped a feature.

This project takes the opposite approach: it is a **thin, boring wrapper**. Roughly 300 lines of Rust that:

1. find a `dsh` backend,
2. start it on a loopback port,
3. read the URL it announces,
4. point a native webview at it,
5. and reliably kill the whole process tree on exit.

Everything the user sees is upstream's UI. When the harness updates, the desktop app inherits the update for free.

## How it works

```
┌──────────────────────────────┐
│  Tauri window (native)       │
│  ┌────────────────────────┐  │
│  │  Webview               │  │   http://127.0.0.1:<port>
│  │  (DeepSeek Harness UI) │──┼──────────────────────────────┐
│  └────────────────────────┘  │                              │
└──────────────────────────────┘                              ▼
                                      ┌─────────────────────────────────┐
        Rust `backend.rs` spawns ───▶ │  `dsh web --no-open --port <p>` │
                                      │  (the stock harness server)     │
                                      └─────────────────────────────────┘
```

1. The Rust shell resolves a backend command (see [Backend resolution](#backend-resolution)).
2. It runs `dsh web --no-open --port 0` (`0` = the OS picks a free port), reading stdout for the line `dsh web: http://127.0.0.1:<port>`.
3. Once that line appears, it creates the main window pointed at the announced URL.
4. On window close / app exit it terminates the whole server **process group**, so no orphan server — and no orphan tool subprocess the agent spawned — survives the window.

A step-by-step walkthrough of the startup and shutdown sequences lives in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Prerequisites

- **Node.js ≥ 20** — the harness is a Node program; the shell shells out to it.
- **Rust** (stable) + Cargo — <https://rustup.rs>
- Tauri system prerequisites — <https://tauri.app/start/prerequisites/>
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  - **Windows**: WebView2 runtime + MSVC build tools
  - **Linux**: `webkit2gtk-4.1`, `libappindicator3`, `librsvg2`, `patchelf` (see the Tauri page for the exact package names on your distro)

Optional but recommended: **pnpm**, which installs the backend runtime considerably faster than npm on this dependency graph.

## Quick start

```sh
# 1. Install the shell's JS dependencies (just the Tauri CLI)
npm install

# 2. Install the bundled harness runtime into backend/node_modules
npm run backend:install

# 3. Run in dev mode (compiles the Rust shell, then opens the window)
npm run dev
```

Steps 1 and 2 are combined in `npm run setup`.

The first `npm run dev` compiles the full Tauri dependency tree and takes several minutes; subsequent runs are incremental and start in seconds. Once `src-tauri/target/debug/dsh-desktop` exists you can also launch that binary directly.

## Building distributables

```sh
npm run build
```

`tauri build` produces the platform package for the OS you are building on. **Multi-platform means building once per OS** — Tauri and Rust do not cross-compile the GUI bundles. The included GitHub Actions [release workflow](.github/workflows/release.yml) builds macOS, Windows and Linux from a matrix and attaches the artifacts to a GitHub Release.

## Shipping a self-contained app

By default the built app expects a `dsh` backend to be resolvable on the target machine. To ship an app that needs nothing but Node.js, bundle the backend runtime into the app resources — add this to `src-tauri/tauri.conf.json`:

```json
"bundle": {
  "resources": {
    "../backend/node_modules/": "backend/node_modules/"
  }
}
```

Then run `npm run backend:install` before `npm run build`. The shell already searches the packaged `resource_dir` for `backend/node_modules/@deepseek-ai/dsh/lib/bin.js`, so the bundled copy is found automatically. Without this step, the app falls back to `dsh` on `PATH` or `npx`.

> The bundled runtime is ~270 MB on disk, which is why it is opt-in rather than the default.

## Backend resolution

The shell picks the **first available** backend, in this order:

| # | Source | Command it builds |
|---|---|---|
| 1 | `DSH_DESKTOP_BACKEND` env var | the given executable, or `node <given .js/.mjs/.cjs>` |
| 2 | `<root>/backend/node_modules/@deepseek-ai/dsh/lib/bin.js` | `node <that path>` |
| 3 | `dsh` on `PATH` | `dsh` |
| 4 | `npx` on `PATH` | `npx --yes @deepseek-ai/dsh@latest` |

`<root>` is searched across the packaged resource directory, up to six ancestors of the executable, the current working directory, and `$CARGO_MANIFEST_DIR/..` — so the same lookup works for a dev build, a `cargo run`, and a packaged `.app`.

If none of the four resolve, the app prints a diagnostic to stderr and exits with status `1`.

## Configuration

| Variable | Default | Effect |
|---|---|---|
| `DSH_DESKTOP_BACKEND` | *(unset)* | Override the backend command — an absolute path to a `dsh` executable, or to a `.js`/`.mjs`/`.cjs` entry point that will be run through `node`. |
| `DSH_DESKTOP_PORT` | `0` | Pin the server port. `0` lets the OS assign a free one, so two copies never collide. |

Everything else — API keys, models, sessions, plugins — is configured **inside the harness UI** and stored by the harness itself, exactly as in the browser. The shell holds no configuration of its own. See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) for the full reference.

## Project layout

```
src-tauri/                 Rust (Tauri) shell
  src/main.rs              binary entry point
  src/lib.rs               app setup, window creation, exit handling
  src/backend.rs           backend resolution, spawn, URL discovery, teardown
  capabilities/default.json Tauri permission set for the main window
  tauri.conf.json          app config (external webview, CSP, bundle targets)
  icons/                   generated platform icons
backend/                   the harness runtime (installed by `npm run backend:install`)
src/index.html             minimal fallback page; the real UI is served by dsh
scripts/
  install-backend.mjs      installs @deepseek-ai/dsh into backend/
  make-icon.mjs            regenerates app-icon.png (hand-rolled PNG encoder)
docs/                      architecture, development, configuration, troubleshooting
.github/workflows/         CI (fmt, clippy, test, build) and release automation
```

## Documentation

| Document | What it covers |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Startup and shutdown sequences, module responsibilities, design decisions and their trade-offs. |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Local setup, the dev loop, tests, linting, icon regeneration, release process. |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Every environment variable and config file, with examples. |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Symptom-to-cause table for the failures this shell can produce. |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to propose changes, coding conventions, commit and PR expectations. |
| [SECURITY.md](SECURITY.md) | Security model, the CSP decision, how to report a vulnerability. |
| [CHANGELOG.md](CHANGELOG.md) | Release history. |

## Design notes

- **`app.security.csp` is `null` on purpose.** The harness UI evaluates client-side Cordis plugin code and opens a WebSocket to its own loopback origin; a restrictive CSP would break both. The window only ever loads `http://127.0.0.1:<port>` — see [SECURITY.md](SECURITY.md) for the full reasoning.
- **The port defaults to `0`.** The shell always reads the URL the server announces rather than assuming a port, so multiple instances coexist.
- **Shutdown kills a process group, not a process.** The harness spawns shell tool subprocesses; killing only the server PID would strand them. On Unix the child is started as its own group leader and receives `SIGTERM` to the negated PID; on Windows the equivalent is `taskkill /T /F`.
- **The shell is stateless.** No database, no config file, no IPC commands exposed to the webview. If a feature belongs to the agent, it belongs upstream in the harness.

## Contributing

Issues and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first — it covers the dev setup, the lint/test commands CI runs, and the scope boundary between this shell and the upstream harness.

## License

[MIT](LICENSE) © Alexandre Cyriaco.

This project is an independent desktop wrapper. The DeepSeek Harness itself is a separate project with its own license and copyright; this repository does not vendor its source, it installs it from npm at build time.
