use mpx::layout::{Divider, Rect, WindowTab};
use mpx::tui::canvas::Canvas;
use mpx::config::{BorderStyle, InactiveColorMode};
use mpx::tui::render::{
    Compositor, DividerStyle, RenderPaneContext, SelectionRange, StatusBar,
    apply_inactive_fg, color_to_rgb, desaturate_color, dim_color, format_tab,
    hud_rect, parse_color, status_left, status_right, tint_color, vt_to_crossterm,
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
        active_fg: None,
        active_bg: None,
        inactive_fg: None,
        inactive_bg: None,
        color_mode: InactiveColorMode::Dim,
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
    assert!(out.contains("\x1b[0;38;2;204;204;204mZ"), "{out:?}");
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
        width: 80,
        y: 23,
        bg: None,
        fg: None,
        zoomed: false,
        copy_pane: None,
        marked_pane: None,
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
        width: 40,
        y: 9,
        bg: None,
        fg: None,
        zoomed: false,
        copy_pane: None,
        marked_pane: None,
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
        width: 80,
        y: 23,
        bg: None,
        fg: None,
        zoomed: false,
        copy_pane: None,
        marked_pane: None,
    };
    assert_eq!(status_left(&bar), "[dev] 0:claude- 1:cargo* ");
    assert_eq!(status_right(&bar, "12:00"), "12:00");
}

#[test]
fn format_tab_renders_tmux_flags() {
    let active = WindowTab {
        index: 0,
        name: "dev".into(),
        is_active: true,
        ..Default::default()
    };
    assert_eq!(format_tab(&active, false), "0:dev* ");
    assert_eq!(format_tab(&active, true), "0:dev*Z ");

    let last_window = WindowTab {
        index: 1,
        name: "backend".into(),
        is_active: false,
        is_last: true,
        ..Default::default()
    };
    assert_eq!(format_tab(&last_window, false), "1:backend- ");

    let activity_window = WindowTab {
        index: 2,
        name: "build".into(),
        is_active: false,
        has_activity: true,
        ..Default::default()
    };
    assert_eq!(format_tab(&activity_window, false), "2:build# ");

    let bell_window = WindowTab {
        index: 3,
        name: "alerts".into(),
        is_active: false,
        has_bell: true,
        ..Default::default()
    };
    assert_eq!(format_tab(&bell_window, false), "3:alerts! ");
}

#[test]
fn format_tab_renders_marked_flag() {
    let marked_tab = WindowTab {
        index: 0,
        name: "dev".into(),
        is_active: true,
        has_marked: true,
        ..Default::default()
    };
    assert_eq!(format_tab(&marked_tab, false), "0:dev*M ");

    let marked_inactive = WindowTab {
        index: 1,
        name: "other".into(),
        is_active: false,
        is_last: true,
        has_marked: true,
        ..Default::default()
    };
    assert_eq!(format_tab(&marked_inactive, false), "1:other-M ");
}


#[test]
fn status_bar_shows_zoom_flag_and_copy_pane_badge() {
    let panes = vec![("pane-1".to_string(), Rect::new(0, 0, 80, 23), true)];
    let bar = StatusBar {
        session: "main",
        tabs: &[],
        panes: &panes,
        prefix_active: false,
        width: 80,
        y: 23,
        bg: None,
        fg: None,
        zoomed: true,
        copy_pane: Some("pane-1"),
        marked_pane: None,
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
    Compositor::draw_dividers(
        &mut canvas,
        &mut buf,
        &dividers,
        active_rect,
        None,
        DividerStyle::default(),
    )
    .unwrap();
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
    // Default text and white text scale with dim_factor
    assert_eq!(dim_color(None, 0.20), VtColor::Rgb(204, 204, 204));
    assert_eq!(dim_color(None, 0.05), VtColor::Rgb(242, 242, 242));
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
        pane_ctx(&grid, Rect::new(0, 0, 10, 5), true),
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
        None,
        DividerStyle {
            active_color: VtColor::BrightGreen,
            inactive_color: VtColor::BrightBlack,
            marked_color: VtColor::BrightMagenta,
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
        None,
        DividerStyle {
            active_color: VtColor::BrightGreen,
            inactive_color: VtColor::BrightBlack,
            marked_color: VtColor::BrightMagenta,
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
        width: 80,
        y: 23,
        bg: Some(VtColor::Rgb(18, 24, 32)),
        fg: Some(VtColor::Rgb(240, 246, 252)),
        zoomed: false,
        copy_pane: None,
        marked_pane: None,
    };
    let mut buf = Vec::new();
    Compositor::draw_status_bar(&mut buf, &bar).unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains(";48;2;18;24;32"), "bg color missing: {text}");
    assert!(text.contains(";38;2;240;246;252"), "fg color missing: {text}");
}

#[test]
fn active_pane_template_applies_default_fg_and_bg() {
    let mut grid = Grid::new(10, 5);
    grid.put_char('A');
    let mut canvas = Canvas::default();
    canvas.begin(10, 5, None);
    let mut buf = Vec::new();
    let mut ctx = pane_ctx(&grid, Rect::new(0, 0, 10, 5), false);
    ctx.active_fg = Some(VtColor::Rgb(216, 222, 233));
    ctx.active_bg = Some(VtColor::Rgb(46, 52, 64));
    Compositor::draw_pane_content(&mut canvas, &mut buf, ctx).unwrap();
    let out = String::from_utf8_lossy(&buf);
    assert!(out.contains(";38;2;216;222;233"), "active fg missing: {out}");
    assert!(out.contains(";48;2;46;52;64"), "active bg missing: {out}");
}

#[test]
fn inactive_pane_template_uses_explicit_fg_and_bg() {
    let mut grid = Grid::new(10, 5);
    grid.put_char('Z');
    let mut canvas = Canvas::default();
    canvas.begin(10, 5, None);
    let mut buf = Vec::new();
    let mut ctx = pane_ctx(&grid, Rect::new(0, 0, 10, 5), true);
    ctx.inactive_fg = Some(VtColor::Rgb(76, 86, 106));
    ctx.inactive_bg = Some(VtColor::Rgb(36, 41, 51));
    Compositor::draw_pane_content(&mut canvas, &mut buf, ctx).unwrap();
    let out = String::from_utf8_lossy(&buf);
    assert!(out.contains(";38;2;76;86;106"), "inactive fg missing: {out}");
    assert!(out.contains(";48;2;36;41;51"), "inactive bg missing: {out}");
}

#[test]
fn color_to_rgb_and_desaturation_and_tint() {
    let (r, g, b) = color_to_rgb(VtColor::Red);
    assert_eq!((r, g, b), (205, 49, 49));

    // Desaturate pure red by 100% -> luminance grayscale
    let desat_full = desaturate_color(VtColor::Rgb(255, 0, 0), 1.0);
    if let VtColor::Rgb(dr, dg, db) = desat_full {
        assert_eq!(dr, dg);
        assert_eq!(dg, db);
        assert_eq!(dr, 76); // 0.299 * 255 = 76.245
    } else {
        panic!("expected rgb");
    }

    // Tint pure red 50% toward black
    let tinted = tint_color(VtColor::Rgb(200, 100, 50), Some(VtColor::Rgb(0, 0, 0)), 0.5);
    if let VtColor::Rgb(tr, tg, tb) = tinted {
        assert_eq!(tr, 100);
        assert_eq!(tg, 50);
        assert_eq!(tb, 25);
    } else {
        panic!("expected rgb");
    }
}

#[test]
fn inactive_color_modes_transform_appropriately() {
    let orig = Some(VtColor::Rgb(200, 100, 50));
    // None mode keeps original
    let (none_fg, _) = apply_inactive_fg(orig, InactiveColorMode::None, None, None, 0.2, false);
    assert_eq!(none_fg, orig);

    // Desaturate mode pulls toward luminance
    let (desat_fg, _) =
        apply_inactive_fg(orig, InactiveColorMode::Desaturate, None, None, 0.5, false);
    assert_ne!(desat_fg, orig);

    // Tint mode pulls toward target
    let target = Some(VtColor::Rgb(10, 20, 30));
    let (tinted_fg, _) =
        apply_inactive_fg(orig, InactiveColorMode::Tint, target, None, 0.5, false);
    assert_ne!(tinted_fg, orig);
}

