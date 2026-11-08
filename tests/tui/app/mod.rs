//! App-level TUI tests: drive `tui::App` against in-process daemons in a temp dir.
mod binds;
mod clients;
mod copy_commands;
mod copy_mode;
mod drawing;
mod fidelity;
#[cfg(unix)]
mod keys;
mod mouse_menus;
mod moves;
mod options;
mod overlay_io;
mod overlay_keys;
mod passthrough;
#[cfg(unix)]
mod popups;
mod prompt_cmds;
mod reload;
mod resize;
mod sessions;
mod statusline;
pub mod support;
mod views;
mod windows;
