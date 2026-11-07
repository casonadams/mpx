use regex::{NoExpand, RegexBuilder};

use super::time::{local_time, pretty_time};
use super::{Lists, Vars, flag, truthy};

/// Recursion limit for nested formats and `#{E:...}` re-expansion.
const MAX_DEPTH: usize = 32;

/// Characters `#{q:...}` backslash-escapes for the shell.
const SHELL_SPECIAL: &str = "\\\"'$`!*?&;|<>()[]{}#~^ \t\n";

/// Expands `#{...}` formats, the aliases `#S #I #P #W #T #D`, and the escapes `##`, `#,`, `#}`.
///
/// Unknown variables expand to empty. Malformed input (an unclosed `#{`) is left as text.
pub fn expand(fmt: &str, vars: &Vars) -> String {
    expand_with(fmt, vars, &Lists::default())
}

/// [`expand`] with the lists the loop formats `#{W:...}`, `#{P:...}` and `#{S:...}` iterate over.
pub fn expand_with(fmt: &str, vars: &Vars, lists: &Lists) -> String {
    Scope {
        vars,
        lists,
        depth: 0,
    }
    .expand(fmt)
}

/// What a format is expanded against: its variables, the loop lists and how deeply it is nested.
#[derive(Clone, Copy)]
struct Scope<'a> {
    vars: &'a Vars,
    lists: &'a Lists,
    depth: usize,
}

impl Scope<'_> {
    fn deeper(self) -> Self {
        Self {
            depth: self.depth + 1,
            ..self
        }
    }

    fn expand(self, text: &str) -> String {
        if self.depth > MAX_DEPTH {
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
                        out.push_str(&eval(&rest[1..1 + end], self.deeper()));
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
                        out.push_str(lookup(self.vars, name));
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

    /// Value of a variable name, or the expansion of a nested format. Unknown names are empty.
    fn resolve(self, s: &str) -> String {
        if s.contains('#') {
            self.expand(s)
        } else {
            lookup(self.vars, s).to_string()
        }
    }

    /// The expansion of the `i`th operand, empty when there is none.
    fn operand(self, args: &[&str], i: usize) -> String {
        args.get(i).map_or_else(String::new, |a| self.expand(a))
    }
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

fn lookup<'a>(vars: &'a Vars, name: &str) -> &'a str {
    vars.get(name).map_or("", String::as_str)
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
fn eval(body: &str, sc: Scope) -> String {
    if let Some(cond) = body.strip_prefix('?') {
        return conditional(&split_args(cond), sc);
    }
    if let Some(spec) = body.strip_prefix("s/") {
        return substitute(spec, sc);
    }
    let Some((op, arg)) = body.split_once(':') else {
        return lookup(sc.vars, body).to_string();
    };
    match op {
        "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" | "!" => {
            compare(op, &split_args(arg), sc)
        }
        "b" => basename(&sc.resolve(arg)),
        "d" => dirname(&sc.resolve(arg)),
        "n" => sc.resolve(arg).chars().count().to_string(),
        "q" => backslash_escape(&sc.resolve(arg), SHELL_SPECIAL),
        "E" => sc.deeper().expand(&sc.resolve(arg)),
        "l" => unescape(arg),
        "m" | "m/i" | "m/r" | "m/ri" => matching(op, &split_args(arg), sc),
        "t" => local_time(&sc.resolve(arg)),
        "t/p" => pretty_time(&sc.resolve(arg)),
        "W" => each(&sc.lists.windows, arg, sc),
        "P" => each(&sc.lists.panes, arg, sc),
        "S" => each(&sc.lists.sessions, arg, sc),
        _ if op.starts_with("e|") => arithmetic(op, &split_args(arg), sc),
        _ => match op.strip_prefix('=').and_then(|n| n.parse::<i64>().ok()) {
            Some(n) => truncate(&sc.resolve(arg), n),
            None => lookup(sc.vars, body).to_string(),
        },
    }
}

/// Expands `body` once per entry of `items`, each with only that entry's variables in scope.
fn each(items: &[Vars], body: &str, sc: Scope) -> String {
    items
        .iter()
        .map(|item| Scope { vars: item, ..sc }.expand(body))
        .collect()
}

/// `#{?c1,v1,c2,v2,...,else}`: the first value whose condition is true, else the trailing value.
fn conditional(args: &[&str], sc: Scope) -> String {
    let mut rest = args;
    while let [cond, then, tail @ ..] = rest {
        if truthy(&sc.resolve(cond)) {
            return sc.expand(then);
        }
        rest = tail;
    }
    match rest {
        [otherwise] => sc.expand(otherwise),
        _ => String::new(),
    }
}

/// Comparison and boolean operators; yields `1` or `0`. Comparisons are numeric when both sides parse as integers.
fn compare(op: &str, args: &[&str], sc: Scope) -> String {
    let (a, b) = (sc.operand(args, 0), sc.operand(args, 1));
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
fn substitute(spec: &str, sc: Scope) -> String {
    let Some((pat, rest)) = spec.split_once('/') else {
        return String::new();
    };
    let Some((rep, rest)) = rest.split_once('/') else {
        return String::new();
    };
    let Some((flags, arg)) = rest.split_once(':') else {
        return String::new();
    };
    let value = sc.resolve(arg);
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
fn matching(op: &str, args: &[&str], sc: Scope) -> String {
    let (pattern, text) = (sc.operand(args, 0), sc.operand(args, 1));
    let ignore_case = op.ends_with('i');
    let on = if op.contains('r') {
        RegexBuilder::new(&pattern)
            .case_insensitive(ignore_case)
            .build()
            .is_ok_and(|re| re.is_match(&text))
    } else {
        let fold = |s: &str| -> Vec<char> {
            let chars = s.chars();
            if ignore_case {
                chars.map(|c| c.to_ascii_lowercase()).collect()
            } else {
                chars.collect()
            }
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
fn arithmetic(op: &str, args: &[&str], sc: Scope) -> String {
    let parts: Vec<&str> = op.split('|').collect();
    let Some(sym) = parts.get(1).copied() else {
        return String::new();
    };
    let (a, b) = (sc.operand(args, 0), sc.operand(args, 1));
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

fn basename(v: &str) -> String {
    let trimmed = v.trim_end_matches(is_separator);
    if trimmed.is_empty() {
        return if v.is_empty() { "" } else { "/" }.to_string();
    }
    let name = trimmed.rsplit(is_separator).next().unwrap_or(trimmed);
    name.to_string()
}

fn dirname(v: &str) -> String {
    let trimmed = v.trim_end_matches(is_separator);
    if trimmed.is_empty() {
        return if v.is_empty() { "." } else { "/" }.to_string();
    }
    let Some(i) = trimmed.rfind(is_separator) else {
        return ".".to_string();
    };
    let dir = trimmed[..i].trim_end_matches(is_separator);
    if dir.is_empty() { "/" } else { dir }.to_string()
}

/// Whether `c` separates path components: `/`, and also `\` on Windows.
fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
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
