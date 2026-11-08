# mpx

[![Crates.io](https://img.shields.io/crates/v/mpx.svg)](https://crates.io/crates/mpx)
[![CI](https://github.com/casonadams/mpx/actions/workflows/ci.yml/badge.svg)](https://github.com/casonadams/mpx/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Documentation](https://img.shields.io/badge/docs-casonadams.github.io%2Fmpx-blue)](https://casonadams.github.io/mpx/)

A modern, ultra-fast terminal multiplexer in Rust: tmux key bindings and `mpx import-tmux` for your `.tmux.conf`, crash-resilient with automatic session recovery, zero-flicker rendering, and a sleek fuzzy-search command palette.

> **100% Local & Zero Telemetry**: `mpx` is an offline-by-design terminal engine. It contains **zero network calls, zero analytics, zero telemetry, and zero model training**. Commands, keystrokes, and outputs never leave your machine. It operates purely over local Unix domain sockets or Windows Named Pipes.

---

## Features

- tmux key bindings, and `mpx import-tmux` to convert your `.tmux.conf`.
- Sessions survive crashes and reboots: layout, working directories and tools are snapshotted continuously and brought back with `mpx restore`.
- Flicker-free rendering with DEC 2026 synchronized updates.
- Fuzzy command palette (`Ctrl-Space`) and vi copy mode.
- Runs on Linux, macOS and Windows (ConPTY), talking over local sockets or named pipes.

---

## Quickstart

### Installation

```bash
# macOS & Linux (Homebrew)
brew install casonadams/tap/mpx

# Ubuntu & Debian (APT)
echo "deb [trusted=yes] https://casonadams.github.io/homebrew-tap/apt stable main" | sudo tee /etc/apt/sources.list.d/casonadams.list
sudo apt update && sudo apt install mpx

# Fedora & RHEL (DNF)
curl -fsSL https://casonadams.github.io/homebrew-tap/rpm/casonadams.repo | sudo tee /etc/yum.repos.d/casonadams.repo
sudo dnf install mpx

# Cargo (crates.io)
cargo binstall mpx || cargo install mpx
```

See **[Installation Guide](docs/INSTALL.md)** for prebuilt `.deb`, `.rpm`, `.tar.gz`, and Windows `.zip` archives.

---

### 30-Second Tour

```bash
# 1. Start or attach to the default session "0" (Ctrl-b prefix, tmux keys)
mpx

# 2. Run a background watcher without nesting shells
mpx run --name test-watcher -- cargo watch -x test

# 3. List sessions (tmux `ls`) and panes
mpx ls
mpx list-panes

# 4. Stream logs, or wait for a regex instead of sleep-polling
mpx logs -f test-watcher
mpx wait test-watcher --match 'test result: ok' --timeout 120

# 5. Split a pane without stealing focus (tmux -h / -d)
mpx split -h -d --pty -- htop

# 6. Rename session (Prefix $ or CLI)
mpx rename-session -t 0 dev
```

---

## Command Palette & Navigation

`Ctrl-Space` (no prefix needed) opens the unified **Command Palette**: instant fuzzy search across:
- **Sessions & Windows**: Jump instantly to any pane across any session.
- **Commands**: Fuzzy search and execute all multiplexer commands (`split-window`, `kill-pane`, `resize`, `rename`).
- **Keybindings**: Searchable cheat sheet of all keyboard shortcuts.

`Prefix s` and `Prefix w` open the interactive session and window tree chooser (`choose-tree`).

With the mouse, a left click focuses a pane, a middle click pastes the top paste buffer into the pane under the pointer, and a right click opens a menu for what is there: pane (split, swap, kill, mark, zoom, disable input), window tab, session name, or split border (layouts). [KEYBINDINGS.md](docs/KEYBINDINGS.md#mouse) lists every mouse action.

Several terminals can attach to one session. `Prefix D` (`choose-client`) lists every attached client and detaches the one you pick. From the shell, `mpx list-clients` lists the attached clients across sessions, `mpx detach-client -t CLIENT` detaches one, and `mpx refresh-client -C 120x40` gives a client a size. `mpx attach -r` attaches read-only: keys, paste and mouse reports stay out of the panes, while navigation, copy mode and detach still work. `mpx attach -d` detaches the session's other clients first, and `mpx attach -x` does the same and, on Unix, sends SIGHUP to their parent processes (the shells they were started from, as tmux does).

The window is laid out for the clients the `window_size` option names: `latest` (the default) follows the most recently active client, `smallest` lays it out so every client sees all of it, `largest` for the biggest client, and `manual` uses `default_size`. `mpx resize-window -x 100 -y 30` gives a window a size of its own, which overrides that for the window until it is resized again. `mpx link-window` shows a window at a second index of the same session, and `mpx unlink-window` removes one index; the panes are shared. Linking a window of another session, and session groups (`new-session -t`), are refused: each session owns its panes in its own daemon, so windows cannot be shared between sessions.

---

## Survives Crashes and Reboots (`mpx restore`)

The daemon continuously and atomically snapshots your workspace. Every time a session starts (a plain `mpx` after a crash, a reboot or `mpx kill-server`), it brings back:
- All windows, split layouts, and pane proportions.
- Each pane's active working directory (`cwd`).
- Config-driven process resumption for tools running in your panes:
  - Claude Code: resumes with `claude --resume {id}` or `claude --continue`.
  - oh-my-pi: resumes with `omp --restore {id}` or `omp --continue`.
  - Codex / rho / Antigravity / OpenCode: resumes conversation state.
  - Neovim: sources `Session.vim` from the pane's directory when it exists, and starts empty otherwise.

Run `mpx --no-restore` to start a session with a brand new state instead.

Configure or add custom tool resumption rules in `~/.config/mpx/config.toml`:

```toml
[[resume]]
name = "claude"
detect = ["claude"]
resume = ["claude", "--resume", "{id}"]
fallback = ["claude", "--continue"]

[[resume]]
name = "nvim"
detect = ["nvim"]
fallback = ["nvim", "-c", "silent! source Session.vim"]
```

---

## Documentation

- **[Architecture & Design](docs/ARCHITECTURE.md)**: Tokio async daemon, split tree, dual-buffer engine, and TUI.
- **[CLI Reference & Automation](docs/CLI.md)**: tmux-style commands, waiting, process runs, what carries over from tmux and what does not, and `mpx import-tmux` to convert a `.tmux.conf`.
- **[Keyboard Shortcuts](docs/KEYBINDINGS.md)**: Prefix bindings, `:` command prompt with vim aliases, and Vi copy mode.
- **[Configuration](docs/CONFIG.md)**: Prefix, key bindings, `remain_on_exit`, and `[[resume]]` rules.
- **[Installation Guide](docs/INSTALL.md)**: Package managers, static archives, and cross-platform builds.
- **[Interactive Website](https://casonadams.github.io/mpx/)**: Online visual demo and live documentation.

---

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
