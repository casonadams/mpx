# Configuration

mpx reads TOML from the first of:

1. `$MPX_CONFIG`
2. `$XDG_CONFIG_HOME/mpx/config.toml`
3. `%APPDATA%\mpx\config.toml` (Windows)
4. `~/.config/mpx/config.toml`

A missing file means defaults. Unknown keys are rejected, so typos surface as errors. The daemon reads the file at start-up (restart a session with `mpx kill-server` to apply daemon settings); the TUI reads it when attaching.

`-f FILE` / `--config FILE` (before the command) replaces this list for that invocation; a missing file there means defaults, like the default path.

Coming from tmux? `mpx import-tmux ~/.tmux.conf` prints the equivalent TOML (see [Importing tmux config](CLI.md#importing-tmux-config)); it never writes the file.

```toml
prefix = "C-b"          # tmux notation: C-a, C-Space, M-b ...
remain_on_exit = false  # panes stay on screen after exit: false, true, or "failed" (non-zero exit only)
history_limit = 64      # exited, removed panes whose logs stay readable
scrollback_lines = 100000  # lines of scrollback per pane for captures (tmux history-limit)
allow_passthrough = true # passthrough from active pane: true, false, or "all" (every pane shown)
restore = true          # when a session starts, recreate its last layout and resume processes
which_key_delay_ms = 300  # wait after the prefix key before showing the shortcut popup (0 = at once)
display_panes_time_ms = 1000  # how long display-panes numbers stay up (0 = until a key)
automatic_rename = true  # name windows after their foreground process
monitor_activity = true  # flag background windows that print output
monitor_bell = true      # flag background windows that ring the bell
default_terminal = "xterm-256color"  # TERM exported to panes
# default_shell = "/bin/zsh"  # program for panes started without a command (default: $SHELL)
# default_command = "htop"    # command run by panes started without one (default: none)

[style]
border_active = "bright-green"      # active border color or hex ("#3fb950")
border_inactive = "bright-black"    # inactive border color or hex ("#263345")
border_marked = "bright-magenta"    # marked border color or hex ("#bc8cff")
border_style = "single"             # "single", "rounded", "double", "ascii"
status_bg = "dark-green"            # status line background or hex ("#121820")
status_fg = "black"                 # status line foreground or hex ("#c5d1de")
dim_inactive = true                 # desaturate inactive panes when multiple are visible
dim_factor = 0.5                    # how far toward grey (0.0 = untouched, 1.0 = grey)

# Optional template for active (focused) pane
[style.active]
fg = ""                             # default text color (empty for terminal default)
bg = ""                             # default background color

# Optional template for inactive (unfocused) panes
[style.inactive]
fg = ""                             # optional: replaces the default text color in inactive panes
bg = ""                             # optional: replaces the default background in inactive panes

[bind]                  # prefix key -> command line (see KEYBINDINGS.md)
"M-h" = "select-pane -L"
"$" = "command-prompt -F -I \"#S\" \"rename-session -- '%%'\""

[bind_root]             # keys without the prefix; "" unbinds
"F12" = "palette"
"r" = "command-prompt -I 'rename-window '"

# Process resumption rules for crash & reboot recovery
[[resume]]
name = "claude"
detect = ["claude"]
resume = ["claude", "--resume", "{id}"]
fallback = ["claude", "--continue"]
env_id = "CLAUDE_SESSION_ID"

[[resume]]
name = "omp"
detect = ["omp"]
resume = ["omp", "--resume", "{id}"]
fallback = ["omp", "--continue"]
env_id = "OMP_SESSION_ID"

[[resume]]
name = "nvim"
detect = ["nvim"]
fallback = ["nvim", "-S", "Session.vim"]
```

## Configuration Reference

### Core Settings

| Key | Type | Default | Scope | Description |
|---|---|---|---|---|
| `prefix` | string | `"C-b"` | TUI | Prefix key for all multiplexer shortcuts (e.g. `"C-a"`, `"C-Space"`). Press twice to send literal key to active pane. |
| `remain_on_exit` | `true`, `false`, or `"failed"` | `false` | Daemon | Keep a pane on screen after its process exits until it is closed. `"failed"` keeps only panes that exited with a non-zero status (or by a signal). |
| `history_limit` | integer | `64` | Daemon | Number of exited and removed panes whose output buffers remain readable via CLI. |
| `allow_passthrough` | `true`, `false`, or `"all"` | `true` | TUI | `true` lets terminal passthrough sequences (DCS tmux passthrough, OSC 4/10/11/12/52) from the active pane reach the host terminal. `"all"` lets them from every pane the terminal shows; `false` blocks them. |
| `restore` | bool | `true` | Daemon | On startup, automatically restore the last saved session layout and resume registered processes, however the previous run ended. `mpx --no-restore` skips it for one start. |
| `which_key_delay_ms` | integer | `300` | TUI | Milliseconds to wait after pressing the prefix before displaying the which-key HUD popup (`0` displays immediately). |
| `display_panes_time_ms` | integer | `1000` | TUI | Milliseconds the `display-panes` numbers stay on screen before they go away (`0` keeps them until a key is pressed). |
| `scrollback_lines` | integer | `100000` | Daemon | Lines of scrollback each pane keeps for captures (`capture-pane`, `logs --plain`), for panes started after the setting. Unlike `history_limit`, which counts exited panes. |
| `automatic_rename` | bool | `true` | Daemon | Name each window after its foreground process. `false` keeps the name it was created with. |
| `monitor_activity` | bool | `true` | Daemon | Flag background windows that print output (`#` in the window list). Flags are tracked while a terminal is attached. |
| `monitor_bell` | bool | `true` | Daemon | Flag background windows whose panes ring the bell (`!`); same rule as `monitor_activity`. |
| `default_terminal` | string | `"xterm-256color"` | Daemon | `TERM` exported to panes started after the setting. |
| `default_shell` | string (absolute path) | unset, so `$SHELL` | Daemon | Program that a pane started without a command runs. |
| `default_command` | string | unset | Daemon | Command line run through the default shell by a pane started without a command. |
| `base_index` | integer | `0` | TUI | Number of the first window (tmux `base-index`). Tabs, `Prefix` digits, `select-window -t` and `#{window_index}` count from it. The daemon numbers windows from 0 internally, and the native `mpx select-window` still takes that internal index. |
| `pane_base_index` | integer | `0` | TUI | Number of the first pane of a window as `#{pane_index}` shows it (tmux `pane-base-index`). |
| `mouse` | bool | `true` | TUI | `false` turns the terminal's mouse reporting off and ignores mouse events. |
| `status` | bool | `true` | TUI | Show the status line. `false` gives its row to the panes; messages and prompts then use the bottom row. |
| `status_position` | `"top"` or `"bottom"` | `"bottom"` | TUI | Row the status line occupies. |
| `status_interval` | integer (seconds) | `1` | TUI | Seconds between timed status redraws, which keep the clock current. `0` redraws only on events. |
| `status_left`, `status_right` | string (tmux format) | unset | TUI | Replace the left side (`[session]`) or the right side (copy badge, zoom flag and clock). Formats expand as in tmux; `#[fg=red,bold]`-style directives and strftime (`%H:%M %d-%b-%y`) apply. Unset keeps mpx's built-in side. |
| `status_left_length`, `status_right_length` | integer | `10`, `40` | TUI | Widest a custom `status_left` / `status_right` may be; longer text is cut. The built-in sides are never cut. |
| `window_status_format`, `window_status_current_format`, `window_status_separator` | string | unset | TUI | Format of each window tab, of the current window's tab, and the text between tabs (a single space when unset). Unset keeps mpx's built-in tabs. |
| `status_justify` | `"left"`, `"centre"` or `"right"` | `"left"` | TUI | Where the window list sits between the two sides. |
| `display_time_ms` | integer | `3000` | TUI | Milliseconds a message stays on the status row (tmux `display-time`). |
| `mode_keys` | `"vi"` or `"emacs"` | `"vi"` | TUI | Copy-mode key table (tmux `mode-keys`); see [KEYBINDINGS.md](KEYBINDINGS.md#emacs-copy-mode-mode-keys-emacs). |
| `set_clipboard` | bool | `true` | TUI | `false` keeps copy-mode yanks off the system clipboard. They still go to the paste buffers. |

### tmux option names

These tmux options have an mpx file key; `mpx import-tmux` maps them. Each row names the file key. Where mpx differs from tmux, the behavior column says so.

| tmux option | mpx behavior | Default |
|---|---|---|
| `remain-on-exit on\|off\|failed` | `remain_on_exit`. `failed` keeps only panes that exited non-zero (a signal counts). Decided when a pane exits, so panes already running follow a change. | `off` |
| `history-limit N` | `scrollback_lines`: scrollback lines per pane for captures, for panes started after the change. The TUI's copy mode keeps mpx's built-in 100000-line limit. The file key `history_limit` is a different setting (exited panes) and is not changed. | `100000` (tmux: `2000`) |
| `allow-passthrough on\|off\|all` | `allow_passthrough`. `all` covers every pane the terminal shows (the attached window); background windows are not streamed to the terminal, so their passthrough is not forwarded. | `on` (tmux: `off`) |
| `automatic-rename on\|off` | `automatic_rename`. | `on` |
| `monitor-activity on\|off` | `monitor_activity`. Background windows get the `#` flag on output, with or without an attached terminal. | `on` |
| `monitor-bell on\|off` | `monitor_bell`. Background windows get the `!` flag on a bell. | `on` |
| `monitor-silence N` | Not supported: `0` (off) is accepted, any other value is an error. | `0` |
| `default-terminal` | `default_terminal`: `TERM` for panes started after the change. | `xterm-256color` |
| `default-shell` | `default_shell`: program for panes started with no command. Must be an absolute path. | `$SHELL` |
| `default-command` | `default_command`: run through the default shell by panes started with no command. Empty means the shell. | empty |
| `display-panes-time` | `display_panes_time_ms`. | `1000` |

Panes started by `split-window`, `new-window` or `new-session` with no command use the default program. Options that are session-wide apply to all windows.

### Terminal option names

These tmux options are applied by the terminal UI.

| tmux option | mpx behavior | Default |
|---|---|---|
| `base-index N` | `base_index`. Digit keys, `select-window -t`, `kill-window -t`, `move-window -t` and `#{window_index}` use it. | `0` |
| `pane-base-index N` | `pane_base_index` for `#{pane_index}`. The display-panes numbers are not offset. | `0` |
| `renumber-windows on` | Accepted: mpx always renumbers windows without gaps. `off` is refused with "renumber-windows off is not supported: windows are always renumbered". | `on` (tmux: `off`) |
| `mouse on\|off` | `mouse`. | `on` |
| `status on\|off` | `status`. Multi-line counts (`2` to `5`) are refused. | `on` |
| `status-position top\|bottom` | `status_position`. | `bottom` |
| `status-interval N` | `status_interval`. | `1` |
| `status-left`, `status-right` | `status_left`, `status_right`. | `[#{session_name}] `, and mpx's clock |
| `status-left-length`, `status-right-length` | `status_left_length`, `status_right_length`. | `10`, `40` |
| `window-status-format`, `window-status-current-format`, `window-status-separator` | `window_status_format`, `window_status_current_format`, `window_status_separator`. Tab flags follow tmux's order (`#`, `!`, `*`, `Z`, `-`, `M`). | `#I:#W#{window_flags}`, same, ` ` |
| `status-justify left\|centre\|right` | `status_justify`. | `left` |
| `status-style`, `status-bg`, `status-fg` | `[style] status_bg` / `status_fg`. Only `fg=` and `bg=` are accepted; attributes and `default` are refused. | `bg=green,fg=black` (mpx: `dark-green`, `black`) |
| `pane-border-style fg=...`, `pane-active-border-style fg=...` | `[style] border_inactive` / `border_active`. A `bg=` item is refused. | mpx's border colours |
| `message-style` | Refused: status messages use the status colours. | n/a |
| `mode-keys vi\|emacs` | `mode_keys`. | `vi` (tmux picks emacs unless `$VISUAL`/`$EDITOR` mentions vi) |
| `display-time N` | `display_time_ms`. | `3000` (tmux: `750`) |
| `display-panes-time N` | `display_panes_time_ms`. | `1000` |
| `set-clipboard on\|external\|off` | `set_clipboard`. `off` keeps copy-mode yanks off the system clipboard. Applications' OSC 52 requests are still governed by `allow-passthrough`. | `external` |
| `escape-time N` | Stored and reported only: mpx reads Escape without a delay. | `500` (no effect) |
| `focus-events`, `aggressive-resize`, `extended-keys`, `xterm-keys`, `status-keys` | Stored and reported only; they have no effect in mpx. | tmux defaults (no effect) |


### Inactive panes (`[style]`)

With several panes visible, every pane but the focused one is desaturated: each color moves toward its own grey by `dim_factor` (`0.0` leaves it alone, `1.0` is fully grey), and bold is dropped. Set `dim_inactive = false` to turn it off. There is no other dimming mode.

- Cells with an explicit color (the ANSI palette, 256-color, truecolor; syntax highlighting, diffs, selections) are desaturated, text and background alike.
- Cells in the terminal's default colors are desaturated too. At attach, mpx asks the terminal for its default foreground and background (`OSC 10` / `OSC 11`). A terminal that does not answer leaves those cells untouched. Colors are read once per attach; attach again after changing the terminal theme.
- `[style.active] fg` / `bg` replace the terminal's colors as the default to desaturate. `[style.inactive] fg` / `bg` replace the result: an inactive pane then shows exactly that color for default-colored cells.
- Removed: `color_mode`, `factor` and `dim_content_bg` (under `[style]` or `[style.inactive]`). The config now fails to load with an unknown-key error if they are present; use `dim_factor` instead.

### Process Resumption Rules (`[[resume]]`)

Rules that dictate how tools (AI CLIs, editors, background tasks) are resumed when a workspace is restored across crashes or reboots:

| Field | Type | Description |
|---|---|---|
| `name` | string | Identifier for this resumption rule. |
| `detect` | string list | Program names or foreground commands that match this rule (case-insensitive). |
| `resume` | string list | Command line used when a session ID is known. `{id}` is replaced with the active session ID. |
| `fallback` | string list | Command line used when no session ID is present (e.g. `["claude", "--continue"]`). |
| `env_id` | string (optional) | Name of an environment variable, read from the daemon's environment, that carries the session ID. Used when `id_source` finds nothing. |
| `id_source` | `"claude"` or `"omp"` (optional) | Finds the session ID of the tool running in the pane from its own state files: `claude` reads `~/.claude/sessions/<pid>.json` for the pane's foreground process, `omp` reads `~/.omp/agent/terminal-sessions/<tty>`. Each pane resumes its own session, even with several in one directory. |

`mpx` ships with built-in defaults for `claude`, `omp`, `codex`, `rho`, `antigravity`, `opencode`, and `nvim`. `claude` and `omp` use `id_source`. A `[[resume]]` list in `config.toml` replaces the defaults, so repeat every rule you want to keep.

## Reloading the config

`Prefix r` (or `:reload-config`) re-reads the config file while mpx runs: the file given with `-f`, else `$MPX_CONFIG`, else the default location. The TUI picks up style, `prefix`, `bind` and `which_key_delay_ms` at once. The session's daemon picks up the settings it owns for panes created afterwards and for the next snapshot and restore: `[[resume]]` rules, `restore`, `remain_on_exit`, `history_limit`, `scrollback_lines`, `automatic_rename`, `monitor_activity`, `monitor_bell`, `default_terminal`, `default_shell` and `default_command`. Running panes keep their environment.

A file that does not parse, or a `bind`/`prefix` that does not resolve, is rejected with a message on the status row and nothing changes. A daemon started by an older mpx does not know the request; the UI still reloads and the message says to restart the session.
