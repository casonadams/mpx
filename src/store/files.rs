use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_COMPONENT: usize = 128;
const KEPT_PREFIX: usize = 100;

/// Resolves the state directory from an environment lookup and the home directory.
pub fn state_dir_with(env: impl Fn(&str) -> Option<OsString>, home: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = env("MPX_STATE_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = env("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("mpx");
    }
    if let Some(local) = env("LOCALAPPDATA").filter(|v| cfg!(windows) && !v.is_empty()) {
        return PathBuf::from(local).join("mpx").join("state");
    }
    home.unwrap_or_else(std::env::temp_dir)
        .join(".local")
        .join("state")
        .join("mpx")
}

/// One safe path component for `name`: bytes outside `[A-Za-z0-9_-]` are percent-encoded, so the
/// result never holds separators or dot segments. Very long names keep a prefix and an FNV-1a hash.
pub fn component(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    if out.is_empty() {
        return "%".to_string();
    }
    if out.len() > MAX_COMPONENT {
        return format!("{}-{:016x}", &out[..KEPT_PREFIX], fnv1a(name.as_bytes()));
    }
    out
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Creates `path` and missing parents, owner-only on unix.
pub fn create_private_dir(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| format!("create {}: {e}", path.display()))
}

/// Replaces `path` with `bytes` through a same-directory temp file: write, fsync, rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    create_private_dir(parent)?;
    let tmp = temp_path(path)?;
    let result = write_then_rename(&tmp, path, bytes);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn temp_path(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    Ok(path.with_file_name(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    )))
}

fn write_then_rename(tmp: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let io_err = |e: std::io::Error| format!("write {}: {e}", path.display());
    let mut file = fs::File::create(tmp).map_err(io_err)?;
    file.write_all(bytes).map_err(io_err)?;
    file.sync_all().map_err(io_err)?;
    drop(file);
    fs::rename(tmp, path).map_err(io_err)?;
    sync_parent(path).map_err(io_err)
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(dir) => fs::File::open(dir)?.sync_all(),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
