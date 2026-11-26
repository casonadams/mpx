#!/usr/bin/env python3
"""tee_pty.py LOG CMD...: runs CMD on a PTY the size of this one, relaying input, output and
window size changes, and logs what CMD writes so a pane's raw output can be replayed elsewhere.
LOG gets one segment per size: LOG.0.COLSxROWS, LOG.1.COLSxROWS, ... in order."""

import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import tty

prefix = sys.argv[1]
state = {"n": 0, "log": None}


def winsize():
    return fcntl.ioctl(0, termios.TIOCGWINSZ, b"\0" * 8)


def open_segment(size):
    rows, cols = struct.unpack("HHHH", size)[:2]
    if state["log"]:
        state["log"].close()
    state["log"] = open(f"{prefix}.{state['n']}.{cols}x{rows}", "wb")
    state["n"] += 1


size = winsize()
pid, fd = pty.fork()
if pid == 0:
    os.execvp(sys.argv[2], sys.argv[2:])
fcntl.ioctl(fd, termios.TIOCSWINSZ, size)
open_segment(size)


def on_winch(*_):
    size = winsize()
    open_segment(size)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, size)


signal.signal(signal.SIGWINCH, on_winch)
tty.setraw(0)
while True:
    try:
        r, _, _ = select.select([0, fd], [], [])
    except InterruptedError:
        continue
    if 0 in r:
        os.write(fd, os.read(0, 4096))
    if fd in r:
        try:
            data = os.read(fd, 65536)
        except OSError:
            break
        if not data:
            break
        state["log"].write(data)
        state["log"].flush()
        os.write(1, data)
