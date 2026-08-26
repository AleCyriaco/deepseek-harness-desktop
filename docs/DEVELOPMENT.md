# Development guide

Everything you need to build, run, test and release DeepSeek Harness Desktop.

## Contents

- [Toolchain](#toolchain)
- [First-time setup](#first-time-setup)
- [The dev loop](#the-dev-loop)
- [npm scripts](#npm-scripts)
- [Tests](#tests)
- [Formatting and linting](#formatting-and-linting)
- [Working against a local harness checkout](#working-against-a-local-harness-checkout)
- [Regenerating the app icon](#regenerating-the-app-icon)
- [Building distributables](#building-distributables)
- [Releasing](#releasing)
- [Repository hygiene](#repository-hygiene)

## Toolchain

| Tool | Version | Why |
|---|---|---|
| Node.js | ≥ 20 | Runs the Tauri CLI and the harness backend. |
| Rust | stable (via [rustup](https://rustup.rs)) | Builds the shell. |
| Cargo components | `rustfmt`, `clippy` | CI enforces both. |
| pnpm *(optional)* | any recent | Much faster backend install than npm. |

Plus the platform prerequisites listed at
<https://tauri.app/start/prerequisites/>:

- **macOS** — Xcode Command Line Tools: `xcode-select --install`
- **Windows** — MSVC build tools and the WebView2 runtime (preinstalled on
  Windows 11 and up-to-date Windows 10)
- **Linux** — `webkit2gtk-4.1`, `libappindicator3`, `librsvg2`, `patchelf`;
  package names vary by distro

## First-time setup

```sh
git clone https://github.com/AleCyriaco/dsh-desktop.git
cd dsh-desktop
npm run setup   # npm install && npm run backend:install
```

`npm run setup` installs two separate dependency trees:

- the repository root gets `@tauri-apps/cli` only (a few MB);
- `backend/` gets the full `@deepseek-ai/dsh` runtime (~270 MB).

Both are gitignored.

## The dev loop

```sh
npm run dev
```

This runs `tauri dev`, which compiles the Rust shell in debug mode and launches
it. The first build pulls and compiles the whole Tauri dependency tree and can
take several minutes; incremental rebuilds are seconds.

In debug builds, every line the backend prints is echoed to your terminal with
a `[dsh web]` prefix — that is your window into what the harness server is
doing. Release builds stay silent.

Editing Rust triggers a rebuild and restart. Editing `src/index.html` only
matters for the fallback page, which you will rarely see.

To run an already-built debug binary without going through the CLI:

```sh
./src-tauri/target/debug/dsh-desktop
```

## npm scripts

| Script | What it does |
|---|---|
| `npm run setup` | `npm install` + `npm run backend:install`. |
| `npm run backend:install` | Installs `@deepseek-ai/dsh` into `backend/node_modules` (pnpm → bun → npm). |
| `npm run dev` | `tauri dev` — debug build, opens the window. |
| `npm run build -- --target universal-apple-darwin` | Universal macOS bundle. |
| `npm run build` | `tauri build` — release build and platform bundles. |
| `npm run tauri` | Raw passthrough to the Tauri CLI, e.g. `npm run tauri icon app-icon.png`. |

## Tests

The Rust unit tests cover `extract_url`, the parser that finds the loopback URL
in the backend's stdout — the one piece of pure logic in the shell, and the one
most likely to break if upstream changes its startup banner.

```sh
cd src-tauri
cargo test
```

> **`resource path ../backend/node_modules doesn't exist`?** `tauri-build`
> validates every `bundle.resources` path at compile time, so the directory
> must exist even for a plain `cargo test`. Run `npm run backend:install`, or
> `mkdir -p backend/node_modules` if you only want to compile.

Expected output:

```
test backend::tests::extracts_announced_loopback_url ... ok
test backend::tests::extracts_url_with_lan_suffix ... ok
test backend::tests::ignores_unrelated_lines ... ok
```

When adding logic, prefer pulling it into a pure function that can be tested
without spawning a process — that is the pattern `extract_url` follows.

## Formatting and linting

CI runs all three of these, so run them before opening a pull request:

```sh
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

To apply formatting rather than just check it, drop the `-- --check`.

## Working against a local harness checkout

Point the shell at any backend you like with `DSH_DESKTOP_BACKEND`:

```sh
# a local harness checkout
DSH_DESKTOP_BACKEND=/path/to/deepseek-harness/lib/bin.js npm run dev

# a globally installed dsh
DSH_DESKTOP_BACKEND="$(which dsh)" npm run dev
```

A path ending in `.js`, `.mjs` or `.cjs` is run through `node`; anything else
is executed directly. Combine it with a pinned port when you want to hit the
same server from a browser at the same time:

```sh
DSH_DESKTOP_PORT=5173 DSH_DESKTOP_BACKEND=/path/to/lib/bin.js npm run dev
```

## Regenerating the app icon

`app-icon.png` is generated, not committed. `scripts/make-icon.mjs` rasterises
the DeepSeek whale mark from `assets/deepseek-whale.svg` over the app's blue
rounded-rect background — with a hand-rolled PNG encoder and a small
scanline rasteriser, so no image library is needed:

```sh
node scripts/make-icon.mjs        # writes app-icon.png (1024×1024)
npx tauri icon app-icon.png       # fans it out into src-tauri/icons/
```

`assets/deepseek-whale.svg` is the source of truth: it holds the mark exactly
as the harness itself serves it at `/favicon.svg`. To restyle the icon, edit
the background constants (`top`, `bottom`, `MARK_SCALE`) at the bottom of
`make-icon.mjs` rather than touching the artwork.

The rasteriser supports the `M`/`L`/`C`/`Z` subset the mark uses, absolute and
relative, filling with the nonzero winding rule and 4×4 supersampling. It
throws on any other command rather than silently drawing the wrong shape.

The generated per-platform icons under `src-tauri/icons/` **are** committed, so
that a fresh clone builds without running this step.

## Building distributables

`npm run backend:install` is a **prerequisite**, not an optional step: the
bundle config copies `backend/node_modules/` into the app resources so the
packaged app is self-contained.

```sh
npm run backend:install
npm run build
```

For a universal macOS binary (Apple Silicon + Intel), add both Rust targets and
pass the universal target:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run build -- --target universal-apple-darwin
```

Bundles land in `src-tauri/target/release/bundle/`:

| Platform | Output |
|---|---|
| macOS | `macos/DeepSeek Harness Desktop.app`, `dmg/*.dmg` |
| Windows | `msi/*.msi`, `nsis/*.exe` |
| Linux | `deb/*.deb`, `rpm/*.rpm`, `appimage/*.AppImage` |

Tauri does not cross-compile GUI bundles: each platform must be built on that
platform (or in a CI runner for it).

To build a slim app that resolves `dsh` on the target machine instead, remove
the `bundle.resources` entry from `src-tauri/tauri.conf.json` — see
[Shipping a self-contained app](../README.md#shipping-a-self-contained-app).

### Code signing

Unsigned builds are fine for local use but will be blocked by Gatekeeper on
macOS and SmartScreen on Windows for anyone else. Signing is configured through
Tauri's standard mechanism — see
<https://tauri.app/distribute/sign/> — and the secrets are wired into the
release workflow as repository secrets. No signing identity ships with this
repository.

## Releasing

1. Update the version in **three** places, keeping them identical:
   - `package.json` → `version`
   - `src-tauri/Cargo.toml` → `[package] version`
   - `src-tauri/tauri.conf.json` → `version`
2. Add the release notes to [CHANGELOG.md](../CHANGELOG.md).
3. Commit, then tag and push:

   ```sh
   git commit -am "release: v0.2.0"
   git tag v0.2.0
   git push origin main --tags
   ```

4. The [release workflow](../.github/workflows/release.yml) builds macOS,
   Windows and Linux from a matrix and attaches the bundles to a **draft**
   GitHub Release. Review the draft, then publish it.

## Repository hygiene

These are gitignored and must never be committed:

| Path | Size | Why it is ignored |
|---|---|---|
| `node_modules/`, `backend/node_modules/` | ~285 MB | Installable from the manifests. |
| `src-tauri/target/` | 1–2 GB | Build output. |
| `src-tauri/gen/schemas/` | small | Generated by `tauri-build` on every build. |
| `app-icon.png` | 62 KB | Regenerate with `node scripts/make-icon.mjs`. |
| `.DS_Store` | — | macOS noise. |

`backend/` intentionally has no lockfile, so `npm run backend:install` always
resolves the newest `@deepseek-ai/dsh` matching the semver range in
`backend/package.json`. Pin the exact version there if you need reproducible
backend installs.
