//! The command lines of a config file read by `source-file`: comment lines and `\` continuations
//! are joined and skipped, `%if` / `%elif` / `%else` / `%endif` select lines, and `%hidden NAME=value`
//! defines a variable that `#{NAME}` in later lines and conditions expands to.
use std::collections::BTreeMap;

use crate::format::truthy;

/// The command lines of `text` that are active, with the line number of each one's first line.
/// `eval` expands the tmux formats of a condition or a `%hidden` value.
pub fn lines(text: &str, eval: impl Fn(&str) -> String) -> Result<Vec<(usize, String)>, String> {
    let mut out = Vec::new();
    let mut conditions = Conditions::default();
    for (line_no, line) in logical_lines(text) {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(directive) = line.strip_prefix('%') else {
            if conditions.active() {
                out.push((line_no, substitute(line, &conditions.hidden)));
            }
            continue;
        };
        let (word, rest) = directive
            .split_once(char::is_whitespace)
            .map_or((directive, ""), |(word, rest)| (word, rest.trim()));
        conditions.directive(word, rest, line_no, &eval)?;
    }
    match conditions.frames.last() {
        Some(frame) => Err(format!("line {}: %if without %endif", frame.line)),
        None => Ok(out),
    }
}

/// The `%if` stack and the `%hidden` variables read so far.
#[derive(Default)]
struct Conditions {
    frames: Vec<Frame>,
    hidden: BTreeMap<String, String>,
}

impl Conditions {
    /// Whether the lines read now are in an active branch.
    fn active(&self) -> bool {
        self.frames.last().is_none_or(|f| f.active)
    }

    /// Applies the directive `word` with its argument `rest`, read on `line_no`.
    fn directive(
        &mut self,
        word: &str,
        rest: &str,
        line_no: usize,
        eval: &impl Fn(&str) -> String,
    ) -> Result<(), String> {
        match word {
            "if" | "elif" | "else" | "endif" => self.branch(word, rest, line_no, eval),
            "hidden" if self.active() => self.set_hidden(rest, line_no, eval),
            "hidden" => Ok(()),
            other => Err(format!("line {line_no}: unknown directive %{other}")),
        }
    }

    /// `%if` opens a block, `%elif` and `%else` continue it and `%endif` closes it.
    fn branch(
        &mut self,
        word: &str,
        rest: &str,
        line_no: usize,
        eval: &impl Fn(&str) -> String,
    ) -> Result<(), String> {
        let Conditions { frames, hidden } = self;
        let active = frames.last().is_none_or(|f| f.active);
        match word {
            "if" => {
                let cond = active && truthy(&eval(&substitute(rest, hidden)));
                frames.push(Frame {
                    parent: active,
                    seen: cond,
                    active: cond,
                    line: line_no,
                });
            }
            "elif" => {
                let frame = frames
                    .last_mut()
                    .ok_or_else(|| format!("line {line_no}: %elif without %if"))?;
                let cond = frame.parent && !frame.seen && truthy(&eval(&substitute(rest, hidden)));
                frame.active = cond;
                frame.seen |= cond;
            }
            "else" => {
                let frame = frames
                    .last_mut()
                    .ok_or_else(|| format!("line {line_no}: %else without %if"))?;
                frame.active = frame.parent && !frame.seen;
                frame.seen = true;
            }
            _ => {
                frames
                    .pop()
                    .ok_or_else(|| format!("line {line_no}: %endif without %if"))?;
            }
        }
        Ok(())
    }

    /// `%hidden NAME=value`: defines a variable whose value is expanded first.
    fn set_hidden(
        &mut self,
        rest: &str,
        line_no: usize,
        eval: &impl Fn(&str) -> String,
    ) -> Result<(), String> {
        let (name, value) = hidden_assignment(rest)
            .ok_or_else(|| format!("line {line_no}: %hidden needs NAME=value"))?;
        let value = eval(&substitute(value, &self.hidden));
        self.hidden.insert(name.to_string(), value);
        Ok(())
    }
}

/// One `%if` block: whether a branch was taken, whether the enclosing block is active, and
/// whether the current branch is.
struct Frame {
    parent: bool,
    seen: bool,
    active: bool,
    line: usize,
}

/// The lines of `text` with `\` continuations joined, each with the number of its first line.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut continuing = false;
    for (index, raw) in text.lines().enumerate() {
        let continued = raw.ends_with('\\');
        let piece = raw.strip_suffix('\\').unwrap_or(raw);
        match out.last_mut() {
            Some((_, line)) if continuing => line.push_str(piece),
            _ => out.push((index + 1, piece.to_string())),
        }
        continuing = continued;
    }
    out
}

/// `NAME=value` with the value's surrounding quotes removed.
fn hidden_assignment(text: &str) -> Option<(&str, &str)> {
    let (name, value) = text.split_once('=')?;
    let valid = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    if !valid {
        return None;
    }
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    Some((name, value))
}

/// `text` with every `#{NAME}` of a hidden variable replaced by its value.
fn substitute(text: &str, hidden: &BTreeMap<String, String>) -> String {
    hidden.iter().fold(text.to_string(), |text, (name, value)| {
        text.replace(&format!("#{{{name}}}"), value)
    })
}
