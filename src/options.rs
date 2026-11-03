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
    ($name:literal, $($field:ident).+, $show:expr, $parse:expr) => {
        Opt {
            name: $name,
            get: |c| $show(&c.$($field).+),
            set: |c, v| {
                c.$($field).+ = $parse(v)?;
                Ok(())
            },
            reset: |c| c.$($field).+ = Config::default().$($field).+,
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

/// A free-text option: shown quoted, set as typed.
macro_rules! text_option {
    ($name:literal, $($field:ident).+) => {
        option!(
            $name,
            $($field).+,
            |v: &String| quoted(v.clone()),
            |v: &str| Ok::<_, String>(v.to_string())
        )
    };
}

/// Session behaviour: input, mouse, prefix, timing and the search wrap.
static GENERAL_OPTIONS: &[Opt] = &[
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
    option!(
        "mode-keys",
        mode_keys,
        |v: &crate::config::ModeKeys| v.as_str().to_string(),
        |v: &str| v.parse()
    ),
    option!("mouse", mouse, |v: &bool| on_off(*v), parse_switch),
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
    option!(
        "which-key-delay",
        which_key_delay_ms,
        |v: &u64| v.to_string(),
        number::<u64>
    ),
    option!(
        "wrap-search",
        wrap_search,
        |v: &bool| on_off(*v),
        parse_switch
    ),
];

/// Pane placement and border styles.
static PANE_OPTIONS: &[Opt] = &[
    option!(
        "pane-base-index",
        window.pane_base_index,
        |v: &usize| v.to_string(),
        number::<usize>
    ),
    style_option!("pane-active-border-style", pane_active_border_style),
    style_option!("pane-border-style", pane_border_style),
];

/// Terminal emulation: bell, titles, key encoding, focus and visual alerts.
static TERMINAL_OPTIONS: &[Opt] = &[
    option!(
        "bell-action",
        terminal.bell_action,
        |v: &crate::config::BellAction| v.as_str().to_string(),
        |v: &str| v.parse()
    ),
    option!(
        "extended-keys",
        terminal.extended_keys,
        |v: &crate::config::ExtendedKeys| v.as_str().to_string(),
        |v: &str| v.parse()
    ),
    option!(
        "focus-events",
        terminal.focus_events,
        |v: &bool| on_off(*v),
        parse_switch
    ),
    option!(
        "set-titles",
        terminal.set_titles,
        |v: &bool| on_off(*v),
        parse_switch
    ),
    text_option!("set-titles-string", terminal.set_titles_string),
    option!(
        "visual-activity",
        terminal.visual_activity,
        |v: &bool| on_off(*v),
        parse_switch
    ),
    option!(
        "visual-bell",
        terminal.visual_bell,
        |v: &bool| on_off(*v),
        parse_switch
    ),
    option!(
        "xterm-keys",
        terminal.xterm_keys,
        |v: &bool| on_off(*v),
        parse_switch
    ),
];

/// The status line and the messages shown on it.
static STATUS_OPTIONS: &[Opt] = &[
    style_option!("message-command-style", message_command_style),
    style_option!("message-style", message_style),
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
    text_option!("status-left", status_left),
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
    text_option!("status-right", status_right),
    option!(
        "status-right-length",
        status_right_length,
        |v: &usize| v.to_string(),
        number::<usize>
    ),
    style_option!("status-style", status_style),
];

/// Window tabs in the status line: their formats and styles.
static WINDOW_OPTIONS: &[Opt] = &[
    style_option!("window-status-activity-style", window_status_activity_style),
    style_option!("window-status-bell-style", window_status_bell_style),
    text_option!("window-status-current-format", window_status_current_format),
    style_option!("window-status-current-style", window_status_current_style),
    text_option!("window-status-format", window_status_format),
    style_option!("window-status-last-style", window_status_last_style),
    text_option!("window-status-separator", window_status_separator),
    style_option!("window-status-style", window_status_style),
];

/// Every table of settable options.
fn tables() -> impl Iterator<Item = &'static Opt> {
    [
        GENERAL_OPTIONS,
        PANE_OPTIONS,
        TERMINAL_OPTIONS,
        STATUS_OPTIONS,
        WINDOW_OPTIONS,
    ]
    .into_iter()
    .flatten()
}

fn find(name: &str) -> Result<&'static Opt, String> {
    if let Some(opt) = tables().find(|o| o.name == name) {
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
    tables().any(|o| o.name == name) || DAEMON_OPTIONS.contains(&name)
}

/// The value of option `name` as `show-options` prints it.
pub fn get(config: &Config, name: &str) -> Result<String, String> {
    Ok((find(name)?.get)(config))
}

/// Every settable option as `(name, value)`, sorted by name.
pub fn list(config: &Config) -> Vec<(&'static str, String)> {
    let mut all: Vec<_> = tables().map(|o| (o.name, (o.get)(config))).collect();
    all.sort_by_key(|(name, _)| *name);
    all
}
