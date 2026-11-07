//! The attached TUI: its state, the daemon traffic it applies, input handling and the event loop.

mod daemon;
mod input;
mod layout;
mod pacing;
mod run;
mod state;

pub use input::latest_drag;
pub use pacing::{
    FRAME_BUDGET, FRAMES_PER_BUDGET, TYPING_FRAMES, TYPING_WINDOW, needs_redraw, next_frame,
};
pub use run::{run_tui, sync_mouse_capture};
pub use state::{
    App, AppOptions, AttachOpts, Attachment, Chrome, Clipboard, ClockSource, Flow, FramePacing,
    HostTerm, Interaction, Launch, Mode, StartView, StatusHits, StreamRx, Subscription, YankState,
};
