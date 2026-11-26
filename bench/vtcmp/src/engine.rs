//! Adapters that drive each terminal engine and reduce its screen to a common `Snap`.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AColor, NamedColor, Processor};
use mpx::vt::{Color as MColor, VtScreen};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Idx(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sty {
    pub fg: Option<Col>,
    pub bg: Option<Col>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snap {
    pub text: Vec<String>,
    /// The text of each cell, row by row: empty for the right half of a wide character.
    pub cells: Vec<Vec<String>>,
    pub styles: Vec<Vec<Sty>>,
    pub cursor: (usize, usize),
    pub alt: bool,
    pub history: Option<Vec<String>>,
}

pub enum Step {
    Feed(Vec<u8>),
    Resize(usize, usize),
}

pub trait Engine {
    fn new(cols: usize, rows: usize) -> Self
    where
        Self: Sized;
    fn feed(&mut self, bytes: &[u8]);
    fn resize(&mut self, cols: usize, rows: usize);
    fn snap(&self) -> Snap;
}

fn finish(
    rows: Vec<Vec<(String, Sty)>>,
    cursor: (usize, usize),
    alt: bool,
    history: Option<Vec<String>>,
) -> Snap {
    let mut text = Vec::new();
    let mut cells = Vec::new();
    let mut styles = Vec::new();
    for row in rows {
        text.push(
            row.iter()
                .map(|c| c.0.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned(),
        );
        cells.push(row.iter().map(|c| c.0.clone()).collect());
        styles.push(row.into_iter().map(|c| c.1).collect());
    }
    Snap {
        text,
        cells,
        styles,
        cursor,
        alt,
        history,
    }
}

pub struct Mpx(pub VtScreen);

fn mcol(c: MColor) -> Col {
    match c.named_slot() {
        Some(n) => Col::Idx(n),
        None => match c {
            MColor::Indexed(i) => Col::Idx(i),
            MColor::Rgb(r, g, b) => Col::Rgb(r, g, b),
            _ => unreachable!(),
        },
    }
}

impl Engine for Mpx {
    fn new(cols: usize, rows: usize) -> Self {
        Mpx(VtScreen::new(cols, rows))
    }
    fn feed(&mut self, bytes: &[u8]) {
        self.0.feed(bytes);
    }
    fn resize(&mut self, cols: usize, rows: usize) {
        self.0.grid_mut().resize(cols, rows);
    }
    fn snap(&self) -> Snap {
        let g = self.0.grid();
        let rows = g
            .screen_lines()
            .map(|r| {
                r.iter()
                    .map(|c| {
                        let s = c.style;
                        let mut text = String::new();
                        c.push_text(&mut text);
                        (
                            text,
                            Sty {
                                fg: s.fg.map(mcol),
                                bg: s.bg.map(mcol),
                                bold: s.bold,
                                italic: s.italic,
                                underline: s.is_underlined(),
                                reverse: s.reverse,
                            },
                        )
                    })
                    .collect()
            })
            .collect();
        let history = (0..g.history_len())
            .map(|i| {
                let cells = g.line_cells(i).unwrap();
                let mut text = String::new();
                cells.iter().for_each(|c| c.push_text(&mut text));
                text.trim_end().to_owned()
            })
            .collect();
        finish(
            rows,
            (g.cursor.x.min(g.cols - 1), g.cursor.y),
            g.alternate_screen(),
            Some(history),
        )
    }
}

struct Size(usize, usize);
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.1
    }
    fn screen_lines(&self) -> usize {
        self.1
    }
    fn columns(&self) -> usize {
        self.0
    }
}
#[derive(Clone)]
struct Quiet;
impl EventListener for Quiet {
    fn send_event(&self, _: Event) {}
}

pub struct Alac {
    term: Term<Quiet>,
    parser: Processor,
}

fn acol(c: AColor) -> Option<Col> {
    match c {
        AColor::Spec(rgb) => Some(Col::Rgb(rgb.r, rgb.g, rgb.b)),
        AColor::Indexed(i) => Some(Col::Idx(i)),
        AColor::Named(n) => {
            let i = match n {
                NamedColor::Black => 0,
                NamedColor::Red => 1,
                NamedColor::Green => 2,
                NamedColor::Yellow => 3,
                NamedColor::Blue => 4,
                NamedColor::Magenta => 5,
                NamedColor::Cyan => 6,
                NamedColor::White => 7,
                NamedColor::BrightBlack => 8,
                NamedColor::BrightRed => 9,
                NamedColor::BrightGreen => 10,
                NamedColor::BrightYellow => 11,
                NamedColor::BrightBlue => 12,
                NamedColor::BrightMagenta => 13,
                NamedColor::BrightCyan => 14,
                NamedColor::BrightWhite => 15,
                NamedColor::Foreground | NamedColor::Background => return None,
                _ => return None,
            };
            Some(Col::Idx(i))
        }
    }
}

impl Engine for Alac {
    fn new(cols: usize, rows: usize) -> Self {
        Alac {
            term: Term::new(Config::default(), &Size(cols, rows), Quiet),
            parser: Processor::new(),
        }
    }
    fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }
    fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(Size(cols, rows));
    }
    fn snap(&self) -> Snap {
        let t = &self.term;
        let g = t.grid();
        let cell_of = |line: i32, x: usize| {
            let c = &g[Point::new(Line(line), Column(x))];
            if c.flags.contains(Flags::WIDE_CHAR_SPACER) {
                return (String::new(), Sty::default());
            }
            let mut s = c.c.to_string();
            if let Some(extra) = c.zerowidth() {
                s.extend(extra.iter());
            }
            let f = c.flags;
            (
                s,
                Sty {
                    fg: acol(c.fg),
                    bg: acol(c.bg),
                    bold: f.contains(Flags::BOLD),
                    italic: f.contains(Flags::ITALIC),
                    underline: f.intersects(Flags::ALL_UNDERLINES),
                    reverse: f.contains(Flags::INVERSE),
                },
            )
        };
        let rows = (0..g.screen_lines() as i32)
            .map(|y| (0..g.columns()).map(|x| cell_of(y, x)).collect())
            .collect();
        let history = (1..=g.history_size() as i32)
            .rev()
            .map(|y| {
                (0..g.columns())
                    .map(|x| cell_of(-y, x).0)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect();
        let p = g.cursor.point;
        finish(
            rows,
            (p.column.0.min(g.columns() - 1), p.line.0 as usize),
            t.mode().contains(TermMode::ALT_SCREEN),
            Some(history),
        )
    }
}

pub struct Vt100(vt100::Parser);

fn vcol(c: vt100::Color) -> Option<Col> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(Col::Idx(i)),
        vt100::Color::Rgb(r, g, b) => Some(Col::Rgb(r, g, b)),
    }
}

impl Engine for Vt100 {
    fn new(cols: usize, rows: usize) -> Self {
        Vt100(vt100::Parser::new(rows as u16, cols as u16, 0))
    }
    fn feed(&mut self, bytes: &[u8]) {
        self.0.process(bytes);
    }
    fn resize(&mut self, cols: usize, rows: usize) {
        self.0.screen_mut().set_size(rows as u16, cols as u16);
    }
    fn snap(&self) -> Snap {
        let s = self.0.screen();
        let (rows_n, cols_n) = s.size();
        let rows = (0..rows_n)
            .map(|y| {
                (0..cols_n)
                    .map(|x| {
                        let c = s.cell(y, x).unwrap();
                        if c.is_wide_continuation() {
                            return (String::new(), Sty::default());
                        }
                        let t = if c.has_contents() {
                            c.contents().to_string()
                        } else {
                            " ".into()
                        };
                        (
                            t,
                            Sty {
                                fg: vcol(c.fgcolor()),
                                bg: vcol(c.bgcolor()),
                                bold: c.bold(),
                                italic: c.italic(),
                                underline: c.underline(),
                                reverse: c.inverse(),
                            },
                        )
                    })
                    .collect()
            })
            .collect();
        let (cy, cx) = s.cursor_position();
        finish(
            rows,
            ((cx as usize).min(cols_n as usize - 1), cy as usize),
            s.alternate_screen(),
            None,
        )
    }
}
