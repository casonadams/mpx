use std::collections::VecDeque;

/// Bounded byte buffer that remembers how many bytes were ever written, so readers can
/// resume from an absolute offset.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    buffer: VecDeque<u8>,
    capacity: usize,
    written: u64,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
            written: 0,
        }
    }

    pub fn write(&mut self, data: &[u8]) {
        self.written += data.len() as u64;
        if data.is_empty() {
            return;
        }

        if data.len() >= self.capacity {
            self.buffer.clear();
            let start = data.len() - self.capacity;
            self.buffer.extend(&data[start..]);
            return;
        }

        let available = self.capacity - self.buffer.len();
        if data.len() > available {
            let to_remove = data.len() - available;
            self.buffer.drain(0..to_remove);
        }

        self.buffer.extend(data);
    }

    /// Total bytes ever written; the absolute offset just past the newest byte.
    pub fn total_written(&self) -> u64 {
        self.written
    }

    /// Bytes at or after `offset` still retained (an offset older than the retained data starts
    /// at the oldest byte), and the offset to resume from (`total_written`).
    pub fn since(&self, offset: u64) -> (Vec<u8>, u64) {
        let oldest = self.written - self.buffer.len() as u64;
        let start = offset.clamp(oldest, self.written);
        let skip = (start - oldest) as usize;
        let out = self.buffer.iter().skip(skip).copied().collect();
        (out, self.written)
    }

    pub fn snapshot(&self) -> Vec<u8> {
        let (first, second) = self.buffer.as_slices();
        let mut out = Vec::with_capacity(self.buffer.len());
        out.extend_from_slice(first);
        out.extend_from_slice(second);
        out
    }

    pub fn snapshot_str(&self) -> String {
        String::from_utf8_lossy(&self.snapshot()).into_owned()
    }

    pub fn tail_lines(&self, n: usize) -> String {
        if n == 0 || self.buffer.is_empty() {
            return String::new();
        }

        let (first, second) = self.buffer.as_slices();
        let total_len = first.len() + second.len();
        let start_idx = find_tail_start(first, second, n);

        let mut out = Vec::with_capacity(total_len - start_idx);
        if start_idx < first.len() {
            out.extend_from_slice(&first[start_idx..]);
            out.extend_from_slice(second);
        } else {
            out.extend_from_slice(&second[start_idx - first.len()..]);
        }

        let mut result = String::from_utf8_lossy(&out).into_owned();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

fn find_tail_start(first: &[u8], second: &[u8], n: usize) -> usize {
    let total_len = first.len() + second.len();
    let get_byte = |idx: usize| -> u8 {
        if idx < first.len() {
            first[idx]
        } else {
            second[idx - first.len()]
        }
    };

    let mut idx = total_len;
    if idx > 0 && get_byte(idx - 1) == b'\n' {
        idx -= 1;
        if idx > 0 && get_byte(idx - 1) == b'\r' {
            idx -= 1;
        }
    }

    let mut lines_found = 0;
    while idx > 0 {
        idx -= 1;
        if get_byte(idx) == b'\n' {
            lines_found += 1;
            if lines_found == n {
                return idx + 1;
            }
        }
    }
    0
}

/// Plain text from terminal output: CSI/OSC/DCS sequences and two-byte escapes are removed,
/// carriage returns and other control bytes dropped; newlines and tabs kept.
pub fn strip_ansi(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0x1b => i = skip_escape(bytes, i),
            b'\n' | b'\t' => {
                out.push(bytes[i]);
                i += 1;
            }
            c if c < 0x20 || c == 0x7f => i += 1,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Index just past the escape sequence starting at `start` (which holds ESC).
fn skip_escape(bytes: &[u8], start: usize) -> usize {
    let len = bytes.len();
    match bytes.get(start + 1) {
        None => len,
        Some(b'[') => {
            let end = bytes[start + 2..]
                .iter()
                .position(|b| (0x40..=0x7e).contains(b))
                .map_or(len, |p| start + 2 + p + 1);
            end.min(len)
        }
        Some(b']' | b'P' | b'_' | b'^' | b'X') => skip_string(bytes, start + 2),
        Some(b) if (0x20..=0x2f).contains(b) => {
            let mut i = start + 2;
            while i < len && (0x20..=0x2f).contains(&bytes[i]) {
                i += 1;
            }
            (i + 1).min(len)
        }
        Some(_) => (start + 2).min(len),
    }
}

/// Skips a string-type sequence (OSC, DCS, APC, PM, SOS) terminated by BEL or ST (ESC \).
fn skip_string(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == 0x07 {
            return i + 1;
        }
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

impl Default for RingBuffer {
    fn default() -> Self {
        // Default 2MB scrollback buffer per stream node
        Self::new(2 * 1024 * 1024)
    }
}
