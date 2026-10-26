use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

#[derive(Debug, Clone)]
pub struct VirtualPipe {
    name: String,
    buffer: Arc<Mutex<VecDeque<u8>>>,
    capacity: usize,
    notifier: broadcast::Sender<Vec<u8>>,
}

impl VirtualPipe {
    pub fn new(name: String, capacity: usize) -> Self {
        let (notifier, _) = broadcast::channel(1024);
        Self {
            name,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            notifier,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn write(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        let mut buf = self.buffer.lock().await;
        if data.len() >= self.capacity {
            buf.clear();
            let start = data.len() - self.capacity;
            buf.extend(&data[start..]);
        } else {
            let available = self.capacity.saturating_sub(buf.len());
            if data.len() > available {
                let to_remove = data.len() - available;
                buf.drain(0..to_remove);
            }
            buf.extend(data);
        }

        let _ = self.notifier.send(data.to_vec());
    }

    pub async fn read_all(&self) -> Vec<u8> {
        let buf = self.buffer.lock().await;
        let (first, second) = buf.as_slices();
        let mut out = Vec::with_capacity(buf.len());
        out.extend_from_slice(first);
        out.extend_from_slice(second);
        out
    }

    pub async fn read_str(&self) -> String {
        let bytes = self.read_all().await;
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.notifier.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_virtual_pipe_write_read() {
        let pipe = VirtualPipe::new("agent_comm".into(), 1024);
        pipe.write(b"hello from omp").await;
        pipe.write(b"\nresponse from claude").await;

        let content = pipe.read_str().await;
        assert_eq!(content, "hello from omp\nresponse from claude");
    }

    #[tokio::test]
    async fn test_virtual_pipe_streaming() {
        let pipe = VirtualPipe::new("stream_pipe".into(), 1024);
        let mut rx = pipe.subscribe();

        pipe.write(b"packet_1").await;
        let received = rx.recv().await.unwrap();
        assert_eq!(received, b"packet_1");
    }
}
