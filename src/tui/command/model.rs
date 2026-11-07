//! The typed commands: what a parsed command line becomes.
use crate::protocol::{
    ChannelOp, EnvChange, LayoutPreset, LinkOpts, PasteBufferOpts, ResizeOpts, Side, UnlinkOpts,
};
use crate::tui::copy::CopyAction;
use crate::tui::keymap::Table;
use crate::tui::menu::MenuItem;
use crate::tui::placement::Offset;
use crate::tui::popup::PopupOpts;
use crate::tui::prompt::PromptKind;
use crate::vt::capture::CaptureSpec;

/// The options of `copy-mode`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyModeOpts {
    /// `-t`: the pane; `None` is the active pane.
    pub target: Option<String>,
    /// `-u`: also one page up into the history.
    pub page_up: bool,
    /// `-e`: leave copy mode once scrolling brings the view back to the live screen.
    pub exit_at_bottom: bool,
    /// `-q`: leave copy mode instead of entering it.
    pub quit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteTab {
    #[default]
    Commands,
    Sessions,
    Help,
}

/// What a `command-prompt` asks and how its answers are used.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromptSpec {
    /// Prompt texts (`-p`), one question each; commas separate them.
    pub prompts: Vec<String>,
    /// Initial text per question (`-I`).
    pub initial: Vec<String>,
    /// Command template with `%%` / `%1`..`%9` placeholders; `None` runs the typed line.
    pub template: Option<String>,
    /// What Tab completes (`-T`).
    pub kind: PromptKind,
    /// The first key typed answers (`-1`, `-k`), with no Enter.
    pub single_key: bool,
    /// Digits only (`-N`).
    pub numeric: bool,
    /// The prompts and initial texts are formats, expanded for the active pane (`-F`).
    pub expand: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// The window commands: see `WindowCommand`.
    Window(WindowCommand),
    /// The pane commands: see `PaneCommand`.
    Pane(PaneCommand),
    /// The session commands: see `SessionCommand`.
    Session(SessionCommand),
    /// `set-option [-gwsqu] name [value]`: sets a TUI option for this client; `-u` restores the
    /// default, `-q` ignores an unknown option.
    SetOption {
        name: String,
        value: Option<String>,
        unset: bool,
        quiet: bool,
    },
    /// `show-options [-gwsqv] [name]`: the options and their values in an overlay; with a name and
    /// `-v`, only the value on the status row.
    ShowOptions {
        name: Option<String>,
        value_only: bool,
        quiet: bool,
    },
    /// `display-message [-p] [format]`: the expanded format on the status row; with no format it
    /// describes the current pane.
    DisplayMessage {
        format: Option<String>,
    },
    SendKeys {
        target: Option<String>,
        keys: Vec<String>,
    },
    Paste {
        target: Option<String>,
        text: String,
        submit: bool,
    },
    Zoom,
    /// `set-option [-g] synchronize-panes on|off`: typed input goes to every pane of the active
    /// window (every window with `-g`).
    SetSynchronize {
        all: bool,
        on: bool,
    },
    /// `copy-mode [-eHMqu] [-t pane]`: enters copy mode, or with `quit` leaves it.
    CopyMode(CopyModeOpts),
    /// `send-keys -X command [args]`: runs one copy-mode command in the copy mode of `target`
    /// (the active pane when `None`), `count` times.
    CopyCommand {
        target: Option<String>,
        action: CopyAction,
        count: usize,
    },
    Detach,
    KillServer,
    /// Send the prefix key itself to the active pane.
    SendPrefix,
    /// Re-read the config file into the TUI and the session's daemon.
    ReloadConfig,
    OpenTree,
    OpenHelp,
    OpenPalette {
        tab: Option<PaletteTab>,
    },
    /// `command-prompt`: one question per `prompts` entry (`-p`), each starting with its `initial`
    /// text (`-I`). With a `template` the answers fill it in (`%%`, `%1`..`%9`); without one the
    /// typed line runs as a command.
    CommandPrompt(PromptSpec),
    Confirm {
        prompt: String,
        then: Box<Command>,
    },
    /// Commands run one after the other (`a ; b`), stopping at the first that fails or quits.
    Sequence(Vec<Command>),
    /// `bind-key`: bind `key` in `table`; an empty `line` is `unbind-key`.
    Bind {
        table: Table,
        key: String,
        line: String,
    },
    /// `display-panes [-d ms]`: number each visible pane until a digit picks one, a key is
    /// pressed or the time runs out. `None` takes `display-panes-time`; `Some(0)` waits for a key.
    DisplayPanes {
        duration_ms: Option<u64>,
    },
    /// `clock-mode [-t pane]`: a large clock fills the pane until a key is pressed.
    ClockMode {
        target: Option<String>,
    },
    /// `show-messages`: the status-row messages of this session, oldest first.
    ShowMessages,
    /// `choose-buffer`: the paste buffers, newest first; Enter pastes, `d` deletes.
    ChooseBuffer,
    /// Paste buffers, capture and the buffer files: see `BufferCommand`.
    Buffer(BufferCommand),
    /// `set-environment` and `show-environment`: see `EnvCommand`.
    Environment(EnvCommand),
    /// The client commands: listing, detaching, refreshing and choosing attached terminals, and
    /// suspending this one. See `ClientCommand`.
    Client(ClientCommand),
    /// The popup and menu commands: see `PopupCommand`.
    Popup(PopupCommand),
    /// The script commands: `run-shell`, `if-shell`, `wait-for`, hooks, `command-alias` and
    /// `source-file`.
    Script(ScriptCommand),
}

/// The window commands: the active window, or the one at the index, name or offset they carry.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowCommand {
    NewWindow {
        name: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
    },
    KillWindow {
        index: Option<usize>,
    },
    RenameWindow {
        name: String,
    },
    SelectWindow {
        index: usize,
    },
    /// `select-window -t name`: the window with that name.
    SelectWindowNamed {
        name: String,
    },
    /// `rotate-window [-DZ]`: the active window's panes move through its layout slots; `-Z` keeps
    /// the window zoomed.
    RotateWindow {
        down: bool,
        keep_zoom: bool,
    },
    /// `respawn-window [-k] [-c dir] [-t index] [command...]`: restarts every pane of the window.
    RespawnWindow {
        target: Option<usize>,
        kill: bool,
        cwd: Option<String>,
        command: Vec<String>,
    },
    /// `move-window -t index`: moves the active window to that index (`-r` renumbers instead).
    MoveWindow {
        to: usize,
    },
    /// `move-window -r`: renumbers the windows 0, 1, ... in order.
    RenumberWindows,
    /// `swap-window [-s src] -t dst`: exchanges two windows; `src` defaults to the active one.
    SwapWindow {
        src: Option<usize>,
        dst: usize,
    },
    /// `link-window`: shows a window at another index as well.
    LinkWindow(LinkOpts),
    /// `unlink-window`: removes one index of a window.
    UnlinkWindow(UnlinkOpts),
    /// `resize-window`: gives a window a size of its own.
    ResizeWindow(ResizeOpts),
    /// `find-window [-iNT] pattern`: the sessions and windows palette filtered by `pattern`.
    FindWindow {
        pattern: String,
    },
    /// `next-window` / `previous-window`; `alert` (`-a`) stops only at activity or bells.
    StepWindow {
        forward: bool,
        alert: bool,
    },
    LastWindow,
}

/// `join-pane [-dh] [-s src] [-t dst]`: moves `src` (default: the marked pane) next to `dst`
/// (default: the active pane), side by side with `-h`, else stacked. `-d` keeps the active pane.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinOpts {
    pub src: Option<String>,
    pub dst: Option<String>,
    pub side_by_side: bool,
    pub detach: bool,
}

/// The pane commands. They act on the active pane, or on the pane a `target` names.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneCommand {
    /// `side_by_side`: vertical divider (`:vsplit`, tmux `split-window -h`).
    SplitWindow {
        side_by_side: bool,
        argv: Vec<String>,
        cwd: Option<String>,
        detach: bool,
        keep_zoom: bool,
    },
    KillPane {
        target: Option<String>,
    },
    SelectPane {
        target: String,
        keep_zoom: bool,
    },
    SelectPaneDir {
        side: Side,
        keep_zoom: bool,
    },
    MarkPane {
        target: Option<String>,
    },
    ClearMark,
    /// `select-pane -d` (`off`) or `-e`: stops or resumes input to a pane.
    InputOff {
        target: Option<String>,
        off: bool,
    },
    /// `join-pane [-dh] [-s src] [-t dst]`: moves a pane next to another (see `JoinOpts`).
    JoinPane(JoinOpts),
    /// `respawn-pane [-k] [-c dir] [-t target] [command...]`: restarts the pane's process.
    RespawnPane {
        target: Option<String>,
        kill: bool,
        cwd: Option<String>,
        command: Vec<String>,
    },
    NextPane,
    LastPane {
        keep_zoom: bool,
    },
    ResizePane {
        side: Side,
        cells: u16,
    },
    SwapPane {
        next: bool,
        keep_zoom: bool,
    },
    SwapPaneMarked {
        source: Option<String>,
        target: Option<String>,
        keep_zoom: bool,
    },
    BreakPane,
    SelectLayout {
        preset: Option<LayoutPreset>,
    },
    /// `select-layout -E`: spread the panes evenly, keeping their arrangement.
    SpreadLayout,
}

/// The session commands: switching between sessions and renaming the current one.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    SwitchSession {
        name: String,
    },
    NextSession,
    PrevSession,
    /// `switch-client -l`: the session this client was attached to before the current one.
    LastSession,
    RenameSession {
        name: String,
    },
}

/// Commands that run scripts, hooks, channels and aliases in the daemon, or read them from files.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptCommand {
    /// `run-shell [-b] [-t pane] [-d seconds] command`: runs `command` through the shell in the
    /// daemon, with formats expanded for the pane. Its output goes to the status row, a message per
    /// line; `-b` does not wait for it.
    RunShell {
        background: bool,
        target: Option<String>,
        delay_ms: Option<u64>,
        command: String,
    },
    /// `if-shell [-bF] [-t pane] shell-command command [command]`: runs `then` when the shell
    /// command exits 0, else `otherwise`. With `-F` the condition is a format, true when non-empty
    /// and not `0`.
    IfShell {
        background: bool,
        format: bool,
        target: Option<String>,
        condition: String,
        then: Box<Command>,
        otherwise: Option<Box<Command>>,
    },
    /// `wait-for [-L|-S|-U] channel`: waits for a signal on, or takes or releases the lock of, a
    /// named channel of the daemon.
    WaitFor { op: ChannelOp, channel: String },
    /// `set-hook [-a] hook command` sets the commands a hook runs, `-a` after the existing ones;
    /// `set-hook -u hook` removes them (`command` is `None`).
    SetHook {
        name: String,
        command: Option<String>,
        append: bool,
    },
    /// `set-hook -R hook`: runs the commands of a hook now.
    RunHook { name: String },
    /// `show-hooks`: every hook command, one line each, in an overlay.
    ShowHooks,
    /// `set -s command-alias name=command` sets an alias; `-u` removes it (`command` is `None`).
    SetAlias {
        name: String,
        command: Option<String>,
    },
    /// `source-file [-q] path...`: runs the commands of each file, one command line per line.
    SourceFile { paths: Vec<String>, quiet: bool },
}

/// The paste-buffer and capture commands. They act on the daemon's buffers, which the CLI shares.
#[derive(Debug, Clone, PartialEq)]
pub enum BufferCommand {
    /// `list-buffers [-F format]`: the paste buffers, one line each, in an overlay.
    ListBuffers { format: Option<String> },
    /// `set-buffer [-a] [-b name] [-n new] [data]`: stores or renames a paste buffer.
    SetBuffer {
        name: Option<String>,
        append: bool,
        rename: Option<String>,
        data: Option<String>,
    },
    /// `show-buffer [-b name]`: the text of a paste buffer, in an overlay.
    ShowBuffer { name: Option<String> },
    /// `delete-buffer [-b name]`.
    DeleteBuffer { name: Option<String> },
    /// `load-buffer [-b name] path`: a file becomes a paste buffer.
    LoadBuffer { name: Option<String>, path: String },
    /// `save-buffer [-a] [-b name] path`: a paste buffer is written to a file.
    SaveBuffer {
        name: Option<String>,
        append: bool,
        path: String,
    },
    /// `paste-buffer [-d] [-p] [-r] [-s sep] [-b name] [-t pane]`.
    PasteBuffer { opts: PasteBufferOpts },
    /// `capture-pane [-p] [-e] [-J] [-S line] [-E line] [-b name] [-t pane]`.
    CapturePane {
        target: Option<String>,
        print: bool,
        buffer: Option<String>,
        spec: CaptureSpec,
    },
}

/// The session environment commands.
#[derive(Debug, Clone, PartialEq)]
pub enum EnvCommand {
    /// `set-environment [-g] [-u] [-r] name [value]`: a variable of the session environment.
    SetEnvironment { name: String, change: EnvChange },
    /// `show-environment [-g] [name]`: the session environment, in an overlay.
    ShowEnvironment { name: Option<String> },
}

/// The client commands: the terminals attached to this session, and this one's suspension.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientCommand {
    /// `list-clients [-F format] [-t session]`: the attached clients, one line each, in an overlay.
    ListClients {
        target: Option<String>,
        format: Option<String>,
    },
    /// `detach-client [-a] [-t client]`: detaches `target`, or with `all_others` every other client
    /// of this session. Plain `detach-client` is `Detach`.
    DetachClients {
        target: Option<String>,
        all_others: bool,
    },
    /// `refresh-client [-S] [-C WxH] [-t client]`: redraws `target` (this client when `None`), first
    /// giving it `size` when one is set.
    RefreshClient {
        target: Option<String>,
        size: Option<(u16, u16)>,
    },
    /// `choose-client`: the attached clients; Enter detaches the highlighted one.
    ChooseClient,
    /// `suspend-client`: stop the process as Ctrl-Z does; the TUI resumes on SIGCONT.
    Suspend,
}

/// The popup and menu commands: popups run a command over the panes, menus show command lines.
#[derive(Debug, Clone, PartialEq)]
pub enum PopupCommand {
    /// `display-popup [-E] [-w W] [-h H] [-x X] [-y Y] [-d dir] [-T title] [-t pane] [command]`: the
    /// command runs on a pty drawn in a box over the panes; its keys go to it.
    Display(PopupOpts),
    /// `display-menu [-T title] [-x X] [-y Y] name key command ...`: a menu of command lines.
    Menu {
        title: Option<String>,
        x: Offset,
        y: Offset,
        items: Vec<MenuItem>,
    },
}
