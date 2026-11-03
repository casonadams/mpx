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
dim_inactive = false                # true: dim inactive panes when multiple are visible
dim_factor = 0.5                    # how far toward black (0.0 = untouched, 1.0 = black)

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
fallback = ["nvim", "-c", "silent! source Session.vim"]
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
| `monitor_silence` | integer (seconds) | `0` | Daemon | Flag a background window `~` once it has printed nothing for this many seconds (tmux `monitor-silence`); `0` is off. Same rule as `monitor_activity`. |
| `default_terminal` | string | `"xterm-256color"` | Daemon | `TERM` exported to panes started after the setting. |
| `default_shell` | string (absolute path) | unset, so `$SHELL` | Daemon | Program that a pane started without a command runs. |
| `default_command` | string | unset | Daemon | Command line run through the default shell by a pane started without a command. |
| `window_size` | `"latest"`, `"smallest"`, `"largest"` or `"manual"` | `"latest"` | Daemon | Which attached clients the window is laid out for (tmux `window-size`). `latest`: the most recently active client (tmux's default). `smallest`: the smallest width and height among them, so every client sees all of it. `largest`: the largest. `manual`: `default_size`, whatever the clients. Clients that have reported no size are ignored; with no sized client the caller's size is used. Panes are sized to the window, and a client larger than the window shows blank space around it. Read at start and by `reload-config`. |
| `default_size` | `"WIDTHxHEIGHT"` | `"80x24"` | Daemon | Window size under `window_size = "manual"` (tmux `default-size`). |
| `detach_on_destroy` | bool | `true` | TUI | When the session a terminal is attached to ends: `true` detaches it (tmux `detach-on-destroy on`); `false` switches it to another running session with panes, and detaches only when there is none. |
| `base_index` | integer | `0` | TUI | Number of the first window (tmux `base-index`). Tabs, `Prefix` digits, `select-window -t` and `#{window_index}` count from it. The daemon numbers windows from 0 internally, and the native `mpx select-window` still takes that internal index. |
| `pane_base_index` | integer | `0` | TUI | Number of the first pane of a window as `#{pane_index}` shows it (tmux `pane-base-index`). |
| `mouse` | bool | `true` | TUI | `false` turns the terminal's mouse reporting off and ignores mouse events. |
| `status` | bool | `true` | TUI | Show the status line. `false` gives its row to the panes; messages and prompts then use the bottom row. |
| `status_lines` | integer `1`..`5` | `1` | TUI | Rows the status line takes (tmux `status 2`..`5`). Rows after the first show `status_format`. |
| `status_format` | list of strings (tmux format) | empty | TUI | `status_format[N]` is drawn on status row N. A non-empty entry 0 replaces the main status line (window list and `status_left`/`status_right` included). |
| `status_position` | `"top"` or `"bottom"` | `"bottom"` | TUI | Row the status line occupies. |
| `status_interval` | integer (seconds) | `1` | TUI | Seconds between timed status redraws, which keep the clock current. `0` redraws only on events. |
| `status_left`, `status_right` | string (tmux format) | `"[#{session_name}] "`, `"#{?pane_in_mode,[copy] ,}\"#{=21:pane_title}\" %H:%M %d-%b-%y"` | TUI | The left and right sides of the status line. Formats expand as in tmux; `#[fg=red,bold]`-style directives and strftime apply. The defaults are tmux's, plus a `[copy]` badge while the pane is in copy mode. |
| `status_left_length`, `status_right_length` | integer | `10`, `40` | TUI | Widest `status_left` / `status_right` may be; longer text is cut. |
| `window_status_format`, `window_status_current_format`, `window_status_separator` | string | `"#I:#W#{window_flags}"`, same, `" "` | TUI | Format of each window tab, of the current window's tab, and the text between tabs. |
| `status_justify` | `"left"`, `"centre"` or `"right"` | `"left"` | TUI | Where the window list sits between the two sides. |
| `display_time_ms` | integer | `3000` | TUI | Milliseconds a message stays on the status row (tmux `display-time`). |
| `mode_keys` | `"vi"` or `"emacs"` | `"vi"` | TUI | Copy-mode key table (tmux `mode-keys`); see [KEYBINDINGS.md](KEYBINDINGS.md#emacs-copy-mode-mode-keys-emacs). |
| `set_clipboard` | bool | `true` | TUI | `false` keeps copy-mode yanks off the system clipboard. They still go to the paste buffers. |
| `synchronize_panes` | bool | `false` | TUI | Windows created afterwards start with `synchronize-panes` on: typed input reaches every running pane of the window. `set-option -w synchronize-panes` changes a window. |
| `[style]` `status_style`, `message_style`, `message_command_style`, `pane_border_style`, `pane_active_border_style`, `window_status_style`, `window_status_current_style`, `window_status_activity_style`, `window_status_bell_style`, `window_status_last_style` | tmux style strings (see [tmux option names](#terminal-option-names)). | empty (no change) | TUI | A value that is not a valid style fails the file to load. |
| `wrap_search` | bool | `true` | TUI | `false` stops copy-mode searches at their first or last match instead of wrapping around the history (tmux `wrap-search`). |

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
| `monitor-silence N` | `monitor_silence`: seconds without output after which a background window gets the `~` flag. `0` is off. | `0` |
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
| `renumber-windows on\|off` | `renumber_windows`. `on` (default): closing a window renumbers the windows after it. `off`: windows keep their numbers and a new window takes the lowest free number; `move-window -r` renumbers them. | `on` (tmux: `off`) |
| `mouse on\|off` | `mouse`. | `on` |
| `status on\|off\|2`..`5` | `status`; a count shows that many status rows. `status-format[0]` replaces the main line; rows 1 and up draw `status-format[N]`, expanded with the focused pane's variables. | `on` (one row) |
| `status-position top\|bottom` | `status_position`. | `bottom` |
| `status-interval N` | `status_interval`. | `1` |
| `status-left`, `status-right` | `status_left`, `status_right`. | `[#{session_name}] `, and `"#{=21:pane_title}" %H:%M %d-%b-%y` (mpx adds `#{?pane_in_mode,[copy] ,}` in front) |
| `status-left-length`, `status-right-length` | `status_left_length`, `status_right_length`. | `10`, `40` |
| `window-status-format`, `window-status-current-format`, `window-status-separator` | `window_status_format`, `window_status_current_format`, `window_status_separator`. Tab flags follow tmux's order (`#`, `!`, `~`, `*`, `Z`, `-`, `M`). | `#I:#W#{window_flags}`, same, ` ` |
| `window-status-style`, `window-status-current-style`, `window-status-activity-style`, `window-status-bell-style`, `window-status-last-style` | `[style] window_status_style`, `window_status_current_style`, `window_status_activity_style`, `window_status_bell_style`, `window_status_last_style`. Each tab takes the styles that apply to it, laid in this order: all tabs, last, current, activity, bell. | none (no change) |
| `status-justify left\|centre\|right` | `status_justify`. | `left` |
| `status-style`, `status-bg`, `status-fg` | `[style] status_style`, laid over `status_bg` / `status_fg`. A tmux style string: `fg=`/`bg=` colours and the attributes `bold`, `dim`, `underscore`, `italics`, `reverse` (with `no` forms), `default` and `none`. Other attributes (`blink`, `hidden`, `strikethrough`, ...) are refused. | `bg=green,fg=black` (mpx: `dark-green`, `black`) |
| `pane-border-style`, `pane-active-border-style` | `[style] pane_border_style` / `pane_active_border_style`: a style over `border_inactive` / `border_active`. `fg=` sets the border colour and `bg=` the border's background. | mpx's border colours |
| `message-style`, `message-command-style` | `[style] message_style` / `message_command_style`: the status row's message, and the command prompt, confirmation and search line. | white on black |
| `synchronize-panes on\|off` | Window option: typed input goes to every running pane of the window. `set-option -g` sets every window. | `off` |
| `mode-keys vi\|emacs` | `mode_keys`. | `vi` (tmux picks emacs unless `$VISUAL`/`$EDITOR` mentions vi) |
| `display-time N` | `display_time_ms`. | `3000` (tmux: `750`) |
| `display-panes-time N` | `display_panes_time_ms`. | `1000` |
| `set-clipboard on\|external\|off` | `set_clipboard`. `off` keeps copy-mode yanks off the system clipboard. Applications' OSC 52 requests are still governed by `allow-passthrough`. | `external` |
| `wrap-search on\|off` | `wrap_search`. | `on` |
| `escape-time N` | Stored and reported only: mpx reads Escape without a delay. | `500` (no effect) |
| `focus-events on\|off` | `focus_events`. Focus in and out reach the panes that asked for focus reports (DEC 1004) only while this is on. | `off` |
| `extended-keys on\|always\|off` | `extended_keys`. Modified keys go to panes as CSI u or xterm modifyOtherKeys sequences: `on` when a pane asked for them (kitty `CSI > flags u` or `CSI > 4 ; n m`), `always` regardless (modifyOtherKeys level 2 unless the pane asked for kitty), `off` never. The kitty flags a pane sets shape its encoding: `1` disambiguates Ctrl and Alt keys, Enter, Tab, Backspace and Esc as CSI u; `2` adds the event-type field (`:2` repeat, `:3` release) to CSI keys; `4` adds the shifted key (`97:65`); `8` sends every key, text keys included, as CSI u; `16` adds the associated text as a third field (`97;;97u` for `a`, `97;2;65u` for `A`, as the kitty spec's text-as-code-points examples). Limits: mpx pushes only flag `1` to the outer terminal (`CSI > 1 u`, while this option is not `off`), so the outer terminal sends no event types, and the TUI drops release events before encoding, so no pane receives a release. Text keys are sent as text unless flag `8` is set. Sixel and kitty graphics are not supported. | `off` |
| `xterm-keys on\|off` | `xterm_keys`. With `on`, function and cursor keys keep their modifiers (`CSI 1;5A` for Ctrl-Up). `off` sends them without modifiers. | `on` |
| `set-titles on\|off` | `set_titles`. Sets the outer terminal's title from `set-titles-string` whenever it changes, and restores the title it had on exit. | `off` |
| `set-titles-string FORMAT` | `set_titles_string`: a format for the outer terminal's title, expanded for the active pane (`#S`, `#W`, `#{pane_title}`, ...). Control characters are dropped. | `#S:#I:#W - "#T"` (same as tmux) |
| `bell-action none\|any\|current\|other` | `bell_action`. Which windows' bells ring the outer terminal: `none` never, `any` all, `current` only the shown window, `other` only windows not shown. | `any` |
| `visual-bell on\|off` | `visual_bell`. A bell shows `Bell in window NAME` on the status row instead of ringing the outer terminal. | `off` |
| `visual-activity on\|off` | `visual_activity`. A window that gains output while not shown announces `Activity in window NAME` once on the status row. | `off` |
| `aggressive-resize`, `status-keys` | Stored and reported only; they have no effect in mpx. | tmux defaults (no effect) |

Hyperlinks (OSC 8), underline styles and colours (`4:n`, `58`), and synchronized output (DEC 2026, held for at most one second) are passed through to the outer terminal without options.


### Inactive panes (`[style]`)

Off by default. With `dim_inactive = true` and several panes visible, every pane but the focused one is dimmed, as tmux's `dim=` does: every color channel is scaled down by `dim_factor` (`0.0` leaves it alone, `1.0` is black, default `0.5`), and bold is dropped. Colors keep their hue; mpx no longer desaturates toward grey.

- Cells with an explicit color (the ANSI palette, 256-color, truecolor; syntax highlighting, diffs, selections) are dimmed, text and background alike.
- Cells in the terminal's default colors are dimmed too. With `dim_inactive = true`, mpx asks the terminal at attach for its default foreground and background (`OSC 10` / `OSC 11`) and for its 256 palette colors (`OSC 4`); with it off it asks for neither, which keeps attach quick. Turning it on with `reload-config` does not query the terminal, so attach again. A terminal that does not answer the first leaves default-colored cells untouched, and one that does not answer the second dims ANSI colors from a built-in palette, which may differ from your theme. Colors are read at attach and then follow the color sequences panes send to the terminal (`OSC 4`, `10`, `11` and their resets, as a theme switcher such as walh-shell emits), as long as `allow_passthrough` lets them through; a theme changed from the terminal's own menu is only seen at the next attach.
- `[style.active] fg` / `bg` replace the terminal's colors as the default to dim. `[style.inactive] fg` / `bg` replace the result: an inactive pane then shows exactly that color for default-colored cells.
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

A tool that was running inside a shell is resumed by typing its command into a fresh shell in the same directory, so quitting the tool leaves that shell, as it did before the restore. A pane whose own process is the tool (for example `mpx new-window claude`) is respawned as the tool, and quitting it closes the pane.

`mpx` ships with built-in defaults for `claude`, `omp`, `codex`, `rho`, `antigravity`, `opencode`, and `nvim`. `claude` and `omp` use `id_source`. A `[[resume]]` list in `config.toml` replaces the defaults, so repeat every rule you want to keep.

#### Neovim sessions

The `nvim` rule only replays a `Session.vim`; mpx never writes one. Create it once in a project root with `:mksession`, then keep it current from your `init.lua`. This snippet rewrites only a `Session.vim` that already exists, so no other directory gets one, and it saves on write, on focus loss and on exit so a crash or reboot loses at most your last edit:

```lua
vim.api.nvim_create_autocmd({ "VimLeavePre", "BufWritePost", "FocusLost" }, {
  group = vim.api.nvim_create_augroup("mpx_session", { clear = true }),
  callback = function()
    if vim.fn.filereadable("Session.vim") == 1 then
      vim.cmd("silent! mksession!")
    end
  end,
})
```

### Command aliases (`[alias]`)

A table of command names to the command line they stand for, as tmux's `command-alias` does. The first word of a command is replaced by the alias's words, and the words you type follow it:

```toml
[alias]
sp = "split-window -h"
```

With this, `sp -d` at the `:` prompt, in a key binding, in a hook, or in a file read by `source-file` runs `split-window -h -d`. An alias is expanded once. `set -s command-alias NAME=COMMAND` adds one at run time (`set -su command-alias NAME` removes it); the daemon's copy is what hooks use.

## Options at run time

`:set-option NAME VALUE` (alias `set`) changes an option of the terminal you type it in, with tmux's option names and values. Scope flags: `-w` and `-g` are the same for every option except `synchronize-panes` (a window option: `-w`, the default, changes the active window, `-g` every window); `-p` (pane options) is refused, since mpx has no pane options at run time. `-s` (server options) is refused too, except `-s command-alias`, which sets an alias (see [Command aliases](#command-aliases-alias)). `-u` restores the default, `-q` ignores an unknown name, and `--` ends the flags for a value that starts with `-`. `:show-options [-v] [NAME]` lists the values. A change lasts until `reload-config`, which re-reads the file, and applies at once; a value that does not parse, or a `prefix` that does not resolve, is refused with a message and changes nothing.

Settable: `allow-passthrough`, `base-index`, `bell-action`, `display-panes-time`, `display-time`, `extended-keys`, `focus-events`, `message-command-style`, `message-style`, `mode-keys`, `mouse`, `pane-active-border-style`, `pane-base-index`, `pane-border-style`, `prefix`, `set-clipboard`, `set-titles`, `set-titles-string`, `status`, `status-interval`, `status-justify`, `status-left`, `status-left-length`, `status-position`, `status-right`, `status-right-length`, `status-style`, `visual-activity`, `visual-bell`, `which-key-delay`, `window-status-activity-style`, `window-status-bell-style`, `window-status-current-format`, `window-status-current-style`, `window-status-format`, `window-status-last-style`, `window-status-separator`, `window-status-style`, `wrap-search`, `xterm-keys`. `synchronize-panes` is a window option (see below).

Options the daemon reads (`automatic-rename`, `default-command`, `default-shell`, `default-terminal`, `history-limit`, `monitor-activity`, `monitor-bell`, `monitor-silence`, `remain-on-exit`, `restore`) are not settable this way: set them in the file and run `reload-config`.

## Reloading the config

`Prefix r` (or `:reload-config`) re-reads the config file while mpx runs: the file given with `-f`, else `$MPX_CONFIG`, else the default location. The TUI picks up style, `prefix`, `bind` and `which_key_delay_ms` at once. The session's daemon picks up the settings it owns for panes created afterwards and for the next snapshot and restore: `[[resume]]` rules, `restore`, `remain_on_exit`, `history_limit`, `scrollback_lines`, `automatic_rename`, `monitor_activity`, `monitor_bell`, `monitor_silence`, `default_terminal`, `default_shell` and `default_command`. Running panes keep their environment.

A file that does not parse, or a `bind`/`prefix` that does not resolve, is rejected with a message on the status row and nothing changes. A daemon started by an older mpx does not know the request; the UI still reloads and the message says to restart the session.
