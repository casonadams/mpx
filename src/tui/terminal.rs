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

/// Longest wait for the terminal to answer the margin query at startup.
const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);

/// What the terminal said about left/right margins (DECLRMM, mode 69), once its reply is complete:
/// the DECRQM report, if any, precedes the DA1 answer every terminal sends. `None` until then.
pub fn margins_reply(reply: &[u8]) -> Option<bool> {
    let text = String::from_utf8_lossy(reply);
    let answered = text.match_indices("\x1b[?").any(|(start, _)| {
        let rest = &text[start + 3..];
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == ';')).unwrap_or(rest.len());
        rest[end..].starts_with('c')
    });
    if !answered {
        return None;
    }
    let supported = ["1", "2", "3"]
        .iter()
        .any(|ps| text.contains(&format!("\x1b[?69;{ps}$y")));
    Some(supported)
}

/// Asks the terminal whether it supports left/right margins. Run in raw mode before anything
/// else reads input; a terminal that does not answer the mode query still answers DA1.
#[cfg(unix)]
pub fn query_lr_margins() -> bool {
    let mut out = stdout();
    if out.write_all(b"\x1b[?69$p\x1b[c").and_then(|()| out.flush()).is_err() {
        return false;
    }
    let deadline = std::time::Instant::now() + QUERY_TIMEOUT;
    let mut reply = Vec::new();
    while margins_reply(&reply).is_none() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() || !read_some(&mut reply, left) {
            break;
        }
    }
    margins_reply(&reply).unwrap_or(false)
}

#[cfg(not(unix))]
pub fn query_lr_margins() -> bool {
    false
}

/// Reads what stdin has within `wait` into `reply`; false when nothing came.
#[cfg(unix)]
fn read_some(reply: &mut Vec<u8>, wait: std::time::Duration) -> bool {
    let mut fds = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = wait.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: one valid pollfd for stdin.
    if unsafe { libc::poll(&mut fds, 1, ms) } <= 0 {
        return false;
    }
    let mut buf = [0u8; 256];
    // SAFETY: reads at most `buf.len()` bytes into `buf`.
    let n = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
    if n <= 0 {
        return false;
    }
    reply.extend_from_slice(&buf[..n as usize]);
    true
}
