use uuid::Uuid;

use super::args::{Invocation, is_noop, parse};
use super::format::{PaneView, Vars, expand, pane_vars, session_vars};
use crate::client::Client;
use crate::ipc;
use crate::layout::Rect;
use crate::protocol::{LayoutPreset, NewWindowOpts, NodeInfo, Side, SpawnOpts, SplitOpts};

const VERSION: &str = "tmux 3.4\n";
const DEFAULT_VIEW: (u16, u16) = (80, 24);

/// Connection target derived from the environment of the process (inside an mpx pane or outside).
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
            let session = std::env::var("MPX_SESSION").unwrap_or_else(|_| "main".to_string());
            ipc::default_endpoint(&session)
        });
        Self {
            session: super::session_of(&endpoint),
            endpoint,
        }
    }
}

/// Multicall entry point. Returns the process exit code and prints tmux-style output.
pub async fn run(argv: Vec<String>) -> i32 {
    match execute(&argv).await {
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

async fn execute(argv: &[String]) -> Result<String, String> {
    let inv = parse(argv)?;
    let cmd = inv.command.as_str();
    if cmd == "-V" {
        return Ok(VERSION.to_string());
    }
    if is_noop(cmd) {
        return Ok(String::new());
    }
    let env = EnvTarget::from_env();
    match cmd {
        "list-sessions" => Ok(list_sessions(&inv)),
        "has-session" => has_session(&inv, &env).await,
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
}

fn list_sessions(inv: &Invocation) -> String {
    let fmt = inv.value('F').unwrap_or("#{session_name}");
    ipc::list_sessions()
        .iter()
        .map(|s| format!("{}\n", expand(fmt, &session_vars(&s.name, 0, 0))))
        .collect()
}

async fn has_session(inv: &Invocation, env: &EnvTarget) -> Result<String, String> {
    let name = inv
        .value('t')
        .map(|t| t.rsplit(':').next().unwrap_or(t))
        .unwrap_or(&env.session);
    let endpoint = if name == env.session {
        env.endpoint.clone()
    } else {
        ipc::default_endpoint(name)
    };
    let mut client = Client::connect(&endpoint).await?;
    client.ping().await?;
    Ok(String::new())
}

/// Target key for a tmux target: strips `session:` and `window.` prefixes, keeping `%id` or a node name.
pub fn pane_key(target: &str) -> &str {
    let last = target.rsplit(':').next().unwrap_or(target);
    match last.rsplit_once('.') {
        Some((_, pane)) if pane.starts_with('%') => pane,
        _ => last,
    }
}

/// Window index for a `session:window` or `@N` target.
pub fn window_key(target: &str) -> Option<usize> {
    let last = target.rsplit(':').next().unwrap_or(target);
    last.strip_prefix('@').unwrap_or(last).parse().ok()
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

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
}

fn fresh_name() -> String {
    let id = Uuid::new_v4().simple().to_string();
    format!("pane-{}", &id[..8])
}

/// Spawn options for a tmux pane: trailing operands run through `sh -c`, as tmux does.
pub fn spawn_opts(inv: &Invocation) -> SpawnOpts {
    let (command, args) = if inv.operands.is_empty() {
        (default_shell(), Vec::new())
    } else {
        ("sh".to_string(), vec!["-c".to_string(), inv.operand_text()])
    };
    SpawnOpts {
        name: fresh_name(),
        command,
        args,
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

struct Shim {
    client: Client,
    session: String,
}

impl Shim {
    async fn layout(&mut self) -> Result<Vec<(String, Rect, bool)>, String> {
        let (w, h) = terminal_view();
        Ok(self.client.layout(w, h).await?.0)
    }

    async fn panes(&mut self) -> Result<Vec<PaneView>, String> {
        let nodes = self.client.list().await?;
        let rects = self.layout().await?;
        Ok(nodes
            .iter()
            .enumerate()
            .map(|(i, n)| pane_view(&self.session, i, n, &rects))
            .collect())
    }

    async fn resolve(&mut self, target: Option<&str>) -> Result<NodeInfo, String> {
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

    async fn pane_view_of(&mut self, node: &NodeInfo) -> Result<PaneView, String> {
        let panes = self.panes().await?;
        Ok(panes
            .into_iter()
            .find(|p| p.id == node.id)
            .unwrap_or_else(|| pane_view(&self.session, 0, node, &[])))
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
            "last-pane" => self.client.focus_last().await.map(|_| String::new()),
            "last-window" => self.client.last_window().await.map(|_| String::new()),
            "rename-window" => self.rename_window(inv).await,
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
        let direction = if inv.has('v') {
            crate::layout::Direction::Vertical
        } else {
            crate::layout::Direction::Horizontal
        };
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
                let count = panes.iter().filter(|p| p.window == w).count();
                let first = panes
                    .iter()
                    .find(|p| p.window == w)
                    .cloned()
                    .unwrap_or_default();
                let mut vars: Vars = pane_vars(&first);
                vars.insert("window_panes", count.to_string());
                format!("{}\n", expand(fmt, &vars))
            })
            .collect())
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
        let index = inv.value('t').and_then(window_key);
        self.client.kill_window(index).await.map(|_| String::new())
    }

    async fn select_pane(&mut self, inv: &Invocation) -> Result<String, String> {
        if let Some(side) = side_of(inv) {
            let node = self.resolve(inv.value('t')).await?;
            let rects = self.layout().await?;
            let target = neighbor(&rects, &node.name, side)
                .ok_or_else(|| "no pane in that direction".to_string())?;
            return self.client.focus(&target).await.map(|_| String::new());
        }
        if inv.value('T').is_some() {
            return Ok(String::new());
        }
        let node = self.resolve(inv.value('t')).await?;
        self.client.focus(&node.name).await.map(|_| String::new())
    }

    async fn select_window(&mut self, inv: &Invocation) -> Result<String, String> {
        let index = inv
            .value('t')
            .and_then(window_key)
            .ok_or_else(|| "window index required".to_string())?;
        self.client
            .select_window(index)
            .await
            .map(|_| String::new())
    }

    async fn select_layout(&mut self, inv: &Invocation) -> Result<String, String> {
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
        let src = self.resolve(inv.value('s')).await?;
        let dst = self.resolve(inv.value('t')).await?;
        self.client
            .swap_pane(&src.name, &dst.name)
            .await
            .map(|_| String::new())
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
        let index = inv.value('t').and_then(window_key);
        self.client
            .rename_window(index, &name)
            .await
            .map(|_| String::new())
    }
}

/// View of one pane from its node position and the current layout rectangles.
pub fn pane_view(
    session: &str,
    index: usize,
    node: &NodeInfo,
    rects: &[(String, Rect, bool)],
) -> PaneView {
    let (width, height, active) = rects
        .iter()
        .find(|(n, _, _)| n == &node.name)
        .map(|(_, r, f)| (r.width, r.height, *f))
        .unwrap_or((0, 0, false));
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
        window: node.window.unwrap_or(0),
        window_name: String::new(),
    }
}
