#!/usr/bin/env bash
# mpx vs tmux benchmark suite. Usage: bench/run.sh [--only flood,idle,startup,latency,spawn]
#
# Environment overrides:
#   MPX_BIN     mpx binary            (default: target/release/mpx, built if missing)
#   TMUX_BIN    tmux binary           (default: first tmux on PATH; empty -> skip tmux)
#   BENCH_REPS  repetitions per cell  (default: 5, minimum 5)
#   BENCH_OUT   output directory      (default: a fresh mktemp directory)
#
# Exits non-zero if any step fails. Every mpx/tmux call runs in an isolated
# sandbox; see bench/suite.py.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"

mpx_bin="${MPX_BIN:-$root/target/release/mpx}"
if [[ ! -x "$mpx_bin" ]]; then
  echo "bench: $mpx_bin missing, running cargo build --release" >&2
  (cd "$root" && cargo build --release)
fi

tmux_bin="${TMUX_BIN-}"
if [[ -z "${TMUX_BIN+set}" ]]; then
  tmux_bin="$(command -v tmux || true)"
fi
if [[ -z "$tmux_bin" ]]; then
  echo "bench: tmux not found; tmux columns will be skipped" >&2
fi

out="${BENCH_OUT:-$(mktemp -d /tmp/mpx-bench.XXXXXX)}"
mkdir -p "$out"

{
  echo "date:      $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "uname -m:  $(uname -m)"
  echo "uname -a:  $(uname -a)"
  if [[ "$(uname -s)" == "Darwin" ]]; then
    echo "macOS:     $(sw_vers -productName) $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  elif [[ -r /etc/os-release ]]; then
    echo "distro:    $(. /etc/os-release && echo "$PRETTY_NAME")"
  fi
  echo "mpx:       $("$mpx_bin" --version)"
  if [[ -n "$tmux_bin" ]]; then
    echo "tmux:      $("$tmux_bin" -V) ($tmux_bin)"
  else
    echo "tmux:      not installed"
  fi
} >"$out/machine.txt"

cat "$out/machine.txt" >&2
echo "bench: results in $out" >&2

exec python3 -B "$here/suite.py" \
  --mpx "$mpx_bin" \
  --tmux "$tmux_bin" \
  --reps "${BENCH_REPS:-5}" \
  --out "$out" \
  "$@"
