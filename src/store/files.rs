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

/// Renames the state directory of session `legacy` to that of `current` under `root`, when only the
/// former exists. Returns whether it moved.
pub fn adopt_legacy_dir(root: &Path, legacy: &str, current: &str) -> bool {
    let from = root.join(component(legacy));
    let to = root.join(component(current));
    from.is_dir() && !to.exists() && fs::rename(from, to).is_ok()
}

/// One safe path component for `name`: bytes outside `[A-Za-z0-9_-]` are percent-encoded, so the
/// result never holds separators or dot segments. Very long names keep a prefix and an FNV-1a hash.
pub fn component(name: &str) -> String {
    encode_component(name, false)
}

/// `component` that also encodes uppercase letters, so names differing only by letter case stay
/// apart on case-insensitive file systems.
pub fn case_safe_component(name: &str) -> String {
    encode_component(name, true)
}

fn encode_component(name: &str, encode_upper: bool) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        let plain = b.is_ascii_lowercase()
            || b.is_ascii_digit()
            || b == b'_'
            || b == b'-'
            || (b.is_ascii_uppercase() && !encode_upper);
        if plain {
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

/// Creates `path` and missing parents, owner-only on unix. An existing `path` owned by this user is
/// tightened to owner-only too.
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
        .map_err(|e| format!("create {}: {e}", path.display()))?;
    tighten_dir(path)
}

#[cfg(unix)]
fn tighten_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let err = |e: std::io::Error| format!("private {}: {e}", path.display());
    let meta = fs::metadata(path).map_err(err)?;
    if meta.uid() == unsafe { libc::getuid() } && meta.mode() & 0o777 != 0o700 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(err)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_dir(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Refuses a file another user could have written: one not owned by this user, or writable by its
/// group or others.
#[cfg(unix)]
pub fn check_private_file(path: &Path, meta: &fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if meta.uid() != unsafe { libc::getuid() } || meta.mode() & 0o022 != 0 {
        return Err(format!(
            "{} is not private to this user; refusing to load it",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn check_private_file(_path: &Path, _meta: &fs::Metadata) -> Result<(), String> {
    Ok(())
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

/// One temp name per target: a crash leaves at most one stale file, which the next write replaces.
fn temp_path(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    Ok(path.with_file_name(format!(".{}.tmp", name.to_string_lossy())))
}

fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn write_then_rename(tmp: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let io_err = |e: std::io::Error| format!("write {}: {e}", path.display());
    let _ = fs::remove_file(tmp);
    let mut file = create_private_file(tmp).map_err(io_err)?;
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
