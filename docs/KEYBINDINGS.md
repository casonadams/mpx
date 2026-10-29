# Keyboard Shortcuts & TUI Navigation

Bindings follow tmux muscle memory. Where tmux is silent or ambiguous, they follow vim (`h/j/k/l`, `gg`/`G`, `/`, `:` commands, `q`/`Esc` to close).

---

## Prefix

The prefix defaults to **`Ctrl-b`**. Change it with `prefix = "C-a"` in the [config file](CONFIG.md). Press the prefix, release, then an action key. Pressing the prefix twice sends it to the program in the pane (for example, `Ctrl-b Ctrl-b` reaches Claude Code's "run in background").

If you pause after the prefix, a shortcut popup lists the common action keys. It appears after `which_key_delay_ms` (300 ms by default, `0` shows it at once), so fast sequences never flash it.

Only the configured prefix is intercepted. `Ctrl-a` reaches your shell unless you make it the prefix.

### Windows

| Key | Command | Description |
| :--- | :--- | :--- |
| `c` | `new-window` | New window in the current pane's directory |
| `n` / `p` | `next-window` / `previous-window` | Cycle windows |
| `0`..`9` | `select-window` | Jump to a window |
| `^` | `last-window` | Previous window (vim's alternate buffer) |
| `,` | `rename-window` | Rename (opens the prompt prefilled) |
| `&` | `kill-window` | Close the window (asks y/n) |

### Panes

| Key | Command | Description |
| :--- | :--- | :--- |
| `%` / `\|` | `split-window -h` | Split side by side |
| `"` / `-` | `split-window` | Split stacked |
| `h` `j` `k` `l` / arrows | `select-pane -L/-D/-U/-R` | Move focus |
| `o` / `Tab` | `next-pane` | Cycle panes |
| `;` | `last-pane` | Previously focused pane |
| `H` `J` `K` `L` | `resize-pane -L/-D/-U/-R` | Resize by 5 cells; repeat without the prefix for 500 ms |
| `z` | `resize-pane -Z` | Zoom toggle |
| `x` | `kill-pane` | Kill the pane (asks y/n) |
| `!` | `break-pane` | Move the pane to its own window |
| `{` / `}` | `swap-pane -U/-D` | Swap with the previous/next pane |
| `Space` | `next-layout` | Cycle even-horizontal, even-vertical, main-vertical, main-horizontal, tiled |
| `=` | `select-layout -E` | Spread the panes evenly, keeping their arrangement |

### Sessions and agents

| Key | Command | Description |
| :--- | :--- | :--- |
| `s` | `agents` | [Agent manager](AGENT-MANAGER.md) |
| `w` | `choose-tree` | Sessions: session → window → pane tree with preview |
| `e` | `sidebar` | Focus the [agents rail](#agents-rail), expanding it; again collapses it |
| `(` / `)` | `prev-session` / `next-session` | Switch sessions |
| `A` | `next-attention` | Jump to the agent that has waited longest for input, in any session |
| `d` / `q` | `detach-client` | Detach (daemon and panes keep running) |

### Other

| Key | Command | Description |
| :--- | :--- | :--- |
| `Ctrl-Space` (no prefix) | `palette` | Open the unified command palette |
| `:` | `command-prompt` | Command line on the status row |
| `?` | `list-keys` | Searchable key list |
| `[` | `copy-mode` | Vi copy mode |

---

## Command prompt (`Prefix :`)

A vim-style command line on the status row. Editing keys: `Left`/`Right`, `Backspace`, `Ctrl-u` (clear), `Ctrl-w` (delete word), `Up`/`Down` (history), `Tab` (complete the command name), `Enter`, `Esc`. Arguments support quotes and backslash escapes. Errors show on the status row.

| Command | Aliases | Notes |
| :--- | :--- | :--- |
| `split-window [-h] [-d] [-c dir] [cmd...]` | `split`, `sp` (stacked), `vsplit`, `vs` (side by side) | |
| `new-window [-n name] [-c dir] [cmd...]` | `new`, `tabnew` | |
| `next-window` / `previous-window` | `tabnext`, `tabn` / `prev-window`, `tabprevious`, `tabp` | |
| `select-window -t N`, `last-window`, `rename-window NAME`, `kill-window [-t N]` | | |
| `select-pane -t TARGET \| -L/-R/-U/-D`, `next-pane`, `last-pane`, `kill-pane [-t TARGET]` | | `TARGET` is `name`, `%id` or `session:pane` |
| `resize-pane -L/-R/-U/-D [N]`, `resize-pane -Z` | | `N` defaults to 5 |
| `swap-pane -U/-D`, `break-pane`, `select-layout [preset]`, `select-layout -E`, `next-layout` | | |
| `switch-client -t SESSION` | `session` | |
| `next-session`, `prev-session` | | |
| `send-keys [-t TARGET] KEYS...` | | tmux key names |
| `paste [-t TARGET] [--submit] TEXT...` | | |
| `launch-agent HARNESS [--resume ID] [--cwd DIR] [--split]` | | |
| `agents`, `next-attention`, `choose-tree`, `sidebar`, `next-agent`, `prev-agent`, `list-keys` | `help` | |
| `palette [commands\|agents\|sessions\|help]` | `command-palette` | |
| `copy-mode`, `send-prefix`, `command-prompt` | | |
| `detach-client` | `detach`, `q`, `quit` | |
| `kill-server` | `qa` | Asks y/n |

Custom bindings map a key to any of these command lines; see [CONFIG.md](CONFIG.md).

---

## Overlays

All overlays use the same motions: `j`/`k` (or arrows), `gg`/`G`, `Ctrl-d`/`Ctrl-u`, `/` filter (fuzzy; `Enter` keeps the filter, `Esc` clears it), and `q`/`Esc` to close.

- **Command Palette** (`Ctrl-Space` or `:palette`): unified tabbed modal with instant fuzzy search across **Commands**, **Agents**, **Sessions & Panes**, and **Help**. Navigate rows with `Ctrl-n`/`Ctrl-p` or `Up`/`Down`, switch tabs with `Tab`/`Shift-Tab` or `Left`/`Right`, type to filter, `Ctrl-w`/`Ctrl-u` to delete word/clear, `Enter` to execute or switch, and `Esc` to close.
- **Agent manager** (`Prefix s`): see [AGENT-MANAGER.md](AGENT-MANAGER.md).
- **Sessions** (`Prefix w`): every session, window and pane, with the current session first. Pane rows show the running program, working directory and time since last output; session and window rows count waiting (`◆`), working (`●`) and exited (`✕`) panes, even when collapsed. `l`/`Right`/`Enter` expands or selects, `h`/`Left` collapses, `/` filters by name, program or directory, `x` kills the selected pane, window or session (asks y/n), `p` toggles a preview of the selected pane. Selecting switches session and window as needed.
- **Help** (`Prefix ?`): every active binding and its command.

---

## Agents rail

While any agent runs in any session, a thin rail on the left shows one dot per agent: `◆` waiting (yellow), `●` working (green), `○` idle, `✕` exited. Sessions are separated by a blank row, the attached one first, and the agent in the focused pane is drawn in reverse video. Without agents there is no rail, and screens narrower than 42 columns leave it out.

Focused, the rail expands into a list over the panes, which keep their size: each agent's title and, below it, its latest message and the time since its state changed.

| Key | Action |
| :--- | :--- |
| `Alt-j` / `Alt-k` (no prefix) | Show the next / previous agent's pane, in any window or session; the reverse-video dot follows. With no agents running the keys go to the program |
| `Prefix e` | Focus the rail, expanding it; again collapses it |

While the rail has focus:

| Key | Action |
| :--- | :--- |
| `j`/`k`, `gg`/`G`, `Ctrl-d`/`Ctrl-u` | Move the highlight; the agent's pane shows at once (another session's after a 150 ms pause) |
| `Enter` / `l` / `Right` / `Esc` / `q` | Hand the keys to the shown pane and collapse the rail |

`Prefix h` from the leftmost pane also focuses the rail, and clicking a dot shows that agent. `Alt-j`/`Alt-k` never reach the program in the pane; rebind or unbind them with [`[bind_root]`](CONFIG.md#key-bindings-bind).

On macOS, terminals type `∆` and `˚` for Option-j and Option-k unless Option is set to act as Alt, so mpx binds those two characters too and the keys work either way (unbind them with `[bind_root] "∆" = ""` to type them). Setting Option to act as Alt also makes every other Alt shortcut work, in mpx and in the programs it runs:

| Terminal | Setting |
| :--- | :--- |
| iTerm2 | Settings → Profiles → Keys → Left Option key: **Esc+** |
| Terminal.app | Settings → Profiles → Keyboard → **Use Option as Meta key** |
| Ghostty | `macos-option-as-alt = true` |
| WezTerm | `send_composed_key_when_left_alt_is_pressed = false` |
| Alacritty | `[window] option_as_alt = "Both"` |
| kitty | `macos_option_as_alt yes` |

---

## Input passthrough

- Keys are encoded like xterm, including modifiers: `Shift-Tab` (`CSI Z`), `Alt-<key>` (ESC prefix), `Ctrl`/`Alt`/`Shift` arrows and function keys, `F1`-`F12`. `Shift-Enter` and `Alt-Enter` send `ESC CR`, which Claude Code and readline treat as a newline.
- Arrow keys follow the program's cursor-key mode (DECCKM).
- Pastes use bracketed paste when the program enabled it, so multi-line prompts are not submitted line by line.
- Focus in/out events reach programs that request them (`?1004`), both for terminal focus and for pane focus changes.

---

## Status bar

`[session] 0:claude* 1:cargo` lists windows, which are named after the agent or foreground process of their focused pane until you rename them. `◆N` counts agents in any session waiting for input. Exited panes kept by `remain_on_exit` show an `[exited N]` badge. When an agent in a pane you can't see starts waiting, mpx sends a desktop notification (OSC 9) unless `agent.notify = false`.

---

## Mouse

- **Click** focuses a pane and nothing else. **Drag** selects text (entering copy mode from where the press started), and releasing copies it (`pbcopy` on macOS, OSC 52 elsewhere).
- **Drag a border** between panes to resize them; the pane on the side you drag toward grows.
- **Wheel up** enters copy mode and scrolls history; **wheel down** scrolls back toward the prompt and, when the wheel entered copy mode, leaves it on reaching the prompt (tmux's `copy-mode -e`). Copy mode entered with `Prefix [` stays until `q` or `Esc`.

---

## Copy mode (`Prefix [`)

| Key | Action |
| :--- | :--- |
| `h` `j` `k` `l` / arrows | Move |
| `w` / `b` | Word forward / backward |
| `0` / `^` / `$` | Line start / end |
| `Ctrl-u` / `Ctrl-d`, `PageUp` / `PageDown` | Half page |
| `g` / `G` | Top / bottom of history |
| `?` / `/` | Search backward (up) / forward (down) |
| `n` / `N` | Repeat search next / previous |
| `v` / `V` | Character / line selection |
| `y` / `Enter` | Yank to clipboard and exit |
| `Esc` | Clear selection, or exit |
| `q` | Exit |
