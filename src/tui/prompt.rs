//! Line editor for the `:` command prompt. Pure: no terminal or daemon access.

use std::collections::VecDeque;

use crate::format::backslash_escape;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptState {
    chars: Vec<char>,
    /// Insertion point, in characters.
    cursor: usize,
    /// Index into history while browsing with Up/Down.
    hist: Option<usize>,
    /// Text typed before history browsing started.
    draft: Vec<char>,
    comp: Option<CompletionState>,
    /// Text before the input (`(command) `, `:`, or a `command-prompt -p` text).
    label: String,
    /// What Tab completes (`command-prompt -T`).
    kind: PromptKind,
    /// Only digits are accepted (`command-prompt -N`).
    numeric: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionState {
    original_prefix: String,
    candidates: Vec<String>,
    index: Option<usize>,
}

impl PromptState {
    /// The `:` prompt: the typed line is a command, completing command names.
    pub fn new(initial: &str) -> Self {
        Self::with(":", initial, PromptKind::Command, false)
    }

    /// A prompt showing `label` before its text, completing `kind`; `numeric` accepts digits only.
    pub fn with(label: &str, initial: &str, kind: PromptKind, numeric: bool) -> Self {
        let chars: Vec<char> = initial.chars().collect();
        Self {
            cursor: chars.len(),
            chars,
            hist: None,
            draft: Vec::new(),
            comp: None,
            label: label.to_string(),
            kind,
            numeric,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn kind(&self) -> PromptKind {
        self.kind
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn insert(&mut self, c: char) {
        if self.numeric && !c.is_ascii_digit() {
            return;
        }
        self.edit();
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        self.edit();
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        self.edit();
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    /// Ctrl-u: clears the whole line.
    pub fn clear(&mut self) {
        self.edit();
        self.chars.clear();
        self.cursor = 0;
    }

    /// Ctrl-w: deletes the word before the cursor (trailing blanks first, then the word).
    pub fn delete_word(&mut self) {
        self.edit();
        let mut start = self.cursor;
        while start > 0 && self.chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !self.chars[start - 1].is_whitespace() {
            start -= 1;
        }
        self.chars.drain(start..self.cursor);
        self.cursor = start;
    }

    /// Up: steps back through history, keeping the typed text as the draft.
    pub fn history_prev(&mut self, history: &[String]) {
        if history.is_empty() {
            return;
        }
        let idx = match self.hist {
            None => {
                self.draft = self.chars.clone();
                history.len() - 1
            }
            Some(0) => return,
            Some(i) => i - 1,
        };
        self.show(&history[idx]);
        self.hist = Some(idx);
    }

    /// Down: steps forward through history, restoring the draft past the newest entry.
    pub fn history_next(&mut self, history: &[String]) {
        match self.hist {
            None => {}
            Some(i) if i + 1 < history.len() => {
                self.show(&history[i + 1]);
                self.hist = Some(i + 1);
            }
            Some(_) => {
                self.hist = None;
                self.chars = self.draft.clone();
                self.cursor = self.chars.len();
            }
        }
    }

    /// Tab: completes the command name like tmux. A unique match gets a trailing space; several matches
    /// extend to their common prefix on the first press and cycle through candidates on repeated Tab.
    pub fn complete(&mut self, names: &[&str]) {
        self.cycle_complete(names, true);
    }

    /// Shift-Tab / BackTab: cycles through matching candidates in reverse order.
    pub fn complete_prev(&mut self, names: &[&str]) {
        self.cycle_complete(names, false);
    }

    fn cycle_complete(&mut self, names: &[&str], forward: bool) {
        let text = self.text();
        if text.chars().any(char::is_whitespace) {
            return;
        }

        if let Some(mut comp) = self.comp.take()
            && !comp.candidates.is_empty()
        {
            let n = comp.candidates.len();
                let next_idx = match comp.index {
                    None => {
                        if forward {
                            0
                        } else {
                            n - 1
                        }
                    }
                    Some(i) => {
                        if forward {
                            (i + 1) % n
                        } else {
                            (i + n - 1) % n
                        }
                    }
                };
                comp.index = Some(next_idx);
                let choice = &comp.candidates[next_idx];
                self.show(choice);
                self.comp = Some(comp);
                return;
        }

        let prefix = text;
        let matches: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| n.starts_with(&prefix))
            .collect();

        match matches.as_slice() {
            [] => {}
            [only] => {
                self.show(&format!("{only} "));
                self.comp = None;
            }
            many => {
                let common = common_prefix(many);
                let candidates: Vec<String> = many.iter().map(|s| s.to_string()).collect();
                if common.chars().count() > prefix.chars().count() {
                    self.show(&common);
                }
                self.comp = Some(CompletionState {
                    original_prefix: prefix,
                    candidates,
                    index: None,
                });
            }
        }
    }

    /// Any edit leaves history browsing and resets completion cycling.
    fn edit(&mut self) {
        self.hist = None;
        self.comp = None;
    }

    fn show(&mut self, text: &str) {
        self.chars = text.chars().collect();
        self.cursor = self.chars.len();
    }
}

fn common_prefix(names: &[&str]) -> String {
    let Some((first, rest)) = names.split_first() else {
        return String::new();
    };
    let mut len = first.len();
    for name in rest {
        let shared = first
            .char_indices()
            .zip(name.chars())
            .take_while(|((_, a), b)| a == b)
            .last()
            .map_or(0, |((i, c), _)| i + c.len_utf8());
        len = len.min(shared);
    }
    first[..len].to_string()
}

/// What a prompt's Tab completes, as `command-prompt -T` names it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PromptKind {
    /// Command names (the default).
    #[default]
    Command,
    /// Accepted for tmux compatibility; nothing to complete.
    Search,
    /// Accepted for tmux compatibility; nothing to complete.
    Target,
    /// Window names.
    WindowTarget,
}

/// One question of a `command-prompt`: its label, the text it starts with, and its completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub label: String,
    pub initial: String,
    pub kind: PromptKind,
    pub numeric: bool,
}

/// A `command-prompt` in progress: the question on screen, the questions still to come, and the
/// answers so far. Without a template the typed line is the command; with one, every answer is
/// substituted into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptRun {
    pub line: PromptState,
    /// Questions after the one on screen, in order.
    pub queued: VecDeque<Question>,
    /// Answers given so far, in question order.
    pub answers: Vec<String>,
    /// Command template with `%%` and `%1`..`%9` placeholders; `None` runs the typed line.
    pub template: Option<String>,
    /// `-1` / `-k`: the first key typed is the answer, with no Enter.
    pub single_key: bool,
}

impl PromptRun {
    /// A run asking `questions` in order (the `:` prompt when there are none).
    pub fn new(
        mut questions: VecDeque<Question>,
        template: Option<String>,
        single_key: bool,
    ) -> Self {
        let default_label = if template.is_some() { "(command) " } else { ":" };
        let first = questions.pop_front().unwrap_or(Question {
            label: default_label.to_string(),
            initial: String::new(),
            kind: PromptKind::Command,
            numeric: false,
        });
        Self {
            line: PromptState::with(&first.label, &first.initial, first.kind, first.numeric),
            queued: questions,
            answers: Vec::new(),
            template,
            single_key,
        }
    }

    /// Records the answer to the question on screen. Returns the command line to run once every
    /// question is answered; until then it moves on to the next question and returns `None`.
    pub fn answer(&mut self, text: String) -> Option<String> {
        let Some(template) = self.template.clone() else {
            return Some(text);
        };
        self.answers.push(text.trim().to_string());
        match self.queued.pop_front() {
            Some(next) => {
                self.line = PromptState::with(&next.label, &next.initial, next.kind, next.numeric);
                None
            }
            None => Some(expand_template(&template, &self.answers)),
        }
    }
}

/// Expands a `command-prompt` template: `%%` is the first answer, `%1`..`%9` the answer with that
/// number, and `%%%` the first answer escaped for the command parser. A placeholder without an
/// answer stays as written.
pub fn expand_template(template: &str, answers: &[String]) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && chars.get(i + 1) == Some(&'%') {
            let escaped = chars.get(i + 2) == Some(&'%');
            let width = if escaped { 3 } else { 2 };
            match answers.first() {
                Some(answer) if escaped => out.push_str(&escape_word(answer)),
                Some(answer) => out.push_str(answer),
                None => out.extend(&chars[i..i + width]),
            }
            i += width;
            continue;
        }
        if chars[i] == '%'
            && let Some(answer) = chars
                .get(i + 1)
                .and_then(|c| c.to_digit(10))
                .and_then(|n| n.checked_sub(1).and_then(|k| answers.get(k as usize)))
        {
            out.push_str(answer);
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Backslash-escapes the characters the command parser treats specially.
fn escape_word(text: &str) -> String {
    backslash_escape(text, "\\;'\"$")
}
