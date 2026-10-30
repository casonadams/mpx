# CLI Reference & Automation

`mpx` uses tmux command names wherever tmux has an equivalent, so scripts and muscle memory carry over seamlessly.

```bash
mpx [OPTIONS] [COMMAND]
```

### Global Options

- `-s, --session <NAME>`: Target session. Default: `$MPX_SESSION`, then `main`.
- `--endpoint <PATH>`: Daemon socket or Named Pipe. Default: `$MPX_ENDPOINT` when `-s` is not given, else the session socket.
- `-f, --config <FILE>`: Load this config file instead of the default location (see [CONFIG.md](CONFIG.md)). Like tmux `-f`, it goes before the command.
- `--state-dir <DIR>`: Durable state directory of the session's daemon (snapshots). Default: the session's directory under the state directory.
- `--idempotency-key <KEY>`: Run this command's mutating request at most once per key. A retry with the same key returns the first result instead of acting twice. Applies to `run`, `split`, `new-window`, `send`, `send-keys`, `paste`, `kill`, `kill-window`, `rename-window`, `rename-session`. Keys are kept for 10 minutes.
- `-h, --help` / `-V, --version`.

Inside an mpx pane, `MPX_SESSION`, `MPX_ENDPOINT`, `MPX_PANE`, `MPX_PANE_ID`, `TMUX`, and `TMUX_PANE` are set, so commands run there target their own session automatically.

### Targets

Commands that take a pane accept its name or its stable id `%N` (tmux style). TUI commands also accept `session:pane`.

---

## Sessions

| Command | Description |
| :--- | :--- |
| `mpx` / `mpx attach [-t session] [--force]` | Start the daemon if needed and attach the TUI. Refuses to nest inside an mpx pane unless `--force`. |
| `mpx list-sessions` (`ls`) `[--json]` | List sessions with window and pane counts. |
| `mpx has-session [-t session]` | Exit 0 if the session's daemon answers, 1 otherwise. |
| `mpx rename-session [-t target] <name>` (`rename`) | Rename a session. |
| `mpx kill-server` (`stop`, `kill-session`) | Kill every pane and stop the daemon process. |
| `mpx restore` | Recreate the session's windows and panes from its last snapshot (see [Session restore](#session-restore)). |

> `mpx ls` lists **sessions** (as in tmux). Use `mpx list-panes` (`list`, `lsp`) for panes.

---

## Panes and Processes

### `mpx run`
```bash
mpx run [--name N] [--pty] [--cwd DIR] [--env K=V]... [--remain] [--tmux-compat] <COMMAND> [ARGS...]
```

Options go before the command. Everything after `<COMMAND>` belongs to the command.

- `--pty`: interactive PTY (ConPTY on Windows) instead of a plain pipe.
- `--remain`: keep the pane on screen after the process exits (`remain_on_exit`).
- `--tmux-compat`: export `TMUX`/`TMUX_PANE` and put the mpx `tmux` shim first on `PATH`.

### `mpx list-panes` (`list`, `lsp`) `[--json]`
```
ID     NAME             PID      TYPE     STATUS         COMMAND
%0     watcher          41892    task     running        cargo watch -x test
%1     repl             41910    pty      running        python3
```

### `mpx status <name>`, `mpx kill <name>`, `mpx screen <name>`

`screen` prints the rendered 2D terminal grid as plain text.

### `mpx logs`
```bash
mpx logs [-n LINES | --tail LINES] [--plain] [--since OFFSET] [--json] [-f] <name>
```

- `--plain`: PTY scrollback rendered as text without escape sequences.
- `--since OFFSET` with `--json`: incremental reads (`{"data": ..., "next": N}`).
- `-f`: streams live output from where history ended without gaps.

### Input

| Command | Description |
| :--- | :--- |
| `mpx send <name> <data>` | Raw bytes to stdin (a newline is appended). |
| `mpx send-keys [-t pane] [-l] KEYS...` | tmux key names (`Enter`, `Escape`, `C-c`, `M-x`, `S-Tab`, `F5`, `Up`, ...). `-l` sends everything literally. |
| `mpx paste [-t pane] [--submit] <text \| ->` | Bracketed paste into the pane (stdin with `-`). `--submit` presses Enter afterwards. |

### `mpx wait`
```bash
mpx wait [--timeout SECS] [--idle MS | --match REGEX] <name>
```

| Condition | Satisfied when | Exit code |
| :--- | :--- | :--- |
| *(default)* | the process exits | the process exit code |
| `--idle MS` | no output for `MS` milliseconds | 0 |
| `--match REGEX` | the screen or output matches regex | 0 |

Timeout exits with `124`, matching coreutils `timeout`.

---

## Windows and Layout

| Command | Description |
| :--- | :--- |
| `mpx split [-t pane] [-h \| -v] [-l PERCENT] [-d] [--pty] [--env K=V] CMD...` | Split a pane (`-h` side by side, `-v` stacked; `-d` keeps focus). |
| `mpx new-window [-n name] [-c dir] [-d] [--pty] [--env K=V] [CMD...]` | New window (defaults to user's shell). |
| `mpx kill-window [-t index]` / `mpx rename-window [-t index] <name>` | Close / rename a window. |
| `mpx select-window -t <index>` / `mpx last-window` / `mpx last-pane` | Navigation. |
| `mpx focus <pane>` | Focus a pane (switches window if needed). |
| `mpx select-pane [-t <pane>] [-m] [-M]` | Focus a pane, mark it (`-m`), or clear the mark (`-M`). |
| `mpx swap-pane [--source <pane>] [-t <pane>]` | Swap two panes. |
| `mpx break-pane <pane>` | Move a pane into its own window. |
| `mpx select-layout [preset]` | `even-horizontal`, `even-vertical`, `main-vertical`, `main-horizontal`, `tiled`, `-E`. |
| `mpx resize-pane [-t pane] -L\|-R\|-U\|-D [cells]` | Move split border (or `-x`/`-y` for absolute size). |
| `mpx layout [--width W --height H]` | Print the computed pane geometry. |

---

## Session Restore

The daemon saves an atomic snapshot of the session's layout whenever it changes: windows, splits, focus, window names, and each pane's command, working directory, and environment.

- **After a crash or reboot**, the next start of the session restores it automatically (disable with `restore = false`).
- **After `kill-server`**, run `mpx restore` to bring the workspace back.

### Process Resumption
Panes restore in their recorded working directory (`cwd`):
- Standard panes restore their shell or command.
- Tools matching `[[resume]]` rules in `config.toml` expand their resume command or fallback:
  - Claude Code: `claude --resume {id}` or `claude --continue`
  - oh-my-pi: `omp --resume {id}` or `omp --continue`
  - Neovim: `nvim -S Session.vim`

---

## tmux Compatibility

When invoked as `tmux` (or via symlink in `socket_dir/bin/tmux`), `mpx` translates standard tmux CLI commands.

Supported: `split-window`, `new-window`, `new-session`, `send-keys`, `capture-pane`, `list-panes`, `list-windows`, `list-sessions`, `display-message`, `kill-pane`, `kill-window`, `kill-session`, `select-pane`, `select-window`, `select-layout`, `resize-pane`, `swap-pane`, `break-pane`, `last-window`, `last-pane`, `rename-window`, `rename-session`, `has-session`, `-V`.

`-F` format strings support `pane_id`, `pane_index`, `pane_pid`, `pane_title`, `pane_current_command`, `pane_current_path`, `pane_width`, `pane_height`, `pane_active`, `pane_dead`, `session_name`, `window_index`, `window_id`, and `window_name`.
