## What and why

<!-- What does this change, and what problem does it solve? -->

## How it was verified

<!-- Commands you ran and what you observed. Compilation passing is not the
     same as the lifecycle working — say whether the app still starts, loads
     the harness UI, and leaves no orphan server on close. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] The app starts and the window loads the harness UI
- [ ] No backend process survives closing the window (`pgrep -fl "dsh.*web"`)

## Platforms tested

<!-- e.g. macOS 15 (Apple Silicon). Testing on one platform is fine — just say
     which. -->

## Notes

- [ ] Documentation updated for any behaviour change
- [ ] This change is in scope for a thin native shell (see CONTRIBUTING.md)
