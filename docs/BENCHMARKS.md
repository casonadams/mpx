# Benchmarks: mpx vs tmux

`bench/run.sh` measures `mpx` against `tmux` on the same machine. Every cell is the median of 5 repetitions, with min–max in parentheses. Lower is better everywhere. The ratio column is mpx median ÷ tmux median.

## Harness correction

The first run of this suite reported mpx cold startup at 2,116 ms. That number measured the harness, not mpx. On attach, mpx sends terminal queries (OSC 10/11 colour, DECRQM for mode 69, DA1) and waits up to 2 s for the DA1 reply. The PTY harness never answered, so every mpx attach sat in that timeout.

`bench/ptyclient.py` now answers those queries as a real terminal does. The startup figure dropped from 2,116 ms to 112 ms. The attached flood figures were rerun on the corrected harness. The other metrics are unaffected by the change, and the full table below is from the corrected run.

## Machine

| Item | Value |
| :--- | :--- |
| Date | 2026-10-30T20:36:44Z (suite), 2026-10-30T20:51:59Z (flood overhead floor) |
| Hardware | `uname -m`: arm64 (Apple Silicon) |
| OS | macOS 26.5.2 (25F84); Darwin 25.5.0 |
| tmux | 3.8 (`/opt/homebrew/bin/tmux`) |
| mpx | `mpx 0.2.0`, release build of the working tree at git `7bbfb55` |

## Results

| Metric | mpx | tmux | mpx / tmux |
| :--- | :--- | :--- | :--- |
| Flood 100 MB, client attached (time to pane exit) | 28.62 s (27.95–31.85) | 21.97 s (21.91–25.75) | 1.30× |
| Flood 100 MB, detached (time to pane exit) | 28.12 s (27.70–28.90) | 21.82 s (21.68–24.58) | 1.29× |
| Daemon RSS idle, 1 pane | 5.2 MB (5.2–5.2) | 3.8 MB (3.8–3.8) | 1.36× |
| Daemon RSS idle, 20 panes | 8.3 MB (8.3–8.3) | 3.9 MB (3.9–3.9) | 2.13× |
| Startup: launch to first status-bar bytes (cold server) | 111.7 ms (110.4–113.3) | 15.6 ms (14.2–16.8) | 7.15× |
| 200 sequential `list-panes`, total | 499.7 ms (489.1–578.7) | 713.0 ms (673.0–823.8) | 0.70× |
| `list-panes` per-call median | 2.41 ms (2.37–2.76) | 3.45 ms (3.29–3.97) | 0.70× |
| Create 50 panes via `new-window -d` | 208.5 ms (202.1–234.5) | 223.6 ms (217.9–246.5) | 0.93× |

## Flood floors

The floor was meant to show how much time the multiplexer adds to the flood. On this machine it cannot do that, for the reason below.

| Run | Time (median, min–max) |
| :--- | :--- |
| `yes \| head -c 100000000 > /dev/null`, no PTY | 0.02 s (0.02–0.04) |
| Same flood in a PTY, output drained by the Python harness | 30.53 s (28.51–36.19) |

The `/dev/null` floor skips the PTY and terminal output entirely. A plain shell pipeline moves 100 MB in about 40 ms, so it is not a PTY floor and does not bound the flood.

The drained PTY floor is the only PTY-based reference. It is no faster than either multiplexer: mpx attached 28.62 s, mpx detached 28.12 s, tmux attached 21.97 s, tmux detached 21.82 s. A negative overhead means the harness reader is the slowest of the three readers, so it is not a clean ceiling. The detached runs involve no harness reader at all, and they still take 22 s to 28 s, so each multiplexer's own pane reading and processing dominates. A native PTY reader would be needed to establish the true ceiling. I did not build one.

## Reading

- **Flood throughput: mpx is about 30% slower than tmux.** It moves 100 MB in about 28 s against 22 s, roughly 3.6 MB/s against 4.6 MB/s. Attached and detached runs are nearly identical for each tool, so the client's rendering is not the main cost [INFERENCE: not profiled].
- **Idle memory: mpx is larger, and the gap grows with pane count.** The mpx daemon starts at about 5 MB and adds about 0.16 MB per idle pane. tmux stays near 4 MB with 20 panes.
- **Cold startup: mpx is about 7× slower, 112 ms against 16 ms.** The 2 s originally reported was the harness timeout, now removed. The remaining gap is plausibly one 100 ms readiness-poll cycle in mpx's daemon spawn path (`ensure_client` in `src/cli/sessions.rs` sleeps 100 ms between probes) [INFERENCE: not profiled].
- **`list-panes`: mpx is faster**, about 2.4 ms per call against 3.5 ms. Both include the CLI round trip to the server.
- **Pane creation: mpx is slightly faster**, 209 ms against 224 ms for 50 panes. The ranges overlap, so this is within noise on five repetitions.

## Method

Each repetition runs in a fresh sandbox under `/tmp`, with its own `HOME`, `XDG_RUNTIME_DIR`, `MPX_STATE_DIR`, and `TMUX_TMPDIR`. The child environment is built from scratch, with `SHELL=/bin/sh` and no user rc files. Both tools run with default config (`MPX_CONFIG=/dev/null`, `tmux -f /dev/null`). Each call names its session and socket explicitly, so the user's own sessions are never addressed.

- **Flood** (`yes | head -c 100000000` in a PTY pane). Attached: an interactive client is on a 120×40 PTY, drained continuously and answering terminal queries, and the pane is opened as a new visible window. Detached: no client. mpx waits with `mpx wait` (exit status). tmux waits on a `wait-for` signal sent by the pane after the pipeline ends. Panes use a PTY for both tools.
- **Flood floors**: the same pipeline run directly in a PTY through the harness, once with output to `/dev/null` and once with output drained by the harness.
- **Idle RSS**: `ps -o rss=` on the daemon pid, after a 1 s settle. One shell pane, then 19 more running `sleep 600`. Pane children and attached clients are not counted.
- **Startup**: a cold server. Time from spawning `mpx -s bench` or `tmux new -s bench` on a PTY until the status-bar text `[bench]` appears.
- **Command latency**: 200 sequential `list-panes` calls on a server with one pane. Reported as the total and the median of the per-call times.
- **Spawn**: 50 sequential `new-window -d` calls running `sleep 600`, on a server that already has one pane.

## Caveats

- Single machine, five repetitions per cell. Treat differences under about 10% as noise.
- The mpx binary was built from the working tree at `7bbfb55`, which may include uncommitted changes.
- Flood timing covers the whole `yes | head` pipeline, not the pane's own throughput alone. The flood floor does not isolate the multiplexer overhead on this machine (see above).
- `split-window` is not benchmarked. Splitting 50 times in a fixed terminal produces unusable panes, so spawn uses `new-window`.
- mpx panes are created with `--pty` to match tmux, which always uses a PTY.
- Idle RSS measures only the daemon. Add the pane children if you need the whole footprint.
- The tmux-absent path (skipped columns, exit 0) and the failure path (non-zero exit) were checked separately from the full run.

## Rerunning

Requirements: `python3`, and tmux if you want its columns. `bench/run.sh` builds `target/release/mpx` when it is missing.

```sh
bench/run.sh                          # full suite, about 12 minutes here
bench/run.sh --only latency,spawn     # a subset: flood, baseline, idle, startup, latency, spawn
BENCH_REPS=7 bench/run.sh             # more repetitions (minimum 5)
MPX_BIN=... TMUX_BIN= bench/run.sh    # alternate mpx binary; empty TMUX_BIN skips tmux
BENCH_OUT=/path/to/dir bench/run.sh   # keep results.json and results.md
```

The script exits non-zero if any step fails. Raw samples are in `results.json` and the rendered table in `results.md`, both under the output directory it prints on start.

## Update after performance work (mpx only)

tmux was no longer installed when these were taken, so there is no same-run tmux column. Compare against the tmux medians above (same machine, earlier the same day).

| Metric | mpx now | tmux (table above) |
| :--- | :--- | :--- |
| Startup: launch to first status-bar bytes | 11.5 ms (11.2-12.2) | 15.6 ms |
| Flood 30 MB detached (single runs, not the 100 MB suite) | 6.1 s, 6.8 s | 6.9 s (one run) |
| Daemon RSS idle, 20 panes | 8.2 MB | 3.9 MB |

What changed: the daemon readiness poll is 2 ms instead of 100 ms; the pane reader pauses 40 us after a read under 1 KB so output arrives in fewer, larger reads; the scrollback ring grows on demand instead of reserving 2 MB per pane. mpx is still about 2x tmux on idle memory with many panes: each pane holds a full 80x24 cell grid plus three OS threads.
