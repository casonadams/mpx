//! tmux option names over the config fields the TUI owns: `set-option` and `show-options`.
//!
//! Options the daemon reads (`remain-on-exit`, `history-limit`, ...) are not settable at run time;
//! they come from the config file and `reload-config`.
use crate::config::{Config, parse_status, parse_switch};

/// One option: how it is shown, set and reset.
struct Opt {
    name: &'static str,
    get: fn(&Config) -> String,
    set: fn(&mut Config, &str) -> Result<(), String>,
    reset: fn(&mut Config),
}

/// Options whose value the daemon reads when it starts or reloads its config.
const DAEMON_OPTIONS: &[&str] = &[
    "automatic-rename",
    "default-command",
    "default-shell",
    "default-terminal",
    "history-limit",
    "monitor-activity",
    "monitor-bell",
    "monitor-silence",
    "renumber-windows",
    "remain-on-exit",
    "restore",
];

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
}

fn number<T: std::str::FromStr>(value: &str) -> Result<T, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("not a number: {value}"))
}

fn quoted(value: String) -> String {
    let plain =
        !value.is_empty() && !value.contains(|c: char| c.is_whitespace() || "\"'#;".contains(c));
    if plain {
        value
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

macro_rules! option {
    ($name:literal, $field:ident, $show:expr, $parse:expr) => {
        Opt {
            name: $name,
            get: |c| $show(&c.$field),
            set: |c, v| {
                c.$field = $parse(v)?;
                Ok(())
            },
            reset: |c| c.$field = Config::default().$field,
        }
    };
}

macro_rules! style_option {
    ($name:literal, $field:ident) => {
        Opt {
            name: $name,
            get: |c| quoted(c.style.$field.clone()),
            set: |c, v| {
                crate::tui::style::parse(v)?;
                c.style.$field = v.to_string();
                Ok(())
            },
            reset: |c| c.style.$field = Config::default().style.$field,
        }
    };
}

static OPTIONS: &[Opt] = &[
    option!(
        "allow-passthrough",
        allow_passthrough,
        |v: &crate::config::Passthrough| v.as_str().to_string(),
        |v: &str| v.parse()
    ),
    option!(
        "base-index",
        base_index,
        |v: &usize| v.to_string(),
        number::<usize>
    ),
    option!(
        "display-panes-time",
        display_panes_time_ms,
        |v: &u64| v.to_string(),
        number::<u64>
    ),
    option!(
        "display-time",
        display_time_ms,
        |v: &u64| v.to_string(),
        number::<u64>
    ),
    style_option!("message-command-style", message_command_style),
    style_option!("message-style", message_style),
    option!(
        "mode-keys",
        mode_keys,
        |v: &crate::config::ModeKeys| v.as_str().to_string(),
        |v: &str| v.parse()
    ),
    option!("mouse", mouse, |v: &bool| on_off(*v), parse_switch),
    style_option!("pane-active-border-style", pane_active_border_style),
    option!(
        "pane-base-index",
        pane_base_index,
        |v: &usize| v.to_string(),
        number::<usize>
    ),
    style_option!("pane-border-style", pane_border_style),
    option!(
        "prefix",
        prefix,
        |v: &String| quoted(v.clone()),
        |v: &str| Ok::<_, String>(v.trim().to_string())
    ),
    option!(
        "set-clipboard",
        set_clipboard,
        |v: &bool| on_off(*v),
        parse_switch
    ),
    Opt {
        name: "status",
        get: |c| {
            if !c.status {
                "off".to_string()
            } else if c.status_lines > 1 {
                c.status_lines.to_string()
            } else {
                "on".to_string()
            }
        },
        set: |c, v| {
            match parse_status(v)? {
                None => c.status = false,
                Some(lines) => {
                    c.status = true;
                    c.status_lines = lines;
                }
            }
            Ok(())
        },
        reset: |c| {
            c.status = Config::default().status;
            c.status_lines = Config::default().status_lines;
        },
    },
    option!(
        "status-interval",
        status_interval,
        |v: &u64| v.to_string(),
        number::<u64>
    ),
    option!(
        "status-justify",
        status_justify,
        |v: &crate::config::Justify| v.as_str().to_string(),
        |v: &str| v.parse()
    ),
    option!(
        "status-left",
        status_left,
        |v: &String| quoted(v.clone()),
        |v: &str| Ok::<_, String>(v.to_string())
    ),
    option!(
        "status-left-length",
        status_left_length,
        |v: &usize| v.to_string(),
        number::<usize>
    ),
    option!(
        "status-position",
        status_position,
        |v: &crate::config::StatusPosition| v.as_str().to_string(),
        |v: &str| v.parse()
    ),
    option!(
        "status-right",
        status_right,
        |v: &String| quoted(v.clone()),
        |v: &str| Ok::<_, String>(v.to_string())
    ),
    option!(
        "status-right-length",
        status_right_length,
        |v: &usize| v.to_string(),
        number::<usize>
    ),
    style_option!("status-style", status_style),
    option!(
        "which-key-delay",
        which_key_delay_ms,
        |v: &u64| v.to_string(),
        number::<u64>
    ),
    style_option!("window-status-activity-style", window_status_activity_style),
    style_option!("window-status-bell-style", window_status_bell_style),
    option!(
        "window-status-current-format",
        window_status_current_format,
        |v: &String| quoted(v.clone()),
        |v: &str| Ok::<_, String>(v.to_string())
    ),
    style_option!("window-status-current-style", window_status_current_style),
    option!(
        "window-status-format",
        window_status_format,
        |v: &String| quoted(v.clone()),
        |v: &str| Ok::<_, String>(v.to_string())
    ),
    style_option!("window-status-last-style", window_status_last_style),
    option!(
        "window-status-separator",
        window_status_separator,
        |v: &String| quoted(v.clone()),
        |v: &str| Ok::<_, String>(v.to_string())
    ),
    style_option!("window-status-style", window_status_style),
];

fn find(name: &str) -> Result<&'static Opt, String> {
    if let Some(opt) = OPTIONS.iter().find(|o| o.name == name) {
        return Ok(opt);
    }
    if DAEMON_OPTIONS.contains(&name) {
        return Err(format!(
            "{name} is read by the daemon: set it in the config file and run reload-config"
        ));
    }
    Err(format!("unknown option: {name}"))
}

/// Sets option `name` to `value`; a bad value leaves `config` unchanged.
pub fn set(config: &mut Config, name: &str, value: &str) -> Result<(), String> {
    let opt = find(name)?;
    let mut next = config.clone();
    (opt.set)(&mut next, value).map_err(|e| format!("{name}: {e}"))?;
    *config = next;
    Ok(())
}

/// Restores option `name` to its default.
pub fn unset(config: &mut Config, name: &str) -> Result<(), String> {
    (find(name)?.reset)(config);
    Ok(())
}

/// Whether `name` is a tmux option mpx knows, settable here or read by the daemon.
pub fn is_known(name: &str) -> bool {
    OPTIONS.iter().any(|o| o.name == name) || DAEMON_OPTIONS.contains(&name)
}

/// The value of option `name` as `show-options` prints it.
pub fn get(config: &Config, name: &str) -> Result<String, String> {
    Ok((find(name)?.get)(config))
}

/// Every settable option as `(name, value)`, sorted by name.
pub fn list(config: &Config) -> Vec<(&'static str, String)> {
    OPTIONS.iter().map(|o| (o.name, (o.get)(config))).collect()
}
