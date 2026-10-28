//! The pipes actor: owns every virtual pipe and their durable store. Timed reads wait inside the
//! actor and are answered by the write that satisfies them or by their deadline.
use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};
use tracing::warn;

use crate::node::now_ms;
use crate::pipe::VirtualPipe;
use crate::protocol::{PipeInfo, PipeMessage};
use crate::store::{PipeStore, StoredPipe};

const DEFAULT_PIPE_CAPACITY: usize = 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct ReadArgs {
    pub since: Option<u64>,
    pub consumer: Option<String>,
    pub wait_ms: Option<u64>,
    pub limit: Option<usize>,
}

type ReadReply = oneshot::Sender<Result<(Vec<PipeMessage>, u64), String>>;

enum PipeMsg {
    Create {
        name: String,
        capacity: Option<usize>,
        reply: oneshot::Sender<()>,
    },
    Write {
        name: String,
        data: String,
        reply: oneshot::Sender<PipeMessage>,
    },
    Read {
        name: String,
        args: ReadArgs,
        reply: ReadReply,
    },
    List {
        reply: oneshot::Sender<Vec<PipeInfo>>,
    },
}

/// Cloneable reference to the pipes actor.
#[derive(Clone)]
pub struct PipesHandle {
    tx: mpsc::UnboundedSender<PipeMsg>,
}

const GONE: &str = "Pipes are unavailable";

impl PipesHandle {
    pub async fn create(&self, name: &str, capacity: Option<usize>) {
        let (reply, rx) = oneshot::channel();
        let _ = self.tx.send(PipeMsg::Create {
            name: name.to_string(),
            capacity,
            reply,
        });
        let _ = rx.await;
    }

    /// Appends a message, creating the pipe when missing.
    pub async fn write(&self, name: &str, data: &str) -> Result<PipeMessage, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(PipeMsg::Write {
                name: name.to_string(),
                data: data.to_string(),
                reply,
            })
            .map_err(|_| GONE.to_string())?;
        rx.await.map_err(|_| GONE.to_string())
    }

    /// Reads messages; with `wait_ms`, first waits until one beyond the cursor exists or time runs out.
    pub async fn read(
        &self,
        name: &str,
        args: ReadArgs,
    ) -> Result<(Vec<PipeMessage>, u64), String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(PipeMsg::Read {
                name: name.to_string(),
                args,
                reply,
            })
            .map_err(|_| GONE.to_string())?;
        rx.await.map_err(|_| GONE.to_string())?
    }

    pub async fn list(&self) -> Vec<PipeInfo> {
        let (reply, rx) = oneshot::channel();
        let _ = self.tx.send(PipeMsg::List { reply });
        rx.await.unwrap_or_default()
    }
}

struct PendingRead {
    name: String,
    args: ReadArgs,
    base: u64,
    deadline: Instant,
    reply: ReadReply,
}

/// Starts the pipes actor. With a store (persisting), pipes saved on disk are loaded first.
pub fn spawn(store: Option<PipeStore>) -> PipesHandle {
    let mut pipes = HashMap::new();
    if let Some(store) = &store {
        for stored in store.load_all() {
            let pipe = restore_pipe(stored);
            pipes.insert(pipe.name().to_string(), pipe);
        }
    }
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(
        PipesActor {
            pipes,
            store,
            pending: Vec::new(),
        }
        .run(rx),
    );
    PipesHandle { tx }
}

fn restore_pipe(stored: StoredPipe) -> VirtualPipe {
    VirtualPipe::restore(
        stored.name,
        stored.capacity,
        stored.next_seq,
        stored.messages,
        stored.cursors,
    )
}

/// Pipe `name`, creating (and persisting) it when missing.
fn ensure<'a>(
    pipes: &'a mut HashMap<String, VirtualPipe>,
    store: &Option<PipeStore>,
    name: &str,
    capacity: Option<usize>,
) -> &'a mut VirtualPipe {
    pipes.entry(name.to_string()).or_insert_with(|| {
        let capacity = capacity.unwrap_or(DEFAULT_PIPE_CAPACITY);
        if let Some(store) = store
            && let Err(e) = store.create(name, capacity)
        {
            warn!("could not persist pipe '{}': {}", name, e);
        }
        VirtualPipe::new(name.to_string(), capacity)
    })
}

struct PipesActor {
    pipes: HashMap<String, VirtualPipe>,
    store: Option<PipeStore>,
    pending: Vec<PendingRead>,
}

impl PipesActor {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<PipeMsg>) {
        loop {
            let wake = self.pending.iter().map(|p| p.deadline).min();
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg),
                    None => break,
                },
                () = sleep_until_opt(wake) => self.expire(Instant::now()),
            }
        }
    }

    fn handle(&mut self, msg: PipeMsg) {
        match msg {
            PipeMsg::Create {
                name,
                capacity,
                reply,
            } => {
                ensure(&mut self.pipes, &self.store, &name, capacity);
                let _ = reply.send(());
            }
            PipeMsg::Write { name, data, reply } => {
                let message = self.write(&name, &data);
                let _ = reply.send(message);
                self.wake(&name);
            }
            PipeMsg::Read { name, args, reply } => self.read(name, args, reply),
            PipeMsg::List { reply } => {
                let _ = reply.send(self.pipes.values().map(VirtualPipe::info).collect());
            }
        }
    }

    fn write(&mut self, name: &str, data: &str) -> PipeMessage {
        let ts_ms = now_ms();
        let pipe = ensure(&mut self.pipes, &self.store, name, None);
        let before = pipe.message_count();
        let seq = pipe.write(data, ts_ms);
        let message = PipeMessage {
            seq,
            ts_ms,
            data: data.to_string(),
        };
        let trimmed = pipe.message_count() <= before;
        if let Some(store) = &self.store {
            let result = if trimmed {
                let retained = pipe.read(0, None);
                store.rewrite(name, pipe.capacity(), pipe.next_seq(), &retained)
            } else {
                store.append(name, &message)
            };
            if let Err(e) = result {
                warn!("could not persist pipe '{}': {}", name, e);
            }
        }
        message
    }

    fn read(&mut self, name: String, args: ReadArgs, reply: ReadReply) {
        ensure(&mut self.pipes, &self.store, &name, None);
        let base = self.cursor_base(&name, &args);
        match args.wait_ms {
            Some(ms) if !self.ready(&name, base) => self.pending.push(PendingRead {
                name,
                args,
                base,
                deadline: Instant::now() + Duration::from_millis(ms),
                reply,
            }),
            _ => {
                let _ = reply.send(self.read_now(&name, &args));
            }
        }
    }

    /// Answers pending reads on `name` that now have a message beyond their base.
    fn wake(&mut self, name: &str) {
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| p.name == name && self.ready(name, p.base));
        self.pending = waiting;
        for pending in ready {
            let _ = pending
                .reply
                .send(self.read_now(&pending.name, &pending.args));
        }
    }

    fn expire(&mut self, now: Instant) {
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| p.deadline <= now);
        self.pending = waiting;
        for pending in due {
            let _ = pending
                .reply
                .send(self.read_now(&pending.name, &pending.args));
        }
    }

    fn ready(&self, name: &str, base: u64) -> bool {
        self.pipes.get(name).is_some_and(|p| p.last_seq() > base)
    }

    fn cursor_base(&self, name: &str, args: &ReadArgs) -> u64 {
        let since = args.since.unwrap_or(0);
        match (&args.consumer, self.pipes.get(name)) {
            (Some(consumer), Some(pipe)) => pipe.cursor(consumer).unwrap_or(since),
            _ => since,
        }
    }

    fn read_now(&mut self, name: &str, args: &ReadArgs) -> Result<(Vec<PipeMessage>, u64), String> {
        let base = self.cursor_base(name, args);
        let pipe = self
            .pipes
            .get_mut(name)
            .ok_or_else(|| format!("Pipe '{}' not found", name))?;
        let messages = match &args.consumer {
            Some(consumer) => pipe.read_consumer(consumer, args.since.unwrap_or(0), args.limit),
            None => pipe.read(base, args.limit),
        };
        let next = messages.last().map_or(base, |m| m.seq);
        if let (Some(consumer), Some(last), Some(store)) =
            (&args.consumer, messages.last(), &self.store)
            && let Err(e) = store.set_cursor(name, consumer, last.seq)
        {
            warn!("could not persist cursor of pipe '{}': {}", name, e);
        }
        Ok((messages, next))
    }
}

async fn sleep_until_opt(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}
