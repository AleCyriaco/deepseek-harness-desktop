# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet.

## [0.1.0] — 2026-08-25

Initial release: a native Tauri v2 shell around the DeepSeek Harness web GUI.

### Added

- **Backend lifecycle** (`src-tauri/src/backend.rs`)
  - Four-step backend resolution: `DSH_DESKTOP_BACKEND`, a bundled
    `@deepseek-ai/dsh` runtime, `dsh` on `PATH`, then `npx --yes @deepseek-ai/dsh@latest`.
  - Starts `dsh web --no-open --port <port>` and discovers the announced
    loopback URL from stdout, with a 45-second readiness timeout.
  - Both output streams are drained for the process's whole life so a full pipe
    can never stall the server; backend output is echoed in debug builds only.
  - Startup failures name the exact command that was run and include the
    backend's last stderr lines, rather than reporting a bare timeout.
  - Teardown signals the entire process group (`SIGTERM` to the negated PID on
    Unix, `taskkill /T /F` on Windows) so agent tool subprocesses cannot be
    orphaned, with a `Drop` implementation as a backstop.
- **Native window** (`src-tauri/src/lib.rs`)
  - Created only after the backend is ready, so no empty frame is ever shown.
  - 1280×800 default, 900×600 minimum, centered, pointed at the announced URL
    via `WebviewUrl::External`.
  - Backend shutdown wired to `RunEvent::Exit`.
- **Node discovery** — Node is located outside `PATH` when necessary:
  Homebrew, MacPorts, nvm, fnm, Volta and asdf install locations are all
  searched, newest Node version first. A macOS `.app` opened from Finder
  inherits launchd's minimal `PATH`, which contains no Node at all, so
  `PATH`-only lookup would fail for nearly every user of a downloaded build.
  The directory Node was resolved from is prepended to the backend's `PATH`,
  so the tool subprocesses the agent spawns inherit the same installation.
- **Self-contained bundles** — `bundle.resources` ships
  `backend/node_modules/` inside the app, so no `dsh` install is needed on the
  target machine. macOS builds are universal (Apple Silicon + Intel).
- **Configuration** — `DSH_DESKTOP_BACKEND`, `DSH_DESKTOP_NODE` and
  `DSH_DESKTOP_PORT`.
- **Fallback page** (`src/index.html`) shown only if the window cannot reach
  the harness.
- **Tooling**
  - `scripts/install-backend.mjs` — installs the harness runtime, preferring
    pnpm, then bun, then npm with `--legacy-peer-deps`.
  - `scripts/make-icon.mjs` — dependency-free 1024×1024 icon generator with a
    hand-rolled PNG encoder.
- **Tests** — unit coverage for the loopback URL parser (including the
  LAN-suffix and no-match cases), Node version-directory parsing, numeric
  version ordering, and JavaScript entry-point detection.
- **Documentation** — README plus architecture, development, configuration and
  troubleshooting guides, contribution guidelines, and a security policy.
- **CI** — GitHub Actions running `cargo fmt`, `clippy`, and `cargo test` on
  every push, with a tag-triggered three-platform release build.

[Unreleased]: https://github.com/AleCyriaco/dsh-desktop/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/AleCyriaco/dsh-desktop/releases/tag/v0.1.0
