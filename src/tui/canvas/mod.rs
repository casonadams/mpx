//! The layout area as last sent to the terminal, so each frame emits only the cells that changed.
use std::io::{self, Write};

use crate::layout::geometry::Rect;
use crate::vt::{Cell, CellStyle};

mod scroll;
mod sgr;

use sgr::write_link;
pub use sgr::write_sgr;

/// Shadow of the layout area. `None` marks a cell whose on-screen content is unknown (never drawn,
/// or hidden under an overlay), which forces the next frame to paint it.
#[derive(Debug, Default)]
pub struct Canvas {
    width: u16,
    height: u16,
    front: Vec<Option<Cell>>,
    occluded: Vec<Rect>,
    cursor: Option<(u16, u16)>,
    style: Option<CellStyle>,
    status: Vec<u8>,
    lr_margins: bool,
}

/// Whether two rectangles share a cell.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

impl Canvas {
    /// Starts a frame over a `width` x `height` area; a size change forgets everything on screen.
    /// Cells inside the `occluded` rectangles are covered by chrome drawn later in the frame:
    /// they are not painted, and are forgotten, so whatever is drawn there once the chrome is
    /// gone is sent again, even what nothing drew while it was up.
    pub fn begin(&mut self, width: u16, height: u16, occluded: impl IntoIterator<Item = Rect>) {
        if (width, height) != (self.width, self.height) {
            self.width = width;
            self.height = height;
            self.front = vec![None; width as usize * height as usize];
            self.status.clear();
        }
        self.occluded = occluded.into_iter().collect();
        for r in &self.occluded {
            let cols = r.x.min(width) as usize..(r.x + r.width).min(width) as usize;
            for y in r.y..(r.y + r.height).min(height) {
                let row = y as usize * width as usize;
                self.front[row + cols.start..row + cols.end].fill(None);
            }
        }
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

    /// Writes `cell` at `(x, y)` unless the terminal already shows it there.
    pub fn put<W: Write>(&mut self, out: &mut W, x: u16, y: u16, cell: Cell) -> io::Result<()> {
        if x >= self.width || y >= self.height {
            return Ok(());
        }
        let i = self.index(x, y);
        if self.covers(Rect::new(x, y, 1, 1)) {
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
        let hidden = self.covers(Rect::new(from, y, x - from, 1));
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
        let covered = self.covers(Rect::new(x, y, end - x, 1));
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
            let hidden = self.covers(Rect::new(x + dx as u16, y, 1, 1));
            self.front[start + dx] = (!hidden).then_some(*cell);
        }
        self.cursor = None;
        let row_end = self.index(0, y) + self.width as usize;
        self.front[start + cells.len()..row_end].fill(None);
        Ok(())
    }

    fn write_cell<W: Write>(&mut self, out: &mut W, cell: Cell) -> io::Result<()> {
        if self.style != Some(cell.style) {
            let open = self.style.and_then(|s| s.link);
            write_sgr(out, &cell.style)?;
            if open != cell.style.link {
                write_link(out, cell.style.link)?;
            }
            self.style = Some(cell.style);
        }
        let mut utf8 = [0u8; 4];
        out.write_all(cell.c.encode_utf8(&mut utf8).as_bytes())
    }

    /// Whether chrome covers any cell of `region`.
    fn covers(&self, region: Rect) -> bool {
        self.occluded.iter().any(|r| overlaps(*r, region))
    }

    fn index(&self, x: u16, y: u16) -> usize {
        y as usize * self.width as usize + x as usize
    }

    /// Ends the hyperlink the terminal is in, if any.
    fn end_link<W: Write>(&self, out: &mut W) -> io::Result<()> {
        match self.style.and_then(|s| s.link) {
            Some(_) => write_link(out, None),
            None => Ok(()),
        }
    }

    /// Ends the frame's cell writes on default attributes, so chrome drawn next starts clean.
    pub fn finish<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        self.end_link(out)?;
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

fn digits(n: u16) -> usize {
    n.checked_ilog10().map_or(1, |d| d as usize + 1)
}
