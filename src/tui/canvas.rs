//! The layout area as last sent to the terminal, so each frame emits only the cells that changed.
use std::io::{self, Write};

use crate::layout::geometry::Rect;
use crate::vt::{Cell, CellStyle, Color};

/// Shadow of the layout area. `None` marks a cell whose on-screen content is unknown (never drawn,
/// or hidden under an overlay), which forces the next frame to paint it.
#[derive(Debug, Default)]
pub struct Canvas {
    width: u16,
    height: u16,
    front: Vec<Option<Cell>>,
    occluded: Option<Rect>,
    cursor: Option<(u16, u16)>,
    style: Option<CellStyle>,
    status: Vec<u8>,
    lr_margins: bool,
}

/// Characters known to take one column: ASCII, and the box drawing and block elements mpx draws its
/// own borders and dividers with (the layout already assumes those are narrow).
fn narrow(c: char) -> bool {
    c.is_ascii() || ('\u{2500}'..='\u{259F}').contains(&c)
}

/// Whether two rectangles share a cell.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

impl Canvas {
    /// Starts a frame over a `width` x `height` area; a size change forgets everything on screen.
    /// Cells inside `occluded` are covered by chrome drawn later in the frame and are not painted.
    pub fn begin(&mut self, width: u16, height: u16, occluded: Option<Rect>) {
        if (width, height) != (self.width, self.height) {
            self.width = width;
            self.height = height;
            self.front = vec![None; width as usize * height as usize];
            self.status.clear();
        }
        self.occluded = occluded;
        self.cursor = None;
        self.style = None;
    }

    /// Forgets what the terminal shows, so the next frame repaints every cell and the status row.
    pub fn invalidate(&mut self) {
        self.front.fill(None);
        self.status.clear();
    }

    /// Whether the terminal supports left/right scroll margins (DECLRMM), so regions narrower than
    /// the screen can be scrolled too.
    pub fn set_lr_margins(&mut self, supported: bool) {
        self.lr_margins = supported;
    }

    /// Scrolls `region` by `lines` (up when positive) on the terminal and in the shadow, so content
    /// that only moved is not sent again; rows scrolled in are blank. A region narrower than the
    /// screen needs left/right margins, and only ASCII known on screen in and beside it: the screen
    /// model has no width table to say where a wide glyph near a margin really sits. Returns false,
    /// doing nothing, when chrome covers the region, the move is degenerate or cannot be done.
    pub fn scroll<W: Write>(&mut self, out: &mut W, region: Rect, lines: isize) -> io::Result<bool> {
        let n = lines.unsigned_abs();
        let fits = region.height >= 2
            && region.y + region.height <= self.height
            && region.x + region.width <= self.width
            && n < region.height as usize;
        let covered = self.occluded.is_some_and(|r| overlaps(r, region));
        if lines == 0 || !fits || covered {
            return Ok(false);
        }
        let full = region.x == 0 && region.width == self.width;
        if !full && !(self.lr_margins && self.plain_around(region)) {
            return Ok(false);
        }
        let (top, bottom) = (region.y + 1, region.y + region.height);
        let verb = if lines > 0 { 'S' } else { 'T' };
        // Default attributes first: the terminal fills scrolled-in cells with the current background.
        if full {
            write!(out, "\x1b[0m\x1b[{top};{bottom}r\x1b[{n}{verb}\x1b[r")?;
        } else {
            let (left, right) = (region.x + 1, region.x + region.width);
            write!(
                out,
                "\x1b[0m\x1b[?69h\x1b[{top};{bottom}r\x1b[{left};{right}s\x1b[{n}{verb}\x1b[s\x1b[?69l\x1b[r"
            )?;
        }
        self.style = None;
        self.cursor = None;
        self.shift(region, lines);
        Ok(true)
    }

    /// Moves the shadow of `region` as the terminal just did; scrolled-in cells are known blanks.
    fn shift(&mut self, region: Rect, lines: isize) {
        let n = lines.unsigned_abs();
        let (top, bottom) = (region.y as usize, (region.y + region.height - 1) as usize);
        let cols = region.x as usize..(region.x + region.width) as usize;
        let rows: Vec<usize> = if lines > 0 {
            (top..=bottom).collect()
        } else {
            (top..=bottom).rev().collect()
        };
        let width = self.width as usize;
        for y in rows {
            let from = if lines > 0 {
                Some(y + n).filter(|&f| f <= bottom)
            } else {
                y.checked_sub(n).filter(|&f| f >= top)
            };
            let dst = y * width;
            match from {
                Some(f) => self
                    .front
                    .copy_within(f * width + cols.start..f * width + cols.end, dst + cols.start),
                None => self.front[dst + cols.start..dst + cols.end].fill(Some(Cell::default())),
            }
        }
    }

    /// Every cell of `region`'s rows from the column before it to the column after it is known to
    /// the shadow and narrow.
    fn plain_around(&self, region: Rect) -> bool {
        let from = region.x.saturating_sub(1) as usize;
        let to = ((region.x + region.width) as usize + 1).min(self.width as usize);
        let width = self.width as usize;
        (region.y..region.y + region.height).all(|y| {
            let row = y as usize * width;
            self.front[row + from..row + to]
                .iter()
                .all(|cell| cell.is_some_and(|c| narrow(c.c)))
        })
    }

    /// Writes `cell` at `(x, y)` unless the terminal already shows it there.
    pub fn put<W: Write>(&mut self, out: &mut W, x: u16, y: u16, cell: Cell) -> io::Result<()> {
        if x >= self.width || y >= self.height {
            return Ok(());
        }
        let i = self.index(x, y);
        if self.occluded.is_some_and(|r| r.contains(x, y)) {
            self.front[i] = None;
            return Ok(());
        }
        if self.front[i] == Some(cell) {
            return Ok(());
        }
        if self.cursor != Some((x, y)) && !self.bridge(out, x, y)? {
            write!(out, "\x1b[{};{}H", y + 1, x + 1)?;
        }
        self.write_cell(out, cell)?;
        // The grid has no width model: after a non-ASCII glyph the terminal's column is not known.
        self.cursor = (cell.c.is_ascii() && x + 1 < self.width).then_some((x + 1, y));
        self.front[i] = Some(cell);
        Ok(())
    }

    /// Moves the cursor right to `(x, y)` by reprinting the cells in between, when the terminal
    /// already shows them as plain ASCII in the current style and that is shorter than a cursor
    /// move: a new line in text whose spaces match the blank row under it is then sent as one run
    /// rather than a jump per word.
    fn bridge<W: Write>(&mut self, out: &mut W, x: u16, y: u16) -> io::Result<bool> {
        let Some((from, row)) = self.cursor else {
            return Ok(false);
        };
        let jump = 4 + digits(y + 1) + digits(x + 1);
        if row != y || from >= x || usize::from(x - from) > jump {
            return Ok(false);
        }
        let start = self.index(from, y);
        let gap = &self.front[start..start + usize::from(x - from)];
        let plain = |cell: &Option<Cell>| {
            cell.is_some_and(|c| c.c.is_ascii() && Some(c.style) == self.style)
        };
        let hidden = self
            .occluded
            .is_some_and(|r| (from..x).any(|gx| r.contains(gx, y)));
        if hidden || !gap.iter().all(plain) {
            return Ok(false);
        }
        let text: Vec<u8> = gap.iter().flatten().map(|c| c.c as u8).collect();
        out.write_all(&text)?;
        Ok(true)
    }

    /// Writes a row of pane cells starting at `(x, y)`. A row holding non-ASCII glyphs, now or as last
    /// sent, is reprinted whole and in sequence when anything in it changed or chrome covers part of it:
    /// the grid has no width model, so where a wide glyph pushes later cells is left to the terminal, as
    /// in a full repaint. Whatever such a row may spill over to its right is forgotten, to be repainted.
    pub fn put_row<W: Write>(
        &mut self,
        out: &mut W,
        x: u16,
        y: u16,
        cells: &[Cell],
    ) -> io::Result<()> {
        if x >= self.width || y >= self.height {
            return Ok(());
        }
        let cells = &cells[..cells.len().min((self.width - x) as usize)];
        let start = self.index(x, y);
        let shown = &self.front[start..start + cells.len()];
        let maybe_wide = cells
            .iter()
            .chain(shown.iter().flatten())
            .any(|cell| !cell.c.is_ascii());
        if !maybe_wide {
            for (dx, cell) in cells.iter().enumerate() {
                self.put(out, x + dx as u16, y, *cell)?;
            }
            return Ok(());
        }
        let end = x + cells.len() as u16;
        let covered = self
            .occluded
            .is_some_and(|r| y >= r.y && y < r.y + r.height && x < r.x + r.width && r.x < end);
        if !covered
            && shown
                .iter()
                .zip(cells)
                .all(|(was, cell)| *was == Some(*cell))
        {
            return Ok(());
        }
        self.print_row(out, x, y, cells)
    }

    fn print_row<W: Write>(
        &mut self,
        out: &mut W,
        x: u16,
        y: u16,
        cells: &[Cell],
    ) -> io::Result<()> {
        write!(out, "\x1b[{};{}H", y + 1, x + 1)?;
        let start = self.index(x, y);
        for (dx, cell) in cells.iter().enumerate() {
            self.write_cell(out, *cell)?;
            let hidden = self.occluded.is_some_and(|r| r.contains(x + dx as u16, y));
            self.front[start + dx] = (!hidden).then_some(*cell);
        }
        self.cursor = None;
        let row_end = self.index(0, y) + self.width as usize;
        self.front[start + cells.len()..row_end].fill(None);
        Ok(())
    }

    fn write_cell<W: Write>(&mut self, out: &mut W, cell: Cell) -> io::Result<()> {
        if self.style != Some(cell.style) {
            write_sgr(out, &cell.style)?;
            self.style = Some(cell.style);
        }
        let mut utf8 = [0u8; 4];
        out.write_all(cell.c.encode_utf8(&mut utf8).as_bytes())
    }

    fn index(&self, x: u16, y: u16) -> usize {
        y as usize * self.width as usize + x as usize
    }

    /// Ends the frame's cell writes on default attributes, so chrome drawn next starts clean.
    pub fn finish<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        if self.style.is_some_and(|s| s != CellStyle::default()) {
            out.write_all(b"\x1b[0m")?;
        }
        self.style = None;
        Ok(())
    }

    /// Whether the status row bytes differ from the ones last sent; remembers `row` as sent.
    pub fn status_changed(&mut self, row: &[u8]) -> bool {
        if self.status == row {
            return false;
        }
        self.status.clear();
        self.status.extend_from_slice(row);
        true
    }
}

/// One SGR sequence that fully specifies `style`, starting from a reset so no attribute leaks.
pub fn write_sgr<W: Write>(out: &mut W, style: &CellStyle) -> io::Result<()> {
    out.write_all(b"\x1b[0")?;
    if style.bold {
        out.write_all(b";1")?;
    }
    if style.italic {
        out.write_all(b";3")?;
    }
    if style.underline {
        out.write_all(b";4")?;
    }
    if style.reverse {
        out.write_all(b";7")?;
    }
    if let Some(fg) = style.fg {
        write_color(out, fg, 30)?;
    }
    if let Some(bg) = style.bg {
        write_color(out, bg, 40)?;
    }
    out.write_all(b"m")
}

/// `base` is 30 for foreground and 40 for background; bright colors use the 90/100 range.
fn write_color<W: Write>(out: &mut W, color: Color, base: u8) -> io::Result<()> {
    let code = match color {
        Color::Indexed(i) => return write!(out, ";{};5;{i}", base + 8),
        Color::Rgb(r, g, b) => return write!(out, ";{};2;{r};{g};{b}", base + 8),
        Color::Black => base,
        Color::Red => base + 1,
        Color::Green => base + 2,
        Color::Yellow => base + 3,
        Color::Blue => base + 4,
        Color::Magenta => base + 5,
        Color::Cyan => base + 6,
        Color::White => base + 7,
        Color::BrightBlack => base + 60,
        Color::BrightRed => base + 61,
        Color::BrightGreen => base + 62,
        Color::BrightYellow => base + 63,
        Color::BrightBlue => base + 64,
        Color::BrightMagenta => base + 65,
        Color::BrightCyan => base + 66,
        Color::BrightWhite => base + 67,
    };
    write!(out, ";{code}")
}

fn digits(n: u16) -> usize {
    n.checked_ilog10().map_or(1, |d| d as usize + 1)
}
