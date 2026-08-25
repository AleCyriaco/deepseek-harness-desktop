# Contributing

Thanks for considering a contribution. This is a small, deliberately minimal
project, so the most valuable thing you can read before writing code is the
[scope boundary](#scope-what-belongs-here) below.

## Contents

- [Scope: what belongs here](#scope-what-belongs-here)
- [Getting set up](#getting-set-up)
- [Before you open a pull request](#before-you-open-a-pull-request)
- [Coding conventions](#coding-conventions)
- [Commit messages](#commit-messages)
- [Pull requests](#pull-requests)
- [Reporting bugs](#reporting-bugs)
- [Code of conduct](#code-of-conduct)

## Scope: what belongs here

This repository is a **thin native shell** around the upstream DeepSeek
Harness. It starts the harness server, shows it in a window, and cleans up
afterwards. That is the whole job.

**In scope** — things only a native shell can do:

- window behaviour: state persistence, single-instance enforcement, deep links
- native menus, a system tray icon, keyboard shortcuts at the OS level
- packaging, code signing, auto-update
- backend lifecycle: resolution, startup, teardown, error reporting
- documentation, CI, tests

**Out of scope** — anything about the agent itself:

- chat, sessions, models, prompts, tools, plugins, permissions, settings UI

Those live in the [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
and should be proposed there. If a change here would require adding a Tauri IPC
command that the harness UI has to call, it is almost certainly out of scope.

## Getting set up

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the full guide. The short
version:

```sh
npm run setup
npm run dev
```

## Before you open a pull request

Run exactly what CI runs:

```sh
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Then confirm the app still starts and the window loads the harness UI —
compilation passing is not the same as the lifecycle working.

If your change touches startup or shutdown, verify the teardown by hand: close
the window and check that no server survives.

```sh
pgrep -fl "dsh.*web"    # should print nothing
```

## Coding conventions

### Rust

- `rustfmt` defaults; `clippy` clean at `-D warnings`.
- **Doc comments carry the reasoning.** Every module and public item in
  `src-tauri/src/` has a `//!` or `///` comment explaining *why*, not just
  what. Keep that up.
- **Fail with a message, never a panic**, on any path a user can reach.
  Startup errors return `Err(String)` with a message that names the fix — the
  "no backend found" message listing three remedies is the model to follow.
- **Prefer pure functions for logic.** `extract_url` is a free function taking
  a `&str` precisely so it can be unit-tested without spawning anything. New
  parsing or resolution logic should follow the same shape.
- **Guard platform code with `cfg`**, and provide every branch. `terminate_group`
  has a `#[cfg(unix)]` and a `#[cfg(windows)]` implementation; a new platform
  path needs both.

### JavaScript

The two scripts in `scripts/` are plain ESM with **zero dependencies**, and
that is a feature — `make-icon.mjs` hand-rolls a PNG encoder rather than pull
in an image library. Please keep the dependency count at zero.

### Documentation

- All documentation, comments, commit messages and identifiers are in
  **English**.
- If you change behaviour, update the affected document in the same pull
  request. The docs are part of the change, not a follow-up.

## Commit messages

Conventional-commit style, imperative mood:

```
feat: persist window size and position across launches
fix: kill the backend group when window creation fails
docs: document DSH_DESKTOP_PORT on packaged macOS builds
chore: bump @tauri-apps/cli to 2.1
```

Prefixes in use: `feat`, `fix`, `docs`, `refactor`, `test`, `build`, `ci`,
`chore`.

## Pull requests

- One logical change per pull request.
- Describe what changed and **why**, and how you verified it.
- Note the platforms you tested on — a shell like this fails in
  platform-specific ways, and "macOS only" is useful information, not a
  disqualifier.
- Draft PRs are welcome for early feedback.

## Reporting bugs

Before filing, run the
[isolation test](docs/TROUBLESHOOTING.md#is-it-the-shell-or-the-harness) so we
know whether the bug is in this shell or upstream.

A good report includes:

- OS and version, Node version (`node -v`), Rust version (`rustc -V`)
- whether it is a dev build (`npm run dev`) or a packaged bundle
- the full stderr output — every shell diagnostic starts with `dsh-desktop:`
- which backend resolution path is in play (bundled, `PATH`, `npx`, or
  `DSH_DESKTOP_BACKEND`)

## Code of conduct

Be decent to each other. Assume good faith, keep criticism about the code, and
skip anything you would not say to a colleague in person. Behaviour that makes
the project a worse place to participate is grounds for having a contribution
declined.
