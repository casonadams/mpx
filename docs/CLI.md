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
| `mpx` / `mpx attach [-t session] [-d] [-x] [-r] [--force]` | Start the daemon if needed and attach the TUI. `-d` (and `-x`, which mpx treats the same) detaches the session's other clients first. `-r` attaches read-only: keys, paste and mouse reports do not reach panes; only navigation, copy mode, overlays and detach are allowed. Refuses to nest inside an mpx pane unless `--force`. |
| `mpx new-session [-d] [-s name]` (`new`) | Create a session with no restored state: the next free number (`1`, `2`, ...) unless `-s` names it; `duplicate session` if it is already running. `-d` creates it without attaching. `-t` (session groups) is refused: each session owns its panes in its own daemon, so windows cannot be shared between sessions. |
| `mpx list-clients [-t session] [-F format]` (`lsc`) | One line per attached client across sessions, or the clients of `-t`. Format variables: `client_name`, `client_tty`, `client_width`, `client_height`, `client_session`, `client_readonly`, `client_created`, `client_activity`, `session_attached` (clients of the client's session). Default `#{client_name}: #{client_session} [WxH]`. |
| `mpx detach-client [-t client] [-a]` | Detach `client`, or with `-a` the other clients of its session. Without `-t` the most recently active client is detached. Pick a session first with `mpx -s name detach-client`. |
| `mpx refresh-client [-t client] [-C WxH] [-S]` | Redraw clients (all when `-t` is absent). `-C` gives the client a size for the window-size policy until its terminal reports a new size. `-S` is accepted; the whole client is redrawn either way. |
| `mpx switch-client [-c client] (-t session \| -l \| -n \| -p)` (`switchc`) | Switch a client to a session, or to the last, next or previous one. The command runs inside the client, so the client must be attached. `-c` defaults to the most recently active client. |
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

- Converted: `prefix` (`set -g prefix`), `history-limit` (to `scrollback_lines`), `remain-on-exit`, `allow-passthrough`, `automatic-rename`, `monitor-activity`, `monitor-bell`, `monitor-silence`, `default-terminal`, `default-shell`, `default-command`, `display-panes-time`, the style options (`status-style`, `message-style`, `message-command-style`, `pane-border-style`, `pane-active-border-style`, `window-status-style`, `window-status-current-style`, `window-status-activity-style`, `window-status-bell-style`, `window-status-last-style`, each a tmux style string with its colours and attributes), `pane-border-lines` (single, double, rounded, simple as ascii), `bind`/`bind -n`/`bind -T root`/`unbind` for commands mpx accepts (including `if-shell` and `run-shell`, which bind like any command), and `set -s command-alias name=command` as the `[alias]` table.
- Skipped or approximated lines are listed on stderr as `# line N: ...` and again in the output as `# unsupported (line N): ...` comments. Examples: `run-shell` and `if-shell` at the top level of the file (they run at tmux startup, which an mpx config cannot express; use `mpx run-shell` or bind them), TPM plugins (`run '~/.tmux/plugins/tpm/tpm'` is never executed), `source-file` (only the given file is converted), `%if` blocks (skipped; use `source-file` for a live, evaluated config), `escape-time` (no mpx setting), `set -p` and `set -s` (mpx has no pane options, and no server options other than `command-alias`), `set -w synchronize-panes` (a window option set at run time, not read from the file), `bind` of commands mpx does not have, and `split-window -c '#{pane_current_path}'` (dropped, since mpx already starts new panes in the active pane's directory).
- Also converted: `mouse`, `mode-keys`, and the terminal options `focus-events`, `extended-keys`, `xterm-keys`, `set-titles`, `set-titles-string`, `bell-action`, `visual-bell` and `visual-activity`.
- `unbind KEY` is written as an empty binding, which removes mpx's default for that key.
- `~` and `$HOME` are copied as written.

---

## Panes and Processes

### `mpx run`
```bash
mpx run [--name N] [--pty] [--cwd DIR] [--env K=V]... [--remain] <COMMAND> [ARGS...]
```

Options go before the command. Everything after `<COMMAND>` belongs to the command.

- `--pty`: interactive PTY (ConPTY on Windows) instead of a plain pipe.
- `--remain`: keep the pane on screen after the process exits (`remain_on_exit`).

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

### Capture and paste buffers

Paste buffers belong to the session's daemon, so the CLI, the TUI and copy mode share them. Unnamed buffers are named `buffer0000`, `buffer0001`, ... as tmux names them. `-s` is the session flag here, so a paste separator is `--separator`.

| Command | Description |
| :--- | :--- |
| `mpx capture-pane [-t pane] [-p] [-e] [-J] [-S line] [-E line] [-b buffer]` (`capturep`) | Capture the pane's history and screen. Without `-p` the text goes into a paste buffer (a new one, or `-b`). |
| `mpx set-buffer [-a] [-b name] [-n new] [data]` (`setb`) | Store `data` in a buffer, replacing it, or appending with `-a` (to the newest one unless `-b`). `-n` renames the buffer (`-b`, or the newest) to `new`. |
| `mpx show-buffer [-b name]` (`showb`) | Print a buffer's text, with no newline added. |
| `mpx delete-buffer [-b name]` (`deleteb`) | Delete a buffer (the newest unless `-b`). |
| `mpx load-buffer [-b name] <path \| ->` (`loadb`) | Store a file, or stdin with `-`, in a buffer. |
| `mpx save-buffer [-a] [-b name] <path \| ->` (`saveb`) | Write a buffer to a file (`-a` appends), or to stdout with `-`. |
| `mpx paste-buffer [-d] [-p] [-r] [--separator sep] [-b name] [-t pane]` (`pasteb`) | Paste a buffer into a pane. `-d` deletes the buffer afterwards, `-p` brackets the paste, `-r` keeps line feeds, `--separator` replaces each line feed with `sep`. |
| `mpx list-buffers [-F format]` (`lsb`) | List the buffers, newest first: `buffer_name: N bytes: "sample"`, or each line formatted by `-F`. |

`capture-pane` numbers lines as tmux does: `0` is the first visible line, negative numbers are lines of the history above it, `-S -` is the start of the history and `-E -` the last visible line. `-e` keeps the color and attribute escapes. `-J` joins wrapped lines. Known difference: trailing blanks of a line that did not wrap are trimmed even with `-J`, since the screen does not record which blanks were typed. `-a`, `-N`, `-C` and `-P` are not implemented and fail as unknown options.

### Session environment

| Command | Description |
| :--- | :--- |
| `mpx set-environment [-g] [-u] [-r] [-t session] NAME [VALUE]` (`setenv`) | Set a variable for the panes started afterwards. `-u` removes it from the session environment. `-r` keeps it out of new panes even when mpx inherited it. |
| `mpx show-environment [-g] [-t session] [NAME]` (`showenv`) | Print `NAME=value` lines, and `-NAME` for a variable kept out. With `NAME`, only that variable (an error if it is unset). |

mpx keeps one environment per session, so `-g` names the same one. `-t` picks the session as elsewhere. The environment is saved with the session snapshot and restored before the windows start. At the `:` prompt, `set-environment` and `show-environment` take no `-t`.

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
| `mpx split [-t pane] [-h \| -v] [-l PERCENT] [-d] [-Z] [--pty] [--env K=V] CMD...` | Split a pane (`-h` side by side, `-v` stacked; `-d` keeps focus; `-Z` keeps a zoomed window zoomed). |
| `mpx new-window [-n name] [-c dir] [-d] [--pty] [--env K=V] [CMD...]` | New window (defaults to user's shell). |
| `mpx kill-window [-t index]` / `mpx rename-window [-t index] <name>` | Close / rename a window. |
| `mpx select-window -t <index>` / `mpx last-window` / `mpx last-pane [-Z]` | Navigation (`-Z` keeps a zoomed window zoomed). |
| `mpx focus <pane>` | Focus a pane (switches window if needed). |
| `mpx select-pane [-t <pane>] [-Z] [-m] [-M]` | Focus a pane (`-Z` keeps a zoomed window zoomed), mark it (`-m`), or clear the mark (`-M`). |
| `mpx swap-pane [--source <pane>] [-t <pane>] [-Z]` | Swap two panes (`-Z` keeps a zoomed window zoomed). |
| `mpx break-pane <pane>` | Move a pane into its own window. |
| `mpx join-pane [-h \| -v] [-d] [--source <pane>] [-t <pane>]` (`move-pane`) | Move a pane next to another, splitting it: `-h` side by side, `-v` stacked (the default). `--source` defaults to the marked pane, `-t` to the active pane; `-d` keeps the active pane focused. A window left empty goes away. |
| `mpx respawn-pane [-t <pane>] [-k] [-c dir] [CMD...]` | Restart a pane's process in place. `-k` kills a running one first; otherwise only an exited pane restarts. |
| `mpx respawn-window [-t index] [-k] [-c dir] [CMD...]` | Restart every pane of a window, in place. Refused while a pane still runs unless `-k`. |
| `mpx rotate-window [-D \| -U] [-Z]` | Rotate the active window's panes through its layout slots: each moves to the slot before its own (`-U`, the default) or after it (`-D`). `-Z` keeps a zoomed window zoomed. |
| `mpx swap-window [--source <index>] -t <index>` | Exchange two windows by index (`--source` defaults to the active window). |
| `mpx move-window [--source <index>] -t <index>` | Move a window to another index (`--source` defaults to the active window). |
| `mpx link-window`, `mpx unlink-window`, `mpx resize-window` | Not supported, and refused with the reason. A pane belongs to one window, so a window cannot be linked elsewhere (`unlink-window` has nothing to undo). A window has no size of its own: the attached terminal sets it. |
| `mpx select-layout [preset]` | `even-horizontal`, `even-vertical`, `main-vertical`, `main-vertical-mirrored`, `main-horizontal`, `main-horizontal-mirrored`, `tiled`, `-E`. |
| `mpx resize-pane [-t pane] -L\|-R\|-U\|-D [cells]` | Move split border (or `-x`/`-y` for absolute size). |
| `mpx resize-pane [-t pane] -Z` | Zoom the pane's window so its focused pane fills it, or unzoom it. Zoom is the daemon's, so every attached terminal shows it. |
| `mpx layout [--width W --height H]` | Print the computed pane geometry. |

`-s` is the session flag, so the commands that take a source pane or window spell it `--source` (as `swap-pane` always has). Window indices are the daemon's, starting at 0, as `select-window` uses them.

---

## Shell, Channels and Hooks

| Command | Description |
| :--- | :--- |
| `mpx run-shell [-b] [-t pane] [-d SECONDS] COMMAND...` | Run a command through the session's shell (`default-shell`, else `$SHELL`) in the daemon. `#{...}` formats are expanded for `-t` (the active pane by default), and the command starts in that pane's directory. Its output is printed; `-b` does not wait, `-d` waits first. The exit status does not change the command's own. |
| `mpx if-shell [-F] [-t pane] CONDITION COMMAND [COMMAND]` | Run COMMAND when the shell command CONDITION exits 0, else the second COMMAND (tmux `if-shell`). With `-F`, CONDITION is a format, true when non-empty and not `0`. The daemon decides the branch and runs it through its hook executor, so the branch can be any command a hook can run; a `run-shell` branch prints nothing. `-t` is the pane the condition and the commands act on (the active pane by default). |
| `mpx wait-for [-S \| -L \| -U] CHANNEL` | Named channels of the daemon (tmux `wait-for`). With no flag, waits until `-S` signals the channel; a signal with nobody waiting is kept for the next wait. `-L` takes the channel's lock (waiting while another holder has it), `-U` releases it (an error when it is not locked). The command blocks in the terminal and exits with status 1 when the daemon stops. |
| `mpx set-hook [-a] HOOK COMMAND...` | Set the command a hook runs; `-a` adds one after the hook's existing commands. |
| `mpx set-hook -u HOOK` | Remove the hook's commands. |
| `mpx set-hook -R HOOK` | Run the hook's commands now, for the active pane. |
| `mpx show-hooks` | Print every hook command as `HOOK COMMAND`, by hook name and then in order. |

Hooks run in the daemon when their event happens, with the event's pane as the target of formats (`#{hook}` is the hook name, `#{hook_pane}` the pane's id). Hooks are kept in memory: they last as long as the session's daemon does, so set them again after `mpx kill-server`. Events:

- `after-new-window`, `after-split-window`, `after-kill-pane`: after the window or pane is made or killed by a client request.
- `after-select-window`, `after-select-pane`: after a request that changes the selected window or pane.
- `pane-exited`: a pane's process ended. `pane-died`: the same, when the pane stays on screen (`remain-on-exit`).
- `alert-bell`, `alert-activity`: a window that was not shown rings or shows output for the first time since the user last saw it (`monitor-bell`, `monitor-activity`).
- `client-attached`, `client-detached`: a terminal attaches to or leaves the session.
- `alert-silence`: a background window has been quiet for `monitor-silence` seconds (once, until it produces output again).
- `window-linked`, `window-unlinked`: a window is made, or removed, by a client's request, or when its last pane exits. A hook's own requests fire neither.
- `after-new-session`: the session's daemon starts with a fresh session, not one restored from its snapshot. Hooks live in the daemon's memory and are set after it starts, so this runs only for hooks that are already set when it starts.
- `session-closed`: `mpx kill-server` (or a terminal's kill) is starting the shutdown. The hooks run as the daemon shuts down, so one that takes longer may not finish.

Not supported: window and pane hooks (`set-hook -w`, `-t`; a hook applies to the whole session), which fail with an error. A command a hook runs does not fire hooks itself.

Commands a hook can run: `run-shell`, `if-shell`, `wait-for`, `set-hook` (including `-R`), `set -s command-alias`, `rename-session`, `split-window`, `new-window`, `kill-pane`, `kill-window`, `rename-window`, `select-window` (by index or name), `next-window`, `previous-window`, `last-window`, `move-window`, `swap-window`, `move-window -r`, `respawn-window`, `select-pane`, `next-pane`, `last-pane`, `swap-pane` (`-U`/`-D`, `-s`/`-t`), `select-pane -m`/`-M`, `join-pane`, `break-pane`, `respawn-pane`, `select-layout`, `rotate-window`, `resize-pane -Z`, `send-keys`, `paste-buffer`, `capture-pane`, `set-buffer`, `load-buffer`, `save-buffer`, `delete-buffer`, `set-environment`, `show-environment`, `set-option synchronize-panes`, `show-buffer` and `list-buffers` (their output is not shown in a hook, as with `run-shell`). A `paste` with no text pastes the newest paste buffer. Commands that need the terminal (copy mode, prompts, overlays, popups, menus, client commands, `bind-key`, `display-message`, `set-option` of client options, `select-pane` and `resize-pane` by direction, `reload-config`, `source-file`, `choose-buffer`) fail with `command not available in hooks`.

`if-shell [-bF] [-t pane] SHELL-COMMAND COMMAND [COMMAND]` runs the shell command and then the first command when it exits 0, else the second. With `-F` the first argument is a format, true when non-empty and not `0`. It is a command of the `:` prompt, key bindings, config files and `source-file`, and of hooks; `mpx if-shell` runs it from a shell (see above).

`source-file [-q] FILE...` (in the `:` prompt, bindings, or a config file read this way) runs each file's command lines in order. Lines ending in `\` continue on the next line; `#` starts a comment line. `%if`, `%elif`, `%else` and `%endif` select lines by a format condition, and `%hidden NAME=value` defines a variable that `#{NAME}` in later lines and conditions expands to. A missing file is an error unless `-q` is given.

`set -s command-alias NAME=COMMAND` (`set -su command-alias NAME` removes it) makes `NAME` a command for the `:` prompt, bindings, hooks and files; the index in `command-alias[N]` is accepted and ignored, as aliases are looked up by name. The same aliases are read from the `[alias]` table of the config.

In the TUI, a foreground `run-shell`, `if-shell` or `wait-for` does not block the screen: its output goes to the status row (one message per line, all in `show-messages`), and the commands after it on the same line run once it finishes. `-b` runs without that wait.

## Session Restore

The daemon saves an atomic snapshot of the session's layout whenever it changes: windows, splits, focus, window names, the session environment, and each pane's command, working directory, and environment.

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
  - Neovim: `nvim -c 'silent! source Session.vim'` (starts empty when there is no `Session.vim`)

---

---

## tmux Compatibility

mpx does not install or impersonate a `tmux` binary. Its CLI uses tmux's command names where they overlap (`new-session`, `list-sessions`, `attach`, `send-keys`, `kill-server`, `has-session`, ...), key bindings follow tmux, and `mpx import-tmux` converts a tmux config. To type `tmux` for those commands, `alias tmux=mpx`; anything mpx does not have is an error. Scripts and tools that call `tmux` for other commands, or that read `$TMUX`, do not work with mpx.

### Formats

Formats (`status_left` / `status_right`, `display-message`, `command-prompt -F`) use the tmux grammar:

- `#{name}` expands a variable; unknown names expand to empty. `#S #I #P #W #T #D` are aliases for the session, window, pane, window name, pane title and pane id. `##` is a literal `#`.
- Conditionals `#{?cond,then,else}`, with chains `#{?c1,v1,c2,v2,else}`. A condition is a variable or a nested format, true when non-empty and not `0`.
- Comparisons `#{==:a,b}`, `!=`, `<`, `>`, `<=`, `>=` (numeric when both sides are integers, otherwise string order), and `#{&&:a,b}`, `#{||:a,b}`, `#{!:a}`. They yield `1` or `0`. Arguments are literal text or nested formats.
- Modifiers: `#{=N:var}` keeps the first N characters, `#{=-N:var}` the last N. `#{s/pat/rep/:var}` replaces every regex match with a literal replacement (flag `i` ignores case; neither pattern nor replacement may contain `/`). `#{b:var}` basename, `#{d:var}` dirname, `#{l:text}` literal text, `#{E:var}` expands the value again as a format, `#{n:var}` length in characters, `#{q:var}` shell-escapes it. Unknown modifiers expand to empty.
- Inside `#{...}`, `#,` is a comma and `#}` a closing brace. An unclosed `#{` is printed as text.

Variables:

- Pane: `pane_id`, `pane_index`, `pane_pid`, `pane_title`, `pane_current_command`, `pane_current_path`, `pane_width`, `pane_height`, `pane_active`, `pane_dead`, `pane_marked`, `pane_marked_set`, `pane_in_mode` and `pane_mode`, `pane_tty`, `pane_start_command`, `pane_synchronized` (`1` when the window synchronizes its panes). `pane_left`, `pane_top`, `pane_right` and `pane_bottom` come from the current layout and are empty for a pane that is not laid out. `pane_at_left`, `pane_at_top`, `pane_at_right` and `pane_at_bottom` are `1` when the pane touches that edge of its window. `cursor_x` and `cursor_y` are the terminal cursor's column and row, and `history_size` the scrollback lines its captures reach; all three are empty for a pane without a terminal.
- Window: `window_index`, `window_id`, `window_name`, `window_active`, `window_panes`, `window_flags` (`#` activity, `!` bell, `~` silent, `*` active, `Z` zoomed, `-` last, `M` holds the marked pane), `window_zoomed_flag` (`1` when the window is zoomed), and `window_activity_flag`, `window_bell_flag`, `window_silence_flag`, `window_last_flag`, `window_marked_flag` (each `1` or `0`).
- Session: `session_name`, `session_windows`, `session_attached` (the number of clients attached to the session), `client_session`, and the times `session_id`, `session_created` and `start_time` described below.
- Host: `host`, `host_short`, `pid`, `version` (`3.4`).
- Paste buffers, in `list-buffers -F`: `buffer_name`, `buffer_size` (bytes), `buffer_sample` (the first 50 characters, control characters escaped).
- Session times: `session_id` is `$` and the daemon's start in milliseconds, the same for as long as it runs. `session_created` is the unix time the session was made (its restore time when it was restored from its snapshot, else its daemon's start), and `start_time` the unix time the daemon started.
- Operators: `#{t:var}` prints a Unix time as local time (`Thu Nov  2 10:00:00 2026`); `#{t/p:var}` shows how long ago it was: `HH:MM` within a day, `Wed14:05` within a week, `Wed02Nov` beyond; `#{m:pattern,string}` is `1` when a glob matches (`#{m/r:...}` a regex, `i` in the operator ignores case); `#{e|+:a,b}` does integer arithmetic with `+ - * / % m M` (`/` and `%` by zero give `0`), and `#{e|+|f|N:a,b}` floats with `N` decimals.
- Loops: `#{W:body}` expands `body` once per window of the session, `#{P:body}` once per pane of the current window, and `#{S:body}` once for the session. Inside the body the variables are those of that window, pane or session.

### Copy mode

`copy-mode [-eHMqu] [-t pane]` enters copy mode, as the `Prefix [` key does. `-e` leaves once scrolling returns to the live screen, `-u` also pages up into history, `-q` leaves copy mode. `-H` and `-M` are accepted with no effect. Copy mode belongs to a pane: a pane that is not focused keeps its copy state, as in the TUI.

`send-keys [-t pane] -X [-N n] command [args]`, typed at the `:` prompt or bound in a key table, runs one copy-mode command in the copy mode of the pane, `n` times (default 1). The pane must be in copy mode, else the command fails with `pane %N not in a mode`. A name mpx does not implement fails with `copy-mode command not supported: NAME`; a name that needs an argument without one fails with `send-keys -X NAME: needs an argument` (`needs a character` for the `jump-*` commands). `goto-line` needs a number (`invalid line 'x'` otherwise). `-N` without `-X` fails with `send-keys: -N needs -X`. The name and arguments are checked before the pane's mode, so a bad name fails the same way in any pane.

The commands mpx runs:

| Group | Commands |
| :--- | :--- |
| Cursor | `cursor-left`, `cursor-right`, `cursor-up`, `cursor-down`, `cursor-down-and-cancel` (leaves when the cursor is at the bottom of the screen), `start-of-line`, `end-of-line`, `back-to-indentation`, `top-line`, `middle-line`, `bottom-line`, `goto-line n` |
| Words and columns | `next-word`, `next-word-end`, `previous-word`, `next-space`, `next-space-end`, `previous-space`, `forward-four-columns`, `back-four-columns` |
| Jumps | `jump-forward c`, `jump-backward c`, `jump-to-forward c`, `jump-to-backward c` |
| Scrolling | `history-top`, `history-bottom`, `page-up`, `page-down`, `halfpage-up`, `halfpage-down`, `halfpage-down-and-cancel`, `scroll-up`, `scroll-down` |
| Search | `search-forward text`, `search-backward text`, `search-again`, `search-reverse`, `search-prompt-forward`, `search-prompt-backward` (open the `/` or `?` prompt) |
| Selection | `begin-selection`, `select-line`, `select-word`, `clear-selection`, `other-end`, `rectangle-toggle` (the selection covers the rectangle between its corners; toggling again returns to a run of text; without a selection it starts one at the cursor) |
| Rectangles | `rectangle-on`, `rectangle-off` (set the rectangle selection on or off, where `rectangle-toggle` flips it; turning it off returns to a run of text with the same anchor) |
| Marks and prompts | `set-mark` (marks the cursor's line), `jump-to-mark` (the cursor goes to the mark and the mark to the line the cursor left, so a second jump returns; nothing happens without a mark), `next-prompt [-o]`, `previous-prompt [-o]` (move to the next or previous shell prompt, which the shell marks with OSC 133;A; `-o` moves to the start of command output marked with OSC 133;C; nothing happens when there is none) |
| Copying | `copy-selection`, `copy-selection-no-clear`, `copy-selection-and-cancel`, `copy-selection-or-line-and-cancel`, `copy-selection-if-any`, `copy-line`, `copy-line-and-cancel`, `copy-end-of-line`, `copy-end-of-line-and-cancel` |
| Piping | `copy-pipe command`, `copy-pipe-no-clear`, `copy-pipe-and-cancel`, `copy-pipe-line`, `copy-pipe-line-and-cancel`, `copy-pipe-end-of-line`, `copy-pipe-end-of-line-and-cancel`. The text goes to the shell command's standard input and its output is discarded. |
| Leaving | `cancel` |

Copied text goes to the session's paste buffers (`show-buffer`) and, unless `set-clipboard off`, to the system clipboard. Each copied row loses its trailing blanks, as in tmux.

Key tables: `copy-mode-vi` is the table for `mode-keys vi` and `copy-mode` the table for `mode-keys emacs` (see [KEYBINDINGS.md](KEYBINDINGS.md#vi-copy-mode-prefix-)). `bind-key -T copy-mode-vi KEY send-keys -X command [args]` binds a key in the table and `unbind-key -T copy-mode-vi KEY` removes it. A binding takes effect in every attached terminal at once. Other commands in a copy-mode table are refused with `copy-mode-vi: only send-keys -X commands can be bound`.

Names mpx does not implement, such as `stop-selection`, `selection-mode`, and the line-number and bracket-matching commands, fail with `copy-mode command not supported`.

Zoom follows tmux: focusing another pane (`select-pane`, `last-pane`, `next-pane`), swapping panes (`swap-pane`), splitting, rotating, and switching windows unzoom the window. The forms that take `-Z` keep a zoomed window zoomed: `select-pane -Z`, `last-pane -Z`, `swap-pane -Z`, `split -Z`, and `rotate-window -Z`.

Searches wrap around the ends of the history by default. `set -g wrap-search off` (config `wrap_search = false`) stops a search at its first or last match instead.
