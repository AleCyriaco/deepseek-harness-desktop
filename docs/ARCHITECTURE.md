# Architecture

This document explains how DeepSeek Harness Desktop is put together, what each
module is responsible for, and why the non-obvious decisions were made the way
they were.

## Contents

- [The one-sentence version](#the-one-sentence-version)
- [Component map](#component-map)
- [Startup sequence](#startup-sequence)
- [Shutdown sequence](#shutdown-sequence)
- [Module reference](#module-reference)
- [Design decisions](#design-decisions)
- [Failure modes](#failure-modes)
- [Extending the shell](#extending-the-shell)

## The one-sentence version

The app is a Tauri window whose webview points at a `dsh web` server that the
app itself started on a loopback port, plus the process bookkeeping needed to
make that lifecycle reliable.

## Component map

```
                       process boundary
                              │
  ┌───────────────────────────┼──────────────────────────────────────┐
  │  dsh-desktop (Rust)       │        dsh web (Node)                │
  │                           │                                      │
  │  main.rs                  │   ┌──────────────────────────────┐   │
  │    └─ lib.rs::run()       │   │  HTTP + WebSocket server     │   │
  │         ├─ BackendState   │   │  serves the harness GUI      │   │
  │         │    (Mutex)      │   │                              │   │
  │         ├─ backend.rs ────┼──▶│  spawned as a child process, │   │
  │         │   spawn/kill    │   │  own process-group leader    │   │
  │         └─ WebviewWindow ─┼──▶│  ◀── HTTP/WS on 127.0.0.1    │   │
  │                           │   └───────────┬──────────────────┘   │
  └───────────────────────────┼───────────────┼──────────────────────┘
                              │               ▼
                              │      tool subprocesses the agent
                              │      spawns (bash, node, git, …)
                              │      — same process group
```

The shell owns **no application state**. There is no database, no settings
file, and no Tauri IPC command exposed to the webview. Every piece of user
state — sessions, API keys, model selection, plugins — lives inside the harness
and is identical to what the browser version stores.

## Startup sequence

```
  run()                                     backend.rs
   │
   ├─ tauri::Builder::setup ──────────────▶ search_roots(app)
   │                                          resource_dir
   │                                          6 ancestors of current_exe
   │                                          current_dir
   │                                          $CARGO_MANIFEST_DIR/..
   │                                             │
   ├─────────────────────────────────────▶ spawn_backend(roots)
   │                                          │
   │                                          ├─ find_node()
   │                                          │    $DSH_DESKTOP_NODE
   │                                          │    PATH
   │                                          │    extra_bin_dirs()  ← Homebrew, nvm,
   │                                          │      fnm, Volta, asdf, MacPorts, …
   │                                          │
   │                                          ├─ build_command(roots)
   │                                          │    1. $DSH_DESKTOP_BACKEND
   │                                          │    2. bundled bin.js
   │                                          │    3. `dsh` (PATH + extra dirs)
   │                                          │    4. `npx --yes @deepseek-ai/dsh@latest`
   │                                          │    (+ `web --no-open --port $DSH_DESKTOP_PORT|0`)
   │                                          │    (+ PATH prepended with the tool's dir)
   │                                          │
   │                                          ├─ process_group(0)      [unix]
   │                                          ├─ spawn with piped stdout/stderr
   │                                          │
   │                                          ├─ reader thread
   │                                          │    scans stdout for
   │                                          │    "http://127.0.0.1:<digits>"
   │                                          │    sends the first match over
   │                                          │    an mpsc channel, then keeps
   │                                          │    draining both pipes forever
   │                                          │
   │                                          └─ recv_timeout(45s)
   │                                               Ok(url)      → return (Backend, url)
   │                                               Timeout      → kill group, Err
   │                                               Disconnected → kill group, Err
   │
   ├─ app.manage(BackendState(Some(backend)))
   │
   ├─ create_main_window(app, url)
   │    WebviewUrl::External(parsed_url)
   │    1280×800, min 900×600, centered
   │
   └─ on any Err: eprintln + BackendState::shutdown() + exit(1)
```

The 45-second `READY_TIMEOUT` is deliberately generous: resolution path 4
(`npx --yes @deepseek-ai/dsh@latest`) may download the package on a cold cache
before the server can print anything.

## Shutdown sequence

Two paths lead to teardown, and both converge on `Backend::shutdown`:

1. **Normal exit** — `RunEvent::Exit` fires in the `app.run` handler, which
   calls `BackendState::shutdown()`.
2. **Backstop** — `Drop for Backend` runs if the value is dropped without an
   explicit shutdown (for example a panic during setup).

`Backend::shutdown` does three things in order:

1. `terminate_group(pid)` — a **group**-wide signal, not a single-process kill:
   - **Unix**: `libc::kill(-(pid as i32), SIGTERM)`. The negated PID addresses
     the process group, which works because the child was started with
     `process_group(0)`, making it its own group leader.
   - **Windows**: `taskkill /PID <pid> /T /F`, where `/T` walks the tree.
2. Sleep 600 ms so the harness's Cordis host can flush state to disk.
3. `child.kill()` then `child.wait()` — force-kill whatever ignored the signal
   and reap the zombie.

### Why the group matters

The harness runs agent tools as subprocesses: shells, `git`, `node`, language
servers. Killing only the server PID would orphan every one of them, and on
macOS those orphans keep running with no window to stop them. Signalling the
group is the only reliable way to guarantee that closing the window ends
everything the app started.

## Module reference

### `src-tauri/src/main.rs`

Nine lines. Sets `windows_subsystem = "windows"` for release builds (so no
console window appears on Windows) and calls `dsh_desktop_lib::run()`.

### `src-tauri/src/lib.rs`

- **`BackendState(Mutex<Option<Backend>>)`** — Tauri-managed state that holds
  the live backend handle. `Option` because it is empty between `manage()` and
  a successful spawn; `Mutex` because Tauri state must be `Sync`. `shutdown()`
  `take()`s the value, so a double shutdown is a no-op rather than a
  double-kill.
- **`create_main_window`** — parses the announced URL and builds the window
  with `WebviewUrl::External`. Parse failures are surfaced as
  `io::ErrorKind::InvalidInput` rather than panicking.
- **`run`** — wires the setup hook, builds the app, and registers the
  `RunEvent::Exit` handler.

Note that `tauri.conf.json` declares `"windows": []` — no window is created
declaratively. The window is only built *after* the backend is ready, so the
user never sees an empty frame pointed at nothing.

### `src-tauri/src/backend.rs`

| Item | Responsibility |
|---|---|
| `READY_TIMEOUT` | 45 s budget for the server to announce itself. |
| `URL_PREFIX` | `http://127.0.0.1:` — the exact prefix the harness prints. |
| `Backend` | Owns the `Child` plus its PID/PGID; implements `shutdown` and `Drop`. |
| `extract_url` | Pulls the loopback URL out of a stdout line. Unit-tested. |
| `find_on_path` | `PATH` lookup that honours `.exe`/`.cmd`/`.bat` on Windows. |
| `extra_bin_dirs` | Well-known install directories a GUI launcher does not put on `PATH`. |
| `version_manager_bin_dirs` | nvm and fnm version directories, newest first. |
| `parse_version` | Turns `v20.11.0` into a sortable triple. Unit-tested. |
| `find_tool` | `find_on_path`, then `extra_bin_dirs`. |
| `find_node` | `DSH_DESKTOP_NODE`, then `find_tool("node")`. |
| `prepend_path` | Puts the resolved tool's directory on the child's `PATH`. |
| `describe` | Renders the resolved command for diagnostics. |
| `add_web_args` | Appends `web --no-open --port <port>`. |
| `build_command` | The four-step backend resolution. |
| `terminate_group` | Platform-specific group/tree kill. |
| `spawn_backend` | Spawn, drain, wait for the URL, or fail cleanly. |
| `search_roots` | The four families of directories where a bundled backend may live. |

### `src/index.html`

A static fallback page referenced by `build.frontendDist`. Tauri requires a
frontend directory to exist; in practice this page is almost never shown,
because the main window loads the external harness URL instead. It renders a
pulsing indicator and one line of explanatory text.

### `scripts/install-backend.mjs`

Installs `@deepseek-ai/dsh` into `backend/node_modules`. It probes for
`pnpm`, then `bun`, then falls back to `npm install --legacy-peer-deps` — the
flag avoids npm's very slow ideal-tree resolution on the harness's peer
dependency graph.

### `scripts/make-icon.mjs`

Generates `app-icon.png` (1024×1024) with a hand-rolled PNG encoder — CRC32
table, IHDR/IDAT/IEND chunks, `zlib.deflateSync` — so icon regeneration needs
no image dependency at all. `npx tauri icon app-icon.png` then fans it out to
every platform size.

## Design decisions

### External webview instead of a bundled frontend

`WebviewUrl::External` points the webview at the running server. The
alternative — bundling a copy of the harness UI as `frontendDist` — would
require keeping that copy in sync with upstream and would break the harness's
own asset and WebSocket routing. Loading the server's own origin means the UI
behaves exactly as it does in a browser.

### `csp: null`

The harness UI evaluates client-side Cordis plugin code and opens a WebSocket
back to its own origin. Any `script-src` policy strict enough to be worth
having would break plugin evaluation, and the window only ever loads
`http://127.0.0.1:<port>`, an origin this app started itself. See
[SECURITY.md](../SECURITY.md) for the threat model this trades against.

### Port `0` by default

Letting the OS assign the port means two copies of the app never collide, and
it removes a whole class of "address already in use" startup failures. The
shell never assumes a port — it always reads the one the server announces.
`DSH_DESKTOP_PORT` exists for the cases where a fixed port is genuinely needed
(a proxy, a firewall rule, an external tool pointed at the server).

### Draining stdout forever

The reader thread keeps reading both pipes for the process's whole life, even
after the URL is found. If it stopped, a full pipe buffer would block the
server's next write and silently hang the harness. Post-announcement lines are
echoed to stderr in debug builds only.

### Looking beyond `PATH` for Node

This is the least obvious code in the project and it exists for one concrete
failure. A macOS `.app` launched from Finder does not inherit your shell
environment; it inherits launchd's, whose `PATH` is
`/usr/bin:/bin:/usr/sbin:/sbin`. Homebrew installs Node in `/opt/homebrew/bin`
and nvm under `~/.nvm` — neither is on that list. A build that only consulted
`PATH` therefore started fine from a terminal and failed for every user who
double-clicked it, which is the worst possible split between how it is
developed and how it is used.

`extra_bin_dirs` enumerates the places Node is actually installed, and
`version_manager_bin_dirs` walks nvm and fnm version directories sorted
**numerically** — a lexical sort would rank `v9` above `v20`, which
`orders_node_versions_numerically_not_lexically` guards against.

The resolved directory is then prepended to the child's `PATH`, because the
harness spawns its own `npm`/`npx`/language-server subprocesses that need the
same Node the shell just found.

Shipping a Node runtime inside the bundle would sidestep all of this, at the
cost of doubling the download and pinning every user to one version. Finding
the user's own Node is the better trade.

### Four backend resolution paths

Each path serves a distinct scenario: `DSH_DESKTOP_BACKEND` for developing
against a local harness checkout, the bundled `bin.js` for packaged and dev
builds, `PATH` for users who already have `dsh` installed globally, and `npx`
as a last-resort bootstrap that works on a machine with nothing but Node.

## Failure modes

| Condition | Behaviour |
|---|---|
| Node not found anywhere | `build_command` returns `Err` naming `DSH_DESKTOP_NODE` as the escape hatch; exit `1`. |
| No backend resolvable | `build_command` returns `Err`; the message names all three fixes; exit `1`. |
| Backend spawns but never prints a URL | `recv_timeout` times out after 45 s; group killed; the resolved command and the last stderr lines are included in the error; exit `1`. |
| Backend exits early (bad args, crash) | The channel disconnects when the reader thread ends; group killed; stderr tail reported; exit `1`. |
| Announced URL is unparseable | `create_main_window` returns `Err`; backend shut down; exit `1`. |
| App killed with `SIGKILL` | Neither `RunEvent::Exit` nor `Drop` runs — the server is orphaned. This is unavoidable; `SIGKILL` cannot be handled. |

All diagnostics are printed to stderr with a `dsh-desktop:` prefix.

## Extending the shell

The scope boundary is deliberate: **agent behaviour belongs upstream in the
harness, not here.** Changes that fit this repository are ones that only a
native shell can provide — a system tray icon, a native menu bar, deep-link
handling, auto-update, single-instance enforcement, or window state
persistence.

If you find yourself adding a Tauri IPC command that the harness UI would have
to call, that is a strong sign the feature belongs in the harness instead.
