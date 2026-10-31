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
| `M-n` / `M-p` | `next-window -a` / `previous-window -a` | Next / previous window with activity or a bell |
| `'` | `command-prompt -T window-target` | Prompt for a window name or index (Tab completes names), then select it |
| `.` | `command-prompt` | Prompt for a target and move this window there (`move-window -t`) |
| `,` | `command-prompt -I "#W"` | Rename window (prompt prefilled with the current name) |
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
| `Ctrl-arrows` / `Alt-arrows` | `resize-pane` | Resize by 1 / 5 cells (tmux's keys) |
| `Ctrl-o` / `Alt-o` | `rotate-window` / `rotate-window -D` | Rotate pane positions |
| `q` | `display-panes` | Show pane numbers; press a digit to select that pane |
| `z` | `resize-pane -Z` | Toggle zoom |
| `x` | `kill-pane` | Kill pane (asks y/n) |
| `!` | `break-pane` | Move pane into its own window |
| `{` / `}` | `swap-pane -U/-D` | Swap with previous / next pane |
| `Space` | `next-layout` | Cycle layouts (even-horizontal, even-vertical, main-vertical, main-vertical-mirrored, main-horizontal, main-horizontal-mirrored, tiled) |
| `E` | `select-layout -E` | Spread panes evenly |
| `Alt-1`..`Alt-7` | `select-layout` | even-horizontal, even-vertical, main-horizontal, main-vertical, tiled, main-horizontal-mirrored, main-vertical-mirrored |

### Sessions

| Key | Command | Description |
| :--- | :--- | :--- |
| `s` | `palette sessions` / `choose-tree` | Interactive session and pane picker |
| `w` | `palette sessions` / `choose-tree` | Interactive window and pane picker |
| `$` | `command-prompt -I "#S"` | Rename current session (prompt prefilled with the current name) |
| `(` / `)` | `prev-session` / `next-session` | Switch sessions |
| `d` | `detach-client` | Detach (daemon and panes keep running) |
| `Ctrl-z` | `suspend-client` | Suspend mpx and return to the shell (`fg` resumes) |

### Command Palette & Navigation

| Key | Command | Description |
| :--- | :--- | :--- |
| `Ctrl-Space` (no prefix) | `palette` | Open the unified fuzzy Command Palette |
| `:` | `command-prompt` | Command line on the status row |
| `r` | `reload-config` | Re-read `config.toml` without restarting (see [Reloading](CONFIG.md#reloading-the-config)) |
| `?` | `palette help` | Searchable key bindings list |
| `[` | `copy-mode` | Vi copy mode with 100k scrollback |
| `PageUp` | `copy-mode -u` | Enter copy mode one page up |
| `]` | `paste` | Paste the last copy |
| `#` / `=` | `list-buffers` / `choose-buffer` | The session's paste buffers |
| `t` | `clock-mode` | Big clock in the active pane |
| `i` | `display-message` | Describe the current pane on the status row |
| `~` | `show-messages` | Log of status messages |

---

## Changing bindings while running

`:bind-key [-n] key command...` and `:unbind-key [-n] key` change the keymap of the running session at once, in every attached terminal; a terminal attaching later picks them up too. `-n` binds without the prefix. Chain commands with `\;` (`:bind r reload-config \; display-message reloaded`). Bindings last until the daemon stops; put lasting ones in `[bind]` / `[bind_root]` of `config.toml`. Only the commands in the table below are available; others fail with `unknown command`.

## Command Prompt (`Prefix :`)

Vim-style command line on the status row. Supports tab-completion, history (`Up`/`Down`), quotes, and escapes.

| Command | Aliases | Description |
| :--- | :--- | :--- |
| `split-window [-h] [-d] [-c dir] [cmd...]` | `split`, `sp`, `vsplit`, `vs` | Split active pane |
| `new-window [-n name] [-c dir] [cmd...]` | `new`, `tabnew` | Create window |
| `rename-session NAME` | `rename` | Rename current session |
| `reload-config` | `source-file` (without a file) | Re-read the config file into the TUI and the session's daemon |
| `rotate-window [-D]` | `rotatew` | Move each pane to the layout slot before its own (`-D`: after) |
| `move-window -t INDEX` | `movew` | Move the active window to INDEX |
| `display-message [-p] [FORMAT]` | `display` | Show the expanded [format](CLI.md#formats) on the status row; without one, describe the current pane |
| `rename-window NAME` | `renamew` | Rename active window |
| `kill-pane [-t TARGET]` | `killp` | Close pane |
| `kill-window [-t INDEX]` | `killw` | Close window |
| `kill-server` | `qa` | Terminate session daemon |
| `select-pane [-t TARGET] [-m\|-M] \| -L/-R/-U/-D` | `selectp` | Move pane focus |
| `resize-pane -L/-R/-U/-D [N]`, `resize-pane -Z` | `resizep` | Resize / zoom |
| `choose-tree` | | Interactive session/window tree |
| `display-panes [-d ms]` | `displayp` | Number each pane large; a digit selects that pane, any other key dismisses. Stays `display-panes-time` ms (`-d 0`: until a key) |
| `clock-mode [-t PANE]` | | Big clock filling the pane until a key is pressed |
| `command-prompt [-1bFkN] [-I initial,...] [-p prompt,...] [-T type] [template]` | | Asks each prompt (`-p`, default `(command) ` with a template). With a template the answers replace `%%` (`%1`..`%9` for the nth prompt; `%%%` quotes the answer for the command parser) and the result runs; without one the typed line runs. `-1`/`-k`: one key, no Enter. `-N`: digits only. `-F`: formats in the prompt and initial text |
| `confirm-before [-y] [-p prompt] command` | | Asks `y`/`n` (default `Confirm 'command'? (y/n)`), then runs the command on `y`. `-y` runs it at once |
| `show-messages` | `showmsgs` | Status-row messages of this session, oldest first (last 100) |
| `list-buffers` | `lsb` | Paste buffers in a list: `name: N bytes: "sample"` |
| `choose-buffer` | | Paste buffers, newest first, with a preview: Enter pastes the highlighted one, `d` deletes it, `/` filters |
| `suspend-client` | `suspendc` | Stop mpx as Ctrl-Z does and return to the shell; `fg` brings it back |
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
| `C-v` | Start or stop rectangle selection: the selection covers the block between its corners |
| `y` | Yank selection (syncs to OS clipboard via OSC 52) |
| `/` | Regex search forward |
| `n` / `N` | Next / previous search match |
| `q` / `Esc` | Exit copy mode |

These keys are the `copy-mode-vi` table, and each is a `send-keys -X` command: `v` runs `toggle-selection`, `y` runs `copy-selection-or-line-and-cancel`, `C-v` runs `rectangle-toggle`. Scripts run the same commands with `send-keys -t pane -X`, and rebind keys with `bind-key -T copy-mode-vi KEY send-keys -X command`; `unbind-key -T copy-mode-vi KEY` removes one, and `list-keys -T copy-mode-vi` lists the table. A binding takes effect in every attached terminal at once. The full command list is in [CLI.md](CLI.md#copy-mode). The emacs table, `copy-mode`, is bound the same way.

## Emacs Copy Mode (`mode-keys emacs`)

Set `mode_keys = "emacs"` in `config.toml`, or `set -g mode-keys emacs` at runtime, to use this table in copy mode (`Prefix [`) instead of the vi table above. Vi keys are unbound in it.

| Key | Action |
| :--- | :--- |
| `C-b` / `C-f`, Left / Right | Move cursor left / right |
| `C-p` / `C-n`, Up / Down | Move cursor up / down |
| `M-b` / `M-f` | Word backward / forward |
| `C-a` / `C-e`, Home / End | Start / end of line |
| `C-v` / PageDown | Scroll down a page, toward the bottom |
| `M-v` / PageUp | Scroll up a page, into history |
| `M-<` / `M->` | Top / bottom of scrollback |
| `C-Space` | Start or stop character selection |
| `C-w` / `M-w` | Copy the selection and leave (with no selection, just leave) |
| `Enter` | Copy the selection (no effect without a selection) |
| `C-g` / `Esc` / `q` | Leave copy mode |
| `C-s` / `C-r` | Open a search prompt forward / backward; `Enter` runs it |

`C-s` and `C-r` are not incremental: the search runs on `Enter`, not on each keystroke as in emacs.

