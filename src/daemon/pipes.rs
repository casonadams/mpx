use std::time::Duration;

use super::DaemonState;
use crate::node::now_ms;
use crate::pipe::VirtualPipe;
use crate::protocol::{PipeInfo, PipeMessage, StreamEvent};

const DEFAULT_PIPE_CAPACITY: usize = 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub(super) struct ReadArgs {
    pub since: Option<u64>,
    pub consumer: Option<String>,
    pub wait_ms: Option<u64>,
    pub limit: Option<usize>,
}

fn pipe_not_found(name: &str) -> String {
    format!("Pipe '{}' not found", name)
}

fn cursor_base(pipe: &VirtualPipe, args: &ReadArgs) -> u64 {
    let since = args.since.unwrap_or(0);
    match &args.consumer {
        Some(consumer) => pipe.cursor(consumer).unwrap_or(since),
        None => since,
    }
}

impl DaemonState {
    pub(super) fn create_pipe(&self, name: &str, capacity: Option<usize>) {
        self.lock()
            .pipes
            .entry(name.to_string())
            .or_insert_with(|| {
                VirtualPipe::new(name.to_string(), capacity.unwrap_or(DEFAULT_PIPE_CAPACITY))
            });
    }

    /// Appends a message, creating the pipe when missing. Returns the sequence number.
    pub(super) fn append_pipe(&self, name: &str, data: &str) -> u64 {
        let ts_ms = now_ms();
        let seq = {
            let mut inner = self.lock();
            inner
                .pipes
                .entry(name.to_string())
                .or_insert_with(|| VirtualPipe::new(name.to_string(), DEFAULT_PIPE_CAPACITY))
                .write(data, ts_ms)
        };
        let _ = self.event_tx.send(StreamEvent::Pipe {
            name: name.to_string(),
            message: PipeMessage {
                seq,
                ts_ms,
                data: data.to_string(),
            },
        });
        seq
    }

    /// Reads messages, creating the pipe when missing so readers may start before writers.
    /// With `wait_ms`, first waits until a message beyond the cursor exists.
    pub(super) async fn read_pipe(
        &self,
        name: &str,
        args: ReadArgs,
    ) -> Result<(Vec<PipeMessage>, u64), String> {
        self.create_pipe(name, None);
        let (mut tip, base) = {
            let inner = self.lock();
            let pipe = inner.pipes.get(name).ok_or_else(|| pipe_not_found(name))?;
            (pipe.watch_tip(), cursor_base(pipe, &args))
        };
        if let Some(ms) = args.wait_ms {
            let _ =
                tokio::time::timeout(Duration::from_millis(ms), tip.wait_for(|seq| *seq > base))
                    .await;
        }
        let mut inner = self.lock();
        let pipe = inner
            .pipes
            .get_mut(name)
            .ok_or_else(|| pipe_not_found(name))?;
        let base = cursor_base(pipe, &args);
        let messages = match &args.consumer {
            Some(consumer) => pipe.read_consumer(consumer, args.since.unwrap_or(0), args.limit),
            None => pipe.read(base, args.limit),
        };
        let next = messages.last().map_or(base, |m| m.seq);
        Ok((messages, next))
    }

    pub(super) fn list_pipes(&self) -> Vec<PipeInfo> {
        self.lock().pipes.values().map(VirtualPipe::info).collect()
    }
}
