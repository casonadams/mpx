use futures_util::SinkExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{error, info};

use crate::ipc::{AsyncStream, IpcListener};
use crate::node::pty::PtyNode;
use crate::node::task::TaskNode;
use crate::node::Node;
use crate::protocol::{DaemonMessage, NewWindowOpts, NodeInfo, Request, Response, SplitOpts, StreamEvent};
use crate::layout::{Direction, Divider, Rect, WindowTab, Workspace};
use crate::pipe::VirtualPipe;

pub struct DaemonState {
    nodes: RwLock<HashMap<String, Arc<dyn Node>>>,
    event_tx: broadcast::Sender<StreamEvent>,
    workspace: RwLock<Workspace>,
    pipes: RwLock<HashMap<String, VirtualPipe>>,
    exited_history: RwLock<HashMap<String, (String, Option<i32>)>>,
}

impl DaemonState {
    pub fn new() -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(1024);
        let state = Arc::new(Self {
            nodes: RwLock::new(HashMap::new()),
            event_tx,
            workspace: RwLock::new(Workspace::new("main".to_string())),
            pipes: RwLock::new(HashMap::new()),
            exited_history: RwLock::new(HashMap::new()),
        });

        // Automatically purge and collapse panes from workspace and active nodes when process exits
        let mut rx = state.event_tx.subscribe();
        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if let StreamEvent::Exit { name, code } = event {
                    let logs = {
                        let nodes = state_clone.nodes.read().await;
                        nodes.get(&name).map(|n| n.read_buffer(None)).unwrap_or_default()
                    };
                    {
                        let mut hist = state_clone.exited_history.write().await;
                        hist.insert(name.clone(), (logs, code));
                    }
                    {
                        let mut ws = state_clone.workspace.write().await;
                        ws.remove(&name);
                    }
                    {
                        let mut nodes = state_clone.nodes.write().await;
                        nodes.remove(&name);
                    }
                }
            }
        });

        state
    }

    pub async fn spawn_node(
        &self,
        name: String,
        cmd: String,
        args: Vec<String>,
        pty: bool,
        cwd: Option<String>,
    ) -> Result<NodeInfo, String> {
        let mut nodes = self.nodes.write().await;
        if nodes.contains_key(&name) {
            return Err(format!("Node '{}' already exists", name));
        }

        let node: Arc<dyn Node> = if pty {
            let pty_node = PtyNode::spawn(
                name.clone(),
                cmd,
                &args,
                cwd.as_deref(),
                self.event_tx.clone(),
            )
            .map_err(|e| e.to_string())?;
            Arc::new(pty_node)
        } else {
            let task_node = TaskNode::spawn(
                name.clone(),
                cmd,
                &args,
                cwd.as_deref(),
                self.event_tx.clone(),
            )
            .map_err(|e| e.to_string())?;
            Arc::new(task_node)
        };

        let info = node.info();
        let _ = self.event_tx.send(StreamEvent::Spawned {
            name: name.clone(),
            pid: info.pid,
        });

        nodes.insert(name.clone(), node);
        {
            let mut ws = self.workspace.write().await;
            ws.add_or_split(name.clone(), Direction::Horizontal);
        }
        Ok(info)
    }

    pub async fn list_nodes(&self) -> Vec<NodeInfo> {
        let nodes = self.nodes.read().await;
        let mut list: Vec<NodeInfo> = nodes.values().map(|n| n.info()).collect();
        let hist = self.exited_history.read().await;
        for (name, (_, code)) in hist.iter() {
            if !nodes.contains_key(name) {
                list.push(NodeInfo {
                    name: name.clone(),
                    pid: None,
                    pty: false,
                    command: String::new(),
                    status: match code {
                        Some(c) => format!("exited ({})", c),
                        None => "exited".to_string(),
                    },
                });
            }
        }
        list
    }

    pub async fn get_status(&self, name: &str) -> Result<NodeInfo, String> {
        let nodes = self.nodes.read().await;
        if let Some(node) = nodes.get(name) {
            return Ok(node.info());
        }
        drop(nodes);
        let hist = self.exited_history.read().await;
        if let Some((_, code)) = hist.get(name) {
            return Ok(NodeInfo {
                name: name.to_string(),
                pid: None,
                pty: false,
                command: String::new(),
                status: match code {
                    Some(c) => format!("exited ({})", c),
                    None => "exited".to_string(),
                },
            });
        }
        Err(format!("Node '{}' not found", name))
    }

    pub async fn get_logs(&self, name: &str, tail: Option<usize>) -> Result<String, String> {
        let nodes = self.nodes.read().await;
        if let Some(node) = nodes.get(name) {
            return Ok(node.read_buffer(tail));
        }
        drop(nodes);
        let hist = self.exited_history.read().await;
        if let Some((logs, _)) = hist.get(name) {
            return Ok(logs.clone());
        }
        Err(format!("Node '{}' not found", name))
    }

    pub async fn send_input(&self, name: &str, data: &str) -> Result<(), String> {
        let nodes = self.nodes.read().await;
        let node = nodes
            .get(name)
            .ok_or_else(|| format!("Node '{}' not found", name))?;
        node.send_input(data.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn kill_node(&self, name: &str) -> Result<(), String> {
        let node = {
            let mut nodes = self.nodes.write().await;
            nodes.remove(name).ok_or_else(|| format!("Node '{}' not found", name))?
        };
        let _ = node.kill();
        let mut ws = self.workspace.write().await;
        ws.remove(name);
        Ok(())
    }

    pub async fn wait_node(&self, name: &str) -> Result<Option<i32>, String> {
        let node_opt = {
            let nodes = self.nodes.read().await;
            nodes.get(name).cloned()
        };
        if let Some(node) = node_opt {
            return Ok(node.wait_exit().await);
        }
        let hist = self.exited_history.read().await;
        if let Some((_, code)) = hist.get(name) {
            return Ok(*code);
        }
        Err(format!("Node '{}' not found", name))
    }

    pub async fn get_screen(&self, name: &str) -> Result<String, String> {
        let nodes = self.nodes.read().await;
        if let Some(node) = nodes.get(name) {
            return Ok(node.read_screen());
        }
        drop(nodes);
        let hist = self.exited_history.read().await;
        if let Some((logs, _)) = hist.get(name) {
            return Ok(logs.clone());
        }
        Err(format!("Node '{}' not found", name))
    }


    pub async fn split_node(&self, opts: SplitOpts) -> Result<NodeInfo, String> {
        let mut nodes = self.nodes.write().await;
        if nodes.contains_key(&opts.name) {
            return Err(format!("Node '{}' already exists", opts.name));
        }

        let active_cwd = opts
            .cwd
            .or_else(|| opts.target.as_ref().and_then(|t| nodes.get(t).and_then(|n| n.cwd())))
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()));

        let node: Arc<dyn Node> = if opts.pty {
            let pty_node = PtyNode::spawn(
                opts.name.clone(),
                opts.command,
                &opts.args,
                active_cwd.as_deref(),
                self.event_tx.clone(),
            )
            .map_err(|e| e.to_string())?;
            Arc::new(pty_node)
        } else {
            let task_node = TaskNode::spawn(
                opts.name.clone(),
                opts.command,
                &opts.args,
                active_cwd.as_deref(),
                self.event_tx.clone(),
            )
            .map_err(|e| e.to_string())?;
            Arc::new(task_node)
        };

        let info = node.info();
        let _ = self.event_tx.send(StreamEvent::Spawned {
            name: opts.name.clone(),
            pid: info.pid,
        });

        nodes.insert(opts.name.clone(), node);

        {
            let mut ws = self.workspace.write().await;
            if let Some(ref t) = opts.target {
                ws.focus(t);
            }
            ws.add_or_split_with_focus(opts.name, opts.direction, opts.focus);
        }

        Ok(info)
    }

    pub async fn new_window(&self, opts: NewWindowOpts) -> Result<NodeInfo, String> {
        let win_label = opts.name.unwrap_or_else(|| {
            std::path::Path::new(&opts.command)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "shell".into())
        });

        let node_name = {
            let ws = self.workspace.read().await;
            format!("{}-w{}", win_label, ws.next_window_id)
        };

        let active_cwd = opts
            .cwd
            .or_else(|| {
                let ws = self.workspace.try_read().ok()?;
                let focused_node = ws.active_window().and_then(|w| w.focused.clone())?;
                let nodes = self.nodes.try_read().ok()?;
                nodes.get(&focused_node).and_then(|n| n.cwd())
            })
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()));

        let node: Arc<dyn Node> = if opts.pty {
            let pty_node = PtyNode::spawn(
                node_name.clone(),
                opts.command,
                &opts.args,
                active_cwd.as_deref(),
                self.event_tx.clone(),
            )
            .map_err(|e| e.to_string())?;
            Arc::new(pty_node)
        } else {
            let task_node = TaskNode::spawn(
                node_name.clone(),
                opts.command,
                &opts.args,
                active_cwd.as_deref(),
                self.event_tx.clone(),
            )
            .map_err(|e| e.to_string())?;
            Arc::new(task_node)
        };

        let info = node.info();
        let _ = self.event_tx.send(StreamEvent::Spawned {
            name: node_name.clone(),
            pid: info.pid,
        });

        {
            let mut nodes = self.nodes.write().await;
            nodes.insert(node_name.clone(), node);
        }

        {
            let mut ws = self.workspace.write().await;
            ws.new_window(win_label, node_name);
        }

        Ok(info)
    }

    pub async fn next_window(&self) {
        let mut ws = self.workspace.write().await;
        ws.next_window();
    }

    pub async fn prev_window(&self) {
        let mut ws = self.workspace.write().await;
        ws.prev_window();
    }

    pub async fn select_window(&self, idx: usize) -> bool {
        let mut ws = self.workspace.write().await;
        ws.select_window(idx)
    }

    pub async fn compute_layout(
        &self,
        width: u16,
        height: u16,
    ) -> (Vec<(String, Rect, bool)>, Vec<Divider>, Vec<WindowTab>) {
        let ws = self.workspace.read().await;
        let (panes, dividers) = ws.compute_layout_with_dividers(width, height);
        let tabs = ws.window_tabs();
        (panes, dividers, tabs)
    }

    pub async fn get_cwd(&self, name: &str) -> Option<String> {
        let nodes = self.nodes.read().await;
        nodes.get(name).and_then(|n| n.cwd())
    }

    pub async fn create_pipe(&self, name: String, capacity: Option<usize>) -> Result<(), String> {
        let mut pipes = self.pipes.write().await;
        if pipes.contains_key(&name) {
            return Err(format!("Pipe '{}' already exists", name));
        }
        let cap = capacity.unwrap_or(1024 * 1024);
        pipes.insert(name.clone(), VirtualPipe::new(name, cap));
        Ok(())
    }

    pub async fn write_pipe(&self, name: &str, data: &str) -> Result<(), String> {
        let pipes = self.pipes.read().await;
        let pipe = pipes.get(name).ok_or_else(|| format!("Pipe '{}' not found", name))?;
        pipe.write(data.as_bytes()).await;
        Ok(())
    }

    pub async fn read_pipe(&self, name: &str) -> Result<String, String> {
        let pipes = self.pipes.read().await;
        let pipe = pipes.get(name).ok_or_else(|| format!("Pipe '{}' not found", name))?;
        Ok(pipe.read_str().await)
    }

    pub async fn list_pipes(&self) -> Vec<String> {
        let pipes = self.pipes.read().await;
        pipes.keys().cloned().collect()
    }

    pub async fn resize_node(&self, name: &str, cols: u16, rows: u16) -> Result<(), String> {
        let nodes = self.nodes.read().await;
        let node = nodes.get(name).ok_or_else(|| format!("Node '{}' not found", name))?;
        node.resize(cols, rows).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.event_tx.subscribe()
    }
}

pub async fn handle_client<S: AsyncStream>(state: Arc<DaemonState>, stream: S) {
    let mut framed = Framed::new(stream, LinesCodec::new());
    use futures_util::StreamExt;

    while let Some(line_res) = framed.next().await {
        let line = match line_res {
            Ok(l) => l,
            Err(e) => {
                error!("Error reading line from client: {}", e);
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = Response::Error(format!("Invalid request JSON: {}", e));
                let resp_str = serde_json::to_string(&DaemonMessage::Response(err_resp)).unwrap();
                let _ = framed.send(resp_str).await;
                continue;
            }
        };

        match req {
            Request::Subscribe { name: filter_name } => {
                let mut rx = state.subscribe();
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(Response::Ok)).unwrap())
                    .await;

                while let Ok(event) = rx.recv().await {
                    let should_send = match (&filter_name, &event) {
                        (Some(filter), StreamEvent::Output { name, .. }) => filter == name,
                        (Some(filter), StreamEvent::Exit { name, .. }) => filter == name,
                        (Some(filter), StreamEvent::Spawned { name, .. }) => filter == name,
                        (None, _) => true,
                    };

                    if should_send {
                        let msg = DaemonMessage::Event(event);
                        let json = match serde_json::to_string(&msg) {
                            Ok(j) => j,
                            Err(_) => continue,
                        };
                        if framed.send(json).await.is_err() {
                            break;
                        }
                    }
                }
                break;
            }
            other => {
                let resp = dispatch_request(&state, other).await;
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
        }
    }
}

async fn dispatch_request(state: &Arc<DaemonState>, req: Request) -> Response {
    match req {
        Request::Ping => Response::Pong,
        Request::Spawn {
            name,
            command,
            args,
            pty,
            cwd,
        } => match state.spawn_node(name, command, args, pty, cwd).await {
            Ok(info) => Response::Node(info),
            Err(e) => Response::Error(e),
        },
        Request::List => Response::NodeList(state.list_nodes().await),
        Request::Logs { name, tail } => match state.get_logs(&name, tail).await {
            Ok(logs) => Response::Logs(logs),
            Err(e) => Response::Error(e),
        },
        Request::Send { name, data } => match state.send_input(&name, &data).await {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error(e),
        },
        Request::Kill { name } => match state.kill_node(&name).await {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error(e),
        },
        Request::Wait { name } => match state.wait_node(&name).await {
            Ok(code) => Response::Exit { code },
            Err(e) => Response::Error(e),
        },
        Request::Screen { name } => match state.get_screen(&name).await {
            Ok(screen) => Response::Screen(screen),
            Err(e) => Response::Error(e),
        },
        Request::Split(opts) => match state.split_node(opts).await {
            Ok(info) => Response::Node(info),
            Err(e) => Response::Error(e),
        },
        Request::Layout { width, height } => {
            let (rects, dividers, tabs) = state.compute_layout(width, height).await;
            Response::Layout(rects, dividers, tabs)
        }
        Request::FocusNext => {
            state.workspace.write().await.focus_next();
            Response::Ok
        }
        Request::FocusPrev => {
            state.workspace.write().await.focus_prev();
            Response::Ok
        }
        Request::Focus { name } => {
            state.workspace.write().await.focus(&name);
            Response::Ok
        }
        Request::ResizePane { name, cols, rows } => match state.resize_node(&name, cols, rows).await {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error(e),
        },
        Request::NewWindow(opts) => match state.new_window(opts).await {
            Ok(info) => Response::Node(info),
            Err(e) => Response::Error(e),
        },
        Request::GetCwd { name } => Response::Cwd(state.get_cwd(&name).await),
        Request::NextWindow => {
            state.next_window().await;
            Response::Ok
        }
        Request::PrevWindow => {
            state.prev_window().await;
            Response::Ok
        }
        Request::SelectWindow { index } => {
            if state.select_window(index).await {
                Response::Ok
            } else {
                Response::Error("Window index out of bounds".into())
            }
        }
        Request::CreatePipe { name, capacity } => match state.create_pipe(name, capacity).await {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error(e),
        },
        Request::WritePipe { name, data } => match state.write_pipe(&name, &data).await {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error(e),
        },
        Request::ReadPipe { name } => match state.read_pipe(&name).await {
            Ok(text) => Response::PipeData(text),
            Err(e) => Response::Error(e),
        },
        Request::ListPipes => Response::PipeList(state.list_pipes().await),
        Request::Status { name } => match state.get_status(&name).await {
            Ok(info) => Response::Node(info),
            Err(e) => Response::Error(e),
        },
        Request::Subscribe { .. } => unreachable!(),
    }
}

pub async fn run_daemon(endpoint: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    let listener = IpcListener::bind(endpoint)?;
    #[cfg(windows)]
    let mut listener = IpcListener::bind(endpoint)?;

    info!("mpx daemon listening on {}", endpoint);
    let state = DaemonState::new();

    loop {
        match listener.accept().await {
            Ok(stream) => {
                let state_clone = Arc::clone(&state);
                tokio::spawn(async move {
                    handle_client(state_clone, stream).await;
                });
            }
            Err(e) => {
                error!("Accept error: {}", e);
            }
        }
    }
}
