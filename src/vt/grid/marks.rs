use super::{Grid, ShellMark};

impl Grid {
    /// Records a shell mark on the cursor's row.
    pub fn mark_shell(&mut self, mark: ShellMark) {
        let row = self.scrolled_off + self.cursor.y;
        let first = self.first_kept_row();
        let marks = match mark {
            ShellMark::Prompt => &mut self.prompts,
            ShellMark::Output => &mut self.outputs,
        };
        while marks.front().is_some_and(|&kept| kept < first) {
            marks.pop_front();
        }
        if marks.back() != Some(&row) {
            marks.push_back(row);
        }
    }

    /// The absolute lines (numbered as `abs_line` numbers them) of the marks of `mark` whose rows
    /// are still in history or on screen, oldest first.
    pub fn shell_marks(&self, mark: ShellMark) -> impl Iterator<Item = usize> + '_ {
        let first = self.first_kept_row();
        let marks = match mark {
            ShellMark::Prompt => &self.prompts,
            ShellMark::Output => &self.outputs,
        };
        marks
            .iter()
            .filter(move |&&row| row >= first)
            .map(move |&row| row - first)
    }

    fn first_kept_row(&self) -> usize {
        self.scrolled_off - self.history.len()
    }

    /// The row number of absolute line `abs_line`, which keeps its number while the line moves
    /// into history.
    pub fn row_at(&self, abs_line: usize) -> usize {
        self.first_kept_row() + abs_line
    }

    /// The absolute line of row `row`, or `None` once that row has left history.
    pub fn abs_at(&self, row: usize) -> Option<usize> {
        row.checked_sub(self.first_kept_row())
    }
}
