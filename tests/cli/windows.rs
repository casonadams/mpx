use super::fixture::{Fixture, run_command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geometry {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

/// Every laid-out pane as (name, geometry, focused), at the terminal's screen size (or 80x24).
fn layout(f: &Fixture) -> Vec<(String, Geometry, bool)> {
    let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
    f.query(|mut c| async move {
        let (rects, _, _) = c.layout(w, h).await.expect("layout");
        rects
            .into_iter()
            .map(|(name, r, focused)| {
                let g = Geometry {
                    x: r.x,
                    y: r.y,
                    w: r.width,
                    h: r.height,
                };
                (name, g, focused)
            })
            .collect()
    })
}

fn place(f: &Fixture, name: &str) -> Geometry {
    layout(f)
        .into_iter()
        .find(|(n, _, _)| n == name)
        .map(|(_, g, _)| g)
        .unwrap_or_else(|| panic!("{name} is not in the layout"))
}

fn focused(f: &Fixture) -> String {
    layout(f)
        .into_iter()
        .find(|(_, _, focused)| *focused)
        .map(|(n, _, _)| n)
        .expect("a focused pane")
}

/// A window whose only pane is `base`, with `side` split off to its right.
fn base_and_side(f: &Fixture) {
    let window = ["new-window", "-n", "base", "sleep", "30"];
    assert_eq!(run_command(f, &window), 0);
    let split = ["split", "-t", "base", "-n", "side", "sleep", "30"];
    assert_eq!(run_command(f, &split), 0);
}

#[test]
fn new_window_adds_a_window_and_detach_keeps_the_focus() {
    let f = Fixture::start();
    let first = ["new-window", "-n", "first", "sleep", "30"];
    assert_eq!(run_command(&f, &first), 0);
    assert_eq!(f.info().windows, 1);
    let detached = ["new-window", "-d", "-n", "second", "sleep", "30"];
    assert_eq!(run_command(&f, &detached), 0);
    assert_eq!(f.info().windows, 2);
    assert_eq!(focused(&f), "first");
}

#[test]
fn split_places_the_new_pane_beside_or_below_its_target() {
    let f = Fixture::start();
    base_and_side(&f);
    let (base, side) = (place(&f, "base"), place(&f, "side"));
    assert_eq!(base.y, side.y);
    assert_ne!(base.x, side.x);

    let vertical = ["split", "-v", "-t", "side", "-n", "below", "sleep", "30"];
    assert_eq!(run_command(&f, &vertical), 0);
    let (side, below) = (place(&f, "side"), place(&f, "below"));
    assert_eq!(below.x, side.x);
    assert_ne!(below.y, side.y);
}

#[test]
fn split_detach_keeps_focus_and_percent_sizes_the_new_pane() {
    let f = Fixture::start();
    let window = ["new-window", "-n", "base", "sleep", "30"];
    assert_eq!(run_command(&f, &window), 0);
    let before = place(&f, "base");
    let detached = [
        "split", "-d", "-l", "25", "-t", "base", "-n", "small", "sleep", "30",
    ];
    assert_eq!(run_command(&f, &detached), 0);
    assert_eq!(focused(&f), "base");
    let small = place(&f, "small");
    assert!(small.w * 2 < before.w, "small {small:?} vs base {before:?}");
}

#[test]
fn layout_command_succeeds_with_panes_on_screen() {
    let f = Fixture::start();
    base_and_side(&f);
    let show = ["layout", "--width", "80", "--height", "24"];
    assert_eq!(run_command(&f, &show), 0);
}

#[test]
fn swap_pane_exchanges_the_two_positions() {
    let f = Fixture::start();
    base_and_side(&f);
    let (base, side) = (place(&f, "base"), place(&f, "side"));
    let swap = ["swap-pane", "--source", "base", "-t", "side"];
    assert_eq!(run_command(&f, &swap), 0);
    assert_eq!(place(&f, "base"), side);
    assert_eq!(place(&f, "side"), base);
    let unknown = ["swap-pane", "--source", "nowhere", "-t", "side"];
    assert_eq!(run_command(&f, &unknown), 1);
}

#[test]
fn select_layout_applies_a_preset_and_rejects_unknown_names() {
    let f = Fixture::start();
    base_and_side(&f);
    assert_eq!(run_command(&f, &["select-layout", "even-vertical"]), 0);
    let (base, side) = (place(&f, "base"), place(&f, "side"));
    assert_eq!(base.x, side.x);
    assert_ne!(base.y, side.y);
    assert_eq!(run_command(&f, &["select-layout"]), 0);
    assert_eq!(run_command(&f, &["select-layout", "bogus"]), 1);
}

#[test]
fn resize_pane_sets_the_absolute_pty_size_and_needs_a_direction_or_size() {
    let f = Fixture::start();
    let script = "read line; stty size; sleep 30";
    let pane = ["new-window", "-n", "r", "--pty", "--", "sh", "-c", script];
    assert_eq!(run_command(&f, &pane), 0);
    let resize = ["resize-pane", "-t", "r", "-x", "33", "-y", "11"];
    assert_eq!(run_command(&f, &resize), 0);
    assert_eq!(run_command(&f, &["send", "r", "go"]), 0);
    let wait = ["wait", "r", "--match", "11 33", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
    assert_eq!(run_command(&f, &["resize-pane", "-t", "r"]), 1);
}

#[test]
fn resize_pane_direction_moves_the_border_by_the_amount() {
    let f = Fixture::start();
    base_and_side(&f);
    let before = place(&f, "base").w;
    let right = ["resize-pane", "-t", "base", "-R", "5"];
    assert_eq!(run_command(&f, &right), 0);
    assert_eq!(place(&f, "base").w, before + 5);
}

#[test]
fn new_window_and_split_pass_dash_c_to_the_command_not_to_mpx() {
    let f = Fixture::start();
    let window = [
        "new-window",
        "-n",
        "nc",
        "sh",
        "-c",
        "echo from-new; sleep 30",
    ];
    assert_eq!(run_command(&f, &window), 0);
    let wait = ["wait", "nc", "--match", "from-new", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
    let split = [
        "split",
        "-h",
        "-t",
        "nc",
        "-n",
        "sc",
        "sh",
        "-c",
        "echo from-split; sleep 30",
    ];
    assert_eq!(run_command(&f, &split), 0);
    let wait = ["wait", "sc", "--match", "from-split", "--timeout", "5"];
    assert_eq!(run_command(&f, &wait), 0);
}
