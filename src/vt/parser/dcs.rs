use super::VtHandler;

const MAX_DCS_LEN: usize = 1024 * 1024;
const PASSTHROUGH_PREFIXES: [&[u8]; 2] = [b"tmux;", b"mpx;"];

/// Whether a DCS that begins with `head` is, or may still turn out to be, a passthrough.
fn may_pass_through(head: &[u8]) -> bool {
    PASSTHROUGH_PREFIXES
        .iter()
        .any(|prefix| prefix.starts_with(head) || head.starts_with(prefix))
}

impl VtHandler {
    pub(super) fn dcs_hook(&mut self, action: char) {
        let buf = vec![action as u8];
        self.dcs_buf = Some(buf);
    }

    pub(super) fn dcs_put(&mut self, byte: u8) {
        let Some(buf) = &mut self.dcs_buf else {
            return;
        };
        if buf.len() < MAX_DCS_LEN {
            buf.push(byte);
        }
        if buf.len() <= PASSTHROUGH_PREFIXES[0].len() && !may_pass_through(buf) {
            self.dcs_buf = None;
        }
    }

    pub(super) fn dcs_unhook(&mut self) {
        if let Some(buf) = self.dcs_buf.take() {
            let payload = buf
                .strip_prefix(b"tmux;")
                .or_else(|| buf.strip_prefix(b"mpx;"));
            if let Some(payload) = payload {
                let unescaped = unescape_tmux_dcs(payload);
                if !unescaped.is_empty() {
                    self.passthroughs.push(unescaped);
                }
            }
        }
    }
}

fn unescape_tmux_dcs(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len());
    let mut i = 0;
    while i < payload.len() {
        if payload[i] == 0x1b && i + 1 < payload.len() && payload[i + 1] == 0x1b {
            out.push(0x1b);
            i += 2;
        } else {
            out.push(payload[i]);
            i += 1;
        }
    }
    out
}
