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
        if self.cursor != Some((x, y)) {
            write!(out, "\x1b[{};{}H", y + 1, x + 1)?;
        }
        self.write_cell(out, cell)?;
        // The grid has no width model: after a non-ASCII glyph the terminal's column is not known.
        self.cursor = (cell.c.is_ascii() && x + 1 < self.width).then_some((x + 1, y));
        self.front[i] = Some(cell);
        Ok(())
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
