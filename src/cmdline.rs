//! Command-line text helpers: splitting a command string into commands and words, and joining words back.

/// Commands and words read so far.
#[derive(Default)]
struct Splitter {
    commands: Vec<Vec<String>>,
    words: Vec<String>,
    word: String,
    in_word: bool,
}

impl Splitter {
    fn push_str(&mut self, text: &str) {
        self.in_word = true;
        self.word.push_str(text);
    }

    fn end_word(&mut self) {
        if self.in_word {
            self.words.push(std::mem::take(&mut self.word));
            self.in_word = false;
        }
    }

    fn end_command(&mut self) {
        self.end_word();
        if !self.words.is_empty() {
            self.commands.push(std::mem::take(&mut self.words));
        }
    }
}

/// The text of a `'..'` quote whose opening quote is already read.
fn single_quoted(chars: &mut std::str::Chars) -> Result<String, String> {
    let mut text = String::new();
    for c in chars.by_ref() {
        if c == '\'' {
            return Ok(text);
        }
        text.push(c);
    }
    Err("unterminated ' in command".to_string())
}

/// The text of a `".."` quote whose opening quote is already read; `\"`, `\\` and `\$` unescape.
fn double_quoted(chars: &mut std::str::Chars) -> Result<String, String> {
    let mut text = String::new();
    while let Some(c) = chars.next() {
        match (c, chars.as_str().chars().next()) {
            ('"', _) => return Ok(text),
            ('\\', Some(escaped @ ('"' | '\\' | '$'))) => {
                chars.next();
                text.push(escaped);
            }
            (c, _) => text.push(c),
        }
    }
    Err("unterminated \" in command".to_string())
}

/// Splits a tmux command string into commands and their words. Whitespace separates words, `'..'`
/// is literal, `".."` honours backslash escapes, an unescaped `;` ends a command and `\;` is a
/// literal semicolon. Empty commands are dropped.
pub fn split_commands(line: &str) -> Result<Vec<Vec<String>>, String> {
    let mut split = Splitter::default();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => split.push_str(&single_quoted(&mut chars)?),
            '"' => split.push_str(&double_quoted(&mut chars)?),
            '\\' => split.push_str(&chars.next().unwrap_or('\\').to_string()),
            ';' => split.end_command(),
            c if c.is_whitespace() => split.end_word(),
            c => split.push_str(c.encode_utf8(&mut [0; 4])),
        }
    }
    split.end_command();
    Ok(split.commands)
}

/// Joins words into a command line that `parse` reads back as the same words: a word that would
/// split or end a command is quoted, and a lone `;` stays a separator.
pub fn join_words(words: &[String]) -> String {
    let quote = |w: &String| {
        let plain = !w.is_empty() && !w.chars().any(|c| c.is_whitespace() || "'\"\\;".contains(c));
        if plain || w == ";" {
            w.clone()
        } else {
            format!("'{}'", w.replace('\'', "'\\''"))
        }
    };
    words.iter().map(quote).collect::<Vec<_>>().join(" ")
}

/// The command line for the words after a key or hook name: one word is already a command string
/// (`bind x "split-window -h"`), several are the command's own words.
pub fn command_line(words: &[String]) -> String {
    match words {
        [one] => one.clone(),
        many => join_words(many),
    }
}
