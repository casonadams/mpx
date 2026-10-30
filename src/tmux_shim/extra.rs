//! tmux commands backed by session state kept in the daemon: options, environment, paste
//! buffers, `wait-for`, and the commands that reach attached terminals.
use std::io::{Read, Write};

use super::args::Invocation;
use super::exec::{Shim, window_key};
use super::format::{Vars, expand};
use crate::config::Config;
use crate::protocol::{Buffer, ChannelAction, VarKind};

/// Longest buffer prefix `list-buffers` shows.
const SAMPLE_CHARS: usize = 50;

/// Value of a tmux option that mpx has a real equivalent for, or the same fixed answer as tmux.
/// Anything else is only known once it has been set.
pub fn builtin_option(name: &str, config: &Config) -> Option<String> {
    let on = |b: bool| if b { "on" } else { "off" }.to_string();
    match name {
        "prefix" => Some(config.prefix.clone()),
        "base-index" | "pane-base-index" => Some("0".to_string()),
        "default-terminal" => Some("xterm-256color".to_string()),
        "default-shell" => std::env::var("SHELL").ok().filter(|s| !s.is_empty()),
        "allow-passthrough" => Some(on(config.allow_passthrough)),
        "remain-on-exit" => Some(on(config.remain_on_exit)),
        _ => None,
    }
}

/// An option value as `show-options` prints it: bare when plain, double-quoted otherwise.
pub fn quote_option_value(value: &str) -> String {
    let plain = !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_whitespace() || "\"'\\$;#~".contains(c));
    if plain {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if matches!(c, '"' | '\\' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The first characters of a buffer for `list-buffers`, control characters shown as escapes.
pub fn buffer_sample(data: &str) -> String {
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

fn buffer_vars(buffer: &Buffer) -> Vars {
    Vars::from([
        ("buffer_name", buffer.name.clone()),
        ("buffer_size", buffer.data.len().to_string()),
        ("buffer_sample", buffer_sample(&buffer.data)),
    ])
}

fn operand(inv: &Invocation, i: usize, what: &str) -> Result<String, String> {
    inv.operands
        .get(i)
        .cloned()
        .ok_or_else(|| format!("too few arguments: {what} required"))
}

fn read_input(path: &str) -> Result<String, String> {
    let mut data = String::new();
    if path == "-" {
        std::io::stdin()
            .read_to_string(&mut data)
            .map_err(|e| e.to_string())?;
    } else {
        data = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(data)
}

impl Shim {
    pub(super) async fn set_option(&mut self, inv: &Invocation) -> Result<String, String> {
        let name = operand(inv, 0, "option")?;
        if inv.has('u') || inv.has('U') {
            self.client
                .set_var(VarKind::Option, &name, None)
                .await?;
            return Ok(String::new());
        }
        let current = self
            .option_value(&name)
            .await?
            .filter(|_| inv.has('a') || inv.has('o'));
        if inv.has('o') && current.is_some() && !inv.has('q') {
            return Err(format!("already set: {name}"));
        }
        let mut value = inv.operands.get(1).cloned().unwrap_or_default();
        if inv.has('F') {
            value = expand(&value, &Vars::new());
        }
        if inv.has('a') {
            value = format!("{}{value}", current.unwrap_or_default());
        }
        self.client
            .set_var(VarKind::Option, &name, Some(&value))
            .await?;
        Ok(String::new())
    }

    async fn option_value(&mut self, name: &str) -> Result<Option<String>, String> {
        let stored = self.client.get_vars(VarKind::Option).await?;
        Ok(stored
            .into_iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
            .or_else(|| builtin_option(name, &Config::load_or_default())))
    }

    pub(super) async fn show_options(&mut self, inv: &Invocation) -> Result<String, String> {
        let value_only = inv.has('v');
        let line = |name: &str, value: &str| {
            if value_only {
                format!("{value}\n")
            } else {
                format!("{name} {}\n", quote_option_value(value))
            }
        };
        let Some(name) = inv.operands.first() else {
            let stored = self.client.get_vars(VarKind::Option).await?;
            return Ok(stored.iter().map(|(k, v)| line(k, v)).collect());
        };
        match self.option_value(name).await? {
            Some(value) => Ok(line(name, &value)),
            None if inv.has('q') => Ok(String::new()),
            None => Err(format!("invalid option: {name}")),
        }
    }

    pub(super) async fn set_environment(&mut self, inv: &Invocation) -> Result<String, String> {
        let name = operand(inv, 0, "name")?;
        let value = if inv.has('u') || inv.has('r') {
            None
        } else {
            Some(inv.operands.get(1).cloned().unwrap_or_default())
        };
        self.client
            .set_var(VarKind::Environment, &name, value.as_deref())
            .await?;
        Ok(String::new())
    }

    pub(super) async fn show_environment(&mut self, inv: &Invocation) -> Result<String, String> {
        let vars = self.client.get_vars(VarKind::Environment).await?;
        match inv.operands.first() {
            Some(name) => vars
                .iter()
                .find(|(k, _)| k == name)
                .map(|(k, v)| format!("{k}={v}\n"))
                .ok_or_else(|| format!("unknown variable: {name}")),
            None => Ok(vars.iter().map(|(k, v)| format!("{k}={v}\n")).collect()),
        }
    }

    pub(super) async fn set_buffer(&mut self, inv: &Invocation) -> Result<String, String> {
        let name = inv.value('b');
        if let Some(new_name) = inv.value('n') {
            let buffer = self.client.get_buffer(name).await?;
            self.client.delete_buffer(Some(&buffer.name)).await?;
            self.client.set_buffer(Some(new_name), &buffer.data).await?;
            return Ok(String::new());
        }
        let mut data = operand(inv, 0, "data")?;
        if inv.has('a')
            && let Ok(old) = self.client.get_buffer(name).await
        {
            data = format!("{}{data}", old.data);
        }
        self.client.set_buffer(name, &data).await?;
        Ok(String::new())
    }

    pub(super) async fn show_buffer(&mut self, inv: &Invocation) -> Result<String, String> {
        Ok(self.client.get_buffer(inv.value('b')).await?.data)
    }

    pub(super) async fn list_buffers(&mut self, inv: &Invocation) -> Result<String, String> {
        let fmt = inv
            .value('F')
            .unwrap_or("#{buffer_name}: #{buffer_size} bytes: \"#{buffer_sample}\"");
        let buffers = self.client.list_buffers().await?;
        Ok(buffers
            .iter()
            .map(|b| format!("{}\n", expand(fmt, &buffer_vars(b))))
            .collect())
    }

    pub(super) async fn delete_buffer(&mut self, inv: &Invocation) -> Result<String, String> {
        self.client.delete_buffer(inv.value('b')).await?;
        Ok(String::new())
    }

    pub(super) async fn load_buffer(&mut self, inv: &Invocation) -> Result<String, String> {
        let data = read_input(&operand(inv, 0, "path")?)?;
        self.client.set_buffer(inv.value('b'), &data).await?;
        Ok(String::new())
    }

    pub(super) async fn save_buffer(&mut self, inv: &Invocation) -> Result<String, String> {
        let path = operand(inv, 0, "path")?;
        let data = self.client.get_buffer(inv.value('b')).await?.data;
        if path == "-" {
            return Ok(data);
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(inv.has('a'))
            .truncate(!inv.has('a'))
            .open(&path)
            .map_err(|e| format!("{path}: {e}"))?;
        file.write_all(data.as_bytes())
            .map_err(|e| format!("{path}: {e}"))?;
        Ok(String::new())
    }

    /// `paste-buffer`: line feeds become the separator (carriage return unless `-s`), unless `-r`;
    /// `-p` pastes bracketed when the pane asked for it.
    pub(super) async fn paste_buffer(&mut self, inv: &Invocation) -> Result<String, String> {
        let node = self.resolve(inv.value('t')).await?;
        let buffer = self.client.get_buffer(inv.value('b')).await?;
        if inv.has('p') {
            self.client.paste(&node.name, &buffer.data, false).await?;
        } else {
            let text = if inv.has('r') {
                buffer.data.clone()
            } else {
                buffer.data.replace('\n', inv.value('s').unwrap_or("\r"))
            };
            self.client.send_keys(&node.name, vec![text], true).await?;
        }
        if inv.has('d') {
            self.client.delete_buffer(Some(&buffer.name)).await?;
        }
        Ok(String::new())
    }

    pub(super) async fn wait_for(&mut self, inv: &Invocation) -> Result<String, String> {
        let channel = operand(inv, 0, "channel")?;
        let action = match (inv.has('S'), inv.has('L'), inv.has('U')) {
            (true, _, _) => ChannelAction::Signal,
            (_, true, _) => ChannelAction::Lock,
            (_, _, true) => ChannelAction::Unlock,
            _ => ChannelAction::Wait,
        };
        self.client.wait_for(&channel, action).await?;
        Ok(String::new())
    }

    pub(super) async fn detach_client(&mut self) -> Result<String, String> {
        self.client.client_command("detach-client").await?;
        Ok(String::new())
    }

    /// `switch-client -t session`, or `-n` / `-p` for the next / previous session.
    pub(super) async fn switch_client(&mut self, inv: &Invocation) -> Result<String, String> {
        let line = if inv.has('n') {
            "next-session".to_string()
        } else if inv.has('p') {
            "prev-session".to_string()
        } else {
            let target = inv
                .value('t')
                .ok_or_else(|| "no target session".to_string())?;
            let session = target.split(':').next().unwrap_or(target);
            format!("switch-client '{}'", session.replace('\'', ""))
        };
        self.client.client_command(&line).await?;
        Ok(String::new())
    }
}

impl Shim {
    /// Index of the window the current pane is in.
    async fn current_window(&mut self) -> Result<usize, String> {
        Ok(self.resolve(None).await?.window.unwrap_or(0))
    }

    async fn window_arg(&mut self, target: Option<&str>) -> Result<usize, String> {
        match target {
            Some(t) => window_key(t).ok_or_else(|| format!("can't find window: {t}")),
            None => self.current_window().await,
        }
    }

    pub(super) async fn join_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        let src = self.resolve(inv.value('s')).await?;
        let dst = self.resolve(inv.value('t')).await?;
        self.client
            .join_pane(&src.name, &dst.name, inv.has('h'))
            .await?;
        Ok(String::new())
    }

    pub(super) async fn swap_window(&mut self, inv: &Invocation) -> Result<String, String> {
        let a = self.window_arg(inv.value('s')).await?;
        let b = self.window_arg(inv.value('t')).await?;
        self.client.swap_window(a, b).await?;
        Ok(String::new())
    }

    /// `move-window -s src -t dst`: windows are positions here, so the window moves to index `dst`.
    pub(super) async fn move_window(&mut self, inv: &Invocation) -> Result<String, String> {
        if inv.has('r') {
            return Ok(String::new());
        }
        let from = self.window_arg(inv.value('s')).await?;
        let to = inv
            .value('t')
            .and_then(window_key)
            .ok_or_else(|| "no target window".to_string())?;
        self.client.move_window(from, to).await?;
        Ok(String::new())
    }

    /// `pipe-pane [-o] [command]`: output of the pane goes to the command; without one, piping stops.
    pub(super) async fn pipe_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        if inv.has('I') {
            return Err("pipe-pane -I is not supported".to_string());
        }
        let node = self.resolve(inv.value('t')).await?;
        let command = (!inv.operands.is_empty()).then(|| inv.operand_text());
        self.client
            .pipe_pane(&node.name, command.as_deref(), inv.has('o'))
            .await?;
        Ok(String::new())
    }
}
