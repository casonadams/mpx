use super::VtHandler;
use super::screen::Alert;
use crate::vt::grid::ShellMark;
use crate::vt::link::LinkId;

impl VtHandler {
    pub(super) fn handle_osc(&mut self, params: &[&[u8]], bell_terminated: bool) {
        match params {
            [b"0" | b"2", rest @ ..] => self.title = Some(join_osc(rest)),
            [b"133", b"A", ..] => self.grid.mark_shell(ShellMark::Prompt),
            [b"133", b"C", ..] => self.grid.mark_shell(ShellMark::Output),
            [b"8", rest @ ..] => self.set_hyperlink(rest),
            // OSC 9;4 is ConEmu/Windows Terminal progress reporting, not a notification.
            [b"9", b"4", ..] => {}
            [b"9", rest @ ..] => self.push_notify(join_osc(rest)),
            [b"777", b"notify", title, body @ ..] => {
                let title = String::from_utf8_lossy(title).into_owned();
                let body = join_osc(body);
                let text = if body.is_empty() {
                    title
                } else {
                    format!("{title}: {body}")
                };
                self.push_notify(text);
            }
            // Respond to background color query
            [b"11", b"?", ..] => {
                let bg = self.bg_color.as_deref().unwrap_or("rgb:0000/0000/0000");
                self.responses
                    .push(format!("\x1b]11;{bg}\x1b\\").into_bytes());
            }
            // Background color set: save for queries and pass through
            [b"11", color, ..] => {
                self.bg_color = Some(String::from_utf8_lossy(color).into_owned());
                self.passthroughs.push(format_osc(params, bell_terminated));
            }
            // Palette (4), Foreground (10), Cursor (12), Clipboard (52), Resets (104, 110, 111, 112)
            [
                b"4" | b"10" | b"12" | b"52" | b"104" | b"110" | b"111" | b"112",
                ..,
            ] => {
                if params[0] == b"111" {
                    self.bg_color = None;
                }
                self.passthroughs.push(format_osc(params, bell_terminated));
            }
            // iTerm2 custom sequences (e.g. OSC Pg... for colors)
            [first, ..] if first.starts_with(b"P") => {
                self.passthroughs.push(format_osc(params, bell_terminated));
            }
            _ => {}
        }
    }

    /// OSC 8: the text printed from now links to `rest` (`params;uri`); an empty URI ends the link.
    fn set_hyperlink(&mut self, rest: &[&[u8]]) {
        let uri_empty = rest.get(1).is_none_or(|uri| uri.is_empty());
        self.grid.current_style.link = if uri_empty {
            None
        } else {
            LinkId::intern(&join_osc(rest))
        };
    }

    fn push_notify(&mut self, text: String) {
        if !text.is_empty() {
            self.alerts.push(Alert::Notify(text));
        }
    }
}

fn format_osc(params: &[&[u8]], bell_terminated: bool) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(b"\x1b]");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            buf.push(b';');
        }
        buf.extend_from_slice(p);
    }
    if bell_terminated {
        buf.push(0x07);
    } else {
        buf.extend_from_slice(b"\x1b\\");
    }
    buf
}

fn join_osc(parts: &[&[u8]]) -> String {
    parts
        .iter()
        .map(|part| String::from_utf8_lossy(part))
        .collect::<Vec<_>>()
        .join(";")
}
