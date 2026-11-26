use super::Grid;

impl Grid {
    /// The column the cursor is on, a pending wrap counting as the last column.
    fn column(&self) -> usize {
        self.cursor.x.min(self.cols - 1)
    }

    /// HT and CHT: moves the cursor to the `n`th tab stop to its right, or the last column.
    pub fn tab_forward(&mut self, n: usize) {
        let mut x = self.column();
        for _ in 0..n {
            x = (x + 1..self.cols)
                .find(|&x| self.tab_stops[x])
                .unwrap_or(self.cols - 1);
        }
        self.cursor.x = x;
    }

    /// CBT: moves the cursor to the `n`th tab stop to its left, or the first column.
    pub fn tab_backward(&mut self, n: usize) {
        let mut x = self.column();
        for _ in 0..n {
            x = (0..x).rev().find(|&x| self.tab_stops[x]).unwrap_or(0);
        }
        self.cursor.x = x;
    }

    /// HTS: sets a tab stop at the cursor's column.
    pub fn set_tab_stop(&mut self) {
        let x = self.column();
        self.tab_stops[x] = true;
    }

    /// TBC: `0` clears the tab stop at the cursor's column, `3` every tab stop; others are ignored.
    pub fn clear_tab_stops(&mut self, mode: usize) {
        match mode {
            0 => {
                let x = self.column();
                self.tab_stops[x] = false;
            }
            3 => self.tab_stops.fill(false),
            _ => {}
        }
    }
}
