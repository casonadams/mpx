use std::collections::BTreeMap;

use regex::{NoExpand, RegexBuilder};

use crate::layout::{Rect, WindowTab};
use crate::protocol::NodeInfo;

/// Format variables available to `#{name}` references.
pub type Vars = BTreeMap<&'static str, String>;

/// tmux version reported by `#{version}`.
const TMUX_VERSION: &str = "3.4";

/// Recursion limit for nested formats and `#{E:...}` re-expansion.
const MAX_DEPTH: usize = 32;

/// Everything a format needs to describe one pane.
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
    /// The pane is in a mode (copy mode), (`pane_in_mode`).
    pub in_mode: bool,
    pub window: usize,
    pub window_name: String,
    /// Command the pane was started with (`#{pane_start_command}`).
    pub start_command: String,
    /// Device path of the pane's terminal, empty when unknown.
    pub tty: String,
    /// Position in the current layout, `None` when the pane is not laid out.
    pub rect: Option<Rect>,
    pub window_active: bool,
    pub window_last: bool,
    pub window_activity: bool,
    pub window_bell: bool,
    /// The window holds the marked pane.
    pub window_marked: bool,
    /// The window is zoomed (tmux `window_zoomed_flag`).
    pub window_zoomed: bool,
    /// Number of panes in this pane's window.
    pub window_panes: usize,
    /// Number of windows in the session.
    pub session_windows: usize,
    /// `base-index`: the number of the first window, added to `window` for `#{window_index}`.
    pub window_base: usize,
    /// `pane-base-index`: the number of the first pane, added to `index` for `#{pane_index}`.
    pub pane_base: usize,
}

/// Expands `#{...}` formats, the aliases `#S #I #P #W #T #D`, and the escapes `##`, `#,`, `#}`.
///
/// Unknown variables expand to empty. Malformed input (an unclosed `#{`) is left as text.
pub fn expand(fmt: &str, vars: &Vars) -> String {
    expand_text(fmt, vars, 0)
}

fn expand_text(text: &str, vars: &Vars, depth: usize) -> String {
    if depth > MAX_DEPTH {
        return String::new();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('#') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        match rest.chars().next() {
            Some('{') => match close_brace(&rest[1..]) {
                Some(end) => {
                    out.push_str(&eval(&rest[1..1 + end], vars, depth + 1));
                    rest = &rest[2 + end..];
                }
                None => {
                    out.push_str("#{");
                    rest = &rest[1..];
                }
            },
            Some(c @ ('#' | ',' | '}')) => {
                out.push(c);
                rest = &rest[1..];
            }
            Some(c) => match alias_name(c) {
                Some(name) => {
                    out.push_str(lookup(vars, name));
                    rest = &rest[1..];
                }
                None => out.push('#'),
            },
            None => out.push('#'),
        }
    }
    out.push_str(rest);
    out
}

/// Long variable name behind a single-letter alias.
fn alias_name(c: char) -> Option<&'static str> {
    Some(match c {
        'S' => "session_name",
        'I' => "window_index",
        'P' => "pane_index",
        'W' => "window_name",
        'T' => "pane_title",
        'D' => "pane_id",
        _ => return None,
    })
}

/// Byte offset of the `}` closing a `#{` whose body starts at `s[0]`, skipping nested `#{ }` and escapes.
fn close_brace(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'#' if i + 1 < b.len() => {
                if b[i + 1] == b'{' {
                    depth += 1;
                }
                i += 2;
                continue;
            }
            b'}' if depth == 0 => return Some(i),
            b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

/// Splits a format body on top-level commas, keeping nested formats and escapes intact.
fn split_args(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'#' if i + 1 < b.len() => {
                if b[i + 1] == b'{' {
                    depth += 1;
                }
                i += 2;
                continue;
            }
            b'}' if depth > 0 => depth -= 1,
            b',' if depth == 0 => {
                args.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    args.push(&s[start..]);
    args
}

/// Evaluates the body of one `#{...}` (without the braces).
fn eval(body: &str, vars: &Vars, depth: usize) -> String {
    if let Some(cond) = body.strip_prefix('?') {
        return conditional(&split_args(cond), vars, depth);
    }
    if let Some(spec) = body.strip_prefix("s/") {
        return substitute(spec, vars, depth);
    }
    let Some((op, arg)) = body.split_once(':') else {
        return lookup(vars, body).to_string();
    };
    match op {
        "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" | "!" => {
            compare(op, &split_args(arg), vars, depth)
        }
        "b" => basename(&resolve(arg, vars, depth)),
        "d" => dirname(&resolve(arg, vars, depth)),
        "n" => resolve(arg, vars, depth).chars().count().to_string(),
        "q" => shell_quote(&resolve(arg, vars, depth)),
        "E" => expand_text(&resolve(arg, vars, depth), vars, depth + 1),
        "l" => unescape(arg),
        _ => match op.strip_prefix('=').and_then(|n| n.parse::<i64>().ok()) {
            Some(n) => truncate(&resolve(arg, vars, depth), n),
            None => lookup(vars, body).to_string(),
        },
    }
}

/// `#{?c1,v1,c2,v2,...,else}`: the first value whose condition is true, else the trailing value.
fn conditional(args: &[&str], vars: &Vars, depth: usize) -> String {
    let mut rest = args;
    while let [cond, then, tail @ ..] = rest {
        if truthy(&resolve(cond, vars, depth)) {
            return expand_text(then, vars, depth);
        }
        rest = tail;
    }
    match rest {
        [otherwise] => expand_text(otherwise, vars, depth),
        _ => String::new(),
    }
}

/// Comparison and boolean operators; yields `1` or `0`. Comparisons are numeric when both sides parse as integers.
fn compare(op: &str, args: &[&str], vars: &Vars, depth: usize) -> String {
    let value = |i: usize| {
        args.get(i)
            .map_or_else(String::new, |a| expand_text(a, vars, depth))
    };
    let (a, b) = (value(0), value(1));
    let order = || match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => a.cmp(&b),
    };
    let on = match op {
        "==" => a == b,
        "!=" => a != b,
        "<" => order().is_lt(),
        ">" => order().is_gt(),
        "<=" => order().is_le(),
        ">=" => order().is_ge(),
        "&&" => truthy(&a) && truthy(&b),
        "||" => truthy(&a) || truthy(&b),
        _ => !truthy(&a),
    };
    flag(on)
}

/// `s/pat/rep/flags:var`: replaces every match of `pat` (a regex; invalid patterns match literally).
/// Flag `i` ignores case. Neither pattern nor replacement may contain `/`.
fn substitute(spec: &str, vars: &Vars, depth: usize) -> String {
    let Some((pat, rest)) = spec.split_once('/') else {
        return String::new();
    };
    let Some((rep, rest)) = rest.split_once('/') else {
        return String::new();
    };
    let Some((flags, arg)) = rest.split_once(':') else {
        return String::new();
    };
    let value = resolve(arg, vars, depth);
    let ignore_case = flags.contains('i');
    let re = RegexBuilder::new(pat)
        .case_insensitive(ignore_case)
        .build()
        .or_else(|_| {
            RegexBuilder::new(&regex::escape(pat))
                .case_insensitive(ignore_case)
                .build()
        });
    match re {
        Ok(re) => re.replace_all(&value, NoExpand(rep)).into_owned(),
        Err(_) => value,
    }
}

/// Value of a variable name, or the expansion of a nested format. Unknown names are empty.
fn resolve(s: &str, vars: &Vars, depth: usize) -> String {
    if s.contains('#') {
        expand_text(s, vars, depth)
    } else {
        lookup(vars, s).to_string()
    }
}

fn lookup<'a>(vars: &'a Vars, name: &str) -> &'a str {
    vars.get(name).map_or("", String::as_str)
}

fn truthy(s: &str) -> bool {
    !s.is_empty() && s != "0"
}

fn flag(on: bool) -> String {
    if on { "1" } else { "0" }.to_string()
}

/// `#{=N:var}` keeps the first N characters; `#{=-N:var}` the last N.
fn truncate(s: &str, n: i64) -> String {
    let len = s.chars().count();
    let keep = usize::try_from(n.unsigned_abs()).unwrap_or(usize::MAX);
    if keep >= len {
        s.to_string()
    } else if n >= 0 {
        s.chars().take(keep).collect()
    } else {
        s.chars().skip(len - keep).collect()
    }
}

fn basename(v: &str) -> String {
    let trimmed = v.trim_end_matches('/');
    if trimmed.is_empty() {
        return if v.is_empty() { "" } else { "/" }.to_string();
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

fn dirname(v: &str) -> String {
    let trimmed = v.trim_end_matches('/');
    if trimmed.is_empty() {
        return if v.is_empty() { "." } else { "/" }.to_string();
    }
    match trimmed.rfind('/') {
        None => ".".to_string(),
        Some(i) => {
            let dir = trimmed[..i].trim_end_matches('/');
            if dir.is_empty() { "/" } else { dir }.to_string()
        }
    }
}

fn shell_quote(v: &str) -> String {
    const SPECIAL: &str = "\\\"'$`!*?&;|<>()[]{}#~^ \t\n";
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        if SPECIAL.contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Literal text of `#{l:...}`: only the escapes `##`, `#,`, `#}` are processed.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('#', Some(n @ ('#' | ',' | '}'))) => {
                chars.next();
                out.push(n);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Machine host name, empty when it cannot be read.
fn host_name() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is valid for writes of `buf.len()` bytes.
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
        if !ok {
            return String::new();
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }
    #[cfg(not(unix))]
    {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    }
}

/// Variables that do not depend on the pane or session being described.
fn shared_vars(vars: &mut Vars, session: &str) {
    let host = host_name();
    let host_short = host.split('.').next().unwrap_or_default().to_string();
    vars.insert("client_session", session.to_string());
    // One attached client is assumed; attachment state is not tracked.
    vars.insert("session_attached", "1".to_string());
    vars.insert("host", host);
    vars.insert("host_short", host_short);
    vars.insert("pid", std::process::id().to_string());
    vars.insert("version", TMUX_VERSION.to_string());
}

pub fn pane_vars(p: &PaneView) -> Vars {
    let mut vars = Vars::new();
    vars.insert("pane_id", format!("%{}", p.id));
    vars.insert("pane_index", (p.index + p.pane_base).to_string());
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
    vars.insert("pane_in_mode", flag(p.in_mode));
    vars.insert(
        "pane_mode",
        if p.in_mode { "copy-mode" } else { "" }.to_string(),
    );
    vars.insert("pane_tty", p.tty.clone());
    vars.insert("pane_start_command", p.start_command.clone());
    if let Some(r) = p.rect {
        vars.insert("pane_left", r.x.to_string());
        vars.insert("pane_top", r.y.to_string());
        vars.insert(
            "pane_right",
            r.x.saturating_add(r.width).saturating_sub(1).to_string(),
        );
        vars.insert(
            "pane_bottom",
            r.y.saturating_add(r.height).saturating_sub(1).to_string(),
        );
    }
    vars.insert("session_name", p.session.clone());
    vars.insert("session_windows", p.session_windows.to_string());
    vars.insert("window_index", (p.window + p.window_base).to_string());
    vars.insert("window_id", format!("@{}", p.window));
    vars.insert("window_name", p.window_name.clone());
    vars.insert("window_active", flag(p.window_active));
    vars.insert("window_panes", p.window_panes.to_string());
    vars.insert("window_flags", window_flags(p));
    vars.insert("window_zoomed_flag", flag(p.window_zoomed));
    shared_vars(&mut vars, &p.session);
    vars
}

/// tmux's window flag order: `#` activity, `!` bell, `*` active, `Z` zoomed, `-` last, `M` marked pane.
fn window_flags(p: &PaneView) -> String {
    let mut flags = String::new();
    if p.window_activity {
        flags.push('#');
    }
    if p.window_bell {
        flags.push('!');
    }
    if p.window_active {
        flags.push('*');
    }
    if p.window_zoomed {
        flags.push('Z');
    }
    if p.window_last {
        flags.push('-');
    }
    if p.window_marked {
        flags.push('M');
    }
    flags
}

/// View of one pane from its node position, the current layout rectangles and the window tabs.
pub fn pane_view(
    session: &str,
    index: usize,
    node: &NodeInfo,
    rects: &[(String, Rect, bool)],
    tabs: &[WindowTab],
) -> PaneView {
        let window = node.window.unwrap_or(0);
        let (width, height, active, rect) = rects
            .iter()
            .find(|(n, _, _)| n == &node.name)
            .map(|(_, r, f)| (r.width, r.height, *f, Some(*r)))
            .unwrap_or((0, 0, false, None));
        let marked = node.marked.unwrap_or(false);
        let marked_set = tabs.iter().any(|t| t.has_marked) || marked;
        let tab = tabs.iter().find(|t| t.index == window);
        PaneView {
            session: session.to_string(),
            id: node.id,
            index,
            pid: node.pid,
            title: node.title.clone().unwrap_or_default(),
            command: node
                .foreground
                .clone()
                .unwrap_or_else(|| node.command.clone()),
            path: node.cwd.clone().unwrap_or_default(),
            width,
            height,
            active: tab.map_or(active, |t| t.focused.as_deref() == Some(node.name.as_str())),
            dead: !node.is_running(),
            marked,
            marked_set,
            in_mode: false,
            window,
            window_name: tab.map(|t| t.name.clone()).unwrap_or_default(),
            start_command: node.command.clone(),
            tty: node
                .tty
                .as_ref()
                .map(|t| format!("/dev/{t}"))
                .unwrap_or_default(),
            rect,
            window_active: tab.is_some_and(|t| t.is_active),
            window_last: tab.is_some_and(|t| t.is_last),
            window_activity: tab.is_some_and(|t| t.has_activity),
            window_bell: tab.is_some_and(|t| t.has_bell),
            window_marked: tab.is_some_and(|t| t.has_marked),
            window_zoomed: tab.is_some_and(|t| t.zoomed),
            session_windows: tabs.len(),
            ..Default::default()
        }
}
