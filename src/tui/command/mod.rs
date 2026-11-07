//! Typed TUI commands. Key bindings, the `:` prompt and overlays all produce `Command`s.
//! Targets use `[session:]pane` where pane is a node name or `%id`.
//!
//! A line is split into words, aliased, and handed to the parser of its command; each domain
//! has its own submodule of parsers.
mod buffer;
mod client;
mod display;
mod keys;
mod model;
mod opts;
mod pane;
mod popup;
mod script;
mod session;
mod window;
mod words;

pub use model::{
    BufferCommand, ClientCommand, Command, CopyModeOpts, EnvCommand, JoinOpts, PaletteTab,
    PaneCommand, PopupCommand, PromptSpec, ScriptCommand, SessionCommand, WindowCommand,
};
pub use window::window_target;
pub use words::split_words;

use std::collections::BTreeMap;
use words::expand_alias;

/// Command names and aliases accepted by `parse`; used for `:` completion.
pub const COMMAND_NAMES: &[&str] = &[
    "bind-key",
    "break-pane",
    "capture-pane",
    "choose-buffer",
    "choose-client",
    "choose-tree",
    "clock-mode",
    "command-palette",
    "command-prompt",
    "confirm-before",
    "copy-mode",
    "delete-buffer",
    "detach",
    "detach-client",
    "display-message",
    "display-panes",
    "display-menu",
    "display-popup",
    "find-window",
    "help",
    "if-shell",
    "join-pane",
    "kill-pane",
    "kill-server",
    "kill-window",
    "last-pane",
    "last-window",
    "link-window",
    "list-buffers",
    "list-clients",
    "list-keys",
    "load-buffer",
    "mark-pane",
    "move-pane",
    "move-window",
    "new",
    "new-window",
    "next-layout",
    "next-pane",
    "next-session",
    "next-window",
    "palette",
    "paste",
    "paste-buffer",
    "prev-session",
    "prev-window",
    "previous-window",
    "q",
    "qa",
    "quit",
    "reload-config",
    "refresh-client",
    "rename",
    "rename-session",
    "rename-window",
    "resize-pane",
    "resize-window",
    "respawn-pane",
    "respawn-window",
    "rotate-window",
    "run-shell",
    "save-buffer",
    "select-layout",
    "select-pane",
    "select-window",
    "send-keys",
    "send-prefix",
    "session",
    "set-buffer",
    "set-environment",
    "set-hook",
    "set-option",
    "set-window-option",
    "show-buffer",
    "show-environment",
    "show-hooks",
    "show-messages",
    "show-options",
    "show-window-options",
    "source-file",
    "sp",
    "split",
    "split-window",
    "suspend-client",
    "swap-pane",
    "swap-window",
    "switch-client",
    "tabn",
    "tabnew",
    "tabnext",
    "tabp",
    "tabprevious",
    "unbind-key",
    "unlink-window",
    "vs",
    "vsplit",
    "wait-for",
];

/// Parses a command line (tmux command names plus vim aliases such as `vsplit`, `q`), without
/// command aliases, and with no session known: a `session:N` window target is refused.
pub fn parse(line: &str) -> Result<Command, String> {
    parse_in(line, &BTreeMap::new())
}

/// Parses a command line with the command `aliases` of the config (tmux `command-alias`): a
/// command whose name is an alias runs the alias's command line, with its own arguments after it.
pub fn parse_in(line: &str, aliases: &BTreeMap<String, String>) -> Result<Command, String> {
    parse_scoped_in(
        line,
        Scope {
            aliases,
            session: None,
        },
    )
}

/// `parse_in` for a line run in the session `session`: its own name is an accepted window target
/// (`link-window -s session:N`), and other session names are refused.
pub fn parse_scoped(
    line: &str,
    aliases: &BTreeMap<String, String>,
    session: &str,
) -> Result<Command, String> {
    parse_scoped_in(
        line,
        Scope {
            aliases,
            session: Some(session),
        },
    )
}

/// What a command line is parsed against: the config's command aliases, and the session the line
/// runs in when it is known.
#[derive(Clone, Copy)]
struct Scope<'a> {
    aliases: &'a BTreeMap<String, String>,
    session: Option<&'a str>,
}

fn parse_scoped_in(line: &str, scope: Scope) -> Result<Command, String> {
    if line.contains(';') {
        let mut commands = Vec::new();
        for words in crate::cmdline::split_commands(line)? {
            commands.push(parse_words(&expand_alias(words, scope.aliases)?, scope)?);
        }
        return match commands.len() {
            0 => Err("empty command".to_string()),
            1 => Ok(commands.remove(0)),
            _ => Ok(Command::Sequence(commands)),
        };
    }
    parse_words(&expand_alias(split_words(line)?, scope.aliases)?, scope)
}

fn parse_words(words: &[String], scope: Scope) -> Result<Command, String> {
    let (name, args) = words
        .split_first()
        .ok_or_else(|| "empty command".to_string())?;
    dispatch(name, args, scope)
}

fn dispatch(name: &str, args: &[String], scope: Scope) -> Result<Command, String> {
    match name {
        "split-window" | "split" | "sp" => pane::split(name, args, false),
        "vsplit" | "vs" => pane::split(name, args, true),
        "new-window" | "new" | "tabnew" => window::new_window(name, args),
        "kill-window" => window::kill_window(name, args),
        "respawn-window" | "respawnw" => window::respawn_window(name, args),
        "link-window" | "linkw" => window::link_window(name, args, scope),
        "unlink-window" | "unlinkw" => window::unlink_window(name, args, scope),
        "resize-window" | "resizew" => window::resize_window(name, args, scope),
        "rename-window" => window::rename_window(args),
        "select-window" => window::select_window(name, args),
        "next-window" if args == ["-a"] => Ok(step_window(true, true)),
        "previous-window" if args == ["-a"] => Ok(step_window(false, true)),
        "next-window" | "tabnext" | "tabn" => bare(name, args, step_window(true, false)),
        "previous-window" | "prev-window" | "tabprevious" | "tabp" => {
            bare(name, args, step_window(false, false))
        }
        "last-window" => bare(name, args, Command::Window(WindowCommand::LastWindow)),
        "rotate-window" | "rotatew" => window::rotate_window(name, args),
        "move-window" | "movew" => window::move_window(name, args),
        "swap-window" | "swapw" => window::swap_window(name, args),
        "find-window" | "findw" => window::find_window(name, args),

        "kill-pane" => pane::kill_pane(name, args),
        "mark-pane" => pane::mark_pane(name, args),
        "select-pane" => pane::select_pane(name, args),
        "resize-pane" => pane::resize_pane(name, args),
        "swap-pane" => pane::swap_pane(name, args),
        "join-pane" | "joinp" | "move-pane" | "movep" => pane::join_pane(name, args),
        "respawn-pane" | "respawnp" => pane::respawn_pane(name, args),
        "break-pane" => bare(name, args, Command::Pane(PaneCommand::BreakPane)),
        "last-pane" => pane::last_pane(name, args),
        "next-pane" => bare(name, args, Command::Pane(PaneCommand::NextPane)),
        "select-layout" => pane::select_layout(name, args),
        "next-layout" => bare(
            name,
            args,
            Command::Pane(PaneCommand::SelectLayout { preset: None }),
        ),

        "send-keys" => keys::send_keys(name, args),
        "paste" => keys::paste(name, args),
        "copy-mode" => keys::copy_mode(name, args),
        "bind-key" | "bind" => keys::bind_key(name, args),
        "unbind-key" | "unbind" => keys::unbind_key(name, args),

        "detach" | "q" | "quit" => bare(name, args, Command::Detach),
        "detach-client" => client::detach_client(name, args),
        "list-clients" | "lsc" => client::list_clients(name, args),
        "refresh-client" | "refresh" => client::refresh_client(name, args),
        "choose-client" => bare(name, args, Command::Client(ClientCommand::ChooseClient)),
        "suspend-client" | "suspendc" => bare(name, args, Command::Client(ClientCommand::Suspend)),

        "display-message" | "display" => display::display_message(name, args),
        "display-panes" | "displayp" => display::display_panes(name, args),
        "clock-mode" => display::clock_mode(name, args),
        "choose-tree" => display::choose_tree(name, args),
        "choose-buffer" => bare(name, args, Command::ChooseBuffer),
        "show-messages" | "showmsgs" => bare(name, args, Command::ShowMessages),
        "palette" | "command-palette" => display::palette(name, args),
        "command-prompt" => display::command_prompt(name, args),
        "confirm-before" | "confirm" => display::confirm_before(name, args, scope),
        "display-menu" | "menu" => popup::display_menu(name, args),
        "display-popup" | "popup" => popup::display_popup(name, args),

        "list-buffers" | "lsb" => buffer::list_buffers(name, args),
        "set-buffer" | "setb" => buffer::set_buffer(name, args),
        "show-buffer" | "showb" => {
            buffer::named_buffer(name, args, |name| BufferCommand::ShowBuffer { name })
        }
        "delete-buffer" | "deleteb" => {
            buffer::named_buffer(name, args, |name| BufferCommand::DeleteBuffer { name })
        }
        "load-buffer" | "loadb" => buffer::load_buffer(name, args),
        "save-buffer" | "saveb" => buffer::save_buffer(name, args),
        "paste-buffer" | "pasteb" => buffer::paste_buffer(name, args),
        "capture-pane" | "capturep" => buffer::capture_pane(name, args),
        "set-environment" | "setenv" => buffer::set_environment(name, args),
        "show-environment" | "showenv" => buffer::show_environment(name, args),

        "switch-client" | "session" => session::switch_client(name, args),
        "rename-session" | "rename" => session::rename_session(name, args),
        "next-session" => bare(name, args, Command::Session(SessionCommand::NextSession)),
        "prev-session" => bare(name, args, Command::Session(SessionCommand::PrevSession)),
        "set-option" | "set" | "set-window-option" | "setw" => session::set_option(name, args),
        "show-options" | "show" | "show-window-options" | "showw" => {
            session::show_options(name, args)
        }

        "run-shell" => script::run_shell(name, args).map(Command::Script),
        "if-shell" => script::if_shell(name, args, scope).map(Command::Script),
        "wait-for" => script::wait_for(name, args).map(Command::Script),
        "set-hook" => script::set_hook(name, args, scope).map(Command::Script),
        "show-hooks" => bare(name, args, Command::Script(ScriptCommand::ShowHooks)),
        "source-file" if args.is_empty() => Ok(Command::ReloadConfig),
        "source-file" => script::source_file(name, args).map(Command::Script),
        "reload-config" => bare(name, args, Command::ReloadConfig),

        "kill-server" | "qa" => bare(
            name,
            args,
            Command::Confirm {
                prompt: "kill-server? (y/n)".to_string(),
                then: Box::new(Command::KillServer),
            },
        ),
        "list-keys" | "help" => bare(name, args, Command::OpenHelp),
        "send-prefix" => bare(name, args, Command::SendPrefix),

        _ => Err(format!("unknown command: {name}")),
    }
}

/// `next-window` / `previous-window` with or without `-a`, which stops only at alerts.
fn step_window(forward: bool, alert: bool) -> Command {
    Command::Window(WindowCommand::StepWindow { forward, alert })
}

/// A command that takes no arguments.
fn bare(name: &str, args: &[String], cmd: Command) -> Result<Command, String> {
    if args.is_empty() {
        Ok(cmd)
    } else {
        Err(format!("{name}: takes no arguments"))
    }
}

/// Whether a client attached read-only (`-r`) may run `cmd`: viewing, navigating, copying and
/// leaving. Anything that sends keys to a pane or changes windows, panes, layout, options or other
/// clients is refused.
pub fn read_only_allowed(cmd: &Command) -> bool {
    match cmd {
        Command::Sequence(cmds) => cmds.iter().all(read_only_allowed),
        Command::Confirm { then, .. } => read_only_allowed(then),
        _ => matches!(
            cmd,
            Command::Detach
                | Command::CopyMode(_)
                | Command::CopyCommand { .. }
                | Command::OpenTree
                | Command::OpenHelp
                | Command::OpenPalette { .. }
                | Command::CommandPrompt(_)
                | Command::ShowMessages
                | Command::ShowOptions { .. }
                | Command::ChooseBuffer
                | Command::DisplayMessage { .. }
                | Command::DisplayPanes { .. }
                | Command::ClockMode { .. }
                | Command::Buffer(BufferCommand::ListBuffers { .. })
                | Command::Client(
                    ClientCommand::ListClients { .. }
                        | ClientCommand::ChooseClient
                        | ClientCommand::RefreshClient { .. }
                        | ClientCommand::Suspend
                )
                | Command::Window(
                    WindowCommand::StepWindow { .. }
                        | WindowCommand::LastWindow
                        | WindowCommand::SelectWindow { .. }
                        | WindowCommand::SelectWindowNamed { .. }
                )
                | Command::Pane(
                    PaneCommand::NextPane
                        | PaneCommand::LastPane { .. }
                        | PaneCommand::SelectPane { .. }
                        | PaneCommand::SelectPaneDir { .. }
                )
                | Command::Session(
                    SessionCommand::SwitchSession { .. }
                        | SessionCommand::NextSession
                        | SessionCommand::PrevSession
                        | SessionCommand::LastSession
                )
        ),
    }
}
