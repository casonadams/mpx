# CLI Reference & Automation

`mpx` provides a uniform CLI for both human terminal usage and scripted automation.

```
mpx [OPTIONS] [COMMAND]
```

### Global Options

- `-s, --session <NAME>`: Target session name (defaults to `main`).
- `-e, --endpoint <PATH>`: Custom IPC socket path or Named Pipe.
- `-h, --help`: Print help.
- `-V, --version`: Print version.

---

## Commands

### `mpx` or `mpx attach`
Starts the session daemon if not already running, and attaches the full-screen interactive TUI.
```bash
# Attach to default session "main"
mpx

# Attach to named session "build-agent"
mpx -s build-agent
```

---

### `mpx run`
Runs a persistent command in a managed stream node.

```bash
mpx run [OPTIONS] -- <COMMAND> [ARGS...]
```

**Options**:
- `--name <NAME>`: Custom identifier for the node (defaults to executable name).
- `--pty`: Allocates a full pseudo-terminal (ConPTY on Windows, POSIX PTY on Unix).
- `--cwd <DIR>`: Working directory for process execution.

**Examples**:
```bash
# Run a background test watcher
mpx run --name "watcher" -- cargo watch -x test

# Launch an interactive REPL inside a managed PTY
mpx run --pty --name "python-repl" -- python3

# Run a dev server in a specific working directory
mpx run --cwd ./apps/web --name "vite" -- npm run dev
```

---

### `mpx list`
Lists all active and recently exited stream nodes in the current session.

```bash
mpx list
```

**Sample Output**:
```
NAME             PID      TYPE     STATUS         COMMAND                       
--------------------------------------------------------------------------------
watcher          41892    task     running        cargo watch -x test           
python-repl      41910    pty      running        python3                       
vite             41925    task     exited (0)     npm run dev                   
```

---

### `mpx status`
Inspects non-blocking status for a specific node without scraping terminal text.

```bash
mpx status watcher
```

---

### `mpx logs`
Reads stream logs from a node's circular buffer.

```bash
# Read recent logs
mpx logs watcher

# Read specific number of tail lines (O(tail) fast reverse scan)
mpx logs --tail 50 watcher

# Stream live output (like `tail -f` or `docker logs -f`)
mpx logs -f watcher
```

---

### `mpx send`
Writes raw data directly to a running node's standard input.

```bash
mpx send python-repl "import sys; print(sys.version)\n"
```

---

### `mpx wait`
Waits for a node to complete execution and exits with its return code.

```bash
mpx wait watcher
echo "Watcher exited with status: $?"

# Wait with a timeout in seconds
mpx wait --timeout 30 build-step
```

---

### `mpx split`
Splits an existing pane horizontally or vertically without entering the TUI.

```bash
mpx split [OPTIONS] -- <COMMAND> [ARGS...]
```

**Options**:
- `-d, --direction <h|v>`: Split direction (`h` for vertical split/side-by-side, `v` for horizontal split/stacked).
- `--target <NAME>`: Target pane to split next to.
- `--detach`: Keeps focus on current pane without switching focus to new split.
- `--pty`: Allocates a managed PTY for interactive tools.

**Examples**:
```bash
# Split active window horizontally and run htop
mpx split --pty --direction h -- htop

# Split next to "watcher" without stealing focus
mpx split --detach --target watcher --name linter -- cargo clippy
```

---

### `mpx focus`
Shifts workspace focus directly to a named pane.

```bash
mpx focus watcher
```

---

### `mpx screen`
Takes an instantaneous snapshot of a node's 2D virtual terminal grid.

```bash
mpx screen python-repl
```

---

### `mpx layout`
Computes and prints current workspace split tree geometry, divider lines, and window tabs.

```bash
mpx layout --width 120 --height 40
```

---

### `mpx pipe`
Manages inter-agent virtual FIFO pipes.

```bash
# Create a virtual pipe
mpx pipe create bus

# Write to virtual pipe
mpx pipe write bus '{"event": "pr_analyzed", "findings": 0}'

# Read data from virtual pipe
mpx pipe read bus

# List all virtual pipes
mpx pipe list
```

---

### `mpx stop`
Terminates all nodes in a session and stops the background daemon process.

```bash
mpx stop
mpx -s dev stop
```
