# vtcmp

Compares mpx's terminal model (`mpx::vt`) with `alacritty_terminal` and `vt100`, and
checks mpx end to end against editors run on their own. A standalone crate: the mpx build
and tests do not depend on it.

```sh
cd bench/vtcmp
python3 record.py              # record vim, nvim, less, ls, git log into captures/
cargo run --release            # conformance, chunking, captures, then throughput
cargo run --release -- conf    # conformance only (add -v for every case)
cargo run --release -- bench   # throughput only
cargo run --release -- show uni/   # one case (substring) on all three engines
```

- **Conformance**: about 130 hand-written cases (`src/cases.rs`). For each one the
  report names the engine that disagrees with the other two, on screen text, cursor and
  alternate screen; per-cell style; and scrollback.
- **Chunk invariance**: every case and capture is also fed byte by byte, and must give
  the same screen.
- **Captures**: `captures/NAME.COLSxROWS.bin` files, compared after every 400 bytes.
- **Throughput**: MB/s on synthetic streams, for mpx with scrollback, mpx as a live pane
  (no scrollback), and the other two engines.
- `spin STREAM [history]` feeds one stream for ten seconds so a profiler can attach:
  `sample vtcmp 4` on macOS.
- `replay SEG...` feeds `NAME.N.COLSxROWS` segments with a resize between each, in every
  engine and along the daemon's resize path.

End to end, against real editors in an isolated sandbox:

```sh
cargo build --release --manifest-path ../../Cargo.toml
python3 through_mpx.py OLD_MPX NEW_MPX   # nvim and vim, alone and beside a split
python3 client_vs_daemon.py NEW_MPX 3    # the client draws what the daemon holds
python3 daemon_vs_replay.py NEW_MPX 3    # the daemon holds what vim's bytes make
```

`through_mpx.py` sends the same keys to the editor on its own PTY and inside mpx, renders
both outputs with `alacritty_terminal`, and compares the editor pane row by row.
`tee_pty.py` logs what a program writes, one file per window size, for `replay`.
