//! App-level TUI tests: drive `tui::App` against in-process daemons in a temp dir.
mod copy_mode;
mod drawing;
mod fidelity;
#[cfg(unix)]
mod keys;
mod overlay_io;
mod overlay_keys;
mod passthrough;
mod prompt_cmds;
mod sessions;
pub mod support;
mod windows;
