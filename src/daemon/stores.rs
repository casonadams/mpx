//! Session-owned state outside the layout: runtime key bindings, paste buffers and the session
//! environment.
use std::collections::{BTreeMap, VecDeque};

use crate::protocol::{Buffer, EnvChange, EnvEntry};

/// tmux's default `buffer-limit`.
const BUFFER_LIMIT: usize = 50;

#[derive(Default)]
pub(super) struct Stores {
    /// Runtime key bindings by name (`prefix:KEY`), value the command line.
    bindings: BTreeMap<String, String>,
    /// Newest first.
    buffers: VecDeque<Buffer>,
    /// Counter behind the automatic buffer names (`buffer0000`, `buffer0001`, ...).
    next_buffer: u64,
    /// Session environment: `Some(value)` is set for new panes, `None` keeps the variable out.
    environment: BTreeMap<String, Option<String>>,
}

pub(super) fn no_buffer(name: Option<&str>) -> String {
    match name {
        Some(name) => format!("no buffer {name}"),
        None => "no buffers".to_string(),
    }
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

    /// Stores `data` as the newest buffer and returns its name. A named buffer replaces the one of
    /// that name. With `append`, `data` is added to the end of the named buffer (the newest one
    /// when unnamed) instead, and starts a buffer when there is none. The oldest buffers fall off
    /// past `BUFFER_LIMIT`.
    pub(super) fn set_buffer(
        &mut self,
        name: Option<String>,
        data: String,
        append: bool,
    ) -> String {
        if append && let Some(buffer) = self.buffer_mut(name.as_deref()) {
            buffer.data.push_str(&data);
            return buffer.name.clone();
        }
        let name = name.unwrap_or_else(|| self.auto_name());
        self.buffers.retain(|b| b.name != name);
        self.buffers.push_front(Buffer {
            name: name.clone(),
            data,
        });
        self.buffers.truncate(BUFFER_LIMIT);
        name
    }

    /// The first unused `bufferNNNN` name, as tmux numbers automatic buffers.
    fn auto_name(&mut self) -> String {
        while self.buffers.iter().any(|b| b.name == format!("buffer{:04}", self.next_buffer)) {
            self.next_buffer += 1;
        }
        self.next_buffer += 1;
        format!("buffer{:04}", self.next_buffer - 1)
    }

    pub(super) fn buffer(&self, name: Option<&str>) -> Option<&Buffer> {
        self.index_of(name).map(|i| &self.buffers[i])
    }

    fn buffer_mut(&mut self, name: Option<&str>) -> Option<&mut Buffer> {
        self.index_of(name).map(|i| &mut self.buffers[i])
    }

    /// Position of the named buffer, or of the newest one when `name` is `None`.
    fn index_of(&self, name: Option<&str>) -> Option<usize> {
        match name {
            Some(name) => self.buffers.iter().position(|b| b.name == name),
            None => (!self.buffers.is_empty()).then_some(0),
        }
    }

    pub(super) fn buffers(&self) -> Vec<Buffer> {
        self.buffers.iter().cloned().collect()
    }

    pub(super) fn delete_buffer(&mut self, name: Option<&str>) -> bool {
        match self.index_of(name) {
            Some(i) => {
                self.buffers.remove(i);
                true
            }
            None => false,
        }
    }

    pub(super) fn rename_buffer(&mut self, name: Option<&str>, to: String) -> Result<(), String> {
        let index = self.index_of(name).ok_or_else(|| no_buffer(name))?;
        if self.buffers.iter().any(|b| b.name == to) {
            return Err(format!("buffer {to} already exists"));
        }
        self.buffers[index].name = to;
        Ok(())
    }

    pub(super) fn set_environment(
        &mut self,
        name: String,
        change: EnvChange,
    ) -> Result<(), String> {
        if name.is_empty() || name.contains('=') {
            return Err(format!("invalid environment variable name '{name}'"));
        }
        match change {
            EnvChange::Set(value) => {
                self.environment.insert(name, Some(value));
            }
            EnvChange::Unset => {
                self.environment.remove(&name);
            }
            EnvChange::Remove => {
                self.environment.insert(name, None);
            }
        }
        Ok(())
    }

    /// Every variable of the session environment, sorted by name.
    pub(super) fn environment(&self) -> Vec<EnvEntry> {
        self.environment
            .iter()
            .map(|(name, value)| EnvEntry {
                name: name.clone(),
                value: value.clone(),
            })
            .collect()
    }

    /// Variables new panes get: the values to set, then the names to keep out.
    pub(super) fn spawn_environment(&self) -> (Vec<(String, String)>, Vec<String>) {
        let mut set = Vec::new();
        let mut removed = Vec::new();
        for (name, value) in &self.environment {
            match value {
                Some(value) => set.push((name.clone(), value.clone())),
                None => removed.push(name.clone()),
            }
        }
        (set, removed)
    }

    pub(super) fn restore_environment(&mut self, entries: &[EnvEntry]) {
        self.environment = entries
            .iter()
            .map(|e| (e.name.clone(), e.value.clone()))
            .collect();
    }
}
