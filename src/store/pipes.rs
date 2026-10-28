use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::files::{component, create_private_dir, write_atomic};
use super::{PipeStore, StoredPipe};
use crate::protocol::PipeMessage;

const LOG_FILE: &str = "log.jsonl";
const META_FILE: &str = "meta.json";

#[derive(Debug, Serialize, Deserialize)]
struct PipeMeta {
    name: String,
    capacity: usize,
    next_seq: u64,
    cursors: Vec<(String, u64)>,
}

impl PipeMeta {
    fn fresh(name: &str, capacity: usize) -> Self {
        Self {
            name: name.to_string(),
            capacity,
            next_seq: 1,
            cursors: Vec::new(),
        }
    }
}

impl PipeStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn for_session(session: &str) -> Self {
        Self::new(super::session_dir(session).join("pipes"))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn pipe_dir(&self, name: &str) -> PathBuf {
        self.dir.join(component(name))
    }

    /// Every stored pipe, sorted by name. Corrupt trailing lines (a crash mid-append) are ignored;
    /// pipes whose metadata is unreadable are skipped with a warning.
    pub fn load_all(&self) -> Vec<StoredPipe> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut pipes: Vec<StoredPipe> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join(META_FILE).is_file())
            .filter_map(|path| match load_pipe(&path) {
                Ok(pipe) => Some(pipe),
                Err(e) => {
                    tracing::warn!("skipping pipe at {}: {e}", path.display());
                    None
                }
            })
            .collect();
        pipes.sort_by(|a, b| a.name.cmp(&b.name));
        pipes
    }

    /// Creates the pipe's metadata when it does not exist yet.
    pub fn create(&self, name: &str, capacity: usize) -> Result<(), String> {
        let dir = self.pipe_dir(name);
        create_private_dir(&dir)?;
        if !dir.join(META_FILE).exists() {
            write_meta(&dir, &PipeMeta::fresh(name, capacity))?;
        }
        open_log(&dir).map(drop)
    }

    /// Appends one message as a JSONL line; the log is never rewritten.
    pub fn append(&self, name: &str, message: &PipeMessage) -> Result<(), String> {
        let dir = self.pipe_dir(name);
        if !dir.join(META_FILE).is_file() {
            return Err(format!("pipe {name} does not exist"));
        }
        let mut file = open_log(&dir)?;
        let mut line =
            serde_json::to_string(message).map_err(|e| format!("encode message: {e}"))?;
        if !ends_with_newline(&mut file).map_err(|e| format!("read log: {e}"))? {
            line.insert(0, '\n');
        }
        line.push('\n');
        file.write_all(line.as_bytes())
            .map_err(|e| format!("append to pipe {name}: {e}"))
    }

    pub fn set_cursor(&self, name: &str, consumer: &str, seq: u64) -> Result<(), String> {
        let dir = self.pipe_dir(name);
        let mut meta = read_meta(&dir)?;
        match meta.cursors.iter_mut().find(|(c, _)| c == consumer) {
            Some(entry) => entry.1 = seq,
            None => meta.cursors.push((consumer.to_string(), seq)),
        }
        write_meta(&dir, &meta)
    }

    /// Replaces the log with exactly `messages` (after capacity trimming), keeping `next_seq`.
    pub fn rewrite(
        &self,
        name: &str,
        capacity: usize,
        next_seq: u64,
        messages: &[PipeMessage],
    ) -> Result<(), String> {
        let dir = self.pipe_dir(name);
        create_private_dir(&dir)?;
        let mut meta = match read_meta(&dir) {
            Ok(meta) => meta,
            Err(_) if !dir.join(META_FILE).exists() => PipeMeta::fresh(name, capacity),
            Err(e) => return Err(e),
        };
        meta.capacity = capacity;
        meta.next_seq = next_seq;
        let mut log = String::new();
        for message in messages {
            log.push_str(&encode_line(message)?);
        }
        write_atomic(&dir.join(LOG_FILE), log.as_bytes())?;
        write_meta(&dir, &meta)
    }
}

fn load_pipe(dir: &Path) -> Result<StoredPipe, String> {
    let meta = read_meta(dir)?;
    let messages = read_log(&dir.join(LOG_FILE))?;
    let log_next = messages.iter().map(|m| m.seq + 1).max().unwrap_or(0);
    Ok(StoredPipe {
        name: meta.name,
        capacity: meta.capacity,
        next_seq: meta.next_seq.max(log_next),
        messages: trim_to_capacity(messages, meta.capacity),
        cursors: meta.cursors,
    })
}

/// Parseable lines only; a line that fails to parse (a torn append) is dropped.
fn read_log(path: &Path) -> Result<Vec<PipeMessage>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    let text = String::from_utf8_lossy(&bytes);
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// Keeps the newest messages whose total data fits `capacity` bytes, always keeping one.
fn trim_to_capacity(mut messages: Vec<PipeMessage>, capacity: usize) -> Vec<PipeMessage> {
    let mut bytes: usize = messages.iter().map(|m| m.data.len()).sum();
    let mut dropped = 0;
    while bytes > capacity && messages.len() - dropped > 1 {
        bytes -= messages[dropped].data.len();
        dropped += 1;
    }
    messages.drain(..dropped);
    messages
}

fn read_meta(dir: &Path) -> Result<PipeMeta, String> {
    let path = dir.join(META_FILE);
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn write_meta(dir: &Path, meta: &PipeMeta) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(meta).map_err(|e| format!("encode pipe meta: {e}"))?;
    write_atomic(&dir.join(META_FILE), &bytes)
}

fn encode_line(message: &PipeMessage) -> Result<String, String> {
    let mut line = serde_json::to_string(message).map_err(|e| format!("encode message: {e}"))?;
    line.push('\n');
    Ok(line)
}

fn open_log(dir: &Path) -> Result<File, String> {
    let path = dir.join(LOG_FILE);
    OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))
}

fn ends_with_newline(file: &mut File) -> std::io::Result<bool> {
    if file.metadata()?.len() == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}
