//! Copy mode: entering and leaving it, copy-mode commands, yanks and pipes into shell commands.
use crate::tui::app::{App, Flow, Mode};
use crate::tui::command::CopyModeOpts;
use crate::tui::copy::{CopyAction, CopyOutcome, CopyState, Motion, apply_action};
use crate::tui::session::split_target;
use std::io::Write;
use std::process::Stdio;

impl App {
    /// `copy-mode [-equ] [-t pane]`. A pane that is not focused gets its copy state parked, so
    /// focus does not move. A pane already in copy mode stays so (`-u` still pages up).
    pub(super) async fn copy_mode_cmd(&mut self, o: CopyModeOpts) -> Flow {
        let raw = self.target_or_active(o.target);
        let (_, name) = split_target(&raw);
        let name = name.to_string();
        if name.is_empty() {
            return self.say("no active pane");
        }
        if !self.panes.iter().any(|(pane, ..)| *pane == name) {
            return self.say(format!("no pane {raw}"));
        }
        if o.quit {
            if self.in_copy_mode(&name) {
                if name == self.active_pane {
                    self.mode = Mode::Normal;
                }
                self.parked_copy.remove(&name);
                self.release_scrollback(&name);
            }
            return Flow::Continue;
        }
        let page_up = CopyAction::Move {
            motion: Motion::PageUp,
            cancel_at_bottom: false,
        };
        if self.in_copy_mode(&name) {
            return if o.page_up {
                self.run_copy(&name, &page_up, 1).await
            } else {
                Flow::Continue
            };
        }
        self.load_scrollback(&name).await;
        let mut cs = CopyState::enter(self.vt.get(&name).map(|vt| vt.grid()));
        cs.wheel_exit = o.exit_at_bottom;
        if name == self.active_pane {
            self.mode = Mode::Copy(cs);
        } else {
            self.parked_copy.insert(name.clone(), cs);
        }
        if o.page_up {
            return self.run_copy(&name, &page_up, 1).await;
        }
        Flow::Continue
    }

    /// Whether pane `name` is in copy mode in this terminal: focused in it, or parked.
    pub fn in_copy_mode(&self, name: &str) -> bool {
        (name == self.active_pane && matches!(self.mode, Mode::Copy(_)))
            || self.parked_copy.contains_key(name)
    }

    /// `send-keys -X`: a copy-mode command in the copy mode of `target`, the active pane when
    /// `None`. Nothing happens in a terminal that does not have that pane in copy mode.
    pub(super) async fn copy_command(
        &mut self,
        target: Option<String>,
        action: CopyAction,
        count: usize,
    ) -> Flow {
        let raw = self.target_or_active(target);
        let (_, name) = split_target(&raw);
        self.run_copy(name, &action, count).await
    }

    /// Runs `action` `count` times in the copy mode of pane `name`, stopping at the first outcome
    /// that is not a plain move.
    async fn run_copy(&mut self, name: &str, action: &CopyAction, count: usize) -> Flow {
        let active = name == self.active_pane;
        let mut cs = if active {
            match std::mem::replace(&mut self.mode, Mode::Normal) {
                Mode::Copy(cs) => cs,
                other => {
                    self.mode = other;
                    return Flow::Continue;
                }
            }
        } else {
            match self.parked_copy.remove(name) {
                Some(cs) => cs,
                None => return Flow::Continue,
            }
        };
        cs.wrap_search = self.config.wrap_search;
        let size = self.copy_pane_size(name);
        let grid = self.vt.get(name).map(|vt| vt.grid());
        let mut outcome = CopyOutcome::Stay;
        for _ in 0..count.max(1) {
            outcome = apply_action(&mut cs, action, grid, size);
            if outcome != CopyOutcome::Stay {
                break;
            }
        }
        self.apply_copy_outcome(name, active, cs, outcome);
        Flow::Continue
    }

    /// The size of pane `name` in cells, which copy-mode motions are clamped to.
    fn copy_pane_size(&self, name: &str) -> (u16, u16) {
        match self.active_rect {
            Some(rect) if name == self.active_pane => (rect.width, rect.height),
            _ => self.pane_dims.get(name).copied().unwrap_or((80, 24)),
        }
    }

    /// Carries out what a copy-mode command decided for pane `name` and puts its copy state back:
    /// `active` says whether the pane is the focused one, else its state is parked.
    pub fn apply_copy_outcome(
        &mut self,
        name: &str,
        active: bool,
        cs: CopyState,
        outcome: CopyOutcome,
    ) {
        let (text, pipe, exit) = match outcome {
            CopyOutcome::Stay => (None, None, false),
            CopyOutcome::Exit => (None, None, true),
            CopyOutcome::Yank(text) => (Some(text), None, true),
            CopyOutcome::Copy(text) => (Some(text), None, false),
            CopyOutcome::Pipe {
                text,
                command,
                exit,
            } => (Some(text), Some(command), exit),
            CopyOutcome::SearchFailed(query) => {
                self.say(format!("Pattern not found: {query}"));
                (None, None, false)
            }
        };
        if let Some(text) = &text {
            if self.config.set_clipboard {
                self.yank.clipboard.copy(text);
            }
            self.yank.yanks.push(text.clone());
            self.yank.last_copy = Some(text.clone());
        }
        if let (Some(command), Some(text)) = (pipe, &text)
            && let Err(e) = pipe_text(&command, text)
        {
            self.say(e);
        }
        if exit {
            if active {
                self.mode = Mode::Normal;
            }
            self.release_scrollback(name);
        } else if active {
            self.mode = Mode::Copy(cs);
        } else {
            self.parked_copy.insert(name.to_string(), cs);
        }
    }
}

/// Pipes `text` to the shell `command` (`copy-pipe`). Its output is discarded and the TUI does not
/// wait for it: the text is written and the child reaped on a thread of its own. Fails when the
/// shell cannot start (on Windows, a POSIX `sh` must be on the `PATH`).
fn pipe_text(command: &str, text: &str) -> Result<(), String> {
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("copy-pipe: cannot start sh: {e}"))?;
    let text = text.to_string();
    std::thread::spawn(move || {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
    Ok(())
}
