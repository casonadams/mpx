"""Pseudo-terminal harness for the attach and flood benchmarks.

Runs an interactive client (``mpx`` or ``tmux new``) on a PTY and drains its
output on a background thread, so the client never stalls on a full PTY
buffer. Records when a marker first appears on the PTY, which is how the
startup benchmark detects the first status-bar frame.

It also answers the terminal queries a multiplexer sends at attach, as a real
terminal would. mpx sends OSC 10/11 (default colours), DECRQM for mode 69 and
DA1, and waits up to 2 s for the DA1 reply. Without these replies every attach
would sit in that timeout, which would measure the harness, not the multiplexer.
"""

from __future__ import annotations

import fcntl
import os
import pty
import re
import signal
import struct
import termios
import threading
import time

ROWS = 40
COLS = 120

# Queries a terminal must answer. DA1 is ESC [ c; DECRQM for DECLRMM (mode 69)
# is ESC [ ? 69 $ p; OSC 10/11 ask for the default foreground/background colour
# and end with ST or BEL.
_QUERY = re.compile(
    rb"\x1b\[0?c"
    rb"|\x1b\]10;\?(?:\x07|\x1b\\)"
    rb"|\x1b\]11;\?(?:\x07|\x1b\\)"
    rb"|\x1b\[\?69\$p"
)
_DA1_ANSWER = b"\x1b[?62c"
_FG_ANSWER = b"\x1b]10;rgb:d8d8/dede/e9e9\x1b\\"
_BG_ANSWER = b"\x1b]11;rgb:2e2e/3434/4040\x1b\\"
_DECRPM_ANSWER = b"\x1b[?69;2$y"  # mode 69 is supported and reset
# Longest query fragment that can straddle two reads.
_PARTIAL_MAX = 64


def _answer_for(query: bytes) -> bytes:
    if query.startswith(b"\x1b]10;"):
        return _FG_ANSWER
    if query.startswith(b"\x1b]11;"):
        return _BG_ANSWER
    if query.endswith(b"$p"):
        return _DECRPM_ANSWER
    return _DA1_ANSWER


class PtyClient:
    """An interactive client attached to a pseudo-terminal we own."""

    def __init__(self, argv: list[str], env: dict[str, str], marker: bytes | None = None):
        self.marker = marker
        self.drained = 0
        self.marker_at: float | None = None
        self._tail = b""
        self._seen = threading.Event()
        self.spawned_at = time.perf_counter()
        pid, fd = pty.fork()
        if pid == 0:  # child: become the client on the slave side
            try:
                fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
                os.execvpe(argv[0], argv, env)
            finally:
                os._exit(127)
        self.pid = pid
        self._fd = fd
        self._reader = threading.Thread(target=self._drain, daemon=True)
        self._reader.start()

    def _drain(self) -> None:
        keep = 2 * len(self.marker) if self.marker else 0
        pending = b""
        while True:
            try:
                chunk = os.read(self._fd, 65536)
            except OSError:  # EIO once the client has exited
                return
            if not chunk:
                return
            self.drained += len(chunk)
            if self.marker and not self._seen.is_set():
                window = self._tail + chunk
                if self.marker in window:
                    self.marker_at = time.perf_counter()
                    self._seen.set()
                self._tail = window[-keep:]
            pending = self._answer_queries(pending + chunk)

    def _answer_queries(self, buf: bytes) -> bytes:
        """Reply to every complete query in ``buf``; return the unanswered tail."""
        end = 0
        for match in _QUERY.finditer(buf):
            try:
                os.write(self._fd, _answer_for(match.group()))
            except OSError:
                return b""
            end = match.end()
        return buf[end:][-_PARTIAL_MAX:]

    def wait_marker(self, timeout: float) -> float:
        """Seconds from spawn until the marker first arrives on the PTY."""
        if not self._seen.wait(timeout):
            raise TimeoutError(f"marker {self.marker!r} not seen within {timeout}s")
        assert self.marker_at is not None
        return self.marker_at - self.spawned_at

    def wait_exit(self, timeout: float) -> tuple[float, int]:
        """Seconds from spawn until the client exits, and its exit code."""
        deadline = time.monotonic() + timeout
        while True:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid == self.pid:
                return time.perf_counter() - self.spawned_at, os.waitstatus_to_exitcode(status)
            if time.monotonic() > deadline:
                raise TimeoutError(f"client still running after {timeout}s")
            time.sleep(0.001)

    def close(self) -> None:
        """Terminate the client, reap it, and release the PTY."""
        try:
            os.kill(self.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        reaped = False
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            try:
                if os.waitpid(self.pid, os.WNOHANG)[0] == self.pid:
                    reaped = True
                    break
            except ChildProcessError:
                reaped = True
                break
            time.sleep(0.02)
        if not reaped:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except (ProcessLookupError, ChildProcessError):
                pass
        os.close(self._fd)
        self._reader.join(timeout=2)
