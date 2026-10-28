//! Line editor for the `:` command prompt. Pure: no terminal or daemon access.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptState {
    chars: Vec<char>,
    /// Insertion point, in characters.
    cursor: usize,
    /// Index into history while browsing with Up/Down.
    hist: Option<usize>,
    /// Text typed before history browsing started.
    draft: Vec<char>,
}

impl PromptState {
    pub fn new(initial: &str) -> Self {
        let chars: Vec<char> = initial.chars().collect();
        Self {
            cursor: chars.len(),
            chars,
            hist: None,
            draft: Vec::new(),
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn insert(&mut self, c: char) {
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

    /// Tab: completes the command name. A unique match gets a trailing space; several matches
    /// extend to their common prefix. Arguments are not completed.
    pub fn complete(&mut self, names: &[&str]) {
        let prefix = self.text();
        if prefix.chars().any(char::is_whitespace) {
            return;
        }
        let matches: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| n.starts_with(&prefix))
            .collect();
        match matches.as_slice() {
            [] => {}
            [only] => self.show(&format!("{only} ")),
            many => {
                let common = common_prefix(many);
                if common.chars().count() > prefix.chars().count() {
                    self.show(&common);
                }
            }
        }
    }

    /// Any edit leaves history browsing.
    fn edit(&mut self) {
        self.hist = None;
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
