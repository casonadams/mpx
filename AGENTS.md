# Repository instructions

## Testing

- **Test layout**: Put every test in `tests/<mod>/<file>.rs`, rooted by
  `tests/<mod>.rs` (`mod <mod> { mod <file>; }`), with one root per top-level
  `src/` module (`tests/vt.rs` -> `tests/vt/grid.rs`, `tests/vt/parser.rs`).
  A module whose tests fit in one file may keep them directly in
  `tests/<mod>.rs`. Never add inline `#[cfg(test)]` modules in `src/`.
- **Public API only**: Tests import `mpx::...` public items. When logic cannot
  be reached through the public API, extract it into a pure `pub` function in
  its domain module instead of testing private internals.
- **Fast and in-memory**: Pure-logic tests do no disk, network, or process
  I/O; feed in-memory strings and byte slices (`< 500ms` per test). The only
  exception is end-to-end daemon, PTY, and CLI tests under `tests/daemon/` and
  `tests/cli/`, which may bind a socket in a temp dir and spawn child processes,
  and on-disk persistence tests under `tests/store/`, which may read and write a
  temp dir. Never touch the real state or runtime directories in tests: pass
  temp paths explicitly instead of setting process-wide environment variables.
  Bound every wait with a timeout and keep each test under 2s.
- **Behavior, not wiring**: Assert consumer-visible behavior (outputs, state
  transitions, boundaries, errors). No tests of forwarding, copies, or
  incidental defaults.

## Key bindings and UX

- tmux muscle memory comes first. When a binding is ambiguous, conflicts, or
  has no tmux precedent, follow vim: `h/j/k/l` movement, `gg`/`G`,
  `Ctrl-d`/`Ctrl-u`, `/` search with `n`/`N`, `:` commands (`:split`,
  `:vsplit`, `:q`), `y` yank, `Esc`/`q` close.

## Verification

- Run `cargo test` and `cargo clippy --all-targets` before finishing.
- When changing CLI commands, flags, MCP tools, key bindings, or config keys,
  update `README.md`, `docs/`, and `www/` in the same change.
