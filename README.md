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

### TUI Keyboard Shortcuts

- **`Ctrl+A |`** or **`Ctrl+A v`**: Split pane vertically
- **`Ctrl+A -`** or **`Ctrl+A h`**: Split pane horizontally
- **`Ctrl+A o`** / **`Tab`**: Switch focus to next pane
- **`Ctrl+A p`**: Switch focus to previous pane
- **`Ctrl+A x`**: Kill focused pane
- **`Ctrl+A d`**: Detach cleanly from TUI (leaves daemon running in background)
```

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
