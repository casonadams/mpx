mod layout {
    mod alerts;
    mod tree;
    mod workspace;

    use mpx::layout::Workspace;
    use mpx::layout::tree::Direction;

    /// A `main` tab with the panes `names` split side by side.
    fn tab_with(names: &[&str]) -> Workspace {
        let mut ws = Workspace::new("main".to_string());
        for name in names {
            ws.add_or_split(name.to_string(), Direction::Horizontal);
        }
        ws
    }

    /// A `main` tab holding `pane1`, and a second window `second` holding `pane2`; window 0 is active.
    fn two_windows() -> Workspace {
        let mut ws = tab_with(&["pane1"]);
        ws.new_window("second".to_string(), "pane2".to_string());
        ws.select_window(0);
        ws
    }
}
