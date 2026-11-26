#!/usr/bin/env python3
"""Record raw terminal output of real programs on a PTY for vtcmp.

Writes captures/<name>.<cols>x<rows>.bin: the bytes the program wrote, replayed
into every engine by `vtcmp conf`.
"""

import fcntl
import os
import pty
import select
import struct
import sys
import termios
import time

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "captures")
SRC = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../src/vt/parser/csi.rs"))
WIDE = os.path.join(OUT, "wide.txt")

ESC = "\x1b"
SCRIPT_EDITOR = [
    ":set number\r", ":syntax on\r", ":set cursorline\r",
    "\x06", "\x06", "\x02", "G", "gg", "/fn\r", "n", "n",
    "Vjjj", ESC, ":vsplit\r", ":split\r", "\x17l", "\x17j", "\x04", "\x15",
    "ggVG", ESC, "dd", "u", ":q\r", ":q\r", ":q\r",
]
SCRIPT_WIDE = ["j", "j", "$", "0", "3j", "\x04", "gg", ":q\r"]
SCRIPT_LESS = [" ", " ", "b", "G", "g", "/fn\r", "n", "q"]

JOBS = [
    ("vim_edit", ["vim", "-u", "NONE", "-N", "-n", SRC], SCRIPT_EDITOR, 80, 24),
    ("vim_edit_small", ["vim", "-u", "NONE", "-N", "-n", SRC], SCRIPT_EDITOR, 40, 12),
    ("vim_edit_wide", ["vim", "-u", "NONE", "-N", "-n", SRC], SCRIPT_EDITOR, 160, 50),
    ("vim_wide_chars", ["vim", "-u", "NONE", "-N", "-n", "--cmd", "set encoding=utf-8", WIDE], SCRIPT_WIDE, 80, 24),
    ("nvim_edit", ["nvim", "--clean", SRC], SCRIPT_EDITOR, 80, 24),
    ("nvim_edit_wide", ["nvim", "--clean", SRC], SCRIPT_EDITOR, 160, 50),
    ("nvim_wide_chars", ["nvim", "--clean", WIDE], SCRIPT_WIDE, 80, 24),
    ("less_source", ["less", SRC], SCRIPT_LESS, 80, 24),
    ("ls_color", ["sh", "-c", "CLICOLOR_FORCE=1 ls -lG /usr/bin | head -60"], [], 80, 24),
    ("git_log_graph", ["sh", "-c", "git log --graph --color=always --oneline -n 40"], [], 100, 30),
    ("colortest_256", ["sh", "-c", "for i in $(seq 0 255); do printf '\\033[48;5;%dm %3d \\033[0m' $i; [ $((i%16)) = 15 ] && echo; done"], [], 100, 24),
]


def spawn(argv, cols, rows, env):
    """Starts `argv` on a new PTY of `cols` x `rows`; returns (pid, master fd)."""
    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(argv[0], argv, env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    return pid, fd


def drain_pty(fd, wait):
    """Reads what `fd` writes for `wait` seconds, longer while output keeps coming (up to 3s more)."""
    data = bytearray()
    end = time.time() + wait
    hard = end + 3
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if not r:
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        data.extend(chunk)
        end = min(hard, max(end, time.time() + 0.05))
    return bytes(data)


def stop(pid, fd):
    """Kills the program on `fd` and reaps it. The master closes first: on macOS a process
    exiting with unread PTY output blocks until it does."""
    try:
        os.kill(pid, 9)
    except ProcessLookupError:
        pass
    os.close(fd)
    os.waitpid(pid, 0)


def record(name, argv, keys, cols, rows):
    pid, fd = spawn(argv, cols, rows, os.environ | {"TERM": "xterm-256color", "LANG": "en_US.UTF-8"})
    data = bytearray(drain_pty(fd, 0.6))
    for k in keys:
        try:
            os.write(fd, k.encode())
        except OSError:
            break
        data += drain_pty(fd, 0.15)
    data += drain_pty(fd, 0.5)
    stop(pid, fd)
    path = os.path.join(OUT, f"{name}.{cols}x{rows}.bin")
    with open(path, "wb") as f:
        f.write(data)
    print(f"{path}: {len(data)} bytes")


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    with open(WIDE, "w", encoding="utf-8") as f:
        f.write("ascii line\n\u4e2d\u6587\u5b57\u7b26 CJK line with wide chars\n")
        f.write("emoji \U0001f600 \U0001f680 in text\ne\u0301 combining and \u3042\u3044\u3046 kana\n")
        f.write("\uff21\uff22\uff23 fullwidth latin\n" + "\u2500" * 20 + "\n")
        f.write("tail line " * 8 + "\n")
    only = sys.argv[1:]
    for job in JOBS:
        if not only or job[0] in only:
            record(*job)
