# Changelog

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
