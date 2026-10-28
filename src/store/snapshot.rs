use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::files::write_atomic;
use super::{SNAPSHOT_VERSION, SessionSnapshot, SnapshotStore};

const SNAPSHOT_FILE: &str = "snapshot.json";

impl SnapshotStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn for_session(session: &str) -> Self {
        Self::new(super::session_dir(session).join(SNAPSHOT_FILE))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `Ok(None)` when no snapshot exists; an unreadable or newer-version file is an error.
    pub fn load(&self) -> Result<Option<SessionSnapshot>, String> {
        match fs::read_to_string(&self.path) {
            Ok(text) => decode_snapshot(&text).map(Some),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("read {}: {e}", self.path.display())),
        }
    }

    pub fn save(&self, snapshot: &SessionSnapshot) -> Result<(), String> {
        let text = encode_snapshot(snapshot)?;
        write_atomic(&self.path, text.as_bytes())
    }
}

pub fn encode_snapshot(snapshot: &SessionSnapshot) -> Result<String, String> {
    serde_json::to_string_pretty(snapshot).map_err(|e| format!("encode snapshot: {e}"))
}

/// Rejects snapshots written by a newer mpx before the body is interpreted.
pub fn decode_snapshot(text: &str) -> Result<SessionSnapshot, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("parse snapshot: {e}"))?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or("snapshot has no version")?;
    if version > u64::from(SNAPSHOT_VERSION) {
        return Err(format!(
            "snapshot version {version} is newer than supported version {SNAPSHOT_VERSION}"
        ));
    }
    serde_json::from_value(value).map_err(|e| format!("parse snapshot: {e}"))
}
