# TODO: Redesign Agents Window and Sidebar

## Overview & Motivation

The agent management UI in mpx has evolved through practical daily use. This redesign refines the interface based on real developer workflow feedback:
1. **Zero Stolen Columns**: The permanent 2-column gutter (`● │`) stolen by the collapsed rail is eliminated. Terminal panes get 100% horizontal screen width unconditionally.
2. **On-Demand Drawer (`Prefix .`)**: The quick vertical agent drawer is retained for fast inspection, but rebound from `Prefix e` to `Prefix .` (easier to reach on the home/bottom row). When closed, it consumes 0 columns.
3. **No Shell Key Conflicts or Redundant Jump Keys**:
   - Remove `Alt-j` / `Alt-k` (`M-j` / `M-k` and macOS Option bindings `∆`/`˚`) to prevent collisions with shell navigation and readline shortcuts.
   - Remove `Prefix A` (`next-attention`) to eliminate redundant jump shortcuts.
   - Keep keybindings lean: navigation through agents is done intentionally via the drawer (`Prefix .`), the picker (`Prefix s`), or clicking window tabs.
4. **Status Line Window Flags & Agent Badges**: Keep the clean single bottom status bar (preserving all vertical space for editors like Neovim), but enhance window tabs to display tmux flags (`*`, `-`, `#`, `!`, `Z`) and agent indicators (`◆` needs input / `◆2` multi-agent attention, `●` working). Existing right-side telemetry (`◆ 1`) continues to report aggregate waiting agents.
5. **Unified Agent Switcher (`Prefix s`)**: Already implemented via the command palette's Agents tab (`palette agents` / Ctrl-Space) with instant fuzzy search across live and resumable agents. No changes needed to item 5.

---

## Target Experience & Architecture

### 1. Zero Stolen Columns: Eliminate the Collapsed Rail
- **Remove the 2-column persistent collapsed rail** (`● │`).
- `sidebar_width()` returns `0` whenever the drawer is not open.
- Panes and borders always have 100% of horizontal screen space.
- Ambient awareness moves entirely into the bottom status bar tabs and telemetry.

### 2. On-Demand Vertical Drawer (`Prefix .`)
- Rebind the drawer from `Prefix e` to **`Prefix .`**:
  - `.` is immediately accessible on the home/bottom row.
  - Opens the 34-column vertical drawer overlay for quick agent switching and inspection.
  - Closes with `Esc`, `q`, or `Prefix .`, instantly restoring full 100% pane width.

### 3. Clean Keybindings: Remove Root Keys & `Prefix A`
- **Remove `M-j` / `M-k` / `Alt-j` / `Alt-k`**:
  - Drops root key interception (`ROOT_BINDINGS` and `MAC_OPTION_BINDINGS`) so terminal shells receive all Alt/Option combinations unmodified.
- **Remove `Prefix A` (`next-attention`)**:
  - Drops the separate jump key in favor of intentional navigation.
- Agent switching is handled deliberately via:
  - **`Prefix .`**: On-demand vertical drawer with instant visual feedback.
  - **`Prefix s`**: Master-detail fuzzy finder.
  - **Mouse clicks**: Direct clicks on window tabs or drawer rows.

### 4. Single Status Line: Window Flags + Agent Badges
Keep the single bottom bar to maximize vertical code lines, while enriching the window tabs:
- **Standard tmux Window Flags**:
  - `*` Active window
  - `-` Last window
  - `#` Activity detected in window
  - `!` Bell / alert in window
  - `Z` Zoomed window
- **Agent Badges per Window**:
  - Single agent waiting: `1:dev ◆` (amber/bold)
  - Multiple agents waiting: `1:dev ◆2` (amber/bold with count)
  - Agents working: `2:tests ●` (green)
- **Aggregate Telemetry (Right Side)**:
  - Existing `◆ 1` badge reports total waiting agents across all sessions.
- **Mouse Hit-Testing**:
  - Clicking a window tab jumps straight to that window/pane.

### 5. Telescope-Style Agent Switcher (`Prefix s`)
Replace the cramped 5-row list with a modern side-by-side **Master-Detail Picker**:

```
┌─ Find Agent ─────────────────────────────────────────────────────────────────────────────┐
│ > auth                                                                                   │
├────────────────────────────────────────┬─────────────────────────────────────────────────┤
│ [main] worker-1   claude  ◆ needs you  │ worker-1 (claude · ~/src/mpx)                   │
│   auth.rs · Waiting for tool approval  │                                                 │
│                                        │ you › implement JWT auth validation             │
│ [dev]  api-bot    omp     ● working    │                                                 │
│   tests/auth_test.rs · Running tests   │ claude › I've created src/auth/jwt.rs and added │
│                                        │ validation logic. I'd like to run:              │
│ [main] reviewer   codex   ○ idle       │                                                 │
│   PR #42 review complete               │ $ cargo test --package auth                     │
│                                        │                                                 │
│ ◌ RESUME: auth-refactor   2d ago       │ Approve running `cargo test`? [y/N]             │
│   claude · ~/src/mpx (01a17ff1)        │                                                 │
├────────────────────────────────────────┴─────────────────────────────────────────────────┤
│ ↑/↓ move  Enter switch  Tab toggle preview  m message  x kill  r resume  Esc close        │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Fuzzy Search by Default**:
  - Opening `Prefix s` starts directly in filter/search mode.
  - Typing immediately filters by name, session, harness, directory, or status message.
  - `Ctrl-j`/`Ctrl-k` and `Up`/`Down` navigate results.
  - `Esc` clears filter or closes when filter is empty.
- **Master-Detail Layout**:
  - Split horizontally into two columns: Left ~40% (agent list), Right ~60% (live VT screen / transcript preview).
  - Left column uses full overlay height (15–25 rows visible at once instead of 5).
  - Resumable/stored sessions list cleanly at the bottom or appear seamlessly when filtering.
- **Responsive Fallback**:
  - On terminals narrower than ~100 columns, gracefully collapse into a single-column list with `Tab` to toggle preview.

---

## Action Items & Implementation Plan

### Step 1: Status Line & Activity Monitoring
- [x] Add activity (`#`) and bell (`!`) tracking to windows/panes in `src/daemon/session.rs` and `src/layout/workspace.rs`.
- [x] Track agent states per window tab:
  - Aggregate agent states in each window (`None`, `Working`, `NeedsInput(count)`).
  - Format window tab titles with flags and agent glyphs: e.g. `1:dev*`, `2:backend◆2`, `3:tests●-`.
- [x] Update `src/tui/render.rs` (`status_left`, `format_tab`) and `StatusBar`.
- [x] Add unit tests in `tests/tui/render.rs`, `tests/tui/status.rs`, and `tests/layout/workspace.rs`.

### Step 2: Remove Collapsed Rail & Rebind Drawer to `Prefix .`
- [x] Remove `rail_width`, `rail_rows`, and collapsed rail rendering from `src/tui/sidebar.rs` and `src/tui/frame.rs`.
- [x] Ensure `sidebar_width()` returns 0 when the sidebar mode is inactive (100% horizontal pane width).
- [x] Rebind drawer toggle in `src/tui/keymap.rs` and `DEFAULT_BINDINGS` from `e` (`sidebar`) to `.` (`sidebar`).
- [x] Update mouse hit-testing in `src/tui/mouse.rs` so pane clicks work across 100% width.
- [x] Update tests in `tests/tui/sidebar.rs` and `tests/tui/app/sidebar.rs`.

### Step 3: Remove Root Agent Keys, `Prefix A`, & Clean Keymap
- [x] Remove `M-j` / `M-k` and macOS Option bindings `∆`/`˚` from `ROOT_BINDINGS` and `MAC_OPTION_BINDINGS` in `src/tui/keymap.rs`.
- [x] Remove `Prefix A` (`next-attention`) binding from `DEFAULT_BINDINGS` in `src/tui/keymap.rs`.
- [x] Keep `Command::StepAgent` and `Command::NextAttention` for scriptable command-prompt / config usage if desired.
- [x] Clean up tests that assert on root `M-j` / `M-k` and `Prefix A`.

### Step 4: Retain Unified Palette Agent Switcher (`Prefix s`)
- [x] Already satisfied by `palette agents` on `Prefix s` (introduced in commit `67646da`). No modifications needed.

### Step 5: Documentation & Verification
- [x] Update documentation:
  - `README.md`
  - `docs/AGENT-MANAGER.md`
  - `docs/KEYBINDINGS.md`
  - `docs/CONFIG.md`
  - `www/`
- [ ] Run full test suite and verification gate:
  - `cargo test`
  - `cargo clippy --all-targets`
  - `ripwire-rs . --quality-delta`
  - `ripwire-rs . --test-gate`
