# mpx

[![Crates.io](https://img.shields.io/crates/v/mpx.svg)](https://crates.io/crates/mpx)
[![CI](https://github.com/casonadams/mpx/actions/workflows/ci.yml/badge.svg)](https://github.com/casonadams/mpx/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Documentation](https://img.shields.io/badge/docs-casonadams.github.io%2Fmpx-blue)](https://casonadams.github.io/mpx/)

A modern, ultra-fast terminal multiplexer in Rust: tmux key bindings and a tmux command shim, crash-resilient with automatic session recovery, zero-flicker rendering, and a sleek fuzzy-search command palette.

> **100% Local & Zero Telemetry**: `mpx` is an offline-by-design terminal engine. It contains **zero network calls, zero analytics, zero telemetry, and zero model training**. Commands, keystrokes, and outputs never leave your machine. It operates purely over local Unix domain sockets or Windows Named Pipes.

---

## What Sets `mpx` Apart?

Traditional multiplexers like `tmux` rely on 30-year-old C architectures (`libevent`) and external scripts (`tmux-resurrect`) that scrape process tables to save state. Heavyweight alternatives like `zellij` consume 60–120MB+ idle RAM with WebAssembly runtimes.

`mpx` provides a pure, modern terminal multiplexer in Tokio async Rust:

```mermaid
graph TD
    subgraph ClientInterfaces [Client & Invocation Interfaces]
        direction LR
        TMUX["<b>tmux Invocation Shim</b><br/>(split-window, send-keys, capture-pane)"]
        CLI["<b>mpx CLI</b><br/>(run, attach, restore, list)"]
        TUI["<b>Human TUI (60 FPS)</b><br/>• DEC 2026 Synchronized Updates<br/>• 3-Tab Command Palette (Ctrl-Space)<br/>• Vi Copy Mode (100k lines)"]
    end

    subgraph DaemonEngine [mpx Daemon Engine (~5MB)]
        direction TB
        IPC["<b>Local Socket / Named Pipe IPC</b><br/>• Typed JSON-RPC Protocol"]
        
        subgraph SessionCore [Session & Layout Core]
            direction LR
            WORKSPACE["<b>Workspace & Split Tree</b><br/>(Even, Tiled, Main-Vertical)"]
            RECOVERY["<b>Continuous Snapshot Store</b><br/>• Auto-save layout, cwd, env<br/>• Config-driven [[resume]] rules"]
        end

        subgraph ExecutionNodes [Execution Nodes]
            direction LR
            PTY["<b>Interactive PTY Panes</b><br/>(zsh / nvim / htop / claude / omp)"]
            VT["<b>Dual-Buffer Engine</b><br/>• Raw Byte Stream + 2D VT Grid"]
        end

        IPC --> SessionCore
        SessionCore --> ExecutionNodes
    end

    ClientInterfaces --> IPC
```

| Dimension | `tmux` | `zellij` | `mpx` |
| :--- | :--- | :--- | :--- |
| **Language** | C (libevent) | Rust (multi-thread actor + async-std) | **Rust (Tokio async)** |
| **Idle Memory** | ~5 MB | ~60–120 MB | **~5 MB** |
| **WASM Runtime** | None | Yes (wasmer/WASI) | **None (Zero Bloat)** |
| **Crash & Reboot Recovery** | External cron scripts (`tmux-resurrect`) | Basic session save | **Built-in Continuous Atomic Snapshots (`mpx restore`)** |
| **Tool Resumption** | Re-runs command name only | None | **Config-driven `[[resume]]` (Claude, omp, nvim, etc.)** |
| **Flicker Protection** | None (tearing on redraw) | Terminal dependent | **DEC 2026 Synchronized Updates** |
| **Quick Navigation** | Clunky `choose-tree` | Search tab | **Instant Fuzzy Command Palette (`Ctrl-Space`)** |
| **Windows Support** | Cygwin / WSL required | WSL required | **Native (Windows Named Pipes + ConPTY)** |
| **Telemetry** | None | None | **100% Offline (Zero Telemetry)** |

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

---

## Survives Crashes and Reboots (`mpx restore`)

The daemon continuously and atomically snapshots your workspace. Every time a session starts (a plain `mpx` after a crash, a reboot or `mpx kill-server`), it brings back:
- All windows, split layouts, and pane proportions.
- Each pane's active working directory (`cwd`).
- Config-driven process resumption for tools running in your panes:
  - Claude Code: resumes with `claude --resume {id}` or `claude --continue`.
  - oh-my-pi: resumes with `omp --resume {id}` or `omp --continue`.
  - Codex / rho / Antigravity / OpenCode: resumes conversation state.
  - Neovim: resumes with `nvim -S Session.vim`.

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
fallback = ["nvim", "-S", "Session.vim"]
```

---

## Documentation

- **[Architecture & Design](docs/ARCHITECTURE.md)**: Tokio async daemon, split tree, dual-buffer engine, and TUI.
- **[CLI Reference & Automation](docs/CLI.md)**: tmux-style commands, waiting, process runs, the tmux shim (what it supports and what it does not), and `mpx import-tmux` to convert a `.tmux.conf`.
- **[Keyboard Shortcuts](docs/KEYBINDINGS.md)**: Prefix bindings, `:` command prompt with vim aliases, and Vi copy mode.
- **[Configuration](docs/CONFIG.md)**: Prefix, key bindings, `remain_on_exit`, and `[[resume]]` rules.
- **[Installation Guide](docs/INSTALL.md)**: Package managers, static archives, and cross-platform builds.
- **[Interactive Website](https://casonadams.github.io/mpx/)**: Online visual demo and live documentation.

---

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
