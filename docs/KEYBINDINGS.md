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
| `D` | `choose-client` | Pick another attached client from a list; Enter detaches it |
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
| `<` | `display-menu` | Menu of window commands: rename, respawn, mark, kill, new window at the end |
| `>` | `display-menu` | Menu of pane commands: go to top or bottom of history, zoom, kill, break out, mark, respawn |

### Popups and menus

A menu lists its items with their keys. `j`/`k` or the arrows move the selection, Enter runs the selected item, an item's key runs that item at once, and `q` or Esc closes it. Disabled items (`-` before the name in the command) are shown but never run; separators are drawn as lines. Menus are placed with `-x`/`-y`: a cell offset, `N%`, `C` (centred, the default) or `R` (far edge).

A popup (`display-popup [-E] [-w W] [-h H] [-x X] [-y Y] [-T title] [-d dir] [-t pane] [command]`) runs its command on a pty of its own in a box over the panes, 80% of the screen by default. Keys go to the popup's command. Without `-E` the popup stays up after the command exits, and the next key closes it; with `-E` it closes as soon as the command exits.

---

## Mouse

These need `mouse = true` (the default). With `mouse = false` mpx ignores mouse events. In a read-only attach (`mpx attach -r`) right click opens no menu and middle click pastes nothing.

- Left click focuses the pane under the pointer. Left drag selects text from the press point, and releasing copies it to the clipboard.
- Drag a split border to resize the panes on either side of it.
- Scroll wheel enters copy mode and scrolls the history.
- Middle click pastes the top paste buffer into the pane under the pointer (`paste-buffer`), focusing that pane first.
- Right click opens a menu for what is under the pointer. Its items are the same commands as the keys above, and a menu closes with `q` or Esc, as the popup menus below do:
  - a pane (the pane is focused first): horizontal and vertical split, swap up and down, kill, respawn, mark or unmark, zoom or unzoom, disable or enable input (`select-pane -d`/`-e`), break out;
  - a window tab (the window is selected first): rename, kill, respawn, new window;
  - the session name in `status-left` or `status-right`: next and previous session, new window, choose tree, detach;
  - a split border: the layouts (even horizontal and vertical, main horizontal and vertical, tiled, spread out).

In copy mode the active pane keeps the mouse for its own selection, so a right click there opens no menu.

---

## Changing bindings while running

`:bind-key [-n] key command...` and `:unbind-key [-n] key` change the keymap of the running session at once, in every attached terminal; a terminal attaching later picks them up too. `-n` binds without the prefix. Chain commands with `\;` (`:bind r reload-config \; display-message reloaded`). Bindings last until the daemon stops; put lasting ones in `[bind]` / `[bind_root]` of `config.toml`. Only the commands in the table below are available; others fail with `unknown command`.

## Command Prompt (`Prefix :`)

Vim-style command line on the status row. Supports tab-completion, history (`Up`/`Down`), quotes, and escapes.

| Command | Aliases | Description |
| :--- | :--- | :--- |
| `split-window [-h] [-d] [-Z] [-c dir] [cmd...]` | `split`, `sp`, `vsplit`, `vs` | Split active pane (`-Z` keeps a zoomed window zoomed) |
| `new-window [-n name] [-c dir] [cmd...]` | `new`, `tabnew` | Create window |
| `rename-session NAME` | `rename` | Rename current session |
| `reload-config` | `source-file` (without a file) | Re-read the config file into the TUI and the session's daemon |
| `run-shell [-b] [-t PANE] [-d SECONDS] COMMAND` | | Run a shell command in the daemon, formats expanded for PANE; its output goes to the status row, a message per line (`-b` does not wait) |
| `if-shell [-bF] [-t PANE] SHELL-COMMAND COMMAND [COMMAND]` | | Run the first command when the shell command exits 0, else the second; with `-F` the first word is a format (true when non-empty and not `0`) |
| `wait-for [-S \| -L \| -U] CHANNEL` | | Wait for a signal on CHANNEL, or signal it (`-S`), or take (`-L`) or release (`-U`) its lock |
| `set-hook [-a] HOOK COMMAND`, `set-hook -u HOOK`, `set-hook -R HOOK` | | Set, append to, remove or run now the commands of a session hook (see [Shell, Channels and Hooks](CLI.md#shell-channels-and-hooks)) |
| `show-hooks` | | List every hook command, `HOOK COMMAND`, in an overlay |
| `set -s command-alias NAME=COMMAND` | | Make NAME a command for this prompt, bindings and hooks (`set -su command-alias NAME` removes it) |
| `source-file [-q] FILE...` | | Run the command lines of each file, in order; `-q` ignores a missing file |
| `set-option [-g] [-u] [-q] NAME [VALUE]` | `set`, `set-window-option`, `setw` | Change a UI option of this terminal until the next `reload-config` (names and values in [Options at run time](CONFIG.md#options-at-run-time)) |
| `show-options [-g] [-v] [-q] [NAME]` | `show`, `show-window-options`, `showw` | List the options, or show one on the status row |
| `rotate-window [-D\|-U] [-Z]` | `rotatew` | Move each pane to the layout slot before its own (`-D`: after); `-Z` keeps a zoomed window zoomed |
| `move-window -t INDEX` | `movew` | Move the active window to INDEX |
| `swap-window [-s INDEX] -t INDEX` | `swapw` | Exchange two windows (`-s` defaults to the active one) |
| `join-pane [-dh] [-s PANE] [-t PANE]` | `joinp`, `move-pane`, `movep` | Move PANE (default: the marked pane) next to the target (default: the active pane), stacked or side by side with `-h`; `-d` keeps the active pane focused; an emptied window goes away |
| `respawn-pane [-k] [-c DIR] [-t PANE] [CMD...]` | `respawnp` | Restart the pane's process in place; `-k` kills a running one first, otherwise only an exited pane is restarted |
| `respawn-window [-k] [-c DIR] [-t INDEX] [CMD...]` | `respawnw` | Restart every pane of the window in place; refused while a pane still runs unless `-k` |
| `find-window [-iNT] PATTERN` | `findw` | Open the sessions and windows palette filtered by PATTERN (names and titles; content search is not supported) |
| `display-message [-p] [FORMAT]` | `display` | Show the expanded [format](CLI.md#formats) on the status row; without one, describe the current pane |
| `rename-window NAME` | `renamew` | Rename active window |
| `kill-pane [-t TARGET]` | `killp` | Close pane |
| `kill-window [-t INDEX]` | `killw` | Close window |
| `kill-server` | `qa` | Terminate session daemon |
| `select-pane [-t TARGET] [-Z] [-m\|-M] \| -L/-R/-U/-D` | `selectp` | Move pane focus (`-Z` keeps a zoomed window zoomed) |
| `select-pane [-t TARGET] -d\|-e` | `selectp` | Turn a pane's input off (`-d`: keys sent to it are dropped) or back on (`-e`) |
| `resize-pane -L/-R/-U/-D [N]`, `resize-pane -Z` | `resizep` | Resize / zoom |
| `link-window`, `unlink-window`, `resize-window` | | Refused: a window cannot be linked elsewhere, and a window has no size of its own (see [CLI.md](CLI.md#windows-and-layout)) |
| `choose-tree` | | Interactive session/window tree |
| `display-panes [-d ms]` | `displayp` | Number each pane large; a digit selects that pane, any other key dismisses. Stays `display-panes-time` ms (`-d 0`: until a key) |
| `clock-mode [-t PANE]` | | Big clock filling the pane until a key is pressed |
| `command-prompt [-1bFkN] [-I initial,...] [-p prompt,...] [-T type] [template]` | | Asks each prompt (`-p`; with a template the default is the template's command in parentheses, such as `(rename-window) `, and without one `:`). With a template the answers replace `%%` (`%1`..`%9` for the nth prompt; `%%%` quotes the answer for the command parser) and the result runs; without one the typed line runs. `-1`/`-k`: one key, no Enter. `-N`: digits only. `-F`: formats in the prompt and initial text |
| `confirm-before [-y] [-p prompt] command` | | Asks `y`/`n` (default `Confirm 'command'? (y/n)`), then runs the command on `y`. `-y` runs it at once |
| `show-messages` | `showmsgs` | Status-row messages of this session, oldest first (last 100) |
| `list-buffers [-F format]` | `lsb` | Paste buffers in a list: `name: N bytes: "sample"`, or each line formatted by `-F` (`#{buffer_name}`, `#{buffer_size}`, `#{buffer_sample}`) |
| `choose-buffer` | | Paste buffers, newest first, with a preview: Enter pastes the highlighted one, `d` deletes it, `/` filters |
| `set-buffer [-a] [-b name] [-n new] [data]` | `setb` | Store `data` in a buffer, or append with `-a`; `-n` renames the buffer |
| `show-buffer [-b name]` | `showb` | The text of a buffer, in an overlay |
| `delete-buffer [-b name]` | `deleteb` | Delete a buffer (the newest unless `-b`) |
| `load-buffer [-b name] path` | `loadb` | Store a file in a buffer |
| `save-buffer [-a] [-b name] path` | `saveb` | Write a buffer to a file (`-a` appends) |
| `paste-buffer [-d] [-p] [-r] [-s sep] [-b name] [-t pane]` | `pasteb` | Paste a buffer into a pane of this session (`-d` deletes it after) |
| `capture-pane [-p] [-e] [-J] [-S line] [-E line] [-b name] [-t pane]` | `capturep` | Capture a pane's history and screen into a buffer, or into an overlay with `-p` (see [CLI.md](CLI.md#capture-and-paste-buffers)) |
| `set-environment [-u\|-r] name [value]` | `setenv` | Set a variable for panes started afterwards; `-u` unsets it, `-r` keeps it out of new panes |
| `show-environment [name]` | `showenv` | The session environment, or one variable, in an overlay |
| `suspend-client` | `suspendc` | Stop mpx as Ctrl-Z does and return to the shell; `fg` brings it back |
| `list-clients [-t SESSION] [-F FORMAT]` | `lsc` | Attached clients, one line each (format variables as in [CLI.md](CLI.md)) |
| `detach-client [-a] [-t CLIENT]` | | Detach CLIENT, or with `-a` the other clients of this session; without `-t` this client (plain `detach` is the same) |
| `refresh-client [-S] [-C WxH] [-t CLIENT]` | `refresh` | Redraw this client (or CLIENT), giving it a size with `-C` |
| `switch-client [-t SESSION] [-l] [-n] [-p]` | `switchc`, `session` | Switch to SESSION, or to the last, next or previous session. `-c` is refused here: run it from the command line |
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

