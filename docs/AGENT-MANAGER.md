# Agent Manager

mpx tracks AI coding agents (Claude Code, oh-my-pi, Codex, rho, Antigravity, OpenCode, or any configured harness) running in its panes. It shows which ones need you, jumps between them across sessions, resumes stored sessions, and launches new ones.

Open it with **`Prefix s`**, or start mpx straight into it with **`mpx agents`**.

```
┌─ Agents · 1 need you ───────────────────────────────────────────────────┐
│◆ NEEDS YOU                                                              │
│ ◆ reviewer        claude   main:%2    Claude needs your permission  12s │
│● WORKING                                                                │
│ ● impl-auth       omp      main:%1    editing src/auth.rs            3m │
│○ IDLE                                                                   │
│ ○ docs            claude   dev:%0                                     8m │
│◌ RESUME · ~/src/mpx                                                     │
│ ◌ Fix VT scroll margins     omp      01a17ff1…  ~/src/mpx             2d │
└─ j/k move  Enter open  r/R resume  n new  m msg  Tab mark  x kill  y id ┘
```

On screens at least 12 rows tall, a preview panel below a five-row scrollable list shows the selected agent's live screen or the tail of a stored transcript.

---

## Keys

| Key | Action |
| :--- | :--- |
| `j` / `k`, `gg` / `G`, `Ctrl-d` / `Ctrl-u` | Move |
| `/` | Fuzzy filter on name, harness, title, cwd and detail (`Enter` keeps it, `Esc` clears it) |
| `Enter` | Live agent: jump to its pane, switching session and window. Stored session: resume it in a new window. |
| `r` / `R` | Resume the stored session in a new window / as a split |
| `n` | New agent: pick a harness, launched in the current pane's directory |
| `m` | Message: type a prompt, which is pasted and submitted to the selected agent or every marked one |
| `Tab` | Mark or unmark for `m` |
| `x` | Kill the agent's pane (asks y/n) |
| `y` | Copy the session id |
| `p` | Toggle the preview |
| `s` | Scope stored sessions to this project's directory or all |
| `q` / `Esc` | Close |

**`Prefix A`** (outside the overlay) jumps to the agent that has waited longest for input, in any session. The status bar shows `◆N` while agents are waiting.

---

## States

| Glyph | State | Meaning |
| :--- | :--- | :--- |
| `◆` | needs input | The agent asked a question or for permission (hook notification, terminal bell, or OSC 9/777) |
| `●` | working | Producing output, or a hook reported a turn in progress |
| `○` | idle | Finished its turn |
| `✕` | exited | The process ended (visible with `remain_on_exit`) |
| `◌` | resumable | A stored session that is not running |

Typing into an agent that needs input moves it back to working.

### How state is detected

1. **Hooks** (authoritative). Agents launched with `mpx agents new` / `Prefix s n` get hooks automatically:
   - **Claude Code** receives `--settings` with hooks for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `Notification`, `Stop` and `SessionEnd`. Each runs `mpx hook claude`. mpx also pre-assigns the session id with `--session-id`, so pane and session are linked exactly.
   - **oh-my-pi** receives an extension via `--hook=<file>` that reports session start (with the session id), each prompt, run start/end with the final reply, file writes and edits, tool approval requests and resolutions, and shutdown through `mpx hook omp <event>`. Approval prompts appear as *needs input* with the tool name as detail.
   - For Claude Code started some other way inside an mpx pane, install the hooks once: `mpx hook print-config claude` prints the `settings.json` block. The hook is silent and exits 0 outside mpx.
2. **Terminal signals.** A bell or an OSC 9/777 notification from an agent pane means it needs input.
3. **Output activity.** Without hooks, an agent is working while it produces output and idle after `agent.idle_ms` (default 1500 ms) of quiet.

Agents are recognized by the foreground process of their pane (`claude`, `omp`, `codex`, `rho`, `antigravity`, `agy`, `opencode`, or a configured name), including Claude Code's versioned binary and scripts run by interpreters.

---

## Stored sessions

| Harness | Store | Resume |
| :--- | :--- | :--- |
| Claude Code | `~/.claude/projects/<cwd>/<uuid>.jsonl` | `claude --resume <id>` |
| oh-my-pi | `~/.omp/agent/sessions/<cwd>/<ts>_<id>.jsonl` | `omp --resume <id>` |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | `codex resume <id>` |
| rho | `~/.config/rho/sessions/<id>.jsonl` | `rho --resume <id>` |
| antigravity | - | `agy --conversation <id>` |
| opencode | `~/.local/share/opencode/storage/session/<project>/<id>.json` | `opencode --session <id>` |

Only the head of each file is read for the title, id and working directory, so listing stays fast.

---

## Command line and MCP

```bash
mpx agents --json                         # {"live": [...], "resumable": [...]}
mpx agents new claude --split             # launch beside the current pane
mpx agents new claude --teams             # Claude Code agent teams; teammates become mpx panes
mpx agents resume 01a17ff1                # resume by id prefix from any store
mpx wait reviewer --state needs_input     # block until the agent asks for something
mpx paste -t reviewer --submit "yes, go ahead"
```

MCP clients get the same capabilities through `mpx_agents`, `mpx_agent_launch`, `mpx_agent_prompt` and `mpx_peers` (see [MCP.md](MCP.md)), so one agent can supervise or coordinate with others.

---

## Agent awareness

Agents mpx launches can see each other. Claude Code's hooks and the oh-my-pi extension report to mpx what each agent works on: its latest prompt (`task`), the reply it ended its last turn with, and the files it edited in the last 30 minutes. Other agents in the same repository (the nearest directory with `.git`, in any session) read that back two ways:

- **Told, sparingly.** When an agent starts or gets a prompt and the set of agents in its repository has changed, it gets one short list of them with their tasks. When it reads or edits a file another agent edited recently, it is told who and why, once per file and agent. Nothing is repeated.
- **On demand.** `mpx_peers` lists the agents in the repository with their task, last reply and recent edits, and `mpx_agent_prompt` with a `session:pane` name asks one of them something, for example how to split work so their edits do not collide.

Launched agents get the mpx MCP tools for this automatically (`[agent] mcp`): Claude Code through `--mcp-config`, oh-my-pi through a small plugin passed with `--plugin-dir`. Paths are compared with symlinks resolved, so agents that name a file differently (`/var/…` and `/private/var/…` on macOS) still match. Hooks never block an agent: when a session is slow or unreachable, the hook says nothing. Agents launched before an mpx upgrade pick this up when relaunched.

## Adding a harness

```toml
[[harness]]
name = "aider"
detect = ["aider"]                  # foreground process names
command = ["aider"]                 # fresh session
resume = []                         # argv with {id}; empty means not resumable
session_id_args = []                # argv appended to pre-assign an id ({id})
```

A `[[harness]]` entry with a built-in name (`claude`, `omp`, `codex`, `rho`, `antigravity`, `opencode`) replaces that harness's launch settings. See [CONFIG.md](CONFIG.md).
