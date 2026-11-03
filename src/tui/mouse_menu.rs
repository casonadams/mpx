//! The menus a right click opens, as tmux 3.4's default `MouseDown3*` bindings do: the pane, a
//! window tab, the session (status-left and status-right) and a split border. Each menu is a
//! `display-menu` command line, so it runs through the same parser and overlay as any other menu.

use std::borrow::Cow;

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
type Item = (&'static str, &'static str, &'static str);

/// The entry that draws a blank line between groups of items.
const SEPARATOR: Item = ("", "", "");

const WINDOW_ITEMS: &[Item] = &[
    (
        "Rename",
        "n",
        r#"command-prompt -F -I #W \"rename-window -- '%%'\""#,
    ),
    SEPARATOR,
    ("Kill", "X", "kill-window"),
    ("Respawn", "R", "respawn-window -k"),
    SEPARATOR,
    ("New Window", "c", "new-window"),
];

const SESSION_ITEMS: &[Item] = &[
    ("Next Session", "n", "next-session"),
    ("Previous Session", "p", "prev-session"),
    SEPARATOR,
    ("New Window", "c", "new-window"),
    ("Choose Tree", "w", "choose-tree"),
    SEPARATOR,
    ("Detach", "d", "detach-client"),
];

const BORDER_ITEMS: &[Item] = &[
    ("Even Horizontal", "1", "select-layout even-horizontal"),
    ("Even Vertical", "2", "select-layout even-vertical"),
    ("Main Horizontal", "3", "select-layout main-horizontal"),
    ("Main Vertical", "4", "select-layout main-vertical"),
    ("Tiled", "5", "select-layout tiled"),
    SEPARATOR,
    ("Spread Out", "E", "select-layout -E"),
];

/// Text placed inside a quoted word of a command line.
fn quoted(text: &str) -> String {
    text.replace(['"', '\\'], "'")
}

impl MouseMenu {
    /// The `display-menu` command line that opens this menu with its top-left corner at `col`,
    /// `row`.
    pub fn command_line(&self, col: u16, row: u16) -> String {
        let (title, items): (String, Cow<[Item]>) = match self {
            Self::Pane {
                input_off,
                marked,
                zoomed,
            } => (
                String::new(),
                Cow::Owned(pane_items(*input_off, *marked, *zoomed)),
            ),
            Self::Window { label } => (label.replace('#', "##"), Cow::Borrowed(WINDOW_ITEMS)),
            Self::Session { name } => (name.replace('#', "##"), Cow::Borrowed(SESSION_ITEMS)),
            Self::Border => ("Layout".to_string(), Cow::Borrowed(BORDER_ITEMS)),
        };
        let mut line = format!("display-menu -x {col} -y {row} -T \"{}\"", quoted(&title));
        for &(label, key, command) in items.iter() {
            line.push_str(&format!(" \"{}\" \"{key}\" \"{command}\"", quoted(label)));
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
        ("Horizontal Split", "h", "split-window -h"),
        ("Vertical Split", "v", "split-window"),
        SEPARATOR,
        ("Swap Up", "u", "swap-pane -U"),
        ("Swap Down", "d", "swap-pane -D"),
        SEPARATOR,
        ("Kill", "X", "kill-pane"),
        ("Respawn", "R", "respawn-pane -k"),
        SEPARATOR,
        (mark, "m", "select-pane -m"),
        (zoom, "z", "resize-pane -Z"),
        (input, "i", input_cmd),
        ("Break Out", "!", "break-pane"),
    ]
}
