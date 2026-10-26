# mpx (Multi-PlemeXer)

A simple, ultra-performant process and stream multiplexer in Rust.

Designed from first principles for human developers collaborating side-by-side with autonomous AI agents (`oh-my-pi`, `claude code`).

## Key Highlights

- **Unified Stream Nodes**: Multiplex interactive PTYs, raw pipe task runners, and agent message channels under a single engine.
- **Cross-Platform Native IPC**:
  - Unix Domain Sockets on macOS & Linux (`/tmp/mpx-<uid>/<session>.sock` or `$XDG_RUNTIME_DIR`)
  - Named Pipes on Windows (`\\.\pipe\mpx-<session>`)
- **Zero-Scraping Agent Protocol**: Standard line-delimited JSON-RPC 2.0 / NDJSON over the local socket. Subscribes to token-by-token output and explicit exit code notifications without terminal screen scraping or sentinel token hacks.
- **Dual Buffer Engine**: Clean raw stream logs preserved alongside terminal state.
- **Fast & Minimal**: Zero WASM runtime overhead, no crypto/web tunneling bloat, compiles in seconds.

## CLI Usage

```bash
# Start or attach to a session (defaults to "main", exactly like typing `tmux`)
mpx

# Start or attach to a specific named session
mpx -s dev

# Run a background task in a named stream node
mpx run --name "test-watcher" -- cargo test

# Run an interactive command inside a managed PTY
mpx run --pty --name "repl" -- python3

# List all active nodes in the session
mpx list

# Inspect buffered log output
mpx logs test-watcher

# Follow live output stream
mpx logs -f test-watcher

# Send input to a running node's stdin
mpx send repl "print('hello from mpx')"

# Await completion and exit with child status code
mpx wait test-watcher

# Terminate a node
mpx kill test-watcher

# Split active pane horizontally or vertically
mpx split --direction h --name "worker-2" -- cargo watch

# Inspect 2D virtual terminal screen buffer
mpx screen repl

# View workspace pane layout geometry
mpx layout

# Attach interactive full-screen TUI
mpx attach

### TUI Keyboard Shortcuts & Mouse Mode (tmux Parity)

Supports both **`Ctrl-B`** (default tmux) and **`Ctrl-A`** (popular rebind) as prefix keys.

#### Windows (Tabs)
- **`Prefix + c`**: Create new window tab running default shell
- **`Prefix + n`**: Next window tab
- **`Prefix + p`**: Previous window tab
- **`Prefix + 0..9`**: Select window tab by index

#### Panes (Splits)
- **`Prefix + %`** / **`Prefix + |`**: Split window vertically (side-by-side, vertical divider `│`)
- **`Prefix + "`** / **`Prefix + -`**: Split window horizontally (stacked, horizontal divider `─`)
- **`Prefix + o`** / **`Tab`**: Cycle focus to next pane
- **`Prefix + h/j/k/l`** or **Arrow Keys**: Directional pane focus (left / down / up / right)
- **`Prefix + z`**: Toggle zoom (maximize active pane to 100% full screen, press again to un-zoom)
- **`Prefix + x`**: Kill active pane
- **`Prefix + d`**: Detach cleanly from session

#### Mouse Mode
- **Click**: Focus clicked pane
- **Scroll Up**: Enter copy mode and scroll history upwards
- **Scroll Down**: Scroll down in copy mode

#### Copy Mode (Vi-Keys)
- **`Prefix + [`**: Enter copy mode
- **`j` / `k`** (or `Down` / `Up`): Scroll down / up by 1 line
- **`Ctrl+D` / `Ctrl+U`** (or `PageDown` / `PageUp`): Scroll half page down / up
- **`g` / `G`**: Jump to top / bottom of scrollback history
- **`y` / `Enter`**: Copy text buffer to system clipboard (`pbcopy` / OSC 52) and exit copy mode
- **`q` / `Esc`**: Exit copy mode
```

### Inter-Agent Virtual Pipes

```bash
# Create a named virtual pipe channel for cross-agent communication
mpx pipe create agent_bus

# Write structured data or instructions into the pipe
mpx pipe write agent_bus '{"task": "review_pr", "pr": 42}'

# Read data from the pipe
mpx pipe read agent_bus

# List active virtual pipes
mpx pipe list
```

## Model Context Protocol (MCP) Server

`mpx` features a built-in, zero-dependency MCP stdio server. Any AI coding agent (`oh-my-pi`, `claude code`, Cursor, Windsurf, Zed) can configure `mpx` directly in its `.mcp.json`:

```json
{
  "mcpServers": {
    "mpx": {
      "command": "mpx",
      "args": ["mcp"]
    }
  }
}
```

### Exposed MCP Tools

- **`mpx_run`**: Spawn a command in a managed stream node.
- **`mpx_split`**: Split a workspace pane and run a command.
- **`mpx_list`**: List all active stream nodes, their PIDs, types, and statuses.
- **`mpx_read`**: Read scrollback log buffer from a stream node.
- **`mpx_screen`**: Snapshot the 2D virtual terminal screen buffer.
- **`mpx_send`**: Send keystrokes / text to a node's stdin.
- **`mpx_wait`**: Wait for command execution and retrieve exit status.
- **`mpx_kill`**: Terminate a running node.
- **`mpx_pipe_write`**: Write structured messages into an inter-agent virtual pipe.
- **`mpx_pipe_read`**: Read messages from an inter-agent virtual pipe.

## JSON-RPC 2.0 / NDJSON Socket API

Agents can connect directly to `/tmp/mpx-<uid>/<session>.sock` (or Windows Named Pipe) and send single-line JSON requests:

```json
{"method":"Spawn","params":{"name":"agent-job","command":"cargo","args":["check"],"pty":false,"cwd":null}}
{"method":"Subscribe","params":{"name":"agent-job"}}
{"method":"Wait","params":{"name":"agent-job"}}
```

The daemon streams real-time events as they occur:
```json
{"event":"Output","data":{"name":"agent-job","data":"Compiling...\n"}}
{"event":"Exit","data":{"name":"agent-job","code":0}}
```

## License

Apache-2.0
