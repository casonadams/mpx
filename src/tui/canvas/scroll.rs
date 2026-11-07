use std::io::{self, Write};

use super::Canvas;
use crate::layout::geometry::Rect;
use crate::vt::Cell;

/// Characters known to take one column: ASCII, and the box drawing and block elements mpx draws its
/// own borders and dividers with (the layout already assumes those are narrow).
fn narrow(c: char) -> bool {
    c.is_ascii() || ('\u{2500}'..='\u{259F}').contains(&c)
}

impl Canvas {
    /// Scrolls `region` by `lines` (up when positive) on the terminal and in the shadow, so content
    /// that only moved is not sent again; rows scrolled in are blank. A region narrower than the
    /// screen needs left/right margins, and only ASCII known on screen in and beside it: the screen
    /// model has no width table to say where a wide glyph near a margin really sits. Returns false,
    /// doing nothing, when chrome covers the region, the move is degenerate or cannot be done.
    pub fn scroll<W: Write>(
        &mut self,
        out: &mut W,
        region: Rect,
        lines: isize,
    ) -> io::Result<bool> {
        let n = lines.unsigned_abs();
        let fits = region.height >= 2
            && region.y + region.height <= self.height
            && region.x + region.width <= self.width
            && n < region.height as usize;
        let covered = self.covers(region);
        if lines == 0 || !fits || covered {
            return Ok(false);
        }
        let full = region.x == 0 && region.width == self.width;
        if !full && !(self.lr_margins && self.plain_around(region)) {
            return Ok(false);
        }
        let (top, bottom) = (region.y + 1, region.y + region.height);
        let verb = if lines > 0 { 'S' } else { 'T' };
        self.end_link(out)?;
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
                Some(f) => self.front.copy_within(
                    f * width + cols.start..f * width + cols.end,
                    dst + cols.start,
                ),
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
}
