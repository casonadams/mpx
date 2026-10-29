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
tmux_compat = false     # export TMUX/TMUX_PANE + the tmux shim in every pane
restore = true          # after a crash or reboot, recreate the last layout and resume agents
persist_pipes = true    # keep pipe messages and consumer cursors across daemon restarts
which_key_delay_ms = 300  # wait after the prefix key before showing the shortcut popup (0 = at once)

[agent]
idle_ms = 1500          # quiet period before a hook-less agent counts as idle
notify = true           # OSC 9 desktop notification when an off-screen agent needs input
mcp = true              # give agents mpx launches the mpx MCP tools (mpx_peers, mpx_agent_prompt, ...)

[style]
border_active = "bright-green"      # active border color or hex ("#3fb950")
border_inactive = "bright-black"    # inactive border color or hex ("#263345")
border_style = "single"             # "single", "rounded", "double", "ascii"
status_bg = "dark-green"            # status line background or hex ("#121820")
status_fg = "black"                 # status line foreground or hex ("#c5d1de")
dim_inactive = true                 # dim inactive panes when multiple are visible
dim_factor = 0.20                   # dim factor (0.20 = 20% reduction)
dim_content_bg = true               # dim explicit app backgrounds (diffs, selections)

[bind]                  # prefix key -> command line (see KEYBINDINGS.md)
"M-h" = "select-pane -L"
"g" = "launch-agent claude --split"

[bind_root]             # keys without the prefix; "" unbinds
"F12" = "palette"
"r" = "command-prompt -I 'rename-window '"

[[harness]]             # extra or overriding agent harnesses (see AGENT-MANAGER.md)
name = "aider"
detect = ["aider"]
command = ["aider"]
resume = []
session_id_args = []
```

## Configuration Reference

### Core Settings

| Key | Type | Default | Scope | Description |
|---|---|---|---|---|
| `prefix` | string | `"C-b"` | TUI | Prefix key for all multiplexer shortcuts (e.g. `"C-a"`, `"C-Space"`). Press twice to send literal key to active pane. |
| `remain_on_exit` | bool | `false` | Daemon | If `true`, pane stays open on screen after its child process exits until explicitly closed. |
| `history_limit` | integer | `64` | Daemon | Number of exited and removed panes whose output buffers remain readable via CLI/MCP. |
| `tmux_compat` | bool | `false` | Daemon | Exports `TMUX` and `TMUX_PANE` environment variables and injects the `mpx` tmux compatibility shim. |
| `restore` | bool | `true` | Daemon | On startup, automatically restore the last saved session layout and resume registered agent harnesses. |
| `persist_pipes` | bool | `true` | Daemon | Store virtual pipe message queues and consumer cursors on disk across daemon restarts. |
| `which_key_delay_ms` | integer | `300` | TUI | Milliseconds to wait after pressing the prefix before displaying the which-key HUD popup (`0` displays immediately). |

### Styling & Theming (`[style]`)

| Key | Type | Default | Scope | Description |
|---|---|---|---|---|
| `border_active` | string | `"bright-green"` | TUI | Active pane border color. Supports standard ANSI names (`"bright-green"`, `"cyan"`) or hex (`"#3fb950"`). |
| `border_inactive` | string | `"bright-black"` | TUI | Inactive pane border color. Supports ANSI names or hex (`"#263345"`). |
| `border_style` | string | `"single"` | TUI | Border style: `"single"` (─│┌┐), `"rounded"` (─│╭╮), `"double"` (═║╔╗), or `"ascii"` (-\|+). |
| `status_bg` | string | `"dark-green"` | TUI | Status line background color name or hex (`"#121820"`). |
| `status_fg` | string | `"black"` | TUI | Status line text foreground color name or hex (`"#c5d1de"`). |
| `dim_inactive` | bool | `true` | TUI | When multiple panes are visible, subtly dim inactive panes to emphasize focus: default text turns grey, bright colors take their normal shade, normal colors are drawn faint (SGR 2, so your theme's shade is kept), and RGB and 256-color text is scaled by `dim_factor`. |
| `dim_factor` | float | `0.20` | TUI | Dimming reduction factor between `0.0` and `1.0` (`0.20` = dim by 20%, keeping text legible). |
| `dim_content_bg` | bool | `true` | TUI | When dimming inactive panes, also dim explicit application background colors (diffs, selections, statuslines) while preserving transparent terminal canvas. |

### Agent Settings (`[agent]`)

| Key | Type | Default | Scope | Description |
|---|---|---|---|---|
| `idle_ms` | integer | `1500` | Daemon | Milliseconds of I/O inactivity before a hook-less agent is classified as `idle`. |
| `notify` | bool | `true` | TUI | Emit an OSC 9 desktop notification when an off-screen background agent transitions to `needs_input`. |
| `mcp` | bool | `true` | Launcher | Start Claude Code and oh-my-pi agents with the mpx MCP server (`--mcp-config`, or an omp plugin via `--plugin-dir`), so they can see and message the other agents. Read by whatever launches the agent: the TUI, the CLI, the MCP server or a daemon restore. |

### Key Bindings (`[bind]`)

Map custom prefix key shortcuts to `mpx` command lines. Format is `"<key>" = "<command>"`.

```toml
[bind]
"M-h" = "select-pane -L"
"g"   = "launch-agent claude --split"
"r"   = "command-prompt -I 'rename-window '"
```

`[bind_root]` binds keys pressed without the prefix (tmux `bind -n`); they never reach the program in the pane. By default `C-Space` opens the unified Command Palette (`palette`). Terminal Alt/Option combinations pass through unmodified to programs in the pane. In either table an empty command line unbinds a key:

```toml
[bind_root]
"M-n" = "next-agent"
```

Key names follow tmux notation:
- Modifiers: `C-` (Ctrl), `M-` (Alt / Option), `S-` (Shift)
- Special keys: `Enter`, `Escape`, `Tab`, `BTab` (Backtab / Shift-Tab), `Space`, `BSpace`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp` (`PPage`), `PageDown` (`NPage`), `IC` (Insert), `DC` (Delete), `F1`–`F12`
- Characters: single case-sensitive character (e.g., `h`, `H` for Shift-h)

#### Bindable Commands Reference

All commands available in `[bind]` can also be run interactively from the command prompt (`Prefix + :`):

| Command | Aliases | Arguments & Flags | Description |
|---|---|---|---|
| `split-window` | `splitw`, `split`, `vsplit` | `[-h|-v] [-d] [-c <dir>] [-- <cmd...>]` | Split active pane vertically (`-h` / columns) or horizontally (`-v` / rows). `-d` leaves focus on current pane. |
| `select-pane` | `selectp` | `[-L|-R|-U|-D] [-t <pane>]` | Directional focus movement (Left, Right, Up, Down) or jump to target pane ID/name. |
| `resize-pane` | `resizep` | `[-L|-R|-U|-D] [-x <w>] [-y <h>] [-Z]` | Resize active pane boundary in direction or set absolute size. `-Z` toggles zoom. |
| `swap-pane` | `swapp` | `[-U|-D] [-s <pane>] [-t <pane>]` | Swap active pane with previous (`-U`) or next (`-D`) neighbor. |
| `break-pane` | `breakp` | `[-d]` | Break active pane out into its own window tab. |
| `kill-pane` | `killp`, `close`, `q` | `[-t <pane>]` | Kill pane process and remove split (confirms if active). |
| `zoom` | `resize-pane -Z` | none | Toggle 100% full-screen zoom on the active pane. |
| `new-window` | `neww`, `tabnew`, `tabe` | `[-n <name>] [-c <dir>] [-- <cmd...>]` | Create a new window tab running default shell or command. |
| `kill-window` | `killw`, `tabclose`, `tabc` | `[-t <index>]` | Close the window tab and terminate all its child panes. |
| `rename-window` | `renamew` | `<new-name>` | Rename the active window tab. |
| `select-window` | `selectw` | `[-t <index>]` | Jump to window tab index (`0..9`). |
| `next-window` | `tabnext`, `tabn` | none | Switch to next window tab. |
| `previous-window` | `prev-window`, `tabp` | none | Switch to previous window tab. |
| `last-window` | none | none | Toggle back to the previously active window tab. |
| `select-layout` | `selectl`, `next-layout` | `[preset]`, `-E` | Cycle split layout presets or pick layout preset; `-E` spreads the panes evenly, keeping their arrangement. |
| `launch-agent` | none | `[--split] [--teams] [--resume <id>] <harness>` | Launch an AI coding agent with hooks configured. |
| `agents` | none | none | Open full-screen Agent Manager overlay with status and resume list. |
| `next-attention` | none | none | Focus the background agent that has waited longest for user input. |
| `choose-tree` | `tree` | none | Open interactive session / window / pane hierarchy tree. |
| `palette` | `command-palette` | `[commands\|agents\|sessions\|help]` | Open unified tabbed command palette with instant fuzzy search. |
| `sidebar` | none | none | Open or close the vertical agents drawer overlay. |
| `next-agent`, `prev-agent` | none | none | Show the next / previous agent's pane without focusing the rail. |
| `list-keys` | `help` | none | Open searchable keybindings cheat sheet overlay. |
| `copy-mode` | none | none | Enter Vi visual scrollback navigation and copy mode. |
| `command-prompt` | none | `[-I <initial>] [-p <prompt>]` | Open command line prompt at bottom row. |
| `detach-client` | `detach`, `quit` | none | Detach terminal client cleanly, leaving daemon and panes running. |
| `switch-client` | `session`, `switchc` | `[-p|-n] [-t <session>]` | Switch attached client to another mpx session. |
| `send-prefix` | none | none | Send the literal prefix key through to the active pane process. |
| `send-keys` | none | `[-t <pane>] <keys...>` | Send key names (e.g. `Enter`, `C-c`) or text into pane. |
| `paste` | `paste-buffer` | `[-s] [-p] [-t <pane>] <text>` | Paste text into pane (`-p` for bracketed paste, `-s` appends Enter). |
| `kill-server` | `qa` | none | Prompt to terminate the session daemon and all child processes. |

### Agent Harnesses (`[[harness]]`)

mpx includes built-in harnesses for popular autonomous agents: `claude`, `omp`, `codex`, `rho`, `antigravity`, and `opencode`.

You can define **any number of additional harnesses** by specifying multiple `[[harness]]` sections in `config.toml`. You can also override a built-in harness by using the same `name`.

```toml
# Define custom aider harness
[[harness]]
name = "aider"
detect = ["aider"]
command = ["aider"]
resume = []
session_id_args = []

# Define custom internal agent
[[harness]]
name = "myagent"
detect = ["myagent-bin"]
command = ["myagent", "--mode", "autonomous"]
resume = ["myagent", "--resume", "{id}"]
session_id_args = ["--session", "{id}"]
```

| Key | Type | Default | Description |
|---|---|---|---|
| `name` | string | *required* | Identifier used in `mpx launch-agent <name>` or MCP tools. |
| `detect` | array of strings | `[]` | Foreground process executable names that identify this agent. |
| `command` | array of strings | `[]` | Command line argv used to spawn a fresh agent session. |
| `resume` | array of strings | `[]` | Command line argv used to resume a session; `{id}` is replaced with the session ID. |
| `session_id_args` | array of strings | `[]` | Argv appended to `command` to pre-assign a session ID (`{id}` is replaced). |

## Applying & Reloading Configuration

Configuration changes are applied based on option scope:

1. **Client / TUI Settings** (`prefix`, `[style]`, `[bind]`, `which_key_delay_ms`, `agent.notify`):
   - Re-read every time the client attaches to a session.
   - To apply edits without killing running processes, detach with **`Prefix + d`** and re-attach with **`mpx`**.
2. **Daemon Settings** (`remain_on_exit`, `restore`, `persist_pipes`, `tmux_compat`, `history_limit`, `agent.idle_ms`, `[[harness]]`):
   - Read when the session daemon first boots.
   - To apply changes to a running server, stop the daemon with `mpx kill-server` and start a new session with `mpx` (or `mpx restore` to keep your window/pane layout).
## State Directory

Snapshots and durable pipes live outside the runtime directory (which is cleared on reboot), in the first of:

1. `$MPX_STATE_DIR`
2. `$XDG_STATE_HOME/mpx`
3. `%LOCALAPPDATA%\mpx\state` (Windows)
4. `~/.local/state/mpx`

Each session has its own directory there: `snapshot.json` (the latest layout snapshot) and `pipes/` (a message log and metadata file per pipe).

## Environment Variables

| Variable | Purpose |
|---|---|
| `MPX_CONFIG` | Path to custom `config.toml` file (highest precedence). |
| `MPX_STATE_DIR` | Directory where layout snapshots and virtual pipe data are stored. |
| `XDG_CONFIG_HOME` | Base directory for user configuration files (defaults to `~/.config`). |
| `XDG_STATE_HOME` | Base directory for user state files (defaults to `~/.local/state`). |

## CLI Overrides

- `-f <FILE>` / `--config <FILE>`: Override config file path for that command.
- `--state-dir <DIR>`: Override state directory for that command.
