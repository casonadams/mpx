//! Splitting a command line into words, and expanding the config's command aliases.
use std::collections::BTreeMap;

/// Splits a command line into words: whitespace separates, `'..'` is literal, `".."` honours
/// `\"` and `\\`, and a bare backslash escapes the next character.
pub fn split_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '\\' && matches!(chars.peek(), Some(&('"' | '\\'))) => {
                cur.push(chars.next().unwrap_or(c));
            }
            Some(_) => cur.push(c),
            None if c.is_whitespace() => flush_word(&mut words, &mut cur, &mut in_word),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                in_word = true;
            }
            None if c == '\\' => {
                in_word = true;
                match chars.peek() {
                    Some(&next)
                        if matches!(next, '\\' | '\'' | '"' | ';') || next.is_whitespace() =>
                    {
                        cur.push(next);
                        chars.next();
                    }
                    _ => cur.push('\\'),
                }
            }
            None => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated quote".to_string());
    }
    flush_word(&mut words, &mut cur, &mut in_word);
    Ok(words)
}

fn flush_word(words: &mut Vec<String>, cur: &mut String, in_word: &mut bool) {
    if *in_word {
        words.push(std::mem::take(cur));
        *in_word = false;
    }
}

/// The words with an aliased command name replaced by the alias's words. An alias is expanded
/// once, so an alias may name another command but not loop.
pub(super) fn expand_alias(
    mut words: Vec<String>,
    aliases: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let Some(target) = words.first().and_then(|name| aliases.get(name)) else {
        return Ok(words);
    };
    let mut expanded = split_words(target)?;
    expanded.extend(words.drain(1..));
    Ok(expanded)
}
