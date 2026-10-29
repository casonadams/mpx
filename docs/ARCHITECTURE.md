# Architecture & Design

`mpx` is a Rust / Tokio daemon per session, plus thin clients: the TUI, the CLI, the stdio MCP server, and the `tmux` shim. All of them speak the same newline-delimited JSON protocol (`src/protocol.rs`) over a Unix domain socket or Windows Named Pipe. One exception carries the bulk of the traffic: a subscriber that asks for it (`Subscribe { binary: true }`, which the TUI does) and a daemon that answers `BinaryStream` switch that connection to length-prefixed binary frames (`src/wire.rs`), where pane output is raw bytes instead of escaped JSON; other events stay JSON inside their frame. Either side may be an older mpx: a daemon that ignores the field answers `Ok` and streams JSON lines, so mixed versions keep working. Agents never see this: MCP and the CLI keep their JSON interfaces.

Inside the daemon, state is owned by **actors**: tokio tasks that each own their data outright and are reached only through messages. Nothing on the request or output path is shared behind a lock. Every mpx process runs its tasks on a single thread: actors pass messages constantly, and across a worker pool each one was a cross-thread wakeup, which cost a flood 40% more CPU. Work that takes milliseconds (replaying a pane's history for a plain read) runs off that thread.

```mermaid
graph LR
C[Clients: TUI / CLI / MCP / tmux shim / hooks] --> S[Session actor]
S --> P1[Pane actor %0]
S --> P2[Pane actor %1]
S --> Q[Pipes actor]
S --> SN[(snapshot.json)]
Q --> PL[(pipe logs)]
P1 -->|numbered events| C
```

---

## 1. Actors

- **Session actor** owns:
  - the workspace (windows, split trees, focus history) and the pane registry (name and `%N`);
  - exited-pane history and the idempotency cache;
  - the session snapshot.

  Pane exits arrive on the same mailbox as client requests, so every request sees a consistent layout. Requests that can take a while are not awaited there: log reads, waits, pipe reads and kills resolve their pane through the session and then talk to it directly, since every pane's output passes through the session on its way to clients and would queue behind a slow request.
- **Pane actor**, one per pane, owns its process handle, output ring buffer, VT screen, agent state machine, subscribers and pending waits. The blocking PTY reader thread only forwards bytes to it.
- **Pipes actor** owns the virtual pipes, their consumer cursors and the on-disk pipe logs. Pending timed reads are answered by the write that satisfies them or by their deadline.

Every wait is resolved inside the actor that owns the relevant state, with no polling:
- process exit;
- quiet output (a timer reset by output);
- a regex over the screen plus new output;
- an agent state;
- a pipe message.

## 2. Panes

Every pane has a stable id (`%N`):

1. **PTY** (`--pty`): a full terminal (POSIX PTY or ConPTY) mirrored into a VT grid. The grid tracks the program's input modes (cursor keys, bracketed paste, focus events), window title (OSC 0/2) and alerts (BEL, OSC 9/777).
2. **Task**: a child process with captured stdout/stderr and no terminal emulation.

Each pane gets `MPX_ENDPOINT`, `MPX_SESSION`, `MPX_PANE` and `MPX_PANE_ID` in its environment, so programs inside it (agents, scripts, `mpx` itself) address their own session. With tmux compatibility, `TMUX`, `TMUX_PANE` and the shim directory on `PATH` are added.

A pane's exit is reported only after its output has been fully read, so `wait` on exit never races the last bytes a short-lived command printed.

## 3. Event stream

Output is kept in a ring buffer addressed by **absolute byte offsets** (the last 2 MB per pane). It is the one source of scrollback: live screens, in the daemon and in the TUI, keep only what is on screen, so a flood costs no history work. Copy mode and plain log reads rebuild scrollback on demand by replaying the ring into a fresh screen at the pane's size. Every `Output` event carries `offset` and `next`, the range of the stream it covers. Multi-byte characters split across reads are held back until complete, so each chunk is valid text and consecutive chunks have contiguous offsets.

- `Subscribe { name, since }`: the pane actor first replays everything from `since`, then switches to live output, atomically. `logs -f` uses this, so history and live output join without a gap or duplicate.
- **Slow subscribers**: each one has a bounded queue. A subscriber that falls behind gets a single `Lagged` event instead of a silent drop. Clients then re-read what they missed by offset: the TUI rebuilds affected panes, and `logs -f` re-reads from its last offset.
- **Pacing** (`src/node/pacing.rs`): output flows at the speed it can be shown, as in a real terminal, so a flood never queues ahead of a keystroke's echo. A pane's reader holds at most a bounded amount of output its actor has not parsed, so requests to a flooding pane (input, Ctrl-C, kill) are answered at once. While an attached TUI is more than a few dozen events behind, PTY readers wait for it to catch up; the TUI reads its stream through a small bounded queue so that it falls behind visibly rather than buffering. A TUI that makes no progress for a second, or detaches, no longer holds any pane, so a stopped client never freezes programs.
- **Only what is on screen**: the TUI subscribes with `active_window`, so of pane output it receives only the active window's; exits, agent and other events still come for every pane. A pane coming into view asks for its log's end offset: nothing new keeps its screen, a gap the log still holds is fetched and fed, and a larger one rebuilds the pane from its log. Busy panes in other windows cost the TUI nothing.
- **Agent events**: every agent state change is pushed as an `Agent` event, so the TUI's waiting badge and notifications update immediately.
- **Layout events**: a subscriber that asks for them (`Subscribe { layout: true }`, as the TUI does) gets a `Layout` event whenever windows or panes are rearranged by anyone (a focus change, window switch, split, swap or resize from the CLI, MCP or another client), so it redraws at once. Others never receive it, and clients skip binary frames they cannot decode, so mixed versions keep working.

## 4. Agent tracking

For each pane, the daemon reads the PTY foreground process (comm and argv) to recognize agent harnesses. It keeps a small state machine (`src/node/agent.rs`):

- Hook reports (`mpx hook`) are authoritative: session id, working / idle / needs input, detail text.
- Terminal alerts move an agent to *needs input*; any input to the pane moves it back to *working*.
- Without hooks, output recency decides *working* versus *idle*. A timer runs only while such an agent is working.

## 5. Workspace

Windows hold binary split trees with ratios. The daemon implements focus history (last pane, last window), swap, break, resize (tmux border semantics), layout presets (even-horizontal, even-vertical, main-vertical, main-horizontal, tiled), sticky window names (set by a rename or given at creation, as with tmux `new-window -n`), and automatic window names from the focused pane's agent or foreground process for windows without one. With `remain_on_exit`, an exited pane stays in the layout with its final screen.

## 6. Durability

State that should outlive the daemon lives in the state directory (`$XDG_STATE_HOME/mpx/<session>/`, see [CONFIG.md](CONFIG.md#state-directory)). The runtime directory holds only the socket.

- **Session snapshot** (`snapshot.json`): windows, layout trees, focus, sticky names, and each running pane's command, live working directory, user environment and agent session id. It is saved atomically (temp file + rename), debounced, after every relevant change, and marked clean only by `kill-server`.
  - If the daemon starts and finds an unclean snapshot (a crash or reboot), it restores before accepting clients: PTY panes rerun their command in their last directory, agents resume their sessions through their harness, and task panes are skipped.
  - `mpx restore` restores on demand.
- **Durable pipes** (`pipes/`): an append-only message log plus metadata (capacity, next sequence number, consumer cursors) per pipe, reloaded on start. A torn last line from a crash is ignored.
- **Idempotency**: `Idempotent { key, request }` runs a request at most once per key. Concurrent repeats wait for the first result and later repeats get it from a bounded cache (1024 keys, 10 minutes). This makes CLI and MCP retries safe for any orchestrator.

## 7. TUI

A mode state machine (normal, prefix, copy, prompt, confirm, overlay) over a per-pane VT mirror:

- **Keymap**: a tmux-notation key → command-line table; config bindings replace or add keys. Commands share one parser with the `:` prompt.
- **Input**: xterm key encoding with modifiers and DECCKM, bracketed paste, and focus events forwarded according to each pane's modes.
- **Pane sizes**: each visible pane's PTY is sized to its place on screen. The daemon reports every terminal pane's actual size; a pane that gains focus is checked against the size last sent, so a pane resized by another client or `mpx resize-pane` gets its size back and its program reflows. (Only on focus: two clients of different sizes would otherwise take a pane back and forth.) While a split border is dragged, panes are drawn at their new places as they are; their terminals are resized, and their screens rebuilt, once on release.
- **Stream integrity**: per-pane expected offsets. A gap is filled from the pane's log; `Lagged` rebuilds the panes from their logs.
- **Sessions**: the client can switch daemons (`(`, `)`, tree, agent jumps) without restarting.
- **Overlays**: the agent manager, tree, and help, drawn over the panes.
- **Rendering**: the TUI keeps a shadow of what the terminal shows and each frame sends only the cells that changed, inside a DEC 2026 synchronized update with autowrap disabled. Rows holding non-ASCII glyphs are reprinted whole when they change, since the screen model has no width table; cells under an overlay are repainted when it closes, and a resize repaints everything. Pane colors, bold/italic/underline and reverse video pass through exactly (basic and bright palette slots are kept apart, and `NO_COLOR` does not strip a program's own colors). Erasing paints the current background (BCE, as `xterm-256color` promises), so editors can fill bars by erasing to the end of a line.
- **Scrolling**: the screen model records how a pane's rows scrolled since the last frame. The TUI then scrolls those rows on the terminal (DECSTBM + SU/SD) and moves its shadow the same way, so only new lines are sent, as tmux does. A pane narrower than the screen (beside the agents rail or a split) is scrolled through left/right margins (DECLRMM/DECSLRM) when the terminal reports them at startup (DECRQM for mode 69, followed by DA1 so a terminal that does not know the mode costs one round trip), and repainted otherwise. A wrong scroll guess only costs bytes: the cell diff that follows still brings the terminal to the pane's exact contents.

## 8. Cross-platform IPC

| Platform | Transport | Endpoint |
| :--- | :--- | :--- |
| macOS / Linux | Unix Domain Socket | `$XDG_RUNTIME_DIR/mpx/<session>.sock` or `/tmp/mpx-<uid>/<session>.sock` (directory mode 0700) |
| Windows | Named Pipe | `\\.\pipe\mpx-<session>` |

Sessions are discovered by listing that directory (or the pipe namespace). `kill-server` stops the daemon process and removes its socket.
