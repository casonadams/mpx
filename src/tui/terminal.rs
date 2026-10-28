use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use std::io::{self, Write, stdout};

pub struct TerminalGuard {
    _private: (),
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(
            out,
            EnterAlternateScreen,
            Clear(ClearType::All),
            Hide,
            EnableMouseCapture,
            EnableBracketedPaste,
            EnableFocusChange
        )?;
        let _ = out.write_all(b"\x1b[?7l"); // Disable autowrap so status bar never scrolls screen
        out.flush()?;
        Ok(Self { _private: () })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = stdout();
        let _ = out.write_all(b"\x1b[?2026l");
        let _ = out.write_all(b"\x1b[?7h"); // Re-enable autowrap
        let _ = execute!(
            out,
            DisableFocusChange,
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            Show
        );
        let _ = out.flush();
        let _ = disable_raw_mode();
    }
}
