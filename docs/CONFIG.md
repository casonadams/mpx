# Configuration

mpx reads TOML from the first of:

1. `$MPX_CONFIG`
2. `$XDG_CONFIG_HOME/mpx/config.toml`
3. `%APPDATA%\mpx\config.toml` (Windows)
4. `~/.config/mpx/config.toml`

A missing file means defaults. Unknown keys are rejected, so typos surface as errors. The daemon reads the file at start-up (restart a session with `mpx kill-server` to apply daemon settings); the TUI reads it when attaching.

`-f FILE` / `--config FILE` (before the command) replaces this list for that invocation; a missing file there means defaults, like the default path.

```toml
prefix = "C-b"          # tmux notation: C-a, C-Space, M-b ...
remain_on_exit = false  # keep panes on screen after their process exits
history_limit = 64      # exited, removed panes whose logs stay readable
tmux_compat = true      # export TMUX/TMUX_PANE + the tmux shim in every pane (default: true)
allow_passthrough = true # allow terminal passthrough sequences (DCS tmux, OSC 4/10/11/12/52) from active pane
restore = true          # after a crash or reboot, recreate the last layout and resume processes
which_key_delay_ms = 300  # wait after the prefix key before showing the shortcut popup (0 = at once)

[style]
border_active = "bright-green"      # active border color or hex ("#3fb950")
border_inactive = "bright-black"    # inactive border color or hex ("#263345")
border_marked = "bright-magenta"    # marked border color or hex ("#bc8cff")
border_style = "single"             # "single", "rounded", "double", "ascii"
status_bg = "dark-green"            # status line background or hex ("#121820")
status_fg = "black"                 # status line foreground or hex ("#c5d1de")
dim_inactive = true                 # dim inactive panes when multiple are visible
dim_factor = 0.20                   # dim factor (0.20 = 20% reduction)
dim_content_bg = true               # dim explicit app backgrounds (diffs, selections)

# Optional template for active (focused) pane
[style.active]
fg = ""                             # default text color (empty for terminal default)
bg = ""                             # default background color

# Optional template for inactive (unfocused) panes
[style.inactive]
fg = "#4c566a"                      # default text color in inactive panes (e.g. muted slate/grey)
bg = "#242933"                      # optional surface background tint for inactive panes
color_mode = "desaturate"           # syntax color mode: "dim", "desaturate", "tint", or "none"
factor = 0.25                       # color transformation intensity (0.0 to 1.0)
dim_content_bg = true               # transform explicit app background colors in inactive panes

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
| `restore` | bool | `true` | Daemon | On startup, automatically restore the last saved session layout and resume registered processes. |
| `which_key_delay_ms` | integer | `300` | TUI | Milliseconds to wait after pressing the prefix before displaying the which-key HUD popup (`0` displays immediately). |

### Process Resumption Rules (`[[resume]]`)

Rules that dictate how tools (AI CLIs, editors, background tasks) are resumed when a workspace is restored across crashes or reboots:

| Field | Type | Description |
|---|---|---|
| `name` | string | Identifier for this resumption rule. |
| `detect` | string list | Program names or foreground commands that match this rule (case-insensitive). |
| `resume` | string list | Command line used when a session ID is known. `{id}` is replaced with the active session ID. |
| `fallback` | string list | Command line used when no session ID is present (e.g. `["claude", "--continue"]`). |
| `env_id` | string (optional) | Name of environment variable to extract the session ID from. |

`mpx` ships with built-in defaults for `claude`, `omp`, `codex`, `rho`, `antigravity`, `opencode`, and `nvim`.
