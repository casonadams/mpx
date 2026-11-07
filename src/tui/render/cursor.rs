use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
};

use super::Compositor;
use crate::layout::geometry::Rect;

impl Compositor {
    /// Emits DEC 2026 Begin Synchronized Update sequence to eliminate tearing and cursor jitter
    pub fn begin_sync<W: Write>(w: &mut W) -> io::Result<()> {
        w.write_all(b"\x1b[?2026h")
    }

    /// Emits DEC 2026 End Synchronized Update sequence to flush complete frame atomically
    pub fn end_sync<W: Write>(w: &mut W) -> io::Result<()> {
        w.write_all(b"\x1b[?2026l")
    }

    /// Places the live terminal hardware cursor at the focused pane's active position
    pub fn position_cursor<W: Write>(
        w: &mut W,
        pane_rect: Rect,
        cursor_x: usize,
        cursor_y: usize,
        visible: bool,
    ) -> io::Result<()> {
        if !visible {
            queue!(w, Hide)?;
            return Ok(());
        }

        let cx = (pane_rect.x as usize + cursor_x)
            .min((pane_rect.x + pane_rect.width).saturating_sub(1) as usize)
            as u16;
        let cy = (pane_rect.y as usize + cursor_y)
            .min((pane_rect.y + pane_rect.height).saturating_sub(1) as usize)
            as u16;

        queue!(w, MoveTo(cx, cy), Show)?;
        Ok(())
    }
}
