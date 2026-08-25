# Troubleshooting

Symptom-first guide to the failures this shell can produce. Anything that
happens *inside* the harness UI after the window opens is an upstream harness
issue, not a shell issue — the fastest way to tell the two apart is the
[isolation test](#is-it-the-shell-or-the-harness) at the bottom.

## Contents

- [The app exits immediately](#the-app-exits-immediately)
- [The window never appears](#the-window-never-appears)
- [The window is blank or shows the fallback page](#the-window-is-blank-or-shows-the-fallback-page)
- [A server keeps running after closing the app](#a-server-keeps-running-after-closing-the-app)
- [Build failures](#build-failures)
- [`npm run backend:install` is slow or fails](#npm-run-backendinstall-is-slow-or-fails)
- [macOS: the app is blocked or damaged](#macos-the-app-is-blocked-or-damaged)
- [Is it the shell or the harness?](#is-it-the-shell-or-the-harness)

## The app exits immediately

Run the app from a terminal so you can see stderr — every diagnostic is
prefixed with `dsh-desktop:`.

### `no DeepSeek Harness backend found`

None of the four resolution paths matched. Pick one:

```sh
npm run backend:install                       # install the bundled runtime
npm i -g @deepseek-ai/dsh                     # or install dsh globally
export DSH_DESKTOP_BACKEND=/path/to/lib/bin.js  # or point at a checkout
```

If you are launching a packaged macOS app from Finder, note that it does not
inherit your shell `PATH` — a `dsh` that works in your terminal may be
invisible to the app. Bundle the backend (see the
[README](../README.md#shipping-a-self-contained-app)) or set
`DSH_DESKTOP_BACKEND` explicitly.

### `failed to start the DeepSeek Harness backend: …`

The command was found but could not be executed. Usual causes: the file is not
executable (`chmod +x`), `node` is missing from `PATH`, or `DSH_DESKTOP_BACKEND`
points at a path that no longer exists.

### `backend did not become ready within 45s`

The backend started but never printed `http://127.0.0.1:<port>`. Common causes:

- The `npx` bootstrap path is downloading the package on a slow connection —
  run `npm run backend:install` once so resolution takes the fast local path.
- `DSH_DESKTOP_PORT` names a port already in use, so the server exits before
  announcing. Unset it and let the OS assign one.
- The harness itself is failing at startup — reproduce it directly with the
  [isolation test](#is-it-the-shell-or-the-harness).

### `backend exited before announcing its URL`

The child process died. Run the backend command by hand to see its error; in a
debug build (`npm run dev`) the backend's own output is echoed with a
`[dsh web]` prefix, which usually contains the real message.

## The window never appears

If nothing is printed and no window opens, the shell is still inside the
45-second readiness wait. Give it the full timeout — it will either open or
print a diagnostic. A cold `npx` bootstrap is the usual reason.

## The window is blank or shows the fallback page

The fallback page ("Starting DeepSeek Harness…") means the window was created
but could not load the harness URL. Check that the server is really listening:

```sh
curl -sS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:<port>
```

If that fails, the server died after announcing. If it succeeds but the window
stays blank, the webview failed to render — on Linux this is almost always a
missing or mismatched `webkit2gtk` version; on Windows, a missing WebView2
runtime.

## A server keeps running after closing the app

Closing the window should terminate the whole process group. It will not if the
app was killed with `SIGKILL` (`kill -9`, Force Quit) — no handler can run in
that case.

Find and stop the survivor:

```sh
# macOS / Linux
pgrep -fl "dsh.*web" 
pkill -f "dsh.*web"

# Windows (PowerShell)
Get-CimInstance Win32_Process | Where-Object CommandLine -like '*dsh*web*'
```

If it happens on a *normal* close, that is a bug worth reporting — include your
OS and how you closed the app.

## Build failures

### `cargo build` fails on a Tauri dependency

Almost always a missing system prerequisite. Re-check
<https://tauri.app/start/prerequisites/> for your platform, then:

```sh
cd src-tauri && cargo clean && cargo build
```

### Linux: `Package webkit2gtk-4.1 was not found`

Install the development package for your distro (`libwebkit2gtk-4.1-dev` on
Debian/Ubuntu, `webkit2gtk4.1-devel` on Fedora, `webkit2gtk-4.1` on Arch).

### Windows: `link.exe not found`

The MSVC toolchain is missing. Install "Desktop development with C++" from the
Visual Studio Build Tools installer.

### macOS: `xcrun: error: invalid active developer path`

```sh
xcode-select --install
```

### The bundle step fails but compilation succeeded

Bundling needs extra tools per platform (`rpmbuild` for `.rpm`, WiX for `.msi`).
Narrow the target while iterating:

```sh
npm run build -- --bundles app        # macOS .app only
npm run build -- --bundles deb        # Linux .deb only
```

## `npm run backend:install` is slow or fails

The harness has a large peer-dependency graph that npm resolves slowly. The
script already prefers `pnpm`, then `bun`, then npm with `--legacy-peer-deps`.
Installing pnpm is the single biggest improvement:

```sh
npm i -g pnpm && npm run backend:install
```

If it fails partway, clear and retry:

```sh
rm -rf backend/node_modules && npm run backend:install
```

## macOS: the app is blocked or damaged

Locally built bundles are unsigned, so Gatekeeper quarantines them.

```sh
xattr -dr com.apple.quarantine "/Applications/DeepSeek Harness Desktop.app"
```

Only do this for a bundle you built yourself or obtained from a source you
trust. Proper distribution requires signing and notarization — see
[docs/DEVELOPMENT.md](DEVELOPMENT.md#code-signing).

## Is it the shell or the harness?

Run the backend directly, exactly as the shell would:

```sh
node backend/node_modules/@deepseek-ai/dsh/lib/bin.js web --no-open --port 0
```

- **It prints `dsh web: http://127.0.0.1:<port>` and the UI works in a
  browser** → the harness is fine; the problem is in this shell. Please
  [open an issue](https://github.com/AleCyriaco/dsh-desktop/issues) with your OS,
  the stderr output, and the steps you took.
- **It fails, or the UI misbehaves in the browser too** → the problem is
  upstream in the harness, and the shell will reproduce it faithfully because
  it changes nothing about the runtime.
