#!/usr/bin/env python3
"""Run nvim and vim inside mpx in an isolated sandbox, and directly, and compare what the user sees.

Usage: through_mpx.py MPX_BIN [MPX_BIN...]

For each editor the same keys are sent to the editor alone on a PTY the size of the pane, and to
an mpx client whose focused pane runs it. Both outputs are rendered by alacritty_terminal
(`vtcmp render`); the pane rows of the client's screen must equal the editor's own screen.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from record import ESC, WIDE, drain_pty, spawn, stop  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
VTCMP = os.path.join(HERE, "target/release/vtcmp")
COLS, ROWS = 100, 30
KEYS = ["j", "j", "$", "0", "v", "l", "l", "l", "l", ESC, "3j", "$", ":vsplit\r", "\x17l", "gg", "j", "w", "w", "w"]
EDITORS = {
    "nvim": ["nvim", "--clean", WIDE],
    "vim": ["vim", "-u", "NONE", "-N", "-n", "--cmd", "set encoding=utf-8", WIDE],
}


def render(data: bytes, cols: int, rows: int, span: tuple[int, int] | None = None) -> list[str]:
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(data)
    extra = [str(n) for n in span] if span else []
    out = subprocess.run([VTCMP, "render", f.name, str(cols), str(rows), *extra], capture_output=True, text=True).stdout
    os.unlink(f.name)
    return out.splitlines()[:rows]


def run_keys(fd: int, keys: list[str]) -> bytes:
    data = bytearray(drain_pty(fd, 1.5))
    for k in keys:
        os.write(fd, k.encode())
        data += drain_pty(fd, 0.25)
    data += drain_pty(fd, 1.0)
    return bytes(data)


def direct(argv: list[str], cols: int, rows: int) -> list[str]:
    pid, fd = spawn(argv, cols, rows, os.environ | {"TERM": "xterm-256color"})
    data = run_keys(fd, KEYS)
    stop(pid, fd)
    return render(data, cols, rows)


class Sandbox:
    """An mpx client on a PTY of COLS x ROWS in a fresh temp directory, answering the queries a
    real terminal answers at attach (DECRQM 69, DA1) so it attaches without its 2 s timeout."""

    def __init__(self, binary: str) -> None:
        self.root = tempfile.mkdtemp(prefix="mpxv-", dir="/tmp")
        self.env = {
            "PATH": os.environ["PATH"], "HOME": self.root, "SHELL": "/bin/sh", "TERM": "xterm-256color",
            "LANG": "en_US.UTF-8", "XDG_RUNTIME_DIR": self.root, "MPX_STATE_DIR": self.root,
            "MPX_CONFIG": "/dev/null",
        }
        self.base = [binary, "-f", "/dev/null", "-s", "v", "--endpoint", f"{self.root}/v.sock",
                     "--state-dir", self.root]
        self.pid, self.fd = spawn(self.base, COLS, ROWS, self.env)
        drain_pty(self.fd, 0.3)
        os.write(self.fd, b"\x1b[?69;2$y\x1b[?62;22c")
        drain_pty(self.fd, 1.0)

    def cli(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(self.base + list(args), env=self.env, capture_output=True, text=True, timeout=10)

    def close(self) -> None:
        self.cli("kill-server")
        stop(self.pid, self.fd)
        shutil.rmtree(self.root, ignore_errors=True)


def through(binary: str, argv: list[str], split: bool) -> tuple[list[str], tuple[int, int]]:
    """The client's screen and the editor pane's size; with `split`, a pane showing the same file
    opens beside the editor (unfocused) once it runs, so the editor pane narrows to the left half."""
    box = Sandbox(binary)
    box.cli("new-window", "-n", "ed", "--pty", "--", *argv)
    if split:
        drain_pty(box.fd, 1.0)
        box.cli("split", "-h", "-d", "--pty", "--", "sh", "-c", f"cat {WIDE}; sleep 60")
    data = run_keys(box.fd, KEYS)
    size = next(p["size"] for p in json.loads(box.cli("list-panes", "--json").stdout) if p["name"] == "ed")
    box.close()
    return render(data, COLS, ROWS, (0, size[0])), (size[0], size[1])


def main() -> int:
    failed = False
    for name, argv in EDITORS.items():
        for split in (False, True):
            for binary in sys.argv[1:]:
                got, (cols, rows) = through(binary, argv, split)
                want = direct(argv, cols, rows)
                # The editor pane is at the top left, or one row down when the status bar is on top.
                pane = got[:rows]
                if pane != want:
                    pane = got[1 : rows + 1]
                bad = [y for y, (a, b) in enumerate(zip(pane, want)) if a != b]
                where = "beside a split" if split else "alone"
                print(f"{name} {where} ({cols}x{rows}) via {binary}: " + (f"{len(bad)} rows differ" if bad else "identical"))
                for y in bad[:4]:
                    print(f"  row {y:2} editor: {want[y]!r}\n         mpx:    {pane[y]!r}")
                failed |= bool(bad)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
