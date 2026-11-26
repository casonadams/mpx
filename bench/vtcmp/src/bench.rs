//! Throughput: bytes per second each engine sustains on synthetic output streams.

use crate::engine::{Alac, Engine, Mpx, Vt100};
use mpx::vt::VtScreen;
use std::time::Instant;

struct Stream {
    name: &'static str,
    bytes: Vec<u8>,
}

fn streams() -> Vec<Stream> {
    let mut plain = Vec::new();
    for i in 0..400_000 {
        plain.extend_from_slice(
            format!("line {i}: the quick brown fox jumps over the lazy dog 0123456789\r\n")
                .as_bytes(),
        );
    }
    let mut sgr = Vec::new();
    for i in 0..200_000u32 {
        sgr.extend_from_slice(
            format!(
                "\x1b[{}m\x1b[38;5;{}m{:>6}\x1b[0m \x1b[1;48;2;{};{};{}m item {}\x1b[0m trailing text here\r\n",
                31 + i % 7,
                i % 256,
                i,
                i % 256,
                (i / 3) % 256,
                (i / 7) % 256,
                i
            )
            .as_bytes(),
        );
    }
    let mut tui = Vec::new();
    for i in 0..60_000u32 {
        tui.extend_from_slice(
            format!(
                "\x1b[{};{}H\x1b[7mstatus {i}\x1b[0m\x1b[K",
                1 + i % 24,
                1 + i % 40
            )
            .as_bytes(),
        );
        tui.extend_from_slice(
            format!(
                "\x1b[{};1H\x1b[2K{} ~ editor line contents {i}",
                1 + (i * 7) % 24,
                i
            )
            .as_bytes(),
        );
        if i % 50 == 0 {
            tui.extend_from_slice(b"\x1b[2;23r\x1b[23;1H\n\n\x1b[r");
        }
    }
    let mut cjk = Vec::new();
    for i in 0..200_000u32 {
        cjk.extend_from_slice(
            format!("{i:>6} \u{4e2d}\u{6587}\u{5b57}\u{7b26} caf\u{e9} \u{1f600} \u{3042}\u{3044} e\u{301}x mixed text\r\n").as_bytes(),
        );
    }
    vec![
        Stream {
            name: "CJK/emoji/combining lines",
            bytes: cjk,
        },
        Stream {
            name: "plain text (scrolling)",
            bytes: plain,
        },
        Stream {
            name: "SGR heavy colored lines",
            bytes: sgr,
        },
        Stream {
            name: "cursor-addressed TUI",
            bytes: tui,
        },
    ]
}

fn time(mut f: impl FnMut()) -> f64 {
    let best = (0..3)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64()
        })
        .fold(f64::MAX, f64::min);
    best
}

/// Feeds one stream into mpx for ten seconds, for a sampling profiler to attach to.
pub fn spin(stream: &str, history: bool) {
    let s = streams()
        .into_iter()
        .find(|s| s.name.starts_with(stream))
        .expect("stream");
    let end = Instant::now() + std::time::Duration::from_secs(10);
    while Instant::now() < end {
        let mut e = if history {
            VtScreen::new(200, 50)
        } else {
            VtScreen::live(200, 50)
        };
        e.feed(&s.bytes);
    }
}

pub fn run() {
    println!("\n== throughput, 200x50 screen, best of 3 (MB/s) ==");
    println!(
        "{:<28} {:>8} {:>10} {:>11} {:>10} {:>8}",
        "stream", "MB", "mpx", "mpx(live)", "alacritty", "vt100"
    );
    for s in streams() {
        let mb = s.bytes.len() as f64 / 1e6;
        let rate = |secs: f64| format!("{:.0}", mb / secs);
        let mpx = time(|| {
            let mut e = Mpx::new(200, 50);
            e.feed(&s.bytes);
        });
        let live = time(|| {
            let mut e = VtScreen::live(200, 50);
            e.feed(&s.bytes);
        });
        let alac = time(|| {
            let mut e = Alac::new(200, 50);
            e.feed(&s.bytes);
        });
        let v100 = time(|| {
            let mut e = Vt100::new(200, 50);
            e.feed(&s.bytes);
        });
        println!(
            "{:<28} {:>8.1} {:>10} {:>11} {:>10} {:>8}",
            s.name,
            mb,
            rate(mpx),
            rate(live),
            rate(alac),
            rate(v100)
        );
    }
}
