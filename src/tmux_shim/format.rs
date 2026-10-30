use std::collections::BTreeMap;

/// Format variables available to `#{name}` references.
pub type Vars = BTreeMap<&'static str, String>;

/// Everything the shim needs to describe one pane.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneView {
    pub session: String,
    pub id: u32,
    pub index: usize,
    pub pid: Option<u32>,
    pub title: String,
    pub command: String,
    pub path: String,
    pub width: u16,
    pub height: u16,
    pub active: bool,
    pub dead: bool,
    pub marked: bool,
    pub marked_set: bool,
    pub window: usize,
    pub window_name: String,
}

/// Expands `#{name}` and single-letter `#S`, `#I`, `#P`, `#W`, `#T`, `#D`, `##` references.
pub fn expand(fmt: &str, vars: &Vars) -> String {
    let mut out = String::with_capacity(fmt.len());
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' {
            match chars.peek() {
                Some('{') => {
                    chars.next();
                    let mut name = String::new();
                    let mut closed = false;
                    for nc in chars.by_ref() {
                        if nc == '}' {
                            closed = true;
                            break;
                        }
                        name.push(nc);
                    }
                    if closed {
                        if let Some(val) = vars.get(name.as_str()) {
                            out.push_str(val);
                        }
                    } else {
                        out.push_str("#{");
                        out.push_str(&name);
                    }
                }
                Some('#') => {
                    chars.next();
                    out.push('#');
                }
                Some('S') => {
                    chars.next();
                    if let Some(val) = vars.get("session_name") {
                        out.push_str(val);
                    }
                }
                Some('I') => {
                    chars.next();
                    if let Some(val) = vars.get("window_index") {
                        out.push_str(val);
                    }
                }
                Some('P') => {
                    chars.next();
                    if let Some(val) = vars.get("pane_index") {
                        out.push_str(val);
                    }
                }
                Some('W') => {
                    chars.next();
                    if let Some(val) = vars.get("window_name") {
                        out.push_str(val);
                    }
                }
                Some('T') => {
                    chars.next();
                    if let Some(val) = vars.get("pane_title") {
                        out.push_str(val);
                    }
                }
                Some('D') => {
                    chars.next();
                    if let Some(val) = vars.get("pane_id") {
                        out.push_str(val);
                    }
                }
                _ => {
                    out.push('#');
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn flag(on: bool) -> String {
    if on { "1" } else { "0" }.to_string()
}

pub fn pane_vars(p: &PaneView) -> Vars {
    let mut vars = Vars::new();
    vars.insert("pane_id", format!("%{}", p.id));
    vars.insert("pane_index", p.index.to_string());
    vars.insert("pane_pid", p.pid.map(|x| x.to_string()).unwrap_or_default());
    vars.insert("pane_title", p.title.clone());
    vars.insert("pane_current_command", p.command.clone());
    vars.insert("pane_current_path", p.path.clone());
    vars.insert("pane_width", p.width.to_string());
    vars.insert("pane_height", p.height.to_string());
    vars.insert("pane_active", flag(p.active));
    vars.insert("pane_dead", flag(p.dead));
    vars.insert("pane_marked", flag(p.marked));
    vars.insert("pane_marked_set", flag(p.marked_set));
    vars.insert("session_name", p.session.clone());
    vars.insert("window_index", p.window.to_string());
    vars.insert("window_id", format!("@{}", p.window));
    vars.insert("window_name", p.window_name.clone());
    vars
}

/// Variables describing a session (for `list-sessions`).
pub fn session_vars(name: &str, windows: usize, panes: usize) -> Vars {
    let mut vars = Vars::new();
    vars.insert("session_name", name.to_string());
    vars.insert("session_windows", windows.to_string());
    vars.insert("session_panes", panes.to_string());
    vars
}
