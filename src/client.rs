use futures_util::{SinkExt, StreamExt};
use tokio_util::codec::{Framed, LinesCodec};

use crate::ipc::{connect, IpcStream};
use crate::protocol::{DaemonMessage, NodeInfo, Request, Response, StreamEvent};
use crate::layout::{Direction, Rect};

pub struct Client {
    framed: Framed<IpcStream, LinesCodec>,
}

impl Client {
    pub async fn connect(endpoint: &str) -> Result<Self, String> {
        let stream = connect(endpoint)
            .await
            .map_err(|e| format!("Failed to connect to daemon at '{}': {}", endpoint, e))?;
        let framed = Framed::new(stream, LinesCodec::new());
        Ok(Self { framed })
    }

    async fn call(&mut self, req: Request) -> Result<Response, String> {
        let req_str = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        self.framed.send(req_str).await.map_err(|e| e.to_string())?;

        let line = self
            .framed
            .next()
            .await
            .ok_or_else(|| "Connection closed by daemon".to_string())?
            .map_err(|e| e.to_string())?;

        let msg: DaemonMessage =
            serde_json::from_str(&line).map_err(|e| format!("Failed to parse response: {}", e))?;

        match msg {
            DaemonMessage::Response(resp) => Ok(resp),
            DaemonMessage::Event(_) => Err("Unexpected stream event in request-response".to_string()),
        }
    }

    pub async fn ping(&mut self) -> Result<(), String> {
        match self.call(Request::Ping).await? {
            Response::Pong => Ok(()),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Ping".into()),
        }
    }

    pub async fn spawn(
        &mut self,
        name: String,
        command: String,
        args: Vec<String>,
        pty: bool,
        cwd: Option<String>,
    ) -> Result<NodeInfo, String> {
        match self
            .call(Request::Spawn {
                name,
                command,
                args,
                pty,
                cwd,
            })
            .await?
        {
            Response::Node(info) => Ok(info),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Spawn".into()),
        }
    }

    pub async fn list(&mut self) -> Result<Vec<NodeInfo>, String> {
        match self.call(Request::List).await? {
            Response::NodeList(list) => Ok(list),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to List".into()),
        }
    }

    pub async fn logs(&mut self, name: &str, tail: Option<usize>) -> Result<String, String> {
        match self
            .call(Request::Logs {
                name: name.to_string(),
                tail,
            })
            .await?
        {
            Response::Logs(text) => Ok(text),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Logs".into()),
        }
    }

    pub async fn send_input(&mut self, name: &str, data: &str) -> Result<(), String> {
        match self
            .call(Request::Send {
                name: name.to_string(),
                data: data.to_string(),
            })
            .await?
        {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Send".into()),
        }
    }

    pub async fn kill(&mut self, name: &str) -> Result<(), String> {
        match self
            .call(Request::Kill {
                name: name.to_string(),
            })
            .await?
        {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Kill".into()),
        }
    }

    pub async fn wait(&mut self, name: &str) -> Result<Option<i32>, String> {
        match self
            .call(Request::Wait {
                name: name.to_string(),
            })
            .await?
        {
            Response::Exit { code } => Ok(code),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Wait".into()),
        }
    }
    pub async fn screen(&mut self, name: &str) -> Result<String, String> {
        match self
            .call(Request::Screen {
                name: name.to_string(),
            })
            .await?
        {
            Response::Screen(text) => Ok(text),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Screen".into()),
        }
    }

    pub async fn split(
        &mut self,
        target: Option<String>,
        name: String,
        command: String,
        args: Vec<String>,
        pty: bool,
        direction: Direction,
    ) -> Result<NodeInfo, String> {
        match self
            .call(Request::Split {
                target,
                name,
                command,
                args,
                pty,
                direction,
            })
            .await?
        {
            Response::Node(info) => Ok(info),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Split".into()),
        }
    }

    pub async fn layout(&mut self, width: u16, height: u16) -> Result<Vec<(String, Rect, bool)>, String> {
        match self.call(Request::Layout { width, height }).await? {
            Response::Layout(rects) => Ok(rects),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Layout".into()),
        }
    }
    pub async fn focus_next(&mut self) -> Result<(), String> {
        match self.call(Request::FocusNext).await? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to FocusNext".into()),
        }
    }

    pub async fn focus_prev(&mut self) -> Result<(), String> {
        match self.call(Request::FocusPrev).await? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to FocusPrev".into()),
        }
    }

    pub async fn focus(&mut self, name: &str) -> Result<(), String> {
        match self.call(Request::Focus { name: name.to_string() }).await? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to Focus".into()),
        }
    }
    pub async fn create_pipe(&mut self, name: &str, capacity: Option<usize>) -> Result<(), String> {
        match self.call(Request::CreatePipe { name: name.to_string(), capacity }).await? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to CreatePipe".into()),
        }
    }

    pub async fn write_pipe(&mut self, name: &str, data: &str) -> Result<(), String> {
        match self.call(Request::WritePipe { name: name.to_string(), data: data.to_string() }).await? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to WritePipe".into()),
        }
    }

    pub async fn read_pipe(&mut self, name: &str) -> Result<String, String> {
        match self.call(Request::ReadPipe { name: name.to_string() }).await? {
            Response::PipeData(text) => Ok(text),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to ReadPipe".into()),
        }
    }

    pub async fn list_pipes(&mut self) -> Result<Vec<String>, String> {
        match self.call(Request::ListPipes).await? {
            Response::PipeList(pipes) => Ok(pipes),
            Response::Error(e) => Err(e),
            _ => Err("Invalid response to ListPipes".into()),
        }
    }




    pub async fn stream_logs<F>(&mut self, name: Option<String>, mut handler: F) -> Result<(), String>
    where
        F: FnMut(StreamEvent) -> bool, // returns false to stop
    {
        let req = Request::Subscribe { name };
        let req_str = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        self.framed.send(req_str).await.map_err(|e| e.to_string())?;

        // First message is Response::Ok
        let init_line = self
            .framed
            .next()
            .await
            .ok_or_else(|| "Connection closed".to_string())?
            .map_err(|e| e.to_string())?;

        let _init_msg: DaemonMessage =
            serde_json::from_str(&init_line).map_err(|e| format!("Invalid init response: {}", e))?;

        while let Some(line_res) = self.framed.next().await {
            let line = line_res.map_err(|e| e.to_string())?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(DaemonMessage::Event(event)) = serde_json::from_str(&line) {
                if !handler(event) {
                    break;
                }
            }
        }

        Ok(())
    }
}
