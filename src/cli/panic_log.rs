use std::io::Write;
use std::path::{Path, PathBuf};

/// File in the state directory that panic reports are appended to.
pub const PANIC_LOG: &str = "panic.log";

/// Appends every panic (time, thread, location, message, backtrace) to `dir/panic.log`, then runs
/// the previous hook. The TUI's stderr is hidden behind the alternate screen, so without this a
/// crash leaves nothing behind.
pub fn install_panic_log(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let thread = std::thread::current();
        let report = format!(
            "[unix {secs}] thread '{}' {info}\n{}\n\n",
            thread.name().unwrap_or("<unnamed>"),
            std::backtrace::Backtrace::force_capture()
        );
        let _ = append(&dir, &report);
        previous(info);
    }));
}

fn append(dir: &Path, report: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(PANIC_LOG))?
        .write_all(report.as_bytes())
}
