# Benchmarks: mpx vs tmux

`bench/run.sh` measures `mpx` against `tmux` on the same machine. Every cell is the median of 5 repetitions, with min–max in parentheses. Lower is better everywhere. The ratio column is mpx median ÷ tmux median.

## Machine

| Item | Value |
| :--- | :--- |
| Date | 2026-10-30T20:15:34Z |
| Hardware | `uname -m`: arm64 (Apple Silicon) |
| OS | macOS 26.5.2 (25F84); Darwin 25.5.0 |
| tmux | 3.8 (`/opt/homebrew/bin/tmux`) |
| mpx | `mpx 0.2.0`, release build of the working tree at git `7bbfb55` |

## Results

| Metric | mpx | tmux | mpx / tmux |
| :--- | :--- | :--- | :--- |
| Flood 100 MB, client attached (time to pane exit) | 28.12 s (27.77–29.20) | 21.64 s (21.49–22.59) | 1.30× |
| Flood 100 MB, detached (time to pane exit) | 27.66 s (27.50–29.63) | 21.99 s (21.79–25.05) | 1.26× |
| Daemon RSS idle, 1 pane | 5.2 MB (5.2–5.3) | 3.8 MB (3.8–3.8) | 1.37× |
| Daemon RSS idle, 20 panes | 8.3 MB (8.2–8.3) | 3.9 MB (3.9–3.9) | 2.11× |
| Startup: launch to first status-bar bytes (cold server) | 2,116.1 ms (2,115.2–2,118.0) | 19.6 ms (17.7–19.8) | 107.78× |
| 200 sequential `list-panes`, total | 480.1 ms (475.1–504.7) | 695.0 ms (671.9–702.2) | 0.69× |
| `list-panes` per-call median | 2.31 ms (2.30–2.40) | 3.30 ms (3.25–3.39) | 0.70× |
| Create 50 panes via `new-window -d` | 204.7 ms (199.5–214.0) | 228.0 ms (220.3–228.6) | 0.90× |

## Reading

- **Flood throughput: mpx is slower.** It takes about 30% longer to push 100 MB through a pane, roughly 3.6 MB/s against 4.6 MB/s for tmux. The ranges do not overlap. Attached and detached runs are nearly identical for both, so client rendering does not appear to be the main cost [INFERENCE: not profiled]. The shell pipeline and PTY throughput are a likely shared ceiling that compresses the ratio; I did not measure a raw-pipe baseline.
- **Idle memory: mpx is larger, and the gap grows with pane count.** The mpx daemon starts at about 5 MB and adds about 0.16 MB per idle pane. tmux stays near 4 MB with 20 panes.
- **Cold startup: mpx is much slower, about 2.1 s against 20 ms.** This is the largest gap in the table. The measurement includes spawning the daemon and waiting for it to answer. The source polls every 100 ms until the daemon answers. I did not split the 2.1 s into daemon spawn, readiness, and first render, so the cause is not isolated.
- **`list-panes`: mpx is faster**, about 2.3 ms per call against 3.3 ms. Both figures include the CLI round trip to the server.
- **Pane creation: mpx is slightly faster**, 205 ms against 228 ms for 50 panes. The ranges do not overlap, but this is one machine and five repetitions.

## Method

Each repetition runs in a fresh sandbox under `/tmp`, with its own `HOME`, `XDG_RUNTIME_DIR`, `MPX_STATE_DIR`, and `TMUX_TMPDIR`. The child environment is built from scratch, with `SHELL=/bin/sh` and no user rc files. Both tools run with default config (`MPX_CONFIG=/dev/null`, `tmux -f /dev/null`). Each call names its session and socket explicitly, so the user's own sessions are never addressed.

- **Flood** (`yes | head -c 100000000` in a PTY pane). Attached: an interactive client is on a 120×40 PTY and drained continuously, and the pane is opened as a new visible window. Detached: no client. mpx waits with `mpx wait` (exit status). tmux waits on a `wait-for` signal sent by the pane after the pipeline ends. Panes use a PTY for both tools.
- **Idle RSS**: `ps -o rss=` on the daemon pid, after a 1 s settle. One shell pane, then 19 more running `sleep 600`. Pane children and attached clients are not counted.
- **Startup**: a cold server. Time from spawning `mpx -s bench` or `tmux new -s bench` on a PTY until the status-bar text `[bench]` appears.
- **Command latency**: 200 sequential `list-panes` calls on a server with one pane. Reported as the total and the median of the per-call times.
- **Spawn**: 50 sequential `new-window -d` calls running `sleep 600`, on a server that already has one pane.

## Caveats

- Single machine, five repetitions per cell. Treat differences under about 10% as noise.
- The mpx binary was built from the working tree at `7bbfb55`, which may include uncommitted changes.
- Flood timing covers the whole `yes | head` pipeline, not the pane's own throughput alone.
- `split-window` is not benchmarked. Splitting 50 times in a fixed terminal produces unusable panes, so spawn uses `new-window`.
- mpx panes are created with `--pty` in these benchmarks to match tmux, which always uses a PTY.
- Idle RSS measures only the daemon. Add the pane children if you need the whole footprint.
- The tmux-absent path (skipped columns, exit 0) and the failure path (non-zero exit) were checked separately from the full run.

## Rerunning

Requirements: `python3`, and tmux if you want its columns. `bench/run.sh` builds `target/release/mpx` when it is missing.

```sh
bench/run.sh                          # full suite, about 10 minutes here
bench/run.sh --only latency,spawn     # a subset: flood, idle, startup, latency, spawn
BENCH_REPS=7 bench/run.sh             # more repetitions (minimum 5)
MPX_BIN=... TMUX_BIN= bench/run.sh    # alternate mpx binary; empty TMUX_BIN skips tmux
BENCH_OUT=/path/to/dir bench/run.sh   # keep results.json and results.md
```

The script exits non-zero if any step fails. Raw samples are in `results.json` and the rendered table in `results.md`, both under the output directory it prints on start.
