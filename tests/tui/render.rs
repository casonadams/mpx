use mpx::layout::{Divider, Rect, WindowTab};
use mpx::tui::canvas::Canvas;
use mpx::config::BorderStyle;
use mpx::tui::render::{
    Compositor, DividerStyle, RenderPaneContext, SelectionRange, StatusBar, dim_color, format_tab,
    hud_rect, parse_color, status_left, status_right, vt_to_crossterm,
};
use mpx::vt::{CellStyle, Color as VtColor};
use mpx::vt::grid::Grid;

fn pane_ctx<'a>(grid: &'a Grid, rect: Rect, dimmed: bool) -> RenderPaneContext<'a> {
    RenderPaneContext {
        rect,
        grid,
        selection: None,
        scroll_offset: 0,
        badge: None,
        dimmed,
        dim_factor: 0.20,
        dim_content_bg: true,
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

/// What drawing one cell of `fg` text sends, in an inactive pane or not.
fn draw_colored(fg: VtColor, dimmed: bool) -> String {
    let mut grid = Grid::new(10, 5);
    grid.current_style.fg = Some(fg);
    grid.put_char('C');
    draw_pane(&grid, dimmed)
}

#[test]
fn dimmed_pane_draws_normal_colors_faint_and_bright_ones_normal() {
    assert!(draw_colored(VtColor::Blue, true).contains("\x1b[0;2;34mC"));
    assert!(draw_colored(VtColor::BrightBlue, true).contains("\x1b[0;34mC"));
    assert!(draw_colored(VtColor::Blue, false).contains("\x1b[0;34mC"));
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
        bg: None,
        fg: None,
        zoomed: false,
        copy_pane: None,
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
        bg: None,
        fg: None,
        zoomed: false,
        copy_pane: None,
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
        bg: None,
        fg: None,
        zoomed: false,
        copy_pane: None,
    };
    Compositor::draw_status_bar(&mut buf, &bar).unwrap();
    assert!(String::from_utf8_lossy(&buf).contains("◆3"));
}

#[test]
fn status_text_prefers_window_tabs_and_agent_badge() {
    let panes = vec![("pane".to_string(), Rect::new(0, 0, 80, 23), true)];
    let tabs = vec![
        WindowTab {
            index: 0,
            name: "claude".into(),
            is_active: false,
            is_last: true,
            ..Default::default()
        },
        WindowTab {
            index: 1,
            name: "cargo".into(),
            is_active: true,
            ..Default::default()
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
        bg: None,
        fg: None,
        zoomed: false,
        copy_pane: None,
    };
    assert_eq!(status_left(&bar), "[dev] 0:claude- 1:cargo* ");
    assert_eq!(status_right(&bar, "12:00"), " \u{25c6}2 12:00");
}

#[test]
fn format_tab_renders_agent_badges_and_tmux_flags() {
    let active = WindowTab {
        index: 0,
        name: "dev".into(),
        is_active: true,
        ..Default::default()
    };
    assert_eq!(format_tab(&active, false), "0:dev* ");
    assert_eq!(format_tab(&active, true), "0:dev*Z ");

    let waiting_agent = WindowTab {
        index: 1,
        name: "backend".into(),
        is_active: false,
        is_last: true,
        agent_badge: Some("◆".into()),
        ..Default::default()
    };
    assert_eq!(format_tab(&waiting_agent, false), "1:backend◆- ");

    let multi_waiting = WindowTab {
        index: 2,
        name: "agents".into(),
        is_active: false,
        agent_badge: Some("◆2".into()),
        ..Default::default()
    };
    assert_eq!(format_tab(&multi_waiting, false), "2:agents◆2 ");

    let working_with_activity = WindowTab {
        index: 3,
        name: "build".into(),
        is_active: false,
        has_activity: true,
        agent_badge: Some("●".into()),
        ..Default::default()
    };
    assert_eq!(format_tab(&working_with_activity, false), "3:build●# ");

    let bell_window = WindowTab {
        index: 4,
        name: "alerts".into(),
        is_active: false,
        has_bell: true,
        ..Default::default()
    };
    assert_eq!(format_tab(&bell_window, false), "4:alerts! ");
}

#[test]
fn update_tab_agent_badges_aggregates_agent_states_per_window() {
    use mpx::agents::LiveAgent;
    use mpx::protocol::{AgentInfo, AgentState, NodeInfo};
    use mpx::tui::status::update_tab_agent_badges;

    let mut tabs = vec![
        WindowTab {
            index: 0,
            name: "win0".into(),
            is_active: true,
            ..Default::default()
        },
        WindowTab {
            index: 1,
            name: "win1".into(),
            is_active: false,
            ..Default::default()
        },
        WindowTab {
            index: 2,
            name: "win2".into(),
            is_active: false,
            ..Default::default()
        },
    ];

    let make_agent = |session: &str, window: usize, state: AgentState| {
        let mut node = NodeInfo::new("agent".into(), true, "claude".into());
        node.window = Some(window);
        node.agent = Some(AgentInfo {
            harness: "claude".into(),
            state,
            session_id: None,
            detail: None,
            since_ms: 0,
            focus: Default::default(),
        });
        LiveAgent {
            session: session.into(),
            endpoint: String::new(),
            node,
        }
    };

    let agents = vec![
        make_agent("main", 0, AgentState::Working),
        make_agent("main", 1, AgentState::NeedsInput),
        make_agent("main", 1, AgentState::NeedsInput),
        make_agent("other", 2, AgentState::NeedsInput),
    ];

    update_tab_agent_badges(&mut tabs, &agents, "main");
    assert_eq!(tabs[0].agent_badge.as_deref(), Some("●"));
    assert_eq!(tabs[1].agent_badge.as_deref(), Some("◆2"));
    assert_eq!(tabs[2].agent_badge, None);
}

#[test]
fn status_bar_shows_zoom_flag_and_copy_pane_badge() {
    let panes = vec![("pane-1".to_string(), Rect::new(0, 0, 80, 23), true)];
    let bar = StatusBar {
        session: "main",
        tabs: &[],
        panes: &panes,
        prefix_active: false,
        waiting_agents: 0,
        width: 80,
        y: 23,
        bg: None,
        fg: None,
        zoomed: true,
        copy_pane: Some("pane-1"),
    };
    assert_eq!(status_left(&bar), "[main] 0:pane-1*Z ");
    assert_eq!(status_right(&bar, "12:00"), "[pane-1] Z 12:00");
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
    Compositor::draw_dividers(&mut canvas, &mut buf, &dividers, active_rect, DividerStyle::default()).unwrap();
    assert!(String::from_utf8_lossy(&buf).contains('┬'));
}

#[test]
fn parse_color_resolves_named_and_hex() {
    assert_eq!(parse_color("black"), Some(VtColor::Black));
    assert_eq!(parse_color("bright-green"), Some(VtColor::BrightGreen));
    assert_eq!(parse_color("bright_green"), Some(VtColor::BrightGreen));
    assert_eq!(parse_color("dark-grey"), Some(VtColor::BrightBlack));
    assert_eq!(parse_color("#3fb950"), Some(VtColor::Rgb(63, 185, 80)));
    assert_eq!(parse_color("#fff"), Some(VtColor::Rgb(255, 255, 255)));
    assert_eq!(parse_color("color238"), Some(VtColor::Indexed(238)));
    assert_eq!(parse_color("238"), Some(VtColor::Indexed(238)));
    assert_eq!(parse_color("invalid_color"), None);
}

#[test]
fn vt_to_crossterm_maps_correctly() {
    use crossterm::style::Color as CtColor;
    assert_eq!(vt_to_crossterm(VtColor::Black), CtColor::Black);
    assert_eq!(vt_to_crossterm(VtColor::Green), CtColor::DarkGreen);
    assert_eq!(vt_to_crossterm(VtColor::BrightGreen), CtColor::Green);
    assert_eq!(vt_to_crossterm(VtColor::Rgb(10, 20, 30)), CtColor::Rgb { r: 10, g: 20, b: 30 });
}

#[test]
fn dim_color_scales_rgb_and_dims_bright() {
    let rgb = VtColor::Rgb(100, 200, 50);
    // 20% dimming factor -> 80% brightness
    assert_eq!(dim_color(Some(rgb), 0.20), VtColor::Rgb(80, 160, 40));
    assert_eq!(dim_color(Some(VtColor::BrightRed), 0.20), VtColor::Red);
    assert_eq!(dim_color(None, 0.20), VtColor::BrightBlack);
}

#[test]
fn dimmed_pane_dims_content_background_and_suppresses_bold() {
    let mut grid = Grid::new(10, 5);
    grid.current_style = CellStyle {
        fg: Some(VtColor::Rgb(200, 200, 200)),
        bg: Some(VtColor::Rgb(100, 100, 100)),
        bold: true,
        ..CellStyle::default()
    };
    grid.put_char('X');
    let mut canvas = Canvas::default();
    canvas.begin(10, 5, None);
    let mut buf = Vec::new();
    Compositor::draw_pane_content(
        &mut canvas,
        &mut buf,
        RenderPaneContext {
            rect: Rect::new(0, 0, 10, 5),
            grid: &grid,
            selection: None,
            scroll_offset: 0,
            badge: None,
            dimmed: true,
            dim_factor: 0.20,
            dim_content_bg: true,
        },
    )
    .unwrap();
    let out = String::from_utf8_lossy(&buf);
    // Bold (\x1b[0;1...) must be suppressed, and bg dimmed (100 * 0.8 = 80)
    assert!(!out.contains(";1m"), "bold was not suppressed in dimmed pane: {out}");
    assert!(out.contains(";48;2;80;80;80"), "background was not dimmed: {out}");
}

#[test]
fn divider_styles_render_distinct_characters() {
    let dividers = vec![
        Divider {
            is_vertical: false,
            x: 0,
            y: 0,
            length: 10,
        },
        Divider {
            is_vertical: true,
            x: 5,
            y: 0,
            length: 5,
        },
    ];

    // Double style
    let mut canvas = Canvas::default();
    canvas.begin(10, 5, None);
    let mut buf = Vec::new();
    Compositor::draw_dividers(
        &mut canvas,
        &mut buf,
        &dividers,
        None,
        DividerStyle {
            active_color: VtColor::BrightGreen,
            inactive_color: VtColor::BrightBlack,
            border_style: BorderStyle::Double,
        },
    )
    .unwrap();
    let out = String::from_utf8_lossy(&buf);
    assert!(out.contains('╦') || out.contains('═'), "double border missing: {out}");

    // Ascii style
    canvas.begin(10, 5, None);
    buf.clear();
    Compositor::draw_dividers(
        &mut canvas,
        &mut buf,
        &dividers,
        None,
        DividerStyle {
            active_color: VtColor::BrightGreen,
            inactive_color: VtColor::BrightBlack,
            border_style: BorderStyle::Ascii,
        },
    )
    .unwrap();
    let out = String::from_utf8_lossy(&buf);
    assert!(out.contains('+') || out.contains('-'), "ascii border missing: {out}");
}

#[test]
fn status_bar_applies_custom_colors() {
    let panes = vec![("main".to_string(), Rect::new(0, 0, 80, 23), true)];
    let bar = StatusBar {
        session: "s",
        tabs: &[],
        panes: &panes,
        prefix_active: false,
        waiting_agents: 0,
        width: 80,
        y: 23,
        bg: Some(VtColor::Rgb(18, 24, 32)),
        fg: Some(VtColor::Rgb(240, 246, 252)),
        zoomed: false,
        copy_pane: None,
    };
    let mut buf = Vec::new();
    Compositor::draw_status_bar(&mut buf, &bar).unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains(";48;2;18;24;32"), "bg color missing: {text}");
    assert!(text.contains(";38;2;240;246;252"), "fg color missing: {text}");
}
