use super::VtHandler;

const MAX_DCS_LEN: usize = 1024 * 1024;

impl VtHandler {
    pub(super) fn dcs_hook(&mut self, action: char) {
        let buf = vec![action as u8];
        self.dcs_buf = Some(buf);
    }

    pub(super) fn dcs_put(&mut self, byte: u8) {
        if let Some(buf) = &mut self.dcs_buf
            && buf.len() < MAX_DCS_LEN
        {
            buf.push(byte);
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
