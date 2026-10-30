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
remain_on_exit = false  # keep panes on screen after their process exits
history_limit = 64      # exited, removed panes whose logs stay readable
tmux_compat = true      # export TMUX/TMUX_PANE + the tmux shim in every pane (default: true)
allow_passthrough = true # allow terminal passthrough sequences (DCS tmux, OSC 4/10/11/12/52) from active pane
restore = true          # when a session starts, recreate its last layout and resume processes
which_key_delay_ms = 300  # wait after the prefix key before showing the shortcut popup (0 = at once)
display_panes_time_ms = 1000  # how long display-panes numbers stay up (0 = until a key)

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
"$" = "command-prompt -I 'rename-session '"

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
| `remain_on_exit` | bool | `false` | Daemon | If `true`, pane stays open on screen after its child process exits until explicitly closed. |
| `history_limit` | integer | `64` | Daemon | Number of exited and removed panes whose output buffers remain readable via CLI. |
| `tmux_compat` | bool | `true` | Daemon | Exports `TMUX` and `TMUX_PANE` environment variables and injects the `mpx` tmux compatibility shim. |
| `allow_passthrough` | bool | `true` | TUI | Allow terminal passthrough sequences (DCS tmux passthrough, OSC 4/10/11/12/52) from the active pane to forward to the host terminal. |
| `restore` | bool | `true` | Daemon | On startup, automatically restore the last saved session layout and resume registered processes, however the previous run ended. `mpx --no-restore` skips it for one start. |
| `which_key_delay_ms` | integer | `300` | TUI | Milliseconds to wait after pressing the prefix before displaying the which-key HUD popup (`0` displays immediately). |
| `display_panes_time_ms` | integer | `1000` | TUI | Milliseconds the `display-panes` numbers stay on screen before they go away (`0` keeps them until a key is pressed). Also set with `set-option display-panes-time`. |

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

`Prefix r` (or `:reload-config`) re-reads the config file while mpx runs: the file given with `-f`, else `$MPX_CONFIG`, else the default location. The TUI picks up style, `prefix`, `bind` and `which_key_delay_ms` at once. The session's daemon picks up the settings it owns for panes created afterwards and for the next snapshot and restore: `[[resume]]` rules, `restore`, `remain_on_exit`, `history_limit` and `tmux_compat`. Running panes keep their environment.

A file that does not parse, or a `bind`/`prefix` that does not resolve, is rejected with a message on the status row and nothing changes. A daemon started by an older mpx does not know the request; the UI still reloads and the message says to restart the session.
