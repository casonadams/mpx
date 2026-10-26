use std::io::{self, stdout, Write};
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
    },
};

pub struct TerminalGuard {
    _private: (),
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen, Clear(ClearType::All), Hide, EnableMouseCapture)?;
        out.flush()?;
        Ok(Self { _private: () })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = stdout();
        let _ = execute!(out, DisableMouseCapture, LeaveAlternateScreen, Show);
        let _ = out.flush();
        let _ = disable_raw_mode();
    }
}
