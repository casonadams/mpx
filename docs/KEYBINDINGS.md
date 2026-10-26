# Keyboard Shortcuts & TUI Navigation

`mpx` provides complete tmux muscle-memory parity. Both standard **`Ctrl-B`** and the popular rebind **`Ctrl-A`** are enabled out of the box as leader prefix keys.

---

## Prefix Leader Key

Press `Ctrl-B` or `Ctrl-A`, release, then press any of the following action keys:

### Window Management (Tabs)

| Key | Action | Description |
| :--- | :--- | :--- |
| `Prefix + c` | **New Window** | Creates a new window tab running default shell in current directory |
| `Prefix + n` | **Next Window** | Switches to the next window tab |
| `Prefix + p` | **Previous Window** | Switches to the previous window tab |
| `Prefix + 0..9`| **Select Window** | Directly jumps to window tab by index |

---

### Pane Management (Splits)

| Key | Action | Description |
| :--- | :--- | :--- |
| `Prefix + %` / `Prefix + \|` | **Split Vertical** | Splits active pane vertically (side-by-side, vertical divider `│`) |
| `Prefix + "` / `Prefix + -` | **Split Horizontal** | Splits active pane horizontally (stacked, horizontal divider `─`) |
| `Prefix + o` / `Tab` | **Cycle Focus** | Cycles focus to the next pane in the current window |
| `Prefix + h` / `Left` | **Focus Left** | Directional pane focus: moves focus to neighboring pane on the left |
| `Prefix + j` / `Down` | **Focus Down** | Directional pane focus: moves focus to neighboring pane below |
| `Prefix + k` / `Up` | **Focus Up** | Directional pane focus: moves focus to neighboring pane above |
| `Prefix + l` / `Right` | **Focus Right** | Directional pane focus: moves focus to neighboring pane on the right |
| `Prefix + z` | **Toggle Zoom** | Maximizes active pane to 100% full screen (press again to un-zoom) |
| `Prefix + x` | **Kill Pane** | Terminates process and removes active pane from window |
| `Prefix + d` / `Prefix + q`| **Detach** | Detaches cleanly from session (leaves background daemon & tasks running) |

---

### Visual Focus Indicators

- **Active Pane Border**: The dividers and corners adjacent to the focused pane glow in vibrant **Green** (`Color::Green`), connected with box-drawing junction connectors (`┬`, `┴`, `├`, `┤`, `┼`).
- **Foreground Dimming**: Inactive panes automatically soften foreground text so the active pane stands out with full brightness while preserving your terminal's natural background theme.

---

## Mouse Mode

Mouse interaction is supported natively across macOS, Linux, and Windows:

- **Left Click**: Focuses the clicked pane immediately.
- **Click & Drag**: Selects text within the pane.
- **Release Drag**: Automatically copies the highlighted text to your system clipboard (`pbcopy` on macOS, OSC 52 ANSI clipboard on Linux/Windows).
- **Scroll Wheel Up**: Enters Copy Mode and smoothly scrolls history upwards.
- **Scroll Wheel Down**: Scrolls downwards toward the live bottom prompt.

---

## Copy Mode (Vi Keybindings)

Press **`Prefix + [`** (or scroll the mouse wheel up) to enter copy mode.

| Key | Action |
| :--- | :--- |
| `h` / `j` / `k` / `l` | Move cursor left, down, up, right (arrow keys also supported) |
| `w` / `b` | Jump word forward / backward |
| `0` / `$` | Jump to beginning / end of line |
| `Ctrl+U` / `Ctrl+D` | Scroll half-page up / down (PageUp / PageDown also supported) |
| `g` / `G` | Jump to the very top / bottom of scrollback history |
| `v` | Start character-wise visual text selection |
| `Shift+V` | Start full-row line selection (`VISUAL LINE`) |
| `y` / `Enter` | Yank (copy) selection to system clipboard and exit copy mode |
| `Esc` | Clear selection, or exit copy mode if no selection is active |
| `q` | Exit copy mode immediately and return to live terminal |
