# Model Context Protocol (MCP) Guide

`mpx mcp` is a zero-dependency stdio MCP server. Any MCP client ([oh-my-pi](https://github.com/can1357/oh-my-pi), [Claude Code](https://claude.ai/code), Cursor, Windsurf, Zed) can run processes, drive interactive programs, coordinate with other agents, and manage agent sessions without scraping a terminal.

---

## Configuration

```json
{
  "mcpServers": {
    "mpx": { "command": "mpx", "args": ["mcp"] }
  }
}
```

The server targets `$MPX_SESSION` / `$MPX_ENDPOINT` when set. Every mpx pane exports them, so an agent started inside mpx talks to its own session automatically. Pin a session explicitly with `"args": ["-s", "build-agent", "mcp"]`.

---

## Tools

Pane arguments accept a node name or a pane id `%N`.

### Processes and panes

| Tool | Parameters | Description |
| :--- | :--- | :--- |
| `mpx_run` | `command`, `args`, `name`, `pty`, `cwd`, `env` (`K=V` list), `remain` | Start a command in a new node. |
| `mpx_list` | | Nodes with pid, type, status, exit code, cwd, title, foreground process and agent state. |
| `mpx_status` | `name` | One node's status. |
| `mpx_read` | `name`, `tail`, `plain`, `since` | Output as `{text, next}`. Pass `next` back as `since` to read only new output. `plain` strips escape sequences. |
| `mpx_screen` | `name` | Rendered terminal screen as text. |
| `mpx_kill` | `name` | Terminate a node. |
| `mpx_split` | `command`, `args`, `name`, `direction` (`h`/`v`), `percent`, `target`, `pty`, `focus`, `cwd` | Split a pane in the user's workspace (focus defaults to false). |
| `mpx_new_window` | `command`, `args`, `name`, `pty`, `focus`, `cwd` | Open a window. |
| `mpx_focus` | `name` | Move the user's focus to a pane. |
| `mpx_sessions` | | Reachable sessions with window and pane counts. |

### Input and waiting

| Tool | Parameters | Description |
| :--- | :--- | :--- |
| `mpx_send` | `name`, `data` | Raw bytes to stdin (newline appended). |
| `mpx_send_keys` | `name`, `keys`, `literal` | tmux key names (`Enter`, `C-c`, `S-Tab`, `F5`, `Up`) or literal text. |
| `mpx_paste` | `name`, `text`, `submit` | Paste (bracketed when the program enabled it) and optionally press Enter. |
| `mpx_wait` | `name`, `condition` (`exit`/`idle`/`match`/`state`), `quiet_ms`, `pattern`, `state`, `timeout` | Wait for exit (returns the code), quiet output, a regex match on screen or new output, or an agent state. Returns `satisfied: false` on timeout. |

### Agent-to-agent messaging

| Tool | Parameters | Description |
| :--- | :--- | :--- |
| `mpx_pipe_write` | `name`, `data` | Append a message; returns its sequence number. Creates the pipe. |
| `mpx_pipe_read` | `name`, `since`, `consumer`, `wait_ms`, `limit` | Read messages. A `consumer` gets a daemon-side cursor (each message once). `wait_ms` long-polls. Readers may start before any writer. |
| `mpx_pipe_create` | `name`, `capacity` | Create with a custom byte capacity (optional). |
| `mpx_pipe_list` | | Pipes with message counts and next sequence numbers. |

### Agent sessions

| Tool | Parameters | Description |
| :--- | :--- | :--- |
| `mpx_agents` | | Live agents across all sessions (harness, state, session id, detail) plus resumable stored sessions. |
| `mpx_agent_launch` | `harness`, `resume`, `cwd`, `split`, `teams`, `name` | Start `claude`, `omp`, `codex`, or a configured harness, optionally resuming a session. |
| `mpx_agent_prompt` | `name`, `text`, `wait` (default true), `timeout` (default 120 s) | Paste and submit a prompt to an agent pane, wait until the agent settles again (idle, needs input or exited, after the prompt; a slow start does not count as done), and return `{text, state, satisfied}`. `satisfied` is false on timeout. |

---

## Workflow patterns

### Run and await a build
```json
mpx_run(name="check", command="cargo", args=["check"])
mpx_wait(name="check", condition="exit", timeout=120)      // {"exit_code": 0, "satisfied": true}
mpx_read(name="check", tail=50)
```

### Drive an interactive program
```json
mpx_run(name="repl", command="python3", pty=true)
mpx_paste(name="repl", text="print(6*7)", submit=true)
mpx_wait(name="repl", condition="match", pattern="^42$", timeout=5)
mpx_send_keys(name="repl", keys=["C-d"])
```

### Delegate to another agent
```json
mpx_agent_launch(harness="claude", name="reviewer", split=true)
mpx_agent_prompt(name="reviewer", text="Review the diff in the working tree and list risks.")
// returns when the reviewer is idle or asks for permission; read state via mpx_status
```

### Coordinate through a pipe
```json
// worker
mpx_pipe_write(name="bus", data="{\"status\":\"tests_passed\",\"commit\":\"abc\"}")
// supervisor: blocks up to 60s, receives each message exactly once
mpx_pipe_read(name="bus", consumer="supervisor", wait_ms=60000)
```
