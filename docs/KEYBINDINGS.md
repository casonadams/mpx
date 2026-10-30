# Keyboard Shortcuts & TUI Navigation

Bindings follow tmux muscle memory. Where tmux is silent or ambiguous, they follow vim (`h/j/k/l`, `gg`/`G`, `/`, `:` commands, `q`/`Esc` to close).

---

## Prefix

The prefix defaults to **`Ctrl-b`**. Change it with `prefix = "C-a"` in the [config file](CONFIG.md). Press the prefix, release, then an action key. Pressing the prefix twice sends it to the program in the pane.

If you pause after the prefix, a shortcut popup lists common action keys after `which_key_delay_ms` (300 ms by default, `0` shows it at once).

### Windows

| Key | Command | Description |
| :--- | :--- | :--- |
| `c` | `new-window` | New window in current pane's directory |
| `n` / `p` | `next-window` / `previous-window` | Cycle windows |
| `0`..`9` | `select-window` | Jump to a window by index |
| `^` | `last-window` | Previous window |
| `,` | `rename-window` | Rename window (opens prompt prefilled) |
| `&` | `kill-window` | Close window (asks y/n) |

### Panes

| Key | Command | Description |
| :--- | :--- | :--- |
| `%` / `\|` | `split-window -h` | Split side by side |
| `"` / `-` | `split-window` | Split stacked |
| `h` `j` `k` `l` / arrows | `select-pane -L/-D/-U/-R` | Move focus |
| `o` / `Tab` | `next-pane` | Cycle panes |
| `;` | `last-pane` | Previously focused pane |
| `H` `J` `K` `L` | `resize-pane -L/-D/-U/-R` | Resize by 5 cells (repeatable without prefix for 500ms) |
| `z` | `resize-pane -Z` | Toggle zoom |
| `x` | `kill-pane` | Kill pane (asks y/n) |
| `!` | `break-pane` | Move pane into its own window |
| `{` / `}` | `swap-pane -U/-D` | Swap with previous / next pane |
| `Space` | `next-layout` | Cycle layouts (even-horizontal, even-vertical, main-vertical, main-horizontal, tiled) |
| `=` | `select-layout -E` | Spread panes evenly |

### Sessions

| Key | Command | Description |
| :--- | :--- | :--- |
| `s` | `palette sessions` / `choose-tree` | Interactive session and pane picker |
| `w` | `palette sessions` / `choose-tree` | Interactive window and pane picker |
| `$` | `rename-session` | Rename current session |
| `(` / `)` | `prev-session` / `next-session` | Switch sessions |
| `d` / `q` | `detach-client` | Detach (daemon and panes keep running) |

### Command Palette & Navigation

| Key | Command | Description |
| :--- | :--- | :--- |
| `Ctrl-Space` (no prefix) | `palette` | Open the unified fuzzy Command Palette |
| `:` | `command-prompt` | Command line on the status row |
| `r` | `reload-config` | Re-read `config.toml` without restarting (see [Reloading](CONFIG.md#reloading-the-config)) |
| `?` | `palette help` | Searchable key bindings list |
| `[` | `copy-mode` | Vi copy mode with 100k scrollback |

---

## Command Prompt (`Prefix :`)

Vim-style command line on the status row. Supports tab-completion, history (`Up`/`Down`), quotes, and escapes.

| Command | Aliases | Description |
| :--- | :--- | :--- |
| `split-window [-h] [-d] [-c dir] [cmd...]` | `split`, `sp`, `vsplit`, `vs` | Split active pane |
| `new-window [-n name] [-c dir] [cmd...]` | `new`, `tabnew` | Create window |
| `rename-session NAME` | `rename` | Rename current session |
| `reload-config` | `source-file` | Re-read the config file into the TUI and the session's daemon |
| `rename-window NAME` | `renamew` | Rename active window |
| `kill-pane [-t TARGET]` | `killp` | Close pane |
| `kill-window [-t INDEX]` | `killw` | Close window |
| `kill-server` | `qa` | Terminate session daemon |
| `select-pane [-t TARGET] [-m\|-M] \| -L/-R/-U/-D` | `selectp` | Move pane focus |
| `resize-pane -L/-R/-U/-D [N]`, `resize-pane -Z` | `resizep` | Resize / zoom |
| `choose-tree` | | Interactive session/window tree |
| `palette [commands\|sessions\|help]` | `command-palette` | Fuzzy command palette |

---

## Vi Copy Mode (`Prefix [`)

| Key | Action |
| :--- | :--- |
| `h` `j` `k` `l` / arrows | Move cursor |
| `Ctrl-u` / `Ctrl-d` | Half page up / down |
| `g` / `G` | Top / bottom of scrollback |
| `0` / `$` | Start / end of line |
| `w` / `b` | Forward / backward word |
| `v` | Start character selection |
| `V` | Start line selection |
| `y` | Yank selection (syncs to OS clipboard via OSC 52) |
| `/` | Regex search forward |
| `n` / `N` | Next / previous search match |
| `q` / `Esc` | Exit copy mode |
