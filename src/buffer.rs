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
        let text = self.snapshot_str();
        if n == 0 {
            return String::new();
        }
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() <= n {
            return text;
        }
        lines[lines.len() - n..].join("\n") + "\n"
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
}
