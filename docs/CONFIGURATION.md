# Configuration reference

The desktop shell is deliberately configuration-free. It exposes exactly three
environment variables, and everything else — API keys, model selection,
sessions, plugins, permissions — is configured **inside the harness UI** and
stored by the harness, identically to the browser version.

## Contents

- [Environment variables](#environment-variables)
- [How Node is located](#how-node-is-located)
- [Where harness state lives](#where-harness-state-lives)
- [Files that configure the build](#files-that-configure-the-build)
- [Recipes](#recipes)

## Environment variables

### `DSH_DESKTOP_BACKEND`

Overrides backend resolution entirely. Accepts either:

- a path to an executable — run directly; or
- a path ending in `.js`, `.mjs` or `.cjs` — run as `node <path>`.

```sh
DSH_DESKTOP_BACKEND=/opt/homebrew/bin/dsh              open-the-app
DSH_DESKTOP_BACKEND=/src/deepseek-harness/lib/bin.js   npm run dev
```

An empty or whitespace-only value is ignored, and resolution falls through to
the next source. The value is **not** shell-parsed: it is a single path, not a
command line, so `"node foo.js --flag"` will not work.

### `DSH_DESKTOP_NODE`

Absolute path to the `node` binary the shell should use, skipping the search
described in [How Node is located](#how-node-is-located) entirely.

```sh
DSH_DESKTOP_NODE=/opt/homebrew/bin/node npm run dev
```

Use it when you have several Node versions installed and want to guarantee
which one the harness runs under, or when your installation lives somewhere the
search does not cover. An empty or whitespace-only value is ignored.

### `DSH_DESKTOP_PORT`

Pins the port passed to `dsh web --port`. Default `0`, which asks the OS for a
free port — the reason two copies of the app never collide.

```sh
DSH_DESKTOP_PORT=5173 npm run dev
```

An empty or whitespace-only value falls back to `0`. Whatever you set, the
shell still reads the URL the server actually announces rather than assuming
the port took effect.

### Setting variables for a packaged app

Environment variables are inherited from the launching process, which on a
desktop is usually the OS launcher rather than your shell.

- **macOS** — launching `DeepSeek Harness Desktop.app` from Finder does *not*
  see your shell profile. Launch it from a terminal instead:

  ```sh
  DSH_DESKTOP_PORT=5173 open -a "DeepSeek Harness Desktop"
  ```

  (or `open` the binary inside `Contents/MacOS/` directly to also see stderr).
- **Windows** — set the variable in the shell that starts the app, or as a User
  environment variable via System Properties.
- **Linux** — set it in the `.desktop` entry's `Exec=` line, or export it in
  the shell you launch from.

## How Node is located

The harness is a Node program, so the shell has to find a `node` binary before
it can start anything. It looks in this order:

1. `DSH_DESKTOP_NODE`, if set.
2. Every directory on `PATH`.
3. The well-known install locations below.

| Platform | Directories |
|---|---|
| macOS | `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin` |
| Linux | `/usr/local/bin`, `/usr/bin`, `/snap/bin` |
| Windows | `%ProgramFiles%\nodejs`, `%LOCALAPPDATA%\Programs\nodejs`, `%LOCALAPPDATA%\Volta\bin`, `%APPDATA%\npm` |
| All | `~/.volta/bin`, `~/.asdf/shims`, `~/.local/bin` |
| All | every nvm (`~/.nvm/versions/node/*/bin`) and fnm version directory, sorted by version, **newest first** |

Step 3 exists for one specific reason: a macOS `.app` launched from Finder
inherits launchd's `PATH`, which is `/usr/bin:/bin:/usr/sbin:/sbin` and
contains no Node installation. Relying on `PATH` alone would make every
downloaded build fail to start.

The same search resolves `dsh` and `npx` in backend resolution steps 3 and 4.

Whatever directory the tool is found in is **prepended to the backend's own
`PATH`**, so the tool subprocesses the agent spawns (`npm`, `npx`, language
servers) inherit the same Node installation rather than falling back to a
different one — or to none.

## Where harness state lives

The shell writes nothing. Sessions, credentials and settings are stored by the
harness in its own location (see the upstream
[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
documentation). Two consequences worth knowing:

- State is **shared** between the desktop app and `dsh web` in a browser — the
  same sessions appear in both.
- Uninstalling the desktop app removes no harness data.

## Files that configure the build

| File | Controls |
|---|---|
| `src-tauri/tauri.conf.json` | Product name, identifier, version, bundle targets and icons, CSP, `frontendDist`. |
| `src-tauri/Cargo.toml` | Rust crate name, version, dependencies, the `staticlib`/`cdylib`/`rlib` crate types Tauri needs. |
| `src-tauri/capabilities/default.json` | The Tauri permission set granted to the `main` window — currently `core:default` only. |
| `package.json` | The npm scripts and the Tauri CLI version. |
| `backend/package.json` | The `@deepseek-ai/dsh` semver range installed by `npm run backend:install`. |

### Bundling the backend

```json
"bundle": {
  "resources": { "../backend/node_modules/": "backend/node_modules/" }
}
```

This is enabled by default, and it is why `npm run backend:install` must run
before `npm run build`: the packaged app carries its own harness runtime
(~270 MB) and needs no `dsh` on the target machine. Remove the entry to build a
slim app that resolves `dsh` at runtime instead.

### Notable settings in `tauri.conf.json`

```json
"app": {
  "withGlobalTauri": false,
  "windows": [],
  "security": { "csp": null }
}
```

- **`withGlobalTauri: false`** — no `window.__TAURI__` is injected into the
  page. The harness UI is unmodified web content and must not gain ambient
  access to native APIs.
- **`windows: []`** — no window is declared. `lib.rs` builds the window
  programmatically *after* the backend announces its URL, so no empty frame is
  ever shown.
- **`csp: null`** — see [SECURITY.md](../SECURITY.md) for why, and what it
  trades away.

### Window geometry

Set in `lib.rs::create_main_window`, not in configuration:

| Property | Value |
|---|---|
| Title | `DeepSeek Harness Desktop` |
| Initial size | 1280 × 800 |
| Minimum size | 900 × 600 |
| Position | centered |

Window size and position are not persisted across launches.

## Recipes

**Run against a local harness checkout**

```sh
DSH_DESKTOP_BACKEND=/src/deepseek-harness/lib/bin.js npm run dev
```

**Use the app and a browser against the same server**

```sh
DSH_DESKTOP_PORT=5173 npm run dev
# then open http://127.0.0.1:5173 in any browser
```

**Force the npx bootstrap path** (no local install, latest published harness)

```sh
# ensure backend/node_modules is absent and `dsh` is not on PATH
rm -rf backend/node_modules
env -u DSH_DESKTOP_BACKEND npm run dev
```

**Pin a specific Node version**

```sh
DSH_DESKTOP_NODE="$HOME/.nvm/versions/node/v20.11.0/bin/node" npm run dev
```

**Diagnose a backend that will not start**

Run the resolved command by hand — the shell does nothing to it beyond adding
the arguments:

```sh
node backend/node_modules/@deepseek-ai/dsh/lib/bin.js web --no-open --port 0
```

If that prints `dsh web: http://127.0.0.1:<port>`, the shell will work; if it
does not, the problem is in the harness, not here.
