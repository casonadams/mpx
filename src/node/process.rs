//! Process introspection: names, argv and working directories of live pids.

/// What the kernel knows about a process. Pure selection functions work on this value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    /// Kernel command name (Linux `comm`, macOS `proc_name`). Can be a version string.
    pub comm: Option<String>,
    pub argv: Vec<String>,
}

/// Interpreters whose own name says little; the script they run names the program.
pub fn is_runtime(name: &str) -> bool {
    matches!(name, "node" | "bun" | "deno") || name.starts_with("python")
}

/// Basename of the first argument of a NUL-separated argv blob (Linux `/proc/<pid>/cmdline`).
pub fn parse_cmdline(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&b| b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

/// Parses a macOS `KERN_PROCARGS2` blob: argc, exec path, NUL padding, then argv.
pub fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(buf.get(0..4)?.try_into().ok()?).max(0) as usize;
    let rest = &buf[4..];
    let exec_end = rest.iter().position(|&b| b == 0)?;
    let rest = &rest[exec_end..];
    let argv_start = rest.iter().position(|&b| b != 0)?;
    Some(
        rest[argv_start..]
            .split(|&b| b == 0)
            .take(argc)
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect(),
    )
}

pub fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Basename of an argv entry without a login-shell dash; `None` when empty.
fn arg_name(arg: Option<&String>) -> Option<String> {
    let name = basename(arg?);
    let name = name.trim_start_matches('-');
    (!name.is_empty()).then(|| name.to_string())
}

/// Ordered names to try against harness detection: comm, argv[0], argv[1] (interpreter script).
pub fn detection_names(id: &Identity) -> Vec<String> {
    let candidates = [
        id.comm.clone().filter(|c| !c.is_empty()),
        arg_name(id.argv.first()),
        arg_name(id.argv.get(1)),
    ];
    let mut names: Vec<String> = Vec::new();
    for name in candidates.into_iter().flatten() {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Display name of the foreground process. Prefers argv[0] when comm is a version string
/// (e.g. Claude Code's `2.1.308`); for interpreters, names the script they run.
pub fn foreground_name(id: &Identity) -> Option<String> {
    let argv0 = arg_name(id.argv.first());
    let script = arg_name(id.argv.get(1));
    match id.comm.as_deref().filter(|c| !c.is_empty()) {
        Some(comm) if is_runtime(comm) => script.or_else(|| Some(comm.to_string())),
        Some(comm) => match argv0 {
            Some(a0) if a0 != comm && !is_runtime(&a0) => Some(a0),
            _ => Some(comm.to_string()),
        },
        None => argv0.filter(|a| !is_runtime(a)).or(script),
    }
}

/// Reads comm and argv of a live process.
pub fn identity(pid: u32) -> Identity {
    Identity {
        comm: process_name(pid),
        argv: argv(pid).unwrap_or_default(),
    }
}

#[cfg(target_os = "macos")]
fn process_name(pid: u32) -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: buffer is valid for the given length; proc_name writes at most that many bytes.
    let n = unsafe {
        libc::proc_name(
            pid as libc::c_int,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
        )
    };
    if n <= 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

#[cfg(target_os = "linux")]
fn process_name(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{}/comm", pid))
        .ok()
        .map(|s| s.trim_end().to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_name(_pid: u32) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn argv(pid: u32) -> Option<Vec<String>> {
    let bytes = std::fs::read(format!("/proc/{}/cmdline", pid)).ok()?;
    Some(parse_cmdline(&bytes))
}

#[cfg(target_os = "macos")]
fn argv(pid: u32) -> Option<Vec<String>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size: libc::size_t = 0;
    // SAFETY: first call only queries the required size; no output buffer is passed.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: buf has `size` writable bytes; the kernel updates `size` to the bytes written.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(size);
    parse_procargs2(&buf)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn argv(_pid: u32) -> Option<Vec<String>> {
    None
}

#[cfg(target_os = "macos")]
pub fn cwd_of(pid: u32) -> Option<String> {
    let output = std::process::Command::new("lsof")
        .args(["-p", &pid.to_string(), "-a", "-d", "cwd", "-Fn"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(path) = line.strip_prefix('n')
            && std::path::Path::new(path).is_dir()
        {
            return Some(path.to_string());
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub fn cwd_of(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{}/cwd", pid))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn cwd_of(_pid: u32) -> Option<String> {
    None
}
