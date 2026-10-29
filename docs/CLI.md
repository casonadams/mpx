# CLI Reference & Automation

`mpx` uses tmux command names wherever tmux has an equivalent, so scripts and muscle memory carry over.

```
mpx [OPTIONS] [COMMAND]
```

### Global Options

- `-s, --session <NAME>`: Target session. Default: `$MPX_SESSION`, then `main`.
- `--endpoint <PATH>`: Daemon socket or Named Pipe. Default: `$MPX_ENDPOINT` when `-s` is not given, else the session socket.
- `-f, --config <FILE>`: Load this config file instead of the default location (see [CONFIG.md](CONFIG.md)). Like tmux `-f`, it goes before the command. It also applies to the daemon when `mpx` auto-starts it. Default: `$MPX_CONFIG` or the default config path.
- `--state-dir <DIR>`: Durable state directory of the session's daemon (snapshot and pipes). Default: the session's directory under the state directory (see [CONFIG.md](CONFIG.md#state-directory)). An auto-started daemon uses the same directory.
- `--idempotency-key <KEY>`: Run this command's mutating request at most once per key. A retry with the same key returns the first result instead of acting twice. Applies to `run`, `split`, `new-window`, `send`, `send-keys`, `paste`, `kill`, `kill-window`, `rename-window`, `pipe write`, `agents new|resume` and `hook`. Keys are kept for 10 minutes and are never read from the environment.
- `-h, --help` / `-V, --version`.

Inside an mpx pane, `MPX_SESSION`, `MPX_ENDPOINT`, `MPX_PANE` and `MPX_PANE_ID` are set, so commands run there target their own session without flags.

### Targets

Commands that take a pane accept its name or its stable id `%N` (tmux style). TUI commands also accept `session:pane`.

---

## Sessions

| Command | Description |
| :--- | :--- |
| `mpx` / `mpx attach [-t session] [--force]` | Start the daemon if needed and attach the TUI. Refuses to nest inside an mpx pane unless `--force`. |
| `mpx list-sessions` (`ls`) `[--json]` | List sessions with window and pane counts. |
| `mpx has-session [-t session]` | Exit 0 if the session's daemon answers, 1 otherwise. |
| `mpx kill-server` (`stop`, `kill-session`) | Kill every pane and stop the daemon process. The session snapshot is kept but marked clean, so the next start does not restore it automatically. Also stops a daemon left running by an incompatible (older) mpx version, which other commands refuse to talk to. |
| `mpx restore` | Recreate the session's windows, panes and agents from its last snapshot (see [Session restore](#session-restore)). Fails if the session already has panes. |

> `mpx ls` lists **sessions** (as in tmux). Use `mpx list-panes` (`list`, `lsp`) for panes.

---

## Panes and processes

### `mpx run`
```bash
mpx run [--name N] [--pty] [--cwd DIR] [--env K=V]... [--remain] [--tmux-compat] <COMMAND> [ARGS...]
```

Options go before the command. Everything after `<COMMAND>` belongs to the command, so `mpx run sh -c 'echo hi'` passes `-c` to `sh`. `--` before the command is optional.

- `--pty`: interactive PTY (ConPTY on Windows) instead of a plain pipe.
- `--remain`: keep the pane on screen after the process exits (`remain_on_exit`).
- `--tmux-compat`: export `TMUX`/`TMUX_PANE` and put the mpx `tmux` shim first on `PATH` (see [tmux compatibility](#tmux-compatibility)).

### `mpx list-panes` (`list`, `lsp`) `[--json]`
```
ID     NAME             PID      TYPE     STATUS         COMMAND
%0     watcher          41892    task     running        cargo watch -x test
%1     repl             41910    pty      running        python3
```
`--json` includes cwd, title, foreground process, timestamps, window index and agent state.

### `mpx status <name>`, `mpx kill <name>`, `mpx screen <name>`

`screen` prints the rendered terminal grid as text.

### `mpx logs`
```bash
mpx logs [-n LINES | --tail LINES] [--plain] [--since OFFSET] [--json] [-f] <name>
```
- `--plain`: PTY scrollback rendered as text, without escape sequences (the pane's retained output, up to 2 MB, replayed at its current width).
- `--since OFFSET` with `--json`: incremental reads. The response is `{"data": ..., "next": N}`, and you pass `N` back as `--since`.
- `-f`: prints the history, then streams live output from exactly where the history ended, with no gap or duplicate even while the pane is writing. If the stream falls behind, the missing bytes are re-read from the pane's log.

### Input

| Command | Description |
| :--- | :--- |
| `mpx send <name> <data>` | Raw bytes to stdin (a newline is appended). |
| `mpx send-keys [-t pane] [-l] KEYS...` | tmux key names (`Enter`, `Escape`, `C-c`, `M-x`, `S-Tab`, `F5`, `Up`, ...). Other words are sent as text; `-l` sends everything literally. Arrow keys honour the program's cursor-key mode. |
| `mpx paste [-t pane] [--submit] <text \| ->` | Paste text (stdin with `-`). Uses bracketed paste when the program enabled it, so multi-line prompts are not submitted line by line. `--submit` presses Enter afterwards. |

### `mpx wait`
```bash
mpx wait [--timeout SECS] [--idle MS | --match REGEX | --state STATE] <name>
```
| Condition | Satisfied when | Exit code |
| :--- | :--- | :--- |
| *(default)* | the process exits | the process exit code |
| `--idle MS` | no output for `MS` milliseconds | 0 |
| `--match REGEX` | the screen or new output matches | 0 |
| `--state needs_input\|working\|idle\|exited` | the agent reaches that state | 0 |

Timeout exits with `124`, like `timeout(1)`.

---

## Windows and layout

| Command | Description |
| :--- | :--- |
| `mpx split [-t pane] [-h \| -v] [-l PERCENT] [-d] [--pty] [--env K=V] [--tmux-compat] CMD...` | Split a pane. `-h` = side by side, `-v` = stacked (tmux semantics); `-d` keeps focus. Help is `--help`. |
| `mpx new-window [-n name] [-c dir] [-d] [--pty] [--env K=V] [CMD...]` | New window (defaults to the shell). A name given with `-n` stays; windows without one are auto-named. |
| `mpx kill-window [-t index]` / `mpx rename-window [-t index] <name>` | Close / rename a window. A renamed window stops auto-naming. |
| `mpx select-window -t <index>` / `mpx last-window` / `mpx last-pane` | Navigation. |
| `mpx focus <pane>` | Focus a pane (switches window if needed). |
| `mpx swap-pane --source <pane> -t <pane>` | Swap two panes (also across windows). |
| `mpx break-pane <pane>` | Move a pane into its own window. |
| `mpx select-layout [preset]` | `even-horizontal`, `even-vertical`, `main-vertical`, `main-horizontal`, `tiled`; no argument cycles; `-E` spreads the panes evenly, keeping their arrangement. |
| `mpx resize-pane [-t pane] -L\|-R\|-U\|-D [cells]` | Move the pane border (tmux semantics). `-x`/`-y` set an absolute size. |
| `mpx layout [--width W --height H]` | Print the computed pane geometry. |

Windows are named automatically after the agent harness or foreground process of their focused pane (`claude`, `nvim`, `cargo`) until renamed.

---

## Pipes (agent-to-agent messaging)

Pipes are sequenced message queues. Writing or reading a pipe creates it. With `persist_pipes = true` (the default), messages and consumer cursors are written to the state directory and survive daemon restarts, crashes included.

```bash
mpx pipe write bus '{"event":"tests_passed"}'          # prints nothing; message gets seq 1, 2, ...
mpx pipe read bus --consumer reviewer --wait 30000      # blocks up to 30s; resumes where 'reviewer' left off
mpx pipe read bus --since 41 --limit 10 --json          # {"messages":[{"seq":42,"ts_ms":...,"data":...}],"next":42}
mpx pipe subscribe bus                                  # stream messages as they arrive
mpx pipe list
mpx pipe create bus                                     # optional; idempotent
```

- `--consumer NAME`: the daemon keeps a cursor per consumer, so each reader gets every message exactly once. A new consumer starts after `--since` (or at the beginning).
- Without a consumer, `--since SEQ` is exclusive. Pipes drop the oldest messages beyond their capacity (1 MiB by default).


---

## Session restore

The daemon saves a snapshot of the session's layout whenever it changes: windows, splits, focus, renamed window names, and each pane's command, working directory and agent session.

- **After a crash or reboot**, the next start of the session restores it automatically before any client connects (disable with `restore = false`). Attaching shows `restored session from HH:MM:SS`.
- **After `kill-server`**, nothing is restored automatically; run `mpx restore` to bring the layout back.

What comes back:
- PTY panes run their original command in their last directory.
- Agent panes with a known session id are resumed through their harness (`claude --resume <id>`, `omp --resume <id>`, `rho --resume <id>`); other agents start fresh.
- Plain `mpx run` task panes (no `--pty`) are not restarted, because they are jobs, not workspace.

Snapshots live in the state directory (see [CONFIG.md](CONFIG.md#state-directory)).
---

## Agents

See the **[Agent Manager guide](AGENT-MANAGER.md)**.

| Command | Description |
| :--- | :--- |
| `mpx agents` | Attach with the agent manager open. |
| `mpx agents --json` | `{live, resumable}`: live agents across all sessions with state, plus stored sessions you can resume. |
| `mpx agents new <harness> [--resume ID] [--split] [--teams] [--cwd DIR] [--name N]` | Launch `claude`, `omp`, `codex`, `rho`, or a configured harness. |
| `mpx agents resume <id-prefix>` | Find a stored session in any harness store and resume it in a new window. |
| `mpx hook <harness> [event]` | Called by harness hooks; reads the event JSON on stdin and reports agent state for `$MPX_PANE`. Exits 0 silently outside mpx. |
| `mpx hook print-config claude` | Print the Claude Code `settings.json` hooks block for agents started outside `mpx agents new`. |

---

## tmux compatibility

When the `mpx` binary is invoked as `tmux` (a symlink), it translates tmux commands into mpx requests. Panes started with `--tmux-compat` (or `tmux_compat = true` in the config, or `mpx agents new claude --teams`) get `TMUX`, `TMUX_PANE` and a `PATH` entry for the shim, so tools that drive tmux, such as Claude Code agent teams, create mpx panes instead.

Supported: `split-window`, `new-window`, `new-session`, `send-keys`, `capture-pane`, `list-panes`, `list-windows`, `list-sessions`, `display-message`, `kill-pane`, `kill-window`, `kill-session`, `select-pane`, `select-window`, `select-layout`, `resize-pane`, `swap-pane`, `break-pane`, `last-window`, `last-pane`, `rename-window`, `has-session`, `-V`. `-F` format strings support `pane_id`, `pane_index`, `pane_pid`, `pane_title`, `pane_current_command`, `pane_current_path`, `pane_width`, `pane_height`, `pane_active`, `pane_dead`, `session_name`, `window_index`, `window_id` and `window_name`. Option commands (`set-option`, `bind-key`, ...) are accepted as no-ops.

---

## MCP server

`mpx mcp` runs the stdio MCP server. See the **[MCP Guide](MCP.md)**.
