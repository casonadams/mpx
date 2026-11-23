use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::files::{component, write_atomic};

const SCROLLBACK_DIR: &str = "scrollback";
const MAGIC: &str = "mpx-scrollback 1";

/// A pane's retained output and the size (cols, rows) its screen had when it was saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneScrollback {
    pub size: (u16, u16),
    pub log: Vec<u8>,
}

/// One file per pane under `<state_dir>/scrollback/`, named after the pane. Writes are atomic.
#[derive(Debug, Clone)]
pub struct ScrollbackStore {
    dir: PathBuf,
}

impl ScrollbackStore {
    pub fn in_dir(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join(SCROLLBACK_DIR),
        }
    }

    fn path(&self, pane: &str) -> PathBuf {
        self.dir.join(component(pane))
    }

    pub fn save(&self, pane: &str, scrollback: &PaneScrollback) -> Result<(), String> {
        let (cols, rows) = scrollback.size;
        let mut bytes = format!("{MAGIC} {cols} {rows}\n").into_bytes();
        bytes.extend_from_slice(&scrollback.log);
        write_atomic(&self.path(pane), &bytes)
    }

    /// `Ok(None)` when the pane has no saved scrollback; an unreadable file is an error.
    pub fn load(&self, pane: &str) -> Result<Option<PaneScrollback>, String> {
        let path = self.path(pane);
        match fs::read(&path) {
            Ok(bytes) => decode(&bytes)
                .map(Some)
                .ok_or_else(|| format!("{} is not a saved scrollback", path.display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("read {}: {e}", path.display())),
        }
    }

    /// Deletes every file but the scrollback of `panes`.
    pub fn retain(&self, panes: &[String]) -> Result<(), String> {
        let keep: HashSet<String> = panes.iter().map(|pane| component(pane)).collect();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("read {}: {e}", self.dir.display())),
        };
        for entry in entries {
            let entry = entry.map_err(|e| format!("read {}: {e}", self.dir.display()))?;
            if keep.contains(entry.file_name().to_string_lossy().as_ref()) {
                continue;
            }
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(format!("remove {}: {e}", entry.path().display())),
            }
        }
        Ok(())
    }
}

fn decode(bytes: &[u8]) -> Option<PaneScrollback> {
    let end = bytes.iter().position(|&b| b == b'\n')?;
    let header = std::str::from_utf8(&bytes[..end]).ok()?;
    let mut size = header.strip_prefix(MAGIC)?.split_whitespace();
    let cols = size.next()?.parse().ok()?;
    let rows = size.next()?.parse().ok()?;
    if size.next().is_some() {
        return None;
    }
    Some(PaneScrollback {
        size: (cols, rows),
        log: bytes[end + 1..].to_vec(),
    })
}
