use mpx::layout::{Divider, Rect, WindowTab};
use mpx::tui::canvas::Canvas;
use mpx::config::BorderStyle;
use mpx::tui::render::{
    Compositor, DividerStyle, HostColors, PaneColors, RenderPaneContext, SelectionRange,
    StatusBar, color_to_rgb, dim_color, format_tab, from_terminal_palette, hud_rect, Palette, inactive_color, parse_color,
    status_left, status_right, vt_to_crossterm,
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
        dim_factor: 0.5,
        active_fg: None,
        active_bg: None,
        inactive_fg: None,
        inactive_bg: None,
        default_fg: None,
        default_bg: None,
        palette: Default::default(),
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

/// Draws `grid` dimmed, after `tweak` has adjusted the context.
fn draw_dimmed_with(grid: &Grid, tweak: impl FnOnce(&mut RenderPaneContext)) -> String {
    let mut canvas = Canvas::default();
    canvas.begin(10, 5, None);
    let mut buf = Vec::new();
    let mut ctx = pane_ctx(grid, Rect::new(0, 0, 10, 5), true);
    tweak(&mut ctx);
    Compositor::draw_pane_content(&mut canvas, &mut buf, ctx).unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn dimmed_default_text_follows_the_terminals_default_colors_when_known() {
    let mut grid = Grid::new(10, 5);
    grid.put_char('Z');
    let known = draw_dimmed_with(&grid, |ctx| {
        ctx.default_fg = Some(VtColor::Rgb(216, 222, 233));
        ctx.default_bg = Some(VtColor::Rgb(46, 52, 64));
        ctx.dim_factor = 0.5;
    });
    assert!(known.contains(";38;2;108;111;117;"), "{known:?}");
    assert!(known.contains(";48;2;23;26;32mZ"), "{known:?}");

    let unknown = draw_dimmed_with(&grid, |_| {});
    assert!(!unknown.contains("38;2;") && !unknown.contains("48;2;"), "{unknown:?}");
}

/// What drawing one cell of `fg` text sends, in an inactive pane or not.
fn draw_colored(fg: VtColor, dimmed: bool) -> String {
    let mut grid = Grid::new(10, 5);
    grid.current_style.fg = Some(fg);
    grid.put_char('C');
    draw_pane(&grid, dimmed)
}

#[test]
fn dimmed_pane_draws_palette_colors_dimmed() {
    let (r, g, bl) = match dim_color(VtColor::Blue, 0.5) {
        VtColor::Rgb(r, g, b) => (r, g, b),
        other => panic!("{other:?}"),
    };
    let dimmed = draw_colored(VtColor::Blue, true);
    assert!(dimmed.contains(&format!(";38;2;{r};{g};{bl}mC")), "{dimmed:?}");
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
fn dimmed_pane_dims_content_colors_and_suppresses_bold() {
    let mut grid = Grid::new(10, 5);
    grid.current_style = CellStyle {
        fg: Some(VtColor::Rgb(200, 100, 50)),
        bg: Some(VtColor::Rgb(100, 150, 50)),
        bold: true,
        ..CellStyle::default()
    };
    grid.put_char('X');
    let out = draw_dimmed_with(&grid, |ctx| ctx.dim_factor = 0.5);
    // Half dimmed, each channel is halved.
    assert!(out.contains(";38;2;100;50;25"), "foreground not dimmed: {out}");
    assert!(out.contains(";48;2;50;75;25"), "background not dimmed: {out}");
    assert!(!out.contains(";1m"), "bold was not suppressed in dimmed pane: {out}");
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
            bg: None,
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
            bg: None,
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
fn color_to_rgb_and_dimming() {
    let (r, g, b) = color_to_rgb(VtColor::Red);
    assert_eq!((r, g, b), (205, 49, 49));

    assert_eq!(dim_color(VtColor::Rgb(255, 0, 0), 1.0), VtColor::Rgb(0, 0, 0));
    assert_eq!(dim_color(VtColor::Rgb(200, 100, 50), 0.5), VtColor::Rgb(100, 50, 25));
    assert_eq!(dim_color(VtColor::Rgb(200, 100, 50), 0.0), VtColor::Rgb(200, 100, 50));
}



#[test]
fn inactive_cells_are_dimmed_by_the_factor() {
    let orange = Some(VtColor::Rgb(200, 100, 50));
    assert_eq!(inactive_color(orange, None, None, 0.0), orange);
    assert_eq!(inactive_color(orange, None, None, 1.0), Some(VtColor::Rgb(0, 0, 0)));
    assert_eq!(inactive_color(orange, None, None, 0.5), Some(VtColor::Rgb(100, 50, 25)));
    // Palette colors are dimmed through their RGB value too.
    assert_ne!(inactive_color(Some(VtColor::Green), None, None, 0.5), Some(VtColor::Green));
}

#[test]
fn default_colored_cells_use_the_configured_color_then_the_terminals_default() {
    let host = Some(VtColor::Rgb(46, 52, 64));
    let configured = Some(VtColor::Rgb(1, 2, 3));
    assert_eq!(inactive_color(None, configured, host, 0.5), configured);
    assert_eq!(
        inactive_color(None, None, host, 1.0),
        Some(dim_color(VtColor::Rgb(46, 52, 64), 1.0))
    );
    assert_eq!(inactive_color(None, None, None, 0.5), None, "nothing is known about the default");
}

#[test]
fn pane_context_takes_defaults_from_the_active_template_before_the_host() {
    let host = HostColors {
        fg: Some(VtColor::Rgb(10, 10, 10)),
        bg: Some(VtColor::Rgb(20, 20, 20)),
        ..HostColors::default()
    };
    let grid = Grid::new(2, 1);
    let rect = Rect::new(0, 0, 2, 1);
    let mut style = mpx::config::StyleConfig::default();
    let ctx = RenderPaneContext::from_config(
        &PaneColors::new(&style, host), rect, &grid, None, 0, None, true,
    );
    assert_eq!((ctx.default_fg, ctx.default_bg), (host.fg, host.bg));
    assert_eq!(ctx.dim_factor, style.dim_factor);

    style.active.fg = "#d8dee9".into();
    let ctx = RenderPaneContext::from_config(
        &PaneColors::new(&style, host), rect, &grid, None, 0, None, true,
    );
    assert_eq!(ctx.default_fg, Some(VtColor::Rgb(0xd8, 0xde, 0xe9)));
    assert_eq!(ctx.default_bg, host.bg);
}

#[test]
fn dividers_take_the_inactive_background_so_no_stripe_shows_between_panes() {
    let dividers = vec![Divider { is_vertical: false, x: 0, y: 2, length: 4 }];
    let mut canvas = Canvas::default();
    canvas.begin(10, 5, None);
    let mut buf = Vec::new();
    Compositor::draw_dividers(
        &mut canvas,
        &mut buf,
        &dividers,
        None,
        None,
        DividerStyle { bg: Some(VtColor::Rgb(30, 40, 50)), ..DividerStyle::default() },
    )
    .unwrap();
    assert!(String::from_utf8_lossy(&buf).contains(";48;2;30;40;50"));

    let host = HostColors { bg: Some(VtColor::Rgb(46, 52, 64)), ..HostColors::default() };
    let mut style = mpx::config::StyleConfig::default();
    style.dim_factor = 0.5;
    let colors = PaneColors::new(&style, host);
    assert_eq!(colors.inactive_default_bg(), Some(VtColor::Rgb(23, 26, 32)));
    style.inactive.bg = "#102030".into();
    assert_eq!(
        PaneColors::new(&style, host).inactive_default_bg(),
        Some(VtColor::Rgb(0x10, 0x20, 0x30)),
        "a configured inactive background wins"
    );
    assert_eq!(PaneColors::new(&mpx::config::StyleConfig::default(), HostColors::default()).inactive_default_bg(), None);
}

#[test]
fn dimmed_ansi_colors_start_from_the_terminals_own_palette_when_it_reported_one() {
    let mut grid = Grid::new(10, 5);
    grid.current_style.fg = Some(VtColor::Red);
    grid.put_char('R');
    // Without a report the built-in red (205,49,49) is halved.
    let builtin = draw_dimmed_with(&grid, |_| {});
    assert!(builtin.contains(";38;2;103;25;25"), "{builtin:?}");

    let themed = draw_dimmed_with(&grid, |ctx| ctx.palette.set(1, (220, 50, 47)));
    assert!(themed.contains(";38;2;110;25;24"), "{themed:?}");
    // Indexed colors below 16 are the same colors; RGB colors are never remapped.
    assert_eq!(
        from_terminal_palette(VtColor::Indexed(1), &themed_palette()),
        VtColor::Rgb(220, 50, 47)
    );
    assert_eq!(
        from_terminal_palette(VtColor::Rgb(1, 2, 3), &themed_palette()),
        VtColor::Rgb(1, 2, 3)
    );
    assert_eq!(from_terminal_palette(VtColor::Indexed(200), &themed_palette()), VtColor::Indexed(200));
    let mut cube = themed_palette();
    cube.set(200, (1, 2, 3));
    assert_eq!(from_terminal_palette(VtColor::Indexed(200), &cube), VtColor::Rgb(1, 2, 3));
}

fn themed_palette() -> Palette {
    let mut palette = Palette::default();
    palette.set(1, (220, 50, 47));
    palette
}

fn theme_sequence(parts: &str) -> Vec<u8> {
    format!("\x1b]{parts}\x1b\\").into_bytes()
}

#[test]
fn a_theme_switcher_changing_colors_changes_what_dimming_starts_from() {
    let original = HostColors {
        fg: Some(VtColor::Rgb(200, 200, 200)),
        bg: Some(VtColor::Rgb(10, 10, 10)),
        ..HostColors::default()
    };
    let mut host = original;
    assert!(host.follow(&original, &theme_sequence("4;1;rgb:ff/00/00;2;rgb:00/ff/00")));
    assert_eq!(host.palette.get(1), Some((255, 0, 0)));
    assert_eq!(host.palette.get(2), Some((0, 255, 0)));
    assert!(host.follow(&original, &theme_sequence("4;200;rgb:01/02/03")));
    assert_eq!(host.palette.get(200), Some((1, 2, 3)), "the 256-color cube follows too");

    assert!(host.follow(&original, &theme_sequence("10;rgb:11/22/33")));
    assert!(host.follow(&original, b"\x1b]11;rgb:44/55/66\x07"));
    assert_eq!((host.fg, host.bg), (Some(VtColor::Rgb(17, 34, 51)), Some(VtColor::Rgb(68, 85, 102))));

    assert!(!host.follow(&original, &theme_sequence("4;3;?")), "a query changes nothing");
    assert!(!host.follow(&original, &theme_sequence("4;3;rgb:zz")), "neither does a bad color");
    assert!(!host.follow(&original, b"\x1b]0;a title\x07"));
}

#[test]
fn resets_return_to_what_the_terminal_reported_at_attach() {
    let mut original = HostColors {
        fg: Some(VtColor::Rgb(200, 200, 200)),
        bg: Some(VtColor::Rgb(10, 10, 10)),
        ..HostColors::default()
    };
    original.palette.set(1, (205, 49, 49));
    let mut host = original;
    host.follow(&original, &theme_sequence("4;1;rgb:ff/00/00;2;rgb:00/ff/00"));
    host.follow(&original, &theme_sequence("10;rgb:11/22/33"));
    host.follow(&original, &theme_sequence("11;rgb:44/55/66"));

    assert!(host.follow(&original, &theme_sequence("104;1")));
    assert_eq!(host.palette.get(1), Some((205, 49, 49)));
    assert_eq!(host.palette.get(2), Some((0, 255, 0)), "only the named entry resets");
    assert!(host.follow(&original, &theme_sequence("104")));
    assert_eq!(host.palette.get(2), None);
    assert!(host.follow(&original, &theme_sequence("110")));
    assert!(host.follow(&original, &theme_sequence("111")));
    assert_eq!(host, original);
}
