//! Shell-side helpers of the tmux surface: command-string splitting and `run-shell` / `if-shell`.
use std::process::Stdio;

use tokio::process::Command;

/// Splits a tmux command string into commands and their words. Whitespace separates words, `'..'`
/// is literal, `".."` honours backslash escapes, an unescaped `;` ends a command and `\;` is a
/// literal semicolon. Empty commands are dropped.
pub fn split_commands(line: &str) -> Result<Vec<Vec<String>>, String> {
    let mut commands: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("unterminated ' in command".to_string()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err("unterminated \" in command".to_string()),
                        },
                        Some(c) => word.push(c),
                        None => return Err("unterminated \" in command".to_string()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(c) => word.push(c),
                    None => word.push('\\'),
                }
            }
            ';' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    if !words.is_empty() {
        commands.push(words);
    }
    Ok(commands)
}

/// Result of a shell command: what it printed and its exit code (`-1` when killed by a signal).
pub struct ShellOutput {
    pub stdout: String,
    pub code: i32,
}

fn shell_command(cmd: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd);
    command
}

/// Runs `cmd` with `sh -c`, collecting its standard output; standard error passes through.
pub async fn run_shell(cmd: &str) -> Result<ShellOutput, String> {
    let output = shell_command(cmd)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .await
        .map_err(|e| format!("failed to run '{cmd}': {e}"))?;
    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        code: output.status.code().unwrap_or(-1),
    })
}

/// Starts `cmd` with `sh -c` and does not wait for it.
pub fn spawn_shell(cmd: &str) -> Result<(), String> {
    shell_command(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to run '{cmd}': {e}"))
}

/// The text of a successful format expansion counts as true unless it is empty or `0`.
pub fn format_is_true(expanded: &str) -> bool {
    !expanded.is_empty() && expanded != "0"
}

/// The commands of a tmux config file with their line numbers: a trailing `\` joins the next line,
/// blank and `#` comment lines are dropped.
pub fn config_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (i, raw) in text.lines().enumerate() {
        let (number, mut line) = pending.take().unwrap_or((i + 1, String::new()));
        line.push_str(raw);
        if let Some(joined) = line.strip_suffix('\\') {
            pending = Some((number, joined.to_string()));
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            out.push((number, trimmed.to_string()));
        }
    }
    if let Some((number, line)) = pending {
        out.push((number, line));
    }
    out
}
