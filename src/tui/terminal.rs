use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableMouseCapture,
    },
    queue,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use std::io::{self, Write, stdout};

use super::color::Palette;

/// Focus reporting on (DEC 1004) and off: sent only when `focus-events` asks for it.
const FOCUS_ON: &[u8] = b"\x1b[?1004h";
const FOCUS_OFF: &[u8] = b"\x1b[?1004l";
/// Kitty keyboard protocol: push the disambiguate flag, pop it on exit.
const KEYBOARD_PUSH: &[u8] = b"\x1b[>1u";
const KEYBOARD_POP: &[u8] = b"\x1b[<u";

/// What the terminal is sent to show the TUI: the alternate screen, cleared, cursor hidden, mouse
/// and paste reporting on, autowrap off so the status bar never scrolls the screen.
pub fn enter_sequence() -> Vec<u8> {
    let mut out = Vec::new();
    let _ = queue!(
        out,
        EnterAlternateScreen,
        Clear(ClearType::All),
        Hide,
        EnableMouseCapture,
        EnableBracketedPaste
    );
    out.extend_from_slice(b"\x1b[?7l");
    out
}

/// What the terminal is sent to hand the screen back to the shell: the synchronized update ended,
/// autowrap on, focus, paste and mouse reporting off, the main screen and the cursor shown.
pub fn leave_sequence() -> Vec<u8> {
    let mut out = b"\x1b[?2026l\x1b[?7h".to_vec();
    let _ = queue!(
        out,
        DisableFocusChange,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        Show
    );
    out
}

/// The window title stack: the title the terminal had is saved before the TUI sets its own, and
/// restored on exit.
const TITLE_PUSH: &[u8] = b"\x1b[22;2t";
const TITLE_POP: &[u8] = b"\x1b[23;2t";

/// The terminal the TUI draws on. Focus reporting and the keyboard protocol are on while the
/// options ask for them, and only then: `sync` writes each change once.
pub struct TerminalGuard {
    focus: bool,
    keys: bool,
    titled: bool,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = stdout();
        out.write_all(&enter_sequence())?;
        out.flush()?;
        Ok(Self {
            focus: false,
            keys: false,
            titled: false,
        })
    }

    /// Turns focus reporting and the keyboard protocol on or off to match `focus` and `keys`, writing
    /// only what changed.
    pub fn sync<W: Write>(&mut self, out: &mut W, focus: bool, keys: bool) -> io::Result<()> {
        if focus != self.focus {
            self.focus = focus;
            out.write_all(if focus { FOCUS_ON } else { FOCUS_OFF })?;
        }
        if keys != self.keys {
            self.keys = keys;
            out.write_all(if keys { KEYBOARD_PUSH } else { KEYBOARD_POP })?;
        }
        Ok(())
    }

    /// Sets the outer terminal's title, saving the one it had on first use.
    pub fn set_title<W: Write>(&mut self, out: &mut W, title: &str) -> io::Result<()> {
        if !self.titled {
            self.titled = true;
            out.write_all(TITLE_PUSH)?;
        }
        write!(out, "\x1b]2;{title}\x1b\\")
    }

    /// Stops the process as Ctrl-Z does: the shell gets its screen and cooked mode back, and the
    /// TUI takes the terminal again once the shell continues the process (SIGCONT).
    pub fn suspend(&self) -> io::Result<()> {
        let mut out = stdout();
        out.write_all(&leave_sequence())?;
        if self.keys {
            out.write_all(KEYBOARD_POP)?;
        }
        out.flush()?;
        disable_raw_mode()?;
        stop_process();
        enable_raw_mode()?;
        out.write_all(&enter_sequence())?;
        if self.focus {
            out.write_all(FOCUS_ON)?;
        }
        if self.keys {
            out.write_all(KEYBOARD_PUSH)?;
        }
        out.flush()
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = stdout();
        if self.keys {
            let _ = out.write_all(KEYBOARD_POP);
        }
        let _ = out.write_all(&leave_sequence());
        if self.titled {
            let _ = out.write_all(TITLE_POP);
        }
        let _ = out.flush();
        let _ = disable_raw_mode();
    }
}

/// Sends SIGTSTP to this process. Its default action stops the process, so the call returns once
/// the shell continues it.
#[cfg(unix)]
fn stop_process() {
    // SAFETY: getpid and kill take no pointers and touch no memory of this process.
    unsafe {
        libc::kill(libc::getpid(), libc::SIGTSTP);
    }
}

#[cfg(not(unix))]
fn stop_process() {}

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
    color_after(reply, &format!("\x1b]{code};"))
}

/// The color a terminal answered to `OSC 4 ; index ; ?` in `reply`.
pub fn palette_reply(reply: &[u8], index: usize) -> Option<(u8, u8, u8)> {
    color_after(reply, &format!("\x1b]4;{index};"))
}

fn color_after(reply: &[u8], head: &str) -> Option<(u8, u8, u8)> {
    color_in(&String::from_utf8_lossy(reply), head)
}

fn color_in(text: &str, head: &str) -> Option<(u8, u8, u8)> {
    let start = text.find(head)? + head.len();
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
    /// The palette colors it reported, so dimming starts from what the terminal really shows.
    pub palette: Palette,
}

/// Interprets a complete reply to the attach query.
pub fn report_from(reply: &[u8]) -> TerminalReport {
    let text = String::from_utf8_lossy(reply);
    let mut palette = Palette::default();
    for index in 0..256 {
        if let Some(rgb) = color_in(&text, &format!("\x1b]4;{index};")) {
            palette.set(index, rgb);
        }
    }
    TerminalReport {
        margins: margins_reply(reply).unwrap_or(false),
        fg: osc_color_reply(reply, 10),
        bg: osc_color_reply(reply, 11),
        palette,
    }
}

/// What attach sends: with `colors`, the default colors and the 256 palette colors; then the
/// margins mode and the DA1 that every terminal answers last. The colors are only worth asking for
/// when something dims by them.
pub fn attach_query(colors: bool) -> Vec<u8> {
    let mut query = Vec::new();
    if colors {
        query.extend_from_slice(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\");
        for index in 0..256 {
            query.extend_from_slice(format!("\x1b]4;{index};?\x1b\\").as_bytes());
        }
    }
    query.extend_from_slice(b"\x1b[?69$p\x1b[c");
    query
}

/// Writes `query` to the terminal while reading what it answers into `reply`. Reading as it goes
/// matters: a terminal that blocks on a full input queue to us while we block on a full output
/// queue to it would hang both. False when the terminal stops taking input for too long.
#[cfg(unix)]
fn send_query(query: &[u8], reply: &mut Vec<u8>) -> bool {
    const SLICE: usize = 256;
    let deadline = std::time::Instant::now() + QUERY_TIMEOUT;
    let mut sent = 0;
    while sent < query.len() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let mut fds = [
            libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: 1, events: libc::POLLOUT, revents: 0 },
        ];
        let ms = left.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: two valid pollfds for stdin and stdout.
        if left.is_zero() || unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) } <= 0 {
            return false;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            read_some(reply, std::time::Duration::ZERO);
        }
        if fds[1].revents & libc::POLLOUT != 0 {
            let end = (sent + SLICE).min(query.len());
            // SAFETY: writes at most `end - sent` bytes from `query`.
            let n = unsafe { libc::write(1, query[sent..end].as_ptr().cast(), end - sent) };
            if n < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return false;
            }
            sent += n.max(0) as usize;
        }
    }
    true
}

/// Asks the terminal whether it supports left/right margins and, with `colors`, for its default
/// colors and its palette. Run in raw mode before anything else reads input. The DA1 that ends the query is
/// answered by every terminal, after the answers it does give, so one round trip covers all of it.
#[cfg(unix)]
pub fn query_terminal(colors: bool) -> TerminalReport {
    let mut reply = Vec::new();
    if !send_query(&attach_query(colors), &mut reply) {
        return report_from(&reply);
    }
    let deadline = std::time::Instant::now() + QUERY_TIMEOUT;
    while margins_reply(&reply).is_none() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() || !read_some(&mut reply, left) {
            break;
        }
    }
    report_from(&reply)
}

#[cfg(not(unix))]
pub fn query_terminal(_colors: bool) -> TerminalReport {
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
