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
use crate::protocol::{DaemonMessage, NodeInfo, Request, Response, StreamEvent};
use crate::layout::{Direction, Rect, Workspace};
use crate::pipe::VirtualPipe;

pub struct DaemonState {
    nodes: RwLock<HashMap<String, Arc<dyn Node>>>,
    event_tx: broadcast::Sender<StreamEvent>,
    workspace: RwLock<Workspace>,
    pipes: RwLock<HashMap<String, VirtualPipe>>,
}

impl DaemonState {
    pub fn new() -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(1024);
        let state = Arc::new(Self {
            nodes: RwLock::new(HashMap::new()),
            event_tx,
            workspace: RwLock::new(Workspace::new("main".to_string())),
            pipes: RwLock::new(HashMap::new()),
        });

        // Automatically purge and collapse panes when their process exits
        let mut rx = state.event_tx.subscribe();
        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if let StreamEvent::Exit { name, .. } = event {
                    let mut ws = state_clone.workspace.write().await;
                    ws.remove(&name);
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
        nodes.values().map(|n| n.info()).collect()
    }

    pub async fn get_logs(&self, name: &str, tail: Option<usize>) -> Result<String, String> {
        let nodes = self.nodes.read().await;
        let node = nodes
            .get(name)
            .ok_or_else(|| format!("Node '{}' not found", name))?;
        Ok(node.read_buffer(tail))
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
        let node = {
            let nodes = self.nodes.read().await;
            nodes
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Node '{}' not found", name))?
        };
        Ok(node.wait_exit().await)
    }

    pub async fn get_screen(&self, name: &str) -> Result<String, String> {
        let nodes = self.nodes.read().await;
        let node = nodes
            .get(name)
            .ok_or_else(|| format!("Node '{}' not found", name))?;
        Ok(node.read_screen())
    }

    pub async fn split_node(
        &self,
        target: Option<String>,
        name: String,
        cmd: String,
        args: Vec<String>,
        pty: bool,
        dir: Direction,
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
                None,
                self.event_tx.clone(),
            )
            .map_err(|e| e.to_string())?;
            Arc::new(pty_node)
        } else {
            let task_node = TaskNode::spawn(
                name.clone(),
                cmd,
                &args,
                None,
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
            if let Some(t) = target {
                ws.focus(&t);
            }
            ws.add_or_split(name, dir);
        }

        Ok(info)
    }

    pub async fn compute_layout(&self, width: u16, height: u16) -> Vec<(String, Rect, bool)> {
        let ws = self.workspace.read().await;
        ws.compute_layout(width, height)
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
            Request::Ping => {
                let msg = DaemonMessage::Response(Response::Pong);
                let _ = framed.send(serde_json::to_string(&msg).unwrap()).await;
            }
            Request::Spawn {
                name,
                command,
                args,
                pty,
                cwd,
            } => {
                let res = state.spawn_node(name, command, args, pty, cwd).await;
                let resp = match res {
                    Ok(info) => Response::Node(info),
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::List => {
                let list = state.list_nodes().await;
                let msg = DaemonMessage::Response(Response::NodeList(list));
                let _ = framed.send(serde_json::to_string(&msg).unwrap()).await;
            }
            Request::Logs { name, tail } => {
                let res = state.get_logs(&name, tail).await;
                let resp = match res {
                    Ok(logs) => Response::Logs(logs),
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::Send { name, data } => {
                let res = state.send_input(&name, &data).await;
                let resp = match res {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::Kill { name } => {
                let res = state.kill_node(&name).await;
                let resp = match res {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::Wait { name } => {
                let res = state.wait_node(&name).await;
                let resp = match res {
                    Ok(code) => Response::Exit { code },
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::Screen { name } => {
                let res = state.get_screen(&name).await;
                let resp = match res {
                    Ok(text) => Response::Screen(text),
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::Split {
                target,
                name,
                command,
                args,
                pty,
                direction,
            } => {
                let res = state
                    .split_node(target, name, command, args, pty, direction)
                    .await;
                let resp = match res {
                    Ok(info) => Response::Node(info),
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::Layout { width, height } => {
                let rects = state.compute_layout(width, height).await;
                let resp = Response::Layout(rects);
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::FocusNext => {
                state.workspace.write().await.focus_next();
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(Response::Ok)).unwrap())
                    .await;
            }
            Request::FocusPrev => {
                state.workspace.write().await.focus_prev();
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(Response::Ok)).unwrap())
                    .await;
            }
            Request::Focus { name } => {
                state.workspace.write().await.focus(&name);
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(Response::Ok)).unwrap())
                    .await;
            }
            Request::CreatePipe { name, capacity } => {
                let res = state.create_pipe(name, capacity).await;
                let resp = match res {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::WritePipe { name, data } => {
                let res = state.write_pipe(&name, &data).await;
                let resp = match res {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::ReadPipe { name } => {
                let res = state.read_pipe(&name).await;
                let resp = match res {
                    Ok(text) => Response::PipeData(text),
                    Err(e) => Response::Error(e),
                };
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::ListPipes => {
                let pipes = state.list_pipes().await;
                let resp = Response::PipeList(pipes);
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(resp)).unwrap())
                    .await;
            }
            Request::Subscribe { name: filter_name } => {
                let mut rx = state.subscribe();
                let _ = framed
                    .send(serde_json::to_string(&DaemonMessage::Response(Response::Ok)).unwrap())
                    .await;

                // Stream events until client disconnects
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
        }
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
