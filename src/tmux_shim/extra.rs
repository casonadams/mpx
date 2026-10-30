//! tmux commands backed by session state kept in the daemon: options, environment, paste
//! buffers, `wait-for`, and the commands that reach attached terminals.
use std::io::{Read, Write};

use super::args::Invocation;
use super::exec::Shim;
use super::format::{Vars, expand};
use super::shell::command_line;
use crate::config::{Config, Justify, ModeKeys, StatusPosition, parse_switch};
use crate::protocol::{Buffer, ChannelAction, VarKind};

/// Longest buffer prefix `list-buffers` shows.
const SAMPLE_CHARS: usize = 50;

/// tmux's default window tab format, which mpx's built-in tabs match when no format is set.
const WINDOW_STATUS_FORMAT: &str = "#I:#W#{window_flags}";

/// Value of a tmux option that mpx has a real equivalent for, or the same fixed answer as tmux.
/// Anything else is only known once it has been set.
pub fn builtin_option(name: &str, config: &Config) -> Option<String> {
    let on = |b: bool| if b { "on" } else { "off" }.to_string();
    match name {
        "prefix" => Some(config.prefix.clone()),
        "base-index" => Some(config.base_index.to_string()),
        "pane-base-index" => Some(config.pane_base_index.to_string()),
        "default-terminal" => Some(config.default_terminal.clone()),
        "default-shell" => config
            .default_shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok().filter(|s| !s.is_empty())),
        "default-command" => Some(config.default_command.clone().unwrap_or_default()),
        "allow-passthrough" => Some(config.allow_passthrough.as_str().to_string()),
        "remain-on-exit" => Some(config.remain_on_exit.as_str().to_string()),
        "history-limit" => Some(config.scrollback_lines.to_string()),
        "display-panes-time" => Some(config.display_panes_time_ms.to_string()),
        "automatic-rename" => Some(on(config.automatic_rename)),
        "monitor-activity" => Some(on(config.monitor_activity)),
        "monitor-bell" => Some(on(config.monitor_bell)),
        "monitor-silence" => Some("0".to_string()),
        "renumber-windows" => Some("on".to_string()),
        "mouse" => Some(on(config.mouse)),
        "status" => Some(on(config.status)),
        "status-position" => Some(
            match config.status_position {
                StatusPosition::Top => "top",
                StatusPosition::Bottom => "bottom",
            }
            .to_string(),
        ),
        "status-interval" => Some(config.status_interval.to_string()),
        "status-left" => Some(
            config
                .status_left
                .clone()
                .unwrap_or_else(|| "[#{session_name}] ".to_string()),
        ),
        "status-right" => Some(
            config
                .status_right
                .clone()
                .unwrap_or_else(|| "\"#{=21:pane_title}\" %H:%M %d-%b-%y".to_string()),
        ),
        "status-left-length" => Some(config.status_left_length.to_string()),
        "status-right-length" => Some(config.status_right_length.to_string()),
        "window-status-format" => Some(
            config
                .window_status_format
                .clone()
                .unwrap_or_else(|| WINDOW_STATUS_FORMAT.to_string()),
        ),
        "window-status-current-format" => Some(
            config
                .window_status_current_format
                .clone()
                .or_else(|| config.window_status_format.clone())
                .unwrap_or_else(|| WINDOW_STATUS_FORMAT.to_string()),
        ),
        "window-status-separator" => Some(
            config
                .window_status_separator
                .clone()
                .unwrap_or_else(|| " ".to_string()),
        ),
        "status-justify" => Some(
            match config.status_justify {
                Justify::Left => "left",
                Justify::Centre => "centre",
                Justify::Right => "right",
            }
            .to_string(),
        ),
        "status-style" => Some(format!(
            "bg={},fg={}",
            config.style.status_bg, config.style.status_fg
        )),
        "status-bg" => Some(config.style.status_bg.clone()),
        "status-fg" => Some(config.style.status_fg.clone()),
        "pane-border-style" => Some(format!("fg={}", config.style.border_inactive)),
        "pane-active-border-style" => Some(format!("fg={}", config.style.border_active)),
        "mode-keys" => Some(
            match config.mode_keys {
                ModeKeys::Vi => "vi",
                ModeKeys::Emacs => "emacs",
            }
            .to_string(),
        ),
        "display-time" => Some(config.display_time_ms.to_string()),
        "set-clipboard" => Some(if config.set_clipboard { "external" } else { "off" }.to_string()),
        "escape-time" => Some("500".to_string()),
        "focus-events" | "aggressive-resize" | "extended-keys" => Some("off".to_string()),
        "xterm-keys" => Some("on".to_string()),
        "status-keys" => Some("emacs".to_string()),
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
    /// `synchronize-panes` is a window option: it applies to the window `-t` names, else the
    /// active window.
    async fn set_synchronized(&mut self, inv: &Invocation) -> Result<String, String> {
        let window = match inv.value('t') {
            Some(target) => Some(self.window_of(target).await?),
            None => None,
        };
        let on = if inv.has('u') || inv.has('U') {
            false
        } else {
            let value = operand(inv, 1, "value")?;
            parse_switch(&value).map_err(|_| format!("invalid value: {value}"))?
        };
        self.client.set_synchronized(window, on).await?;
        Ok(String::new())
    }

    pub(super) async fn set_option(&mut self, inv: &Invocation) -> Result<String, String> {
        let name = operand(inv, 0, "option")?;
        if name == "synchronize-panes" {
            return self.set_synchronized(inv).await;
        }
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
        Config::default().apply_option(&name, &value)?;
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

    /// A numeric option's value: the session's, else the file's default, else 0.
    pub(super) async fn number_option(&mut self, name: &str) -> Result<usize, String> {
        Ok(self.option_value(name).await?.and_then(|v| v.trim().parse().ok()).unwrap_or(0))
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
            Some(t) => self.window_of(t).await,
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
        let to = match inv.value('t') {
            Some(t) => self.window_of(t).await?,
            None => return Err("no target window".to_string()),
        };
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

/// Key table named by `-n` / `-T`: `true` for root. Tables mpx has no equivalent for are refused.
fn root_table(inv: &Invocation) -> Result<bool, String> {
    match (inv.value('T'), inv.has('n')) {
        (Some("root"), _) | (None, true) => Ok(true),
        (Some("prefix"), _) | (None, false) => Ok(false),
        (Some(table), _) => Err(format!("key table '{table}' is not supported")),
    }
}

impl Shim {
    /// `bind-key [-n] [-T table] key command [arguments]`: recorded in the session, so every
    /// attached terminal, and any that attaches later, has the binding.
    pub(super) async fn bind_key(&mut self, inv: &Invocation) -> Result<String, String> {
        let root = root_table(inv)?;
        let (key, command) = inv
            .operands
            .split_first()
            .filter(|(_, command)| !command.is_empty())
            .ok_or_else(|| "too few arguments: key and command required".to_string())?;
        let line = command_line(command);
        crate::tui::command::parse(&line).map_err(|e| format!("bind {key}: {e}"))?;
        crate::tui::keymap::resolve(&Config::default())?.bind(root, key, &line)?;
        self.client
            .set_var(VarKind::Binding, &binding_name(root, key), Some(&line))
            .await?;
        Ok(String::new())
    }

    /// `unbind-key [-a] [-n] [-T table] key`. `-a` removes every binding of the table.
    pub(super) async fn unbind_key(&mut self, inv: &Invocation) -> Result<String, String> {
        let root = root_table(inv)?;
        if inv.has('a') {
            let keymap = self.effective_keymap().await?;
            for (table_root, key, _) in keymap.entries() {
                if table_root == root {
                    self.client
                        .set_var(VarKind::Binding, &binding_name(root, &key), Some(""))
                        .await?;
                }
            }
            return Ok(String::new());
        }
        let key = inv
            .operands
            .first()
            .ok_or_else(|| "too few arguments: key required".to_string())?;
        crate::tui::keymap::resolve(&Config::default())?.bind(root, key, "")?;
        self.client
            .set_var(VarKind::Binding, &binding_name(root, key), Some(""))
            .await?;
        Ok(String::new())
    }

    /// The keymap a terminal of this session has: the config file, the session's options, then its
    /// bindings.
    async fn effective_keymap(&mut self) -> Result<crate::tui::keymap::Keymap, String> {
        let mut config = Config::load_or_default();
        for (name, value) in self.client.get_vars(VarKind::Option).await? {
            let _ = config.apply_option(&name, &value);
        }
        let mut keymap = crate::tui::keymap::resolve(&config)?;
        for (name, line) in self.client.get_vars(VarKind::Binding).await? {
            let (table, key) = name.split_once(':').unwrap_or(("prefix", &name));
            keymap.bind(table == "root", key, &line)?;
        }
        Ok(keymap)
    }

    /// `list-keys [-T table]`, one `bind-key` line per binding.
    pub(super) async fn list_keys(&mut self, inv: &Invocation) -> Result<String, String> {
        let only = inv.value('T');
        let keymap = self.effective_keymap().await?;
        Ok(keymap.entries()
            .into_iter()
            .map(|(root, key, line)| {
                let table = if root { "root" } else { "prefix" };
                (table, key, line)
            })
            .filter(|(table, ..)| only.is_none_or(|t| t == *table))
            .map(|(table, key, line)| format!("bind-key -T {table} {} {line}\n", quote_option_value(&key)))
            .collect())
    }

    /// `set-hook [-gu] [-a] hook command`: see `daemon::hooks::HOOK_NAMES`.
    pub(super) async fn set_hook(&mut self, inv: &Invocation) -> Result<String, String> {
        let name = operand(inv, 0, "hook")?;
        if !crate::daemon::hooks::HOOK_NAMES.contains(&name.as_str()) {
            return Err(format!("unknown hook: {name}"));
        }
        if inv.has('u') || inv.has('R') {
            self.client.set_var(VarKind::Hook, &name, None).await?;
            return Ok(String::new());
        }
        let mut line = command_line(&inv.operands[1..]);
        if line.is_empty() {
            return Err("too few arguments: command required".to_string());
        }
        if inv.has('a') {
            let old = self.client.get_vars(VarKind::Hook).await?;
            if let Some((_, old)) = old.into_iter().find(|(k, _)| *k == name) {
                line = format!("{old} ; {line}");
            }
        }
        crate::tui::command::parse(&line).map_err(|e| format!("hook {name}: {e}"))?;
        self.client.set_var(VarKind::Hook, &name, Some(&line)).await?;
        Ok(String::new())
    }

    pub(super) async fn show_hooks(&mut self, inv: &Invocation) -> Result<String, String> {
        let hooks = self.client.get_vars(VarKind::Hook).await?;
        let wanted = inv.operands.first();
        Ok(hooks
            .iter()
            .filter(|(name, _)| wanted.is_none_or(|w| w == name))
            .map(|(name, line)| format!("{name} {}\n", quote_option_value(line)))
            .collect())
    }
}

fn binding_name(root: bool, key: &str) -> String {
    format!("{}:{key}", if root { "root" } else { "prefix" })
}
