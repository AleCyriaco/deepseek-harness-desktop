# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — 0.2.0, in development

`main` is the development line. Nothing here has shipped yet; the released
build is [0.1.7](#017--2026-08-26), pinned on the `stable` branch.

### Added

- **A status panel**, as a second native webview beside the harness rather than
  markup injected into its page. It shows context usage, token totals with the
  cache hit rate, what fills the context, session timings, and the account
  balance. Pinned, floating or hidden from the View menu.
- Session figures are read from `~/.dsh/storages/session_projcache.json`, which
  the harness writes itself, so they are its own numbers rather than an
  estimate of them.
- The balance comes from `/user/balance`, authenticated with the key the
  harness already stores. It is read for that one request, cached for five
  minutes, and never copied, logged or exposed to a webview — see
  [SECURITY.md](SECURITY.md#the-status-panel-and-your-api-key).
- The command backing the panel refuses any caller that is not the panel
  webview, so the harness's own page cannot read the balance through the shared
  IPC bridge.

### Notes

- **There are no usage charts over time, and there will not be.** DeepSeek's
  public API exposes only `/chat/completions` and `/user/balance`; the platform
  dashboard's request, token and cost charts come from an internal endpoint
  authenticated by a browser session. Reproducing them would mean impersonating
  a login, so the panel shows what can be measured honestly instead.

## [0.1.7] — 2026-08-26

### Fixed

- **Signalling the app no longer orphans the backend.** `SIGTERM`, `SIGINT` and
  `SIGHUP` are now handled and take the harness — and every tool subprocess it
  spawned — down with the app. Previously only closing the window did that, so
  `kill`, a logout or a crashing session manager left a server running with no
  window to stop it. The handler does nothing but `kill` the process group and
  `_exit`, both async-signal-safe; the group id is kept in an atomic because a
  handler cannot take the lock the normal shutdown path uses. `SIGKILL` still
  cannot be caught by anything.

### Changed

- **Linux packages are built again.** AppImage is no longer a bundle target:
  `linuxdeploy` fails on the 300 MB resource tree, and because it ran last its
  failure aborted the job before the working `.deb` and `.rpm` were collected.
  Dropped rather than fixed — the two package formats cover the same ground,
  and chasing `linuxdeploy` was disproportionate to what it added.
- **Building and publishing are separate.** The workflow used to trigger on
  `v*` tags and create a release — but publishing a release *creates the tag*,
  so every publish triggered a build that then tried to attach its own bundles
  to the release just published. It had to be cancelled by hand every time.
  `release.yml` is now `build.yml`: manual only, artifacts only, never touches
  a release. Publishing is an explicit step, documented in
  [Releasing](docs/DEVELOPMENT.md#releasing).

## [0.1.6] — 2026-08-26

### Fixed

- **A console window appeared beside the app on Windows.** `node.exe` is a
  console application, and when a GUI process starts one the OS allocates a
  console window for it — a black box sitting next to the app for the whole
  session. Every process the shell starts is a background worker nobody should
  see, so they are all created with `CREATE_NO_WINDOW`. The same applied to
  `taskkill` on shutdown and to `cmd.exe` when opening the log, both of which
  flashed a window.

## [0.1.5] — 2026-08-26

### Fixed

- **The backend could block mid-session.** Splitting the output readers into
  two threads in 0.1.2 left them returning as soon as the startup channel
  closed — so after the window opened, nothing drained the backend's pipes. The
  next 64 KB it wrote blocked it permanently, long after everything looked
  healthy. The readers now run to EOF regardless, which is what the two-thread
  split was for in the first place.

### Added

- **A log file, written on every run**, holding everything the shell and the
  harness print. A GUI app has no console, so this was previously invisible
  unless the app was relaunched from a terminal — an awkward thing to ask, and
  one that changes the conditions of the run.
- **A Troubleshooting menu**: *Open Log File*, *Show Log Folder*, *Developer
  Tools* and *Reload*. The inspector is enabled in release builds too, because
  a failure inside the page — anything reporting `Failed to fetch` — never
  reaches the backend and cannot be diagnosed from the log.

## [0.1.4] — 2026-08-26

### Fixed

- **The documented Node requirement was wrong.** Every document said Node 20
  was enough. It is not: the harness stores sessions with Zstd, and
  `node:zlib` only gained `createZstdDecompress` in 22.15, so Node 20 fails
  with a module-export error that says nothing about versions. The floor is now
  documented as 22.15 throughout. The portable build is unaffected — it carries
  its own interpreter.
- CI ran its checks under Node 20, so `backend:check` failed against a
  perfectly good tree. It now runs under the interpreter the portable build
  embeds, and the runners use Node 22.

- **The portable build shipped an incomplete harness.** `npm` was invoked with
  `--legacy-peer-deps`, which skips peer dependencies — and the harness has
  real ones, so `@deepseek-ai/dsh-app-boot` was packed without
  `@deepseek-ai/cordis-plugin-group`. The install reported success and the
  failure only appeared at runtime, as `ERR_MODULE_NOT_FOUND`. The flag was
  there to make installation faster; it was trading correctness for speed.
- **`backend/package-lock.json` is now committed and installs go through
  `npm ci`.** Removing `--legacy-peer-deps` fixed the missing package but left
  npm resolving the graph from scratch, which exhausted the heap on GitHub's
  macOS runner. A lockfile sidesteps resolution entirely: 18 seconds instead of
  minutes, no memory blow-up, and the exact package set that was tested. This
  also closes a reproducibility gap the documentation had flagged from the
  start.
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
