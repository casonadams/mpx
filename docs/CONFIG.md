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

[bind]                  # prefix key -> command line (see KEYBINDINGS.md)
"M-h" = "select-pane -L"
"g" = "launch-agent claude --split"
"r" = "command-prompt -I 'rename-window '"

[[harness]]             # extra or overriding agent harnesses (see AGENT-MANAGER.md)
name = "aider"
detect = ["aider"]
command = ["aider"]
resume = []
session_id_args = []
```

Key names use tmux notation: `C-` (Ctrl), `M-` (Alt), `S-` (Shift) prefixes, plus `Enter`, `Escape`, `Tab`, `BTab`, `Space`, `BSpace`, `Up`/`Down`/`Left`/`Right`, `Home`, `End`, `PageUp`/`PPage`, `PageDown`/`NPage`, `IC`, `DC`, `F1`-`F12`, or a single character (case-sensitive: `H` is Shift-h).

## State directory

Snapshots and durable pipes live outside the runtime directory (which is cleared on reboot), in the first of:

1. `$MPX_STATE_DIR`
2. `$XDG_STATE_HOME/mpx`
3. `%LOCALAPPDATA%\mpx\state` (Windows)
4. `~/.local/state/mpx`

Each session has its own directory there: `snapshot.json` (the latest layout snapshot) and `pipes/` (a message log and metadata file per pipe).

`--state-dir <DIR>` on any command replaces this per-session directory for that invocation.
