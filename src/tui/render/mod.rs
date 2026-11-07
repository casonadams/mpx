mod clipboard;
mod colors;
mod cursor;
mod divider;
mod hud;
mod pane;
mod selection;
mod status_bar;

pub use super::color::{
    Palette, color_to_rgb, dim_color, from_terminal_palette, inactive_color, parse_color,
};
pub use clipboard::{copy_to_clipboard, read_clipboard};
pub use colors::{HostColors, PaneColors, vt_to_crossterm};
pub use divider::DividerStyle;
pub use hud::hud_rect;
pub use pane::{RenderPaneContext, apply_active_style};
pub use selection::SelectionRange;
pub use status_bar::{
    StatusBar, draw_status_segments, format_tab, get_current_time, status_left, status_right,
    tab_text,
};

pub struct Compositor;
