//! The menus a right click opens, as tmux 3.4's default `MouseDown3*` bindings do: the pane, a
//! window tab, the session (status-left and status-right) and a split border. Each menu is a
//! `display-menu` command line, so it runs through the same parser and overlay as any other menu.

/// Which menu a right click opens, with the state its items are labelled from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseMenu {
    /// The pane under the pointer, which is the active pane once the click focused it.
    Pane {
        input_off: bool,
        marked: bool,
        zoomed: bool,
    },
    /// A window tab, labelled `number:name` as the status line shows it.
    Window { label: String },
    /// The session, from status-left or status-right.
    Session { name: String },
    /// A split border between panes.
    Border,
}

/// One menu entry: its label, the key that picks it and the command line it runs. An empty label
/// is a separator.
type Item = (String, &'static str, &'static str);

fn item(label: &str, key: &'static str, command: &'static str) -> Item {
    (label.to_string(), key, command)
}

fn separator() -> Item {
    (String::new(), "", "")
}

/// Text placed inside a quoted word of a command line.
fn quoted(text: &str) -> String {
    text.replace(['"', '\\'], "'")
}

impl MouseMenu {
    /// The `display-menu` command line that opens this menu with its top-left corner at `col`,
    /// `row`.
    pub fn command_line(&self, col: u16, row: u16) -> String {
        let (title, items) = match self {
            Self::Pane {
                input_off,
                marked,
                zoomed,
            } => (String::new(), pane_items(*input_off, *marked, *zoomed)),
            Self::Window { label } => (label.replace('#', "##"), window_items()),
            Self::Session { name } => (name.replace('#', "##"), session_items()),
            Self::Border => ("Layout".to_string(), border_items()),
        };
        let mut line = format!("display-menu -x {col} -y {row} -T \"{}\"", quoted(&title));
        for (label, key, command) in items {
            line.push_str(&format!(" \"{}\" \"{key}\" \"{command}\"", quoted(&label)));
        }
        line
    }
}

fn pane_items(input_off: bool, marked: bool, zoomed: bool) -> Vec<Item> {
    let mark = if marked { "Unmark" } else { "Mark" };
    let zoom = if zoomed { "Unzoom" } else { "Zoom" };
    let (input, input_cmd) = if input_off {
        ("Enable Input", "select-pane -e")
    } else {
        ("Disable Input", "select-pane -d")
    };
    vec![
        item("Horizontal Split", "h", "split-window -h"),
        item("Vertical Split", "v", "split-window"),
        separator(),
        item("Swap Up", "u", "swap-pane -U"),
        item("Swap Down", "d", "swap-pane -D"),
        separator(),
        item("Kill", "X", "kill-pane"),
        item("Respawn", "R", "respawn-pane -k"),
        separator(),
        item(mark, "m", "select-pane -m"),
        item(zoom, "z", "resize-pane -Z"),
        item(input, "i", input_cmd),
        item("Break Out", "!", "break-pane"),
    ]
}

fn window_items() -> Vec<Item> {
    vec![
        item(
            "Rename",
            "n",
            r#"command-prompt -F -I #W \"rename-window -- '%%'\""#,
        ),
        separator(),
        item("Kill", "X", "kill-window"),
        item("Respawn", "R", "respawn-window -k"),
        separator(),
        item("New Window", "c", "new-window"),
    ]
}

fn session_items() -> Vec<Item> {
    vec![
        item("Next Session", "n", "next-session"),
        item("Previous Session", "p", "prev-session"),
        separator(),
        item("New Window", "c", "new-window"),
        item("Choose Tree", "w", "choose-tree"),
        separator(),
        item("Detach", "d", "detach-client"),
    ]
}

fn border_items() -> Vec<Item> {
    vec![
        item("Even Horizontal", "1", "select-layout even-horizontal"),
        item("Even Vertical", "2", "select-layout even-vertical"),
        item("Main Horizontal", "3", "select-layout main-horizontal"),
        item("Main Vertical", "4", "select-layout main-vertical"),
        item("Tiled", "5", "select-layout tiled"),
        separator(),
        item("Spread Out", "E", "select-layout -E"),
    ]
}
