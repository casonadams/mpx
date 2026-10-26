use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct RingBuffer {
    buffer: VecDeque<u8>,
    capacity: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn write(&mut self, data: &[u8]) {
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

impl Default for RingBuffer {
    fn default() -> Self {
        // Default 2MB scrollback buffer per stream node
        Self::new(2 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basic() {
        let mut rb = RingBuffer::new(10);
        rb.write(b"hello");
        assert_eq!(rb.snapshot(), b"hello");

        rb.write(b"world");
        assert_eq!(rb.snapshot(), b"helloworld");

        rb.write(b"123");
        assert_eq!(rb.snapshot(), b"loworld123");
    }

    #[test]
    fn test_ring_buffer_oversized() {
        let mut rb = RingBuffer::new(5);
        rb.write(b"0123456789");
        assert_eq!(rb.snapshot(), b"56789");
    }

    #[test]
    fn test_tail_lines() {
        let mut rb = RingBuffer::new(100);
        rb.write(b"line 1\nline 2\nline 3\nline 4\n");
        assert_eq!(rb.tail_lines(2), "line 3\nline 4\n");
    }

    #[test]
    fn test_tail_lines_edge_cases() {
        let mut rb = RingBuffer::new(100);
        assert_eq!(rb.tail_lines(0), "");
        assert_eq!(rb.tail_lines(5), "");

        rb.write(b"single line without newline");
        assert_eq!(rb.tail_lines(1), "single line without newline\n");
        assert_eq!(rb.tail_lines(5), "single line without newline\n");

        let mut rb2 = RingBuffer::new(100);
        rb2.write(b"a\nb\nc\nd\ne");
        assert_eq!(rb2.tail_lines(2), "d\ne\n");
        assert_eq!(rb2.tail_lines(3), "c\nd\ne\n");
        assert_eq!(rb2.tail_lines(10), "a\nb\nc\nd\ne\n");
    }
}
