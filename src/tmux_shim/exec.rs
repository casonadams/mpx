use std::future::Future;
use std::pin::Pin;

use regex::Regex;
use uuid::Uuid;

use super::args::{Invocation, is_noop, parse};
use super::format::{PaneView, expand, pane_vars, session_vars};
use super::shell;
use crate::client::Client;
use crate::ipc;
use crate::layout::{Rect, WindowTab};
use crate::protocol::{LayoutPreset, NewWindowOpts, NodeInfo, Side, SpawnOpts, SplitOpts};
use crate::store;

const VERSION: &str = "tmux 3.4\n";
const DEFAULT_VIEW: (u16, u16) = (80, 24);

/// Connection target derived from the environment of the process (inside an mpx pane or outside).
#[derive(Clone, Debug)]
pub struct EnvTarget {
    pub endpoint: String,
    pub session: String,
}

impl EnvTarget {
    pub fn from_env() -> Self {
        let endpoint = super::endpoint_from_env(
            std::env::var("TMUX").ok().as_deref(),
            std::env::var("MPX_ENDPOINT").ok().as_deref(),
        )
        .unwrap_or_else(|| {
            let session = std::env::var("MPX_SESSION")
                .unwrap_or_else(|_| crate::cli::DEFAULT_SESSION.to_string());
            ipc::default_endpoint(&session)
        });
        Self {
            session: super::session_of(&endpoint),
            endpoint,
        }
    }

    /// Directory of this session's socket, where its sibling sessions live.
    pub fn socket_dir(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.endpoint)
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(ipc::socket_dir)
    }
}

/// Multicall entry point. Returns the process exit code and prints tmux-style output.
pub async fn run(argv: Vec<String>) -> i32 {
    match execute(&argv, None).await {
        Ok(out) => {
            print!("{out}");
            0
        }
        Err(e) => {
            if !e.is_empty() {
                eprintln!("tmux: {e}");
            }
            1
        }
    }
}

type Run<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + 'a>>;

/// The server a command line runs against when it is not the environment's: the attached TUI and
/// the daemon's hooks run tmux commands against their own session.
pub type Target<'a> = Option<&'a EnvTarget>;

/// Runs a tmux command string (commands separated by `;`) against `target`.
pub async fn run_in(target: &EnvTarget, line: &str) -> Result<String, String> {
    run_command_string(line, Some(target)).await
}

/// Runs one tmux command line. Boxed because `if-shell`, `run-shell -C` and `source-file` run more
/// commands through it.
fn execute<'a>(argv: &'a [String], target: Target<'a>) -> Run<'a> {
    Box::pin(async move {
        let inv = parse(argv)?;
        let cmd = inv.command.as_str();
        if cmd == "-V" {
            return Ok(VERSION.to_string());
        }
        if is_noop(cmd) {
            return Ok(String::new());
        }
        match cmd {
            "run-shell" => return run_shell_command(&inv, target).await,
            "source-file" => return source_file(&inv, target).await,
            _ => {}
        }
        let env = match target {
            Some(env) => {
                let mut env = env.clone();
                env.retarget(inv.value('t'));
                env
            }
            None => EnvTarget::for_invocation(&inv),
        };
        match cmd {
            "list-sessions" => list_sessions(&inv, &env.socket_dir()),
            "has-session" => has_session(&inv, &env).await,
            "attach-session" => attach_session(&inv, &env, None).await,
            "new-session" => new_session(&inv, &env).await,
            "if-shell" => if_shell(&inv, &env, target).await,
            "kill-server" => kill_server(&env).await,
            _ => {
                let client = Client::connect(&env.endpoint)
                    .await
                    .map_err(|_| format!("no server running on {}", env.endpoint))?;
                let mut shim = Shim {
                    client,
                    session: env.session,
                };
                shim.pane_command(&inv).await
            }
        }
    })
}

/// Attaches the terminal UI to `session`, else the session `-t` names, else the current one.
async fn attach_session(
    inv: &Invocation,
    env: &EnvTarget,
    session: Option<&str>,
) -> Result<String, String> {
    let name = session
        .or_else(|| inv.value('t').map(|t| t.split(':').next().unwrap_or(t)))
        .unwrap_or(&env.session)
        .to_string();
    let endpoint = if name == env.session {
        env.endpoint.clone()
    } else {
        ipc::endpoint_in(&env.socket_dir(), &name)
    };
    let state_dir = EnvTarget::state_root(inv).map(|root| root.join(store::component(&name)));
    let code = crate::cli::run_cli(crate::cli::Cli {
        session: Some(name),
        endpoint: Some(endpoint),
        config: None,
        state_dir,
        no_restore: false,
        idempotency_key: None,
        command: Some(crate::cli::Commands::Attach {
            target: None,
            force: inv.has('d'),
        }),
    })
    .await?;
    if code == 0 {
        Ok(String::new())
    } else {
        Err(String::new())
    }
}

/// `new-session`: a fresh session named by `-s`, else the next free number, with one window
/// running the command. `-A` attaches to an existing session of that name instead of failing.
async fn new_session(inv: &Invocation, env: &EnvTarget) -> Result<String, String> {
    let dir = env.socket_dir();
    let name = match inv.value('s') {
        Some(name) => name.to_string(),
        None => crate::cli::next_free_session(
            ipc::list_sessions(&dir).iter().map(|s| s.name.as_str()),
        ),
    };
    let endpoint = ipc::endpoint_in(&dir, &name);
    let running = match Client::connect(&endpoint).await {
        Ok(mut client) => client.ping().await.is_ok(),
        Err(_) => false,
    };
    if running && !inv.has('A') {
        return Err(format!("duplicate session: {name}"));
    }
    let mut out = String::new();
    if !running {
        let state_dir = EnvTarget::state_root(inv).map(|root| root.join(store::component(&name)));
        let mut client = crate::cli::ensure_session(&name, &dir, state_dir, true).await?;
        client
            .new_window(NewWindowOpts {
                name: inv.value('n').map(str::to_string),
                spawn: spawn_opts(inv),
                focus: true,
            })
            .await?;
        if inv.has('P') {
            let fmt = inv.value('F').unwrap_or("#{session_name}:");
            out = format!("{}\n", expand(fmt, &session_vars(&name, 1, 1)));
        }
    }
    if inv.has('d') {
        return Ok(out);
    }
    attach_session(inv, env, Some(&name)).await
}

fn list_sessions(inv: &Invocation, socket_dir: &std::path::Path) -> Result<String, String> {
    let sessions = ipc::list_sessions(socket_dir);
    if sessions.is_empty() {
        return Err(format!("no server running on {}", socket_dir.display()));
    }
    let fmt = inv.value('F').unwrap_or("#{session_name}");
    Ok(sessions
        .iter()
        .map(|s| format!("{}\n", expand(fmt, &session_vars(&s.name, 0, 0))))
        .collect())
}

/// `kill-server`: stops every session of the server, as tmux does.
async fn kill_server(env: &EnvTarget) -> Result<String, String> {
    let sessions = ipc::list_sessions(&env.socket_dir());
    if sessions.is_empty() {
        return Err(format!("no server running on {}", env.endpoint));
    }
    for session in sessions {
        if let Ok(mut client) = Client::connect(&session.endpoint).await {
            let _ = client.shutdown().await;
        }
    }
    Ok(String::new())
}

async fn has_session(inv: &Invocation, env: &EnvTarget) -> Result<String, String> {
    let name = inv
        .value('t')
        .map(|t| t.split(':').next().unwrap_or(t))
        .unwrap_or(&env.session);
    let endpoint = if name == env.session {
        env.endpoint.clone()
    } else {
        ipc::endpoint_in(&env.socket_dir(), name)
    };
    let mut client = Client::connect(&endpoint).await?;
    client.ping().await?;
    Ok(String::new())
}

/// `run-shell`: the command's output is the output of this one; `-C` runs it as a tmux command.
async fn run_shell_command(inv: &Invocation, target: Target<'_>) -> Result<String, String> {
    let cmd = inv.operand_text();
    if inv.has('C') {
        return run_command_string(&cmd, target).await;
    }
    if let Some(delay) = inv.value('d').and_then(|d| d.parse::<f64>().ok()) {
        tokio::time::sleep(std::time::Duration::from_secs_f64(delay.max(0.0))).await;
    }
    if inv.has('b') {
        shell::spawn_shell(&cmd)?;
        return Ok(String::new());
    }
    let done = shell::run_shell(&cmd).await?;
    if done.code == 0 {
        return Ok(done.stdout);
    }
    print!("{}", done.stdout);
    Err(format!("'{cmd}' returned {}", done.code))
}

/// `if-shell`: runs the second operand when the first holds, else the optional third. The first is a
/// shell command that must exit 0, or with `-F` a format that must expand to something but `0`.
async fn if_shell(
    inv: &Invocation,
    env: &EnvTarget,
    target: Target<'_>,
) -> Result<String, String> {
    let cond = inv
        .operands
        .first()
        .ok_or_else(|| "too few arguments: condition required".to_string())?;
    let holds = if inv.has('F') {
        let expanded = match Client::connect(&env.endpoint).await {
            Ok(client) => {
                Shim {
                    client,
                    session: env.session.clone(),
                }
                .expand_for(inv.value('t'), cond)
                .await?
            }
            Err(_) => expand(cond, &super::format::Vars::new()),
        };
        shell::format_is_true(&expanded)
    } else {
        shell::run_shell(cond).await?.code == 0
    };
    match inv.operands.get(if holds { 1 } else { 2 }) {
        Some(line) => run_command_string(line, target).await,
        None => Ok(String::new()),
    }
}

/// Runs each command of a tmux command string in turn, stopping at the first error.
async fn run_command_string(line: &str, target: Target<'_>) -> Result<String, String> {
    let mut out = String::new();
    for words in shell::split_commands(line)? {
        out.push_str(&execute(&words, target).await?);
    }
    Ok(out)
}

/// Expands `fmt` for the current pane of `env`; without a server, with no variables.
async fn expand_format(env: &EnvTarget, fmt: &str) -> String {
    match Client::connect(&env.endpoint).await {
        Ok(client) => {
            let mut shim = Shim {
                client,
                session: env.session.clone(),
            };
            shim.expand_for(None, fmt).await.unwrap_or_default()
        }
        Err(_) => expand(fmt, &super::format::Vars::new()),
    }
}

/// One level of `%if` nesting in a config file.
struct Branch {
    /// Lines of this branch run (and the enclosing branches run too).
    live: bool,
    /// An earlier branch of this `%if` already ran, so `%elif` / `%else` must not.
    taken: bool,
    parent_live: bool,
}

/// What a config line is, with `%if` / `%elif` / `%else` / `%endif` resolved against `stack`.
/// Returns whether the line is a command to run.
async fn directive(line: &str, stack: &mut Vec<Branch>, env: &EnvTarget) -> Result<bool, String> {
    let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let rest = rest.trim();
    let parent_live = stack.last().is_none_or(|b| b.live);
    match word {
        "%if" => {
            let holds = parent_live && shell::format_is_true(&expand_format(env, rest).await);
            stack.push(Branch { live: holds, taken: holds, parent_live });
            Ok(false)
        }
        "%elif" => {
            let top = stack.last_mut().ok_or("%elif without %if")?;
            let holds = top.parent_live && !top.taken && shell::format_is_true(&expand_format(env, rest).await);
            top.live = holds;
            top.taken |= holds;
            Ok(false)
        }
        "%else" => {
            let top = stack.last_mut().ok_or("%else without %if")?;
            top.live = top.parent_live && !top.taken;
            top.taken = true;
            Ok(false)
        }
        "%endif" => {
            stack.pop().ok_or("%endif without %if")?;
            Ok(false)
        }
        _ => Ok(parent_live),
    }
}

/// `source-file`: runs each line of the files as a tmux command, honoring `%if` blocks. Errors
/// carry `path:line`. `~` starts a path in the home directory.
async fn source_file(inv: &Invocation, target: Target<'_>) -> Result<String, String> {
    let env = target.cloned().unwrap_or_else(|| EnvTarget::for_invocation(inv));
    let mut out = String::new();
    let mut errors = Vec::new();
    for path in &inv.operands {
        let path = expand_home(path);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) if inv.has('q') => continue,
            Err(e) => return Err(format!("{path}: {e}")),
        };
        let mut stack = Vec::new();
        for (number, line) in shell::config_lines(&text) {
            let run = match directive(&line, &mut stack, &env).await {
                Ok(run) => run,
                Err(e) => {
                    errors.push(format!("{path}:{number}: {e}"));
                    continue;
                }
            };
            if !run || inv.has('n') {
                continue;
            }
            match run_command_string(&line, target).await {
                Ok(text) => out.push_str(&text),
                Err(e) => errors.push(format!("{path}:{number}: {e}")),
            }
        }
        if !stack.is_empty() {
            errors.push(format!("{path}: %if without %endif"));
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors.join("\n"))
    }
}

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), crate::config::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest).to_string_lossy().into_owned(),
        _ => path.to_string(),
    }
}

/// Target key for a tmux target: strips `session:` and `window.` prefixes, keeping `%id` or a node name.
pub fn pane_key(target: &str) -> &str {
    let last = target.rsplit(':').next().unwrap_or(target);
    match last.rsplit_once('.') {
        Some((_, pane)) if pane.starts_with('%') => pane,
        _ => last,
    }
}

/// `split-window` stacks the panes (top / bottom) unless `-h` puts them side by side, as in tmux.
pub fn split_direction(inv: &Invocation) -> crate::layout::Direction {
    if inv.has('h') {
        crate::layout::Direction::Horizontal
    } else {
        crate::layout::Direction::Vertical
    }
}

/// Window index for a `session:window` or `@N` target.
pub fn window_key(target: &str) -> Option<usize> {
    let last = target.rsplit(':').next().unwrap_or(target);
    last.strip_prefix('@').unwrap_or(last).parse().ok()
}

impl Shim {
    /// Daemon index of the window a `-t` target names. A plain number is a tmux window number, counted
    /// from `base-index`; `@N` is the daemon's own index (what `#{window_id}` shows).
    pub(super) async fn window_of(&mut self, target: &str) -> Result<usize, String> {
        let not_found = || format!("can't find window: {target}");
        let number = window_key(target).ok_or_else(not_found)?;
        let last = target.rsplit(':').next().unwrap_or(target);
        if last.starts_with('@') {
            return Ok(number);
        }
        let base = self.number_option("base-index").await?;
        number.checked_sub(base).ok_or_else(not_found)
    }
}

fn matches_target(node: &NodeInfo, key: &str) -> bool {
    match key.strip_prefix('%') {
        Some(id) => id.parse::<u32>().is_ok_and(|id| id == node.id),
        None => node.name == key,
    }
}

/// Direction flag of a `-L/-R/-U/-D` option.
pub fn side_of(inv: &Invocation) -> Option<Side> {
    [
        ('L', Side::Left),
        ('R', Side::Right),
        ('U', Side::Up),
        ('D', Side::Down),
    ]
    .into_iter()
    .find(|(c, _)| inv.has(*c))
    .map(|(_, s)| s)
}

/// Pane adjacent to `from` on `side`, preferring the first overlapping pane in layout order.
pub fn neighbor(panes: &[(String, Rect, bool)], from: &str, side: Side) -> Option<String> {
    let f = rect_box(&panes.iter().find(|(n, _, _)| n == from)?.1);
    panes
        .iter()
        .filter(|(n, _, _)| n != from)
        .filter(|(_, r, _)| touches(&f, &rect_box(r), side))
        .map(|(n, _, _)| n.clone())
        .next()
}

/// Edges of a rectangle as `(x0, x1, y0, y1)` in signed cells.
type Box4 = (i64, i64, i64, i64);

fn rect_box(r: &Rect) -> Box4 {
    let (x0, y0) = (r.x as i64, r.y as i64);
    (x0, x0 + r.width as i64, y0, y0 + r.height as i64)
}

fn overlaps(a0: i64, a1: i64, b0: i64, b1: i64) -> bool {
    a0 < b1 && b0 < a1
}

fn touches(f: &Box4, r: &Box4, side: Side) -> bool {
    let (fx0, fx1, fy0, fy1) = *f;
    let (rx0, rx1, ry0, ry1) = *r;
    match side {
        Side::Left => rx1 == fx0 && overlaps(fy0, fy1, ry0, ry1),
        Side::Right => fx1 == rx0 && overlaps(fy0, fy1, ry0, ry1),
        Side::Up => ry1 == fy0 && overlaps(fx0, fx1, rx0, rx1),
        Side::Down => fy1 == ry0 && overlaps(fx0, fx1, rx0, rx1),
    }
}

fn parse_percent(text: &str) -> Option<u8> {
    text.trim_end_matches('%').parse().ok()
}

fn fresh_name() -> String {
    let id = Uuid::new_v4().simple().to_string();
    format!("pane-{}", &id[..8])
}

/// The program and arguments a trailing command runs: its operands through `sh -c`, as tmux does.
/// With none the list is empty, and the daemon starts the session's default command.
fn command_argv(inv: &Invocation) -> Vec<String> {
    if inv.operands.is_empty() {
        Vec::new()
    } else {
        vec!["sh".to_string(), "-c".to_string(), inv.operand_text()]
    }
}

/// Spawn options for a tmux pane: trailing operands run through `sh -c`, as tmux does.
pub fn spawn_opts(inv: &Invocation) -> SpawnOpts {
    let mut argv = command_argv(inv).into_iter();
    SpawnOpts {
        name: fresh_name(),
        command: argv.next().unwrap_or_default(),
        args: argv.collect(),
        pty: true,
        cwd: inv.value('c').map(str::to_string),
        env: Vec::new(),
        remain_on_exit: None,
        tmux_compat: true,
    }
}

fn terminal_view() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or(DEFAULT_VIEW)
}

pub(super) struct Shim {
    pub(super) client: Client,
    pub(super) session: String,
}

impl Shim {
    async fn layout(&mut self) -> Result<Vec<(String, Rect, bool)>, String> {
        let (w, h) = terminal_view();
        Ok(self.client.layout(w, h).await?.0)
    }

    async fn panes(&mut self) -> Result<Vec<PaneView>, String> {
        let nodes = self.client.list().await?;
        let (w, h) = terminal_view();
        let (rects, _, tabs) = self.client.layout(w, h).await?;
        let mut views: Vec<PaneView> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| pane_view(&self.session, i, n, &rects, &tabs))
            .collect();
        let counts: Vec<usize> = views
            .iter()
            .map(|v| views.iter().filter(|o| o.window == v.window).count())
            .collect();
        let mut next_in_window: std::collections::HashMap<usize, usize> = Default::default();
        for (view, count) in views.iter_mut().zip(counts) {
            view.window_panes = count;
            let slot = next_in_window.entry(view.window).or_default();
            view.index = *slot;
            *slot += 1;
        }
        let window_base = self.number_option("base-index").await?;
        let pane_base = self.number_option("pane-base-index").await?;
        for view in &mut views {
            view.window_base = window_base;
            view.pane_base = pane_base;
        }
        Ok(views)
    }

    pub(super) async fn resolve(&mut self, target: Option<&str>) -> Result<NodeInfo, String> {
        let nodes = self.client.list().await?;
        let key = match target {
            Some(t) => pane_key(t).to_string(),
            None => {
                let rects = self.layout().await?;
                rects
                    .iter()
                    .find(|r| r.2)
                    .map(|r| r.0.clone())
                    .or_else(|| nodes.first().map(|n| n.name.clone()))
                    .ok_or_else(|| "no panes".to_string())?
            }
        };
        nodes
            .into_iter()
            .find(|n| matches_target(n, &key))
            .ok_or_else(|| format!("can't find pane: {key}"))
    }

    pub(super) async fn expand_for(
        &mut self,
        target: Option<&str>,
        fmt: &str,
    ) -> Result<String, String> {
        let node = self.resolve(target).await?;
        let view = self.pane_view_of(&node).await?;
        Ok(expand(fmt, &pane_vars(&view)))
    }

    async fn pane_view_of(&mut self, node: &NodeInfo) -> Result<PaneView, String> {
        let panes = self.panes().await?;
        Ok(panes
            .into_iter()
            .find(|p| p.id == node.id)
            .unwrap_or_else(|| pane_view(&self.session, 0, node, &[], &[])))
    }

    async fn pane_command(&mut self, inv: &Invocation) -> Result<String, String> {
        match inv.command.as_str() {
            "split-window" => self.split_window(inv).await,
            "new-window" => self.new_window(inv, true).await,
            "new-session" => self.new_window(inv, false).await,
            "send-keys" => self.send_keys(inv).await,
            "capture-pane" => self.capture_pane(inv).await,
            "list-panes" => self.list_panes(inv).await,
            "list-windows" => self.list_windows(inv).await,
            "display-message" => self.display_message(inv).await,
            "kill-pane" => self.kill_pane(inv).await,
            "kill-window" => self.kill_window(inv).await,
            "kill-session" => self.client.shutdown().await.map(|_| String::new()),
            "select-pane" => self.select_pane(inv).await,
            "select-window" => self.select_window(inv).await,
            "select-layout" => self.select_layout(inv).await,
            "resize-pane" => self.resize_pane(inv).await,
            "swap-pane" => self.swap_pane(inv).await,
            "break-pane" => self.break_pane(inv).await,
            "rotate-window" => self.rotate_window(inv).await,
            "respawn-pane" => self.respawn_pane(inv).await,
            "respawn-window" => self.respawn_window(inv).await,
            "find-window" => self.find_window(inv).await,
            "last-pane" => self.client.focus_last().await.map(|_| String::new()),
            "last-window" => self.client.last_window().await.map(|_| String::new()),
            "rename-window" => self.rename_window(inv).await,
            "rename-session" => self.rename_session(inv).await,
            "next-window" if inv.has('a') => self.step_alert(true).await,
            "previous-window" if inv.has('a') => self.step_alert(false).await,
            "next-window" => self.client.next_window().await.map(|_| String::new()),
            "previous-window" => self.client.prev_window().await.map(|_| String::new()),
            "detach-client" => self.detach_client().await,
            "switch-client" => self.switch_client(inv).await,
            "set-option" => self.set_option(inv).await,
            "show-options" => self.show_options(inv).await,
            "set-environment" => self.set_environment(inv).await,
            "show-environment" => self.show_environment(inv).await,
            "set-buffer" => self.set_buffer(inv).await,
            "show-buffer" => self.show_buffer(inv).await,
            "list-buffers" => self.list_buffers(inv).await,
            "delete-buffer" => self.delete_buffer(inv).await,
            "load-buffer" => self.load_buffer(inv).await,
            "save-buffer" => self.save_buffer(inv).await,
            "paste-buffer" => self.paste_buffer(inv).await,
            "wait-for" => self.wait_for(inv).await,
            "join-pane" => self.join_pane(inv).await,
            "swap-window" => self.swap_window(inv).await,
            "move-window" => self.move_window(inv).await,
            "pipe-pane" => self.pipe_pane(inv).await,
            "bind-key" => self.bind_key(inv).await,
            "unbind-key" => self.unbind_key(inv).await,
            "list-keys" => self.list_keys(inv).await,
            "set-hook" => self.set_hook(inv).await,
            "show-hooks" => self.show_hooks(inv).await,
            other => Err(format!("unknown command: {other}")),
        }
    }

    async fn print_pane(
        &mut self,
        node: &NodeInfo,
        inv: &Invocation,
        default: &str,
    ) -> Result<String, String> {
        if !inv.has('P') {
            return Ok(String::new());
        }
        let view = self.pane_view_of(node).await?;
        let fmt = inv.value('F').unwrap_or(default);
        Ok(format!("{}\n", expand(fmt, &pane_vars(&view))))
    }

    async fn split_window(&mut self, inv: &Invocation) -> Result<String, String> {
        let target = match inv.value('t') {
            Some(_) => Some(self.resolve(inv.value('t')).await?.name),
            None => None,
        };
        let direction = split_direction(inv);
        let opts = SplitOpts {
            target,
            spawn: spawn_opts(inv),
            direction,
            percent: inv.value('l').and_then(parse_percent),
            focus: !inv.has('d'),
        };
        let node = self.client.split(opts).await?;
        self.print_pane(&node, inv, "#{pane_id}").await
    }

    async fn new_window(&mut self, inv: &Invocation, is_window: bool) -> Result<String, String> {
        let name = inv.value('n').or(inv.value('s')).map(str::to_string);
        let focus = is_window && !inv.has('d');
        let node = self
            .client
            .new_window(NewWindowOpts {
                name,
                spawn: spawn_opts(inv),
                focus,
            })
            .await?;
        self.print_pane(&node, inv, "#{window_id}").await
    }

    async fn send_keys(&mut self, inv: &Invocation) -> Result<String, String> {
        let node = self.resolve(inv.value('t')).await?;
        if inv.has('l') {
            self.client
                .send_keys(&node.name, vec![inv.operands.concat()], true)
                .await?;
        } else {
            self.client
                .send_keys(&node.name, inv.operands.clone(), false)
                .await?;
        }
        Ok(String::new())
    }

    async fn capture_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        let node = self.resolve(inv.value('t')).await?;
        let start = inv.value('S').and_then(|s| s.parse::<i64>().ok());
        let text = match start {
            Some(s) if s < 0 => {
                self.client
                    .logs(&node.name, Some(s.unsigned_abs() as usize), true, None)
                    .await?
                    .0
            }
            _ => self.client.screen(&node.name).await?,
        };
        if !inv.has('p') {
            self.client.set_buffer(inv.value('b'), &text).await?;
            return Ok(String::new());
        }
        Ok(if text.ends_with('\n') {
            text
        } else {
            format!("{text}\n")
        })
    }

    async fn list_panes(&mut self, inv: &Invocation) -> Result<String, String> {
        let panes = self.panes().await?;
        let window = match (inv.has('a') || inv.has('s'), inv.value('t')) {
            (false, Some(t)) => Some(self.resolve(Some(t)).await?.window.unwrap_or(0)),
            _ => None,
        };
        let fmt = inv.value('F').unwrap_or("#{pane_id}");
        Ok(panes
            .iter()
            .filter(|p| window.is_none_or(|w| p.window == w))
            .map(|p| format!("{}\n", expand(fmt, &pane_vars(p))))
            .collect())
    }

    async fn list_windows(&mut self, inv: &Invocation) -> Result<String, String> {
        let panes = self.panes().await?;
        let fmt = inv.value('F').unwrap_or("#{window_index}");
        let mut windows: Vec<usize> = panes.iter().map(|p| p.window).collect();
        windows.sort_unstable();
        windows.dedup();
        Ok(windows
            .into_iter()
            .map(|w| {
                let first = panes
                    .iter()
                    .find(|p| p.window == w)
                    .cloned()
                    .unwrap_or_default();
                format!("{}\n", expand(fmt, &pane_vars(&first)))
            })
            .collect())
    }

    /// `next-window -a` / `previous-window -a`: the next window in that direction with an alert
    /// (activity or bell).
    async fn step_alert(&mut self, forward: bool) -> Result<String, String> {
        let (w, h) = terminal_view();
        let (_, _, tabs) = self.client.layout(w, h).await?;
        let count = tabs.len();
        let current = tabs.iter().position(|t| t.is_active).unwrap_or(0);
        let alert = (1..count)
            .map(|step| if forward { (current + step) % count } else { (current + count - step) % count })
            .find(|&i| tabs[i].has_activity || tabs[i].has_bell);
        match alert {
            Some(index) => self.client.select_window(index).await.map(|_| String::new()),
            None => Err(format!("no {} window with an alert", if forward { "next" } else { "previous" })),
        }
    }

    async fn display_message(&mut self, inv: &Invocation) -> Result<String, String> {
        if !inv.has('p') {
            return Ok(String::new());
        }
        let node = self.resolve(inv.value('t')).await?;
        let view = self.pane_view_of(&node).await?;
        let fmt = if inv.operands.is_empty() {
            "#{session_name}:#{window_index}.#{pane_index}".to_string()
        } else {
            inv.operand_text()
        };
        Ok(format!("{}\n", expand(&fmt, &pane_vars(&view))))
    }

    async fn kill_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        let node = self.resolve(inv.value('t')).await?;
        self.client.kill(&node.name).await.map(|_| String::new())
    }

    async fn kill_window(&mut self, inv: &Invocation) -> Result<String, String> {
        let index = match inv.value('t') {
            Some(t) => Some(self.window_of(t).await?),
            None => None,
        };
        self.client.kill_window(index).await.map(|_| String::new())
    }

    async fn select_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        if inv.has('M') {
            return self.client.mark_pane(None, true).await.map(|_| String::new());
        }
        if inv.has('m') {
            let target = match inv.value('t') {
                Some(t) => Some(self.resolve(Some(t)).await?.name),
                None => None,
            };
            return self
                .client
                .mark_pane(target.as_deref(), false)
                .await
                .map(|_| String::new());
        }
        if let Some(side) = side_of(inv) {
            let node = self.resolve(inv.value('t')).await?;
            let rects = self.layout().await?;
            let target = neighbor(&rects, &node.name, side)
                .ok_or_else(|| "no pane in that direction".to_string())?;
            return self.client.focus(&target).await.map(|_| String::new());
        }
        if inv.has('l') {
            return self.client.focus_last().await.map(|_| String::new());
        }
        if inv.value('T').is_some() {
            return Ok(String::new());
        }
        let node = self.resolve(inv.value('t')).await?;
        self.client.focus(&node.name).await.map(|_| String::new())
    }

    async fn select_window(&mut self, inv: &Invocation) -> Result<String, String> {
        if inv.has('n') {
            return self.client.next_window().await.map(|_| String::new());
        }
        if inv.has('p') {
            return self.client.prev_window().await.map(|_| String::new());
        }
        if inv.has('l') {
            return self.client.last_window().await.map(|_| String::new());
        }
        let target = inv
            .value('t')
            .ok_or_else(|| "window index required".to_string())?;
        let index = self.window_of(target).await?;
        self.client
            .select_window(index)
            .await
            .map(|_| String::new())
    }

    async fn select_layout(&mut self, inv: &Invocation) -> Result<String, String> {
        if inv.has('E') {
            return self.client.spread_layout().await.map(|_| String::new());
        }
        let preset = match inv.operands.first() {
            Some(name) => {
                Some(LayoutPreset::parse(name).ok_or_else(|| format!("unknown layout: {name}"))?)
            }
            None => None,
        };
        self.client
            .select_layout(preset)
            .await
            .map(|_| String::new())
    }

    async fn resize_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        let node = self.resolve(inv.value('t')).await?;
        if inv.value('x').is_some() || inv.value('y').is_some() {
            let view = self.pane_view_of(&node).await?;
            let cols = inv
                .value('x')
                .and_then(|v| v.parse().ok())
                .unwrap_or(view.width);
            let rows = inv
                .value('y')
                .and_then(|v| v.parse().ok())
                .unwrap_or(view.height);
            return self
                .client
                .resize_pane(&node.name, cols, rows)
                .await
                .map(|_| String::new());
        }
        let side = side_of(inv).ok_or_else(|| "resize direction required".to_string())?;
        let cells = inv
            .operands
            .first()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        self.client
            .resize_split(&node.name, side, cells, terminal_view())
            .await
            .map(|_| String::new())
    }

    async fn swap_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        let (w, h) = terminal_view();
        let (rects, _, tabs) = self.client.layout(w, h).await?;
        if inv.has('U') || inv.has('D') {
            let next = inv.has('D');
            let target = self.resolve(inv.value('t')).await?;
            let names: Vec<String> = rects.iter().map(|(n, ..)| n.clone()).collect();
            let Some(idx) = names.iter().position(|n| n == &target.name) else {
                return Err("pane not in current layout".to_string());
            };
            let other_idx = if next {
                (idx + 1) % names.len()
            } else if idx == 0 {
                names.len().saturating_sub(1)
            } else {
                idx - 1
            };
            if idx == other_idx {
                return Ok(String::new());
            }
            return self
                .client
                .swap_pane(&target.name, &names[other_idx])
                .await
                .map(|_| String::new());
        }
        let marked = tabs.iter().find_map(|t| t.marked_pane.as_deref());
        let src = match inv.value('s') {
            Some(s) => self.resolve(Some(s)).await?.name,
            None => match marked {
                Some(m) => m.to_string(),
                None => self.resolve(None).await?.name,
            },
        };
        let dst = match inv.value('t') {
            Some(t) => self.resolve(Some(t)).await?.name,
            None => self.resolve(None).await?.name,
        };
        if src == dst {
            return Ok(String::new());
        }
        self.client
            .swap_pane(&src, &dst)
            .await
            .map(|_| String::new())
    }

    /// The daemon acts on the active window only, so a `-t` naming another window is refused.
    async fn check_active_window(
        &mut self,
        target: Option<&str>,
        command: &str,
    ) -> Result<(), String> {
        let Some(target) = target else {
            return Ok(());
        };
        let index = self.window_of(target).await?;
        let (w, h) = terminal_view();
        let (_, _, tabs) = self.client.layout(w, h).await?;
        if tabs.iter().any(|t| t.is_active && t.index == index) {
            Ok(())
        } else {
            Err(format!("{command}: only the active window can be targeted"))
        }
    }

    async fn rotate_window(&mut self, inv: &Invocation) -> Result<String, String> {
        self.check_active_window(inv.value('t'), "rotate-window")
            .await?;
        self.client
            .rotate_window(inv.has('D'))
            .await
            .map(|_| String::new())
    }

    async fn respawn_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        let node = self.resolve(inv.value('t')).await?;
        self.respawn(&node.name, inv).await
    }

    /// Respawns the window's focused pane, as tmux does for the window's active pane.
    async fn respawn_window(&mut self, inv: &Invocation) -> Result<String, String> {
        self.check_active_window(inv.value('t'), "respawn-window")
            .await?;
        let (w, h) = terminal_view();
        let (rects, _, _) = self.client.layout(w, h).await?;
        let focused = rects
            .iter()
            .find(|(_, _, focused)| *focused)
            .map(|(name, ..)| name.clone())
            .ok_or_else(|| "no pane in the window".to_string())?;
        self.respawn(&focused, inv).await
    }

    /// Restarts the pane `name`; a trailing command replaces the pane's own.
    async fn respawn(&mut self, name: &str, inv: &Invocation) -> Result<String, String> {
        let command = (!inv.operands.is_empty()).then(|| command_argv(inv));
        let cwd = inv.value('c').map(str::to_string);
        self.client
            .respawn(name, inv.has('k'), cwd, command)
            .await
            .map(|_| String::new())
    }

    /// Lists the windows whose name, pane title or screen contains the match string (tmux
    /// `find-window`). Without `-C`, `-N` or `-T`, all three are searched.
    async fn find_window(&mut self, inv: &Invocation) -> Result<String, String> {
        let [needle] = inv.operands.as_slice() else {
            return Err("find-window: match-string required".to_string());
        };
        let text = if inv.has('r') {
            needle.clone()
        } else {
            regex::escape(needle)
        };
        let pattern = Regex::new(&text).map_err(|e| format!("invalid regex: {e}"))?;
        let any = !(inv.has('C') || inv.has('N') || inv.has('T'));
        let (content, names, titles) = (
            any || inv.has('C'),
            any || inv.has('N'),
            any || inv.has('T'),
        );
        let (w, h) = terminal_view();
        let panes = self.panes().await?;
        let mut windows: Vec<usize> = panes.iter().map(|p| p.window).collect();
        windows.sort_unstable();
        windows.dedup();
        let mut out = String::new();
        for index in windows {
            let members: Vec<&PaneView> = panes.iter().filter(|p| p.window == index).collect();
            let Some(&focus) = members.iter().find(|p| p.active).or(members.first()) else {
                continue;
            };
            let mut hit = (names && pattern.is_match(&focus.window_name))
                || (titles && members.iter().any(|p| pattern.is_match(&p.title)));
            if !hit && content {
                for pane in &members {
                    let screen = self.client.screen(&format!("%{}", pane.id)).await?;
                    if pattern.is_match(&screen) {
                        hit = true;
                        break;
                    }
                }
            }
            if hit {
                out.push_str(&format!(
                    "{index}: {} [{w}x{h}] ({} panes) \"{}\"\n",
                    focus.window_name,
                    members.len(),
                    focus.title
                ));
            }
        }
        if out.is_empty() {
            Err(format!("no windows matching: {needle}"))
        } else {
            Ok(out)
        }
    }

    async fn break_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        let node = self.resolve(inv.value('s').or(inv.value('t'))).await?;
        self.client
            .break_pane(&node.name)
            .await
            .map(|_| String::new())
    }

    async fn rename_window(&mut self, inv: &Invocation) -> Result<String, String> {
        let name = inv.operand_text();
        let index = match inv.value('t') {
            Some(t) => Some(self.window_of(t).await?),
            None => None,
        };
        self.client
            .rename_window(index, &name)
            .await
            .map(|_| String::new())
    }

    async fn rename_session(&mut self, inv: &Invocation) -> Result<String, String> {
        let name = inv.operand_text();
        self.client
            .rename_session(&name)
            .await
            .map(|_| String::new())
    }
}

/// View of one pane from its node position, the current layout rectangles and the window tabs.
pub fn pane_view(
    session: &str,
    index: usize,
    node: &NodeInfo,
    rects: &[(String, Rect, bool)],
    tabs: &[WindowTab],
) -> PaneView {
        let window = node.window.unwrap_or(0);
        let (width, height, active, rect) = rects
            .iter()
            .find(|(n, _, _)| n == &node.name)
            .map(|(_, r, f)| (r.width, r.height, *f, Some(*r)))
            .unwrap_or((0, 0, false, None));
        let marked = node.marked.unwrap_or(false);
        let marked_set = tabs.iter().any(|t| t.has_marked) || marked;
        let tab = tabs.iter().find(|t| t.index == window);
        PaneView {
            session: session.to_string(),
            id: node.id,
            index,
            pid: node.pid,
            title: node.title.clone().unwrap_or_default(),
            command: node
                .foreground
                .clone()
                .unwrap_or_else(|| node.command.clone()),
            path: node.cwd.clone().unwrap_or_default(),
            width,
            height,
            active,
            dead: !node.is_running(),
            marked,
            marked_set,
            window,
            window_name: tab.map(|t| t.name.clone()).unwrap_or_default(),
            start_command: node.command.clone(),
            tty: node
                .tty
                .as_ref()
                .map(|t| format!("/dev/{t}"))
                .unwrap_or_default(),
            rect,
            window_active: tab.is_some_and(|t| t.is_active),
            window_last: tab.is_some_and(|t| t.is_last),
            window_activity: tab.is_some_and(|t| t.has_activity),
            window_bell: tab.is_some_and(|t| t.has_bell),
            window_marked: tab.is_some_and(|t| t.has_marked),
            window_synchronized: tab.is_some_and(|t| t.synchronized),
            session_windows: tabs.len(),
            ..Default::default()
        }
}
