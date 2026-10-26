# Model Context Protocol (MCP) Guide

`mpx` features a built-in, zero-dependency stdio Model Context Protocol (MCP) server. Any AI coding agent—including [oh-my-pi](https://github.com/can1357/oh-my-pi), [Claude Code](https://claude.ai/code), Cursor, Windsurf, or Zed—can supervise background processes, run interactive tools, and stream logs directly over stdio without terminal scraping.

---

## Configuration

Add `mpx` to your agent's `.mcp.json` or client configuration:

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

Or target a specific isolated session:

```json
{
  "mcpServers": {
    "mpx-build": {
      "command": "mpx",
      "args": ["-s", "build-agent", "mcp"]
    }
  }
}
```

---

## Available Tools

| Tool | Parameters | Description |
| :--- | :--- | :--- |
| `mpx_run` | `command`, `args`, `name`, `pty`, `cwd` | Launches a persistent command in a managed stream node. Returns PID. |
| `mpx_list` | *(none)* | Lists all active and exited stream nodes, PIDs, statuses, and exit codes. |
| `mpx_status` | `name` | Returns non-blocking status for a specific node without waiting. |
| `mpx_read` | `name`, `tail` | Reads clean, unescaped raw log output from a node's circular buffer. |
| `mpx_write` | `name`, `data` | Writes input directly into a running node's standard input. |
| `mpx_wait` | `name`, `timeout` | Waits for a process to finish and returns its numeric exit code. |
| `mpx_kill` | `name` | Gracefully terminates a running process and purges the node. |
| `mpx_split` | `command`, `direction`, `name`, `detach` | Splits workspace window into a new pane. |
| `mpx_focus` | `name` | Shifts workspace focus to a target pane. |
| `mpx_screen` | `name` | Captures an instantaneous 2D virtual terminal text snapshot. |
| `mpx_pipe_write` | `name`, `data` | Writes structured messages into an in-memory virtual FIFO pipe. |
| `mpx_pipe_read` | `name` | Reads pending messages from a virtual FIFO pipe. |

---

## Agent Workflow Patterns

### 1. Reliable Task Execution & Exit Detection

Traditional multiplexers require agents to inject sentinel markers (e.g. `echo __EXIT_1234__`) and sleep-poll `capture-pane`. In `mpx`, execution is deterministic:

```json
// Step 1: Spawn task
mpx_run(name="compiler", command="cargo", args=["check"])

// Step 2: Await completion with explicit exit code
mpx_wait(name="compiler", timeout=30)
// Returns: {"code": 0}

// Step 3: Read unpolluted output
mpx_read(name="compiler", tail=100)
```

### 2. Side-by-Side Human & Agent Observation

An autonomous agent can split a pane in the developer's live workspace:

```json
mpx_split(direction="h", name="reviewer", command="claude", args=["--review"], detach=true)
```

The split appears immediately on the human developer's screen with full 60 FPS rendering, while the agent continues inspecting outputs programmatically in the background.

### 3. Cross-Agent Message Passing (Virtual Pipes)

Multiple sub-agents can coordinate without touching disk files:

```json
mpx_pipe_write(name="agent_bus", data="{\"status\": \"tests_passed\", \"commit\": \"abc\"}")
```
