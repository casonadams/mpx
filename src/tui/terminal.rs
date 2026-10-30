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

/// Longest wait for the terminal to answer the margin query at startup. Every terminal answers
/// the DA1 that ends it, so the wait is one round trip; the limit only matters for one that never
/// does. It is generous because a reply arriving after it would be read as typed keys (a slow
/// SSH link).
#[cfg(unix)]
const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

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

/// An `rgb:r/g/b` (one to four hex digits per channel) or `#rrggbb` color, as a terminal reports it.
pub fn parse_color_spec(spec: &str) -> Option<(u8, u8, u8)> {
    if let Some(hex) = spec.strip_prefix('#') {
        let value = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6)?;
        return Some(((value >> 16) as u8, (value >> 8) as u8, value as u8));
    }
    let mut channels = spec.strip_prefix("rgb:")?.split('/').map(|digits| {
        let max = 16u32.checked_pow(digits.len() as u32)? - 1;
        let value = u32::from_str_radix(digits, 16).ok().filter(|_| (1..=4).contains(&digits.len()))?;
        Some(((value * 255 + max / 2) / max) as u8)
    });
    let color = (channels.next()??, channels.next()??, channels.next()??);
    channels.next().is_none().then_some(color)
}

/// The color a terminal answered to `OSC code ; ?` in `reply` (10 is the foreground, 11 the
/// background); the answer ends with ST or BEL.
pub fn osc_color_reply(reply: &[u8], code: u8) -> Option<(u8, u8, u8)> {
    let text = String::from_utf8_lossy(reply);
    let head = format!("\x1b]{code};");
    let start = text.find(&head)? + head.len();
    let rest = &text[start..];
    let end = rest.find(['\x1b', '\x07'])?;
    parse_color_spec(&rest[..end])
}

/// What the terminal told us at attach.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalReport {
    /// Supports left/right margins (DECLRMM).
    pub margins: bool,
    /// Default foreground and background colors, when it reported them.
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
}

/// Interprets a complete reply to the attach query.
pub fn report_from(reply: &[u8]) -> TerminalReport {
    TerminalReport {
        margins: margins_reply(reply).unwrap_or(false),
        fg: osc_color_reply(reply, 10),
        bg: osc_color_reply(reply, 11),
    }
}

/// Asks the terminal for its default colors and whether it supports left/right margins. Run in raw
/// mode before anything else reads input. The DA1 that ends the query is answered by every
/// terminal, after the answers it does give, so one round trip covers all of it.
#[cfg(unix)]
pub fn query_terminal() -> TerminalReport {
    let mut out = stdout();
    let query = b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[?69$p\x1b[c";
    if out.write_all(query).and_then(|()| out.flush()).is_err() {
        return TerminalReport::default();
    }
    let deadline = std::time::Instant::now() + QUERY_TIMEOUT;
    let mut reply = Vec::new();
    while margins_reply(&reply).is_none() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() || !read_some(&mut reply, left) {
            break;
        }
    }
    report_from(&reply)
}

#[cfg(not(unix))]
pub fn query_terminal() -> TerminalReport {
    TerminalReport::default()
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
