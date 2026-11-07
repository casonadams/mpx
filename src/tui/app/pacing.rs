use std::time::{Duration, Instant};

use super::state::{App, Mode};

/// Window in which at most `FRAMES_PER_BUDGET` frames are drawn for pane output, so a flood cannot
/// starve input.
pub const FRAME_BUDGET: Duration = Duration::from_millis(16);
/// Output frames allowed per budget window: more than one, so a burst that arrives in a few reads
/// (a pager page) is never held back a whole window between its pieces.
pub const FRAMES_PER_BUDGET: usize = 4;
/// Output of the focused pane this soon after a keypress is likely its response (the terminal's
/// echo, then the program's answer): its frames are drawn at once, past the budget.
pub const TYPING_WINDOW: Duration = Duration::from_millis(100);
/// Frames per keypress drawn past the budget, so a flood started by a keypress (Enter on
/// `cat bigfile`) still cannot starve input.
pub const TYPING_FRAMES: u8 = 16;

/// When to draw output that arrived at `now`: a frame already scheduled stands; otherwise at once,
/// unless the budget is spent: `window_start` is the oldest of the last `FRAMES_PER_BUDGET` output
/// frames when that many were drawn. No settle delay: events that arrived together are already one
/// batch, and timers are too coarse (whole milliseconds) to wait for less than that.
pub fn next_frame(
    window_start: Option<Instant>,
    now: Instant,
    pending: Option<Instant>,
) -> Instant {
    if let Some(due) = pending {
        return due;
    }
    match window_start {
        Some(oldest) => now.max(oldest + FRAME_BUDGET),
        None => now,
    }
}

/// Whether to draw after a wake-up. Pane output waits for its frame unless that frame is already
/// due (drawn now rather than through a timer, which would add up to a millisecond); a key that
/// only went to a pane changes nothing until the pane answers; anything else shows at once.
pub fn needs_redraw(
    output: bool,
    forwarded: bool,
    frame_due: Option<Instant>,
    now: Instant,
) -> bool {
    let due = frame_due.is_some_and(|at| at <= now);
    due || !(output || forwarded)
}

impl App {
    /// Records a drawn frame: it shows all output received so far. Only a frame that showed new
    /// output counts against the frame budget; a redraw for a keypress must not delay the output
    /// that keypress provokes.
    pub fn drew(&mut self, at: Instant) {
        self.pacing.drawn_at = Some(at);
        if self.pacing.frame_due.take().is_some() {
            self.pacing.output_frames.push_back(at);
            if self.pacing.output_frames.len() > FRAMES_PER_BUDGET {
                self.pacing.output_frames.pop_front();
            }
        }
    }

    /// When the pending shortcut popup is due, while prefix mode waits for its next key.
    pub fn hud_deadline(&self) -> Option<Instant> {
        let delay = Duration::from_millis(self.config.which_key_delay_ms);
        match self.mode {
            Mode::Prefix => self.interaction.prefix_at.map(|at| at + delay),
            _ => None,
        }
    }

    /// Whether the shortcut popup shows: prefix mode has lasted at least the configured delay.
    pub fn hud_visible(&self) -> bool {
        self.hud_deadline().is_some_and(|due| Instant::now() >= due)
    }

    /// When the display-panes numbers go away on their own; `None` while they wait for a key.
    pub fn display_deadline(&self) -> Option<Instant> {
        match self.mode {
            Mode::Panes { until } => until,
            _ => None,
        }
    }

    /// Clears the display-panes numbers once their time has passed at `now`.
    pub fn expire_panes(&mut self, now: Instant) {
        if let Mode::Panes { until: Some(until) } = self.mode
            && now >= until
        {
            self.mode = Mode::Normal;
            self.layout.dirty = true;
        }
    }
}
