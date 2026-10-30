//! Session-owned name/value state of the tmux surface: options, environment and paste buffers.
use std::collections::{BTreeMap, VecDeque};

use crate::protocol::{Buffer, VarKind};

/// tmux's default `buffer-limit`.
const BUFFER_LIMIT: usize = 50;

#[derive(Default)]
pub(super) struct Stores {
    /// Indexed by `VarKind`: options, environment, bindings, hooks.
    vars: [BTreeMap<String, String>; 4],
    /// Newest first.
    buffers: VecDeque<Buffer>,
    next_buffer: u64,
}

impl Stores {
    pub(super) fn set_var(&mut self, kind: VarKind, name: String, value: Option<String>) {
        let vars = &mut self.vars[kind as usize];
        match value {
            Some(value) => {
                vars.insert(name, value);
            }
            None => {
                vars.remove(&name);
            }
        }
    }

    /// The command line set for `hook`, if any.
    pub(super) fn hook(&self, hook: &str) -> Option<&String> {
        self.vars[VarKind::Hook as usize].get(hook)
    }

    pub(super) fn vars(&self, kind: VarKind) -> Vec<(String, String)> {
        let vars = &self.vars[kind as usize];
        vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Variables handed to panes created from now on.
    pub(super) fn environment(&self) -> Vec<(String, String)> {
        self.vars(VarKind::Environment)
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
