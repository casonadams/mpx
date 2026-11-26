#!/usr/bin/env python3
"""race_resize.py [RUNS]: resizes vim's terminal while vim is still drawing, as a terminal that
resizes mid-stream does, and checks every engine against the others on that exact interleaving.

Bytes read before each resize are one segment, bytes after it the next, so `vtcmp replay` feeds
each engine the stream split where the terminal really resized. The engines (and mpx along the
daemon's resize path) must agree whatever the timing."""

import fcntl
import os
import random
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

import through_mpx as t
from record import spawn, stop


def read_for(fd: int, seconds: float) -> bytes:
    data = bytearray()
    end = time.time() + seconds
    while True:
        left = end - time.time()
        if left <= 0:
            return bytes(data)
        r, _, _ = select.select([fd], [], [], left)
        if r:
            try:
                data += os.read(fd, 65536)
            except OSError:
                return bytes(data)


def resize(fd: int, pid: int, cols: int, rows: int) -> None:
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    os.kill(pid, signal.SIGWINCH)


def run(rng: random.Random) -> str:
    sizes = [(50, 14), (50, 29), (100, 29), (33, 29), (100, 29)]
    plan = [sizes[0]] + rng.sample(sizes[1:], 3)
    pid, fd = spawn(t.EDITORS["vim"], *plan[0], os.environ | {"TERM": "xterm-256color"})
    segments = [bytearray(read_for(fd, rng.uniform(0.05, 0.6)))]
    for cols, rows in plan[1:]:
        resize(fd, pid, cols, rows)
        segments.append(bytearray(read_for(fd, rng.uniform(0.0, 0.05))))
    for key in t.KEYS:
        os.write(fd, key.encode())
        segments[-1] += read_for(fd, 0.15)
    segments[-1] += read_for(fd, 0.5)
    stop(pid, fd)
    root = tempfile.mkdtemp(prefix="race-")
    paths = []
    for i, ((cols, rows), data) in enumerate(zip(plan, segments)):
        path = os.path.join(root, f"seg.{i}.{cols}x{rows}")
        with open(path, "wb") as f:
            f.write(data)
        paths.append(path)
    out = subprocess.run([t.VTCMP, "replay", *paths], capture_output=True, text=True).stdout
    lines = out.splitlines()
    engines = {}
    current = None
    for line in lines:
        stripped = line.strip()
        if stripped.startswith(("mpx ", "alacritty ", "vt100 ")) and "cursor=" in stripped:
            current = stripped.split()[0]
            engines[current] = []
        elif stripped == "mpx as the daemon":
            current = "daemon"
            engines[current] = []
        elif current and "|" in line and line.lstrip()[:1].isdigit():
            engines[current].append(line.split("|", 1)[1].rstrip())
    verdict = lines[0].split(": ", 1)[1]
    daemon_ok = engines.get("daemon") == engines.get("alacritty")
    calm = t.direct(t.EDITORS["vim"], *plan[-1])
    same_as_calm = [r.rstrip() for r in engines.get("alacritty", [])] == [r.rstrip() for r in calm]
    return (
        f"{plan} engines: {verdict}, daemon path == alacritty: {daemon_ok}, "
        f"alacritty == vim started at the final size: {same_as_calm}"
    )


if __name__ == "__main__":
    rng = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
    for i in range(int(sys.argv[1]) if len(sys.argv) > 1 else 10):
        print(i, run(rng))
