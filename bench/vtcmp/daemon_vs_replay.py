#!/usr/bin/env python3
"""daemon_vs_replay.py MPX_BIN [RUNS]: vim, logged through tee_pty.py, beside an mpx split. The
daemon's screen of the editor pane (capture-pane) must equal a replay of the bytes vim wrote, with
the same resizes, in every engine."""

import glob
import os
import subprocess
import sys

import through_mpx as t
from record import drain_pty

HERE = os.path.dirname(os.path.abspath(__file__))


def run(binary: str) -> tuple[list[str], list[str]]:
    box = t.Sandbox(binary)
    prefix = os.path.join(box.root, "raw")
    box.cli("new-window", "-n", "ed", "--pty", "--", "python3", os.path.join(HERE, "tee_pty.py"), prefix,
            *t.EDITORS["vim"])
    drain_pty(box.fd, 1.0)
    box.cli("split", "-h", "-d", "--pty", "--", "sh", "-c", f"cat {t.WIDE}; sleep 60")
    t.run_keys(box.fd, t.KEYS)
    daemon = [line.rstrip() for line in box.cli("capture-pane", "-p", "-t", "ed").stdout.splitlines()]
    segments = sorted(glob.glob(prefix + ".*"), key=lambda p: int(p.split(".")[-2]))
    out = subprocess.run([t.VTCMP, "replay", *segments], capture_output=True, text=True).stdout.splitlines()
    box.close()
    rows = len(daemon)
    replay = [line.split("|", 1)[1].rstrip() for line in out[2 : 2 + rows]]
    print("  ", out[0], "| segments:", [os.path.basename(s) for s in segments])
    return daemon, replay


if __name__ == "__main__":
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    for i in range(runs):
        daemon, replay = run(sys.argv[1])
        bad = [y for y, (a, b) in enumerate(zip(daemon, replay)) if a != b]
        print(f"run {i}: {'daemon == replay' if not bad else f'{len(bad)} rows differ'}")
        for y in bad[:3]:
            print(f"  row {y:2} daemon: {daemon[y]!r}\n         replay: {replay[y]!r}")
