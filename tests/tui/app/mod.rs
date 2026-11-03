//! App-level TUI tests: drive `tui::App` against in-process daemons in a temp dir.
mod binds;
mod clients;
mod copy_mode;
mod copy_commands;
mod drawing;
mod fidelity;
#[cfg(unix)]
mod keys;
mod moves;
mod overlay_io;
mod options;
mod overlay_keys;
mod passthrough;
mod prompt_cmds;
mod reload;
mod sessions;
mod statusline;
pub mod support;
mod views;
mod windows;
