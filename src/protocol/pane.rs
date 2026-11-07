use crate::layout::Direction;
use serde::{Deserialize, Serialize};

use super::default_true;
use super::node::SpawnOpts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutPreset {
    EvenHorizontal,
    EvenVertical,
    MainVertical,
    MainVerticalMirrored,
    MainHorizontal,
    MainHorizontalMirrored,
    Tiled,
}

impl LayoutPreset {
    pub const ALL: [LayoutPreset; 7] = [
        LayoutPreset::EvenHorizontal,
        LayoutPreset::EvenVertical,
        LayoutPreset::MainVertical,
        LayoutPreset::MainVerticalMirrored,
        LayoutPreset::MainHorizontal,
        LayoutPreset::MainHorizontalMirrored,
        LayoutPreset::Tiled,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LayoutPreset::EvenHorizontal => "even-horizontal",
            LayoutPreset::EvenVertical => "even-vertical",
            LayoutPreset::MainVertical => "main-vertical",
            LayoutPreset::MainVerticalMirrored => "main-vertical-mirrored",
            LayoutPreset::MainHorizontal => "main-horizontal",
            LayoutPreset::MainHorizontalMirrored => "main-horizontal-mirrored",
            LayoutPreset::Tiled => "tiled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitOpts {
    pub target: Option<String>,
    pub spawn: SpawnOpts,
    pub direction: Direction,
    /// Size of the new pane as a percentage of the split target (tmux `-l N%`).
    pub percent: Option<u8>,
    #[serde(default = "default_true")]
    pub focus: bool,
    /// Keeps the window zoomed when it was zoomed (tmux `split-window -Z`).
    #[serde(default)]
    pub keep_zoom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum PaneRequest {
    Split(SplitOpts),
    FocusNext,
    /// `keep_zoom` (tmux `-Z`) leaves a zoomed window zoomed; otherwise focusing another pane
    /// unzooms it.
    FocusLast {
        #[serde(default)]
        keep_zoom: bool,
    },
    Focus {
        name: String,
        #[serde(default)]
        keep_zoom: bool,
    },
    /// Zooms or unzooms the window of pane `pane` (the active pane when `None`), focusing that
    /// pane first (tmux `resize-pane -Z`). `on` sets the state; `None` toggles it.
    Zoom {
        pane: Option<String>,
        on: Option<bool>,
    },
    /// Move the split border of a pane by `cells` towards `side` (tmux resize-pane -L/-R/-U/-D).
    ResizeSplit {
        name: String,
        side: Side,
        cells: u16,
        width: u16,
        height: u16,
    },
    /// Exchanges two panes. `keep_zoom` (tmux `-Z`) leaves a zoomed window zoomed.
    SwapPane {
        a: Option<String>,
        b: Option<String>,
        #[serde(default)]
        keep_zoom: bool,
    },
    /// Mark or unmark a pane. If `name` is None, targets the currently active pane.
    /// If `clear` is true (-M), clears the marked pane unconditionally.
    MarkPane {
        name: Option<String>,
        #[serde(default)]
        clear: bool,
    },
    /// Turns input to a pane off (`off`, tmux `select-pane -d`) or back on (`-e`). If `name` is None,
    /// targets the currently active pane. While off, keys sent to the pane are dropped.
    InputOff {
        name: Option<String>,
        off: bool,
    },
    BreakPane {
        name: String,
    },
    /// Moves pane `src` next to `dst`, splitting `dst` side by side or stacked (tmux `join-pane`).
    Join {
        src: String,
        dst: String,
        side_by_side: bool,
        /// `detach` (tmux `-d`) keeps the pane that was active before the join active.
        #[serde(default)]
        detach: bool,
    },
    SelectLayout {
        /// `None` cycles to the next preset.
        preset: Option<LayoutPreset>,
    },
    /// Spreads the active window's panes evenly, keeping their arrangement (`select-layout -E`).
    SpreadLayout,
    /// Turns `synchronize-panes` on or off for window `window` (the active window when `None`);
    /// `on` None toggles it.
    Synchronize {
        window: Option<usize>,
        on: Option<bool>,
    },
}
