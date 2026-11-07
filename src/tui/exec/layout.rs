//! Window layout: presets, rotation, spreading, renumbering, zoom and synchronized input.
use crate::protocol::LayoutPreset;
use crate::tui::app::{App, Flow};

impl App {
    /// `rotate-window`: the active window's panes move one slot along its layout.
    pub(super) async fn rotate_window(&mut self, down: bool, keep_zoom: bool) -> Flow {
        let result = self.client.rotate_window(down, keep_zoom).await;
        self.settle(result)
    }

    /// `move-window -r`: renumbers the windows 0, 1, ... in order.
    pub(super) async fn renumber_windows(&mut self) -> Flow {
        let result = self.client.renumber_windows().await;
        self.settle(result)
    }

    pub(super) async fn select_layout(&mut self, preset: Option<LayoutPreset>) -> Flow {
        let result = self.client.select_layout(preset).await;
        self.settle(result)
    }

    /// `select-layout -E`: spreads the panes evenly, keeping their arrangement.
    pub(super) async fn spread_layout(&mut self) -> Flow {
        let result = self.client.spread_layout().await;
        self.settle(result)
    }

    /// `resize-pane -Z`: the daemon zooms or unzooms the active window.
    pub(super) async fn toggle_zoom(&mut self) -> Flow {
        let result = self.client.zoom(None, None).await;
        self.settle(result)
    }

    /// `set-option [-g] synchronize-panes`: the active window, or every window with `-g`.
    pub(super) async fn set_synchronize(&mut self, all: bool, on: bool) -> Flow {
        let windows: Vec<usize> = self
            .layout
            .tabs
            .iter()
            .filter(|t| all || t.is_active)
            .map(|t| t.index)
            .collect();
        let mut result = Ok(());
        for window in windows {
            result = self.client.synchronize(Some(window), Some(on)).await;
            if result.is_err() {
                break;
            }
        }
        self.settle(result)
    }
}
