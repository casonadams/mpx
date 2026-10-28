# Changelog

## [0.2.0](https://github.com/casonadams/mpx/compare/v0.1.2...v0.2.0) (2026-10-28)


### ⚠ BREAKING CHANGES

* **tui,vt:** Compositor::draw_shortcut_hud takes a ShortcutLayout, the shortcuts and the prefix; hud_rect is replaced by shortcut_layout. CellStyle gains a `reverse` field.
* **tui,vt,node:** Grid::cells is removed (use Grid::screen_lines); Compositor::draw_pane_content and Compositor::draw_dividers take a &mut Canvas.
* library API: daemon::run_daemon takes state_dir and config,
* protocol Output events gain offset/next, Subscribe gains since, and new Lagged/Agent events; DaemonState::new takes a state directory.
* `mpx ls` lists sessions (use `mpx list-panes`); Ctrl-a is no longer a leader key; `swap-pane` takes `--source`; the JSON protocol changed.

### Features

* actor daemon, idempotent requests, durable pipes, session restore ([267c3bb](https://github.com/casonadams/mpx/commit/267c3bb9bb2c5d94e1c0ca91024aa5d17e165191))
* agent manager, tmux parity, and agent-to-agent primitives ([8a9cf73](https://github.com/casonadams/mpx/commit/8a9cf73f41ecb7e0b4faa1325ea3cb5627fbbdd3))
* **agents:** add rho as built-in agent harness with recovery support ([e840f38](https://github.com/casonadams/mpx/commit/e840f38b73b6ae5978d1217807fb8b15786fb520))
* **config:** add [style] theming, borders, status line, and dimming options ([492998b](https://github.com/casonadams/mpx/commit/492998b70bbc3eeb6b79987b4a65f1ecf1a93e6b))
* **tui,vt:** complete delayed shortcut popup, click-to-focus, editor bar fidelity ([f041d34](https://github.com/casonadams/mpx/commit/f041d34e8e8c04bc8e72af027d0a92b338f996f5))
* **tui:** add copy mode search up (?) and search down (/) ([69a126d](https://github.com/casonadams/mpx/commit/69a126ddc7b9a0640fd5b9eb06036e0b6c105ffc))
* **tui:** enable switching windows and focusing panes by clicking status bar tabs ([cdc601c](https://github.com/casonadams/mpx/commit/cdc601c391d74a6ef3e4e3c2a7ccbf88f4fb74e8))
* **tui:** map prefix s to agents, show zoom and copy indicators in status bar, and add tab completion cycling ([06e78b4](https://github.com/casonadams/mpx/commit/06e78b45e606c0ebbdde158fb18ac6272f53aa75))
* **tui:** restore vertical shortcut popup and clean up web pages ([f22fc2e](https://github.com/casonadams/mpx/commit/f22fc2ed6812f130e743245b1b2cbeb154369d41))


### Bug Fixes

* **build:** resolve windows compilation failure in pty and sessions ([e1e341f](https://github.com/casonadams/mpx/commit/e1e341f9ebc61607b68c9be566c0655b9d7b7bd9))
* **cli,tests:** detach daemon process on Windows and gate Unix-specific e2e tests ([aabdcc7](https://github.com/casonadams/mpx/commit/aabdcc709eabc56444ebaf850adba50614b3bbac))
* **cli:** refuse and stop daemons from incompatible mpx versions ([429bf73](https://github.com/casonadams/mpx/commit/429bf733b2dc100603c244885280676d24e1420a))
* **daemon:** keep window names given at creation ([f56f28f](https://github.com/casonadams/mpx/commit/f56f28fcc2ac9e781388e0a17919239692c3ac30))
* **ipc,tests,ci:** normalize Windows pipe endpoints and add Unix tools to runner PATH ([0652bd4](https://github.com/casonadams/mpx/commit/0652bd4ca5ce0cf7777b3398660233ea8c942e35))
* **node,cli:** treat killed task as None exit code and match terminal viewport in layout test ([a09c59a](https://github.com/casonadams/mpx/commit/a09c59acf3b696008be05bbba9a8065e5c00a77c))
* **tests:** gate raw pty input tests to Unix in tui app suite ([b91bb9b](https://github.com/casonadams/mpx/commit/b91bb9bc2a0cbdc8be999243779f207546d3dd54))
* **tests:** handle unsupported process probe on Windows in node tests ([e72b1ed](https://github.com/casonadams/mpx/commit/e72b1edd88d63aac5f07c75c13a4ac0f0dcd6591))
* **tests:** resolve Windows test compilation and symlink errors ([bb9b731](https://github.com/casonadams/mpx/commit/bb9b7313098a5114d295f4d1cfb1e7cf8a5cc6a8))
* **tests:** silence unused logs helper warning on Windows ([a5d02a8](https://github.com/casonadams/mpx/commit/a5d02a870888b15ac2f81d30db522fd616ec9c90))
* **tests:** use sh instead of /bin/sh in pty basics test for Windows ([8343181](https://github.com/casonadams/mpx/commit/834318117841687abb131d64f91b4ea0c6f9bda7))
* **tui:** align clock to edge, remove prefix badge, silence apple terminal session restore ([7bf3b98](https://github.com/casonadams/mpx/commit/7bf3b98cd34002afabf8f779f989106e79e2fb88))


### Performance Improvements

* **tui,vt,node:** incremental frames, row-rotation scrolling, syscall cwd ([745dc7b](https://github.com/casonadams/mpx/commit/745dc7b285122afb83d00be5c7f6a58c1af5aeba))
* **vt,buffer,ipc,tui:** optimize scrollback tailing, ring buffer reads, probe concurrency, and copy search ([d6c43be](https://github.com/casonadams/mpx/commit/d6c43be02fec19f1732332eedb50ca6961828506))


### Tests

* cover every function in the change blast radius ([93fb026](https://github.com/casonadams/mpx/commit/93fb02624a3d7cb465a1814deed667a18f37c975))

## [0.1.2](https://github.com/casonadams/mpx/compare/v0.1.1...v0.1.2) (2026-10-26)


### Features

* **mcp,layout:** support detached pane split, explicit focus, and non-blocking status tools ([6582055](https://github.com/casonadams/mpx/commit/65820551fd89a2f4c0275f1aff0bfcda74a4692d))
* **tui,pty:** inherit pane cwd on split/new-window, format 12h time with seconds, and eliminate copy mode badge flicker ([58aae49](https://github.com/casonadams/mpx/commit/58aae49bcaff908d685ab74621c257ae808e1bd0))
* **tui:** add 1-second status bar clock update and subtle tasteful pane dimming ([9f62549](https://github.com/casonadams/mpx/commit/9f625493fdcca9dbac09a2a635141c521376cd10))
* **tui:** add green active pane outline, box-drawing junction connectors, and subtle bg dimming ([3cbe4f7](https://github.com/casonadams/mpx/commit/3cbe4f7d3d0b84f47bb231a51f4ef2d33707a816))


### Bug Fixes

* **tests:** use Windows Named Pipe path format for test daemon listener ([290e1d6](https://github.com/casonadams/mpx/commit/290e1d6f1d2c683d00091a0d737e90e5271b7f54))
* **tui,ci:** ensure clean terminal exit and fix Windows Named Pipe connection retry ([c68b7c7](https://github.com/casonadams/mpx/commit/c68b7c791c02093d8ea5d7d79406beb67b150e09))
* **tui:** disable autowrap to eliminate bottom row scrolling trails and join task stdout ([c524e8e](https://github.com/casonadams/mpx/commit/c524e8e4f73c3f8a8907f920ce55c8ad53206721))
* **tui:** eliminate cursor flickering with DEC 2026 sync updates and frame pacing ([f1ab3ae](https://github.com/casonadams/mpx/commit/f1ab3ae73747be89e7f668905126a9c68462f628))
* **tui:** eliminate mouse drag latency, restore clean HUD popup dismissal, and fix Windows CI ([6eda165](https://github.com/casonadams/mpx/commit/6eda16542bb98402cd8d5a4bda74e088726b6443))
* **tui:** eliminate mouse move redraws to stop cursor flicker, enable mouse drag selection, and fix scrollback navigation ([1bc51a8](https://github.com/casonadams/mpx/commit/1bc51a845f05aae7eae43b44c603a3a95ecade4a))
* **tui:** ensure clean exit when last pane exits or user detaches/quits ([5564a6d](https://github.com/casonadams/mpx/commit/5564a6d9037362e485e85b562c3bacc3c97db062))
* **tui:** preserve native terminal background untouched and dim only text ([4dbe13c](https://github.com/casonadams/mpx/commit/4dbe13c2b62fb6d641a2d6a0cce9740d0ae5e382))
* **vt:** implement DECSTBM margins, SU, SD, DL, IL, and RI for perfect Neovim scrolling ([352cbdc](https://github.com/casonadams/mpx/commit/352cbdc5a8803141308e144fe8576fa339d42735))


### Performance Improvements

* **tui,vt:** optimize grid scrolling, layout caching, diff rendering, and tail scan ([917fbe0](https://github.com/casonadams/mpx/commit/917fbe05065205e79f54ae59e5385af8eee784a8))

## [0.1.1](https://github.com/casonadams/mpx/compare/v0.1.0...v0.1.1) (2026-10-26)


### Features

* **cli:** make bare mpx start or attach to interactive session by default ([c110936](https://github.com/casonadams/mpx/commit/c110936511f542a55820dc82a934e21263b12dbc))
* **core:** implement mpx stream supervisor and cross-platform IPC foundation ([ab2ebd7](https://github.com/casonadams/mpx/commit/ab2ebd75ed8fa072d0e7bbd28b47538109a5cbfa))
* **pipe,mcp:** implement inter-agent virtual pipes and stdio MCP server ([ba7194a](https://github.com/casonadams/mpx/commit/ba7194afb3b4d21ef91f2788bda1972014c39aa8))
* **tui,daemon:** auto-destroy and collapse panes on process exit ([6de1502](https://github.com/casonadams/mpx/commit/6de1502ed3b89f4f0b835cc23cdaa50317a8f3bf))
* **tui,layout:** reserve 1-char divider lines between panes and purge dead nodes ([a1d1c34](https://github.com/casonadams/mpx/commit/a1d1c34b9e2d72f84820a7ed21c4f96d73c28c77))
* **tui,vt:** implement 100k line scrollback buffer in copy mode and Neovim vertical popup ([19a2122](https://github.com/casonadams/mpx/commit/19a2122cfd14d84fd598b544ce18dd9873d9819c))
* **tui:** clean minimalist tmux-style layout, live cursor, and which-key popup ([c3c83d9](https://github.com/casonadams/mpx/commit/c3c83d9bf0718083ae41526579b8fedf0ad60978))
* **tui:** full tmux parity with window tabs (c/n/p/0-9), mouse mode, vi copy mode, and modern right-side HUD ([24f629a](https://github.com/casonadams/mpx/commit/24f629a2e21c8dcdb855e75cc9ba312244ab0d48))
* **tui:** implement full vi copy/visual mode with shift-v row highlight, movable cursor, and Neovim floating HUD ([004b62e](https://github.com/casonadams/mpx/commit/004b62e0bbd876b0c61923c3509f32eedb5ff6de))
* **tui:** implement full-screen interactive compositor and mpx attach ([8ff1a38](https://github.com/casonadams/mpx/commit/8ff1a384e9cdca53929d7048d2f7c9eab28a9703))
* **vt,layout:** implement virtual terminal grid and binary split tree layout ([7b7d078](https://github.com/casonadams/mpx/commit/7b7d078baa184ab9be7cc0b5cc2b8d9043703cb4))


### Bug Fixes

* **pty,tui:** prevent byte drops on concurrent reads, ensure active shell spawning, and fix terminal keycodes ([695490b](https://github.com/casonadams/mpx/commit/695490bd414145cadb18255a67ed745ee766754d))
* **tui,vt:** support 24-bit TrueColor, fix color degradation in convert_color, and reset cell backgrounds ([124c2df](https://github.com/casonadams/mpx/commit/124c2dff7549fee746c28c3da70bb8f5b405e4ce))
* **tui:** align popup box borders and remove which-key title ([24b83bc](https://github.com/casonadams/mpx/commit/24b83bc0cdf9d0e8c3bd7d4503f0b3336d08311a))
* **tui:** auto-spawn window on startup, ensure daemon health ping, and add stop/kill-server command ([14d464c](https://github.com/casonadams/mpx/commit/14d464c927f8fd4d5bc2e6590a80b3b7e8a21054))
* **tui:** correct split directions for % and " and allow unlimited consecutive splits ([750b0b3](https://github.com/casonadams/mpx/commit/750b0b3645495a40f2c0638a307e420d3739110a))
* **tui:** eliminate cursor jumping with atomic frame buffer and add live PTY stream reflow on resize ([96b9cf3](https://github.com/casonadams/mpx/commit/96b9cf3caadc53f7365405147c92342934b808ad))
* **vt,pty:** respond immediately to DSR cursor reports and DA queries so Neovim exits without delay ([a9832c1](https://github.com/casonadams/mpx/commit/a9832c143ee14f4de8b74e70c4156e99f9bb93f6))


### Performance Improvements

* **tui:** add Event::Paste, key batching, and chunk draining for instantaneous pasting ([d4e0ab8](https://github.com/casonadams/mpx/commit/d4e0ab883db166147d5c4f086bf6977ee9bdb039))
