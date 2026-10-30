# CLI Reference & Automation

`mpx` uses tmux command names wherever tmux has an equivalent, so scripts and muscle memory carry over seamlessly.

```bash
mpx [OPTIONS] [COMMAND]
```

### Global Options

- `-s, --session <NAME>`: Target session. Default: `$MPX_SESSION`, then `0`, as tmux names its first session.
- `--endpoint <PATH>`: Daemon socket or Named Pipe. Default: `$MPX_ENDPOINT` when `-s` is not given, else the session socket.
- `-f, --config <FILE>`: Load this config file instead of the default location (see [CONFIG.md](CONFIG.md)). Like tmux `-f`, it goes before the command.
- `--state-dir <DIR>`: Durable state directory of the session's daemon (snapshots). Default: the session's directory under the state directory.
- `--no-restore`: Start the session empty instead of restoring its saved layout. Only applies when the session's daemon is not running yet; `mpx attach` refuses it for a running session (stop that with `mpx kill-server` first).
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
| `mpx new-session [-d] [-s name]` (`new`) | Create a session with no restored state: the next free number (`1`, `2`, ...) unless `-s` names it; `duplicate session` if it is already running. `-d` creates it without attaching. |
| `mpx list-sessions` (`ls`) `[--json]` | List sessions with window and pane counts. |
| `mpx has-session [-t session]` | Exit 0 if the session's daemon answers, 1 otherwise. |
| `mpx rename-session [-t target] <name>` (`rename`) | Rename a session. |
| `mpx kill-server` (`stop`, `kill-session`) | Kill every pane and stop the daemon process. |
| `mpx restore` | Recreate the session's windows and panes from its last snapshot (see [Session restore](#session-restore)). |
| `mpx import-tmux [FILE]` | Print an mpx `config.toml` fragment converted from a tmux config (default: `~/.tmux.conf`, else `$XDG_CONFIG_HOME/tmux/tmux.conf` or `~/.config/tmux/tmux.conf`). Writes no files. See [Importing tmux config](#importing-tmux-config). |

> `mpx ls` lists **sessions** (as in tmux). Use `mpx list-panes` (`list`, `lsp`) for panes.

## Importing tmux config

`mpx import-tmux` reads a tmux config and prints the equivalent mpx settings as TOML to stdout. Redirect it into your mpx config yourself; nothing is written.

```sh
mpx import-tmux ~/.tmux.conf > /tmp/mpx-fragment.toml
```

- Converted: `prefix` (`set -g prefix`), `history-limit` (to `scrollback_lines`), `remain-on-exit`, `allow-passthrough`, `automatic-rename`, `monitor-activity`, `monitor-bell`, `default-terminal`, `default-shell`, `default-command`, `display-panes-time`, `status-style` and `pane-border-style`/`pane-active-border-style` colours (`fg`/`bg` as mpx colour names or hex), `pane-border-lines` (single, double, rounded, simple as ascii), and `bind`/`bind -n`/`bind -T root`/`unbind` for commands mpx accepts.
- Skipped or approximated lines are listed on stderr as `# line N: ...` and again in the output as `# unsupported (line N): ...` comments. Examples: `run`/`run-shell` and TPM plugins (never executed), `source-file` (only the given file is converted), `%if` blocks (skipped; use `source-file` for a live, evaluated config), `mouse` and `escape-time` (no mpx setting), `bind` of commands mpx does not have, and `split-window -c '#{pane_current_path}'` (dropped, since mpx already starts new panes in the active pane's directory).
- `unbind KEY` is written as an empty binding, which removes mpx's default for that key.
- `~` and `$HOME` are copied as written.

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
| `mpx select-layout [preset]` | `even-horizontal`, `even-vertical`, `main-vertical`, `main-vertical-mirrored`, `main-horizontal`, `main-horizontal-mirrored`, `tiled`, `-E`. |
| `mpx resize-pane [-t pane] -L\|-R\|-U\|-D [cells]` | Move split border (or `-x`/`-y` for absolute size). |
| `mpx resize-pane [-t pane] -Z` | Zoom the pane's window so its focused pane fills it, or unzoom it. Zoom is the daemon's, so every attached terminal shows it. |
| `mpx layout [--width W --height H]` | Print the computed pane geometry. |

---

## Session Restore

The daemon saves an atomic snapshot of the session's layout whenever it changes: windows, splits, focus, window names, and each pane's command, working directory, and environment.

Earlier versions named the default session `main`. The first plain `mpx` moves a saved `main` state to `0`, unless a `main` daemon is still running; stop it with `mpx kill-server -s main` first.

- **Whenever a session's daemon starts** (`mpx`, or any command that auto-starts it), it restores the last saved layout, whether the previous run ended in a crash, a reboot, or `kill-server`. Disable with `restore = false`.
- **For a brand new state**, start with `mpx --no-restore`. The old snapshot is replaced as soon as the new session saves.
- `mpx restore` replays the snapshot on demand, for example when `restore = false`.

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

Supported: `split-window`, `new-window`, `new-session`, `attach-session`, `send-keys`, `capture-pane`, `list-panes`, `list-windows`, `list-sessions`, `display-message`, `display-popup`, `kill-pane`, `kill-window`, `kill-session`, `kill-server`, `select-pane`, `select-window`, `select-layout`, `resize-pane`, `swap-pane`, `break-pane`, `last-window`, `last-pane`, `rename-window`, `rename-session`, `command-prompt`, `confirm-before`, `has-session`, `-V`, and the commands in the tables below.

`-F` format strings (`list-panes`, `list-windows`, `list-sessions`, `display-message`, and `-P` on `split-window`/`new-window`) use the tmux grammar:

- `#{name}` expands a variable; unknown names expand to empty. `#S #I #P #W #T #D` are aliases for the session, window, pane, window name, pane title and pane id. `##` is a literal `#`.
- Conditionals `#{?cond,then,else}`, with chains `#{?c1,v1,c2,v2,else}`. A condition is a variable or a nested format, true when non-empty and not `0`.
- Comparisons `#{==:a,b}`, `!=`, `<`, `>`, `<=`, `>=` (numeric when both sides are integers, otherwise string order), and `#{&&:a,b}`, `#{||:a,b}`, `#{!:a}`. They yield `1` or `0`. Arguments are literal text or nested formats.
- Modifiers: `#{=N:var}` keeps the first N characters, `#{=-N:var}` the last N. `#{s/pat/rep/:var}` replaces every regex match with a literal replacement (flag `i` ignores case; neither pattern nor replacement may contain `/`). `#{b:var}` basename, `#{d:var}` dirname, `#{l:text}` literal text, `#{E:var}` expands the value again as a format, `#{n:var}` length in characters, `#{q:var}` shell-escapes it. Other modifiers (such as `#{t:...}`) expand to empty.
- Inside `#{...}`, `#,` is a comma and `#}` a closing brace. An unclosed `#{` is printed as text.

Variables:

- Pane: `pane_id`, `pane_index`, `pane_pid`, `pane_title`, `pane_current_command`, `pane_current_path`, `pane_width`, `pane_height`, `pane_active`, `pane_dead`, `pane_marked`, `pane_marked_set`, `pane_in_mode` and `pane_mode` (see [Copy mode](#copy-mode)), `pane_tty`, `pane_start_command`. `pane_left`, `pane_top`, `pane_right` and `pane_bottom` come from the current layout and are empty for a pane that is not laid out.
- Window: `window_index`, `window_id`, `window_name`, `window_active`, `window_panes`, `window_flags` (`#` activity, `!` bell, `*` active, `Z` zoomed, `-` last, `M` holds the marked pane), `window_zoomed_flag` (`1` when the window is zoomed).
- Session: `session_name`, `session_windows` (set for pane and window formats; `list-sessions` does not report counts yet, so it prints `0`), `session_attached` (always `1`), `client_session`.
- Host: `host`, `host_short`, `pid` (the shim process), `version` (`3.4`).
- Not available, so they expand to empty: `session_id`, `session_created`, `start_time`.

### Sessions, servers and targets

A tmux server holds many sessions; `mpx` runs one daemon per session. A *server* is therefore a directory of session sockets:

- `-L name` and `-S path` each select their own server (their own sessions and saved state), separate from the default one. `-L default` is the default server. Commands without `-t` talk to the lowest-numbered running session of the server.
- Inside an mpx pane, `$TMUX` selects the pane's own session. Outside one, a command talks to session `0`, or to the lowest-numbered running session when `0` is down.
- `-t session:...` addresses another running session of the same server.
- `kill-server` stops every session of the server. `kill-session` stops the current one. `list-sessions` and `kill-server` fail with `no server running` when nothing runs.
- `new-session [-d] [-s name] [-n window] [-c dir] [-A] [-P -F fmt] [command]` starts a fresh session (no restore), named by the next free number unless `-s` is given; `duplicate session: name` when it runs (`-A` attaches to it instead). `-x`/`-y` are accepted and ignored.
- `split-window` stacks panes (top / bottom); `-h` puts them side by side, as in tmux.

### More commands

| Command | Notes |
| :--- | :--- |
| `next-window`, `previous-window`, `select-window -n/-p/-l` | Window navigation. |
| `swap-window -s a -t b`, `move-window -s a -t b` | Windows are positions; `move-window` moves to index `b`. `-r` is accepted and does nothing. |
| `join-pane -s src -t dst [-h]` | Moves a pane next to another, across windows. `-b`, `-l` are accepted and ignored. |
| `pipe-pane [-o] [command]` | Pane output goes to the shell command until the pane exits; without a command piping stops; `-o` toggles. `-I` is not supported. |
| `detach-client`, `switch-client -t session` / `-n` / `-p` | Run in every attached terminal of the session. |
| `set-buffer`, `show-buffer`, `list-buffers`, `delete-buffer`, `load-buffer`, `save-buffer`, `paste-buffer` | The session's paste buffers, newest first, at most 50. Copy-mode yanks and `capture-pane` without `-p` push a buffer. `paste-buffer` turns line feeds into carriage returns unless `-r` / `-s`; `-p` pastes bracketed. |
| `wait-for [-S\|-L\|-U] channel` | Blocks until signaled (a signal sent first is remembered), or takes / releases the channel lock. |
| `rotate-window [-D] [-U] [-Z] [-t window]` | Rotates the active window's panes through its layout: by default each pane moves to the slot before its own (the first pane goes last); `-D` moves each pane to the slot after. Geometry stays, and focus stays on the same pane. `-t` must name the active window. `-Z` is accepted and ignored. |
| `find-window [-CNTrZ] [-t window] match` | Lists the windows whose name (`-N`), pane title (`-T`) or screen (`-C`) contains `match`, one `index: name [WxH] (N panes) "title"` line each. Without `-C`, `-N` or `-T` all three are searched; `-r` reads `match` as a regex. No match fails with `no windows matching: match`. `-Z` is accepted with no effect, as for `rotate-window`. |
| `respawn-pane [-k] [-c dir] [-t pane] [command]` | Restarts the pane's process in place: same name and layout slot, and the same command and directory unless given. A running process needs `-k`, otherwise the command fails with `pane %N still active`. An exited pane keeps its `%N`; a process replaced with `-k` gets a new one. |
| `respawn-window [-k] [-c dir] [-t window] [command]` | Respawns the focused pane of the active window, as `respawn-pane` does. `-t` must name the active window. |
| `run-shell [-b] [-C] [-d secs] command` | Prints the output; a non-zero exit fails with `'command' returned N`. `-C` runs a tmux command instead. |
| `if-shell [-F] condition then [else]` | The condition is a shell command (exit 0), or with `-F` a format. The branches are tmux command strings (`;` separates commands). |
| `source-file [-nq] file...` | Runs each line as a tmux command, honoring `%if` / `%elif` / `%else` / `%endif` (conditions are formats) and `~` in paths; failures are reported as `file:line: error`. `-n` only checks. Also works at the `:` prompt, so `:source-file ~/.tmux.conf` applies a tmux config to the running session. |
| `bind-key [-n] [-T prefix\|root] key command...`, `unbind-key [-a] [-n] [-T table] key`, `list-keys [-T table]` | Bindings are changed in the running session and reach every attached terminal at once, and any that attaches later. A command is one string or separate words; `;` chains commands (`\;` at the `:` prompt). The copy-mode tables `copy-mode-vi` and `copy-mode` take only `send-keys -X` commands (see [Copy mode](#copy-mode)); other tables are refused. `-r` is accepted and ignored. |
| `set-hook [-gua] hook command`, `show-hooks` | Runs the command (a tmux command string) when the event happens: `after-new-window`, `after-split-window`, `after-select-window`, `after-select-pane`, `after-kill-window`, `after-resize-pane`, `after-select-layout`, `after-rename-window`, `after-rename-session`, `after-break-pane`, `after-join-pane`, `after-swap-pane`, `after-send-keys`, `pane-exited`, `client-attached`, `client-detached`, `session-closed`. An unknown hook name is an error. A failed request fires no `after-*` hook. |

Options and environment:

- `set-option`, `show-options`, `set-environment`, `show-environment` (and `set-window-option`, `setenv`, ...) keep a name/value store per session, so values read back (`show-options -gv @plugin-var`). `-g`, `-s`, `-w`, `-t` select no scope: there is one store per session.
- Options mpx has an equivalent for act on the session (see the [tmux options table](CONFIG.md#tmux-options-set-option)): `remain-on-exit`, `history-limit`, `allow-passthrough`, `automatic-rename`, `monitor-activity`, `monitor-bell`, `synchronize-panes` (per window), `default-terminal`, `default-shell`, `default-command` and `display-panes-time`. Others are only stored, with a few read-only answers that match mpx: `base-index` and `pane-base-index` are `0`. Invalid values are refused with an error.
- `set-environment` does apply: later panes of the session start with the variable.
- Reading an option that was never set fails with `invalid option` unless `-q` is given.

Accepted and ignored, so tools that configure tmux keep working: `refresh-client`. Any other command fails with `unknown command`. Not supported: `display-menu`, `choose-client`, control mode (`-C`).

`command-prompt [-1bFkN] [-I initial[,...]] [-p prompt[,...]] [-T command|search|target|window-target] [template]` asks on the attached terminals, as tmux does. With a template, each prompt's answer replaces `%%` (or `%1`..`%9` for the nth prompt; `%%%` quotes the answer for the command parser) and the result runs; without one, the typed line runs. `-1` and `-k` take one key with no Enter, `-N` accepts digits only, `-F` expands the prompt and initial text as formats, and `-T window-target` completes window names. `confirm-before [-by] [-p prompt] command` asks `y`/`n` (default `Confirm 'command'? (y/n)`) and runs the command on `y`; `-y` runs it at once.

### Copy mode

`copy-mode [-eHMqu] [-t pane]` enters copy mode in every terminal attached to the session, as the `Prefix [` key does. `-e` leaves once scrolling returns to the live screen, `-u` also pages up into history, `-q` leaves copy mode. `-H` and `-M` are accepted with no effect. Copy mode belongs to a pane: a pane that is not focused keeps its copy state, as in the TUI.

`send-keys -t pane -X [-N n] command [args]` runs one copy-mode command in the copy mode of the pane, `n` times (default 1). The pane must be in copy mode, else the command fails with `pane %N not in a mode`. A name mpx does not implement fails with `copy-mode command not supported: NAME`; a name that needs an argument without one fails with `send-keys -X NAME: needs an argument` (`needs a character` for the `jump-*` commands). `goto-line` needs a number (`invalid line 'x'` otherwise). `-N` without `-X` fails with `send-keys: -N needs -X`. The name and arguments are checked before the pane's mode, so a bad name fails the same way in any pane.

The commands mpx runs:

| Group | Commands |
| :--- | :--- |
| Cursor | `cursor-left`, `cursor-right`, `cursor-up`, `cursor-down`, `cursor-down-and-cancel` (leaves when the cursor is at the bottom of the screen), `start-of-line`, `end-of-line`, `back-to-indentation`, `top-line`, `middle-line`, `bottom-line`, `goto-line n` |
| Words and columns | `next-word`, `next-word-end`, `previous-word`, `next-space`, `next-space-end`, `previous-space`, `forward-four-columns`, `back-four-columns` |
| Jumps | `jump-forward c`, `jump-backward c`, `jump-to-forward c`, `jump-to-backward c` |
| Scrolling | `history-top`, `history-bottom`, `page-up`, `page-down`, `halfpage-up`, `halfpage-down`, `halfpage-down-and-cancel`, `scroll-up`, `scroll-down` |
| Search | `search-forward text`, `search-backward text`, `search-again`, `search-reverse`, `search-prompt-forward`, `search-prompt-backward` (open the `/` or `?` prompt) |
| Selection | `begin-selection`, `select-line`, `select-word`, `clear-selection`, `other-end`, `rectangle-toggle` (the selection covers the rectangle between its corners; toggling again returns to a run of text; without a selection it starts one at the cursor) |
| Copying | `copy-selection`, `copy-selection-no-clear`, `copy-selection-and-cancel`, `copy-selection-or-line-and-cancel`, `copy-selection-if-any`, `copy-line`, `copy-line-and-cancel`, `copy-end-of-line`, `copy-end-of-line-and-cancel` |
| Piping | `copy-pipe command`, `copy-pipe-no-clear`, `copy-pipe-and-cancel`, `copy-pipe-line`, `copy-pipe-line-and-cancel`, `copy-pipe-end-of-line`, `copy-pipe-end-of-line-and-cancel`. The text goes to the shell command's standard input and its output is discarded. |
| Leaving | `cancel` |

Copied text goes to the session's paste buffers (`show-buffer`) and, unless `set-clipboard off`, to the system clipboard. The shim returns only after every attached terminal has applied the command (at most 2 seconds), so `send-keys -X copy-selection-and-cancel` followed by `show-buffer` reads the copied text. Known difference: a character selection keeps the blank cells at the end of its last row, where tmux trims them.

Key tables: `copy-mode-vi` is the table for `mode-keys vi` and `copy-mode` the table for `mode-keys emacs` (see [KEYBINDINGS.md](KEYBINDINGS.md#vi-copy-mode-prefix-)). `bind-key -T copy-mode-vi KEY send-keys -X command [args]` binds a key in the table, `unbind-key -T copy-mode-vi KEY` removes it, and `list-keys -T copy-mode-vi` lists the table as `bind-key -T` lines. A binding takes effect in every attached terminal at once. Other commands in a copy-mode table are refused with `copy-mode-vi: only send-keys -X commands can be bound`.

`#{pane_in_mode}` is `1` while any attached terminal has the pane in copy mode, else `0`; `#{pane_mode}` is `copy-mode` in that case and empty otherwise. Each terminal reports its own copy mode to the daemon, and a terminal's marks are dropped when it disconnects.

Not implemented: commands mpx has no counterpart for, such as the mark commands (`set-mark`, `jump-to-mark`) and `rectangle-on` / `rectangle-off`, fail with `copy-mode command not supported`. `choose-client` is refused; `choose-tree` and `choose-buffer` open the overlay on every attached terminal.

Zoom follows tmux: focusing another pane (`select-pane`, `last-pane`, `next-pane`), swapping panes (`swap-pane`), splitting, and switching windows unzoom the window. `select-pane -Z`, `last-pane -Z` and `swap-pane -Z` keep a zoomed window zoomed. `after-resize-pane` fires on each zoom toggle.

Buffers, options, the environment and `wait-for` channels belong to one session, so a tool that sets a buffer in one session and pastes it in another will not find it. Inside an mpx pane every command reaches the pane's own session, which is what such tools expect.

### Popups

```bash
tmux display-popup [-BCE] [-b border-lines] [-d start-dir] [-e VAR=val]... [-h height] [-w width] [-x pos] [-y pos] [-S border-style] [-t target] [shell-command]
```

Runs `shell-command` (through `sh -c`; the default shell without one) in a floating pane drawn over the attached terminal, with a border. Keys go to the popup while it is open.

- **Size and place**: `-w`/`-h` are cells or `N%` (default 50% of the pane area). `-x` and `-y` are cells, or `C` (centred, the default), `L`/`T`/`W` (the start edge), `R`/`B` (the end edge), or `P` (the active pane's position). A popup that does not fit is kept on screen.
- **Border**: `-B` draws none. `-b` takes `single`, `double`, `rounded` or `simple` (ASCII), and `none` is `-B`; `heavy` and `padded` are refused. Without `-b` the pane border lines (`border_style`) are used. `-S fg=COLOUR` sets the border colour.
- **Closing**: without `-E` the popup stays after its program exits, showing the output, until a key is pressed. `-E` closes it when the program exits; `-EE` closes it only when the status is 0. `-C` closes the open popup and stops its program.
- **One at a time**: a session has one popup; a second is refused with `a popup is already open`.
- **Blocking**: from a script, `display-popup -E ...` (or `-EE`) blocks until the popup closes and exits with the program's status (1 when there is none, e.g. after `-C`). Without `-E` the call returns at once with status 0.
- **Not used**: `-s` (popup style) is accepted and ignored. `-t` is accepted and ignored: every terminal attached to the session shows the popup, as mpx has no per-client targeting. Popups opened from the prompt, a hook or `if-shell` return at once, even with `-E`.
- The popup's pane is not part of the layout: `list-panes` does not list it, and the session snapshot does not keep it.

