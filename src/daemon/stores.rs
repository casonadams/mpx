//! Session-owned state outside the layout: runtime key bindings and paste buffers.
use std::collections::{BTreeMap, VecDeque};

use crate::protocol::Buffer;

/// tmux's default `buffer-limit`.
const BUFFER_LIMIT: usize = 50;

#[derive(Default)]
pub(super) struct Stores {
    /// Runtime key bindings by name (`prefix:KEY`), value the command line.
    bindings: BTreeMap<String, String>,
    /// Newest first.
    buffers: VecDeque<Buffer>,
    next_buffer: u64,
}

impl Stores {
    pub(super) fn set_binding(&mut self, name: String, line: Option<String>) {
        match line {
            Some(line) => {
                self.bindings.insert(name, line);
            }
            None => {
                self.bindings.remove(&name);
            }
        }
    }

    pub(super) fn bindings(&self) -> Vec<(String, String)> {
        self.bindings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Pushes `data` as the newest buffer and returns its name; a named buffer replaces the one of
    /// that name. The oldest buffers fall off past `BUFFER_LIMIT`.
    pub(super) fn push_buffer(&mut self, name: Option<String>, data: String) -> String {
        let name = name.unwrap_or_else(|| {
            let name = format!("buffer{}", self.next_buffer);
            self.next_buffer += 1;
            name
        });
        self.buffers.retain(|b| b.name != name);
        self.buffers.push_front(Buffer {
            name: name.clone(),
            data,
        });
        self.buffers.truncate(BUFFER_LIMIT);
        name
    }

    pub(super) fn buffer(&self, name: Option<&str>) -> Option<&Buffer> {
        match name {
            Some(name) => self.buffers.iter().find(|b| b.name == name),
            None => self.buffers.front(),
        }
    }

    pub(super) fn buffers(&self) -> Vec<Buffer> {
        self.buffers.iter().cloned().collect()
    }

    pub(super) fn delete_buffer(&mut self, name: Option<&str>) -> bool {
        let found = self
            .buffer(name)
            .map(|b| b.name.clone())
            .and_then(|n| self.buffers.iter().position(|b| b.name == n));
        found.is_some_and(|i| self.buffers.remove(i).is_some())
    }
}
