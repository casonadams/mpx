mod bench;
mod cases;
mod engine;

use cases::Case;
use engine::{Alac, Engine, Mpx, Snap, Step, Vt100};
use std::path::Path;

const NAMES: [&str; 3] = ["mpx", "alacritty", "vt100"];

fn run<E: Engine>(case: &Case, chunk: usize) -> Snap {
    let mut e = E::new(case.cols, case.rows);
    for step in &case.steps {
        match step {
            Step::Feed(bytes) => bytes.chunks(chunk.max(1)).for_each(|c| e.feed(c)),
            Step::Resize(c, r) => e.resize(*c, *r),
        }
    }
    e.snap()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dim {
    Screen,
    Style,
    History,
}

fn same(a: &Snap, b: &Snap, dim: Dim) -> Option<bool> {
    match dim {
        Dim::Screen => Some(a.text == b.text && a.cursor == b.cursor && a.alt == b.alt),
        Dim::Style => (a.text == b.text).then(|| a.styles == b.styles),
        Dim::History => match (&a.history, &b.history) {
            (Some(x), Some(y)) => Some(x == y),
            _ => None,
        },
    }
}

/// Which engine disagrees with the other two, if exactly one does.
fn verdict(s: &[Snap; 3], dim: Dim) -> &'static str {
    let eq = |i: usize, j: usize| same(&s[i], &s[j], dim);
    match (eq(0, 1), eq(0, 2), eq(1, 2)) {
        (Some(true), Some(true), _) | (Some(true), None, _) | (None, Some(true), _) => "ok",
        (Some(false), Some(false), Some(true)) => "MPX",
        (Some(false), Some(true), Some(false)) => "alacritty",
        (Some(true), Some(false), Some(false)) => "vt100",
        (Some(false), None, _) => "mpx!=alac",
        (None, Some(false), _) => "mpx!=vt100",
        (None, None, _) => "n/a",
        _ => "split",
    }
}

fn describe(s: &[Snap; 3], dim: Dim) -> String {
    let mut out = String::new();
    for (i, snap) in s.iter().enumerate() {
        out += &format!(
            "    {:<9} cursor={:?} alt={}\n",
            NAMES[i], snap.cursor, snap.alt
        );
        match dim {
            Dim::Screen => {
                for (y, row) in snap.text.iter().enumerate() {
                    out += &format!("      {y:>2}|{row}\n");
                }
            }
            Dim::History => {
                if let Some(h) = &snap.history {
                    out += &format!("      history={h:?}\n");
                }
            }
            Dim::Style => {
                for (y, row) in snap.styles.iter().enumerate() {
                    let d = Sty::default();
                    let cells: Vec<String> = row
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| **s != d)
                        .map(|(x, s)| format!("{x}:{s:?}"))
                        .collect();
                    if !cells.is_empty() {
                        out += &format!("      {y:>2}|{}\n", cells.join(" "));
                    }
                }
            }
        }
    }
    out
}
use engine::Sty;

fn snaps(case: &Case, chunk: usize) -> [Snap; 3] {
    [
        run::<Mpx>(case, chunk),
        run::<Alac>(case, chunk),
        run::<Vt100>(case, chunk),
    ]
}

fn conformance(cases: &[Case], verbose: bool) {
    let mut tally = std::collections::BTreeMap::<&str, usize>::new();
    println!(
        "{:<44} {:<11} {:<11} {:<11}",
        "case", "screen", "style", "history"
    );
    for case in cases {
        let s = snaps(case, usize::MAX);
        let v: Vec<&str> = [Dim::Screen, Dim::Style, Dim::History]
            .iter()
            .map(|d| verdict(&s, *d))
            .collect();
        if v.iter().all(|x| *x == "ok" || *x == "n/a") {
            *tally.entry("ok").or_default() += 1;
            if !verbose {
                continue;
            }
        }
        println!("{:<44} {:<11} {:<11} {:<11}", case.name, v[0], v[1], v[2]);
        for (d, dim) in [Dim::Screen, Dim::Style, Dim::History].iter().enumerate() {
            if v[d] == "MPX" || (verbose && v[d] != "ok") {
                print!("{}", describe(&s, *dim));
            }
            if v[d] != "ok" {
                *tally.entry(v[d]).or_default() += 1;
            }
        }
    }
    println!("\nverdict tally (cases ok counted once, other counts per dimension): {tally:?}");
}

fn chunking(cases: &[Case]) {
    println!("\nchunk invariance (byte-at-a-time feed must equal one-shot feed):");
    let mut bad = 0;
    for case in cases {
        let (one, byte) = (snaps(case, usize::MAX), snaps(case, 1));
        for i in 0..3 {
            if one[i] != byte[i] {
                bad += 1;
                println!("  {:<44} {}", case.name, NAMES[i]);
            }
        }
    }
    println!("  {bad} violations");
}

fn captures(dir: &Path, verbose: bool) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        println!("\n(no captures in {})", dir.display());
        return;
    };
    let mut files: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect();
    files.sort();
    let mut cs = Vec::new();
    for p in files {
        // name.COLSxROWS.bin
        let stem = p.file_stem().unwrap().to_string_lossy().to_string();
        let (name, dims) = stem.rsplit_once('.').unwrap();
        let (c, r) = dims.split_once('x').unwrap();
        cs.push(Case {
            name: format!("capture/{name}"),
            cols: c.parse().unwrap(),
            rows: r.parse().unwrap(),
            steps: vec![Step::Feed(std::fs::read(&p).unwrap())],
        });
    }
    println!("\n== recorded captures (final screen) ==");
    conformance(&cs, verbose);
    chunking(&cs);
    println!("\n== recorded captures (screen after every 400 bytes) ==");
    println!(
        "{:<34} {:>7} {:>9} {:>9} {:>9}  first mpx-odd frame",
        "capture", "frames", "mpx-odd", "alac-odd", "vt100-odd"
    );
    for case in &cs {
        let Step::Feed(bytes) = &case.steps[0] else {
            continue;
        };
        let mut e: [Box<dyn Engine>; 3] = [
            Box::new(Mpx::new(case.cols, case.rows)),
            Box::new(Alac::new(case.cols, case.rows)),
            Box::new(Vt100::new(case.cols, case.rows)),
        ];
        let (mut frames, mut odd, mut first) = (0, [0usize; 3], None);
        for (i, chunk) in bytes.chunks(400).enumerate() {
            e.iter_mut().for_each(|x| x.feed(chunk));
            let s = [e[0].snap(), e[1].snap(), e[2].snap()];
            frames += 1;
            let v = verdict(&s, Dim::Screen);
            let k = match v {
                "MPX" => 0,
                "alacritty" => 1,
                "vt100" => 2,
                _ => continue,
            };
            odd[k] += 1;
            if k == 0 && first.is_none() {
                first = Some((i, s));
            }
        }
        let at = first
            .as_ref()
            .map(|f| format!("#{}", f.0))
            .unwrap_or_default();
        println!(
            "{:<34} {:>7} {:>9} {:>9} {:>9}  {at}",
            case.name, frames, odd[0], odd[1], odd[2]
        );
        if let (true, Some((_, s))) = (verbose, first) {
            print!("{}", describe(&s, Dim::Screen));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "-v");
    let mode = args.first().map(String::as_str).unwrap_or("all");
    let all = cases::all();
    if mode == "replay" {
        // replay SEGMENT...: files named NAME.N.COLSxROWS, fed in order with a resize between.
        let mut steps = Vec::new();
        let mut first = None;
        for path in &args[1..] {
            let dims = path.rsplit('.').next().unwrap();
            let (c, r) = dims.split_once('x').unwrap();
            let (c, r): (usize, usize) = (c.parse().unwrap(), r.parse().unwrap());
            match first {
                None => first = Some((c, r)),
                Some(_) => steps.push(Step::Resize(c, r)),
            }
            steps.push(Step::Feed(std::fs::read(path).unwrap()));
        }
        let (cols, rows) = first.expect("segments");
        let case = Case {
            name: "replay".into(),
            cols,
            rows,
            steps,
        };
        let s = snaps(&case, usize::MAX);
        println!("screen verdict: {}", verdict(&s, Dim::Screen));
        print!("{}", describe(&s, Dim::Screen));
        // The daemon's own path: resize in place where it can, else rebuild from the whole log.
        let mut log = Vec::new();
        let mut screen = mpx::vt::VtScreen::live(cols, rows);
        for step in &case.steps {
            match step {
                Step::Feed(bytes) => {
                    screen.feed(bytes);
                    log.extend_from_slice(bytes);
                }
                Step::Resize(c, r) => {
                    let from = (screen.grid().cols, screen.grid().rows);
                    let in_place = screen.resize_in_place((*c, *r));
                    println!(
                        "    daemon resize {from:?} -> {:?}: in place {in_place}",
                        (c, r)
                    );
                    if !in_place {
                        *screen.grid_mut() =
                            mpx::vt::VtScreen::resized_grid(&log, &[], from, (*c, *r), 0);
                    }
                }
            }
        }
        println!("    mpx as the daemon");
        let grid = screen.grid();
        for y in 0..grid.rows {
            println!(
                "      {y:>2}|{}",
                grid.line_text(grid.history_len() + y).unwrap()
            );
        }
        return;
    }
    if mode == "render" {
        let bytes = std::fs::read(&args[1]).expect("capture");
        let (cols, rows) = (args[2].parse().unwrap(), args[3].parse().unwrap());
        let mut e = Alac::new(cols, rows);
        e.feed(&bytes);
        let s = e.snap();
        // Optional X WIDTH: only those columns of each row.
        let span = args
            .get(4)
            .zip(args.get(5))
            .map(|(x, w)| (x.parse::<usize>().unwrap(), w.parse::<usize>().unwrap()));
        for (row, cells) in s.text.iter().zip(&s.cells) {
            match span {
                Some((x, w)) => println!("{}", cells[x..x + w].concat().trim_end()),
                None => println!("{row}"),
            }
        }
        println!("cursor={:?}", s.cursor);
        return;
    }
    if mode == "spin" {
        bench::spin(
            args.get(1).map_or("plain", String::as_str),
            args.get(2).is_some_and(|a| a == "history"),
        );
        return;
    }
    if mode == "sizes" {
        println!(
            "Cell {} CellStyle {}",
            std::mem::size_of::<mpx::vt::Cell>(),
            std::mem::size_of::<mpx::vt::CellStyle>()
        );
        return;
    }
    if mode == "show" {
        for case in all.iter().filter(|c| c.name.contains(args[1].as_str())) {
            let s = snaps(case, usize::MAX);
            println!("## {}", case.name);
            print!("{}", describe(&s, Dim::Screen));
            print!("{}", describe(&s, Dim::History));
        }
        return;
    }
    if mode == "all" || mode == "conf" || mode == "-v" {
        println!("== hand-written cases ({}) ==", all.len());
        conformance(&all, verbose);
        chunking(&all);
        captures(Path::new("captures"), verbose);
    }
    if mode == "all" || mode == "bench" || mode == "-v" {
        bench::run();
    }
}
