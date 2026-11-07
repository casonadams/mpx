use std::borrow::Cow;

use vte::Parser;

use super::VtHandler;
use crate::keys::TermModes;
use crate::vt::grid::{Cell, Grid};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    Bell,
    Notify(String),
}

pub struct VtScreen {
    parser: Parser,
    handler: VtHandler,
}

impl VtScreen {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            parser: Parser::new(),
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

    /// The grid a pane has after it resizes from `from` to `to` (cols, rows), rebuilt from its raw
    /// log. `before` is the log up to the resize and `after` what was written since. `before` replays
    /// at the old size with its scrollback and the grid resizes as `Grid::resize` does, so rows that
    /// a grow pulls back from history come back on top; `after` is then fed at the new size, as the
    /// live pane took it. A width change replays both at the new width instead, since the lines
    /// reflow. The grid keeps at most `history` lines of scrollback (0 for a live pane, the
    /// scrollback limit for a pane in copy mode).
    pub fn resized_grid(
        before: &[u8],
        after: &[u8],
        from: (usize, usize),
        to: (usize, usize),
        history: usize,
    ) -> Grid {
        let replay_at = if from.0 == to.0 { from } else { to };
        let mut screen = Self::new(replay_at.0, replay_at.1);
        let replay_limit = screen.grid().history_limit.max(history);
        screen.grid_mut().set_history_limit(replay_limit);
        screen.feed(before);
        screen.grid_mut().resize(to.0, to.1);
        screen.feed(after);
        let _ = screen.take_responses();
        let _ = screen.take_passthroughs();
        screen.grid_mut().set_history_limit(history);
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
