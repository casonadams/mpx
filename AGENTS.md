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
  I/O; feed in-memory strings and byte slices (`< 500ms` per test). Tests whose
  subject *is* I/O may use real I/O, confined to temp dirs and in-process test
  daemons:
  - `tests/daemon/` and `tests/cli/`: end-to-end daemon, PTY and CLI tests
    (temp-dir sockets, child processes);
  - `tests/store/`: on-disk persistence in a temp dir;
  - `tests/tui/app/`: app-level TUI tests driving `tui::App` against an
    in-process daemon in a temp dir (no real terminal);
  - any module's tests may read or write a temp file passed explicitly to the
    function under test, and may introspect the test's own process.

  Never touch the real state, runtime, or config directories or the user's
  sessions: thread paths through function parameters (`socket_dir`,
  `state_dir`, `AppOptions`) instead of setting process-wide environment
  variables. Bound every wait with a timeout and keep each test under 2s.
- **Coverage**: Every function in the blast radius of a change must be reached
  by a test (`ripwire-rs . --test-gate` reports `untested="0"`; it exits 4
  whenever the change has tests to run, which is expected). Pass untracked
  files explicitly (`--test-gate=<files>`). The static graph cannot follow
  channel-dispatched actor messages or the `tmux` argv0 subprocess; for rows
  it still flags, prove execution with `cargo llvm-cov` (count > 0) and add a
  behavior test for any function that never runs. When a function reads
  global state (environment, real directories, the terminal), make that input
  a parameter so tests can reach it.
- **Behavior, not wiring**: Assert consumer-visible behavior (outputs, state
  transitions, boundaries, errors). No tests of forwarding, copies, or
  incidental defaults.

## Key bindings and UX

- tmux muscle memory comes first. When a binding is ambiguous, conflicts, or
  has no tmux precedent, follow vim: `h/j/k/l` movement, `gg`/`G`,
  `Ctrl-d`/`Ctrl-u`, `/` search with `n`/`N`, `:` commands (`:split`,
  `:vsplit`, `:q`), `y` yank, `Esc`/`q` close.

## Rendering and the terminal model

- vim/nvim rendering has broken before from TUI and VT changes; treat any
  change to `src/tui/{canvas,render,frame}.rs` or `src/vt/` as high risk.
- `tests/tui/app/fidelity.rs` must keep passing: after every frame, each pane
  region of the harness terminal must equal that pane's own screen. Extend it
  when adding a render path (it covers scroll regions, IL/DL, the alternate
  screen, overlays, the HUD, zoom and non-ASCII rows).
- App tests assert on what the harness terminal shows (`Harness::frame()`,
  `Harness::cursor()`), never on raw frame bytes: frames are incremental.
- The screen model has no width table: never assume where the terminal cursor
  lands after a non-ASCII glyph. Rows with non-ASCII are reprinted whole.
- Before finishing a rendering change, run real `vim` and `nvim` (syntax,
  scrolling, splits, visual mode, wide characters, an mpx split and overlay)
  in an isolated session and compare against the previous build.

## Verification

- Run `cargo test` and `cargo clippy --all-targets` before finishing.
- When changing CLI commands, flags, MCP tools, key bindings, or config keys,
  update `README.md`, `docs/`, and `www/` in the same change.
