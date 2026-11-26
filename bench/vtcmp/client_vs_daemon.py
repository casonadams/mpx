#!/usr/bin/env python3
"""client_vs_daemon.py MPX_BIN [RUNS]: vim beside an mpx split, then compares the editor pane as
the client drew it (rendered by alacritty_terminal) with the daemon's own screen (capture-pane).
They must match: the client draws what the daemon holds."""

import sys

import through_mpx as t
from record import drain_pty


def run(binary: str) -> tuple[list[str], list[str]]:
    box = t.Sandbox(binary)
    box.cli("new-window", "-n", "ed", "--pty", "--", *t.EDITORS["vim"])
    drain_pty(box.fd, 1.0)
    box.cli("split", "-h", "-d", "--pty", "--", "sh", "-c", f"cat {t.WIDE}; sleep 60")
    data = t.run_keys(box.fd, t.KEYS)
    daemon = [line.rstrip() for line in box.cli("capture-pane", "-p", "-t", "ed").stdout.splitlines()]
    box.close()
    client = t.render(data, t.COLS, t.ROWS, (0, 50))
    return client, daemon


if __name__ == "__main__":
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    for i in range(runs):
        client, daemon = run(sys.argv[1])
        pane = client[: len(daemon)] if client[: len(daemon)] == daemon else client[1 : len(daemon) + 1]
        bad = [y for y, (a, b) in enumerate(zip(pane, daemon)) if a != b]
        print(f"run {i}: {'client == daemon' if not bad else f'{len(bad)} rows differ'}")
        for y in bad[:3]:
            print(f"  row {y:2} daemon: {daemon[y]!r}\n         client: {pane[y]!r}")
