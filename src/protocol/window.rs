use crate::layout::LinkPlace;
use serde::{Deserialize, Serialize};

use super::default_true;
use super::node::SpawnOpts;
use super::pane::Side;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWindowOpts {
    /// Window label; also used to derive the node name when `spawn.name` is empty.
    pub name: Option<String>,
    pub spawn: SpawnOpts,
    #[serde(default = "default_true")]
    pub focus: bool,
}

/// `link-window`: shows window `src` (the active window's index when `None`) at index `dst` as well.
/// Without `dst` it takes the lowest free index; `place` says where it goes and what it displaces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkOpts {
    pub src: Option<usize>,
    pub dst: Option<usize>,
    #[serde(default)]
    pub place: LinkPlace,
}

/// `unlink-window`: removes the index `index` (the active one when `None`) of its window. Refused
/// for a window with no other index unless `kill`, which kills it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnlinkOpts {
    pub index: Option<usize>,
    #[serde(default)]
    pub kill: bool,
}

/// `resize-window`: sizes window `index` (the active one when `None`) by hand. The size starts from
/// the window's current size, as `WindowResize` describes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResizeOpts {
    pub index: Option<usize>,
    #[serde(default)]
    pub resize: WindowResize,
}

/// Which attached client's size `resize-window -A` or `-a` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowFit {
    /// The largest attached client (`-A`).
    Largest,
    /// The smallest attached client (`-a`).
    Smallest,
}

/// What `resize-window` changes, as the client gives it: the size it sets, the border it moves and
/// by how many cells, and the fit it takes. The daemon starts from the window's current size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowResize {
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub side: Option<Side>,
    pub cells: Option<u16>,
    pub fit: Option<WindowFit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum WindowRequest {
    New(NewWindowOpts),
    Next,
    Prev,
    Last,
    Select {
        index: usize,
    },
    /// `index: None` targets the active window.
    Rename {
        index: Option<usize>,
        name: String,
    },
    Kill {
        index: Option<usize>,
    },
    Move {
        from: usize,
        to: usize,
    },
    /// Renumbers the windows 0, 1, ... in order whatever `renumber-windows` says (tmux `move-window -r`).
    Renumber,
    /// Rotates the panes of the active window through its layout slots (tmux `rotate-window`).
    /// The default moves every pane to the slot before it (the first pane goes last); `down`
    /// moves every pane to the slot after it. Pane names move, slots and focus stay. `keep_zoom`
    /// (tmux `-Z`) keeps a zoomed window zoomed.
    Rotate {
        down: bool,
        #[serde(default)]
        keep_zoom: bool,
    },
    /// Exchanges the numbers of two windows (tmux `swap-window`).
    Swap {
        a: usize,
        b: usize,
    },
    /// Restarts every pane of window `index` (the active window when `None`), as tmux
    /// `respawn-window`. Fails while a pane still runs unless `kill` is set.
    Respawn {
        index: Option<usize>,
        #[serde(default)]
        kill: bool,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    },
    /// Shows a window at another index as well (tmux `link-window`).
    LinkWindow(LinkOpts),
    /// Removes one index of a window (tmux `unlink-window`).
    UnlinkWindow(UnlinkOpts),
    /// Sizes a window by hand (tmux `resize-window`).
    ResizeWindow(ResizeOpts),
}
