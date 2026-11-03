use std::collections::BTreeMap;

use regex::{NoExpand, RegexBuilder};

use crate::layout::{Rect, WindowTab};
use crate::protocol::{Buffer, ClientInfo, NodeInfo};

/// Format variables available to `#{name}` references.
pub type Vars = BTreeMap<&'static str, String>;

/// The lists the loop formats `#{W:...}`, `#{P:...}` and `#{S:...}` iterate over: one entry of
/// variables per window, pane and session.
#[derive(Debug, Clone, Default)]
pub struct Lists {
    pub windows: Vec<Vars>,
    pub panes: Vec<Vars>,
    pub sessions: Vec<Vars>,
}

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
    /// The window has been quiet for `monitor-silence` seconds (tmux `window_silence_flag`).
    pub window_silence: bool,
    /// The window's panes all receive typed input (`synchronize-panes`).
    pub synchronized: bool,
    /// The pane touches the left, top, right and bottom edge of the window (`pane_at_*`).
    pub edges: [bool; 4],
    /// Cursor column and row of the pane's screen (`cursor_x`, `cursor_y`); `None` for a pane without a terminal.
    pub cursor: Option<(u16, u16)>,
    /// Scrollback lines the pane's captures reach (`history_size`); `None` for a pane without a terminal.
    pub history_size: Option<usize>,
    /// Number of panes in this pane's window.
    pub window_panes: usize,
    /// Number of windows in the session.
    pub session_windows: usize,
    /// `base-index`: the number of the first window, added to `window` for `#{window_index}`.
    pub window_base: usize,
    /// `pane-base-index`: the number of the first pane, added to `index` for `#{pane_index}`.
    pub pane_base: usize,
    /// Clients attached to the pane's session (`#{session_attached}`).
    pub session_attached: usize,
    /// When the session's daemon started and whether it restored a snapshot (`session_id`,
    /// `session_created`, `start_time`).
    pub times: SessionTimes,
}

/// Expands `#{...}` formats, the aliases `#S #I #P #W #T #D`, and the escapes `##`, `#,`, `#}`.
///
/// Unknown variables expand to empty. Malformed input (an unclosed `#{`) is left as text.
pub fn expand(fmt: &str, vars: &Vars) -> String {
    expand_with(fmt, vars, &Lists::default())
}

/// [`expand`] with the lists the loop formats `#{W:...}`, `#{P:...}` and `#{S:...}` iterate over.
pub fn expand_with(fmt: &str, vars: &Vars, lists: &Lists) -> String {
    expand_text(fmt, vars, lists, 0)
}

fn expand_text(text: &str, vars: &Vars, lists: &Lists, depth: usize) -> String {
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
                    out.push_str(&eval(&rest[1..1 + end], vars, lists, depth + 1));
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
fn eval(body: &str, vars: &Vars, lists: &Lists, depth: usize) -> String {
    if let Some(cond) = body.strip_prefix('?') {
        return conditional(&split_args(cond), vars, lists, depth);
    }
    if let Some(spec) = body.strip_prefix("s/") {
        return substitute(spec, vars, lists, depth);
    }
    let Some((op, arg)) = body.split_once(':') else {
        return lookup(vars, body).to_string();
    };
    match op {
        "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" | "!" => {
            compare(op, &split_args(arg), vars, lists, depth)
        }
        "b" => basename(&resolve(arg, vars, lists, depth)),
        "d" => dirname(&resolve(arg, vars, lists, depth)),
        "n" => resolve(arg, vars, lists, depth).chars().count().to_string(),
        "q" => shell_quote(&resolve(arg, vars, lists, depth)),
        "E" => expand_text(&resolve(arg, vars, lists, depth), vars, lists, depth + 1),
        "l" => unescape(arg),
        "m" | "m/i" | "m/r" | "m/ri" => matching(op, &split_args(arg), vars, lists, depth),
        "t" => local_time(&resolve(arg, vars, lists, depth)),
        "t/p" => pretty_time(&resolve(arg, vars, lists, depth)),
        "W" => each(&lists.windows, arg, lists, depth),
        "P" => each(&lists.panes, arg, lists, depth),
        "S" => each(&lists.sessions, arg, lists, depth),
        _ if op.starts_with("e|") => arithmetic(op, &split_args(arg), vars, lists, depth),
        _ => match op.strip_prefix('=').and_then(|n| n.parse::<i64>().ok()) {
            Some(n) => truncate(&resolve(arg, vars, lists, depth), n),
            None => lookup(vars, body).to_string(),
        },
    }
}

/// Expands `body` once per entry of `items`, each with only that entry's variables in scope.
fn each(items: &[Vars], body: &str, lists: &Lists, depth: usize) -> String {
    items
        .iter()
        .map(|item| expand_text(body, item, lists, depth))
        .collect()
}

/// `#{?c1,v1,c2,v2,...,else}`: the first value whose condition is true, else the trailing value.
fn conditional(args: &[&str], vars: &Vars, lists: &Lists, depth: usize) -> String {
    let mut rest = args;
    while let [cond, then, tail @ ..] = rest {
        if truthy(&resolve(cond, vars, lists, depth)) {
            return expand_text(then, vars, lists, depth);
        }
        rest = tail;
    }
    match rest {
        [otherwise] => expand_text(otherwise, vars, lists, depth),
        _ => String::new(),
    }
}

/// Comparison and boolean operators; yields `1` or `0`. Comparisons are numeric when both sides parse as integers.
fn compare(op: &str, args: &[&str], vars: &Vars, lists: &Lists, depth: usize) -> String {
    let value = |i: usize| {
        args.get(i)
            .map_or_else(String::new, |a| expand_text(a, vars, lists, depth))
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
fn substitute(spec: &str, vars: &Vars, lists: &Lists, depth: usize) -> String {
    let Some((pat, rest)) = spec.split_once('/') else {
        return String::new();
    };
    let Some((rep, rest)) = rest.split_once('/') else {
        return String::new();
    };
    let Some((flags, arg)) = rest.split_once(':') else {
        return String::new();
    };
    let value = resolve(arg, vars, lists, depth);
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
fn resolve(s: &str, vars: &Vars, lists: &Lists, depth: usize) -> String {
    if s.contains('#') {
        expand_text(s, vars, lists, depth)
    } else {
        lookup(vars, s).to_string()
    }
}

fn lookup<'a>(vars: &'a Vars, name: &str) -> &'a str {
    vars.get(name).map_or("", String::as_str)
}

/// Whether a format's value is true: non-empty and not `0` (the rule of `#{?...}` and `if-shell -F`).
pub fn truthy(s: &str) -> bool {
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

/// `#{m:pat,str}` matches a shell glob, `#{m/r:...}` a regex; `i` in the operator ignores case.
/// Expands to `1` on a match and `0` otherwise, as tmux does.
fn matching(op: &str, args: &[&str], vars: &Vars, lists: &Lists, depth: usize) -> String {
    let value = |i: usize| {
        args.get(i)
            .map_or_else(String::new, |a| expand_text(a, vars, lists, depth))
    };
    let (pattern, text) = (value(0), value(1));
    let ignore_case = op.ends_with('i');
    let on = if op.contains('r') {
        RegexBuilder::new(&pattern)
            .case_insensitive(ignore_case)
            .build()
            .is_ok_and(|re| re.is_match(&text))
    } else {
        let fold = |s: &str| -> Vec<char> {
            s.chars()
                .map(|c| {
                    if ignore_case {
                        c.to_ascii_lowercase()
                    } else {
                        c
                    }
                })
                .collect()
        };
        glob_at(&fold(&pattern), &fold(&text))
    };
    flag(on)
}

/// Glob match of `p` against `t`: `*`, `?` and `[...]` (a leading `!` or `^` negates the class).
fn glob_at(p: &[char], t: &[char]) -> bool {
    match p.split_first() {
        None => t.is_empty(),
        Some(('*', rest)) => (0..=t.len()).any(|i| glob_at(rest, &t[i..])),
        Some(('?', rest)) => !t.is_empty() && glob_at(rest, &t[1..]),
        Some(('[', rest)) => match rest.iter().position(|&c| c == ']') {
            Some(end) => {
                t.first().is_some_and(|&c| class_matches(&rest[..end], c))
                    && glob_at(&rest[end + 1..], &t[1..])
            }
            None => t.first() == Some(&'[') && glob_at(rest, &t[1..]),
        },
        Some((c, rest)) => t.first() == Some(c) && glob_at(rest, &t[1..]),
    }
}

fn class_matches(class: &[char], c: char) -> bool {
    let (negate, mut body) = match class.first() {
        Some('!' | '^') => (true, &class[1..]),
        _ => (false, class),
    };
    let mut hit = false;
    while let Some(&lo) = body.first() {
        if body.len() >= 3 && body[1] == '-' {
            hit |= (lo..=body[2]).contains(&c);
            body = &body[3..];
        } else {
            hit |= lo == c;
            body = &body[1..];
        }
    }
    hit != negate
}

/// `#{e|op:a,b}`: integer arithmetic with `+ - * / % m M` (`/` and `%` by zero give 0). With
/// `|f|N` the operands are floats printed with `N` decimals.
fn arithmetic(op: &str, args: &[&str], vars: &Vars, lists: &Lists, depth: usize) -> String {
    let parts: Vec<&str> = op.split('|').collect();
    let Some(sym) = parts.get(1).copied() else {
        return String::new();
    };
    let value = |i: usize| {
        args.get(i)
            .map_or_else(String::new, |a| expand_text(a, vars, lists, depth))
    };
    let (a, b) = (value(0), value(1));
    if parts.get(2) == Some(&"f") {
        let decimals = parts.get(3).and_then(|n| n.parse().ok()).unwrap_or(2);
        let (x, y) = (
            a.trim().parse().unwrap_or(0.0),
            b.trim().parse().unwrap_or(0.0),
        );
        let r: f64 = match sym {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            "/" if y != 0.0 => x / y,
            "%" if y != 0.0 => x % y,
            "/" | "%" => 0.0,
            "m" => x.min(y),
            "M" => x.max(y),
            _ => return String::new(),
        };
        return format!("{r:.decimals$}");
    }
    let (x, y) = (
        a.trim().parse::<i64>().unwrap_or(0),
        b.trim().parse::<i64>().unwrap_or(0),
    );
    let r = match sym {
        "+" => x.saturating_add(y),
        "-" => x.saturating_sub(y),
        "*" => x.saturating_mul(y),
        "/" => x.checked_div(y).unwrap_or(0),
        "%" => x.checked_rem(y).unwrap_or(0),
        "m" => x.min(y),
        "M" => x.max(y),
        _ => return String::new(),
    };
    r.to_string()
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The local broken-down time of Unix time `secs`; `None` when it cannot be converted.
#[cfg(unix)]
fn local_tm(secs: libc::time_t) -> Option<libc::tm> {
    // SAFETY: `tm` is a plain C struct of integers, valid when zeroed; `localtime_r` fills it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers refer to live locals; `localtime_r` only writes into `tm`.
    let ok = !unsafe { libc::localtime_r(&secs, &mut tm) }.is_null();
    ok.then_some(tm)
}

#[cfg(unix)]
fn weekday(tm: &libc::tm) -> &'static str {
    WEEKDAYS
        .get(usize::try_from(tm.tm_wday).unwrap_or(7))
        .copied()
        .unwrap_or("")
}

#[cfg(unix)]
fn month(tm: &libc::tm) -> &'static str {
    MONTHS
        .get(usize::try_from(tm.tm_mon).unwrap_or(12))
        .copied()
        .unwrap_or("")
}

/// `#{t:var}`: the Unix time in `var` as local time, `Thu Nov  2 10:00:00 2026` (ctime layout).
/// Empty when `var` is not a number or the time cannot be converted.
#[cfg(unix)]
fn local_time(value: &str) -> String {
    let Some(tm) = value.trim().parse().ok().and_then(local_tm) else {
        return String::new();
    };
    format!(
        "{} {} {:>2} {:02}:{:02}:{:02} {}",
        weekday(&tm),
        month(&tm),
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        tm.tm_year + 1900
    )
}

/// `#{t/p:var}`: how long ago the Unix time in `var` was. Within a day it is `HH:MM`, within a
/// week `Wed14:05`, and older `Wed02Nov`, in local time. Empty when `var` is not a number.
#[cfg(unix)]
fn pretty_time(value: &str) -> String {
    let Ok(secs) = value.trim().parse::<libc::time_t>() else {
        return String::new();
    };
    let Some(tm) = local_tm(secs) else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| libc::time_t::try_from(d.as_secs()).unwrap_or(0));
    let age = now - secs;
    if age < 86_400 {
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    } else if age < 7 * 86_400 {
        format!("{}{:02}:{:02}", weekday(&tm), tm.tm_hour, tm.tm_min)
    } else {
        format!("{}{:02}{}", weekday(&tm), tm.tm_mday, month(&tm))
    }
}

#[cfg(not(unix))]
fn local_time(_value: &str) -> String {
    String::new()
}

#[cfg(not(unix))]
fn pretty_time(_value: &str) -> String {
    String::new()
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

/// `text` with a backslash before each character that appears in `special`.
pub(crate) fn backslash_escape(text: &str, special: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if special.contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn shell_quote(v: &str) -> String {
    backslash_escape(v, "\\\"'$`!*?&;|<>()[]{}#~^ \t\n")
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

/// When a session's daemon started and whether it restored a snapshot: what the session's time
/// variables show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionTimes {
    /// Unix ms the daemon started.
    pub started_ms: u64,
    /// Unix ms the snapshot was restored, when it was.
    pub restored_ms: Option<u64>,
}

impl SessionTimes {
    /// `session_id`: `$` and the daemon's start in ms, the same for as long as the daemon runs.
    pub fn id(&self) -> String {
        format!("${}", self.started_ms)
    }

    /// `session_created`: the unix seconds the session was made; a restored session's is the restore.
    pub fn created(&self) -> u64 {
        self.restored_ms.unwrap_or(self.started_ms) / 1000
    }

    /// `start_time`: the unix seconds the daemon started.
    pub fn start_time(&self) -> u64 {
        self.started_ms / 1000
    }
}

/// Variables that do not depend on the pane or session being described.
fn shared_vars(vars: &mut Vars, session: &str, times: SessionTimes, attached: usize) {
    let host = host_name();
    let host_short = host.split('.').next().unwrap_or_default().to_string();
    vars.insert("client_session", session.to_string());
    vars.insert("session_id", times.id());
    vars.insert("session_created", times.created().to_string());
    vars.insert("start_time", times.start_time().to_string());
    vars.insert("session_attached", attached.to_string());
    vars.insert("host", host);
    vars.insert("host_short", host_short);
    vars.insert("pid", std::process::id().to_string());
    vars.insert("version", TMUX_VERSION.to_string());
}

/// Cursor position and scrollback size of a pane with a terminal; absent for any other pane.
fn terminal_vars(p: &PaneView, vars: &mut Vars) {
    if let Some((x, y)) = p.cursor {
        vars.insert("cursor_x", x.to_string());
        vars.insert("cursor_y", y.to_string());
    }
    if let Some(lines) = p.history_size {
        vars.insert("history_size", lines.to_string());
    }
}

/// The variables of a session with no pane to describe: its name, times and the host's.
pub fn session_vars(session: &str, times: SessionTimes, attached: usize) -> Vars {
    let mut vars = Vars::from([("session_name", session.to_string())]);
    shared_vars(&mut vars, session, times, attached);
    vars
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
    vars.insert("window_activity_flag", flag(p.window_activity));
    vars.insert("window_bell_flag", flag(p.window_bell));
    vars.insert("window_silence_flag", flag(p.window_silence));
    vars.insert("window_last_flag", flag(p.window_last));
    vars.insert("window_marked_flag", flag(p.window_marked));
    vars.insert("pane_synchronized", flag(p.synchronized));
    vars.insert("pane_at_left", flag(p.edges[0]));
    vars.insert("pane_at_top", flag(p.edges[1]));
    vars.insert("pane_at_right", flag(p.edges[2]));
    vars.insert("pane_at_bottom", flag(p.edges[3]));
    terminal_vars(p, &mut vars);
    shared_vars(&mut vars, &p.session, p.times, p.session_attached);
    vars
}

/// Variables of one attached client, for `list-clients` formats; `attached` is how many clients
/// its session has.
pub fn client_vars(info: &ClientInfo, attached: usize) -> Vars {
    let mut vars = Vars::new();
    let tty = if info.name.starts_with("/dev/") {
        info.name.clone()
    } else {
        String::new()
    };
    vars.insert("client_name", info.name.clone());
    vars.insert("client_tty", tty);
    vars.insert("client_width", info.width.to_string());
    vars.insert("client_height", info.height.to_string());
    vars.insert("client_session", info.session.clone());
    vars.insert("client_readonly", flag(info.readonly));
    vars.insert("client_created", (info.created_ms / 1000).to_string());
    vars.insert("client_activity", (info.activity_ms / 1000).to_string());
    vars.insert("session_name", info.session.clone());
    vars.insert("session_attached", attached.to_string());
    vars
}

/// The tmux window flags of one window; `render` writes them in tmux's order.
#[derive(Default)]
pub(crate) struct WindowFlags {
    pub(crate) activity: bool,
    pub(crate) bell: bool,
    pub(crate) silence: bool,
    pub(crate) current: bool,
    pub(crate) zoomed: bool,
    pub(crate) last: bool,
    pub(crate) marked: bool,
}

impl WindowFlags {
    /// `#` activity, `!` bell, `*` current, `Z` zoomed, `-` last, `M` marked pane.
    pub(crate) fn render(&self) -> String {
        let mut flags = String::new();
        if self.activity {
            flags.push('#');
        }
        if self.bell {
            flags.push('!');
        }
        if self.silence {
            flags.push('~');
        }
        if self.current {
            flags.push('*');
        }
        if self.zoomed {
            flags.push('Z');
        }
        if self.last {
            flags.push('-');
        }
        if self.marked {
            flags.push('M');
        }
        flags
    }

    /// Flags of a tab; `zoomed` applies only to the active window.
    pub(crate) fn of_tab(tab: &WindowTab, zoomed: bool) -> Self {
        let (activity, bell, current) = (tab.has_activity, tab.has_bell, tab.is_active);
        Self {
            activity,
            bell,
            current,
            silence: tab.has_silence,
            zoomed: current && zoomed,
            last: tab.is_last,
            marked: tab.has_marked,
        }
    }
}

impl From<&PaneView> for WindowFlags {
    fn from(p: &PaneView) -> Self {
        Self {
            activity: p.window_activity,
            bell: p.window_bell,
            current: p.window_active,
            silence: p.window_silence,
            zoomed: p.window_zoomed,
            last: p.window_last,
            marked: p.window_marked,
        }
    }
}

fn window_flags(p: &PaneView) -> String {
    WindowFlags::from(p).render()
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
    let edges = at_edges(rect, rects);
    PaneView {
        session: session.to_string(),
        times: SessionTimes::default(),
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
        window_silence: tab.is_some_and(|t| t.has_silence),
        synchronized: tab.is_some_and(|t| t.synchronized),
        edges,
        cursor: node.cursor,
        history_size: node.history_size,
        session_windows: tabs.len(),
        ..Default::default()
    }
}

/// The `list-buffers -F` default: `name: N bytes: "sample"`.
pub const DEFAULT_BUFFER_FORMAT: &str =
    "#{buffer_name}: #{buffer_size} bytes: \"#{buffer_sample}\"";

/// Variables of one paste buffer, for `list-buffers -F`.
pub fn buffer_vars(buffer: &Buffer) -> Vars {
    Vars::from([
        ("buffer_name", buffer.name.clone()),
        ("buffer_size", buffer.data.len().to_string()),
        ("buffer_sample", buffer_sample(&buffer.data)),
    ])
}

/// The first 50 characters of a buffer, control characters shown as escapes.
pub fn buffer_sample(data: &str) -> String {
    const SAMPLE_CHARS: usize = 50;
    let mut out = String::new();
    for (i, c) in data.chars().enumerate() {
        if i == SAMPLE_CHARS {
            out.push_str("...");
            break;
        }
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

/// Whether `rect` touches the left, top, right and bottom edges of the laid-out panes.
fn at_edges(rect: Option<Rect>, rects: &[(String, Rect, bool)]) -> [bool; 4] {
    let Some(r) = rect else {
        return [false; 4];
    };
    let left = rects.iter().map(|(_, q, _)| q.x).min().unwrap_or(r.x);
    let top = rects.iter().map(|(_, q, _)| q.y).min().unwrap_or(r.y);
    let right = rects
        .iter()
        .map(|(_, q, _)| q.x.saturating_add(q.width))
        .max()
        .unwrap_or(0);
    let bottom = rects
        .iter()
        .map(|(_, q, _)| q.y.saturating_add(q.height))
        .max()
        .unwrap_or(0);
    [
        r.x == left,
        r.y == top,
        r.x.saturating_add(r.width) == right,
        r.y.saturating_add(r.height) == bottom,
    ]
}
