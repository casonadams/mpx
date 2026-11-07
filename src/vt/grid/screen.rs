use super::Grid;
use super::cell::Cell;

impl Grid {
    /// Whether the alternate screen is shown.
    pub fn alternate_screen(&self) -> bool {
        self.alternate_screen
    }

    /// Turns the alternate screen on or off (DEC modes 47, 1047 and 1049).
    pub fn set_alternate_screen(&mut self, on: bool) {
        self.alternate_screen = on;
    }

    /// The visible rows, top to bottom.
    pub fn screen_lines(&self) -> impl DoubleEndedIterator<Item = &[Cell]> {
        self.lines.iter().map(Vec::as_slice)
    }

    pub fn get_cell(&self, x: usize, y: usize) -> Option<&Cell> {
        self.lines.get(y).and_then(|line| line.get(x))
    }

    pub fn get_cell_mut(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        self.lines.get_mut(y).and_then(|line| line.get_mut(x))
    }

    pub fn save_cursor(&mut self) {
        self.saved_cursor = Some(self.cursor);
    }

    pub fn restore_cursor(&mut self) {
        if let Some(cursor) = self.saved_cursor {
            self.cursor = cursor;
        }
    }

    pub fn move_cursor(&mut self, x: usize, y: usize) {
        self.cursor.x = x.min(self.cols.saturating_sub(1));
        self.cursor.y = y.min(self.rows.saturating_sub(1));
    }

    pub fn render_plain(&self) -> String {
        let mut out = String::with_capacity(self.rows * (self.cols + 1));
        for line in &self.lines {
            let text: String = line.iter().map(|cell| cell.c).collect();
            out.push_str(text.trim_end());
            out.push('\n');
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }
}
