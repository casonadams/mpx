#!/usr/bin/env python3
"""mpx vs tmux benchmark suite. Driven by bench/run.sh; see docs/BENCHMARKS.md.

Isolation: every repetition gets a fresh sandbox under /tmp (short paths, since
unix socket paths are length-limited) with its own HOME, XDG_RUNTIME_DIR,
MPX_STATE_DIR and TMUX_TMPDIR. Child environments are built from scratch, so no
MPX_* or TMUX variables leak in, and every mpx/tmux call names its session and
socket explicitly. The user's real sessions are never addressed.
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ptyclient import PtyClient  # noqa: E402

SESSION = "bench"
STATUS_MARKER = f"[{SESSION}]".encode()  # both status bars render the session name in brackets
FLOOD_SH = "yes | head -c 100000000"
IDLE_PANES = 20
SETTLE_S = 1.0
LATENCY_CALLS = 200
SPAWN_PANES = 50
CMD_TIMEOUT = 120
STARTUP_TIMEOUT = 30
FLOOD_TIMEOUT = 600


class BenchError(RuntimeError):
    pass


def run(argv: list[str], env: dict[str, str], timeout: float = CMD_TIMEOUT) -> str:
    """Run a CLI call; any failure or timeout raises BenchError (non-zero exit)."""
    try:
        proc = subprocess.run(
            argv, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=timeout
        )
    except subprocess.TimeoutExpired as exc:
        raise BenchError(f"timed out after {timeout}s: {shlex.join(argv)}") from exc
    if proc.returncode != 0:
        raise BenchError(f"exit {proc.returncode}: {shlex.join(argv)}\n{proc.stdout}{proc.stderr}")
    return proc.stdout


def rss_kb(pid: int) -> int:
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True, timeout=10)
    if out.returncode != 0 or not out.stdout.strip():
        raise BenchError(f"ps could not read pid {pid}")
    return int(out.stdout.strip())


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


class Sandbox:
    """A fresh isolated environment for one repetition, removed by close()."""

    def __init__(self) -> None:
        self.root = Path(tempfile.mkdtemp(prefix="mpxb-", dir="/tmp"))
        self.runtime = self.root / "r"
        self.state = self.root / "s"
        self.tmpdir = self.root / "t"
        self.home = self.root / "h"
        for d in (self.runtime, self.state, self.tmpdir, self.home):
            d.mkdir()
        self.env = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": str(self.home),
            "SHELL": "/bin/sh",
            "TERM": "xterm-256color",
            "XDG_RUNTIME_DIR": str(self.runtime),
            "MPX_STATE_DIR": str(self.state),
            "MPX_CONFIG": "/dev/null",
            "TMUX_TMPDIR": str(self.tmpdir),
        }

    def close(self) -> None:
        shutil.rmtree(self.root, ignore_errors=True)


class Engine:
    """Adapter exposing the same operations for mpx and tmux."""

    name = ""

    def __init__(self, binary: str, sandbox: Sandbox) -> None:
        self.binary = binary
        self.sandbox = sandbox
        self._pid: int | None = None

    def base(self) -> list[str]:
        raise NotImplementedError

    def cli(self, *args: str, timeout: float = CMD_TIMEOUT) -> str:
        return run(self.base() + list(args), self.sandbox.env, timeout)

    def client_argv(self) -> list[str]:
        """Interactive client that creates or attaches the session."""
        raise NotImplementedError

    def start_detached(self) -> None:
        """Create the session with one shell pane and no client attached."""
        raise NotImplementedError

    def new_pane(self, name: str, argv: list[str], detached: bool) -> None:
        raise NotImplementedError

    def flood(self, detached: bool) -> float:
        """Seconds for a pane to print FLOOD_BYTES and exit."""
        raise NotImplementedError

    def list_panes(self) -> None:
        self.cli(*self.list_args())

    def list_args(self) -> list[str]:
        raise NotImplementedError

    def pane_count(self) -> int:
        raise NotImplementedError

    def server_pid(self) -> int:
        raise NotImplementedError

    def stop(self) -> None:
        """Kill the server; kill its process directly if kill-server cannot."""
        try:
            pid = self.server_pid() if self._pid is None else self._pid
            self._pid = pid
        except BenchError:
            pid = self._pid
        try:
            self.cli("kill-server", timeout=30)
        except BenchError:
            pass
        if pid is not None and pid_alive(pid):
            os.kill(pid, signal.SIGKILL)


class MpxEngine(Engine):
    name = "mpx"

    def __init__(self, binary: str, sandbox: Sandbox) -> None:
        super().__init__(binary, sandbox)
        self.endpoint = str(sandbox.runtime / "mpx" / f"{SESSION}.sock")

    def base(self) -> list[str]:
        return [
            self.binary,
            "-f", "/dev/null",
            "-s", SESSION,
            "--endpoint", self.endpoint,
            "--state-dir", str(self.sandbox.state),
        ]

    def client_argv(self) -> list[str]:
        return self.base()

    def start_detached(self) -> None:
        self.cli("new-session", "-d")
        self.cli("new-window", "-d")  # one idle shell pane

    def new_pane(self, name: str, argv: list[str], detached: bool) -> None:
        args = ["new-window"]
        if detached:
            args.append("-d")
        args += ["-n", name, "--pty", "--", *argv]
        self.cli(*args)

    def flood(self, detached: bool) -> float:
        start = time.perf_counter()
        self.new_pane("flood", ["sh", "-c", FLOOD_SH], detached)
        self.cli("wait", "--timeout", str(FLOOD_TIMEOUT), "flood", timeout=FLOOD_TIMEOUT + 30)
        return time.perf_counter() - start

    def list_args(self) -> list[str]:
        return ["list-panes"]

    def pane_count(self) -> int:
        return len(json.loads(self.cli("list-panes", "--json")))

    def server_pid(self) -> int:
        for session in json.loads(self.cli("list-sessions", "--json")):
            if session["name"] == SESSION:
                self._pid = int(session["pid"])
                return self._pid
        raise BenchError("session missing from list-sessions")


class TmuxEngine(Engine):
    name = "tmux"
    socket = "mpxbench"

    def base(self) -> list[str]:
        return [self.binary, "-f", "/dev/null", "-L", self.socket]

    def client_argv(self) -> list[str]:
        return self.base() + ["new", "-s", SESSION]

    def start_detached(self) -> None:
        self.cli("new-session", "-d", "-s", SESSION)

    def new_pane(self, name: str, argv: list[str], detached: bool) -> None:
        args = ["new-window", "-t", f"{SESSION}:"]
        if detached:
            args.append("-d")
        args += ["-n", name]
        if argv:
            args.append(shlex.join(argv))
        self.cli(*args)

    def flood(self, detached: bool) -> float:
        done = "mpxbench-flood-done"
        start = time.perf_counter()
        waiter = subprocess.Popen(
            self.base() + ["wait-for", done],
            env=self.sandbox.env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        try:
            signal_done = shlex.join(self.base() + ["wait-for", "-S", done])
            self.new_pane("flood", ["sh", "-c", f"{FLOOD_SH}; {signal_done}"], detached)
            try:
                rc = waiter.wait(timeout=FLOOD_TIMEOUT)
            except subprocess.TimeoutExpired as exc:
                raise BenchError("tmux flood pane did not finish") from exc
            if rc != 0:
                raise BenchError(f"tmux wait-for exit {rc}: {waiter.stderr.read().decode()}")
        finally:
            if waiter.poll() is None:
                waiter.kill()
                waiter.wait()
        return time.perf_counter() - start

    def list_args(self) -> list[str]:
        return ["list-panes", "-s", "-t", SESSION]

    def pane_count(self) -> int:
        return len(self.cli("list-panes", "-s", "-t", SESSION, "-F", "#{pane_id}").split())

    def server_pid(self) -> int:
        self._pid = int(self.cli("display-message", "-p", "#{pid}").strip())
        return self._pid


# --- measurements -----------------------------------------------------------
# Each sampler runs one repetition against a fresh sandbox and returns a dict of
# metric key -> value. Units: seconds for flood, ms for latency, MB for RSS.


def sample_flood(engine_cls, binary: str) -> dict[str, float]:
    out = {}
    for attached in (True, False):
        sandbox = Sandbox()
        eng = engine_cls(binary, sandbox)
        client = None
        try:
            if attached:
                client = PtyClient(eng.client_argv(), sandbox.env, marker=STATUS_MARKER)
                client.wait_marker(STARTUP_TIMEOUT)
            else:
                eng.start_detached()
            key = "flood_attached" if attached else "flood_detached"
            out[key] = eng.flood(detached=not attached)
        finally:
            if client is not None:
                client.close()
            eng.stop()
            sandbox.close()
    return out


def sample_baseline() -> dict[str, float]:
    """Floors for the flood: output to /dev/null, and output drained by the harness.

    Both run `yes | head` directly in a PTY with no multiplexer.
    """
    out = {}
    for key, sh in (("flood_baseline", FLOOD_SH + " > /dev/null"), ("flood_baseline_tty", FLOOD_SH)):
        sandbox = Sandbox()
        client = None
        try:
            client = PtyClient(["/bin/sh", "-c", sh], sandbox.env)
            elapsed, code = client.wait_exit(FLOOD_TIMEOUT)
            if code != 0:
                raise BenchError(f"{key} exit {code}")
            out[key] = elapsed
        finally:
            if client is not None:
                client.close()
            sandbox.close()
    return out


def sample_startup(engine_cls, binary: str) -> dict[str, float]:
    sandbox = Sandbox()
    eng = engine_cls(binary, sandbox)
    client = None
    try:
        client = PtyClient(eng.client_argv(), sandbox.env, marker=STATUS_MARKER)
        return {"startup": client.wait_marker(STARTUP_TIMEOUT) * 1000}
    finally:
        if client is not None:
            client.close()
        eng.stop()
        sandbox.close()


def sample_idle(engine_cls, binary: str) -> dict[str, float]:
    sandbox = Sandbox()
    eng = engine_cls(binary, sandbox)
    try:
        eng.start_detached()
        time.sleep(SETTLE_S)
        pid = eng.server_pid()
        if eng.pane_count() != 1:
            raise BenchError(f"{eng.name}: expected 1 pane")
        rss_one = rss_kb(pid)
        for i in range(IDLE_PANES - 1):
            eng.new_pane(f"idle{i}", ["sleep", "600"], detached=True)
        time.sleep(SETTLE_S)
        if eng.pane_count() != IDLE_PANES:
            raise BenchError(f"{eng.name}: expected {IDLE_PANES} panes")
        return {"idle_rss_1": rss_one / 1024, "idle_rss_20": rss_kb(pid) / 1024}
    finally:
        eng.stop()
        sandbox.close()


def sample_latency(engine_cls, binary: str) -> dict[str, float]:
    sandbox = Sandbox()
    eng = engine_cls(binary, sandbox)
    try:
        eng.start_detached()
        per_call = []
        loop_start = time.perf_counter()
        for _ in range(LATENCY_CALLS):
            t = time.perf_counter()
            eng.list_panes()
            per_call.append(time.perf_counter() - t)
        total = time.perf_counter() - loop_start
        return {
            "list_total": total * 1000,
            "list_per_call": statistics.median(per_call) * 1000,
        }
    finally:
        eng.stop()
        sandbox.close()


def sample_spawn(engine_cls, binary: str) -> dict[str, float]:
    sandbox = Sandbox()
    eng = engine_cls(binary, sandbox)
    try:
        eng.start_detached()
        start = time.perf_counter()
        for i in range(SPAWN_PANES):
            eng.new_pane(f"sp{i}", ["sleep", "600"], detached=True)
        elapsed = time.perf_counter() - start
        if eng.pane_count() != SPAWN_PANES + 1:
            raise BenchError(f"{eng.name}: spawn count mismatch")
        return {"spawn_50": elapsed * 1000}
    finally:
        eng.stop()
        sandbox.close()


METRICS = [
    # key, label, decimals, unit
    ("flood_attached", "Flood 100 MB, client attached (time to pane exit)", 2, "s"),
    ("flood_detached", "Flood 100 MB, detached (time to pane exit)", 2, "s"),
    ("idle_rss_1", "Daemon RSS idle, 1 pane", 1, "MB"),
    ("idle_rss_20", "Daemon RSS idle, 20 panes", 1, "MB"),
    ("startup", "Startup: launch to first status-bar bytes (cold server)", 1, "ms"),
    ("list_total", "200 sequential `list-panes`, total", 1, "ms"),
    ("list_per_call", "`list-panes` per-call median", 2, "ms"),
    ("spawn_50", "Create 50 panes via `new-window -d`", 1, "ms"),
]

PLAIN = "plain"  # result column for the engine-independent flood baseline

# group name, metric keys, sampler, per_engine
GROUPS = [
    ("flood", ["flood_attached", "flood_detached"], sample_flood, True),
    ("baseline", ["flood_baseline", "flood_baseline_tty"], sample_baseline, False),
    ("idle", ["idle_rss_1", "idle_rss_20"], sample_idle, True),
    ("startup", ["startup"], sample_startup, True),
    ("latency", ["list_total", "list_per_call"], sample_latency, True),
    ("spawn", ["spawn_50"], sample_spawn, True),
]


# --- reporting --------------------------------------------------------------


def summarize(values: list[float]) -> tuple[float, float, float]:
    return statistics.median(values), min(values), max(values)


def cell(values: list[float], decimals: int, unit: str) -> str:
    if not values:
        return "not run"
    med, lo, hi = summarize(values)
    return f"{med:,.{decimals}f} {unit} ({lo:,.{decimals}f}–{hi:,.{decimals}f})"


def render(results: dict, reps: int, machine: str) -> str:
    lines = ["# mpx vs tmux benchmark results", "", "## Machine", "", "```", machine.rstrip(), "```", ""]
    lines += [
        f"Repetitions per cell: {reps}. Each cell is the median with (min–max) across repetitions.",
        "Lower is better for every metric. The ratio column is mpx median divided by tmux median.",
        "",
    ]
    lines += ["| Metric | mpx | tmux | mpx / tmux |", "|---|---|---|---|"]
    for key, label, decimals, unit in METRICS:
        per = results.get(key, {})
        mpx_vals = per.get("mpx", [])
        tmux_vals = per.get("tmux")
        if tmux_vals is None:
            tmux_cell, ratio = "not installed (skipped)", "n/a"
        else:
            tmux_cell = cell(tmux_vals, decimals, unit)
            ratio = (
                f"{summarize(mpx_vals)[0] / summarize(tmux_vals)[0]:.2f}×"
                if mpx_vals and tmux_vals and summarize(tmux_vals)[0] > 0
                else "n/a"
            )
        lines.append(f"| {label} | {cell(mpx_vals, decimals, unit)} | {tmux_cell} | {ratio} |")

    base = results.get("flood_baseline", {}).get(PLAIN, [])
    tty = results.get("flood_baseline_tty", {}).get(PLAIN, [])
    if base:
        base_med = summarize(base)[0]
        lines += [
            "",
            "## Flood overhead",
            "",
            "Floor: `yes | head -c 100000000 > /dev/null` run directly in a PTY, with no multiplexer and no "
            "terminal output. Overhead is each run's median minus the floor's median, so it includes the "
            "multiplexer and the terminal output it renders.",
            "",
            "| Run | Time | Overhead vs floor |",
            "|---|---|---|",
            f"| Floor: plain PTY, output to /dev/null | {cell(base, 2, 's')} | baseline |",
        ]
        if tty:
            tty_med = summarize(tty)[0]
            lines.append(
                f"| Plain PTY, output drained by the harness | {cell(tty, 2, 's')} | "
                f"{tty_med - base_med:+.2f} s ({tty_med / base_med:.2f}×) |"
            )
        for label, key in (("attached", "flood_attached"), ("detached", "flood_detached")):
            for engine in ("mpx", "tmux"):
                vals = results.get(key, {}).get(engine)
                if not vals:
                    continue
                med = summarize(vals)[0]
                lines.append(
                    f"| {engine}, {label} | {cell(vals, 2, 's')} | "
                    f"{med - base_med:+.2f} s ({med / base_med:.2f}×) |"
                )
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mpx", required=True, help="path to the mpx binary")
    parser.add_argument("--tmux", default="", help="path to tmux; empty skips tmux columns")
    parser.add_argument("--reps", type=int, default=5, help="repetitions per metric (>= 5)")
    parser.add_argument("--out", required=True, help="output directory for results.json/results.md")
    parser.add_argument("--only", default="", help="comma-separated groups: flood,baseline,idle,startup,latency,spawn")
    args = parser.parse_args()
    if args.reps < 5:
        parser.error("--reps must be at least 5")

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    machine_file = out / "machine.txt"
    machine = machine_file.read_text() if machine_file.exists() else "(machine info not recorded)\n"

    engines: list[tuple[str, type[Engine], str]] = [("mpx", MpxEngine, args.mpx)]
    if args.tmux:
        engines.append(("tmux", TmuxEngine, args.tmux))
    else:
        print("tmux not found: skipping tmux columns", file=sys.stderr)

    wanted = {g.strip() for g in args.only.split(",") if g.strip()}
    results: dict[str, dict[str, list[float]]] = {}
    for group, keys, _, per_engine in GROUPS:
        for key in keys:
            results[key] = {name: [] for name, _, _ in engines} if per_engine else {PLAIN: []}
    for key, *_ in METRICS:
        results.setdefault(key, {name: [] for name, _, _ in engines})

    for group, keys, sampler, per_engine in GROUPS:
        if wanted and group not in wanted:
            continue
        for rep in range(1, args.reps + 1):
            if per_engine:
                for name, cls, binary in engines:
                    print(f"[{group}] {name} rep {rep}/{args.reps}", file=sys.stderr, flush=True)
                    sample = sampler(cls, binary)
                    for key in keys:
                        results[key][name].append(sample[key])
            else:
                print(f"[{group}] rep {rep}/{args.reps}", file=sys.stderr, flush=True)
                sample = sampler()
                for key in keys:
                    results[key][PLAIN].append(sample[key])
        (out / "results.json").write_text(json.dumps(results, indent=2))

    report = render(results, args.reps, machine)
    (out / "results.md").write_text(report)
    print(report)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BenchError as exc:
        print(f"bench error: {exc}", file=sys.stderr)
        sys.exit(1)
