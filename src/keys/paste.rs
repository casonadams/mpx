use super::TermModes;

const PASTE_END: &str = "\x1b[201~";

/// Wraps text in `ESC[200~ … ESC[201~` when the program enabled bracketed paste; strips embedded end markers.
pub fn wrap_paste(text: &str, modes: &TermModes) -> Vec<u8> {
    paste_buffer_bytes(text, None, false, false, modes)
}

/// Bytes that paste a paste buffer. Line feeds become `separator` when given, else carriage
/// returns, which `raw` keeps as they are. Bracketed when `bracketed` is set or the program
/// enabled bracketed paste.
pub fn paste_buffer_bytes(
    text: &str,
    separator: Option<&str>,
    raw: bool,
    bracketed: bool,
    modes: &TermModes,
) -> Vec<u8> {
    let body = match separator {
        Some(separator) => text.replace("\r\n", "\n").replace('\n', separator),
        None if raw => text.to_owned(),
        None => text.replace("\r\n", "\r").replace('\n', "\r"),
    };
    if !(bracketed || modes.bracketed_paste) {
        return body.into_bytes();
    }
    bracket(&body)
}

fn bracket(text: &str) -> Vec<u8> {
    let mut body = text.to_owned();
    while body.contains(PASTE_END) {
        body = body.replace(PASTE_END, "");
    }
    format!("\x1b[200~{body}{PASTE_END}").into_bytes()
}
