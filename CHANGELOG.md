# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet.

## [0.1.4] — 2026-08-26

### Fixed

- **The portable build shipped an incomplete harness.** `npm` was invoked with
  `--legacy-peer-deps`, which skips peer dependencies — and the harness has
  real ones, so `@deepseek-ai/dsh-app-boot` was packed without
  `@deepseek-ai/cordis-plugin-group`. The install reported success and the
  failure only appeared at runtime, as `ERR_MODULE_NOT_FOUND`. The flag was
  there to make installation faster; it was trading correctness for speed.
- pnpm is now run with `--node-linker=hoisted`. Its default layout is a farm of
  symlinks into a content store, which the portable packer skips — so a
  developer with pnpm installed would have produced a differently broken
  payload.

### Added

- `npm run backend:check` starts the installed runtime and waits for it to
  announce its URL. CI runs it before anything is packaged, because a tree that
  installs cleanly can still be unusable — which is exactly how the bug above
  reached a release.

## [0.1.3] — 2026-08-26

### Changed

- **The portable build is now genuinely self-contained.** It carries the
  harness runtime *and* a pinned Node interpreter compressed inside the
  executable, and unpacks them once into a per-user cache directory. It no
  longer downloads anything, no longer needs `npx`, and no longer needs Node
  installed at all — the previous portable build required all three, and the
  download was where it failed. The executable is ~87 MB as a result.
- The embedded interpreter is preferred over an installed one, so every
  portable user runs the harness under the version the release was tested
  against.

### Fixed

- Both ends of the backend's output are kept for error reports. Only the last
  40 lines were kept, and a Node crash leads with the cause and follows it with
  dozens of stack frames — so the one line that explained anything was always
  the line thrown away.

### Documentation

- Corrected the claim that only `SIGKILL` orphans the backend: `SIGTERM` does
  too, because neither runs the shutdown handler. Closing the window, the
  normal path, tears everything down correctly.

## [0.1.2] — 2026-08-26

### Fixed

- **The portable build could hang forever on first run.** stdout and stderr
  were drained one after the other, so whichever was read second filled its
  64 KB pipe buffer and blocked the backend permanently. npm writes its
  progress to stderr, which made this the normal case rather than an edge
  case. Each stream now has its own reader thread.
- **`npx` could not be launched on Windows at all.** It ships as `npx.cmd`,
  and `CreateProcess` refuses batch files, so spawning it failed outright.
  Batch scripts now go through `cmd.exe /C`.

### Changed

- **The bootstrap waits on activity, not on a stopwatch.** A fixed 15-minute
  deadline was wrong in both directions: it killed slow downloads that were
  still progressing, and left genuinely stuck ones hanging for a quarter of an
  hour. The bootstrap now fails after 150 seconds of complete silence, with a
  45-minute absolute ceiling.
- **npm runs at `--loglevel=http` during the bootstrap.** It is otherwise
  entirely silent while downloading ~270 MB, which leaves no way to tell a
  slow connection from a wedged process.

### Added

- **The splash window shows live progress.** Backend output is streamed into
  it: the DeepSeek whale swims while bubbles rise, one per event actually
  received — so a stalled download visibly stops bubbling instead of showing a
  progress bar that keeps animating over nothing. A real count of packages
  fetched, the current line, and elapsed time are shown alongside.

## [0.1.1] — 2026-08-26

### Added

- **Portable Windows build.** `dsh-desktop.exe` is published on its own: a
  single 8 MB executable that needs no installation. With no resources beside
  it, it falls through to the `npx` bootstrap and fetches the harness on first
  run.
- **A window now appears immediately**, before the backend is ready. Startup
  used to block the Tauri setup hook, so nothing rendered until the server had
  announced its URL — fine for a local backend, indistinguishable from a crash
  for a portable first run that downloads 270 MB.
- **Startup failures are shown in that window** instead of only on stderr. A
  GUI app has no console, so a failed start previously left the user with no
  explanation at all. The message is selectable for pasting into a report.

### Changed

- The readiness budget now depends on the resolved backend: 45 s for a local
  install, 15 minutes when `npx` has to download the harness first. The old
  fixed 45 s aborted that download mid-flight and reported a timeout.

### Fixed

- **The packaged Windows build could never start.** `resource_dir()` returns a
  Windows extended-length path (`\\?\C:\...`). Every Win32 API accepts that
  form, so backend resolution succeeded and the bug was invisible to our own
  file checks — but Node rejects it, failing with `EISDIR: lstat 'C:'` and
  exiting before announcing its URL. The window therefore never opened. Paths
  derived from a search root are now normalised before being handed to a child
  process. macOS and Linux were unaffected.

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
  - `scripts/make-icon.mjs` — dependency-free 1024×1024 icon generator: a
    hand-rolled PNG encoder plus a scanline SVG-path rasteriser that draws the
    DeepSeek whale mark (`assets/deepseek-whale.svg`) over a blue gradient.
- **Tests** — unit coverage for the loopback URL parser (including the
  LAN-suffix and no-match cases), Node version-directory parsing, numeric
  version ordering, and JavaScript entry-point detection.
- **Documentation** — README plus architecture, development, configuration and
  troubleshooting guides, contribution guidelines, and a security policy.
- **CI** — GitHub Actions running `cargo fmt`, `clippy`, and `cargo test` on
  every push, with a tag-triggered three-platform release build.

[Unreleased]: https://github.com/AleCyriaco/deepseek-harness-desktop/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/AleCyriaco/deepseek-harness-desktop/releases/tag/v0.1.1
[0.1.0]: https://github.com/AleCyriaco/deepseek-harness-desktop/releases/tag/v0.1.0
