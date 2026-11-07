use std::time::{Duration, Instant};

use super::{Link, Workspace};

impl Workspace {
    pub fn mark_activity(&mut self, pane_name: &str) -> bool {
        self.mark_alert(pane_name, |l| &mut l.has_activity)
    }

    pub fn mark_bell(&mut self, pane_name: &str) -> bool {
        self.mark_alert(pane_name, |l| &mut l.has_bell)
    }

    /// Sets an alert flag on every index showing the window of `pane` but the active one. True when
    /// a flag was newly set.
    fn mark_alert(&mut self, pane: &str, flag: impl Fn(&mut Link) -> &mut bool) -> bool {
        let Some(window) = self.window_id_of(pane) else {
            return false;
        };
        let active = self.active_idx;
        let mut marked = false;
        for (pos, link) in self.links.iter_mut().enumerate() {
            if pos == active || link.window != window {
                continue;
            }
            let target = flag(link);
            if !*target {
                *target = true;
                marked = true;
            }
        }
        marked
    }

    /// Records output from a pane: the silence timer of every index showing its window restarts and
    /// their silence flags clear.
    pub fn note_output(&mut self, pane_name: &str, now: Instant) {
        let Some(window) = self.window_id_of(pane_name) else {
            return;
        };
        for link in self.links.iter_mut().filter(|l| l.window == window) {
            link.last_output = Some(now);
            link.has_silence = false;
        }
    }

    /// Flags background indices whose windows have been quiet for `secs` seconds
    /// (`monitor-silence`). Returns the numbers of the indices flagged now.
    pub fn expire_silence(&mut self, secs: u64, now: Instant) -> Vec<usize> {
        let active = self.active_idx;
        let quiet = Duration::from_secs(secs);
        let mut flagged = Vec::new();
        for (pos, link) in self.links.iter_mut().enumerate() {
            if pos != active
                && !link.has_silence
                && link
                    .last_output
                    .is_some_and(|t| now.duration_since(t) >= quiet)
            {
                link.has_silence = true;
                flagged.push(link.number);
            }
        }
        flagged
    }

    /// When the next background index goes quiet for `secs` seconds, if any is still counting.
    pub fn next_silence(&self, secs: u64) -> Option<Instant> {
        let quiet = Duration::from_secs(secs);
        self.links
            .iter()
            .enumerate()
            .filter(|(i, l)| *i != self.active_idx && !l.has_silence)
            .filter_map(|(_, l)| l.last_output)
            .map(|t| t + quiet)
            .min()
    }
}
