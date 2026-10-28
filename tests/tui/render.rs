use mpx::layout::{Divider, Rect, WindowTab};
use mpx::tui::canvas::Canvas;
use mpx::tui::render::{
    Compositor, RenderPaneContext, SelectionRange, StatusBar, hud_rect, status_left, status_right,
};
use mpx::vt::grid::Grid;

fn pane_ctx<'a>(grid: &'a Grid, rect: Rect, dimmed: bool) -> RenderPaneContext<'a> {
    RenderPaneContext {
        rect,
        grid,
        selection: None,
        scroll_offset: 0,
        badge: None,
        dimmed,
    }
}

fn draw_pane(grid: &Grid, dimmed: bool) -> String {
    let mut canvas = Canvas::default();
    canvas.begin(10, 5, None);
    let mut buf = Vec::new();
    Compositor::draw_pane_content(
        &mut canvas,
        &mut buf,
        pane_ctx(grid, Rect::new(0, 0, 10, 5), dimmed),
    )
    .unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn pane_content_draws_cell_text() {
    let mut grid = Grid::new(10, 5);
    grid.put_char('A');
    let out = draw_pane(&grid, false);
    assert!(out.contains("\x1b[0mA"), "{out:?}");
}

#[test]
fn dimmed_pane_draws_default_text_grey() {
    let mut grid = Grid::new(10, 5);
    grid.put_char('Z');
    let out = draw_pane(&grid, true);
    assert!(out.contains("\x1b[0;90mZ"), "{out:?}");
}

#[test]
fn status_bar_lists_session_and_pane_names() {
    let mut buf = Vec::new();
    let panes = vec![("main".to_string(), Rect::new(0, 0, 80, 23), true)];
    let bar = StatusBar {
        session: "sess",
        tabs: &[],
        panes: &panes,
        prefix_active: false,
        waiting_agents: 0,
        width: 80,
        y: 23,
    };
    Compositor::draw_status_bar(&mut buf, &bar).unwrap();
    let out = String::from_utf8_lossy(&buf);
    assert!(out.contains("[sess]"));
    assert!(out.contains("0:main*"));
    assert!(!out.contains('◆'));
}

#[test]
fn status_bar_paints_every_column_of_its_row() {
    let panes = vec![("main".to_string(), Rect::new(0, 0, 40, 9), true)];
    let bar = StatusBar {
        session: "s",
        tabs: &[],
        panes: &panes,
        prefix_active: false,
        waiting_agents: 0,
        width: 40,
        y: 9,
    };
    let mut terminal = mpx::vt::VtScreen::new(40, 10);
    terminal.feed(format!("\x1b[?7l\x1b[10;1H{}", "X".repeat(40)).as_bytes());
    let mut buf = Vec::new();
    Compositor::draw_status_bar(&mut buf, &bar).unwrap();
    terminal.feed(&buf);
    let row: String = (0..40)
        .map(|x| terminal.grid().get_cell(x, 9).map_or('?', |c| c.c))
        .collect();
    assert!(!row.contains('X'), "unpainted columns remain: {row:?}");
}

#[test]
fn status_bar_shows_waiting_agent_badge() {
    let mut buf = Vec::new();
    let bar = StatusBar {
        session: "sess",
        tabs: &[],
        panes: &[],
        prefix_active: false,
        waiting_agents: 3,
        width: 80,
        y: 23,
    };
    Compositor::draw_status_bar(&mut buf, &bar).unwrap();
    assert!(String::from_utf8_lossy(&buf).contains("◆3"));
}

#[test]
fn status_text_prefers_window_tabs_and_flags_the_prefix() {
    let panes = vec![("pane".to_string(), Rect::new(0, 0, 80, 23), true)];
    let tabs = vec![
        WindowTab {
            index: 0,
            name: "claude".into(),
            is_active: false,
        },
        WindowTab {
            index: 1,
            name: "cargo".into(),
            is_active: true,
        },
    ];
    let bar = StatusBar {
        session: "dev",
        tabs: &tabs,
        panes: &panes,
        prefix_active: true,
        waiting_agents: 2,
        width: 80,
        y: 23,
    };
    assert_eq!(status_left(&bar), "[dev] 0:claude- 1:cargo* ");
    assert_eq!(status_right(&bar, "12:00"), " \u{25c6}2  [PREFIX] 12:00");
}

#[test]
fn command_line_prints_text_on_its_row() {
    let mut buf = Vec::new();
    Compositor::draw_command_line(&mut buf, ":split", 10, 40).unwrap();
    assert!(String::from_utf8_lossy(&buf).contains(":split"));
}

#[test]
fn shortcut_hud_lists_split_and_escape_hint() {
    let mut buf = Vec::new();
    Compositor::draw_shortcut_hud(&mut buf, 80, 24).unwrap();
    let out = String::from_utf8_lossy(&buf);
    assert!(out.contains("╭"));
    assert!(out.contains("split vertical"));
    assert!(out.contains("split horizontal"));
    assert!(out.contains("ESC"));
}

#[test]
fn hud_rect_places_box_bottom_right() {
    let rect = hud_rect(80, 24).expect("fits 80x24");
    assert_eq!(rect.width, 30);
    assert_eq!(rect.x, 80 - 32);
    assert_eq!(rect.y + rect.height, 23);
    assert_eq!(hud_rect(20, 10), None);
}

#[test]
fn copy_mode_badge_shows_label() {
    let mut buf = Vec::new();
    Compositor::draw_copy_mode_badge(&mut buf, Rect::new(0, 0, 80, 24), "VISUAL LINE", 15, 100)
        .unwrap();
    assert!(String::from_utf8_lossy(&buf).contains("VISUAL LINE"));
}

#[test]
fn line_selection_range_is_inclusive_of_bounds() {
    let range = SelectionRange::Line {
        start_abs_y: 2,
        end_abs_y: 4,
    };
    assert!(!range.contains_abs(5, 1));
    assert!(range.contains_abs(0, 2));
    assert!(range.contains_abs(50, 3));
    assert!(range.contains_abs(10, 4));
    assert!(!range.contains_abs(0, 5));
}

#[test]
fn dividers_render_junction_connector() {
    let mut buf = Vec::new();
    let dividers = vec![
        Divider {
            is_vertical: false,
            x: 0,
            y: 10,
            length: 80,
        },
        Divider {
            is_vertical: true,
            x: 40,
            y: 11,
            length: 13,
        },
    ];
    let active_rect = Some(Rect::new(41, 11, 39, 13));
    let mut canvas = Canvas::default();
    canvas.begin(80, 24, None);
    Compositor::draw_dividers(&mut canvas, &mut buf, &dividers, active_rect).unwrap();
    assert!(String::from_utf8_lossy(&buf).contains('┬'));
}
