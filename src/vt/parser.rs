use std::borrow::Cow;
use vte::{Params, Perform};

use super::grid::{Cell, CellStyle, Color, Grid, ShellMark, Underline};
use super::link::LinkId;
use crate::keys::TermModes;

pub struct VtHandler {
    pub grid: Grid,
    pub responses: Vec<Vec<u8>>,
    pub passthroughs: Vec<Vec<u8>>,
    pub bg_color: Option<String>,
    modes: TermModes,
    /// Kitty keyboard flags the program pushed (`CSI > n u`), restored by `CSI < n u`.
    kitty_stack: Vec<u8>,
    /// A synchronized update (DEC 2026) the program has begun and not ended.
    sync_output: bool,
    title: Option<String>,
    alerts: Vec<Alert>,
    last_char: Option<char>,
    dcs_buf: Option<Vec<u8>>,
}

impl VtHandler {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            responses: Vec::new(),
            passthroughs: Vec::new(),
            bg_color: None,
            modes: TermModes::default(),
            kitty_stack: Vec::new(),
            sync_output: false,
            title: None,
            alerts: Vec::new(),
            last_char: None,
            dcs_buf: None,
        }
    }

    fn handle_sgr(&mut self, params: &Params) {
        if params.is_empty() {
            self.grid.current_style = CellStyle::default();
            return;
        }

        let mut iter = params.iter();
        while let Some(sub) = iter.next() {
            let code = sub[0];
            let style = &mut self.grid.current_style;
            match code {
                38 => {
                    if let Some(color) = parse_extended_color(sub, &mut iter) {
                        style.fg = Some(color);
                    }
                }
                48 => {
                    if let Some(color) = parse_extended_color(sub, &mut iter) {
                        style.bg = Some(color);
                    }
                }
                58 => {
                    if let Some(color) = underline_color(sub, &mut iter) {
                        style.underline_color = Some(color);
                    }
                }
                _ => apply_sgr_code(style, code, sub),
            }
        }
    }

    fn handle_mode(&mut self, params: &Params, set: bool) {
        for param in params.iter() {
            match param[0] {
                1 => self.modes.app_cursor = set,
                25 => self.grid.cursor.visible = set,
                1049 | 1047 | 47 => {
                    self.grid.set_alternate_screen(set);
                    self.grid.clear_screen(2);
                }
                69 => self.grid.set_lr_margins_mode(set),
                1000 => {
                    self.modes.mouse_tracking = if set {
                        crate::keys::MouseTracking::X10
                    } else {
                        crate::keys::MouseTracking::None
                    };
                }
                1002 => {
                    self.modes.mouse_tracking = if set {
                        crate::keys::MouseTracking::ButtonEvent
                    } else {
                        crate::keys::MouseTracking::None
                    };
                }
                1003 => {
                    self.modes.mouse_tracking = if set {
                        crate::keys::MouseTracking::AnyEvent
                    } else {
                        crate::keys::MouseTracking::None
                    };
                }
                1006 => self.modes.mouse_sgr = set,
                1004 => self.modes.focus_events = set,
                2004 => self.modes.bracketed_paste = set,
                2026 => self.sync_output = set,
                _ => {}
            }
        }
    }

    fn push_notify(&mut self, text: String) {
        if !text.is_empty() {
            self.alerts.push(Alert::Notify(text));
        }
    }

    fn cursor_up(&mut self, n: usize) {
        let grid = &mut self.grid;
        let floor = if grid.cursor.y >= grid.scroll_top {
            grid.scroll_top
        } else {
            0
        };
        grid.cursor.y = grid.cursor.y.saturating_sub(n).max(floor);
    }

    fn cursor_down(&mut self, n: usize) {
        let grid = &mut self.grid;
        let ceil = if grid.cursor.y <= grid.scroll_bottom {
            grid.scroll_bottom
        } else {
            grid.rows - 1
        };
        grid.cursor.y = (grid.cursor.y + n).min(ceil);
    }

    fn reverse_index(&mut self) {
        if self.grid.cursor.y <= self.grid.scroll_top {
            self.grid.scroll_down(1);
        } else {
            self.grid.cursor.y -= 1;
        }
    }

    fn full_reset(&mut self) {
        let history = std::mem::take(&mut self.grid.history);
        let limit = self.grid.history_limit;
        self.grid = Grid::new(self.grid.cols, self.grid.rows);
        self.grid.history = history;
        self.grid.history_limit = limit;
        self.modes = TermModes::default();
        self.kitty_stack.clear();
        self.sync_output = false;
        self.title = None;
        self.bg_color = None;
        self.dcs_buf = None;
        self.last_char = None;
    }

    /// CSI forms that set input modes: xterm modifyOtherKeys (`CSI > 4 ; n m`) and the kitty
    /// keyboard protocol (`CSI > n u`, `CSI < n u`, `CSI = n ; m u`, `CSI ? u`). True when the
    /// sequence was one of them.
    fn keyboard_csi(&mut self, params: &Params, intermediates: &[u8], action: char) -> bool {
        let arg = |i: usize, default: u16| params.iter().nth(i).map_or(default, |p| p[0]);
        match (intermediates, action) {
            ([b'>'], 'm') => {
                if arg(0, 0) == 4 {
                    self.modes.modify_other_keys = arg(1, 0).min(2) as u8;
                }
                true
            }
            ([b'>'], 'u') => {
                if self.kitty_stack.len() == KITTY_STACK_DEPTH {
                    self.kitty_stack.remove(0);
                }
                self.kitty_stack.push(self.modes.kitty_keys);
                self.modes.kitty_keys = kitty_flags(arg(0, 0));
                true
            }
            ([b'<'], 'u') => {
                for _ in 0..arg(0, 1).max(1) {
                    self.modes.kitty_keys = self.kitty_stack.pop().unwrap_or(0);
                }
                true
            }
            ([b'='], 'u') => {
                let flags = kitty_flags(arg(0, 0));
                let current = self.modes.kitty_keys;
                self.modes.kitty_keys = match arg(1, 1) {
                    2 => current | flags,
                    3 => current & !flags,
                    _ => flags,
                };
                true
            }
            ([b'?'], 'u') => {
                self.responses
                    .push(format!("\x1b[?{}u", self.modes.kitty_keys).into_bytes());
                true
            }
            _ => false,
        }
    }

    /// OSC 8: the text printed from now links to `rest` (`params;uri`); an empty URI ends the link.
    fn set_hyperlink(&mut self, rest: &[&[u8]]) {
        let uri_empty = rest.get(1).is_none_or(|uri| uri.is_empty());
        self.grid.current_style.link = if uri_empty {
            None
        } else {
            LinkId::intern(&join_osc(rest))
        };
    }

    fn handle_csi_cursor_edit(&mut self, action: char, params: &Params) {
        let n = count_param(params);
        match action {
            'G' | '`' => {
                let y = self.grid.cursor.y;
                self.grid.move_cursor(n - 1, y);
            }
            'd' => {
                let x = self.grid.cursor.x;
                self.grid.move_cursor(x, n - 1);
            }
            'a' => {
                let x = self.grid.cursor.x + n;
                let y = self.grid.cursor.y;
                self.grid.move_cursor(x, y);
            }
            'E' => {
                self.cursor_down(n);
                self.grid.cursor.x = 0;
            }
            'F' => {
                self.cursor_up(n);
                self.grid.cursor.x = 0;
            }
            'Z' => {
                for _ in 0..n {
                    self.grid.cursor.x = back_tab(self.grid.cursor.x);
                }
            }
            'X' => self.grid.erase_chars(n),
            '@' => self.grid.insert_blanks(n),
            'P' => self.grid.delete_chars(n),
            'b' => {
                if let Some(c) = self.last_char {
                    for _ in 0..n {
                        self.grid.put_char(c);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The colour a 58 underline selector sets, in colon or semicolon form.
fn underline_color<'a>(sub: &[u16], iter: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    if sub.len() > 1 {
        colon_extended_color(sub)
    } else {
        semicolon_extended_color(iter)
    }
}

/// The underline style of SGR 4 (`4:n`); a bare 4 is a single underline, `4:0` none.
fn underline_style(sub: &[u16]) -> Underline {
    match sub.get(1) {
        None | Some(1) => Underline::Single,
        Some(0) => Underline::None,
        Some(2) => Underline::Double,
        Some(3) => Underline::Curly,
        Some(4) => Underline::Dotted,
        Some(5) => Underline::Dashed,
        Some(_) => Underline::Single,
    }
}

/// The kitty keyboard flags a program sets: the five defined bits, nothing else.
fn kitty_flags(value: u16) -> u8 {
    (value & 0x1f) as u8
}

/// Kitty keyboard pushes kept; the oldest is dropped past this, as terminals bound their stacks.
const KITTY_STACK_DEPTH: usize = 16;

const ANSI_COLORS: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::White,
    Color::BrightBlack,
    Color::BrightRed,
    Color::BrightGreen,
    Color::BrightYellow,
    Color::BrightBlue,
    Color::BrightMagenta,
    Color::BrightCyan,
    Color::BrightWhite,
];

/// Applies one SGR code other than the extended colour selectors (38, 48, 58).
fn apply_sgr_code(style: &mut CellStyle, code: u16, sub: &[u16]) {
    match code {
        0 | 22..=24 | 27 => reset_attribute(style, code),
        1..=4 | 7 | 21 => set_attribute(style, code, sub),
        30..=37 | 39 | 90..=97 => style.fg = basic_color(code),
        40..=47 | 49 | 100..=107 => style.bg = basic_color(code),
        59 => style.underline_color = None,
        _ => {}
    }
}

fn set_attribute(style: &mut CellStyle, code: u16, sub: &[u16]) {
    match code {
        1 => style.bold = true,
        2 => style.faint = true,
        3 => style.italic = true,
        4 => style.underline = underline_style(sub),
        7 => style.reverse = true,
        21 => style.underline = Underline::Double,
        _ => {}
    }
}

fn reset_attribute(style: &mut CellStyle, code: u16) {
    match code {
        0 => *style = CellStyle::default(),
        22 => {
            style.bold = false;
            style.faint = false;
        }
        23 => style.italic = false,
        24 => style.underline = Underline::None,
        27 => style.reverse = false,
        _ => {}
    }
}

/// Maps the basic and bright colour codes (30-37, 40-47, 90-97, 100-107) to a colour.
fn basic_color(code: u16) -> Option<Color> {
    match code {
        30..=37 => Some(ANSI_COLORS[(code - 30) as usize]),
        40..=47 => Some(ANSI_COLORS[(code - 40) as usize]),
        90..=97 => Some(ANSI_COLORS[(code - 82) as usize]),
        100..=107 => Some(ANSI_COLORS[(code - 92) as usize]),
        _ => None,
    }
}

/// Parses the colour following a 38/48 selector, in colon or semicolon form.
fn parse_extended_color<'a>(
    sub: &[u16],
    iter: &mut impl Iterator<Item = &'a [u16]>,
) -> Option<Color> {
    colon_extended_color(sub).or_else(|| semicolon_extended_color(iter))
}

fn colon_extended_color(sub: &[u16]) -> Option<Color> {
    if sub.len() >= 5 && sub[1] == 2 {
        let (r, g, b) = if sub.len() >= 6 {
            (sub[3] as u8, sub[4] as u8, sub[5] as u8)
        } else {
            (sub[2] as u8, sub[3] as u8, sub[4] as u8)
        };
        Some(Color::Rgb(r, g, b))
    } else if sub.len() >= 3 && sub[1] == 5 {
        Some(Color::Indexed(sub[2] as u8))
    } else {
        None
    }
}

fn semicolon_extended_color<'a>(iter: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    let sub = iter.next()?;
    match sub[0] {
        5 => iter.next().map(|c| Color::Indexed(c[0] as u8)),
        2 => match (iter.next(), iter.next(), iter.next()) {
            (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r[0] as u8, g[0] as u8, b[0] as u8)),
            _ => None,
        },
        _ => None,
    }
}

const MAX_DCS_LEN: usize = 1024 * 1024;

fn unescape_tmux_dcs(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len());
    let mut i = 0;
    while i < payload.len() {
        if payload[i] == 0x1b && i + 1 < payload.len() && payload[i + 1] == 0x1b {
            out.push(0x1b);
            i += 2;
        } else {
            out.push(payload[i]);
            i += 1;
        }
    }
    out
}

fn format_osc(params: &[&[u8]], bell_terminated: bool) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(b"\x1b]");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            buf.push(b';');
        }
        buf.extend_from_slice(p);
    }
    if bell_terminated {
        buf.push(0x07);
    } else {
        buf.extend_from_slice(b"\x1b\\");
    }
    buf
}

fn join_osc(parts: &[&[u8]]) -> String {
    parts
        .iter()
        .map(|part| String::from_utf8_lossy(part))
        .collect::<Vec<_>>()
        .join(";")
}

fn count_param(params: &Params) -> usize {
    params
        .iter()
        .next()
        .map(|p| p[0] as usize)
        .unwrap_or(1)
        .max(1)
}

fn back_tab(x: usize) -> usize {
    x.saturating_sub(1) / 8 * 8
}

impl Perform for VtHandler {
    fn print(&mut self, c: char) {
        self.last_char = Some(c);
        self.grid.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0B | 0x0C => self.grid.newline(),
            b'\r' => self.grid.carriage_return(),
            0x07 => self.alerts.push(Alert::Bell),
            0x08 => self.grid.backspace(),
            b'\t' => {
                let next_tab = (self.grid.cursor.x / 8 + 1) * 8;
                self.grid.cursor.x = next_tab.min(self.grid.cols.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        if self.keyboard_csi(params, intermediates, action) {
            return;
        }
        // `CSI > 4;2 m` (modifyOtherKeys), `CSI > 1 u` (kitty keyboard), `CSI ? 6 n` and similar
        // private forms share finals with SGR, cursor restore and DSR but mean something else.
        if !intermediates.is_empty() && !matches!(action, 'c' | 'h' | 'l') {
            return;
        }
        match action {
            'H' | 'f' => {
                let row = params.iter().next().map(|p| p[0] as usize).unwrap_or(1);
                let col = params.iter().nth(1).map(|p| p[0] as usize).unwrap_or(1);
                let y = row.saturating_sub(1);
                let x = col.saturating_sub(1);
                self.grid.move_cursor(x, y);
            }
            'A' => self.cursor_up(count_param(params)),
            'B' => self.cursor_down(count_param(params)),
            'C' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.cursor.x =
                    (self.grid.cursor.x + count).min(self.grid.cols.saturating_sub(1));
            }
            'D' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.cursor.x = self.grid.cursor.x.saturating_sub(count);
            }
            'J' => {
                let mode = params.iter().next().map(|p| p[0] as usize).unwrap_or(0);
                self.grid.clear_screen(mode);
            }
            'K' => {
                let mode = params.iter().next().map(|p| p[0] as usize).unwrap_or(0);
                self.grid.clear_line(mode);
            }
            'm' => self.handle_sgr(params),
            // With DECLRMM set, `CSI s` is DECSLRM (left/right margins), not save-cursor.
            's' if self.grid.lr_margins => {
                let value = |i: usize| {
                    params
                        .iter()
                        .nth(i)
                        .map(|p| p[0] as usize)
                        .filter(|&v| v > 0)
                };
                self.grid.set_lr_margins(value(0), value(1));
            }
            's' => {
                self.grid.saved_cursor = Some(self.grid.cursor);
            }
            'u' => {
                if let Some(c) = self.grid.saved_cursor {
                    self.grid.cursor = c;
                }
            }
            'n' => {
                let code = params.iter().next().map(|p| p[0]).unwrap_or(0);
                if code == 6 {
                    // Cursor Position Report (DSR): ESC [ row ; col R
                    let r = self.grid.cursor.y + 1;
                    let c = self.grid.cursor.x + 1;
                    self.responses
                        .push(format!("\x1b[{};{}R", r, c).into_bytes());
                } else if code == 5 {
                    // Terminal status OK
                    self.responses.push(b"\x1b[0n".to_vec());
                }
            }
            'c' => {
                if intermediates == b">" {
                    // Secondary Device Attributes
                    self.responses.push(b"\x1b[>0;10;0c".to_vec());
                } else {
                    // Primary Device Attributes (VT220 compatible)
                    self.responses.push(b"\x1b[?62;c".to_vec());
                }
            }
            'h' => {
                if intermediates == b"?" {
                    self.handle_mode(params, true);
                }
            }
            'l' => {
                if intermediates == b"?" {
                    self.handle_mode(params, false);
                }
            }
            'r' => handle_set_margins(&mut self.grid, params),
            'S' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.scroll_up(count);
            }
            'T' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.scroll_down(count);
            }
            'M' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.delete_lines(count);
            }
            'L' => {
                let count = params
                    .iter()
                    .next()
                    .map(|p| p[0] as usize)
                    .unwrap_or(1)
                    .max(1);
                self.grid.insert_lines(count);
            }
            _ => self.handle_csi_cursor_edit(action, params),
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let buf = vec![action as u8];
        self.dcs_buf = Some(buf);
    }

    fn put(&mut self, byte: u8) {
        if let Some(buf) = &mut self.dcs_buf
            && buf.len() < MAX_DCS_LEN
        {
            buf.push(byte);
        }
    }

    fn unhook(&mut self) {
        if let Some(buf) = self.dcs_buf.take() {
            let payload = buf
                .strip_prefix(b"tmux;")
                .or_else(|| buf.strip_prefix(b"mpx;"));
            if let Some(payload) = payload {
                let unescaped = unescape_tmux_dcs(payload);
                if !unescaped.is_empty() {
                    self.passthroughs.push(unescaped);
                }
            }
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        match params {
            [b"0" | b"2", rest @ ..] => self.title = Some(join_osc(rest)),
            [b"133", b"A", ..] => self.grid.mark_shell(ShellMark::Prompt),
            [b"133", b"C", ..] => self.grid.mark_shell(ShellMark::Output),
            [b"8", rest @ ..] => self.set_hyperlink(rest),
            // OSC 9;4 is ConEmu/Windows Terminal progress reporting, not a notification.
            [b"9", b"4", ..] => {}
            [b"9", rest @ ..] => self.push_notify(join_osc(rest)),
            [b"777", b"notify", title, body @ ..] => {
                let title = String::from_utf8_lossy(title).into_owned();
                let body = join_osc(body);
                let text = if body.is_empty() {
                    title
                } else {
                    format!("{title}: {body}")
                };
                self.push_notify(text);
            }
            // Respond to background color query
            [b"11", b"?", ..] => {
                let bg = self.bg_color.as_deref().unwrap_or("rgb:0000/0000/0000");
                self.responses
                    .push(format!("\x1b]11;{bg}\x1b\\").into_bytes());
            }
            // Background color set: save for queries and pass through
            [b"11", color, ..] => {
                self.bg_color = Some(String::from_utf8_lossy(color).into_owned());
                self.passthroughs
                    .push(format_osc(params, bell_terminated));
            }
            // Palette (4), Foreground (10), Cursor (12), Clipboard (52), Resets (104, 110, 111, 112)
            [b"4" | b"10" | b"12" | b"52" | b"104" | b"110" | b"111" | b"112", ..] => {
                if params[0] == b"111" {
                    self.bg_color = None;
                }
                self.passthroughs
                    .push(format_osc(params, bell_terminated));
            }
            // iTerm2 custom sequences (e.g. OSC Pg... for colors)
            [first, ..] if first.starts_with(b"P") => {
                self.passthroughs
                    .push(format_osc(params, bell_terminated));
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates, byte) {
            ([], b'7') => self.grid.save_cursor(),
            ([], b'8') => self.grid.restore_cursor(),
            ([], b'c') => self.full_reset(),
            ([], b'M') => self.reverse_index(),
            _ => {}
        }
    }
}

fn handle_set_margins(grid: &mut Grid, params: &Params) {
    let top = params
        .iter()
        .next()
        .map(|p| p[0] as usize)
        .unwrap_or(1)
        .max(1);
    let bottom = params
        .iter()
        .nth(1)
        .map(|p| p[0] as usize)
        .unwrap_or(grid.rows);
    let top = top.saturating_sub(1);
    let bottom = bottom.saturating_sub(1).min(grid.rows.saturating_sub(1));
    if top < bottom {
        grid.scroll_top = top;
        grid.scroll_bottom = bottom;
    }
    grid.move_cursor(0, 0);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    Bell,
    Notify(String),
}

pub struct VtScreen {
    parser: vte::Parser,
    handler: VtHandler,
}

impl VtScreen {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            parser: vte::Parser::new(),
            handler: VtHandler::new(cols, rows),
        }
    }

    /// A screen that keeps no scrollback: what a live pane shows. Scrollback lives in the pane's
    /// retained output and is rebuilt from it on demand, so floods cost no history work.
    pub fn live(cols: usize, rows: usize) -> Self {
        let mut screen = Self::new(cols, rows);
        screen.grid_mut().set_history_limit(0);
        screen
    }

    /// The grid a pane has after it resizes from `from` to `to` (cols, rows), rebuilt from `output`,
    /// the pane's raw log. A height change replays the log at the old size with its scrollback and
    /// resizes the grid as `Grid::resize` does, so rows that the grow pulls back from history come
    /// back on top: every end that rebuilds the same log at the same sizes gets the same screen.
    /// A width change replays at the new width instead, since the lines reflow. The scrollback is
    /// kept only when `keep_history` (a pane in copy mode).
    pub fn resized_grid(
        output: &[u8],
        from: (usize, usize),
        to: (usize, usize),
        keep_history: bool,
    ) -> Grid {
        let replay_at = if from.0 == to.0 { from } else { to };
        let mut screen = Self::new(replay_at.0, replay_at.1);
        screen.feed(output);
        let _ = screen.take_responses();
        let _ = screen.take_passthroughs();
        let grid = screen.grid_mut();
        grid.resize(to.0, to.1);
        if !keep_history {
            grid.set_history_limit(0);
        }
        let VtScreen { handler, .. } = screen;
        handler.grid
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.handler, bytes);
    }

    pub fn take_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.handler.responses)
    }

    pub fn snapshot(&self) -> String {
        self.handler.grid.render_plain()
    }

    pub fn grid(&self) -> &Grid {
        &self.handler.grid
    }

    pub fn grid_mut(&mut self) -> &mut Grid {
        &mut self.handler.grid
    }

    /// Input modes the program requested (DECCKM, bracketed paste, focus events).
    pub fn modes(&self) -> TermModes {
        self.handler.modes
    }

    /// Takes on input modes set before the output this screen was fed, which it could not see.
    pub fn set_modes(&mut self, modes: TermModes) {
        self.handler.modes = modes;
    }

    /// Whether the program has begun a synchronized update (DEC 2026) that it has not ended.
    pub fn sync_output(&self) -> bool {
        self.handler.sync_output
    }

    /// Latest OSC 0/2 window title.
    pub fn title(&self) -> Option<&str> {
        self.handler.title.as_deref()
    }

    /// Drains BEL and OSC 9 / OSC 777 notifications received since the last call.
    pub fn take_alerts(&mut self) -> Vec<Alert> {
        std::mem::take(&mut self.handler.alerts)
    }

    /// Drains passthrough sequences (DCS tmux, OSC 4/10/11/12/52) received since the last call.
    pub fn take_passthroughs(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.handler.passthroughs)
    }

    /// Latest background color set via OSC 11.
    pub fn bg_color(&self) -> Option<&str> {
        self.handler.bg_color.as_deref()
    }

    /// Scrollback history plus the visible screen as plain text, trailing blank lines trimmed;
    /// `tail` keeps only the last N lines.
    pub fn scrollback_text(&self, tail: Option<usize>) -> String {
        if tail == Some(0) {
            return String::new();
        }
        let grid = &self.handler.grid;
        let all_rows = grid
            .history
            .iter()
            .map(|row| Cow::Owned(row.to_cells(0)))
            .chain(grid.screen_lines().map(Cow::Borrowed));

        let non_blank_from_end = all_rows.rev().skip_while(|row| is_blank_row(row));

        let rows: Vec<Cow<'_, [Cell]>> = match tail {
            Some(n) => non_blank_from_end.take(n).collect(),
            None => non_blank_from_end.collect(),
        };

        let lines: Vec<String> = rows.iter().rev().map(|row| row_text(row)).collect();
        lines.join("\n")
    }
}

fn is_blank_row(row: &[Cell]) -> bool {
    row.iter().all(|cell| cell.c.is_whitespace())
}

fn row_text(row: &[Cell]) -> String {
    row.iter()
        .map(|cell| cell.c)
        .collect::<String>()
        .trim_end()
        .to_owned()
}
